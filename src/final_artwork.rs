use crate::candidate::{Candidate, StaticFormat};
use crate::config::{Mode, OutputConfig};
use crate::evaluate::is_acceptable;
use crate::inspect::inspect_image;
use crate::range::Range;
use crate::safe_write::replace_binary_file;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageFormat, ImageReader};
use std::fs;
use std::io::Cursor;
use std::path::Path;

const JPEG_QUALITY: u8 = 95;
const SQUARE_EQUIVALENT_TOLERANCE: f64 = 0.005;
const MAX_AUTO_CROP_DEVIATION: f64 = 0.02;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreparedArtworkInfo {
    pub width: u32,
    pub height: u32,
    pub format: StaticFormat,
    pub resized: bool,
    pub converted: bool,
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
        if image.width() != target_width || image.height() != target_height {
            image = image.resize_exact(target_width, target_height, FilterType::Lanczos3);
        }
        encode_image(&image, target_format)?
    };

    let info = PreparedArtworkInfo {
        width: target_width,
        height: target_height,
        format: target_format,
        resized: resized || crop_square,
        converted,
    };
    validate_prepared_bytes(&bytes, info)?;
    Ok(PreparedArtwork { bytes, info })
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
    let resize =
        short_side > range.ideal || (short_side < range.ideal && output.upscale_below_ideal);
    if resize && short_side > 0 {
        (width, height) = dimensions_for_short_side(width, height, range.ideal);
    }
    (width, height, crop_square, resize)
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

    let resized = candidate.short_side() > range.ideal;
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
    fn upper_range_resizes_short_side_to_ideal() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("source.png");
        write_image(&source, 20, 20, ImageFormat::Png);
        let candidate = candidate(&source);
        let prepared =
            prepare_final_artwork(&candidate, &source, &test_range(), StaticFormat::Png).unwrap();
        assert_eq!((prepared.info.width, prepared.info.height), (18, 18));
        assert!(prepared.info.resized);
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
        assert_eq!((installed.width, installed.height), (18, 18));
        assert_eq!(installed.format, StaticFormat::Jpeg);
    }
}
