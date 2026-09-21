use crate::candidate::Candidate;
use image::ImageFormat;
use reqwest::header::{ACCEPT, REFERER, USER_AGENT};
use reqwest::{Client, Response};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::{Builder, NamedTempFile, TempDir};

pub const MAX_DOWNLOAD_BYTES: usize = 25 * 1024 * 1024;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);
const DEEZER_ARTWORK_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36";
const DEEZER_ARTWORK_ACCEPT: &str =
    "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8";
const DEEZER_ARTWORK_REFERER: &str = "https://www.deezer.com/";

enum CacheRoot {
    Temporary(TempDir),
    Persistent(PathBuf),
}

pub struct DownloadCache {
    root: CacheRoot,
    client: Client,
}

#[derive(Debug)]
enum CandidateFile {
    Temporary(NamedTempFile),
    Persistent(PathBuf),
}

#[derive(Debug)]
pub struct DownloadedCandidate {
    pub candidate: Candidate,
    pub url: String,
    file: CandidateFile,
}

impl DownloadCache {
    pub fn new() -> Result<Self, String> {
        let root = Builder::new()
            .prefix("splined-")
            .tempdir()
            .map_err(|error| format!("Unable to create SPLINED download cache: {error}"))?;

        Ok(Self {
            root: CacheRoot::Temporary(root),
            client: build_client()?,
        })
    }

    pub fn new_persistent(root: &Path) -> Result<Self, String> {
        fs::create_dir_all(root).map_err(|error| {
            format!(
                "Unable to create SPLINED persistent download cache {}: {error}",
                root.display()
            )
        })?;

        if !root.is_dir() {
            return Err(format!(
                "SPLINED persistent download cache is not a directory: {}",
                root.display()
            ));
        }

        Ok(Self {
            root: CacheRoot::Persistent(root.to_path_buf()),
            client: build_client()?,
        })
    }

    pub fn path(&self) -> &Path {
        match &self.root {
            CacheRoot::Temporary(root) => root.path(),
            CacheRoot::Persistent(root) => root.as_path(),
        }
    }

    pub async fn download_candidate(
        &self,
        source: impl Into<String>,
        url: &str,
        source_priority: usize,
    ) -> Result<DownloadedCandidate, String> {
        let source = source.into();
        let mut request = self.client.get(url);

        if source == "deezer" {
            request = request
                .header(USER_AGENT, DEEZER_ARTWORK_USER_AGENT)
                .header(ACCEPT, DEEZER_ARTWORK_ACCEPT)
                .header(REFERER, DEEZER_ARTWORK_REFERER);
        }

        let response = request
            .send()
            .await
            .map_err(|error| format!("Unable to download candidate from {url}: {error}"))?
            .error_for_status()
            .map_err(|error| format!("Candidate download failed for {url}: {error}"))?;

        let bytes = read_limited_body(response, url).await?;
        self.candidate_from_bytes(source, url, source_priority, &bytes)
    }

    fn candidate_from_bytes(
        &self,
        source: String,
        url: &str,
        source_priority: usize,
        bytes: &[u8],
    ) -> Result<DownloadedCandidate, String> {
        if bytes.is_empty() {
            return Err(format!("Candidate download from {url} was empty."));
        }

        if bytes.len() > MAX_DOWNLOAD_BYTES {
            return Err(format!(
                "Candidate download from {url} exceeded the {} byte limit.",
                MAX_DOWNLOAD_BYTES
            ));
        }

        match &self.root {
            CacheRoot::Temporary(_) => {
                let mut file = Builder::new()
                    .prefix("splined-candidate-")
                    .suffix(cache_suffix(bytes))
                    .tempfile()
                    .map_err(|error| format!("Unable to create cached candidate file: {error}"))?;

                write_candidate_bytes(&mut file, bytes)?;
                let candidate = Candidate::from_file(source, file.path(), source_priority)?;

                Ok(DownloadedCandidate {
                    candidate,
                    url: url.to_string(),
                    file: CandidateFile::Temporary(file),
                })
            }
            CacheRoot::Persistent(root) => {
                let mut file = Builder::new()
                    .prefix("splined-candidate-")
                    .suffix(cache_suffix(bytes))
                    .tempfile_in(root)
                    .map_err(|error| {
                        format!(
                            "Unable to create cached candidate file in {}: {error}",
                            root.display()
                        )
                    })?;

                write_candidate_bytes(&mut file, bytes)?;
                let candidate = Candidate::from_file(source, file.path(), source_priority)?;
                let (_file, path) = file.keep().map_err(|error| {
                    format!(
                        "Unable to persist cached candidate in {}: {error}",
                        root.display()
                    )
                })?;

                Ok(DownloadedCandidate {
                    candidate,
                    url: url.to_string(),
                    file: CandidateFile::Persistent(path),
                })
            }
        }
    }
}

fn build_client() -> Result<Client, String> {
    Client::builder()
        .user_agent(concat!("SPLINED/", env!("CARGO_PKG_VERSION")))
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .map_err(|error| format!("Unable to create SPLINED HTTP client: {error}"))
}

fn write_candidate_bytes(file: &mut NamedTempFile, bytes: &[u8]) -> Result<(), String> {
    file.write_all(bytes)
        .map_err(|error| format!("Unable to write cached candidate file: {error}"))?;
    file.flush()
        .map_err(|error| format!("Unable to flush cached candidate file: {error}"))
}

fn cache_suffix(bytes: &[u8]) -> &'static str {
    match image::guess_format(bytes) {
        Ok(ImageFormat::Jpeg) => ".jpg",
        Ok(ImageFormat::Png) => ".png",
        Ok(ImageFormat::WebP) => ".webp",
        _ => ".img",
    }
}

async fn read_limited_body(mut response: Response, url: &str) -> Result<Vec<u8>, String> {
    if let Some(length) = response.content_length()
        && length > MAX_DOWNLOAD_BYTES as u64
    {
        return Err(format!(
            "Candidate download from {url} declared {length} bytes, exceeding the {} byte limit.",
            MAX_DOWNLOAD_BYTES
        ));
    }

    let mut bytes = Vec::with_capacity(
        response
            .content_length()
            .unwrap_or(0)
            .min(MAX_DOWNLOAD_BYTES as u64) as usize,
    );

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("Unable to read candidate download from {url}: {error}"))?
    {
        append_with_limit(&mut bytes, &chunk, url)?;
    }

    Ok(bytes)
}

fn append_with_limit(target: &mut Vec<u8>, chunk: &[u8], url: &str) -> Result<(), String> {
    let next_len = target.len().saturating_add(chunk.len());

    if next_len > MAX_DOWNLOAD_BYTES {
        return Err(format!(
            "Candidate download from {url} exceeded the {} byte limit.",
            MAX_DOWNLOAD_BYTES
        ));
    }

    target.extend_from_slice(chunk);
    Ok(())
}

impl DownloadedCandidate {
    pub fn from_existing_path(
        source: impl Into<String>,
        path: PathBuf,
        source_priority: usize,
        url: impl Into<String>,
    ) -> Result<Self, String> {
        let candidate = Candidate::from_file(source, &path, source_priority)?;
        Ok(Self {
            candidate,
            url: url.into(),
            file: CandidateFile::Persistent(path),
        })
    }

    pub fn path(&self) -> &Path {
        match &self.file {
            CandidateFile::Temporary(file) => file.path(),
            CandidateFile::Persistent(path) => path.as_path(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::candidate::StaticFormat;
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb};
    use std::io::Cursor;

    fn encoded_image(width: u32, height: u32, format: ImageFormat) -> Vec<u8> {
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(width, height, Rgb([0, 0, 0])));
        let mut bytes = Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, format)
            .expect("test image should encode");
        bytes.into_inner()
    }

    #[test]
    fn download_cache_is_created_outside_album_paths() {
        let cache = DownloadCache::new().expect("cache should create");
        assert!(cache.path().exists());
        assert!(cache.path().is_dir());
    }

    #[test]
    fn jpeg_bytes_create_candidate_from_actual_pixels() {
        let cache = DownloadCache::new().expect("cache should create");
        let bytes = encoded_image(1800, 1800, ImageFormat::Jpeg);
        let downloaded = cache
            .candidate_from_bytes(
                "deezer".to_string(),
                "https://example.invalid/cover",
                0,
                &bytes,
            )
            .expect("JPEG candidate should inspect");
        assert_eq!(downloaded.candidate.width, 1800);
        assert_eq!(downloaded.candidate.height, 1800);
        assert_eq!(downloaded.candidate.format, StaticFormat::Jpeg);
        assert_eq!(downloaded.candidate.source, "deezer");
        assert!(downloaded.path().exists());
    }

    #[test]
    fn png_and_webp_bytes_are_inspected_by_content() {
        let cache = DownloadCache::new().expect("cache should create");
        let png = encoded_image(2400, 1800, ImageFormat::Png);
        let downloaded = cache
            .candidate_from_bytes(
                "itunes".to_string(),
                "https://example.invalid/fake.jpg",
                1,
                &png,
            )
            .unwrap();
        assert_eq!(downloaded.candidate.format, StaticFormat::Png);

        let webp = encoded_image(1200, 1200, ImageFormat::WebP);
        let downloaded = cache
            .candidate_from_bytes(
                "lastfm".to_string(),
                "https://example.invalid/image",
                2,
                &webp,
            )
            .unwrap();
        assert_eq!(downloaded.candidate.format, StaticFormat::Webp);
    }

    #[test]
    fn corrupt_and_empty_downloads_are_rejected() {
        let cache = DownloadCache::new().expect("cache should create");
        assert!(
            cache
                .candidate_from_bytes(
                    "discogs".to_string(),
                    "https://example.invalid/corrupt",
                    4,
                    b"this is not an image",
                )
                .is_err()
        );
        assert!(
            cache
                .candidate_from_bytes(
                    "deezer".to_string(),
                    "https://example.invalid/empty",
                    0,
                    &[],
                )
                .is_err()
        );
    }

    #[test]
    fn temporary_candidate_survives_cache_drop_then_cleans_up_on_drop() {
        let bytes = encoded_image(1800, 1800, ImageFormat::Jpeg);
        let downloaded = {
            let cache = DownloadCache::new().expect("cache should create");
            cache
                .candidate_from_bytes(
                    "itunes".to_string(),
                    "https://example.invalid/cover",
                    0,
                    &bytes,
                )
                .unwrap()
        };
        let path = downloaded.path().to_path_buf();
        assert!(path.exists());
        drop(downloaded);
        assert!(!path.exists());
    }

    #[test]
    fn persistent_cache_keeps_capture_after_candidate_drop() {
        let root = TempDir::new().expect("persistent cache fixture should create");
        let cache = DownloadCache::new_persistent(root.path()).unwrap();
        let bytes = encoded_image(1800, 1800, ImageFormat::Jpeg);
        let path = {
            let downloaded = cache
                .candidate_from_bytes(
                    "itunes".to_string(),
                    "https://example.invalid/cover",
                    0,
                    &bytes,
                )
                .unwrap();
            downloaded.path().to_path_buf()
        };
        assert!(path.exists());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn chunk_limit_enforces_exact_boundary() {
        let mut target = vec![0_u8; MAX_DOWNLOAD_BYTES - 1];
        append_with_limit(&mut target, &[1], "https://example.invalid").unwrap();
        assert_eq!(target.len(), MAX_DOWNLOAD_BYTES);
        assert!(append_with_limit(&mut target, &[1], "https://example.invalid").is_err());
    }
}
