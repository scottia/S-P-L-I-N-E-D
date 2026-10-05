use crate::candidate::{Candidate, StaticFormat};
use crate::config::{Mode, OutputConfig};
use crate::evaluate::is_acceptable;
use crate::inspect::inspect_image;
use crate::range::Range;
use crate::safe_write::replace_binary_file;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageFormat, ImageReader, RgbaImage};
use std::fs;
use std::io::Cursor;
use std::path::Path;

const JPEG_QUALITY: u8 = 95;
const SQUARE_EQUIVALENT_TOLERANCE: f64 = 0.005;
const MAX_AUTO_CROP_DEVIATION: f64 = 0.02;
const MAX_BRIGHTNESS_ADJUSTMENT: f32 = 0.05;
const MAX_CONTRAST_ADJUSTMENT: f32 = 0.08;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArtworkQualityAssessment {
    pub mean_luminance: f32,
    pub luminance_deviation: f32,
    pub shadow_clip_ratio: f32,
    pub highlight_clip_ratio: f32,
    pub brightness_adjustment: f32,
    pub contrast_adjustment: f32,
    pub automatic_eligible: bool,
}

impl ArtworkQualityAssessment {
    fn balanced() -> Self {
        Self {
            mean_luminance: 0.5,
            luminance_deviation: 0.2,
            shadow_clip_ratio: 0.0,
            highlight_clip_ratio: 0.0,
            brightness_adjustment: 0.0,
            contrast_adjustment: 0.0,
            automatic_eligible: true,
        }
    }

    pub fn brightness_percent(self) -> i8 {
        (self.brightness_adjustment * 100.0).round() as i8
    }

    pub fn contrast_percent(self) -> i8 {
        (self.contrast_adjustment * 100.0).round() as i8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreparedArtworkInfo {
    pub width: u32,
    pub height: u32,
    pub format: StaticFormat,
    pub resized: bool,
    pub converted: bool,
    pub upscale_backend: UpscaleBackend,
    pub brightness_percent: i8,
    pub contrast_percent: i8,
    pub exposure_percent: i8,
    pub sharpen_percent: u8,
    pub color_temperature: i16,
    pub adaptive_defaults: bool,
    pub quality_eligible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpscaleBackend {
    None,
    Cpu,
    Gpu,
}

impl UpscaleBackend {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Cpu => "cpu-lanczos3",
            Self::Gpu => "gpu-lanczos3",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedArtwork {
    pub bytes: Vec<u8>,
    pub info: PreparedArtworkInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalArtworkAction {
    NoSelection,
    ReadOnly,
    Unchanged,
    Installed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinalArtworkResult {
    pub action: FinalArtworkAction,
    pub info: Option<PreparedArtworkInfo>,
}

pub fn prepare_final_artwork(
    candidate: &Candidate,
    source_path: &Path,
    range: &Range,
    target_format: StaticFormat,
) -> Result<PreparedArtwork, String> {
    prepare_final_artwork_inner(candidate, source_path, range, target_format, false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectedArtwork {
    pub width: u32,
    pub height: u32,
    pub cropped: bool,
    pub resized: bool,
    pub upscaled: bool,
    pub acceptable: bool,
}

/// Prepare an explicit manual selection. The Python workflow permits numbered
/// fallback selections outside the normal automatic range while still applying
/// the configured output conversion and validation rules.
pub fn prepare_explicit_artwork(
    candidate: &Candidate,
    source_path: &Path,
    range: &Range,
    target_format: StaticFormat,
) -> Result<PreparedArtwork, String> {
    prepare_final_artwork_inner(candidate, source_path, range, target_format, true)
}

pub fn prepare_configured_artwork(
    candidate: &Candidate,
    source_path: &Path,
    range: &Range,
    target_format: StaticFormat,
    output: &OutputConfig,
    allow_outside_range: bool,
) -> Result<PreparedArtwork, String> {
    let (target_width, target_height, crop_square, resized) =
        projected_dimensions(candidate, range, output);
    let projected_short_side = target_width.min(target_height);
    let projected_acceptable =
        projected_short_side >= range.min && projected_short_side <= range.ladder;
    if !allow_outside_range && !projected_acceptable {
        return Err(format!(
            "Selected artwork candidate is outside the accepted SPLINED range after output policy: {}x{}.",
            target_width, target_height
        ));
    }

    let source = inspect_image(source_path)?;
    if source.width != candidate.width
        || source.height != candidate.height
        || source.format != candidate.format
    {
        return Err(format!(
            "Selected artwork changed after evaluation: expected {}x{} {:?}, found {}x{} {:?}.",
            candidate.width,
            candidate.height,
            candidate.format,
            source.width,
            source.height,
            source.format
        ));
    }

    let converted = candidate.format != target_format;
    let mut upscale_backend = UpscaleBackend::None;
    let mut quality = ArtworkQualityAssessment::balanced();
    let bytes = if !crop_square && !resized && !converted {
        fs::read(source_path).map_err(|error| {
            format!(
                "Unable to read selected artwork {}: {error}",
                source_path.display()
            )
        })?
    } else {
        let mut image = ImageReader::open(source_path)
            .map_err(|error| {
                format!(
                    "Unable to open selected artwork {}: {error}",
                    source_path.display()
                )
            })?
            .with_guessed_format()
            .map_err(|error| {
                format!(
                    "Unable to identify selected artwork {}: {error}",
                    source_path.display()
                )
            })?
            .decode()
            .map_err(|error| {
                format!(
                    "Unable to decode selected artwork {}: {error}",
                    source_path.display()
                )
            })?;

        if crop_square {
            let side = image.width().min(image.height());
            let left = (image.width() - side) / 2;
            let top = (image.height() - side) / 2;
            image = image.crop_imm(left, top, side, side);
        }
        let is_upscale = target_width > image.width() || target_height > image.height();
        if is_upscale {
            quality = assess_dynamic_image_quality(&image);
            if !output.upscale_adaptive_defaults {
                quality.brightness_adjustment = 0.0;
                quality.contrast_adjustment = 0.0;
            }
            quality.brightness_adjustment += output.upscale_brightness_percent as f32 / 100.0;
            quality.contrast_adjustment += output.upscale_contrast_percent as f32 / 100.0;
        }
        if image.width() != target_width || image.height() != target_height {
            let resized_output = resize_output_image(image, target_width, target_height);
            image = resized_output.0;
            upscale_backend = resized_output.1;
        }
        if is_upscale {
            image = apply_upscale_corrections(image, quality, output);
        }
        let mut encoded = encode_image(&image, target_format)?;
        if candidate.format == target_format {
            let source_bytes = fs::read(source_path).map_err(|error| {
                format!(
                    "Unable to preserve selected artwork color profile {}: {error}",
                    source_path.display()
                )
            })?;
            preserve_same_format_color_profile(&source_bytes, &mut encoded, target_format);
        }
        encoded
    };

    let profile_applied = resized
        && candidate.width.min(candidate.height) < range.ideal
        && output.upscale_below_ideal;
    let info = PreparedArtworkInfo {
        width: target_width,
        height: target_height,
        format: target_format,
        resized: resized || crop_square,
        converted,
        upscale_backend,
        brightness_percent: quality.brightness_percent(),
        contrast_percent: quality.contrast_percent(),
        exposure_percent: if profile_applied {
            output.upscale_exposure_percent as i8
        } else {
            0
        },
        sharpen_percent: if profile_applied {
            output.upscale_sharpen_percent as u8
        } else {
            0
        },
        color_temperature: if profile_applied {
            output.upscale_color_temperature as i16
        } else {
            0
        },
        adaptive_defaults: profile_applied && output.upscale_adaptive_defaults,
        quality_eligible: quality.automatic_eligible,
    };
    validate_prepared_bytes(&bytes, info)?;
    Ok(PreparedArtwork { bytes, info })
}

fn resize_output_image(
    image: DynamicImage,
    target_width: u32,
    target_height: u32,
) -> (DynamicImage, UpscaleBackend) {
    let is_upscale = target_width > image.width() || target_height > image.height();
    #[cfg(windows)]
    if is_upscale {
        let rgba = image.to_rgba8();
        if let Ok((resized, _adapter_name)) =
            crate::gpu_upscale::resize_lanczos3(&rgba, target_width, target_height)
        {
            return (DynamicImage::ImageRgba8(resized), UpscaleBackend::Gpu);
        }
    }
    let backend = if is_upscale {
        UpscaleBackend::Cpu
    } else {
        UpscaleBackend::None
    };
    (
        image.resize_exact(target_width, target_height, FilterType::Lanczos3),
        backend,
    )
}

pub fn assess_artwork_quality(path: &Path) -> Result<ArtworkQualityAssessment, String> {
    let image = ImageReader::open(path)
        .map_err(|error| {
            format!(
                "Unable to open artwork quality source {}: {error}",
                path.display()
            )
        })?
        .with_guessed_format()
        .map_err(|error| {
            format!(
                "Unable to identify artwork quality source {}: {error}",
                path.display()
            )
        })?
        .decode()
        .map_err(|error| {
            format!(
                "Unable to decode artwork quality source {}: {error}",
                path.display()
            )
        })?;
    Ok(assess_dynamic_image_quality(&image))
}

fn assess_dynamic_image_quality(image: &DynamicImage) -> ArtworkQualityAssessment {
    let rgb = image.to_rgb8();
    let pixel_count = u64::from(rgb.width()) * u64::from(rgb.height());
    if pixel_count == 0 {
        return ArtworkQualityAssessment {
            automatic_eligible: false,
            ..ArtworkQualityAssessment::balanced()
        };
    }

    // Sample at most roughly 65k pixels. This keeps the quality gate cheap even
    // for large provider images while remaining deterministic.
    let step = ((pixel_count as f64 / 65_536.0).sqrt().floor() as u32).max(1);
    let mut count = 0_u64;
    let mut sum = 0.0_f64;
    let mut sum_squares = 0.0_f64;
    let mut shadows = 0_u64;
    let mut highlights = 0_u64;
    for y in (0..rgb.height()).step_by(step as usize) {
        for x in (0..rgb.width()).step_by(step as usize) {
            let pixel = rgb.get_pixel(x, y).0;
            let luminance = (0.2126 * f64::from(pixel[0])
                + 0.7152 * f64::from(pixel[1])
                + 0.0722 * f64::from(pixel[2]))
                / 255.0;
            count += 1;
            sum += luminance;
            sum_squares += luminance * luminance;
            if luminance <= 0.02 {
                shadows += 1;
            }
            if luminance >= 0.98 {
                highlights += 1;
            }
        }
    }
    let mean = (sum / count as f64) as f32;
    let variance = (sum_squares / count as f64 - f64::from(mean) * f64::from(mean)).max(0.0);
    let deviation = variance.sqrt() as f32;
    let shadow_ratio = shadows as f32 / count as f32;
    let highlight_ratio = highlights as f32 / count as f32;

    // Reject only unmistakably unusable tonal data from Preferred/Auto. It
    // remains visible and manually selectable. Deliberately dark/light cover
    // designs with real contrast are not rejected.
    let nearly_flat = deviation < 0.025;
    let crushed = mean < 0.08 && shadow_ratio > 0.70;
    let blown = mean > 0.92 && highlight_ratio > 0.70;
    let automatic_eligible = !nearly_flat && !crushed && !blown;

    // Corrections are deliberately subtle and applied only to upscaled output.
    // Balanced artwork is left unchanged and sharpening is never introduced.
    let brightness_adjustment = if mean < 0.24 && shadow_ratio < 0.70 {
        ((0.24 - mean) * 0.5).min(MAX_BRIGHTNESS_ADJUSTMENT)
    } else if mean > 0.76 && highlight_ratio < 0.70 {
        -((mean - 0.76) * 0.5).min(MAX_BRIGHTNESS_ADJUSTMENT)
    } else {
        0.0
    };
    let contrast_adjustment = if deviation < 0.16 && shadow_ratio < 0.20 && highlight_ratio < 0.20 {
        ((0.16 - deviation) * 0.8).min(MAX_CONTRAST_ADJUSTMENT)
    } else {
        0.0
    };

    ArtworkQualityAssessment {
        mean_luminance: mean,
        luminance_deviation: deviation,
        shadow_clip_ratio: shadow_ratio,
        highlight_clip_ratio: highlight_ratio,
        brightness_adjustment,
        contrast_adjustment,
        automatic_eligible,
    }
}

fn apply_upscale_corrections(
    image: DynamicImage,
    quality: ArtworkQualityAssessment,
    output: &OutputConfig,
) -> DynamicImage {
    if quality.brightness_adjustment.abs() < f32::EPSILON
        && quality.contrast_adjustment.abs() < f32::EPSILON
        && output.upscale_exposure_percent == 0
        && output.upscale_sharpen_percent == 0
        && output.upscale_color_temperature == 0
    {
        return image;
    }
    let mut pixels: RgbaImage = image.to_rgba8();
    let contrast = 1.0 + quality.contrast_adjustment;
    let exposure = 1.0 + output.upscale_exposure_percent as f32 / 100.0;
    let temperature = output.upscale_color_temperature as f32 / 100.0;
    let red_scale = 1.0 + temperature * 0.10;
    let blue_scale = 1.0 - temperature * 0.10;
    for pixel in pixels.pixels_mut() {
        for (index, channel) in pixel.0[..3].iter_mut().enumerate() {
            let normalized = f32::from(*channel) / 255.0;
            let temperature_scale = match index {
                0 => red_scale,
                2 => blue_scale,
                _ => 1.0,
            };
            let corrected = (((normalized - 0.5) * contrast + 0.5 + quality.brightness_adjustment)
                * exposure
                * temperature_scale)
                .clamp(0.0, 1.0);
            *channel = (corrected * 255.0).round() as u8;
        }
    }
    if output.upscale_sharpen_percent > 0 {
        pixels = sharpen_rgba(&pixels, output.upscale_sharpen_percent as f32 / 100.0);
    }
    DynamicImage::ImageRgba8(pixels)
}

fn sharpen_rgba(source: &RgbaImage, amount: f32) -> RgbaImage {
    if source.width() < 3 || source.height() < 3 || amount <= 0.0 {
        return source.clone();
    }
    let mut output = source.clone();
    let edge = amount.clamp(0.0, 0.20) * 0.25;
    for y in 1..source.height() - 1 {
        for x in 1..source.width() - 1 {
            let center = source.get_pixel(x, y);
            let left = source.get_pixel(x - 1, y);
            let right = source.get_pixel(x + 1, y);
            let top = source.get_pixel(x, y - 1);
            let bottom = source.get_pixel(x, y + 1);
            let target = output.get_pixel_mut(x, y);
            for channel in 0..3 {
                let value = f32::from(center[channel]) * (1.0 + 4.0 * edge)
                    - edge
                        * (f32::from(left[channel])
                            + f32::from(right[channel])
                            + f32::from(top[channel])
                            + f32::from(bottom[channel]));
                target[channel] = value.clamp(0.0, 255.0).round() as u8;
            }
        }
    }
    output
}

fn preserve_same_format_color_profile(source: &[u8], encoded: &mut Vec<u8>, format: StaticFormat) {
    match format {
        StaticFormat::Jpeg => preserve_jpeg_icc_profile(source, encoded),
        StaticFormat::Png => preserve_png_iccp_chunk(source, encoded),
        // The image crate preserves decoded color appearance for WebP. Safely
        // rebuilding an extended WebP ICC container requires a full muxer, so
        // no unsupported metadata mutation is attempted here.
        StaticFormat::Webp => {}
    }
}

fn preserve_jpeg_icc_profile(source: &[u8], encoded: &mut Vec<u8>) {
    if source.len() < 4
        || encoded.len() < 2
        || source[..2] != [0xff, 0xd8]
        || encoded[..2] != [0xff, 0xd8]
    {
        return;
    }
    let mut segments = Vec::<u8>::new();
    let mut offset = 2_usize;
    while offset + 4 <= source.len() {
        if source[offset] != 0xff {
            break;
        }
        let marker = source[offset + 1];
        if marker == 0xda || marker == 0xd9 {
            break;
        }
        let length = u16::from_be_bytes([source[offset + 2], source[offset + 3]]) as usize;
        if length < 2 || offset + 2 + length > source.len() {
            break;
        }
        let end = offset + 2 + length;
        if marker == 0xe2 && source[offset + 4..end].starts_with(b"ICC_PROFILE\0") {
            segments.extend_from_slice(&source[offset..end]);
        }
        offset = end;
    }
    if !segments.is_empty() {
        encoded.splice(2..2, segments);
    }
}

fn preserve_png_iccp_chunk(source: &[u8], encoded: &mut Vec<u8>) {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if !source.starts_with(PNG_SIGNATURE) || !encoded.starts_with(PNG_SIGNATURE) {
        return;
    }
    let mut offset = 8_usize;
    let mut chunk = None::<Vec<u8>>;
    while offset + 12 <= source.len() {
        let length = u32::from_be_bytes(source[offset..offset + 4].try_into().unwrap()) as usize;
        let end = offset.saturating_add(12).saturating_add(length);
        if end > source.len() {
            break;
        }
        if &source[offset + 4..offset + 8] == b"iCCP" {
            chunk = Some(source[offset..end].to_vec());
            break;
        }
        offset = end;
    }
    let Some(chunk) = chunk else {
        return;
    };
    if encoded.len() < 33 || &encoded[12..16] != b"IHDR" {
        return;
    }
    let ihdr_length = u32::from_be_bytes(encoded[8..12].try_into().unwrap()) as usize;
    let insert_at = 8 + 12 + ihdr_length;
    if insert_at <= encoded.len() {
        encoded.splice(insert_at..insert_at, chunk);
    }
}

pub fn project_configured_artwork(
    candidate: &Candidate,
    range: &Range,
    output: &OutputConfig,
) -> ProjectedArtwork {
    let source_short_side = candidate.short_side();
    let (width, height, cropped, resized) = projected_dimensions(candidate, range, output);
    let short_side = width.min(height);
    ProjectedArtwork {
        width,
        height,
        cropped,
        resized,
        upscaled: resized && source_short_side < range.ideal,
        acceptable: short_side >= range.min && short_side <= range.ladder,
    }
}

fn projected_dimensions(
    candidate: &Candidate,
    range: &Range,
    output: &OutputConfig,
) -> (u32, u32, bool, bool) {
    if !output.evaluate_final_image {
        return (candidate.width, candidate.height, false, false);
    }

    let mut width = candidate.width;
    let mut height = candidate.height;
    let longest = width.max(height);
    let deviation = if longest == 0 {
        0.0
    } else {
        f64::from(width.abs_diff(height)) / f64::from(longest)
    };
    let crop_square = output.square
        && output.square_mode == "crop"
        && width != height
        && deviation > SQUARE_EQUIVALENT_TOLERANCE
        && deviation <= MAX_AUTO_CROP_DEVIATION;
    if crop_square {
        let side = width.min(height);
        width = side;
        height = side;
        if output.square_round_to > 1 && side >= output.square_round_to {
            let rounded = (side / output.square_round_to) * output.square_round_to;
            if rounded > 0 {
                width = rounded;
                height = rounded;
            }
        }
    }

    let short_side = width.min(height);
    // Ideal is the preferred quality target for smaller sources, not a ceiling.
    // Preserve accepted high-resolution artwork at its native dimensions.
    let resize = short_side < range.ideal
        && output.upscale_below_ideal
        && upscale_within_limit(short_side, range.ideal, output.upscale_max_percent);
    if resize && short_side > 0 {
        (width, height) = dimensions_for_short_side(width, height, range.ideal);
    }
    (width, height, crop_square, resize)
}

fn upscale_within_limit(source_short_side: u32, ideal: u32, max_percent: u32) -> bool {
    source_short_side > 0
        && u64::from(ideal) * 100 <= u64::from(source_short_side) * u64::from(max_percent)
}

fn prepare_final_artwork_inner(
    candidate: &Candidate,
    source_path: &Path,
    range: &Range,
    target_format: StaticFormat,
    allow_outside_range: bool,
) -> Result<PreparedArtwork, String> {
    if !allow_outside_range && !is_acceptable(candidate, range) {
        return Err(format!(
            "Selected artwork candidate is outside the accepted SPLINED range: {}x{}.",
            candidate.width, candidate.height
        ));
    }

    let source = inspect_image(source_path)?;
    if source.width != candidate.width
        || source.height != candidate.height
        || source.format != candidate.format
    {
        return Err(format!(
            "Selected artwork changed after evaluation: expected {}x{} {:?}, found {}x{} {:?}.",
            candidate.width,
            candidate.height,
            candidate.format,
            source.width,
            source.height,
            source.format
        ));
    }

    // Explicit selections above Ideal retain their validated source pixels.
    let resized = false;
    let converted = candidate.format != target_format;
    let (target_width, target_height) = if resized {
        dimensions_for_short_side(candidate.width, candidate.height, range.ideal)
    } else {
        (candidate.width, candidate.height)
    };

    let bytes = if !resized && !converted {
        fs::read(source_path).map_err(|error| {
            format!(
                "Unable to read selected artwork {}: {error}",
                source_path.display()
            )
        })?
    } else {
        let mut image = ImageReader::open(source_path)
            .map_err(|error| {
                format!(
                    "Unable to open selected artwork {}: {error}",
                    source_path.display()
                )
            })?
            .with_guessed_format()
            .map_err(|error| {
                format!(
                    "Unable to identify selected artwork {}: {error}",
                    source_path.display()
                )
            })?
            .decode()
            .map_err(|error| {
                format!(
                    "Unable to decode selected artwork {}: {error}",
                    source_path.display()
                )
            })?;

        if resized {
            image = image.resize_exact(target_width, target_height, FilterType::Lanczos3);
        }

        encode_image(&image, target_format)?
    };

    let info = PreparedArtworkInfo {
        width: target_width,
        height: target_height,
        format: target_format,
        resized,
        converted,
        upscale_backend: UpscaleBackend::None,
        brightness_percent: 0,
        contrast_percent: 0,
        exposure_percent: 0,
        sharpen_percent: 0,
        color_temperature: 0,
        adaptive_defaults: false,
        quality_eligible: true,
    };

    validate_prepared_bytes(&bytes, info)?;
    Ok(PreparedArtwork { bytes, info })
}

pub fn destination_matches_prepared(
    destination: &Path,
    prepared: &PreparedArtwork,
) -> Result<bool, String> {
    if !destination.exists() {
        return Ok(false);
    }

    let existing = fs::read(destination).map_err(|error| {
        format!(
            "Unable to read existing artwork {}: {error}",
            destination.display()
        )
    })?;

    Ok(existing == prepared.bytes)
}

pub fn install_prepared_artwork(
    destination: &Path,
    prepared: &PreparedArtwork,
) -> Result<(), String> {
    let expected = prepared.info;

    replace_binary_file(
        destination,
        &prepared.bytes,
        "artwork",
        move |staged_path| {
            let inspected = inspect_image(staged_path)?;

            if inspected.width != expected.width
                || inspected.height != expected.height
                || inspected.format != expected.format
            {
                return Err(format!(
                    "Staged artwork validation failed: expected {}x{} {:?}, found {}x{} {:?}.",
                    expected.width,
                    expected.height,
                    expected.format,
                    inspected.width,
                    inspected.height,
                    inspected.format
                ));
            }

            Ok(())
        },
    )
}

pub fn finalize_selected_candidate(
    mode: Mode,
    selected: Option<(&Candidate, &Path)>,
    destination: &Path,
    range: &Range,
    target_format: StaticFormat,
) -> Result<FinalArtworkResult, String> {
    let Some((candidate, source_path)) = selected else {
        return Ok(FinalArtworkResult {
            action: FinalArtworkAction::NoSelection,
            info: None,
        });
    };

    let prepared = prepare_final_artwork(candidate, source_path, range, target_format)?;

    if destination_matches_prepared(destination, &prepared)? {
        return Ok(FinalArtworkResult {
            action: FinalArtworkAction::Unchanged,
            info: Some(prepared.info),
        });
    }

    if mode == Mode::Read {
        return Ok(FinalArtworkResult {
            action: FinalArtworkAction::ReadOnly,
            info: Some(prepared.info),
        });
    }

    install_prepared_artwork(destination, &prepared)?;

    Ok(FinalArtworkResult {
        action: FinalArtworkAction::Installed,
        info: Some(prepared.info),
    })
}

fn dimensions_for_short_side(width: u32, height: u32, target_short_side: u32) -> (u32, u32) {
    if width <= height {
        (
            target_short_side,
            scaled_dimension(height, target_short_side, width),
        )
    } else {
        (
            scaled_dimension(width, target_short_side, height),
            target_short_side,
        )
    }
}

fn scaled_dimension(long_side: u32, target_short_side: u32, source_short_side: u32) -> u32 {
    let numerator = u64::from(long_side) * u64::from(target_short_side);
    let denominator = u64::from(source_short_side);
    ((numerator + denominator / 2) / denominator).max(1) as u32
}

fn encode_image(image: &DynamicImage, format: StaticFormat) -> Result<Vec<u8>, String> {
    match format {
        StaticFormat::Jpeg => {
            let mut bytes = Vec::new();
            let mut encoder = JpegEncoder::new_with_quality(&mut bytes, JPEG_QUALITY);
            encoder
                .encode_image(image)
                .map_err(|error| format!("Unable to encode final JPEG artwork: {error}"))?;
            Ok(bytes)
        }
        StaticFormat::Png | StaticFormat::Webp => {
            let image_format = image_format(format);
            let mut cursor = Cursor::new(Vec::new());
            image
                .write_to(&mut cursor, image_format)
                .map_err(|error| format!("Unable to encode final {format:?} artwork: {error}"))?;
            Ok(cursor.into_inner())
        }
    }
}

fn validate_prepared_bytes(bytes: &[u8], expected: PreparedArtworkInfo) -> Result<(), String> {
    if bytes.is_empty() {
        return Err("Prepared artwork was empty.".to_string());
    }

    let detected = image::guess_format(bytes)
        .map_err(|error| format!("Unable to identify prepared artwork bytes: {error}"))?;
    let detected_format = static_format(detected)?;

    if detected_format != expected.format {
        return Err(format!(
            "Prepared artwork format mismatch: expected {:?}, found {:?}.",
            expected.format, detected_format
        ));
    }

    let decoded = image::load_from_memory_with_format(bytes, detected)
        .map_err(|error| format!("Unable to decode prepared artwork bytes: {error}"))?;
    let (width, height) = decoded.dimensions();

    if width != expected.width || height != expected.height {
        return Err(format!(
            "Prepared artwork dimensions mismatch: expected {}x{}, found {}x{}.",
            expected.width, expected.height, width, height
        ));
    }

    Ok(())
}

fn image_format(format: StaticFormat) -> ImageFormat {
    match format {
        StaticFormat::Jpeg => ImageFormat::Jpeg,
        StaticFormat::Png => ImageFormat::Png,
        StaticFormat::Webp => ImageFormat::WebP,
    }
}

fn static_format(format: ImageFormat) -> Result<StaticFormat, String> {
    match format {
        ImageFormat::Jpeg => Ok(StaticFormat::Jpeg),
        ImageFormat::Png => Ok(StaticFormat::Png),
        ImageFormat::WebP => Ok(StaticFormat::Webp),
        other => Err(format!(
            "Unsupported SPLINED prepared artwork format: {other:?}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};
    use tempfile::TempDir;

    fn test_range() -> Range {
        Range {
            min: 12,
            ideal: 18,
            max: 24,
            ladder: 36,
        }
    }

    fn write_image(path: &Path, width: u32, height: u32, format: ImageFormat) {
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(width, height, Rgb([7, 8, 9])));
        let mut cursor = Cursor::new(Vec::new());
        image.write_to(&mut cursor, format).unwrap();
        fs::write(path, cursor.into_inner()).unwrap();
    }

    fn candidate(path: &Path) -> Candidate {
        Candidate::from_file("fixture", path, 0).unwrap()
    }

    #[test]
    fn lower_range_same_format_is_preserved_byte_for_byte() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("source.jpg");
        write_image(&source, 14, 14, ImageFormat::Jpeg);
        let candidate = candidate(&source);
        let original = fs::read(&source).unwrap();
        let prepared =
            prepare_final_artwork(&candidate, &source, &test_range(), StaticFormat::Jpeg).unwrap();
        assert_eq!(prepared.bytes, original);
        assert!(!prepared.info.resized);
        assert!(!prepared.info.converted);
    }

    #[test]
    fn upper_range_preserves_native_resolution() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("source.png");
        write_image(&source, 20, 20, ImageFormat::Png);
        let candidate = candidate(&source);
        let prepared =
            prepare_final_artwork(&candidate, &source, &test_range(), StaticFormat::Png).unwrap();
        assert_eq!((prepared.info.width, prepared.info.height), (20, 20));
        assert!(!prepared.info.resized);
    }

    #[test]
    fn configured_upper_range_preserves_native_resolution() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("source.png");
        write_image(&source, 30, 30, ImageFormat::Png);
        let candidate = candidate(&source);
        let output = OutputConfig {
            evaluate_final_image: true,
            ..OutputConfig::default()
        };

        let projected = project_configured_artwork(&candidate, &test_range(), &output);
        assert_eq!((projected.width, projected.height), (30, 30));
        assert!(!projected.resized);

        let prepared = prepare_configured_artwork(
            &candidate,
            &source,
            &test_range(),
            StaticFormat::Png,
            &output,
            false,
        )
        .unwrap();
        assert_eq!((prepared.info.width, prepared.info.height), (30, 30));
        assert!(!prepared.info.resized);
    }

    #[test]
    fn configured_upscale_respects_maximum_percentage() {
        let range = test_range();
        let mut output = OutputConfig {
            upscale_below_ideal: true,
            upscale_max_percent: 150,
            ..OutputConfig::default()
        };
        let at_limit = Candidate {
            source: "fixture".to_string(),
            width: 12,
            height: 12,
            format: StaticFormat::Jpeg,
            source_priority: 0,
        };
        let over_limit = Candidate {
            width: 11,
            height: 11,
            ..at_limit.clone()
        };

        let projected = project_configured_artwork(&at_limit, &range, &output);
        assert_eq!((projected.width, projected.height), (18, 18));
        assert!(projected.upscaled);
        let projected = project_configured_artwork(&over_limit, &range, &output);
        assert_eq!((projected.width, projected.height), (11, 11));
        assert!(!projected.upscaled);

        output.upscale_max_percent = 200;
        assert!(project_configured_artwork(&over_limit, &range, &output).upscaled);
    }

    #[test]
    fn advanced_profile_applies_only_to_below_ideal_enlargement() {
        let dir = TempDir::new().unwrap();
        let lower_source = dir.path().join("lower.png");
        write_image(&lower_source, 14, 14, ImageFormat::Png);
        let lower = candidate(&lower_source);
        let output = OutputConfig {
            upscale_below_ideal: true,
            upscale_adaptive_defaults: false,
            upscale_sharpen_percent: 4,
            upscale_contrast_percent: 7,
            upscale_exposure_percent: -3,
            upscale_brightness_percent: 5,
            upscale_color_temperature: -25,
            ..OutputConfig::default()
        };
        let enlarged = prepare_configured_artwork(
            &lower,
            &lower_source,
            &test_range(),
            StaticFormat::Png,
            &output,
            false,
        )
        .unwrap();
        assert_eq!((enlarged.info.width, enlarged.info.height), (18, 18));
        assert!(!enlarged.info.adaptive_defaults);
        assert_eq!(enlarged.info.sharpen_percent, 4);
        assert_eq!(enlarged.info.contrast_percent, 7);
        assert_eq!(enlarged.info.exposure_percent, -3);
        assert_eq!(enlarged.info.brightness_percent, 5);
        assert_eq!(enlarged.info.color_temperature, -25);

        let ideal_source = dir.path().join("ideal.png");
        write_image(&ideal_source, 20, 20, ImageFormat::Png);
        let ideal = candidate(&ideal_source);
        let unchanged = prepare_configured_artwork(
            &ideal,
            &ideal_source,
            &test_range(),
            StaticFormat::Png,
            &output,
            false,
        )
        .unwrap();
        assert_eq!((unchanged.info.width, unchanged.info.height), (20, 20));
        assert!(!unchanged.info.resized);
        assert_eq!(unchanged.info.sharpen_percent, 0);
        assert_eq!(unchanged.info.contrast_percent, 0);
        assert_eq!(unchanged.info.exposure_percent, 0);
        assert_eq!(unchanged.info.brightness_percent, 0);
        assert_eq!(unchanged.info.color_temperature, 0);
    }

    #[test]
    fn balanced_artwork_receives_no_tone_correction() {
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_fn(64, 64, |x, _| {
            if x % 2 == 0 {
                Rgb([40, 80, 120])
            } else {
                Rgb([210, 190, 170])
            }
        }));
        let quality = assess_dynamic_image_quality(&image);
        assert!(quality.automatic_eligible);
        assert_eq!(quality.brightness_percent(), 0);
        assert_eq!(quality.contrast_percent(), 0);
    }

    #[test]
    fn flat_artwork_stays_visible_but_is_not_automatic_quality() {
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(64, 64, Rgb([128, 128, 128])));
        let quality = assess_dynamic_image_quality(&image);
        assert!(!quality.automatic_eligible);
        assert!(quality.contrast_percent() <= 8);
    }

    #[test]
    fn adaptive_tone_adjustments_never_exceed_documented_limits() {
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_fn(64, 64, |x, _| {
            let value = if x % 2 == 0 { 35 } else { 75 };
            Rgb([value, value, value])
        }));
        let quality = assess_dynamic_image_quality(&image);
        assert!((-5..=5).contains(&quality.brightness_percent()));
        assert!(quality.contrast_percent() <= 8);
    }

    #[test]
    fn jpeg_icc_segments_are_carried_into_same_format_output() {
        let payload = b"ICC_PROFILE\0\x01\x01profile";
        let length = (payload.len() + 2) as u16;
        let mut source = vec![0xff, 0xd8, 0xff, 0xe2];
        source.extend_from_slice(&length.to_be_bytes());
        source.extend_from_slice(payload);
        source.extend_from_slice(&[0xff, 0xda, 0x00, 0x02]);
        let mut encoded = vec![0xff, 0xd8, 0xff, 0xd9];
        preserve_jpeg_icc_profile(&source, &mut encoded);
        assert!(
            encoded
                .windows(payload.len())
                .any(|window| window == payload)
        );
    }

    #[test]
    fn read_mode_never_mutates_destination() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("source.jpg");
        let destination = dir.path().join("cover.jpg");
        write_image(&source, 18, 18, ImageFormat::Jpeg);
        fs::write(&destination, b"existing artwork").unwrap();
        let candidate = candidate(&source);
        let result = finalize_selected_candidate(
            Mode::Read,
            Some((&candidate, &source)),
            &destination,
            &test_range(),
            StaticFormat::Jpeg,
        )
        .unwrap();
        assert_eq!(result.action, FinalArtworkAction::ReadOnly);
        assert_eq!(fs::read(&destination).unwrap(), b"existing artwork");
    }

    #[test]
    fn write_mode_installs_validated_final_artwork() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("source.png");
        let destination = dir.path().join("cover.jpg");
        write_image(&source, 20, 20, ImageFormat::Png);
        write_image(&destination, 12, 12, ImageFormat::Jpeg);
        let candidate = candidate(&source);
        let result = finalize_selected_candidate(
            Mode::Write,
            Some((&candidate, &source)),
            &destination,
            &test_range(),
            StaticFormat::Jpeg,
        )
        .unwrap();
        assert_eq!(result.action, FinalArtworkAction::Installed);
        let installed = inspect_image(&destination).unwrap();
        assert_eq!((installed.width, installed.height), (20, 20));
        assert_eq!(installed.format, StaticFormat::Jpeg);
        assert!(!dir.path().join("cover-(2).jpg").exists());
    }
}
