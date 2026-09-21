use crate::candidate::{Candidate, StaticFormat};
use crate::config::{Config, Mode};
use crate::download::DownloadedCandidate;
use crate::final_artwork::{prepare_configured_artwork, project_configured_artwork};
use crate::inspect::inspect_image;
use crate::pipeline::PipelineCandidate;
use crate::range::{Range, RangeClass};
use crate::safe_write::replace_binary_file;
use crate::scan::AlbumDirectory;
use crate::source::ArtworkReference;
use lofty::file::TaggedFileExt;
use lofty::picture::PictureType;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalPreflightAction {
    None,
    Compare,
    LocalIdeal,
    EmbeddedIdeal,
}

#[derive(Debug)]
pub struct LocalPreflight {
    pub action: LocalPreflightAction,
    pub candidate: Option<PipelineCandidate>,
    pub cleanup: Vec<PathBuf>,
    pub diagnostics: Vec<String>,
    pub webp_source: Option<Candidate>,
    pub generated_destination: Option<PathBuf>,
}

impl Default for LocalPreflight {
    fn default() -> Self {
        Self {
            action: LocalPreflightAction::None,
            candidate: None,
            cleanup: Vec::new(),
            diagnostics: Vec::new(),
            webp_source: None,
            generated_destination: None,
        }
    }
}

pub fn inspect_local_preflight(
    album: &AlbumDirectory,
    config: &Config,
    range: &Range,
    cache_dir: &Path,
) -> LocalPreflight {
    let mut result = LocalPreflight::default();
    let files = match canonical_local_files(&album.path, &config.output.file_name) {
        Ok(files) => files,
        Err(error) => {
            result.diagnostics.push(error);
            return result;
        }
    };

    // Match splined_scan.py: cover.webp is source material. Preserve it,
    // generate a safe JPEG still with the normal final-image policy, and put
    // that still into the Local & Suggested comparison.
    for path in &files.webp {
        match webp_still_candidate(path, album, config, range, cache_dir) {
            Ok((candidate, source, destination)) => {
                result.action = LocalPreflightAction::Compare;
                result.candidate = Some(pipeline_candidate(
                    candidate,
                    "CoverFile",
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("cover.webp"),
                ));
                result.webp_source = Some(source);
                result.generated_destination = Some(destination);
                return result;
            }
            Err(error) => result
                .diagnostics
                .push(format!("{}: {error}", path.display())),
        }
    }

    let mut local_candidates = Vec::new();
    for path in files.jpeg.iter().chain(files.png.iter()) {
        match DownloadedCandidate::from_existing_path("local", path.clone(), 0, "") {
            Ok(candidate) => local_candidates.push(candidate),
            Err(error) => result
                .diagnostics
                .push(format!("{}: {error}", path.display())),
        }
    }
    if !local_candidates.is_empty() {
        local_candidates.sort_by_key(|item| local_candidate_key(&item.candidate, range, config));
        let best = local_candidates.remove(0);
        result.cleanup = local_candidates
            .iter()
            .map(|candidate| candidate.path().to_path_buf())
            .collect();
        let projected = project_configured_artwork(&best.candidate, range, &config.output);
        result.action =
            if range.classify(projected.width.min(projected.height)) == RangeClass::Ideal {
                LocalPreflightAction::LocalIdeal
            } else {
                LocalPreflightAction::Compare
            };
        let reference = best
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("cover file")
            .to_string();
        result.candidate = Some(pipeline_candidate(best, "CoverFile", &reference));
        return result;
    }

    if let Some(audio_path) = album.audio_files.iter().min() {
        match embedded_candidate(audio_path, cache_dir) {
            Ok(Some(candidate)) => {
                let projected =
                    project_configured_artwork(&candidate.candidate, range, &config.output);
                result.action =
                    if range.classify(projected.width.min(projected.height)) == RangeClass::Ideal {
                        LocalPreflightAction::EmbeddedIdeal
                    } else {
                        LocalPreflightAction::Compare
                    };
                let reference = audio_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("audio track");
                result.candidate = Some(pipeline_candidate(candidate, "EmbeddedTrack", reference));
            }
            Ok(None) => {}
            Err(error) => result
                .diagnostics
                .push(format!("{}: {error}", audio_path.display())),
        }
    }
    result
}

pub fn cleanup_competing_static(
    paths: &[PathBuf],
    mode: Mode,
    preserve_file: bool,
) -> Result<(), String> {
    if mode == Mode::Read || preserve_file {
        return Ok(());
    }
    for path in paths {
        // These paths came only from validated matching cover files found as
        // regular files inside the current album. Never broaden this removal
        // to a recursive search or an unresolved glob.
        fs::remove_file(path).map_err(|error| {
            format!(
                "Unable to remove competing local artwork {}: {error}",
                path.display()
            )
        })?;
    }
    Ok(())
}

/// After a better candidate has been safely installed in overwrite mode,
/// remove superseded matching JPG/PNG cover files. WebP source material is
/// never removed. Every target is resolved from a regular file directly
/// inside the current album; no glob or recursive deletion is used.
pub fn cleanup_replaced_static_covers(
    album_path: &Path,
    configured_file_name: &str,
    keep_destination: &Path,
    mode: Mode,
    preserve_file: bool,
) -> Result<Vec<PathBuf>, String> {
    if mode == Mode::Read || preserve_file {
        return Ok(Vec::new());
    }
    let keep =
        fs::canonicalize(keep_destination).unwrap_or_else(|_| keep_destination.to_path_buf());
    let configured_prefix = configured_file_name.trim().to_ascii_lowercase();
    let entries = fs::read_dir(album_path).map_err(|error| {
        format!(
            "Unable to inspect replaced local covers in {}: {error}",
            album_path.display()
        )
    })?;
    let mut removed = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "Unable to read a local cover entry in {}: {error}",
                album_path.display()
            )
        })?;
        let file_type = entry.file_type().map_err(|error| {
            format!(
                "Unable to inspect local cover {}: {error}",
                entry.path().display()
            )
        })?;
        if !file_type.is_file() || file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "jpg" | "jpeg" | "png") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !stem.starts_with("cover")
            && (configured_prefix.is_empty() || !stem.starts_with(&configured_prefix))
        {
            continue;
        }
        let resolved = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if resolved == keep {
            continue;
        }
        fs::remove_file(&path).map_err(|error| {
            format!(
                "Unable to remove superseded local cover {}: {error}",
                path.display()
            )
        })?;
        removed.push(path);
    }
    Ok(removed)
}

fn pipeline_candidate(
    downloaded: DownloadedCandidate,
    origin_type: &str,
    reference_id: &str,
) -> PipelineCandidate {
    let source = downloaded.candidate.source.clone();
    PipelineCandidate {
        reference: ArtworkReference {
            source,
            id: reference_id.to_string(),
            url: String::new(),
            front: true,
            approved: true,
            types: vec!["Front".to_string(), origin_type.to_string()],
        },
        downloaded,
    }
}

fn local_candidate_key(candidate: &Candidate, range: &Range, config: &Config) -> (u8, u32, u32) {
    let projected = project_configured_artwork(candidate, range, &config.output);
    let short = projected.width.min(projected.height);
    let class = range.classify(short);
    let class_priority = match class {
        RangeClass::Ideal => 0,
        RangeClass::LowerRange | RangeClass::UpperRange => 1,
        RangeClass::Ladder => 2,
        RangeClass::BelowMinimum | RangeClass::AboveLadder => 3,
    };
    (
        class_priority,
        short.abs_diff(range.ideal),
        u32::MAX - short,
    )
}

struct CanonicalFiles {
    webp: Vec<PathBuf>,
    jpeg: Vec<PathBuf>,
    png: Vec<PathBuf>,
}

fn canonical_local_files(album_path: &Path, file_name: &str) -> Result<CanonicalFiles, String> {
    let mut files = CanonicalFiles {
        webp: Vec::new(),
        jpeg: Vec::new(),
        png: Vec::new(),
    };
    let configured_prefix = file_name.trim().to_ascii_lowercase();
    let entries = fs::read_dir(album_path).map_err(|error| {
        format!(
            "Unable to inspect local artwork in {}: {error}",
            album_path.display()
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "Unable to read local artwork entry in {}: {error}",
                album_path.display()
            )
        })?;
        let file_type = entry.file_type().map_err(|error| {
            format!(
                "Unable to inspect local artwork {}: {error}",
                entry.path().display()
            )
        })?;
        if !file_type.is_file() || file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !stem.starts_with("cover")
            && (configured_prefix.is_empty() || !stem.starts_with(&configured_prefix))
        {
            continue;
        }
        match path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "webp" => files.webp.push(path),
            "jpg" | "jpeg" => files.jpeg.push(path),
            "png" => files.png.push(path),
            _ => {}
        }
    }
    files.webp.sort();
    files.jpeg.sort();
    files.png.sort();
    Ok(files)
}

fn webp_still_candidate(
    path: &Path,
    album: &AlbumDirectory,
    config: &Config,
    range: &Range,
    cache_dir: &Path,
) -> Result<(DownloadedCandidate, Candidate, PathBuf), String> {
    let source = Candidate::from_file("local", path, 0)?;
    if source.format != StaticFormat::Webp {
        return Err("Canonical .webp did not decode as WebP.".to_string());
    }
    let prepared = prepare_configured_artwork(
        &source,
        path,
        range,
        StaticFormat::Jpeg,
        &config.output,
        true,
    )?;
    let destination = if config.mode == Mode::Write {
        album.path.join(format!("{}.jpg", config.output.file_name))
    } else {
        cache_path(cache_dir, "webp-still", path, &prepared.bytes, "jpg")
    };
    replace_binary_file(
        &destination,
        &prepared.bytes,
        "local WebP still",
        |staged| inspect_image(staged).map(|_| ()),
    )?;
    let still = DownloadedCandidate::from_existing_path("webpstill", destination.clone(), 0, "")?;
    Ok((still, source, destination))
}

fn embedded_candidate(
    audio_path: &Path,
    cache_dir: &Path,
) -> Result<Option<DownloadedCandidate>, String> {
    let tagged = lofty::read_from_path(audio_path)
        .map_err(|error| format!("Unable to read embedded artwork tags: {error}"))?;
    let mut first: Option<Vec<u8>> = None;
    let mut front: Option<Vec<u8>> = None;
    for tag in tagged.tags() {
        for picture in tag.pictures() {
            if first.is_none() {
                first = Some(picture.data().to_vec());
            }
            if picture.pic_type() == PictureType::CoverFront {
                front = Some(picture.data().to_vec());
                break;
            }
        }
        if front.is_some() {
            break;
        }
    }
    let Some(bytes) = front.or(first) else {
        return Ok(None);
    };
    let extension = match image::guess_format(&bytes) {
        Ok(image::ImageFormat::Jpeg) => "jpg",
        Ok(image::ImageFormat::Png) => "png",
        Ok(image::ImageFormat::WebP) => "webp",
        _ => return Err("Embedded front cover is not JPEG, PNG, or WebP.".to_string()),
    };
    let path = cache_path(cache_dir, "embedded", audio_path, &bytes, extension);
    replace_binary_file(&path, &bytes, "embedded artwork cache", |staged| {
        inspect_image(staged).map(|_| ())
    })?;
    DownloadedCandidate::from_existing_path("embedded", path, 0, "").map(Some)
}

fn cache_path(
    cache_dir: &Path,
    kind: &str,
    source: &Path,
    bytes: &[u8],
    extension: &str,
) -> PathBuf {
    let mut digest = Sha256::new();
    digest.update(source.to_string_lossy().as_bytes());
    digest.update(&bytes[..bytes.len().min(4096)]);
    let digest: String = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    cache_dir.join(format!(
        "splined-local-{kind}-{}.{}",
        &digest[..24],
        extension
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb};
    use std::io::Cursor;
    use tempfile::tempdir;

    fn write_image(path: &Path, width: u32, height: u32, format: ImageFormat) {
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(width, height, Rgb([3, 4, 5])));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, format).unwrap();
        fs::write(path, bytes.into_inner()).unwrap();
    }

    #[test]
    fn ideal_canonical_static_cover_short_circuits_to_local() {
        let temp = tempdir().unwrap();
        let album_path = temp.path().join("album");
        let cache = temp.path().join("cache");
        fs::create_dir_all(&album_path).unwrap();
        fs::create_dir_all(&cache).unwrap();
        write_image(&album_path.join("Cover.JPG"), 1800, 1800, ImageFormat::Jpeg);
        let album = AlbumDirectory {
            path: album_path,
            audio_files: Vec::new(),
        };
        let preflight =
            inspect_local_preflight(&album, &Config::default(), &Range::default(), &cache);
        assert_eq!(preflight.action, LocalPreflightAction::LocalIdeal);
        assert_eq!(
            preflight.candidate.unwrap().downloaded.candidate.source,
            "local"
        );
    }

    #[test]
    fn webp_is_preserved_and_safe_jpeg_still_is_compared() {
        let temp = tempdir().unwrap();
        let album_path = temp.path().join("album");
        let cache = temp.path().join("cache");
        fs::create_dir_all(&album_path).unwrap();
        fs::create_dir_all(&cache).unwrap();
        let webp = album_path.join("cover.webp");
        write_image(&webp, 600, 600, ImageFormat::WebP);
        let album = AlbumDirectory {
            path: album_path,
            audio_files: Vec::new(),
        };
        let mut config = Config::default();
        config.mode = Mode::Read;
        let preflight = inspect_local_preflight(&album, &config, &Range::default(), &cache);
        assert_eq!(preflight.action, LocalPreflightAction::Compare);
        assert_eq!(
            preflight.candidate.unwrap().downloaded.candidate.source,
            "webpstill"
        );
        assert!(webp.exists());
        assert!(preflight.generated_destination.unwrap().exists());
    }

    #[test]
    fn numbered_cover_files_participate_in_local_preflight() {
        let temp = tempdir().unwrap();
        let album_path = temp.path().join("album");
        let cache = temp.path().join("cache");
        fs::create_dir_all(&album_path).unwrap();
        fs::create_dir_all(&cache).unwrap();
        write_image(
            &album_path.join("cover-(2).png"),
            1800,
            1800,
            ImageFormat::Png,
        );
        let album = AlbumDirectory {
            path: album_path,
            audio_files: Vec::new(),
        };
        let preflight =
            inspect_local_preflight(&album, &Config::default(), &Range::default(), &cache);
        assert_eq!(preflight.action, LocalPreflightAction::LocalIdeal);
        assert_eq!(preflight.candidate.unwrap().reference.id, "cover-(2).png");
    }

    #[test]
    fn overwrite_cleanup_removes_only_matching_static_covers_and_never_webp() {
        let temp = tempdir().unwrap();
        let album = temp.path();
        let keep = album.join("cover.png");
        write_image(&keep, 1800, 1800, ImageFormat::Png);
        write_image(&album.join("cover.jpg"), 1200, 1200, ImageFormat::Jpeg);
        write_image(&album.join("cover-(2).png"), 1200, 1200, ImageFormat::Png);
        write_image(&album.join("cover.webp"), 1200, 1200, ImageFormat::WebP);
        write_image(&album.join("booklet.jpg"), 1200, 1200, ImageFormat::Jpeg);

        let removed =
            cleanup_replaced_static_covers(album, "cover", &keep, Mode::Write, false).unwrap();
        assert_eq!(removed.len(), 2);
        assert!(keep.exists());
        assert!(album.join("cover.webp").exists());
        assert!(album.join("booklet.jpg").exists());
        assert!(!album.join("cover.jpg").exists());
        assert!(!album.join("cover-(2).png").exists());
    }

    #[test]
    fn preserve_mode_never_cleans_numbered_static_covers() {
        let temp = tempdir().unwrap();
        let keep = temp.path().join("cover.jpg");
        let numbered = temp.path().join("cover-(2).jpg");
        write_image(&keep, 1800, 1800, ImageFormat::Jpeg);
        write_image(&numbered, 1200, 1200, ImageFormat::Jpeg);
        let removed =
            cleanup_replaced_static_covers(temp.path(), "cover", &keep, Mode::Write, true).unwrap();
        assert!(removed.is_empty());
        assert!(numbered.exists());
    }
}
