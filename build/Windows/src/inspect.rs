use crate::candidate::StaticFormat;
use image::{ImageFormat, ImageReader};
use std::path::Path;

pub const MAX_IMAGE_PIXELS: u64 = 64 * 1024 * 1024;

fn validate_dimensions(path: &Path, width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err(format!(
            "Invalid image dimensions {}: {}x{}",
            path.display(),
            width,
            height
        ));
    }

    if u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return Err(format!(
            "Image {} exceeds the decoded-image limit: {}x{} (maximum {} pixels)",
            path.display(),
            width,
            height,
            MAX_IMAGE_PIXELS
        ));
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InspectedImage {
    pub width: u32,
    pub height: u32,
    pub format: StaticFormat,
}

pub fn inspect_image(path: &Path) -> Result<InspectedImage, String> {
    let reader = ImageReader::open(path)
        .map_err(|error| format!("Unable to open image {}: {error}", path.display()))?
        .with_guessed_format()
        .map_err(|error| format!("Unable to identify image {}: {error}", path.display()))?;

    let format = reader
        .format()
        .ok_or_else(|| format!("Unable to determine image format: {}", path.display()))?;

    let static_format = match format {
        ImageFormat::Jpeg => StaticFormat::Jpeg,
        ImageFormat::Png => StaticFormat::Png,
        ImageFormat::WebP => StaticFormat::Webp,
        other => {
            return Err(format!(
                "Unsupported SPLINED static image format: {other:?}"
            ));
        }
    };

    let (width, height) = reader.into_dimensions().map_err(|error| {
        format!(
            "Unable to read image dimensions {}: {error}",
            path.display()
        )
    })?;

    validate_dimensions(path, width, height)?;

    Ok(InspectedImage {
        width,
        height,
        format: static_format,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb};
    use std::fs;
    use std::io::Cursor;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_path(extension: &str) -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);

        std::env::temp_dir().join(format!(
            "splined-inspect-{}-{}.{extension}",
            std::process::id(),
            id
        ))
    }

    fn write_image(path: &Path, width: u32, height: u32, format: ImageFormat) {
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(width, height, Rgb([0, 0, 0])));

        let mut bytes = Cursor::new(Vec::new());

        image
            .write_to(&mut bytes, format)
            .expect("test image should encode");

        fs::write(path, bytes.into_inner()).expect("test image should write");
    }

    #[test]
    fn jpeg_dimensions_are_inspected() {
        let path = temp_path("jpg");
        write_image(&path, 1800, 1800, ImageFormat::Jpeg);
        let result = inspect_image(&path).expect("JPEG should inspect");
        assert_eq!(result.width, 1800);
        assert_eq!(result.height, 1800);
        assert_eq!(result.format, StaticFormat::Jpeg);
        fs::remove_file(path).ok();
    }

    #[test]
    fn png_dimensions_are_inspected() {
        let path = temp_path("png");
        write_image(&path, 2400, 1800, ImageFormat::Png);
        let result = inspect_image(&path).expect("PNG should inspect");
        assert_eq!(result.width, 2400);
        assert_eq!(result.height, 1800);
        assert_eq!(result.format, StaticFormat::Png);
        fs::remove_file(path).ok();
    }

    #[test]
    fn webp_dimensions_are_inspected() {
        let path = temp_path("webp");
        write_image(&path, 1200, 1200, ImageFormat::WebP);
        let result = inspect_image(&path).expect("WebP should inspect");
        assert_eq!(result.width, 1200);
        assert_eq!(result.height, 1200);
        assert_eq!(result.format, StaticFormat::Webp);
        fs::remove_file(path).ok();
    }

    #[test]
    fn content_detection_ignores_wrong_extension() {
        let path = temp_path("jpg");
        write_image(&path, 1900, 1700, ImageFormat::Png);
        let result = inspect_image(&path).expect("content should determine format");
        assert_eq!(result.width, 1900);
        assert_eq!(result.height, 1700);
        assert_eq!(result.format, StaticFormat::Png);
        fs::remove_file(path).ok();
    }

    #[test]
    fn corrupt_image_fails() {
        let path = temp_path("jpg");
        fs::write(&path, b"not an image").expect("corrupt fixture should write");
        let result = inspect_image(&path);
        assert!(result.is_err());
        fs::remove_file(path).ok();
    }

    #[test]
    fn missing_image_fails() {
        let path = temp_path("jpg");
        let result = inspect_image(&path);
        assert!(result.is_err());
    }

    #[test]
    fn oversized_decoded_dimensions_are_rejected() {
        let path = Path::new("oversized.jpg");
        let result = validate_dimensions(path, 8192, 8193);
        assert!(result.unwrap_err().contains("decoded-image limit"));
    }
}
