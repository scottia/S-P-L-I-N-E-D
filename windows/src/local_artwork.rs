use crate::candidate::{Candidate, StaticFormat};
use crate::config::Config;
#[cfg(test)]
use crate::config::Mode;
use crate::download::DownloadedCandidate;
use crate::final_artwork::{
    PreparedArtwork, prepare_configured_artwork, project_configured_artwork,
};
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
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tempfile::{Builder, NamedTempFile};

static EMBEDDED_PREVIEW_CACHE_LOCK: Mutex<()> = Mutex::new(());

pub fn embedded_preview_cache_dir(configured_cache_dir: &Path) -> PathBuf {
    crate::portable::app_layout()
        .map(|layout| layout.config_dir.join("preview-cache"))
        .unwrap_or_else(|_| configured_cache_dir.to_path_buf())
}

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
        result.action = if config.output.preserve_file
            && range.classify(projected.width.min(projected.height)) == RangeClass::Ideal
        {
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
                result.action = if config.output.preserve_file
                    && range.classify(projected.width.min(projected.height)) == RangeClass::Ideal
                {
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

/// Install an overwrite-mode canonical presentation as one recoverable Album
/// transaction. Replacement bytes are staged and decoded before any existing
/// file moves. Existing matching JPG/JPEG/PNG files are quarantined inside the
/// selected Album and restored in full if quarantine, publication, or readback
/// verification fails. WebP is never a static-cleanup target. When the selected
/// output is WebP, the WebP source and a verified JPEG still are committed
/// together; an existing exact WebP is then replaced, not discarded.
pub fn install_prepared_canonical_artwork(
    album_path: &Path,
    configured_file_name: &str,
    destination: &Path,
    prepared: &PreparedArtwork,
) -> Result<Vec<PathBuf>, String> {
    install_prepared_canonical_artwork_with(
        album_path,
        configured_file_name,
        destination,
        prepared,
        |_, _| Ok(()),
    )
}

fn install_prepared_canonical_artwork_with<F>(
    album_path: &Path,
    configured_file_name: &str,
    destination: &Path,
    prepared: &PreparedArtwork,
    mut before_quarantine: F,
) -> Result<Vec<PathBuf>, String>
where
    F: FnMut(usize, &Path) -> Result<(), String>,
{
    let configured = configured_file_name.trim();
    if configured.is_empty()
        || configured == "."
        || configured == ".."
        || configured.contains(['/', '\\'])
    {
        return Err("Canonical cover basename must be one filename stem.".to_string());
    }
    let verified_album = fs::canonicalize(album_path).map_err(|error| {
        format!(
            "Unable to verify selected Album directory {}: {error}",
            album_path.display()
        )
    })?;
    if !verified_album.is_dir() {
        return Err("The selected Album artwork path is not a directory.".to_string());
    }
    let destination_parent = destination
        .parent()
        .ok_or_else(|| "Canonical cover destination has no Album directory.".to_string())?;
    let verified_parent = fs::canonicalize(destination_parent).map_err(|error| {
        format!(
            "Unable to verify canonical cover directory {}: {error}",
            destination_parent.display()
        )
    })?;
    let destination_stem = destination
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if verified_parent != verified_album || !destination_stem.eq_ignore_ascii_case(configured) {
        return Err(format!(
            "Canonical cover destination must be directly inside the selected Album and use basename {configured}."
        ));
    }

    let destination_extension = destination
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let extension_matches = match prepared.info.format {
        StaticFormat::Jpeg => matches!(destination_extension.as_str(), "jpg" | "jpeg"),
        StaticFormat::Png => destination_extension == "png",
        StaticFormat::Webp => destination_extension == "webp",
    };
    if !extension_matches {
        return Err("Canonical cover extension does not match prepared artwork.".to_string());
    }

    let mut outputs = vec![(destination.to_path_buf(), prepared.clone())];
    if prepared.info.format == StaticFormat::Webp {
        let image = image::load_from_memory(&prepared.bytes)
            .map_err(|error| format!("Unable to decode WebP source for its still: {error}"))?;
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 95)
            .encode_image(&image)
            .map_err(|error| format!("Unable to encode WebP presentation still: {error}"))?;
        let mut info = prepared.info;
        info.format = StaticFormat::Jpeg;
        info.converted = true;
        outputs.push((
            verified_album.join(format!("{configured}.jpg")),
            PreparedArtwork { bytes, info },
        ));
    }

    let mut staged_outputs = Vec::<(PathBuf, PreparedArtwork, NamedTempFile)>::new();
    for (output, artwork) in outputs {
        let mut staged = Builder::new()
            .prefix(".splined-cover-stage-")
            .suffix(".tmp")
            .tempfile_in(&verified_album)
            .map_err(|error| format!("Unable to create staged Album artwork: {error}"))?;
        staged
            .write_all(&artwork.bytes)
            .map_err(|error| format!("Unable to write staged Album artwork: {error}"))?;
        staged
            .flush()
            .map_err(|error| format!("Unable to flush staged Album artwork: {error}"))?;
        staged
            .as_file()
            .sync_all()
            .map_err(|error| format!("Unable to sync staged Album artwork: {error}"))?;
        verify_prepared_artwork(staged.path(), &artwork)?;
        staged_outputs.push((output, artwork, staged));
    }

    let configured_lower = configured.to_ascii_lowercase();
    let mut targets = fs::read_dir(&verified_album)
        .map_err(|error| {
            format!(
                "Unable to inspect replaced local covers in {}: {error}",
                verified_album.display()
            )
        })?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .is_ok_and(|kind| kind.is_file() && !kind.is_symlink())
        })
        .map(|entry| entry.path())
        .filter(|path| matching_static_canonical(path, &configured_lower))
        .collect::<Vec<_>>();
    if prepared.info.format == StaticFormat::Webp
        && destination.is_file()
        && !destination.is_symlink()
    {
        targets.push(destination.to_path_buf());
    }
    targets.sort_by_key(|path| path.to_string_lossy().to_ascii_lowercase());
    targets.dedup_by(|left, right| left.eq_ignore_ascii_case(right));

    let output_paths = staged_outputs
        .iter()
        .map(|(path, _, _)| path.clone())
        .collect::<Vec<_>>();
    let removed = targets
        .iter()
        .filter(|path| {
            !output_paths
                .iter()
                .any(|output| output.eq_ignore_ascii_case(path))
        })
        .cloned()
        .collect::<Vec<_>>();
    let rollback = Builder::new()
        .prefix(".splined-cover-rollback-")
        .tempdir_in(&verified_album)
        .map_err(|error| format!("Unable to create Album artwork rollback area: {error}"))?;
    let mut quarantined = Vec::<(PathBuf, PathBuf)>::new();
    for (index, original) in targets.iter().enumerate() {
        if let Err(error) = before_quarantine(index, original) {
            restore_canonical_artwork(&[], &quarantined)?;
            return Err(error);
        }
        let backup =
            rollback.path().join(original.file_name().ok_or_else(|| {
                format!("Unable to identify canonical cover {}", original.display())
            })?);
        if let Err(error) = fs::rename(original, &backup) {
            restore_canonical_artwork(&[], &quarantined)?;
            return Err(format!(
                "Unable to quarantine canonical cover {}: {error}",
                original.display()
            ));
        }
        quarantined.push((original.clone(), backup));
    }

    let mut published = Vec::new();
    for (output, artwork, staged) in staged_outputs {
        if let Err(error) = staged.persist(&output) {
            restore_canonical_artwork(&published, &quarantined)?;
            return Err(format!(
                "Unable to publish staged canonical cover {}: {}",
                output.display(),
                error.error
            ));
        }
        published.push(output.clone());
        if let Err(error) = verify_prepared_artwork(&output, &artwork) {
            restore_canonical_artwork(&published, &quarantined)?;
            return Err(error);
        }
    }

    // The canonical transaction is committed once every output has passed
    // readback. Failure to remove the hidden rollback directory cannot expose a
    // competing cover and is safe to retry or clean on a later maintenance run.
    let _ = rollback.close();
    Ok(removed)
}

fn matching_static_canonical(path: &Path, configured_lower: &str) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "jpg" | "jpeg" | "png") {
        return false;
    }
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    stem == configured_lower
        || stem
            .strip_prefix(&format!("{configured_lower}-("))
            .and_then(|number| number.strip_suffix(')'))
            .is_some_and(|number| number.parse::<usize>().is_ok_and(|number| number >= 2))
}

fn verify_prepared_artwork(path: &Path, prepared: &PreparedArtwork) -> Result<(), String> {
    let inspected = inspect_image(path)?;
    if inspected.width != prepared.info.width
        || inspected.height != prepared.info.height
        || inspected.format != prepared.info.format
    {
        return Err(format!(
            "Canonical artwork verification failed for {}: expected {}x{} {:?}, found {}x{} {:?}.",
            path.display(),
            prepared.info.width,
            prepared.info.height,
            prepared.info.format,
            inspected.width,
            inspected.height,
            inspected.format
        ));
    }
    let installed = fs::read(path)
        .map_err(|error| format!("Unable to read back canonical artwork: {error}"))?;
    if installed != prepared.bytes {
        return Err(format!(
            "Canonical artwork failed byte-for-byte readback: {}",
            path.display()
        ));
    }
    Ok(())
}

fn restore_canonical_artwork(
    published: &[PathBuf],
    quarantined: &[(PathBuf, PathBuf)],
) -> Result<(), String> {
    let mut errors = Vec::new();
    for path in published.iter().rev() {
        if path.exists()
            && let Err(error) = fs::remove_file(path)
        {
            errors.push(format!("remove {}: {error}", path.display()));
        }
    }
    for (original, backup) in quarantined.iter().rev() {
        if backup.exists()
            && let Err(error) = fs::rename(backup, original)
        {
            errors.push(format!(
                "restore {} from {}: {error}",
                original.display(),
                backup.display()
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Album artwork rollback requires attention: {}",
            errors.join("; ")
        ))
    }
}

trait PathEqIgnoreAsciiCase {
    fn eq_ignore_ascii_case(&self, other: &Path) -> bool;
}

impl PathEqIgnoreAsciiCase for Path {
    fn eq_ignore_ascii_case(&self, other: &Path) -> bool {
        self.to_string_lossy()
            .eq_ignore_ascii_case(&other.to_string_lossy())
    }
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
        strict: crate::strict_source_policy::StrictContentDecision::default(),
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
    _album: &AlbumDirectory,
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
    // Candidate discovery is read-only for the album even in Write mode.
    // WRITE Cover is the explicit action that installs the WebP still.
    let destination = cache_path(cache_dir, "webp-still", path, &prepared.bytes, "jpg");
    if fs::read(&destination)
        .map(|existing| existing != prepared.bytes)
        .unwrap_or(true)
    {
        replace_binary_file(
            &destination,
            &prepared.bytes,
            "local WebP still preview",
            |staged| inspect_image(staged).map(|_| ()),
        )?;
    }
    let still = DownloadedCandidate::from_existing_path("webpstill", destination.clone(), 0, "")?;
    Ok((still, source, destination))
}

pub fn embedded_candidate(
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
    install_embedded_preview_cache(&path, &bytes)?;
    DownloadedCandidate::from_existing_path("embedded", path, 0, "").map(Some)
}

fn install_embedded_preview_cache(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let _cache_guard = EMBEDDED_PREVIEW_CACHE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if fs::read(path)
        .map(|existing| existing == bytes)
        .unwrap_or(false)
    {
        return Ok(());
    }
    replace_binary_file(path, bytes, "embedded artwork cache", |staged| {
        inspect_image(staged).map(|_| ())
    })
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
    use std::sync::Arc;
    use std::thread;
    use tempfile::tempdir;

    #[test]
    fn embedded_preview_cache_is_owned_by_the_portable_data_directory() {
        let root = PathBuf::from(r"E:\SPLINED");
        let layout = crate::portable::AppLayout::from_root(root.clone());
        assert_eq!(
            layout.config_dir.join("preview-cache"),
            root.join("data").join("preview-cache")
        );
    }

    fn write_image(path: &Path, width: u32, height: u32, format: ImageFormat) {
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(width, height, Rgb([3, 4, 5])));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, format).unwrap();
        fs::write(path, bytes.into_inner()).unwrap();
    }

    fn prepared_image(path: &Path, target: StaticFormat) -> PreparedArtwork {
        let candidate = Candidate::from_file("fixture", path, 0).unwrap();
        let range = Range {
            min: 1,
            ideal: 64,
            max: 4096,
            ladder: 8192,
        };
        prepare_configured_artwork(
            &candidate,
            path,
            &range,
            target,
            &Config::default().output,
            true,
        )
        .unwrap()
    }

    #[test]
    fn concurrent_embedded_preview_requests_share_one_valid_cache_file() {
        let temp = tempdir().unwrap();
        let path = Arc::new(temp.path().join("embedded-preview.jpg"));
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(24, 24, Rgb([7, 8, 9])));
        let mut encoded = Cursor::new(Vec::new());
        image.write_to(&mut encoded, ImageFormat::Jpeg).unwrap();
        let bytes = Arc::new(encoded.into_inner());
        let workers = (0..8)
            .map(|_| {
                let path = Arc::clone(&path);
                let bytes = Arc::clone(&bytes);
                thread::spawn(move || install_embedded_preview_cache(&path, &bytes))
            })
            .collect::<Vec<_>>();

        for worker in workers {
            worker.join().unwrap().unwrap();
        }

        assert_eq!(fs::read(path.as_ref()).unwrap(), bytes.as_ref().as_slice());
        assert!(
            !path
                .with_file_name("embedded-preview.jpg.splined-backup")
                .exists()
        );
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
    fn overwrite_mode_compares_ideal_local_cover_with_providers() {
        let temp = tempdir().unwrap();
        let album_path = temp.path().join("album");
        let cache = temp.path().join("cache");
        fs::create_dir_all(&album_path).unwrap();
        fs::create_dir_all(&cache).unwrap();
        write_image(&album_path.join("cover.jpg"), 1800, 1800, ImageFormat::Jpeg);
        let album = AlbumDirectory {
            path: album_path,
            audio_files: Vec::new(),
        };
        let mut config = Config::default();
        config.output.preserve_file = false;

        let preflight = inspect_local_preflight(&album, &config, &Range::default(), &cache);

        assert_eq!(preflight.action, LocalPreflightAction::Compare);
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
        let config = Config {
            mode: Mode::Read,
            ..Config::default()
        };
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
    fn write_mode_webp_candidate_does_not_replace_existing_cover() {
        let temp = tempdir().unwrap();
        let album_path = temp.path().join("album");
        let cache = temp.path().join("cache");
        fs::create_dir_all(&album_path).unwrap();
        fs::create_dir_all(&cache).unwrap();
        let webp = album_path.join("cover.webp");
        let still = album_path.join("cover.jpg");
        write_image(&webp, 900, 900, ImageFormat::WebP);
        write_image(&still, 600, 600, ImageFormat::Jpeg);
        let original_still = fs::read(&still).unwrap();
        let album = AlbumDirectory {
            path: album_path,
            audio_files: Vec::new(),
        };
        let preflight =
            inspect_local_preflight(&album, &Config::default(), &Range::default(), &cache);
        assert_eq!(preflight.action, LocalPreflightAction::Compare);
        assert_eq!(
            preflight.candidate.unwrap().downloaded.candidate.source,
            "webpstill"
        );
        assert_eq!(fs::read(&still).unwrap(), original_still);
        assert!(preflight.generated_destination.unwrap().starts_with(&cache));
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
    fn overwrite_transaction_replaces_all_static_covers_and_preserves_webp() {
        let temp = tempdir().unwrap();
        let album = temp.path().join("Album");
        fs::create_dir(&album).unwrap();
        write_image(&album.join("cover.jpg"), 64, 64, ImageFormat::Jpeg);
        write_image(&album.join("cover.jpeg"), 64, 64, ImageFormat::Jpeg);
        write_image(&album.join("cover-(2).png"), 64, 64, ImageFormat::Png);
        write_image(&album.join("cover.webp"), 96, 96, ImageFormat::WebP);
        write_image(&album.join("cover-back.jpg"), 64, 64, ImageFormat::Jpeg);
        write_image(&album.join("folder.jpg"), 64, 64, ImageFormat::Jpeg);
        write_image(&album.join("booklet.jpg"), 64, 64, ImageFormat::Jpeg);
        let webp = fs::read(album.join("cover.webp")).unwrap();
        let source = temp.path().join("replacement.png");
        write_image(&source, 64, 64, ImageFormat::Png);
        let prepared = prepared_image(&source, StaticFormat::Png);
        let keep = album.join("cover.png");

        let removed =
            install_prepared_canonical_artwork(&album, "cover", &keep, &prepared).unwrap();

        assert_eq!(removed.len(), 3);
        assert_eq!(fs::read(&keep).unwrap(), prepared.bytes);
        assert_eq!(fs::read(album.join("cover.webp")).unwrap(), webp);
        assert!(album.join("cover-back.jpg").exists());
        assert!(album.join("folder.jpg").exists());
        assert!(album.join("booklet.jpg").exists());
        assert!(!album.join("cover.jpg").exists());
        assert!(!album.join("cover.jpeg").exists());
        assert!(!album.join("cover-(2).png").exists());
    }

    #[test]
    fn partial_quarantine_failure_restores_every_previous_cover() {
        let temp = tempdir().unwrap();
        let album = temp.path().join("Album");
        fs::create_dir(&album).unwrap();
        for (name, format) in [
            ("cover.jpg", ImageFormat::Jpeg),
            ("cover.jpeg", ImageFormat::Jpeg),
            ("cover-(2).png", ImageFormat::Png),
            ("cover.webp", ImageFormat::WebP),
        ] {
            write_image(&album.join(name), 64, 64, format);
        }
        let before = fs::read_dir(&album)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_file())
            .map(|entry| (entry.file_name(), fs::read(entry.path()).unwrap()))
            .collect::<std::collections::HashMap<_, _>>();
        let source = temp.path().join("replacement.png");
        write_image(&source, 64, 64, ImageFormat::Png);
        let prepared = prepared_image(&source, StaticFormat::Png);

        let error = install_prepared_canonical_artwork_with(
            &album,
            "cover",
            &album.join("cover.png"),
            &prepared,
            |index, _| {
                if index == 1 {
                    Err("injected partial cleanup failure".to_string())
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();

        assert!(error.contains("injected partial cleanup failure"));
        let after = fs::read_dir(&album)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_file())
            .map(|entry| (entry.file_name(), fs::read(entry.path()).unwrap()))
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(after, before);
        assert!(!album.join("cover.png").exists());
    }

    #[test]
    fn custom_basename_transaction_never_touches_cover_or_other_artwork() {
        let temp = tempdir().unwrap();
        let album = temp.path().join("Album");
        fs::create_dir(&album).unwrap();
        write_image(&album.join("folder.jpg"), 64, 64, ImageFormat::Jpeg);
        write_image(&album.join("cover.jpg"), 64, 64, ImageFormat::Jpeg);
        write_image(&album.join("booklet.png"), 64, 64, ImageFormat::Png);
        let source = temp.path().join("replacement.png");
        write_image(&source, 64, 64, ImageFormat::Png);
        let prepared = prepared_image(&source, StaticFormat::Png);

        install_prepared_canonical_artwork(&album, "folder", &album.join("folder.png"), &prepared)
            .unwrap();

        assert!(album.join("folder.png").exists());
        assert!(!album.join("folder.jpg").exists());
        assert!(album.join("cover.jpg").exists());
        assert!(album.join("booklet.png").exists());
    }

    #[test]
    fn webp_output_commits_source_and_static_still_together() {
        let temp = tempdir().unwrap();
        let album = temp.path().join("Album");
        fs::create_dir(&album).unwrap();
        write_image(&album.join("cover.jpg"), 64, 64, ImageFormat::Jpeg);
        write_image(&album.join("cover.webp"), 64, 64, ImageFormat::WebP);
        let source = temp.path().join("replacement.webp");
        write_image(&source, 96, 80, ImageFormat::WebP);
        let prepared = prepared_image(&source, StaticFormat::Webp);

        install_prepared_canonical_artwork(&album, "cover", &album.join("cover.webp"), &prepared)
            .unwrap();

        assert_eq!(fs::read(album.join("cover.webp")).unwrap(), prepared.bytes);
        let still = inspect_image(&album.join("cover.jpg")).unwrap();
        assert_eq!(still.format, StaticFormat::Jpeg);
        assert_eq!(
            (still.width, still.height),
            (prepared.info.width, prepared.info.height)
        );
        assert_eq!(
            fs::read_dir(&album)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| matching_static_canonical(&entry.path(), "cover"))
                .count(),
            1
        );
    }

    #[test]
    fn transaction_rejects_destination_outside_selected_album() {
        let temp = tempdir().unwrap();
        let album = temp.path().join("Album");
        let outside = temp.path().join("Other");
        fs::create_dir(&album).unwrap();
        fs::create_dir(&outside).unwrap();
        write_image(&album.join("cover.jpg"), 64, 64, ImageFormat::Jpeg);
        let source = temp.path().join("replacement.png");
        write_image(&source, 64, 64, ImageFormat::Png);
        let prepared = prepared_image(&source, StaticFormat::Png);

        assert!(
            install_prepared_canonical_artwork(
                &album,
                "cover",
                &outside.join("cover.png"),
                &prepared,
            )
            .is_err()
        );
        assert!(album.join("cover.jpg").exists());
        assert!(!outside.join("cover.png").exists());
    }
}
