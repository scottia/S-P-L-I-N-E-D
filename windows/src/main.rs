#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use futures_util::{StreamExt, stream};
use image::AnimationDecoder;
use sha2::{Digest, Sha256};
use slint::{
    Color, ComponentHandle, Model, ModelRc, ModelTracker, SharedString, Timer, TimerMode, VecModel,
};
use splined_core::application::{
    AdjustmentState, AlbumEntry, AlbumState, ApplicationState, ArtistOverviewCacheEntry,
    CandidateState, LaunchMode, LibraryRow, LibraryView, MusicBrainzResult, RunPhase, ScanScope,
    SelectionResult, StatusFilter, musicbrainz_artist_album_preferred, musicbrainz_match_section,
};
use splined_core::backup::{
    BackupSelection, export_backup, export_database_backup_with_artist_art,
    export_diagnostic_summary, read_backup_settings, restore_backup,
    restore_database_backup_with_artist_art,
};
use splined_core::candidate::{Candidate, StaticFormat};
use splined_core::config::{
    Config, Mode, ScanTimeout, Verbosity, configured_samples_dir, configured_shared_artist_art_dir,
};
use splined_core::credential_store::{CredentialEditor, load_editor, save_editor, test_editor};
use splined_core::download::DownloadCache;
use splined_core::embed_queue;
use splined_core::embedded_artwork::{
    embedded_front_matches, embedded_front_metadata, replace_embedded_front,
    replace_embedded_front_with_trace,
};
use splined_core::final_artwork::{
    apply_selected_image_adjustments, install_prepared_artwork, prepare_configured_artwork,
    prepare_embedded_thumbnail, prepare_existing_cover_edit, prepare_selected_artwork_edit,
};
use splined_core::inspect::inspect_image;
use splined_core::local_artwork::embedded_candidate;
use splined_core::media_database::{
    MediaAlbumSnapshot, MediaSnapshot, MediaTrackSnapshot, RuntimeArtworkMaterial,
    clear_album_bypass_from_runtime, fallback_tracks_from_folder, find_unindexed_audio_folders,
    media_snapshot, record_album_outcome, record_album_outcome_from_runtime,
};
use splined_core::range::Range;
use splined_core::safe_write::{
    cleanup_stale_audio_stages, replace_binary_file, take_credential_protection_warning,
};
use splined_core::scan_controller::{ControllerNotice, ProcessingDebugSink, ScanController};
use splined_core::scan_tags::read_local_track_evidence;
use splined_core::settings::{
    ArtistLookupOverride, BannerThemeStyle, MEDIA_PANE_MIN, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH,
    UiPreferences, WorkspaceBounds, WorkspaceLayout, normalize_appearance,
};
use splined_core::source::navidrome::{
    NavidromeAlbum, NavidromeArtist, NavidromeCompanion, NavidromeCredential, match_companion_album,
};
use splined_core::source::{
    ArtworkReference, deezer::Deezer, discogs::Discogs, fanarttv::FanartTv, itunes::ITunes,
    lastfm::LastFm,
};
use splined_core::source_policy::{MinimumRangeType, SourcePolicyConfig};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

slint::include_modules!();

static FULL_THUMBNAIL_REFRESH_ACTIVE: AtomicBool = AtomicBool::new(false);
static FOCUSED_THUMBNAIL_REFRESH_REVISION: AtomicU64 = AtomicU64::new(0);
static PREVIEW_RENDER_REVISION: AtomicU64 = AtomicU64::new(0);
static NEW_MEDIA_CHECK_REVISION: AtomicU64 = AtomicU64::new(0);
thread_local! {
    static EMBED_QUEUE_WINDOW: RefCell<Option<slint::Weak<EmbedQueueWindow>>> = const { RefCell::new(None) };
    static EMBED_MAIN_WINDOW: RefCell<Option<slint::Weak<AppWindow>>> = const { RefCell::new(None) };
}

struct DecodedWebpFrame {
    pixels: image::RgbaImage,
    delay: Duration,
}

struct WebpPlayback {
    frames: Vec<(slint::Image, Duration)>,
    next_frame: usize,
    next_at: Instant,
    started_at: Instant,
}

#[cfg(test)]
fn decode_webp_animation(path: &Path) -> Result<Vec<DecodedWebpFrame>, String> {
    decode_webp_animation_while(path, || true)
}

fn decode_webp_animation_while(
    path: &Path,
    still_selected: impl Fn() -> bool,
) -> Result<Vec<DecodedWebpFrame>, String> {
    if !still_selected() {
        return Ok(Vec::new());
    }
    let size = fs::metadata(path)
        .map_err(|error| format!("Unable to inspect animated WebP: {error}"))?
        .len();
    if size > 100_000_000 {
        return Err("Animated WebP exceeds the 100 MB preview limit.".to_string());
    }
    let bytes = fs::read(path).map_err(|error| format!("Unable to open animated WebP: {error}"))?;
    if !still_selected() {
        return Ok(Vec::new());
    }
    let Some(metadata) = webp_animation_metadata(&bytes) else {
        return Ok(Vec::new());
    };
    if !metadata.suits_preview() {
        return Ok(Vec::new());
    }
    // Keep the decoded frames small, but try to cover five seconds of the
    // source instead of stopping after an arbitrary eight-frame opening.
    // The playback timer repeats a shorter segment only when these bounds
    // prevent decoding the full five seconds.
    let preview_frame_limit = metadata.frame_count.min(160);
    let decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes))
        .map_err(|error| format!("Unable to decode animated WebP: {error}"))?;
    if !decoder.has_animation() {
        return Ok(Vec::new());
    }
    let started = Instant::now();
    let mut total_pixels = 0_u64;
    let mut preview_duration = Duration::ZERO;
    let mut frames: Vec<DecodedWebpFrame> = Vec::new();
    for frame in decoder.into_frames().take(preview_frame_limit) {
        if !still_selected() {
            return Ok(Vec::new());
        }
        let frame = frame.map_err(|error| format!("Invalid animated WebP frame: {error}"))?;
        let (numerator, denominator) = frame.delay().numer_denom_ms();
        let milliseconds = (u64::from(numerator) / u64::from(denominator.max(1))).clamp(40, 5000);
        let delay = Duration::from_millis(milliseconds);
        let pixels = frame.into_buffer();
        let long_side = pixels.width().max(pixels.height());
        let pixels = if long_side > 480 {
            let width = (u64::from(pixels.width()) * 480 / u64::from(long_side)).max(1) as u32;
            let height = (u64::from(pixels.height()) * 480 / u64::from(long_side)).max(1) as u32;
            image::imageops::resize(
                &pixels,
                width,
                height,
                image::imageops::FilterType::Triangle,
            )
        } else {
            pixels
        };
        let frame_pixels = u64::from(pixels.width()) * u64::from(pixels.height());
        if total_pixels + frame_pixels > 40_000_000 {
            break;
        }
        total_pixels += frame_pixels;
        preview_duration += delay;
        frames.push(DecodedWebpFrame { pixels, delay });
        if preview_duration >= Duration::from_secs(5) {
            break;
        }
        if started.elapsed() > Duration::from_secs(5) {
            break;
        }
    }
    Ok(frames)
}

#[derive(Debug, Default)]
struct WebpAnimationMetadata {
    frame_count: usize,
    width: u32,
    height: u32,
    duration_ms: u64,
    shortest_frame_ms: u32,
}

impl WebpAnimationMetadata {
    fn suits_preview(&self) -> bool {
        self.frame_count > 1
            && self.width.max(self.height) <= 1600
            && self.width > 0
            && self.height > 0
            && self.duration_ms <= 30_000
            && self.shortest_frame_ms >= 17
    }
}

fn webp_u24(bytes: &[u8]) -> Option<u32> {
    Some(
        u32::from(*bytes.first()?)
            | (u32::from(*bytes.get(1)?) << 8)
            | (u32::from(*bytes.get(2)?) << 16),
    )
}

fn webp_animation_metadata(bytes: &[u8]) -> Option<WebpAnimationMetadata> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return None;
    }
    let mut offset = 12usize;
    let mut metadata = WebpAnimationMetadata::default();
    while offset + 8 <= bytes.len() {
        let chunk_size =
            u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().ok()?) as usize;
        let payload = offset.checked_add(8)?;
        let end = payload.checked_add(chunk_size)?;
        let chunk = bytes.get(payload..end)?;
        match &bytes[offset..offset + 4] {
            b"VP8X" if chunk.len() >= 10 => {
                metadata.width = webp_u24(&chunk[4..7])? + 1;
                metadata.height = webp_u24(&chunk[7..10])? + 1;
            }
            b"ANMF" if chunk.len() >= 16 => {
                metadata.frame_count += 1;
                let duration = webp_u24(&chunk[12..15])?;
                metadata.duration_ms = metadata.duration_ms.saturating_add(u64::from(duration));
                metadata.shortest_frame_ms = if metadata.frame_count == 1 {
                    duration
                } else {
                    metadata.shortest_frame_ms.min(duration)
                };
            }
            _ => {}
        }
        let next = end.checked_add(chunk_size % 2)?;
        if next > bytes.len() {
            return None;
        }
        offset = next;
    }
    Some(metadata)
}
type EmbeddedMetadataCache = HashMap<String, (u64, u128, Option<(u32, u32, u64)>)>;
static EMBEDDED_METADATA_CACHE: OnceLock<Mutex<EmbeddedMetadataCache>> = OnceLock::new();
static VISIBLE_COMPILATION_CHECKS_PENDING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static COMP_REPORT_THUMB_CACHE: OnceLock<Mutex<HashMap<String, CachedCompReportThumb>>> =
    OnceLock::new();

#[derive(Clone)]
struct CachedCompReportThumb {
    audio_len: u64,
    audio_modified: Option<SystemTime>,
    image_path: Option<PathBuf>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct AlbumEmbeddedCheck {
    tracks: usize,
    missing: usize,
    wrong_size: usize,
}

impl AlbumEmbeddedCheck {
    fn needs_attention(self) -> bool {
        self.missing + self.wrong_size > 0
    }
}

// None means a background metadata check is pending. Never open audio tags
// while projecting cards on the UI thread.
static ALBUM_EMBEDDED_CHECKS: OnceLock<Mutex<HashMap<String, Option<AlbumEmbeddedCheck>>>> =
    OnceLock::new();

#[derive(Clone, Debug)]
struct ArtistAlbumEmbedTarget {
    title: String,
    album_path: PathBuf,
    cover_path: PathBuf,
    track_count: usize,
}

#[derive(Clone, Debug)]
struct ArtistAlbumEmbedPlan {
    artist: String,
    selection_epoch: u64,
    side: u32,
    targets: Vec<ArtistAlbumEmbedTarget>,
    skipped: usize,
}

#[derive(Clone, Debug)]
struct CompilationThumbRepairPlan {
    album_key: String,
    album_path: PathBuf,
    album_paths: Vec<PathBuf>,
    artist_wide: bool,
    artist: String,
    selection_epoch: u64,
    side: u32,
    cache_dir: PathBuf,
    targets: Vec<PathBuf>,
    missing: usize,
}

#[derive(Clone, Debug)]
struct ArtistCompReportTrack {
    album_key: String,
    album_title: String,
    album_path: PathBuf,
    path: PathBuf,
    completed: bool,
}
type NavidromeArtistCatalogCell = Arc<tokio::sync::OnceCell<Arc<Vec<NavidromeArtist>>>>;
type NavidromeAlbumCatalogCell = Arc<tokio::sync::OnceCell<Arc<Vec<NavidromeAlbum>>>>;

static NAVIDROME_ARTIST_CATALOG: OnceLock<Mutex<HashMap<String, NavidromeArtistCatalogCell>>> =
    OnceLock::new();
static NAVIDROME_ALBUM_CATALOG: OnceLock<Mutex<HashMap<String, NavidromeAlbumCatalogCell>>> =
    OnceLock::new();
const PREVIEW_CACHE_DIMENSION: u32 = 1600;
const LARGE_RAW_IMAGE_CACHE_LIMIT: usize = 6;
const COMPILATION_GATHERING_STATUS: &str =
    "Gathering compilation albums and artists track information…";

#[derive(Clone, Debug, PartialEq, Eq)]
struct PreviewContext {
    album_key: Option<String>,
    album_path: String,
    cover_path: String,
    candidate: Option<(usize, String)>,
    musicbrainz_result: Option<usize>,
    musicbrainz_cover_path: String,
    musicbrainz_existing_path: String,
    compilation_track_preview: Option<(String, String)>,
    artist_preview: Option<(String, String)>,
}

fn preview_context(app: &ApplicationState) -> PreviewContext {
    PreviewContext {
        album_key: app.library.focused_album_key.clone(),
        album_path: app.current_album_path.clone(),
        cover_path: app.current_cover_path.clone(),
        candidate: app
            .selected_candidate()
            .map(|candidate| (candidate.index, candidate.cache_path.clone())),
        musicbrainz_result: app.selected_musicbrainz_result,
        musicbrainz_cover_path: app.musicbrainz_cover_preview_path.clone(),
        musicbrainz_existing_path: app.musicbrainz_existing_artwork_path.clone(),
        compilation_track_preview: app.active_compilation_track_preview().map(|path| {
            (
                app.compilation_track_preview_owner.clone(),
                path.to_string(),
            )
        }),
        artist_preview: app.artist_artwork_mode.then(|| {
            if app.artist_preview_focus == "banner" {
                (
                    app.artist_preview_focus.clone(),
                    app.artist_banner_source_path.clone(),
                )
            } else {
                (
                    app.artist_preview_focus.clone(),
                    app.artist_artwork_draft_path.clone(),
                )
            }
        }),
    }
}

fn embedded_thumbnail_owner_key(app: &ApplicationState) -> String {
    format!(
        "{:?}|{:?}|{}|{}",
        app.phase,
        preview_context(app),
        app.current_compilation_track_path,
        app.edit_preview_path
    )
}

fn embedded_thumbnail_ready(app: &ApplicationState) -> bool {
    !app.embedded_thumbnail_preview_path.is_empty()
        && app.embedded_thumbnail_owner_key == embedded_thumbnail_owner_key(app)
}

fn disable_embedded_thumbnail_preview(app: &mut ApplicationState) -> bool {
    if !embedded_thumbnail_ready(app) {
        return false;
    }
    app.embedded_thumbnail_preview_path.clear();
    app.embedded_thumbnail_owner_key.clear();
    true
}

fn add_embedded_thumbnail_decision(app: &ApplicationState, decision: &mut serde_json::Value) {
    let thumbnail_ready = embedded_thumbnail_ready(app);
    if thumbnail_ready && compilation_artwork_context(app) {
        decision["embed_thumbnails"] = serde_json::json!(true);
    }
    if app.edit_preview_path.starts_with("memory-preview://")
        && (thumbnail_ready || !app.current_compilation_track_path.trim().is_empty())
    {
        // A COMP write always derives a preference-sized embed, even when the
        // operator skips the optional thumbnail preview. Carry the visible
        // full-resolution Upscale Preview profile into that write.
        decision["preserve_source_resolution"] = serde_json::json!(false);
        decision["apply_edit_profile"] = serde_json::json!(true);
        decision["use_upscaled_preview"] = serde_json::json!(true);
    }
}

fn preview_request_is_current(guard: Option<(&AtomicU64, u64)>) -> bool {
    guard.is_none_or(|(revision, ticket)| revision.load(Ordering::SeqCst) == ticket)
}

#[derive(Clone)]
struct UiDebugLog {
    file: Arc<Mutex<std::fs::File>>,
    path: Arc<PathBuf>,
}

static UI_DEBUG_LOG_HANDLE: OnceLock<UiDebugLog> = OnceLock::new();
static UI_REFRESH_COUNT: AtomicU64 = AtomicU64::new(0);
static IMAGE_MEMORY_HITS: AtomicU64 = AtomicU64::new(0);
static IMAGE_FILE_DECODES: AtomicU64 = AtomicU64::new(0);
static IMAGE_STALE_INVALIDATIONS: AtomicU64 = AtomicU64::new(0);
static IMAGE_PRELOAD_HITS: AtomicU64 = AtomicU64::new(0);
static STARTUP_TIMING_EVENTS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

fn record_startup_timing(event: String) {
    if let Ok(mut events) = STARTUP_TIMING_EVENTS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
    {
        events.push(event);
    }
}

impl UiDebugLog {
    fn record(&self, event: impl AsRef<str>) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let event = event
            .as_ref()
            .replace(['\r', '\n'], " ")
            .chars()
            .take(2048)
            .collect::<String>();
        if let Ok(mut file) = self.file.lock() {
            let _ = writeln!(file, "{timestamp} {event}");
            let _ = file.flush();
        }
    }

    fn display_path(&self) -> String {
        self.path.display().to_string()
    }
}

fn initialize_ui_debug_log(config: &Config) -> Option<UiDebugLog> {
    if !matches!(config.verbosity, Verbosity::Debug | Verbosity::Trace) {
        return None;
    }
    let run_dir = Path::new(&config.scan.log_dir).join("run");
    if let Err(error) = fs::create_dir_all(&run_dir) {
        eprintln!("Unable to create SPLINED UI debug directory: {error}");
        return None;
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let machine = safe_machine_name(&std::env::var("COMPUTERNAME").unwrap_or_default());
    let path = run_dir.join(ui_debug_log_filename(
        &machine,
        timestamp,
        std::process::id(),
    ));
    let file = match OpenOptions::new().create_new(true).append(true).open(&path) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("Unable to create SPLINED UI debug log: {error}");
            return None;
        }
    };
    let log = UiDebugLog {
        file: Arc::new(Mutex::new(file)),
        path: Arc::new(path),
    };
    log.record(format!(
        "ui-debug-start version={} machine={} pid={} temp_dir={} exe={} log_dir={}",
        env!("CARGO_PKG_VERSION"),
        machine,
        std::process::id(),
        std::env::temp_dir().display(),
        std::env::current_exe()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        config.scan.log_dir
    ));
    let _ = UI_DEBUG_LOG_HANDLE.set(log.clone());
    Some(log)
}

fn prune_old_ui_debug_logs(config: &Config) -> Result<usize, String> {
    let days = config.logging.retention_days;
    if days == 0 {
        return Ok(0);
    }
    let directory = Path::new(&config.scan.log_dir).join("run");
    if !directory.is_dir() {
        return Ok(0);
    }
    let cutoff = SystemTime::now() - Duration::from_secs(u64::from(days) * 86_400);
    let resolved_directory = directory
        .canonicalize()
        .map_err(|error| format!("Unable to resolve log directory: {error}"))?;
    let machine = safe_machine_name(&std::env::var("COMPUTERNAME").unwrap_or_default());
    let mut removed = 0;
    for entry in
        fs::read_dir(&directory).map_err(|error| format!("Unable to list UI logs: {error}"))?
    {
        let entry = entry.map_err(|error| format!("Unable to inspect UI log: {error}"))?;
        if !entry.file_type().is_ok_and(|kind| kind.is_file())
            || !entry
                .file_name()
                .to_str()
                .is_some_and(|name| is_machine_ui_debug_log_filename(name, &machine))
            || !entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .is_ok_and(|modified| modified < cutoff)
        {
            continue;
        }
        let target = entry
            .path()
            .canonicalize()
            .map_err(|error| format!("Unable to resolve UI log: {error}"))?;
        if target.parent() != Some(resolved_directory.as_path()) {
            return Err(format!(
                "Refusing to prune a UI log outside {}",
                resolved_directory.display()
            ));
        }
        fs::remove_file(&target)
            .map_err(|error| format!("Unable to prune UI log {}: {error}", target.display()))?;
        removed += 1;
    }
    Ok(removed)
}

fn recent_debug_lines(path: &Path, limit: usize) -> Result<Vec<String>, String> {
    let mut file =
        fs::File::open(path).map_err(|error| format!("Unable to open diagnostic log: {error}"))?;
    let length = file
        .metadata()
        .map_err(|error| format!("Unable to inspect diagnostic log: {error}"))?
        .len();
    let start = length.saturating_sub(128 * 1024);
    file.seek(SeekFrom::Start(start))
        .map_err(|error| format!("Unable to seek diagnostic log: {error}"))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| format!("Unable to read diagnostic log: {error}"))?;
    let text = String::from_utf8_lossy(&bytes);
    let text = if start > 0 {
        text.split_once('\n').map(|(_, rest)| rest).unwrap_or("")
    } else {
        &text
    };
    Ok(text
        .lines()
        .rev()
        .take(limit)
        .map(str::to_string)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect())
}

fn diagnostic_log_view(state: &ApplicationState, level: &str, backup_directory: &Path) -> String {
    if level == "Error" {
        let errors = state
            .log
            .iter()
            .filter(|line| {
                let lower = line.to_ascii_lowercase();
                lower.contains("error") || lower.contains("failed") || lower.contains("warning")
            })
            .rev()
            .take(100)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>();
        let mut result = if errors.is_empty() {
            "No errors in the current session.".to_string()
        } else {
            errors.join("\n")
        };
        let latest = fs::read_dir(backup_directory)
            .ok()
            .into_iter()
            .flat_map(|entries| entries.filter_map(Result::ok))
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .filter(|entry| {
                entry.file_name().to_str().is_some_and(|name| {
                    name.starts_with("splined-diagnostics-") && name.ends_with(".json")
                })
            })
            .max_by_key(|entry| {
                entry
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .unwrap_or(UNIX_EPOCH)
            });
        result.push_str("\n\nDiagnostic Summary: ");
        if let Some(entry) = latest {
            result.push_str(&entry.path().display().to_string());
            if let Ok(contents) = fs::read_to_string(entry.path()) {
                result.push('\n');
                result.push_str(&contents.chars().take(60_000).collect::<String>());
            }
        } else {
            result.push_str("none yet. Generate one in Backup / Restore → Advanced → Diagnostic Summary when reporting a problem.");
        }
        return result;
    }
    let Some(log) = UI_DEBUG_LOG_HANDLE.get() else {
        return "Save this log level and restart SPLINED to begin file diagnostics. Current session messages remain available under Current.".to_string();
    };
    let lines = match recent_debug_lines(&log.path, 220) {
        Ok(lines) => lines,
        Err(error) => return error,
    };
    let lines = if level == "Verbose" {
        lines
            .into_iter()
            .filter(|line| {
                line.contains("processing-") || line.contains("scan-") || line.contains("backup-")
            })
            .collect::<Vec<_>>()
    } else {
        lines
    };
    if lines.is_empty() {
        "No matching diagnostic events in this session yet.".to_string()
    } else {
        lines.join("\n")
    }
}

#[derive(Clone)]
struct ThumbnailAlbumTarget {
    album_key: String,
    artist: String,
    artist_mbid: String,
    title: String,
    path: String,
    album_mbid: String,
    release_year: String,
    cover_path: String,
}

#[derive(Clone, Debug, Default)]
struct SavedArtistPackage {
    portrait: Option<PathBuf>,
    banner: Option<PathBuf>,
}

fn primary_credit_biography_artist<'a>(
    albums: &[ThumbnailAlbumTarget],
    focused_artist: &str,
    navidrome_artists: &'a [NavidromeArtist],
) -> Option<&'a NavidromeArtist> {
    let mut primary_mbid: Option<&str> = None;
    for album in albums.iter().filter(|album| {
        album
            .artist
            .trim()
            .eq_ignore_ascii_case(focused_artist.trim())
    }) {
        let ids = album
            .artist_mbid
            .split(|character: char| {
                character.is_whitespace() || matches!(character, ',' | ';' | '/' | '\\' | '|')
            })
            .filter(|part| is_valid_mbid(part))
            .collect::<Vec<_>>();
        if ids.len() < 2 {
            continue;
        }
        if primary_mbid.is_some_and(|previous| !previous.eq_ignore_ascii_case(ids[0])) {
            // A combined artist with conflicting first credits has no safe
            // single biography fallback.
            return None;
        }
        primary_mbid = Some(ids[0]);
    }
    let primary_mbid = primary_mbid?;
    navidrome_artists.iter().find(|artist| {
        !artist.id.trim().is_empty()
            && artist.musicbrainz_id.eq_ignore_ascii_case(primary_mbid)
            && !artist
                .name
                .trim()
                .eq_ignore_ascii_case(focused_artist.trim())
    })
}

fn navidrome_artist_for_lookup<'a>(
    artists: &'a [NavidromeArtist],
    requested: &str,
    override_hint: Option<&ArtistLookupOverride>,
) -> Option<&'a NavidromeArtist> {
    let exact = override_hint
        .and_then(|hint| {
            artists
                .iter()
                .find(|candidate| {
                    !hint.musicbrainz_id.trim().is_empty()
                        && candidate
                            .musicbrainz_id
                            .eq_ignore_ascii_case(hint.musicbrainz_id.trim())
                })
                .or_else(|| {
                    artists.iter().find(|candidate| {
                        !hint.lookup_name.trim().is_empty()
                            && candidate
                                .name
                                .trim()
                                .eq_ignore_ascii_case(hint.lookup_name.trim())
                    })
                })
        })
        .or_else(|| {
            artists
                .iter()
                .find(|candidate| candidate.name.trim().eq_ignore_ascii_case(requested.trim()))
        });
    if exact.is_some() {
        return exact;
    }
    // A leading period can be lost when an artist folder is indexed on
    // Windows. Only accept this fallback if it identifies one remote artist.
    let alias = requested.trim().trim_start_matches('.').trim();
    if alias.is_empty() {
        return None;
    }
    let mut matches = artists.iter().filter(|candidate| {
        candidate
            .name
            .trim()
            .trim_start_matches('.')
            .trim()
            .eq_ignore_ascii_case(alias)
    });
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

type ThumbnailPaths = Vec<(String, PathBuf)>;
type ArtistNavidromeIds = Vec<(String, String)>;
#[derive(Clone, Debug, Default)]
struct ArtistOverviewMetadata {
    artist_key: String,
    biography: String,
    external_url: String,
    musicbrainz_id: String,
    banner_path: String,
    banner_uses_artist_art: bool,
    similar_artists: Vec<(String, String)>,
    genres: Vec<String>,
    roles: Vec<String>,
    album_keys: BTreeSet<String>,
    featured_album_keys: BTreeSet<String>,
    track_paths_by_album: HashMap<String, Vec<String>>,
    album_ratings: HashMap<String, u32>,
    artwork_candidates: Vec<(String, String)>,
    fallback_tracks: Vec<(String, Vec<MediaTrackSnapshot>)>,
}
type ThumbnailRefreshResult = (
    ThumbnailPaths,
    ThumbnailPaths,
    ArtistNavidromeIds,
    Option<ArtistOverviewMetadata>,
);

#[derive(Clone)]
struct CustomWindowState {
    layout: Rc<RefCell<WorkspaceLayout>>,
    media_expanded: Rc<RefCell<bool>>,
    media_filter_expanded: Rc<RefCell<bool>>,
}

#[derive(Clone)]
struct UiWindowDiagnostics {
    maximize_guard: MaximizeGuard,
    ui_debug_log: Option<UiDebugLog>,
}

impl CustomWindowState {
    fn from_preferences(preferences: &UiPreferences) -> Self {
        let mut layout = WorkspaceLayout::from_preferences(preferences);
        layout.media_pane_width = preferences.custom_media_pane_width;
        layout.media_pane_expanded_width = preferences.custom_media_pane_expanded_width;
        layout.normal_window_width = preferences.custom_window_width;
        layout.normal_window_height = preferences.custom_window_height;
        layout.maximized = preferences.custom_window_maximized;
        Self {
            layout: Rc::new(RefCell::new(layout)),
            media_expanded: Rc::new(RefCell::new(preferences.custom_media_selector_expanded)),
            media_filter_expanded: Rc::new(RefCell::new(preferences.custom_media_filter_expanded)),
        }
    }
}

#[derive(Clone)]
struct MaximizeGuard {
    expected: Rc<Cell<bool>>,
    restore_until: Rc<Cell<Option<Instant>>>,
}

impl MaximizeGuard {
    fn new(expected: bool) -> Self {
        Self {
            expected: Rc::new(Cell::new(expected)),
            restore_until: Rc::new(Cell::new(None)),
        }
    }

    fn arm(&self, currently_maximized: bool, _width: u32, _height: u32) -> bool {
        let keep_maximized = currently_maximized || self.expected.get();
        if keep_maximized {
            self.expected.set(true);
            self.restore_until
                .set(Some(Instant::now() + Duration::from_secs(2)));
        }
        keep_maximized
    }

    fn maintain(&self, currently_maximized: bool, _width: u32, _height: u32) -> bool {
        if self
            .restore_until
            .get()
            .is_some_and(|deadline| Instant::now() <= deadline)
            && self.expected.get()
        {
            return true;
        }
        self.restore_until.set(None);
        self.expected.set(currently_maximized);
        false
    }
}

struct LibraryUiModel {
    rows: Vec<LibraryRow>,
    preview_size: u32,
    embed_locked_keys: HashSet<String>,
}

impl Model for LibraryUiModel {
    type Data = UiLibraryRow;

    fn row_count(&self) -> usize {
        self.rows.len()
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        let row = self.rows.get(row)?;
        let icon = match row.kind.as_str() {
            "artist" => "◉",
            "folder" => "▰",
            _ => "▣",
        };
        Some(UiLibraryRow {
            key: SharedString::from(row.key.clone()),
            primary: SharedString::from(row.primary.clone()),
            secondary: SharedString::from(row.secondary.clone()),
            status_color: library_status_color(&row.status),
            artwork: load_scaled_image(&row.artwork_path, self.preview_size),
            status: SharedString::from(row.status.clone()),
            icon: SharedString::from(icon),
            kind: SharedString::from(row.kind.clone()),
            depth: row.depth as i32,
            expanded: row.expanded,
            selected: row.selected && !self.embed_locked_keys.contains(&row.key.to_lowercase()),
            eligible: row.eligible,
            queue_locked: self.embed_locked_keys.contains(&row.key.to_lowercase()),
        })
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &()
    }
}

fn publish_thumbnail_paths(
    state: &Arc<Mutex<ApplicationState>>,
    weak: &slint::Weak<AppWindow>,
    artist_paths: ThumbnailPaths,
    album_paths: ThumbnailPaths,
    artist_ids: ArtistNavidromeIds,
    artist_metadata: Option<ArtistOverviewMetadata>,
) {
    if artist_paths.is_empty()
        && album_paths.is_empty()
        && artist_ids.is_empty()
        && artist_metadata.is_none()
    {
        return;
    }
    if let Ok(mut app) = state.lock() {
        for (artist, path) in artist_paths {
            app.library.set_artist_artwork_path(&artist, &path);
        }
        for (album_key, path) in album_paths {
            let focused_idle_album = app.phase == RunPhase::Idle
                && app
                    .library
                    .focused_album_key
                    .as_ref()
                    .is_some_and(|focused| focused.eq_ignore_ascii_case(&album_key));
            if let Some(album) = app.library.albums.iter_mut().find(|album| {
                album.snapshot.album_key.eq_ignore_ascii_case(&album_key)
                    && !album.snapshot.album_mbid.trim().is_empty()
                    && path.parent() == Some(Path::new(&album.snapshot.path))
            }) {
                let previous = std::mem::replace(
                    &mut album.snapshot.cover_path,
                    path.to_string_lossy().into_owned(),
                );
                if focused_idle_album && app.current_cover_path == previous {
                    app.current_cover_path = path.to_string_lossy().into_owned();
                }
            }
            app.library.set_album_artwork_path(&album_key, &path);
        }
        for (artist, id) in artist_ids {
            app.library.set_artist_navidrome_id(&artist, &id);
        }
        if let Some(metadata) = artist_metadata {
            if let Some(log) = UI_DEBUG_LOG_HANDLE.get() {
                let hydrated_tracks = metadata
                    .fallback_tracks
                    .iter()
                    .map(|(_, tracks)| tracks.len())
                    .sum::<usize>();
                let artist_track_paths = metadata
                    .track_paths_by_album
                    .values()
                    .map(Vec::len)
                    .sum::<usize>();
                log.record(format!(
                    "artist-overview-hydrated artist={} appearance_albums={} fallback_albums={} fallback_tracks={} artist_track_paths={}",
                    metadata.artist_key,
                    metadata.album_keys.len(),
                    metadata.fallback_tracks.len(),
                    hydrated_tracks,
                    artist_track_paths,
                ));
            }
            for (album_key, tracks) in &metadata.fallback_tracks {
                replace_fallback_tracks_and_invalidate(&mut app, album_key, tracks.clone());
            }
            // Artist appearances can arrive independently of the local track
            // inventory. Their paths change the scoped COMP thumbnail check.
            for album_key in metadata.track_paths_by_album.keys() {
                if let Some(album) = app.library.albums.iter().find(|album| {
                    album.snapshot.album_key.eq_ignore_ascii_case(album_key)
                        && album.snapshot.album_mbid.trim().is_empty()
                }) {
                    invalidate_album_embedded_check(Path::new(&album.snapshot.path));
                }
            }
            let top_songs = app
                .artist_top_songs_cache
                .get(&metadata.artist_key)
                .and_then(|songs| songs.clone())
                .unwrap_or_default();
            let cached = ArtistOverviewCacheEntry {
                biography: metadata.biography,
                external_url: metadata.external_url,
                musicbrainz_id: metadata.musicbrainz_id,
                banner_path: metadata.banner_path,
                banner_uses_artist_art: metadata.banner_uses_artist_art,
                similar_artists: metadata.similar_artists,
                genres: metadata.genres,
                roles: metadata.roles,
                album_keys: metadata.album_keys,
                featured_album_keys: metadata.featured_album_keys,
                track_paths_by_album: metadata.track_paths_by_album,
                album_ratings: metadata.album_ratings,
                artwork_candidates: metadata.artwork_candidates,
                top_songs: top_songs.clone(),
            };
            let current_overview_artist = overview_artist_name(&app)
                .is_some_and(|artist| artist.trim().eq_ignore_ascii_case(&metadata.artist_key));
            if current_overview_artist {
                app.artist_overview_biography = cached.biography.clone();
                app.artist_overview_external_url = cached.external_url.clone();
                app.artist_overview_musicbrainz_id = cached.musicbrainz_id.clone();
                app.artist_overview_banner_path = cached.banner_path.clone();
                app.artist_overview_similar_artists = cached.similar_artists.clone();
                app.artist_overview_genres = cached.genres.clone();
                app.artist_overview_roles = cached.roles.clone();
                app.artist_overview_album_keys = cached.album_keys.clone();
                app.artist_overview_featured_album_keys = cached.featured_album_keys.clone();
                app.artist_overview_track_paths = cached.track_paths_by_album.clone();
                app.artist_overview_album_ratings = cached.album_ratings.clone();
                app.artist_overview_artwork_candidates = cached.artwork_candidates.clone();
                app.artist_overview_top_songs = top_songs;
                if app.library.view == LibraryView::Artists && !app.library.show_tracks {
                    app.select_artist_appearances_for_run();
                }
            }
            app.artist_overview_cache
                .insert(metadata.artist_key, cached);
        }
    }
    refresh_weak(weak, state);
}

fn logical_dimension(value: f32, minimum: u32) -> u32 {
    if value.is_finite() {
        (value.max(minimum as f32)).round() as u32
    } else {
        minimum
    }
}

fn app_logical_window_size(ui: &AppWindow) -> (u32, u32) {
    (
        logical_dimension(ui.get_logical_window_width(), MIN_WINDOW_WIDTH),
        logical_dimension(ui.get_logical_window_height(), MIN_WINDOW_HEIGHT),
    )
}

fn repair_main_maximized_rect(ui: &AppWindow, guard: &MaximizeGuard) -> bool {
    let (width, height) = app_logical_window_size(ui);
    let maximized = ui.window().is_maximized();
    if !guard.maintain(maximized, width, height) {
        return false;
    }

    if !maximized {
        ui.window().set_maximized(true);
    }
    true
}

fn schedule_main_maximize_repairs(weak: slint::Weak<AppWindow>, guard: MaximizeGuard) {
    for delay in [80, 300, 800] {
        let weak = weak.clone();
        let guard = guard.clone();
        Timer::single_shot(Duration::from_millis(delay), move || {
            if let Some(ui) = weak.upgrade() {
                repair_main_maximized_rect(&ui, &guard);
            }
        });
    }
}

fn capture_ui_preferences(
    ui: &AppWindow,
    state: &Arc<Mutex<ApplicationState>>,
    workspace_layout: &Rc<RefCell<WorkspaceLayout>>,
    appearance: &Rc<RefCell<String>>,
    custom_window: &CustomWindowState,
) -> Option<UiPreferences> {
    let state = state.lock().ok()?;
    let layout = *workspace_layout.borrow();
    let custom_layout = *custom_window.layout.borrow();
    let locked_filter_values = |values: &std::collections::BTreeSet<String>| {
        if state.candidate_filter_locked {
            values.iter().cloned().collect()
        } else {
            Vec::new()
        }
    };
    Some(UiPreferences {
        workspace_layout_revision: splined_core::settings::CURRENT_WORKSPACE_LAYOUT_REVISION,
        appearance: appearance.borrow().clone(),
        status_bar_intensity: ui.get_status_bar_intensity().to_string(),
        status_bar_position: ui.get_status_bar_position().to_string(),
        status_bar_size: ui.get_status_bar_size().to_string(),
        workspace_mode: if ui.get_workspace_mode() == 2 { 2 } else { 0 },
        library_view: state.library.view.label().to_ascii_lowercase(),
        status_filter: state.library.status_filter.label().to_ascii_lowercase(),
        status_filters: state
            .library
            .status_filters
            .iter()
            .map(|value| value.label().to_string())
            .collect(),
        include_bypassed: ui.get_include_bypassed(),
        auto_scan_enabled: state.auto_scan_enabled,
        auto_scan_scope: match state.scan_scope {
            ScanScope::All => "all".to_string(),
            ScanScope::Selected => "selected".to_string(),
        },
        launch_mode: match state.launch_mode {
            LaunchMode::Read => "read".to_string(),
            LaunchMode::LiveWrite => "write".to_string(),
        },
        show_tracks: state.library.show_tracks,
        media_selector_expanded: state.media_selector_expanded,
        media_filter_expanded: ui.get_media_filter_expanded(),
        candidate_filter_expanded: state.candidate_filter_expanded,
        media_filter_locked: state.media_filter_locked,
        candidate_filter_locked: state.candidate_filter_locked,
        animate_webp_preview: state.animate_webp_preview,
        artwork_editing_expanded: state.artwork_editing_expanded,
        show_artwork: state.show_artwork,
        hover_enabled: state.hover_enabled,
        // Preview/apply is transient workflow state, never a persisted setting.
        upscale_preview_only: true,
        disable_intro_message: state.disable_intro_message,
        disable_confirmation_messages: state.disable_confirmation_messages,
        backup_directory: ui.get_backup_directory().to_string(),
        restore_path: ui.get_restore_path().to_string(),
        backup_selected_type: ui.get_backup_selected_type().to_string(),
        max_settings_backup_files: ui.get_max_settings_backup_files().to_string(),
        max_database_backup_files: ui.get_max_database_backup_files().to_string(),
        max_diagnostic_backup_files: ui.get_max_diagnostic_backup_files().to_string(),
        backup_prompt_dismissed: ui.get_backup_prompt_dismissed(),
        backup_schedule: normalize_backup_schedule(ui.get_backup_schedule().as_str()).to_string(),
        database_backup_schedule: normalize_backup_schedule(
            ui.get_database_backup_schedule().as_str(),
        )
        .to_string(),
        diagnostics_backup_schedule: normalize_backup_schedule(
            ui.get_diagnostics_backup_schedule().as_str(),
        )
        .to_string(),
        backup_mode: if ui.get_backup_mode() == "Restore" {
            "Restore".to_string()
        } else {
            "Backup".to_string()
        },
        backup_settings: ui.get_backup_settings(),
        backup_credentials: ui.get_backup_credentials(),
        backup_database: ui.get_backup_database(),
        backup_artist_images: ui.get_backup_artist_images(),
        restore_artist_images: ui.get_restore_artist_images(),
        backup_interface: ui.get_backup_interface(),
        backup_diagnostics: ui.get_backup_diagnostics(),
        artist_filter: state.library.artist_filter.clone(),
        album_filter: state.library.album_filter.clone(),
        artist_lookup_overrides: {
            let mut hints = state
                .artist_lookup_overrides
                .values()
                .cloned()
                .collect::<Vec<_>>();
            hints.sort_by_key(|hint| hint.artist.to_lowercase());
            hints
        },
        // Unlocked filters keep their fresh-run behavior. A locked filter is
        // an explicit user preference and must survive backup and restart.
        candidate_included_sources: locked_filter_values(&state.candidate_included_sources),
        candidate_included_types: locked_filter_values(&state.candidate_included_types),
        candidate_included_policies: locked_filter_values(&state.candidate_included_policies),
        candidate_included_ranges: locked_filter_values(&state.candidate_included_ranges),
        candidate_excluded_sources: locked_filter_values(&state.candidate_excluded_sources),
        candidate_excluded_types: locked_filter_values(&state.candidate_excluded_types),
        candidate_excluded_policies: locked_filter_values(&state.candidate_excluded_policies),
        candidate_excluded_ranges: locked_filter_values(&state.candidate_excluded_ranges),
        primary_adjustments: state.primary_adjustments.clone(),
        adjustment_presets: state.adjustment_presets.clone(),
        banner_theme_styles: state.banner_theme_styles.clone(),
        album_frame_theme_styles: state.album_frame_theme_styles.clone(),
        banner_sync_frame_letters: state.banner_sync_frame_letters,
        banner_random_harmony: state.banner_random_harmony,
        wheel_hue_shift_degrees: state.wheel_hue_shift_degrees,
        wheel_brightness_percent: state.wheel_brightness_percent,
        banner_theme_baseline_styles: state.banner_theme_baseline_styles.clone(),
        album_frame_theme_baseline_styles: state.album_frame_theme_baseline_styles.clone(),
        selected_source_name: ui.get_selected_source_name().to_string(),
        musicbrainz_id_editor_expanded: ui.get_musicbrainz_id_editor_expanded(),
        media_pane_width: layout.media_pane_width,
        media_pane_expanded_width: layout.media_pane_expanded_width,
        artwork_pane_width: layout.artwork_pane_width,
        artwork_pane_expanded_width: layout.artwork_pane_expanded_width,
        candidate_filter_height: layout.candidate_filter_height,
        candidate_filter_expanded_height: layout.candidate_filter_expanded_height,
        activity_pane_height: layout.activity_pane_height,
        enhancement_original_height: ui.get_enhancement_original_width().max(180.0).round() as u32,
        window_width: layout.normal_window_width,
        window_height: layout.normal_window_height,
        window_maximized: layout.maximized,
        custom_media_selector_expanded: *custom_window.media_expanded.borrow(),
        custom_media_filter_expanded: *custom_window.media_filter_expanded.borrow(),
        custom_media_pane_width: custom_layout.media_pane_width,
        custom_media_pane_expanded_width: custom_layout.media_pane_expanded_width,
        custom_window_width: custom_layout.normal_window_width,
        custom_window_height: custom_layout.normal_window_height,
        custom_window_maximized: custom_layout.maximized,
    })
}

async fn navidrome_artist_catalog(
    companion: &NavidromeCompanion,
    server_key: &str,
) -> Result<Arc<Vec<NavidromeArtist>>, String> {
    let cell = NAVIDROME_ARTIST_CATALOG
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "The Navidrome Artist cache is unavailable.".to_string())?
        .entry(server_key.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::OnceCell::new()))
        .clone();
    let cache_hit = cell.get().is_some();
    let started = Instant::now();
    let artists = cell
        .get_or_try_init(|| async { companion.artists().await.map(Arc::new) })
        .await?;
    if let Some(log) = UI_DEBUG_LOG_HANDLE.get() {
        log.record(format!(
            "navidrome-catalog kind=artists cache={} items={} elapsed_ms={}",
            if cache_hit { "hit" } else { "miss" },
            artists.len(),
            started.elapsed().as_millis()
        ));
    }
    Ok(Arc::clone(artists))
}

async fn navidrome_album_catalog(
    companion: &NavidromeCompanion,
    server_key: &str,
) -> Result<Arc<Vec<NavidromeAlbum>>, String> {
    let cell = NAVIDROME_ALBUM_CATALOG
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "The Navidrome Album cache is unavailable.".to_string())?
        .entry(server_key.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::OnceCell::new()))
        .clone();
    let cache_hit = cell.get().is_some();
    let started = Instant::now();
    let albums = cell
        .get_or_try_init(|| async { companion.albums().await.map(Arc::new) })
        .await?;
    if let Some(log) = UI_DEBUG_LOG_HANDLE.get() {
        log.record(format!(
            "navidrome-catalog kind=albums cache={} items={} elapsed_ms={}",
            if cache_hit { "hit" } else { "miss" },
            albums.len(),
            started.elapsed().as_millis()
        ));
    }
    Ok(Arc::clone(albums))
}

fn refresh_navidrome_thumbnails(
    state: &Arc<Mutex<ApplicationState>>,
    weak: slint::Weak<AppWindow>,
    focused_artist: Option<String>,
) {
    let full_refresh = focused_artist.is_none();
    let request_revision = if full_refresh {
        0
    } else {
        FOCUSED_THUMBNAIL_REFRESH_REVISION.fetch_add(1, Ordering::SeqCst) + 1
    };
    if full_refresh && FULL_THUMBNAIL_REFRESH_ACTIVE.swap(true, Ordering::SeqCst) {
        return;
    }
    let requested_artist = focused_artist
        .as_deref()
        .map(|value| value.trim().to_ascii_lowercase());
    // Top Songs runs independently of the heavier artist artwork hydration.
    if requested_artist.is_some()
        && let Some(ui) = weak.upgrade()
    {
        ui.invoke_load_artist_top_songs();
    }
    let Some((config, albums, needs_refresh, lookup_override)) =
        state.lock().ok().map(|mut app| {
            let lookup_override = requested_artist
                .as_ref()
                .and_then(|artist| app.artist_lookup_overrides.get(artist).cloned());
            let albums = app
                .library
                .albums
                .iter()
                .map(|album| ThumbnailAlbumTarget {
                    album_key: album.snapshot.album_key.clone(),
                    artist: album.snapshot.artist.clone(),
                    artist_mbid: album.snapshot.artist_mbid.clone(),
                    title: album.snapshot.title.clone(),
                    path: album.snapshot.path.clone(),
                    album_mbid: album.snapshot.album_mbid.clone(),
                    release_year: album.snapshot.release_year.clone(),
                    cover_path: album.snapshot.cover_path.clone(),
                })
                .collect::<Vec<_>>();
            // A completed artist hydration is authoritative for this session even
            // when Navidrome has no image for one of its albums. Missing artwork
            // must not trigger the entire API sequence again on every click.
            let needs_refresh = requested_artist
                .as_ref()
                .is_none_or(|requested| !app.artist_overview_cache.contains_key(requested));
            if !full_refresh {
                app.artist_overview_loading = needs_refresh;
                if needs_refresh && app.phase == RunPhase::Idle && !app.operation_busy {
                    app.operation_status = COMPILATION_GATHERING_STATUS.to_string();
                    app.operation_tone = "cyan".to_string();
                } else if !needs_refresh && app.operation_status == COMPILATION_GATHERING_STATUS {
                    app.operation_status = "Artist overview ready".to_string();
                    app.operation_tone = "green".to_string();
                }
            }
            (app.config.clone(), albums, needs_refresh, lookup_override)
        })
    else {
        if full_refresh {
            FULL_THUMBNAIL_REFRESH_ACTIVE.store(false, Ordering::SeqCst);
        }
        return;
    };
    if !needs_refresh {
        if full_refresh {
            FULL_THUMBNAIL_REFRESH_ACTIVE.store(false, Ordering::SeqCst);
        } else {
            refresh_weak(&weak, state);
        }
        return;
    }
    if !full_refresh {
        refresh_weak(&weak, state);
    }
    let state = Arc::clone(state);
    std::thread::spawn(move || {
        let refresh_started = Instant::now();
        let local_started = Instant::now();
        // Saved Artist Art is an independent local package. Show it while the
        // slower Navidrome biography, appearances, and candidates hydrate.
        let saved_package = requested_artist
            .as_deref()
            .map(|artist| {
                saved_artist_package_for_albums(
                    &albums,
                    artist,
                    &configured_shared_artist_art_dir(&config),
                )
            })
            .unwrap_or_default();
        let saved_banner = saved_package.banner.as_deref().map(|source| {
            let cache = Path::new(&config.scan.temporary_cache_dir)
                .join("navidrome-thumbnails")
                .join("artist-banners");
            cached_artist_banner_with_bottom_blur(
                source,
                &cache,
                requested_artist.as_deref().unwrap_or("artist"),
            )
            .unwrap_or_else(|_| source.to_path_buf())
        });
        if let Some(artist) = requested_artist.as_ref()
            && (saved_package.portrait.is_some() || saved_banner.is_some())
        {
            if let Some(path) = saved_package.portrait.as_deref() {
                preload_scaled_image(&path.to_string_lossy(), 320);
                preload_scaled_image(&path.to_string_lossy(), 1400);
            }
            if let Some(path) = saved_banner.as_deref() {
                preload_scaled_image(&path.to_string_lossy(), 1400);
            }
            if let Some(log) = UI_DEBUG_LOG_HANDLE.get() {
                log.record(format!(
                    "artist-package-local artist={} portrait={} banner={} elapsed_ms={}",
                    artist,
                    saved_package.portrait.is_some(),
                    saved_banner.is_some(),
                    local_started.elapsed().as_millis(),
                ));
            }
            let state_local = Arc::clone(&state);
            let weak_local = weak.clone();
            let artist = artist.clone();
            let portrait = saved_package.portrait.clone();
            let banner = saved_banner.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if FOCUSED_THUMBNAIL_REFRESH_REVISION.load(Ordering::SeqCst) != request_revision {
                    return;
                }
                if let Ok(mut app) = state_local.lock()
                    && overview_artist_name(&app)
                        .is_some_and(|current| current.trim().eq_ignore_ascii_case(&artist))
                {
                    if let Some(path) = portrait {
                        app.library.set_artist_artwork_path(&artist, &path);
                    }
                    if let Some(path) = banner {
                        app.artist_overview_banner_path = path.to_string_lossy().into_owned();
                    }
                }
                refresh_weak(&weak_local, &state_local);
            });
        }
        let requested_for_local = requested_artist.clone();
        let result = (|| -> Result<ThumbnailRefreshResult, String> {
            let credential_path =
                splined_core::credential_store::credential_path(&config, "Navidrome")?;
            let credential = NavidromeCredential::load(&credential_path)?;
            let server_key = credential.server_url.trim().to_ascii_lowercase();
            let companion = NavidromeCompanion::new(credential)?;
            // Navidrome artwork is loaded lazily for the focused artist and
            // kept only for this run. The configured disposable cache is
            // cleared normally; metadata authority remains in Navidrome.
            let cache_root =
                Path::new(&config.scan.temporary_cache_dir).join("navidrome-thumbnails");
            let artist_cache = cache_root.join("artists");
            let artist_banner_cache = cache_root.join("artist-banners");
            let similar_artist_cache = cache_root.join("similar-artists");
            let album_cache = cache_root.join("albums");
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| format!("Unable to start Navidrome thumbnail runtime: {error}"))?;
            runtime.block_on(async move {
                let remote_artists = navidrome_artist_catalog(&companion, &server_key).await?;
                let selected_remote_id = requested_artist.as_ref().and_then(|requested| {
                    navidrome_artist_for_lookup(
                        &remote_artists,
                        requested,
                        lookup_override.as_ref(),
                    )
                    .map(|candidate| candidate.id.to_ascii_lowercase())
                });
                let local_artists = albums
                    .iter()
                    .map(|album| album.artist.trim().to_ascii_lowercase())
                    .collect::<BTreeSet<_>>();
                let mut artist_paths = Vec::new();
                let mut artist_ids = Vec::new();
                let mut artist_metadata = None;
                let remote_album_catalog = navidrome_album_catalog(&companion, &server_key).await?;
                for artist in remote_artists.iter().filter(|artist| {
                    let normalized = artist.name.trim().to_ascii_lowercase();
                    requested_artist.as_ref().map_or_else(
                        || local_artists.contains(&normalized),
                        |_| selected_remote_id.as_ref().is_some_and(|id| artist.id.eq_ignore_ascii_case(id)),
                    )
                }) {
                    let local_artist = requested_artist.as_deref().unwrap_or(artist.name.as_str());
                    artist_ids.push((local_artist.to_string(), artist.id.clone()));
                    let current_artist_art = companion
                        .cache_artist_art(artist, &artist_cache, 192)
                        .await
                        .ok()
                        .flatten();
                    let saved_artist_art = local_artist_art_for_albums(
                        &albums,
                        local_artist,
                        &configured_shared_artist_art_dir(&config),
                    );
                    let library_artist_art = saved_artist_art.clone()
                        .or_else(|| {
                            current_artist_art.as_deref().and_then(|source| {
                                cached_circular_artist_thumbnail(source, &artist_cache, &artist.id)
                                    .ok()
                            })
                        })
                        .or_else(|| current_artist_art.clone());
                    if let Some(path) = library_artist_art {
                        artist_paths.push((local_artist.to_string(), path));
                    }
                    if requested_artist.is_some() {
                        let info = companion.artist_info(&artist.id).await.unwrap_or_default();
                        let mut similar_artists = stream::iter(
                            info.similar_artists
                                .iter()
                                .filter(|suggested| {
                                    !suggested
                                        .name
                                        .trim()
                                        .eq_ignore_ascii_case(artist.name.trim())
                                })
                                .take(10)
                                .enumerate()
                                .map(|(index, suggested)| {
                                    let artist = remote_artists
                                        .iter()
                                        .find(|candidate| {
                                            (!suggested.id.trim().is_empty()
                                                && candidate.id.eq_ignore_ascii_case(&suggested.id))
                                                || candidate
                                                    .name
                                                    .trim()
                                                    .eq_ignore_ascii_case(suggested.name.trim())
                                        })
                                        .cloned()
                                        .unwrap_or_else(|| suggested.clone());
                                    (index, artist)
                                })
                                .collect::<Vec<_>>(),
                        )
                        .map(|(index, artist)| {
                            let companion = companion.clone();
                            let similar_artist_cache = similar_artist_cache.clone();
                            async move {
                                let downloaded = companion
                                    .cache_artist_art(&artist, &similar_artist_cache, 160)
                                    .await
                                    .ok()
                                    .flatten();
                                let path = downloaded
                                    .as_deref()
                                    .and_then(|source| {
                                        cached_circular_artist_thumbnail(
                                            source,
                                            &similar_artist_cache,
                                            &artist.id,
                                        )
                                        .ok()
                                    })
                                    .or(downloaded)
                                    .map(|path| path.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                (index, (artist.name, path))
                            }
                        })
                        .buffer_unordered(4)
                        .collect::<Vec<_>>()
                        .await;
                        similar_artists.sort_by_key(|(index, _)| *index);
                        let similar_artists = similar_artists
                            .into_iter()
                            .map(|(_, artist)| artist)
                            .collect::<Vec<_>>();
                        let dedicated_banner = local_artist_banner_for_albums(
                            &albums,
                            local_artist,
                            &configured_shared_artist_art_dir(&config),
                        );
                        let has_saved_banner = dedicated_banner.is_some();
                        let navidrome_banner = if dedicated_banner.is_none() {
                            companion.cache_artist_info_art(&info, &artist_banner_cache, 1500)
                                .await.ok().flatten()
                        } else { None };
                        let (raw_banner, banner_uses_artist_art) = choose_artist_banner(
                            dedicated_banner, navidrome_banner, saved_artist_art.clone(),
                            current_artist_art.clone(),
                        );
                        let banner_path = raw_banner
                            .as_deref()
                            .and_then(|path| {
                                cached_artist_banner_with_bottom_blur(
                                    path,
                                    &artist_banner_cache,
                                    &artist.id,
                                )
                                .ok()
                            })
                            .or(raw_banner.clone())
                            .map(|path| path.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let artist_library = companion
                            .artist_library(&artist.id, &artist.name)
                            .await
                            .unwrap_or_default();
                        let appearance_ids = artist_library
                            .album_ids
                            .iter()
                            .map(|value| value.trim().to_ascii_lowercase())
                            .collect::<BTreeSet<_>>();
                        let overview_remote_albums = remote_album_catalog
                            .iter()
                            .filter(|album| {
                                album.artist_id.eq_ignore_ascii_case(&artist.id)
                                    || appearance_ids.contains(&album.id.to_ascii_lowercase())
                            })
                            .collect::<Vec<_>>();
                        let mut album_keys = BTreeSet::new();
                        let mut featured_album_keys = BTreeSet::new();
                        let mut album_ratings = HashMap::new();
                        let mut track_paths_by_album = HashMap::new();
                        for local in &albums {
                            if let Some(remote) = match_artist_overview_album(
                                local,
                                overview_remote_albums.iter().copied(),
                            ) {
                                let key = local.album_key.to_ascii_lowercase();
                                album_keys.insert(key.clone());
                                album_ratings.insert(key.clone(), remote.user_rating);
                                if let Some(paths) = artist_library
                                    .song_paths_by_album_id
                                    .get(&remote.id.to_ascii_lowercase())
                                {
                                    let local_paths = paths
                                        .iter()
                                        .filter_map(|path| path.rsplit(['/', '\\']).next())
                                        .filter(|name| {
                                            !name.is_empty() && *name != "." && *name != ".."
                                        })
                                        .map(|name| {
                                            Path::new(&local.path)
                                                .join(name)
                                                .to_string_lossy()
                                                .into_owned()
                                        })
                                        .collect::<BTreeSet<_>>();
                                    if !local_paths.is_empty() {
                                        if local.album_mbid.trim().is_empty() {
                                            track_paths_by_album
                                                .insert(key, local_paths.into_iter().collect());
                                        } else if local_paths.iter().any(|path| {
                                                read_local_track_evidence(Path::new(path))
                                                    .ok()
                                                    .is_some_and(|track| {
                                                        artist_is_secondary_credit(
                                                            &track.artist,
                                                            &artist.name,
                                                        )
                                                    })
                                            })
                                        {
                                            featured_album_keys.insert(key);
                                        }
                                    }
                                }
                            }
                        }
                        // Navidrome's artist search already proves which
                        // compilation Albums contain this Artist. Do not open
                        // and tag-read every track in those folders merely to
                        // render Artist Overview; a COMP card hydrates its own
                        // tracks only when the operator opens that Album.
                        let fallback_tracks = Vec::new();
                        let mut artwork_candidates = Vec::new();
                        if let Some(path) = current_artist_art.as_ref() {
                            artwork_candidates.push((
                                "Navidrome current".to_string(),
                                path.to_string_lossy().into_owned(),
                            ));
                        }
                        if let Some(path) = raw_banner.as_ref()
                            && !artwork_candidates.iter().any(|(_, candidate)| {
                                candidate.eq_ignore_ascii_case(&path.to_string_lossy())
                            })
                        {
                            artwork_candidates.push((
                                if banner_uses_artist_art {
                                    "Navidrome metadata"
                                } else if has_saved_banner {
                                    "Saved artist banner"
                                } else {
                                    "Navidrome banner"
                                }.to_string(),
                                path.to_string_lossy().into_owned(),
                            ));
                        }
                        artwork_candidates.extend(local_artist_art_candidates(
                            albums
                                .iter()
                                .map(|album| {
                                    (
                                        album.artist.as_str(),
                                        album.path.as_str(),
                                        album.album_mbid.as_str(),
                                        album.artist_mbid.as_str(),
                                    )
                                }),
                            local_artist,
                            &configured_shared_artist_art_dir(&config),
                        ));
                        let artist_key = local_artist.trim().to_lowercase();
                        artist_metadata = Some(ArtistOverviewMetadata {
                            artist_key,
                            biography: info.biography,
                            external_url: info.external_url,
                            musicbrainz_id: if info.musicbrainz_id.trim().is_empty() {
                                if artist.musicbrainz_id.trim().is_empty() {
                                    lookup_override.as_ref().map(|hint| hint.musicbrainz_id.clone()).unwrap_or_default()
                                } else {
                                    artist.musicbrainz_id.clone()
                                }
                            } else {
                                info.musicbrainz_id
                            },
                            banner_path,
                            banner_uses_artist_art,
                            similar_artists,
                            genres: artist_library.genres,
                            roles: artist_library.roles,
                            album_keys,
                            featured_album_keys,
                            track_paths_by_album,
                            album_ratings,
                            artwork_candidates,
                            fallback_tracks,
                        });
                    }
                }
                if artist_metadata.is_none()
                    && let Some(artist_key) = requested_artist.clone()
                {
                    artist_metadata = Some(ArtistOverviewMetadata {
                        artist_key,
                        ..ArtistOverviewMetadata::default()
                    });
                }
                if let Some(requested) = requested_artist.as_deref()
                    && let Some(metadata) = artist_metadata.as_mut()
                    && metadata.biography.trim().is_empty()
                    && let Some(primary) =
                        primary_credit_biography_artist(&albums, requested, &remote_artists)
                    && let Ok(primary_info) = companion.artist_info(&primary.id).await
                    && !primary_info.biography.trim().is_empty()
                {
                    metadata.biography = primary_info.biography;
                    if let Some(log) = UI_DEBUG_LOG_HANDLE.get() {
                        log.record(format!(
                            "artist-bio-primary-credit-fallback artist={} primary={} mbid={}",
                            requested, primary.name, primary.musicbrainz_id
                        ));
                    }
                }

                let remote_albums = remote_album_catalog;
                let mut album_paths = Vec::new();
                let album_downloads = albums
                    .iter()
                    .filter(|album| {
                        requested_artist.as_ref().is_none_or(|requested| {
                            album.artist.trim().eq_ignore_ascii_case(requested)
                                || artist_metadata.as_ref().is_some_and(|metadata| {
                                    metadata
                                        .album_keys
                                        .contains(&album.album_key.to_ascii_lowercase())
                                })
                        })
                    })
                    .filter_map(|album| {
                        if let Some(local) = local_album_art_file(album, &config.output.file_name) {
                            album_paths.push((album.album_key.clone(), local));
                            return None;
                        }
                        let matched = if requested_artist.is_some() {
                            let overview = artist_metadata.as_ref().and_then(|metadata| {
                                metadata
                                    .album_keys
                                    .contains(&album.album_key.to_ascii_lowercase())
                                    .then_some(())
                            });
                            overview.and_then(|_| {
                                match_artist_overview_album(album, remote_albums.iter())
                            })
                        } else {
                            match_companion_album(
                                &album.artist,
                                &album.title,
                                &album.album_mbid,
                                &album.release_year,
                                &remote_albums,
                            )
                        };
                        matched.map(|remote| (album.album_key.clone(), remote.cover_art_id.clone()))
                    })
                    .collect::<Vec<_>>();
                // A small bounded fan-out keeps an artist with many albums
                // responsive without flooding Navidrome or serializing every
                // thumbnail request behind the previous one.
                let remote_album_paths = stream::iter(album_downloads)
                    .map(|(album_key, cover_art_id)| {
                        let companion = companion.clone();
                        let album_cache = album_cache.clone();
                        async move {
                            companion
                                .cache_cover_art(&cover_art_id, &album_cache, 192)
                                .await
                                .ok()
                                .flatten()
                                .map(|path| (album_key, path))
                        }
                    })
                    .buffer_unordered(6)
                    .filter_map(|value| async move { value })
                    .collect::<Vec<_>>()
                    .await;
                album_paths.extend(remote_album_paths);
                // Decode the small list images off the Slint event thread.
                // Later model refreshes only clone cached 64 px buffers, so
                // selecting a row cannot stall behind thousands of decodes.
                for (_, path) in artist_paths.iter().chain(album_paths.iter()) {
                    preload_scaled_image(&path.to_string_lossy(), 64);
                }
                if let Some(metadata) = artist_metadata.as_ref() {
                    for (_, path) in &metadata.similar_artists {
                        if !path.trim().is_empty() {
                            preload_scaled_image(path, 96);
                        }
                    }
                }
                if let Some(log) = UI_DEBUG_LOG_HANDLE.get() {
                    let stats = companion.artwork_cache_stats();
                    log.record(format!(
                        "navidrome-focused-refresh artwork_cache_hits={} artwork_cache_misses={} elapsed_ms={}",
                        stats.hits,
                        stats.misses,
                        refresh_started.elapsed().as_millis()
                    ));
                }
                Ok((artist_paths, album_paths, artist_ids, artist_metadata))
            })
        })()
        .map(|(mut artist_paths, album_paths, artist_ids, mut artist_metadata)| {
            if let (Some(artist), Some(path)) =
                (requested_for_local.as_ref(), saved_package.portrait.as_ref())
            {
                artist_paths.retain(|(name, _)| !name.eq_ignore_ascii_case(artist));
                artist_paths.push((artist.clone(), path.clone()));
            }
            if let (Some(path), Some(metadata)) =
                (saved_banner.as_ref(), artist_metadata.as_mut())
            {
                metadata.banner_path = path.to_string_lossy().into_owned();
                metadata.banner_uses_artist_art = false;
            }
            (artist_paths, album_paths, artist_ids, artist_metadata)
        });
        if full_refresh {
            FULL_THUMBNAIL_REFRESH_ACTIVE.store(false, Ordering::SeqCst);
        }
        let _ = slint::invoke_from_event_loop(move || {
            if !full_refresh
                && FOCUSED_THUMBNAIL_REFRESH_REVISION.load(Ordering::SeqCst) != request_revision
            {
                return;
            }
            if !full_refresh && let Ok(mut app) = state.lock() {
                app.artist_overview_loading = false;
                if app.operation_status == COMPILATION_GATHERING_STATUS {
                    match &result {
                        Ok((_, _, _, Some(_))) => {
                            app.operation_status =
                                "Compilation albums and artist tracks ready".to_string();
                            app.operation_tone = "green".to_string();
                        }
                        Ok(_) | Err(_) => {
                            app.operation_status =
                                "Compilation album and artist track information unavailable"
                                    .to_string();
                            app.operation_tone = "orange".to_string();
                        }
                    }
                }
            }
            match result {
                Ok((artist_paths, album_paths, artist_ids, artist_metadata)) => {
                    publish_thumbnail_paths(
                        &state,
                        &weak,
                        artist_paths,
                        album_paths,
                        artist_ids,
                        artist_metadata,
                    )
                }
                Err(error) => {
                    if let Ok(mut app) = state.lock() {
                        app.push_log(format!("Navidrome thumbnails unavailable: {error}"));
                    }
                    refresh_weak(&weak, &state);
                }
            }
        });
    });
}

fn refresh_visible_navidrome_thumbnails(
    state: &Arc<Mutex<ApplicationState>>,
    weak: slint::Weak<AppWindow>,
    requests: Vec<(String, String)>,
) {
    let Some((config, artist_names, artist_targets, album_targets, hydration_request_keys)) =
        state.lock().ok().map(|mut app| {
            let mut artist_names = BTreeSet::new();
            let mut album_targets = Vec::new();
            let mut hydration_request_keys = Vec::new();
            for (key, kind) in requests {
                let key = key.trim();
                if key.is_empty() {
                    continue;
                }
                if kind.eq_ignore_ascii_case("artist") {
                    let normalized = key.to_ascii_lowercase();
                    let request_key = format!("artist:{normalized}");
                    if app.thumbnail_hydration_attempts.insert(request_key.clone()) {
                        artist_names.insert(key.to_string());
                        hydration_request_keys.push(request_key);
                    }
                    continue;
                }
                let normalized = key.to_ascii_lowercase();
                let request_key = format!("album:{normalized}");
                if !app.thumbnail_hydration_attempts.insert(request_key.clone()) {
                    continue;
                }
                hydration_request_keys.push(request_key);
                if let Some(album) = app
                    .library
                    .albums
                    .iter()
                    .find(|album| album.snapshot.album_key.eq_ignore_ascii_case(key))
                {
                    if app.library.view == LibraryView::Artists
                        && app.library.focused_artist.as_ref().is_some_and(|artist| {
                            album.snapshot.artist.trim().eq_ignore_ascii_case(artist)
                        })
                    {
                        // The focused-artist hydrator supplies its complete card
                        // strip and richer overview in one bounded request.
                        continue;
                    }
                    album_targets.push(ThumbnailAlbumTarget {
                        album_key: album.snapshot.album_key.clone(),
                        artist: album.snapshot.artist.clone(),
                        artist_mbid: album.snapshot.artist_mbid.clone(),
                        title: album.snapshot.title.clone(),
                        path: album.snapshot.path.clone(),
                        album_mbid: album.snapshot.album_mbid.clone(),
                        release_year: album.snapshot.release_year.clone(),
                        cover_path: album.snapshot.cover_path.clone(),
                    });
                }
            }
            let artist_targets = artist_names
                .iter()
                .filter_map(|artist| {
                    focused_artist_art_target(&app, artist)
                        .ok()
                        .map(|target| (artist.clone(), target))
                })
                .collect::<Vec<_>>();
            (
                app.config.clone(),
                artist_names,
                artist_targets,
                album_targets,
                hydration_request_keys,
            )
        })
    else {
        return;
    };
    if artist_names.is_empty() && album_targets.is_empty() {
        return;
    }
    let state = Arc::clone(state);
    std::thread::spawn(move || {
        let hydration_started = Instant::now();
        let result =
            (|| -> Result<(ThumbnailPaths, ThumbnailPaths, ArtistNavidromeIds), String> {
                let mut artist_paths = artist_targets
                    .iter()
                    .filter_map(|(artist, target)| {
                        local_artist_art_at_target(target).map(|path| (artist.clone(), path))
                    })
                    .collect::<Vec<_>>();
                let mut album_paths = album_targets
                    .iter()
                    .filter_map(|album| {
                        local_album_art_file(album, &config.output.file_name)
                            .map(|path| (album.album_key.clone(), path))
                    })
                    .collect::<Vec<_>>();
                let missing_artist_names = artist_names
                    .iter()
                    .filter(|artist| {
                        !artist_paths
                            .iter()
                            .any(|(local, _)| local.eq_ignore_ascii_case(artist))
                    })
                    .cloned()
                    .collect::<BTreeSet<_>>();
                let missing_album_targets = album_targets
                    .iter()
                    .filter(|album| {
                        !album_paths
                            .iter()
                            .any(|(key, _)| key.eq_ignore_ascii_case(&album.album_key))
                    })
                    .collect::<Vec<_>>();
                if missing_artist_names.is_empty() && missing_album_targets.is_empty() {
                    for (_, path) in artist_paths.iter().chain(album_paths.iter()) {
                        preload_scaled_image(&path.to_string_lossy(), 64);
                    }
                    return Ok((artist_paths, album_paths, Vec::new()));
                }
                // Local artwork must not disappear merely because Navidrome
                // is slow, offline, or still serving an older thumbnail.
                if !artist_paths.is_empty() || !album_paths.is_empty() {
                    let local_artists = artist_paths.clone();
                    let local_albums = album_paths.clone();
                    let local_state = Arc::clone(&state);
                    let local_weak = weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        publish_thumbnail_paths(
                            &local_state,
                            &local_weak,
                            local_artists,
                            local_albums,
                            Vec::new(),
                            None,
                        );
                    });
                }
                let credential_path =
                    splined_core::credential_store::credential_path(&config, "Navidrome")?;
                let credential = NavidromeCredential::load(&credential_path)?;
                let server_key = credential.server_url.trim().to_ascii_lowercase();
                let companion = NavidromeCompanion::new(credential)?;
                let cache_root =
                    Path::new(&config.scan.temporary_cache_dir).join("navidrome-thumbnails");
                let artist_cache = cache_root.join("artists");
                let album_cache = cache_root.join("albums");
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| format!("Unable to start lazy thumbnail runtime: {error}"))?;
                runtime.block_on(async move {
                    let mut artist_ids = Vec::new();
                    if !missing_artist_names.is_empty() {
                        let remote_artists =
                            navidrome_artist_catalog(&companion, &server_key).await?;
                        let requested = missing_artist_names
                            .iter()
                            .map(|name| name.trim().to_ascii_lowercase())
                            .collect::<BTreeSet<_>>();
                        let matches = remote_artists
                            .iter()
                            .filter(|artist| {
                                requested.contains(&artist.name.trim().to_ascii_lowercase())
                            })
                            .cloned()
                            .collect::<Vec<_>>();
                        let loaded = stream::iter(matches)
                            .map(|artist| {
                                let companion = companion.clone();
                                let artist_cache = artist_cache.clone();
                                async move {
                                    let path = companion
                                        .cache_artist_art(&artist, &artist_cache, 192)
                                        .await
                                        .ok()
                                        .flatten();
                                    (artist, path)
                                }
                            })
                            .buffer_unordered(4)
                            .collect::<Vec<_>>()
                            .await;
                        for (artist, path) in loaded {
                            artist_ids.push((artist.name.clone(), artist.id));
                            if let Some(path) = path {
                                artist_paths.push((artist.name, path));
                            }
                        }
                    }

                    if !missing_album_targets.is_empty() {
                        let remote_albums =
                            navidrome_album_catalog(&companion, &server_key).await?;
                        let downloads = missing_album_targets
                            .iter()
                            .filter_map(|album| {
                                match_companion_album(
                                    &album.artist,
                                    &album.title,
                                    &album.album_mbid,
                                    &album.release_year,
                                    &remote_albums,
                                )
                                .map(|remote| {
                                    (album.album_key.clone(), remote.cover_art_id.clone())
                                })
                            })
                            .collect::<Vec<_>>();
                        album_paths = stream::iter(downloads)
                            .map(|(album_key, cover_art_id)| {
                                let companion = companion.clone();
                                let album_cache = album_cache.clone();
                                async move {
                                    companion
                                        .cache_cover_art(&cover_art_id, &album_cache, 192)
                                        .await
                                        .ok()
                                        .flatten()
                                        .map(|path| (album_key, path))
                                }
                            })
                            .buffer_unordered(6)
                            .filter_map(|value| async move { value })
                            .collect::<Vec<_>>()
                            .await;
                    }
                    for (_, path) in artist_paths.iter().chain(album_paths.iter()) {
                        preload_scaled_image(&path.to_string_lossy(), 64);
                    }
                    if let Some(log) = UI_DEBUG_LOG_HANDLE.get() {
                        let stats = companion.artwork_cache_stats();
                        log.record(format!(
                            "navidrome-library-hydration artwork_cache_hits={} artwork_cache_misses={} elapsed_ms={}",
                            stats.hits,
                            stats.misses,
                            hydration_started.elapsed().as_millis()
                        ));
                    }
                    Ok((artist_paths, album_paths, artist_ids))
                })
            })();
        let _ = slint::invoke_from_event_loop(move || match result {
            Ok((artist_paths, album_paths, artist_ids)) => {
                publish_thumbnail_paths(&state, &weak, artist_paths, album_paths, artist_ids, None)
            }
            Err(error) => {
                if let Ok(mut app) = state.lock() {
                    for request_key in hydration_request_keys {
                        app.thumbnail_hydration_attempts.remove(&request_key);
                    }
                    app.push_log(format!("Lazy Navidrome thumbnails unavailable: {error}"));
                }
            }
        });
    });
}

fn match_artist_overview_album<'a>(
    local: &ThumbnailAlbumTarget,
    albums: impl IntoIterator<Item = &'a NavidromeAlbum>,
) -> Option<&'a NavidromeAlbum> {
    let albums = albums.into_iter().collect::<Vec<_>>();
    if !local.album_mbid.trim().is_empty() {
        // A tagged release ID is authoritative. Falling through to title/year
        // matching here can attach unrelated same-named releases (for example,
        // multiple artists' 1994 albums named "Greatest Hits") to the focused
        // artist's appearance list.
        return albums.iter().copied().find(|album| {
            album
                .musicbrainz_id
                .eq_ignore_ascii_case(local.album_mbid.trim())
        });
    }
    let title = normalized_overview_identity(&local.title);
    let artist = normalized_overview_identity(&local.artist);
    let mut matches = albums
        .iter()
        .copied()
        .filter(|album| normalized_overview_identity(&album.title) == title)
        .filter(|album| normalized_overview_identity(&album.artist) == artist)
        .filter(|album| {
            local.release_year.trim().is_empty()
                || album.year.trim().is_empty()
                || album.year.trim() == local.release_year.trim()
        });
    let candidate = matches.next()?;
    matches.next().is_none().then_some(candidate)
}

fn normalized_overview_identity(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_artwork_stem(path: &Path, stem: &str) -> bool {
    let Some(name) = path.file_stem().and_then(|value| value.to_str()) else {
        return false;
    };
    // NAS files created through Windows can have an extra dot before the
    // extension (for example `artist..jpg`). Treat that as the same artwork
    // stem without renaming the user's file.
    let name = name.to_ascii_lowercase();
    let name = name.trim_end_matches('.');
    let stem = stem.to_ascii_lowercase();
    name == stem
        || name
            .strip_prefix(&format!("{stem}-("))
            .and_then(|number| number.strip_suffix(')'))
            .is_some_and(|number| number.parse::<usize>().is_ok())
}

fn newest_local_artwork_file(directory: &Path, stems: &[&str], webp: bool) -> Option<PathBuf> {
    fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .is_ok_and(|kind| kind.is_file() && !kind.is_symlink())
        })
        .map(|entry| entry.path())
        .filter(|path| stems.iter().any(|stem| is_artwork_stem(path, stem)))
        .filter(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .is_some_and(|extension| {
                    matches!(
                        extension.to_ascii_lowercase().as_str(),
                        "jpg" | "jpeg" | "png"
                    ) || (webp && extension.eq_ignore_ascii_case("webp"))
                })
        })
        .filter(|path| inspect_image(path).is_ok())
        .max_by_key(|path| {
            fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .unwrap_or(UNIX_EPOCH)
        })
}

fn local_artist_art_file(directory: &Path) -> Option<PathBuf> {
    newest_local_artwork_file(directory, &["artist"], true)
}

#[derive(Clone, Debug)]
struct ArtistArtTarget {
    directory: PathBuf,
    stem: String,
    album_count: usize,
    managed: bool,
}

fn artist_art_targets_for_albums<'a>(
    albums: impl IntoIterator<Item = (&'a str, &'a str, &'a str, &'a str)>,
    artist: &str,
    cache_dir: &Path,
) -> Vec<ArtistArtTarget> {
    if artist.trim().is_empty() {
        return Vec::new();
    }
    let mut directories = HashMap::<String, (PathBuf, usize)>::new();
    let mut has_eligible_album = false;
    let mut artist_ids = BTreeSet::new();
    let normalized_artist = normalized_overview_identity(artist);
    for (album_artist, album_path, album_mbid, artist_mbid) in albums {
        // A missing Album ID is COMP fallback, whose artwork remains track-scoped.
        if album_mbid.trim().is_empty() {
            continue;
        }
        if !album_artist.trim().eq_ignore_ascii_case(artist.trim()) {
            continue;
        }
        let Some(parent) = Path::new(album_path).parent() else {
            continue;
        };
        has_eligible_album = true;
        if is_valid_mbid(artist_mbid) {
            artist_ids.insert(artist_mbid.trim().to_ascii_lowercase());
        }
        if normalized_artist.is_empty()
            || parent
                .file_name()
                .and_then(|name| name.to_str())
                .is_none_or(|name| normalized_overview_identity(name) != normalized_artist)
        {
            continue;
        }
        let key = parent.to_string_lossy().to_ascii_lowercase();
        let entry = directories
            .entry(key)
            .or_insert_with(|| (parent.to_path_buf(), 0));
        entry.1 += 1;
    }
    if !has_eligible_album {
        return Vec::new();
    }
    let mut targets = directories
        .into_values()
        .map(|(directory, album_count)| ArtistArtTarget {
            directory,
            stem: "artist".to_string(),
            album_count,
            managed: false,
        })
        .collect::<Vec<_>>();
    targets.sort_by(|left, right| {
        right
            .album_count
            .cmp(&left.album_count)
            .then_with(|| left.directory.cmp(&right.directory))
    });
    // A Soundtracks/Various Artists directory is not the credited Artist's
    // physical folder, even if it currently contains only one indexed Artist.
    // Never use its unqualified artist.* for Artist Art.
    let artist_key = if artist_ids.len() == 1 {
        let id = artist_ids.into_iter().next().expect("one Artist ID");
        format!("mbid-{id}")
    } else {
        let digest = Sha256::digest(artist.trim().to_lowercase().as_bytes());
        let name_key = digest[..16]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!("name-{name_key}")
    };
    targets.push(ArtistArtTarget {
        directory: cache_dir.to_path_buf(),
        stem: format!("artist-{artist_key}"),
        album_count: 0,
        managed: true,
    });
    targets
}

fn local_artist_art_at_target(target: &ArtistArtTarget) -> Option<PathBuf> {
    newest_local_artwork_file(&target.directory, &[&target.stem], true)
}

fn local_album_art_file(album: &ThumbnailAlbumTarget, configured_stem: &str) -> Option<PathBuf> {
    // Compilation/fallback review is track-scoped. Never pull cover.* from
    // its shared Various Artists folder into the selected artwork pane.
    if album.album_mbid.trim().is_empty() {
        return None;
    }
    let directory = Path::new(&album.path);
    let mut newest = newest_local_artwork_file(directory, &["cover", configured_stem], false);
    let indexed = Path::new(&album.cover_path);
    if indexed.is_file() && inspect_image(indexed).is_ok() {
        let indexed_mtime = fs::metadata(indexed)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(UNIX_EPOCH);
        let newest_mtime = newest
            .as_ref()
            .and_then(|path| fs::metadata(path).ok()?.modified().ok())
            .unwrap_or(UNIX_EPOCH);
        if newest.is_none() || indexed_mtime > newest_mtime {
            newest = Some(indexed.to_path_buf());
        }
    }
    newest
}

fn local_artist_art_for_albums(
    albums: &[ThumbnailAlbumTarget],
    artist: &str,
    cache_dir: &Path,
) -> Option<PathBuf> {
    artist_art_targets_for_albums(
        albums.iter().map(|album| {
            (
                album.artist.as_str(),
                album.path.as_str(),
                album.album_mbid.as_str(),
                album.artist_mbid.as_str(),
            )
        }),
        artist,
        cache_dir,
    )
    .into_iter()
    .find_map(|target| local_artist_art_at_target(&target))
}

fn artist_banner_targets_for_albums<'a>(
    albums: impl IntoIterator<Item = (&'a str, &'a str, &'a str, &'a str)>,
    artist: &str,
    directory: &Path,
) -> Vec<(PathBuf, String)> {
    artist_art_targets_for_albums(albums, artist, directory)
        .into_iter()
        .map(|target| {
            let stem = if target.managed {
                format!("banner-{}", target.stem.trim_start_matches("artist-"))
            } else {
                "banner".to_string()
            };
            (target.directory, stem)
        })
        .collect()
}

fn local_artist_banner_for_albums(
    albums: &[ThumbnailAlbumTarget],
    artist: &str,
    directory: &Path,
) -> Option<PathBuf> {
    artist_banner_targets_for_albums(
        albums.iter().map(|album| {
            (
                album.artist.as_str(),
                album.path.as_str(),
                album.album_mbid.as_str(),
                album.artist_mbid.as_str(),
            )
        }),
        artist,
        directory,
    )
    .into_iter()
    .find_map(|(directory, stem)| newest_local_artwork_file(&directory, &[&stem], true))
}

fn saved_artist_package_for_albums(
    albums: &[ThumbnailAlbumTarget],
    artist: &str,
    directory: &Path,
) -> SavedArtistPackage {
    SavedArtistPackage {
        portrait: local_artist_art_for_albums(albums, artist, directory),
        banner: local_artist_banner_for_albums(albums, artist, directory),
    }
}

fn choose_artist_banner(
    dedicated: Option<PathBuf>,
    navidrome: Option<PathBuf>,
    saved_artist_art: Option<PathBuf>,
    remote_artist_art: Option<PathBuf>,
) -> (Option<PathBuf>, bool) {
    if let Some(path) = dedicated.or(navidrome) {
        (Some(path), false)
    } else if let Some(path) = saved_artist_art {
        (Some(path), true)
    } else {
        (remote_artist_art, false)
    }
}

fn local_artist_art_candidates<'a>(
    albums: impl IntoIterator<Item = (&'a str, &'a str, &'a str, &'a str)>,
    artist: &str,
    cache_dir: &Path,
) -> Vec<(String, String)> {
    let mut result = Vec::new();
    for target in artist_art_targets_for_albums(albums, artist, cache_dir) {
        let banner_stem = if target.managed {
            format!("banner-{}", target.stem.trim_start_matches("artist-"))
        } else {
            "banner".to_string()
        };
        if let Some(path) = newest_local_artwork_file(&target.directory, &[&banner_stem], true) {
            result.push((
                "Local artist banner".to_string(),
                path.to_string_lossy().into_owned(),
            ));
        }
        let Ok(entries) = fs::read_dir(&target.directory) else {
            continue;
        };
        for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
            let is_artist_art = is_artwork_stem(&path, &target.stem);
            let supported = path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| {
                    matches!(
                        value.to_ascii_lowercase().as_str(),
                        "jpg" | "jpeg" | "png" | "webp"
                    )
                });
            if is_artist_art && supported && inspect_image(&path).is_ok() {
                result.push((
                    "Local artist file".to_string(),
                    path.to_string_lossy().into_owned(),
                ));
            }
        }
    }
    result.sort();
    result.dedup_by(|left, right| left.1.eq_ignore_ascii_case(&right.1));
    result
}

fn merge_artist_art_candidates(
    local: ArtistArtworkCandidates,
    existing: ArtistArtworkCandidates,
    remote: ArtistArtworkCandidates,
) -> ArtistArtworkCandidates {
    let mut result = Vec::new();
    for candidate in local.into_iter().chain(existing).chain(remote) {
        if !result
            .iter()
            .any(|(_, path): &(String, String)| path.eq_ignore_ascii_case(&candidate.1))
        {
            result.push(candidate);
        }
    }
    result
}

fn cached_circular_artist_thumbnail(
    source: &Path,
    cache_directory: &Path,
    artist_id: &str,
) -> Result<PathBuf, String> {
    inspect_image(source)?;
    fs::create_dir_all(cache_directory).map_err(|error| {
        format!(
            "Unable to create Navidrome similar-artist cache {}: {error}",
            cache_directory.display()
        )
    })?;
    let source_identity = source
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("artist");
    let path = cache_directory.join(format!(
        "circle-v1-{}-{}.png",
        sanitize_sample_component(artist_id),
        sanitize_sample_component(source_identity)
    ));
    if path.is_file() && inspect_image(&path).is_ok() {
        return Ok(path);
    }
    let decoded = image::ImageReader::open(source)
        .map_err(|error| {
            format!(
                "Unable to open Navidrome similar-artist artwork {}: {error}",
                source.display()
            )
        })?
        .with_guessed_format()
        .map_err(|error| {
            format!(
                "Unable to identify Navidrome similar-artist artwork {}: {error}",
                source.display()
            )
        })?
        .decode()
        .map_err(|error| {
            format!(
                "Unable to decode Navidrome similar-artist artwork {}: {error}",
                source.display()
            )
        })?;
    let mut thumbnail = decoded
        .resize_to_fill(160, 160, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let center = 79.5_f32;
    let radius = 79.25_f32;
    for (x, y, pixel) in thumbnail.enumerate_pixels_mut() {
        let distance = ((x as f32 - center).powi(2) + (y as f32 - center).powi(2)).sqrt();
        if distance >= radius {
            pixel.0[3] = 0;
        } else if distance > radius - 1.0 {
            pixel.0[3] = ((radius - distance) * pixel.0[3] as f32).round() as u8;
        }
    }
    let mut encoded = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(thumbnail)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .map_err(|error| format!("Unable to encode the similar-artist portrait: {error}"))?;
    replace_binary_file(
        &path,
        encoded.get_ref(),
        "Navidrome similar-artist portrait cache",
        |staged| inspect_image(staged).map(|_| ()),
    )?;
    Ok(path)
}

fn circular_artist_art_png(source: &[u8]) -> Result<Vec<u8>, String> {
    prepare_artist_art_png(source, None)
}

fn prepare_artist_art_png(
    source: &[u8],
    output: Option<&splined_core::config::OutputConfig>,
) -> Result<Vec<u8>, String> {
    prepare_artist_art_png_with_fit(source, output, 1.0, 0.0, 0.0)
}

fn prepare_artist_art_png_with_fit(
    source: &[u8],
    output: Option<&splined_core::config::OutputConfig>,
    zoom: f32,
    horizontal: f32,
    vertical: f32,
) -> Result<Vec<u8>, String> {
    const SIZE: u32 = 1024;
    let decoded = image::load_from_memory(source)
        .map_err(|error| format!("Unable to decode selected Artist Art: {error}"))?;
    let source_side = decoded.width().min(decoded.height());
    let crop_side = ((source_side as f32 / zoom.clamp(1.0, 8.0)).round() as u32).max(1);
    let crop_x = ((decoded.width() - crop_side) as f32 / 2.0
        - horizontal.clamp(-0.4, 0.4) * decoded.width() as f32)
        .clamp(0.0, (decoded.width() - crop_side) as f32).round() as u32;
    let crop_y = ((decoded.height() - crop_side) as f32 / 2.0
        - vertical.clamp(-0.4, 0.4) * decoded.height() as f32)
        .clamp(0.0, (decoded.height() - crop_side) as f32).round() as u32;
    let framed = image::imageops::crop_imm(&decoded, crop_x, crop_y, crop_side, crop_side).to_image();
    let mut thumbnail = image::imageops::resize(&framed, SIZE, SIZE, image::imageops::FilterType::Lanczos3);
    if let Some(output) = output {
        thumbnail =
            apply_selected_image_adjustments(image::DynamicImage::ImageRgba8(thumbnail), output)
                .to_rgba8();
    }
    let center = (SIZE as f32 - 1.0) / 2.0;
    let radius = center - 0.25;
    for (x, y, pixel) in thumbnail.enumerate_pixels_mut() {
        let distance = ((x as f32 - center).powi(2) + (y as f32 - center).powi(2)).sqrt();
        let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0);
        pixel.0[3] = (pixel.0[3] as f32 * coverage).round() as u8;
    }
    let mut encoded = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(thumbnail)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .map_err(|error| format!("Unable to encode round Artist Art thumbnail: {error}"))?;
    Ok(encoded.into_inner())
}

fn artist_art_destination(
    directory: &Path,
    stem: &str,
    bytes: &[u8],
    preserve_file: bool,
) -> Result<(PathBuf, bool), String> {
    let canonical = directory.join(format!("{stem}.png"));
    if !preserve_file {
        return Ok((canonical, false));
    }
    for number in 1..=1_000_000usize {
        let candidate = if number == 1 {
            canonical.clone()
        } else {
            directory.join(format!("{stem}-({number}).png"))
        };
        if !candidate.exists() {
            return Ok((candidate, false));
        }
        let existing = fs::read(&candidate).map_err(|error| {
            format!(
                "Unable to compare existing Artist Art {}: {error}",
                candidate.display()
            )
        })?;
        if existing == bytes {
            return Ok((candidate, true));
        }
    }
    Err(format!(
        "No preserved Artist Art filename is available in {}.",
        directory.display()
    ))
}

fn remove_superseded_artist_art(directory: &Path, stem: &str, keep: &Path) -> Vec<String> {
    let mut warnings = Vec::new();
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            warnings.push(format!(
                "Unable to inspect superseded Artist Art in {}: {error}",
                directory.display()
            ));
            return warnings;
        }
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path == keep || !is_artwork_stem(&path, stem) {
            continue;
        }
        if !entry
            .file_type()
            .is_ok_and(|kind| kind.is_file() && !kind.is_symlink())
        {
            continue;
        }
        if !path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "jpg" | "jpeg" | "png" | "webp"
                )
            })
        {
            continue;
        }
        if let Err(error) = fs::remove_file(&path) {
            warnings.push(format!(
                "Unable to remove superseded Artist Art {}: {error}",
                path.display()
            ));
        }
    }
    warnings
}

fn write_artist_art_file(
    directory: &Path,
    source: &[u8],
    preserve_file: bool,
) -> Result<(PathBuf, bool, Vec<String>), String> {
    write_artist_art_file_with_stem(directory, "artist", source, preserve_file)
}

fn write_artist_art_file_with_stem(
    directory: &Path,
    stem: &str,
    source: &[u8],
    preserve_file: bool,
) -> Result<(PathBuf, bool, Vec<String>), String> {
    let bytes = circular_artist_art_png(source)?;
    write_prepared_artist_art_file_with_stem(directory, stem, &bytes, preserve_file)
}

fn write_prepared_artist_art_file_with_stem(
    directory: &Path,
    stem: &str,
    bytes: &[u8],
    preserve_file: bool,
) -> Result<(PathBuf, bool, Vec<String>), String> {
    fs::create_dir_all(directory).map_err(|error| {
        format!(
            "Unable to create Artist Art directory {}: {error}",
            directory.display()
        )
    })?;
    let (target, unchanged) = artist_art_destination(directory, stem, &bytes, preserve_file)?;
    if !unchanged {
        replace_binary_file(&target, &bytes, "artist artwork", |staged| {
            inspect_image(staged).map(|_| ())
        })?;
    }
    let warnings = if preserve_file {
        Vec::new()
    } else {
        remove_superseded_artist_art(directory, stem, &target)
    };
    Ok((target, unchanged, warnings))
}

fn cached_artist_banner_with_bottom_blur(
    source: &Path,
    cache_directory: &Path,
    artist_id: &str,
) -> Result<PathBuf, String> {
    inspect_image(source)?;
    fs::create_dir_all(cache_directory).map_err(|error| {
        format!(
            "Unable to create Navidrome artist-banner cache {}: {error}",
            cache_directory.display()
        )
    })?;
    // A saved artist.* can be replaced at the same path. Key the derivative
    // by image content so the overview never reuses the previous banner.
    let source_bytes = fs::read(source).map_err(|error| {
        format!(
            "Unable to read artist banner source {}: {error}",
            source.display()
        )
    })?;
    let source_digest = Sha256::digest(&source_bytes);
    let source_identity = source_digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let path = cache_directory.join(format!(
        "banner-v6-{}-{}.png",
        sanitize_sample_component(artist_id),
        source_identity
    ));
    if path.is_file() && inspect_image(&path).is_ok() {
        return Ok(path);
    }
    let decoded = image::load_from_memory(&source_bytes).map_err(|error| {
        format!(
            "Unable to decode artist banner source {}: {error}",
            source.display()
        )
    })?;
    // Navidrome can supply a square portrait when no dedicated banner exists.
    // Filling the landscape overview with that image avoids placing the round
    // artist thumbnail a second time in the center of the background.
    let banner = if decoded.width() as f32 / (decoded.height().max(1) as f32) < 1.6 {
        decoded
            .resize_to_fill(1400, 600, image::imageops::FilterType::Lanczos3)
            .to_rgba8()
    } else {
        compose_artist_banner(&decoded, 1.0, 0.0)
    };
    let mut encoded = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(banner)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .map_err(|error| format!("Unable to encode the artist banner: {error}"))?;
    replace_binary_file(
        &path,
        encoded.get_ref(),
        "Navidrome artist banner cache",
        |staged| inspect_image(staged).map(|_| ()),
    )?;
    Ok(path)
}

fn compose_artist_banner(
    decoded: &image::DynamicImage,
    zoom: f32,
    vertical: f32,
) -> image::RgbaImage {
    compose_artist_banner_with_position(decoded, zoom, 0.0, vertical)
}

fn compose_artist_banner_with_position(
    decoded: &image::DynamicImage,
    zoom: f32,
    horizontal: f32,
    vertical: f32,
) -> image::RgbaImage {
    const WIDTH: u32 = 1400;
    const HEIGHT: u32 = 600;
    // Blur a quarter-size backing plate before enlarging it. Blurring the
    // full 1400 × 600 image twice made Artist Overview noticeably slow.
    let mut banner = decoded
        .resize_to_fill(WIDTH / 4, HEIGHT / 4, image::imageops::FilterType::Triangle)
        .blur(5.0)
        .resize_exact(WIDTH, HEIGHT, image::imageops::FilterType::Triangle)
        .to_rgba8();
    let zoom = zoom.clamp(1.0, 8.0);
    let foreground = decoded
        .resize(
            ((WIDTH as f32 * zoom).round() as u32).max(1),
            ((HEIGHT as f32 * zoom).round() as u32).max(1),
            image::imageops::FilterType::Lanczos3,
        )
        .to_rgba8();
    let x = (WIDTH as i64 - foreground.width() as i64) / 2
        + (horizontal.clamp(-0.4, 0.4) * WIDTH as f32).round() as i64;
    let y = (HEIGHT as i64 - foreground.height() as i64) / 2
        + (vertical.clamp(-0.4, 0.4) * HEIGHT as f32).round() as i64;
    image::imageops::overlay(&mut banner, &foreground, x, y);
    banner
}

fn enhanced_artist_banner(
    source: &[u8],
    zoom: f32,
    vertical: f32,
    output: &splined_core::config::OutputConfig,
) -> Result<image::DynamicImage, String> {
    enhanced_artist_banner_with_position(source, zoom, 0.0, vertical, output)
}

fn enhanced_artist_banner_with_position(
    source: &[u8],
    zoom: f32,
    horizontal: f32,
    vertical: f32,
    output: &splined_core::config::OutputConfig,
) -> Result<image::DynamicImage, String> {
    let decoded = image::load_from_memory(source)
        .map_err(|error| format!("Unable to decode banner source: {error}"))?;
    Ok(apply_selected_image_adjustments(
        image::DynamicImage::ImageRgba8(compose_artist_banner_with_position(&decoded, zoom, horizontal, vertical)),
        output,
    ))
}

fn auto_artist_banner_zoom(path: &Path) -> f32 {
    let Ok((width, height)) = image::image_dimensions(path) else {
        return 1.0;
    };
    if width == 0 || height == 0 {
        return 1.0;
    }
    let image_ratio = width as f32 / height as f32;
    let banner_ratio = 1400.0 / 600.0;
    (image_ratio / banner_ratio)
        .max(banner_ratio / image_ratio)
        .clamp(1.0, 8.0)
}

fn formatted_storage_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.2} GiB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MiB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn directory_storage_bytes(root: &Path, excluded: &[PathBuf]) -> Result<u64, String> {
    let mut total = 0u64;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory)
            .map_err(|error| format!("Unable to measure {}: {error}", directory.display()))?;
        for entry in entries {
            let entry =
                entry.map_err(|error| format!("Unable to measure storage entry: {error}"))?;
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|error| format!("Unable to inspect {}: {error}", path.display()))?;
            if kind.is_dir() && !kind.is_symlink() {
                pending.push(path);
            } else if kind.is_file() && !excluded.contains(&path) {
                total = total.saturating_add(
                    entry
                        .metadata()
                        .map_err(|error| format!("Unable to size {}: {error}", path.display()))?
                        .len(),
                );
            }
        }
    }
    Ok(total)
}

fn storage_usage_text(config: &Config) -> (String, String) {
    let database_dir = Path::new(&config.scan.cache_dir);
    let database_files = [
        "splined.db",
        "splined.db-wal",
        "splined.db-shm",
        "splined.db-journal",
    ]
    .map(|name| database_dir.join(name));
    let database = if !database_files[0].is_file() {
        "Not created".to_string()
    } else {
        database_files
            .iter()
            .try_fold(0u64, |size, path| {
                if path.exists() {
                    fs::metadata(path).map(|metadata| size.saturating_add(metadata.len()))
                } else {
                    Ok(size)
                }
            })
            .map(formatted_storage_size)
            .unwrap_or_else(|_| "Unavailable".to_string())
    };
    let cache_dir = Path::new(&config.scan.temporary_cache_dir);
    let cache = if !cache_dir.is_dir() {
        "Unavailable".to_string()
    } else {
        directory_storage_bytes(cache_dir, &database_files)
            .map(formatted_storage_size)
            .unwrap_or_else(|_| "Unavailable".to_string())
    };
    (cache, database)
}

fn main() -> Result<(), slint::PlatformError> {
    // The software renderer is deliberately selected on Windows. It avoids
    // multi-window OpenGL context loss that previously produced white frames
    // and crashed while opening or maximizing Custom Upscale.
    slint::BackendSelector::new()
        .backend_name("winit".to_string())
        .renderer_name("software".to_string())
        .select()?;
    let restore_request = restore_request_from_args(std::env::args_os().skip(1));
    if let Some(result) = command_line_operation() {
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(2);
        }
        return Ok(());
    }
    if package_identity_smoke_requested() {
        package_identity_smoke();
        return Ok(());
    }
    let automatic_settings_import = restore_request
        .as_deref()
        .map(import_settings_from_opened_backup);
    // A moved portable archive can contain paths unavailable on this PC.
    // Keep its settings as an editable Setup draft instead of abandoning the
    // archive or preventing the application window from opening.
    let staged_archive_config = if automatic_settings_import
        .as_ref()
        .is_some_and(Result::is_err)
    {
        restore_request
            .as_deref()
            .and_then(|path| read_backup_settings(path, "").ok())
    } else {
        None
    };
    let archive_setup_staged = staged_archive_config.is_some();
    let prompt_for_import_client_lock = automatic_settings_import
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .or(staged_archive_config.as_ref())
        .is_some_and(|config| config.scan.sqlite_shared && !config.scan.windows_client_lock);
    let association_warning =
        splined_core::file_association::register_portable_backup_association().err();
    let state_load_started = Instant::now();
    let (mut state, mut setup_required, mut startup_error) = load_application_state();
    if let Some(config) = staged_archive_config {
        let reason = automatic_settings_import
            .as_ref()
            .and_then(|result| result.as_ref().err())
            .cloned()
            .unwrap_or_else(|| "Archive paths need review on this PC.".to_string());
        let snapshot = empty_snapshot(&config);
        state = ApplicationState::new(config, snapshot)
            .expect("a parsed Settings Archive must initialize the Setup model");
        setup_required = true;
        startup_error = Some(format!("Settings Archive loaded for review: {reason}"));
    }
    record_startup_timing(format!(
        "startup-state-load elapsed_ms={}",
        state_load_started.elapsed().as_millis()
    ));
    match prune_old_ui_debug_logs(&state.config) {
        Ok(removed) if removed > 0 => state.push_log(format!(
            "Log retention removed {removed} older Windows UI log file(s)."
        )),
        Err(error) => state.push_log(format!(
            "WARNING: Log retention could not complete: {error}"
        )),
        _ => {}
    }
    let ui_debug_log = initialize_ui_debug_log(&state.config);
    if let Some(log) = ui_debug_log.as_ref() {
        state.push_log(format!("Windows UI debug log: {}", log.display_path()));
        if let Some(events) = STARTUP_TIMING_EVENTS.get()
            && let Ok(mut events) = events.lock()
        {
            for event in events.drain(..) {
                log.record(event);
            }
        }
    }
    let mut preferences = splined_core::settings::load_ui_preferences().unwrap_or_default();
    preferences.migrate_workspace_layout();
    let cleanup_directory = if preferences.backup_directory.trim().is_empty() {
        splined_core::portable::app_layout()
            .ok()
            .map(|layout| layout.root.join("backups"))
    } else {
        Some(PathBuf::from(&preferences.backup_directory))
    };
    if let Some(directory) = cleanup_directory {
        match enforce_backup_file_limits(&directory, &preferences) {
            Ok(removed) if removed > 0 => state.push_log(format!(
                "Backup retention removed {removed} older SPLINED backup file(s)."
            )),
            Err(error) => state.push_log(format!(
                "WARNING: Backup retention could not complete: {error}"
            )),
            _ => {}
        }
    }
    apply_ui_preferences(&mut state, &preferences);
    let mut workspace_layout = WorkspaceLayout::from_preferences(&preferences);
    workspace_layout.clamp_for_window(
        preferences.window_width,
        preferences.window_height,
        state.media_selector_expanded,
        state.show_artwork,
        state.candidate_filter_expanded,
    );
    let workspace_layout = Rc::new(RefCell::new(workspace_layout));
    let custom_window_state = CustomWindowState::from_preferences(&preferences);
    // The artwork workspace is designed to open expanded. A stale normal-window
    // preference (including one captured after a DPI transition) must not bring
    // the next launch back in the collapsed half-screen state.
    let main_window_guard = MaximizeGuard::new(true);
    let appearance = Rc::new(RefCell::new(normalize_appearance(&preferences.appearance)));
    let state = Arc::new(Mutex::new(state));
    if let Some(ref error) = startup_error
        && let Ok(mut state) = state.lock()
    {
        state.last_error = Some(error.clone());
        state.push_log(format!("ERROR: {error}"));
    }
    if let Some(warning) = association_warning
        && let Ok(mut state) = state.lock()
    {
        state.push_log(format!("WARNING: {warning}"));
    }
    if let Some(Ok(_)) = automatic_settings_import.as_ref()
        && let Ok(mut state) = state.lock()
    {
        let message = "Config v5 settings imported from the opened .spl backup.".to_string();
        state.service_status = message.clone();
        state.push_log(message);
    }

    let processing_debug_log = ui_debug_log.as_ref().map(|log| {
        let log = log.clone();
        Arc::new(move |event: String| log.record(event)) as ProcessingDebugSink
    });
    let (controller, notices) =
        ScanController::new_with_debug(Arc::clone(&state), processing_debug_log);
    let controller = Arc::new(controller);
    let ui_create_started = Instant::now();
    let ui = AppWindow::new()?;
    if let Some(log) = ui_debug_log.as_ref() {
        log.record(format!(
            "startup-ui-create elapsed_ms={}",
            ui_create_started.elapsed().as_millis()
        ));
    }
    ui.window().set_maximized(true);
    ui.set_setup_required(setup_required);
    ui.set_import_client_lock_prompt_visible(prompt_for_import_client_lock);
    if let Some(path) = restore_request.as_deref() {
        ui.set_restore_path(SharedString::from(path.to_string_lossy().into_owned()));
        if archive_setup_staged
            || automatic_settings_import
                .as_ref()
                .is_some_and(Result::is_ok)
        {
            ui.set_setup_loaded_archive_path(SharedString::from(
                path.to_string_lossy().into_owned(),
            ));
        }
    }
    if setup_required {
        ui.set_page_index(3);
        ui.set_setup_message(SharedString::from(startup_error.clone().unwrap_or_else(
            || "Config v5 was not found. Complete SPLINED Setup.".to_string(),
        )));
    }
    ui.set_include_bypassed(preferences.include_bypassed);
    ui.set_media_filter_expanded(preferences.media_filter_expanded);
    ui.set_settings_animate_webp_preview(preferences.animate_webp_preview);
    ui.set_musicbrainz_id_editor_expanded(preferences.musicbrainz_id_editor_expanded);
    ui.set_selected_source_name(SharedString::from(preferences.selected_source_name.clone()));
    let default_backup_directory = splined_core::portable::app_layout()
        .map(|layout| layout.root.join("backups").to_string_lossy().into_owned())
        .unwrap_or_else(|_| "backups".to_string());
    let backup_directory = if preferences.backup_directory.trim().is_empty() {
        default_backup_directory
    } else {
        preferences.backup_directory.clone()
    };
    ui.set_backup_directory(SharedString::from(backup_directory.clone()));
    let archived = archived_backup_files(Path::new(&backup_directory));
    let latest = archived
        .first()
        .map(|path| path.to_string_lossy().into_owned());
    let restore_path = latest.unwrap_or_else(|| preferences.restore_path.clone());
    ui.set_restore_path(SharedString::from(restore_path.clone()));
    ui.set_archived_backup_selection(SharedString::from(restore_path));
    ui.set_archived_backup_files(model(
        archived
            .iter()
            .map(|path| SharedString::from(path.to_string_lossy().into_owned()))
            .collect(),
    ));
    ui.set_backup_selected_type(SharedString::from(preferences.backup_selected_type.clone()));
    ui.set_max_settings_backup_files(SharedString::from(
        preferences.max_settings_backup_files.clone(),
    ));
    ui.set_max_database_backup_files(SharedString::from(
        preferences.max_database_backup_files.clone(),
    ));
    ui.set_max_diagnostic_backup_files(SharedString::from(
        preferences.max_diagnostic_backup_files.clone(),
    ));
    ui.set_backup_prompt_dismissed(preferences.backup_prompt_dismissed);
    ui.set_backup_first_run_prompt_visible(
        !setup_required
            && !preferences.backup_prompt_dismissed
            && !archived.iter().any(|path| {
                path.extension()
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("spl"))
            }),
    );
    ui.set_backup_schedule(SharedString::from(normalize_backup_schedule(
        &preferences.backup_schedule,
    )));
    ui.set_database_backup_schedule(SharedString::from(normalize_backup_schedule(
        &preferences.database_backup_schedule,
    )));
    ui.set_diagnostics_backup_schedule(SharedString::from(normalize_backup_schedule(
        &preferences.diagnostics_backup_schedule,
    )));
    ui.set_backup_mode(SharedString::from(
        if preferences.backup_mode.eq_ignore_ascii_case("restore") {
            "Restore"
        } else {
            "Backup"
        },
    ));
    ui.set_backup_settings(preferences.backup_settings);
    ui.set_backup_credentials(preferences.backup_credentials);
    ui.set_backup_database(preferences.backup_database);
    ui.set_backup_artist_images(preferences.backup_artist_images);
    ui.set_restore_artist_images(preferences.restore_artist_images);
    ui.set_backup_interface(preferences.backup_interface);
    ui.set_backup_diagnostics(preferences.backup_diagnostics);
    if preferences.window_width >= 960 && preferences.window_height >= 640 {
        ui.window().set_size(slint::LogicalSize::new(
            preferences.window_width as f32,
            preferences.window_height as f32,
        ));
    }
    ui.set_appearance_mode(SharedString::from(appearance.borrow().clone()));
    ui.set_status_bar_intensity(
        normalize_status_bar_intensity(&preferences.status_bar_intensity).into(),
    );
    ui.set_status_bar_position(
        normalize_status_bar_position(&preferences.status_bar_position).into(),
    );
    ui.set_status_bar_size(normalize_status_bar_size(&preferences.status_bar_size).into());
    ui.set_settings_status_bar_intensity(ui.get_status_bar_intensity());
    ui.set_settings_status_bar_position(ui.get_status_bar_position());
    ui.set_settings_status_bar_size(ui.get_status_bar_size());
    ui.set_workspace_mode(if preferences.workspace_mode == 2 {
        2
    } else {
        0
    });
    ui.set_enhancement_original_width(preferences.enhancement_original_height as f32);
    apply_workspace_layout(&ui, &workspace_layout.borrow());
    initialize_service_ui(&ui, &state);
    if let Some(path) = restore_request
        && let Some(Err(error)) = automatic_settings_import
        && !archive_setup_staged
    {
        ui.set_restore_path(SharedString::from(path.to_string_lossy().into_owned()));
        if setup_required {
            ui.set_page_index(3);
            ui.set_setup_message(format!("Could not read the opened .spl archive: {error} Enter its password in Setup and choose Load into Setup, or enter the paths manually.").into());
        } else {
            ui.set_page_index(2);
            ui.set_backup_mode(SharedString::from("Restore"));
            ui.set_backup_settings(true);
            ui.set_backup_credentials(false);
            ui.set_backup_database(false);
            ui.set_backup_interface(false);
            ui.set_backup_diagnostics(false);
        }
        if let Ok(mut app) = state.lock() {
            app.service_status = format!(
                "Automatic Config v5 import needs attention: {error} Enter the archive password if required, then load its settings."
            );
        }
    }
    sync_ui(&ui, &state);
    let interface_restore_pending_restart = Arc::new(AtomicBool::new(false));
    bind_callbacks(
        &ui,
        Arc::clone(&state),
        Arc::clone(&controller),
        Rc::clone(&workspace_layout),
        Rc::clone(&appearance),
        custom_window_state.clone(),
        UiWindowDiagnostics {
            maximize_guard: main_window_guard.clone(),
            ui_debug_log: ui_debug_log.clone(),
        },
        Arc::clone(&interface_restore_pending_restart),
    );
    let embed_queue_stop = match bind_embed_queue(&ui, Arc::clone(&state), Arc::clone(&controller))
    {
        Ok(stop) => Some(stop),
        Err(error) => {
            if let Ok(mut app) = state.lock() {
                app.push_log(format!("Embed queue unavailable: {error}"));
            }
            None
        }
    };
    if embed_queue_stop.is_some() {
        sync_ui(&ui, &state);
    }
    if !setup_required {
        let check_config = state.lock().ok().map(|app| app.config.clone());
        if let Some(check_config) = check_config {
            let check_weak = ui.as_weak();
            let check_state = Arc::clone(&state);
            let check_revision = NEW_MEDIA_CHECK_REVISION.load(Ordering::SeqCst);
            std::thread::spawn(move || {
                let result = find_unindexed_audio_folders(&check_config);
                let _ = slint::invoke_from_event_loop(move || {
                    if NEW_MEDIA_CHECK_REVISION.load(Ordering::SeqCst) != check_revision {
                        return;
                    }
                    let Some(ui) = check_weak.upgrade() else {
                        return;
                    };
                    match result {
                        Ok(found) if found.count > 0 => {
                            let message = format!(
                                "New music found in {} unindexed folder{} · Refresh DB to add it to the library.",
                                found.count,
                                if found.count == 1 { "" } else { "s" },
                            );
                            ui.set_new_media_warning_text(message.clone().into());
                            ui.set_new_media_warning_visible(true);
                            if let Ok(mut app) = check_state.lock() {
                                app.push_log(message);
                            }
                        }
                        Ok(_) => {}
                        Err(error) => {
                            if let Ok(mut app) = check_state.lock() {
                                app.push_log(format!(
                                    "WARNING: New-music folder check could not finish: {error}"
                                ));
                            }
                        }
                    }
                });
            });
        }
    }
    if let Some(log) = ui_debug_log.as_ref() {
        let (width, height) = app_logical_window_size(&ui);
        log.record(format!(
            "main-created size={width}x{height} maximized={} saved_size={}x{} saved_maximized={}",
            ui.window().is_maximized(),
            preferences.window_width,
            preferences.window_height,
            preferences.window_maximized
        ));
    }
    // Winit does not reliably honor a native maximize request made before its
    // event loop starts. Re-assert the expanded launch state once Slint is
    // dispatching so Windows maximizes against the native work area on the
    // active monitor, regardless of stale saved geometry.
    let restore_maximized_timer = Timer::default();
    let maximize_weak = ui.as_weak();
    restore_maximized_timer.start(
        TimerMode::SingleShot,
        Duration::from_millis(120),
        move || {
            if let Some(ui) = maximize_weak.upgrade() {
                ui.window().set_maximized(true);
            }
        },
    );

    // Windows reports transitional logical sizes while a saved maximized
    // window crosses DPI contexts. Re-apply the complete workspace once the
    // native window has settled so Slint paints the full client area.
    let startup_reflow_timer = Timer::default();
    let startup_reflow_weak = ui.as_weak();
    let startup_reflow_state = Arc::clone(&state);
    let startup_reflow_layout = Rc::clone(&workspace_layout);
    let startup_reflow_log = ui_debug_log.clone();
    startup_reflow_timer.start(
        TimerMode::SingleShot,
        Duration::from_millis(420),
        move || {
            let Some(ui) = startup_reflow_weak.upgrade() else {
                return;
            };
            let (media_expanded, artwork_visible, candidate_filter_expanded) = startup_reflow_state
                .lock()
                .map(|app| {
                    (
                        app.media_selector_expanded,
                        app.show_artwork,
                        app.candidate_filter_expanded,
                    )
                })
                .unwrap_or((true, true, true));
            let (window_width, window_height) = app_logical_window_size(&ui);
            if let Some(log) = startup_reflow_log.as_ref() {
                log.record(format!(
                    "startup-reflow size={window_width}x{window_height} maximized={}",
                    ui.window().is_maximized()
                ));
            }
            {
                let mut layout = startup_reflow_layout.borrow_mut();
                layout.record_normal_window(
                    window_width,
                    window_height,
                    ui.window().is_maximized(),
                );
                layout.clamp_for_window(
                    window_width,
                    window_height,
                    media_expanded,
                    artwork_visible,
                    candidate_filter_expanded,
                );
                apply_workspace_layout(&ui, &layout);
            }
            sync_ui(&ui, &startup_reflow_state);
        },
    );

    let weak = ui.as_weak();
    let timer_state = Arc::clone(&state);
    let notice_controller = Arc::clone(&controller);
    let notice_timer = Timer::default();
    notice_timer.start(TimerMode::Repeated, Duration::from_millis(75), move || {
        let mut changed = false;
        let mut run_finished = false;
        while let Ok(notice) = notices.try_recv() {
            changed = true;
            run_finished |= matches!(notice, ControllerNotice::RunFinished);
        }
        if run_finished {
            // A library navigation can abandon an active review. Once the
            // controller releases its album, restore the newly focused item
            // as the preview authority.
            if let Ok(mut app) = timer_state.lock()
                && app.phase == RunPhase::Idle
                && overview_artist_name(&app).is_some_and(|artist| {
                    !artist.trim().eq_ignore_ascii_case(app.current_artist.trim())
                })
            {
                if app.library.view == LibraryView::Artists {
                    app.synchronize_focused_artist();
                } else {
                    app.synchronize_focused_album();
                }
            }
            // Candidate and COMP actions may change embedded tags; the
            // overview bullets must be recomputed after those actions.
            if let Ok(mut checks) = album_embedded_check_cache().lock() {
                checks.clear();
            }
            if let Ok(app) = timer_state.lock()
                && !app.current_compilation_track_path.is_empty()
            {
                invalidate_embedded_track_metadata(Path::new(&app.current_compilation_track_path));
            }
            let restart = timer_state.lock().is_ok_and(|mut app| {
                let pending = std::mem::take(&mut app.pending_compilation_track_path);
                if pending.is_empty()
                    || !app
                        .library
                        .selected_compilation_track_paths
                        .contains(&pending)
                    || app.phase != RunPhase::Idle
                {
                    return false;
                }
                app.library.focused_album_key = app
                    .library
                    .albums
                    .iter()
                    .find(|album| {
                        album
                            .snapshot
                            .compilation_tracks
                            .iter()
                            .any(|track| track.path.eq_ignore_ascii_case(&pending))
                    })
                    .map(|album| album.snapshot.album_key.clone());
                app.operation_status =
                    "Opening MusicBrainz for the newly selected COMP track…".to_string();
                app.operation_tone = "cyan".to_string();
                true
            });
            if restart && let Err(error) = notice_controller.start() {
                record_action_result(&timer_state, Err(error));
            }
            if !restart {
                let hydration = timer_state.lock().ok().and_then(|mut app| {
                    if !app.library.show_tracks || app.phase != RunPhase::Idle {
                        return None;
                    }
                    if app.library.focused_album_key.is_none() {
                        app.library.focused_album_key =
                            app.library.completed_fallback_return_key.clone();
                    }
                    let key = app.library.focused_album_key.clone()?;
                    app.library.fallback_track_hydration_request(&key)
                });
                if let Some(request) = hydration {
                    hydrate_fallback_album(&timer_state, weak.clone(), request);
                }
            }
        }
        if changed && let Some(ui) = weak.upgrade() {
            if run_finished {
                // Clearing the embedded-art checks above must immediately
                // schedule a fresh scan. A plain sync leaves COMP cards at
                // their provisional blue status until another user click.
                refresh_weak(&weak, &timer_state);
            } else {
                sync_ui(&ui, &timer_state);
            }
        }
    });

    let geometry_timer = Timer::default();
    let geometry_weak = ui.as_weak();
    let geometry_state = Arc::clone(&state);
    let geometry_layout = Rc::clone(&workspace_layout);
    let geometry_maximize_guard = main_window_guard.clone();
    geometry_timer.start(TimerMode::Repeated, Duration::from_millis(200), move || {
        let Some(ui) = geometry_weak.upgrade() else {
            return;
        };
        let (window_width, window_height) = app_logical_window_size(&ui);
        let maximized = ui.window().is_maximized();
        if repair_main_maximized_rect(&ui, &geometry_maximize_guard) {
            return;
        }
        let flags = geometry_state.lock().ok().map(|state| {
            (
                state.media_selector_expanded,
                state.show_artwork,
                state.candidate_filter_expanded,
            )
        });
        let Some((media_expanded, artwork_visible, candidate_filter_expanded)) = flags else {
            return;
        };
        let mut layout = geometry_layout.borrow_mut();
        let previous = *layout;
        layout.record_normal_window(window_width, window_height, maximized);
        layout.clamp_for_window(
            window_width,
            window_height,
            media_expanded,
            artwork_visible,
            candidate_filter_expanded,
        );
        if *layout != previous {
            apply_workspace_layout(&ui, &layout);
        }
    });

    // Persist interface choices while the app is running, not only during a
    // clean shutdown. That keeps every changed filter, panel, and window
    // preference after a renderer or driver failure.
    let preference_timer = Timer::default();
    let preference_weak = ui.as_weak();
    let preference_state = Arc::clone(&state);
    let preference_layout = Rc::clone(&workspace_layout);
    let preference_appearance = Rc::clone(&appearance);
    let preference_custom_window = custom_window_state.clone();
    let last_preferences = Rc::new(RefCell::new(preferences.clone()));
    let timer_last_preferences = Rc::clone(&last_preferences);
    let timer_restore_pending = Arc::clone(&interface_restore_pending_restart);
    let diagnostic_last_poll = Rc::new(RefCell::new(Instant::now() - Duration::from_secs(3)));
    let diagnostic_last_level = Rc::new(RefCell::new(String::new()));
    preference_timer.start(TimerMode::Repeated, Duration::from_millis(600), move || {
        let Some(ui) = preference_weak.upgrade() else {
            return;
        };
        if ui.get_setup_required() {
            return;
        }
        // A restored ui.toml becomes authoritative on the next launch. The
        // current window must never save its pre-restore controls over it.
        if timer_restore_pending.load(Ordering::SeqCst) {
            return;
        }
        let section = ui.get_settings_section().to_string();
        let dirty = ui.get_page_index() == 1
            && preference_state.lock().ok().is_some_and(|app| {
                let mut proposed = app.config.clone();
                apply_settings_section_ui(&ui, &mut proposed, &section)
                    .map(|_| proposed != app.config)
                    .unwrap_or(true)
                    || (section == "Library"
                        && (ui.get_settings_disable_intro_message() != app.disable_intro_message
                            || ui.get_settings_disable_confirmation_messages()
                                != app.disable_confirmation_messages))
                    || (section == "Enhancement" && app.enhancement_settings_dirty)
            });
        ui.set_settings_section_dirty(dirty);
        if ui.get_page_index() == 1 && section == "Runtime" {
            let level = ui.get_settings_verbosity().to_string();
            if level != "Current"
                && (level != *diagnostic_last_level.borrow()
                    || diagnostic_last_poll.borrow().elapsed() >= Duration::from_secs(3))
            {
                let backup_directory = PathBuf::from(ui.get_backup_directory().as_str());
                if let Ok(app) = preference_state.lock() {
                    ui.set_diagnostic_log_detail(SharedString::from(diagnostic_log_view(
                        &app,
                        &level,
                        &backup_directory,
                    )));
                }
                *diagnostic_last_poll.borrow_mut() = Instant::now();
                *diagnostic_last_level.borrow_mut() = level;
            }
        }
        let Some(mut current) = capture_ui_preferences(
            &ui,
            &preference_state,
            &preference_layout,
            &preference_appearance,
            &preference_custom_window,
        ) else {
            return;
        };
        if preference_state
            .lock()
            .ok()
            .is_some_and(|app| app.enhancement_settings_dirty)
        {
            let saved = timer_last_preferences.borrow();
            current.adjustment_presets = saved.adjustment_presets.clone();
            current.banner_theme_styles = saved.banner_theme_styles.clone();
            current.album_frame_theme_styles = saved.album_frame_theme_styles.clone();
            current.banner_sync_frame_letters = saved.banner_sync_frame_letters;
            current.banner_random_harmony = saved.banner_random_harmony;
            current.wheel_hue_shift_degrees = saved.wheel_hue_shift_degrees;
            current.wheel_brightness_percent = saved.wheel_brightness_percent;
            current.banner_theme_baseline_styles = saved.banner_theme_baseline_styles.clone();
            current.album_frame_theme_baseline_styles =
                saved.album_frame_theme_baseline_styles.clone();
        }
        if current != *timer_last_preferences.borrow()
            && splined_core::settings::save_ui_preferences(&current).is_ok()
        {
            *timer_last_preferences.borrow_mut() = current;
        }
    });

    // Decode only the selected album's animation, off the UI thread. Slint's
    // normal image path remains the static fallback and all writes stay still.
    let webp_timer = Timer::default();
    let webp_weak = ui.as_weak();
    let webp_state = Arc::clone(&state);
    let webp_pending = Arc::new(Mutex::new(
        None::<(u64, Result<Vec<DecodedWebpFrame>, String>)>,
    ));
    let webp_revision = Arc::new(AtomicU64::new(0));
    let webp_current_path = Rc::new(RefCell::new(String::new()));
    let webp_playback = Rc::new(RefCell::new(None::<WebpPlayback>));
    let webp_log = ui_debug_log.clone();
    webp_timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
        let Some(ui) = webp_weak.upgrade() else {
            return;
        };
        let desired = webp_state.lock().ok().and_then(|app| {
            (app.animate_webp_preview
                && app.phase == RunPhase::Idle
                && !app.current_album_path.is_empty()
                && !app.artist_artwork_mode
                && app.edit_preview_path.is_empty()
                && app.embedded_thumbnail_preview_path.is_empty()
                && ui.get_page_index() == 0)
                .then(|| Path::new(&app.current_album_path).join("cover.webp"))
        });
        let path_text = desired
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        if *webp_current_path.borrow() != path_text {
            *webp_current_path.borrow_mut() = path_text;
            *webp_playback.borrow_mut() = None;
            ui.set_animated_webp_active(false);
            ui.set_animated_webp_loading(false);
            let ticket = webp_revision.fetch_add(1, Ordering::SeqCst) + 1;
            if let Some(path) = desired.filter(|path| path.is_file()) {
                ui.set_animated_webp_loading(true);
                let pending = Arc::clone(&webp_pending);
                let revision = Arc::clone(&webp_revision);
                std::thread::spawn(move || {
                    let result = decode_webp_animation_while(&path, || {
                        revision.load(Ordering::SeqCst) == ticket
                    });
                    if let Ok(mut slot) = pending.lock() {
                        *slot = Some((ticket, result));
                    }
                });
            }
        }
        let completed = webp_pending.lock().ok().and_then(|mut slot| slot.take());
        if let Some((ticket, result)) = completed
            && ticket == webp_revision.load(Ordering::SeqCst)
        {
            ui.set_animated_webp_loading(false);
            match result {
                Ok(decoded) if !decoded.is_empty() => {
                    let frames = decoded
                        .into_iter()
                        .map(|frame| {
                            let buffer =
                                slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                                    frame.pixels.as_raw(),
                                    frame.pixels.width(),
                                    frame.pixels.height(),
                                );
                            (slint::Image::from_rgba8(buffer), frame.delay)
                        })
                        .collect::<Vec<_>>();
                    ui.set_animated_webp_frame(frames[0].0.clone());
                    ui.set_animated_webp_active(true);
                    let started_at = Instant::now();
                    let next_at = started_at + frames[0].1;
                    *webp_playback.borrow_mut() = Some(WebpPlayback {
                        frames,
                        next_frame: 1,
                        next_at,
                        started_at,
                    });
                }
                Err(error) => {
                    if let Some(log) = webp_log.as_ref() {
                        log.record(format!("animated-webp-preview unavailable: {error}"));
                    }
                }
                _ => {}
            }
        }
        let mut playback = webp_playback.borrow_mut();
        if let Some(active) = playback.as_mut()
            && Instant::now() >= active.next_at
        {
            if active.started_at.elapsed() < Duration::from_secs(5) {
                let index = active.next_frame % active.frames.len();
                ui.set_animated_webp_frame(active.frames[index].0.clone());
                active.next_at = Instant::now() + active.frames[index].1;
                active.next_frame += 1;
            } else {
                *playback = None;
                ui.set_animated_webp_active(false);
                ui.set_animated_webp_frame(slint::Image::default());
            }
        }
    });

    let storage_timer = Timer::default();
    let storage_weak = ui.as_weak();
    let storage_state = Arc::clone(&state);
    let storage_busy = Arc::new(AtomicBool::new(false));
    let storage_last_refresh = Rc::new(Cell::new(Instant::now() - Duration::from_secs(60)));
    storage_timer.start(TimerMode::Repeated, Duration::from_secs(2), move || {
        let Some(ui) = storage_weak.upgrade() else {
            return;
        };
        if ui.get_setup_required()
            || ui.get_page_index() != 1
            || ui.get_settings_section() != "Library"
        {
            return;
        }
        if storage_last_refresh.get().elapsed() < Duration::from_secs(30)
            || storage_busy.swap(true, Ordering::SeqCst)
        {
            return;
        }
        let Some(config) = storage_state.lock().ok().map(|app| app.config.clone()) else {
            storage_busy.store(false, Ordering::SeqCst);
            return;
        };
        storage_last_refresh.set(Instant::now());
        let weak = storage_weak.clone();
        let busy = Arc::clone(&storage_busy);
        std::thread::spawn(move || {
            let (cache, database) = storage_usage_text(&config);
            let reset = Arc::clone(&busy);
            if slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    ui.set_storage_cache_usage(cache.into());
                    ui.set_storage_database_usage(database.into());
                }
                reset.store(false, Ordering::SeqCst);
            })
            .is_err()
            {
                busy.store(false, Ordering::SeqCst);
            }
        });
    });

    let scheduled_backup_timer = Timer::default();
    let scheduled_backup_weak = ui.as_weak();
    let scheduled_backup_state = Arc::clone(&state);
    let scheduled_backup_active = Arc::new(AtomicBool::new(false));
    let scheduled_backup_guard = Arc::clone(&scheduled_backup_active);
    let scheduled_restore_pending = Arc::clone(&interface_restore_pending_restart);
    let scheduled_layout = Rc::clone(&workspace_layout);
    let scheduled_appearance = Rc::clone(&appearance);
    let scheduled_custom_window = custom_window_state.clone();
    scheduled_backup_timer.start(TimerMode::Repeated, Duration::from_secs(60), move || {
        if scheduled_restore_pending.load(Ordering::SeqCst) {
            return;
        }
        let Some(ui) = scheduled_backup_weak.upgrade() else {
            return;
        };
        if ui.get_setup_required() {
            return;
        }
        let directory = PathBuf::from(ui.get_backup_directory().as_str());
        let settings_schedule = normalize_backup_schedule(ui.get_backup_schedule().as_str());
        let database_schedule =
            normalize_backup_schedule(ui.get_database_backup_schedule().as_str());
        let backup_artist_images = ui.get_backup_artist_images();
        let diagnostics_schedule =
            normalize_backup_schedule(ui.get_diagnostics_backup_schedule().as_str());
        let mut settings_due = scheduled_backup_state
            .lock()
            .is_ok_and(|app| !app.enhancement_settings_dirty && !unsaved_adjustment_defaults(&app))
            && automatic_backup_is_due(
                &directory,
                settings_schedule,
                "splined-settings-auto",
                "spl",
            );
        let database_due = automatic_backup_is_due(
            &directory,
            database_schedule,
            "splined-database-auto",
            "sqlite",
        );
        let diagnostics_due = automatic_backup_is_due(
            &directory,
            diagnostics_schedule,
            "splined-diagnostics-auto",
            "json",
        );
        if settings_due {
            let snapshot = capture_ui_preferences(
                &ui,
                &scheduled_backup_state,
                &scheduled_layout,
                &scheduled_appearance,
                &scheduled_custom_window,
            )
            .ok_or_else(|| {
                "Unable to snapshot Interface Settings for automatic backup.".to_string()
            })
            .and_then(|preferences| splined_core::settings::save_ui_preferences(&preferences));
            if let Err(error) = snapshot {
                settings_due = false;
                if let Ok(mut app) = scheduled_backup_state.lock() {
                    app.push_log(format!(
                        "WARNING: Automatic Settings Archive deferred: {error}"
                    ));
                }
            }
        }
        if (!settings_due && !database_due && !diagnostics_due)
            || scheduled_backup_guard.swap(true, Ordering::SeqCst)
        {
            return;
        }
        let selection = BackupSelection {
            settings: ui.get_backup_settings(),
            credentials: ui.get_backup_credentials(),
            database: false,
            interface: ui.get_backup_interface(),
            diagnostics: false,
        };
        let config = scheduled_backup_state
            .lock()
            .ok()
            .and_then(|app| (app.phase == RunPhase::Idle).then(|| app.config.clone()));
        let Some(config) = config else {
            scheduled_backup_guard.store(false, Ordering::SeqCst);
            return;
        };
        let weak = scheduled_backup_weak.clone();
        let state = Arc::clone(&scheduled_backup_state);
        let guard = Arc::clone(&scheduled_backup_guard);
        std::thread::spawn(move || {
            let mut created = Vec::new();
            let mut failed = Vec::new();
            if settings_due {
                match backup_file_in_directory(&directory, "splined-settings-auto", "spl")
                    .and_then(|path| export_backup(&path, &config, selection, ""))
                {
                    Ok(path) => created.push(path),
                    Err(error) => failed.push(format!("settings: {error}")),
                }
            }
            if database_due {
                match backup_file_in_directory(&directory, "splined-database-auto", "sqlite")
                    .and_then(|path| {
                        export_database_backup_with_artist_art(&path, &config, backup_artist_images)
                    }) {
                    Ok(path) => created.push(path),
                    Err(error) => failed.push(format!("SQLite: {error}")),
                }
            }
            if diagnostics_due {
                match backup_file_in_directory(&directory, "splined-diagnostics-auto", "json")
                    .and_then(|path| export_diagnostic_summary(&path, &config))
                {
                    Ok(path) => created.push(path),
                    Err(error) => failed.push(format!("diagnostics: {error}")),
                }
            }
            if let Ok(mut app) = state.lock() {
                if !created.is_empty() {
                    let message = format!(
                        "Automatic backup created: {}",
                        created
                            .iter()
                            .map(|path| path.display().to_string())
                            .collect::<Vec<_>>()
                            .join(" · ")
                    );
                    app.service_status = message.clone();
                    app.push_log(message);
                }
                if !failed.is_empty() {
                    let message = format!("Automatic backup failed: {}", failed.join(" · "));
                    app.service_status = message.clone();
                    app.push_log(format!("ERROR: {message}"));
                }
            }
            guard.store(false, Ordering::SeqCst);
            let _ = slint::invoke_from_event_loop(move || refresh_weak(&weak, &state));
        });
    });

    if !ui.get_setup_required() {
        let album_directories = state
            .lock()
            .ok()
            .map(|app| startup_stage_cleanup_paths(&app))
            .unwrap_or_default();
        if !album_directories.is_empty() {
            let cleanup_state = Arc::clone(&state);
            let cleanup_weak = ui.as_weak();
            let cleanup_log = ui_debug_log.clone();
            std::thread::spawn(move || {
                // A separate Windows client may be writing to the same share.
                // Only an old stage that no process still has open is eligible.
                let report =
                    cleanup_stale_audio_stages(&album_directories, Duration::from_secs(2 * 60));
                if report.removed > 0 || report.errors > 0 {
                    let message = format!(
                        "Startup stage cleanup: {} removed, {} recent, {} busy, {} directory/file errors.",
                        report.removed, report.skipped_recent, report.skipped_busy, report.errors
                    );
                    if let Some(log) = cleanup_log.as_ref() {
                        log.record(&message);
                    }
                    if let Ok(mut app) = cleanup_state.lock() {
                        app.push_log(message);
                    }
                    let _ = slint::invoke_from_event_loop(move || {
                        refresh_weak(&cleanup_weak, &cleanup_state)
                    });
                }
            });
        }
    }

    let result = ui.run();
    if let Some(stop) = embed_queue_stop {
        stop.store(true, Ordering::Relaxed);
    }
    let (window_width, window_height) = app_logical_window_size(&ui);
    workspace_layout.borrow_mut().record_normal_window(
        window_width,
        window_height,
        ui.window().is_maximized(),
    );
    if !ui.get_setup_required()
        && !interface_restore_pending_restart.load(Ordering::SeqCst)
        && let Some(preferences) = capture_ui_preferences(
            &ui,
            &state,
            &workspace_layout,
            &appearance,
            &custom_window_state,
        )
        && let Err(error) = splined_core::settings::save_ui_preferences(&preferences)
    {
        eprintln!("Unable to save SPLINED interface preferences: {error}");
    }
    result
}

fn startup_stage_cleanup_paths(app: &ApplicationState) -> Vec<PathBuf> {
    let root = Path::new(&app.library.local_root);
    if app.library.local_root.trim().is_empty() || !root.is_absolute() {
        return Vec::new();
    }
    let mut paths = app
        .library
        .albums
        .iter()
        .map(|album| PathBuf::from(&album.snapshot.path))
        .filter(|path| path != root && path.starts_with(root))
        .collect::<BTreeSet<_>>();
    paths.insert(std::env::temp_dir());
    paths.into_iter().collect()
}

fn normalize_status_bar_intensity(value: &str) -> &'static str {
    match value {
        "Aggressive" => "Aggressive",
        "Mild" => "Mild",
        "Off" => "Off",
        _ => "Full",
    }
}

fn normalize_status_bar_position(value: &str) -> &'static str {
    match value {
        "Middle" => "Middle",
        "Bottom" => "Bottom",
        _ => "Top",
    }
}

fn normalize_status_bar_size(value: &str) -> &'static str {
    match value {
        "Medium" => "Medium",
        "Large" => "Large",
        _ => "Small",
    }
}

fn preserve_filter_state(replacement: &mut ApplicationState, previous: &ApplicationState) {
    replacement.media_filter_locked = previous.media_filter_locked;
    replacement.candidate_filter_locked = previous.candidate_filter_locked;
    replacement.library.set_view(previous.library.view);
    if previous.media_filter_locked {
        replacement
            .library
            .set_artist_filter(previous.library.artist_filter.clone());
        replacement
            .library
            .set_album_filter(previous.library.album_filter.clone());
        replacement
            .library
            .set_status_filters(previous.library.status_filters.iter().copied());
        replacement
            .library
            .set_show_tracks(previous.library.show_tracks);
    }
    if previous.candidate_filter_locked {
        replacement.candidate_included_sources = previous.candidate_included_sources.clone();
        replacement.candidate_included_types = previous.candidate_included_types.clone();
        replacement.candidate_included_policies = previous.candidate_included_policies.clone();
        replacement.candidate_included_ranges = previous.candidate_included_ranges.clone();
        replacement.candidate_excluded_sources = previous.candidate_excluded_sources.clone();
        replacement.candidate_excluded_types = previous.candidate_excluded_types.clone();
        replacement.candidate_excluded_policies = previous.candidate_excluded_policies.clone();
        replacement.candidate_excluded_ranges = previous.candidate_excluded_ranges.clone();
    }
}

fn apply_ui_preferences(state: &mut ApplicationState, preferences: &UiPreferences) {
    state.artist_lookup_overrides = preferences
        .artist_lookup_overrides
        .iter()
        .filter(|entry| !entry.artist.trim().is_empty())
        .map(|entry| (entry.artist.trim().to_lowercase(), entry.clone()))
        .collect();
    state
        .library
        .set_view(match preferences.library_view.as_str() {
            "albums" => LibraryView::Albums,
            _ => LibraryView::Artists,
        });
    if preferences.status_filters.is_empty() {
        state
            .library
            .set_status_filter(parse_status_filter(&preferences.status_filter));
    } else {
        state.library.set_status_filters(
            preferences
                .status_filters
                .iter()
                .map(|value| parse_status_filter(value)),
        );
    }
    state
        .library
        .set_artist_filter(preferences.artist_filter.clone());
    state
        .library
        .set_album_filter(preferences.album_filter.clone());
    state.library.set_show_tracks(preferences.show_tracks);
    state.auto_scan_enabled = preferences.auto_scan_enabled;
    state.scan_scope = if preferences.auto_scan_scope.eq_ignore_ascii_case("all") {
        ScanScope::All
    } else {
        ScanScope::Selected
    };
    state.hover_enabled = false;
    state.media_selector_expanded = preferences.media_selector_expanded;
    state.candidate_filter_expanded =
        preferences.candidate_filter_expanded && !preferences.media_filter_expanded;
    state.artwork_editing_expanded = preferences.artwork_editing_expanded;
    state.show_artwork = preferences.show_artwork;
    state.upscale_preview_only = true;
    state.disable_intro_message = preferences.disable_intro_message;
    state.disable_confirmation_messages = preferences.disable_confirmation_messages;
    state.intro_dismissed = preferences.disable_intro_message;
    state.adjustments.preserve_source_resolution = false;
    state.adjustments.apply_edit_profile = true;
    state.clear_candidate_filters();
    state.media_filter_locked = preferences.media_filter_locked;
    state.candidate_filter_locked = preferences.candidate_filter_locked;
    state.animate_webp_preview = preferences.animate_webp_preview;
    if state.candidate_filter_locked {
        state.candidate_included_sources = preferences
            .candidate_included_sources
            .iter()
            .cloned()
            .collect();
        state.candidate_included_types = preferences
            .candidate_included_types
            .iter()
            .cloned()
            .collect();
        state.candidate_included_policies = preferences
            .candidate_included_policies
            .iter()
            .cloned()
            .collect();
        state.candidate_included_ranges = preferences
            .candidate_included_ranges
            .iter()
            .cloned()
            .collect();
        state.candidate_excluded_sources = preferences
            .candidate_excluded_sources
            .iter()
            .cloned()
            .collect();
        state.candidate_excluded_types = preferences
            .candidate_excluded_types
            .iter()
            .cloned()
            .collect();
        state.candidate_excluded_policies = preferences
            .candidate_excluded_policies
            .iter()
            .cloned()
            .collect();
        state.candidate_excluded_ranges = preferences
            .candidate_excluded_ranges
            .iter()
            .cloned()
            .collect();
    }
    state.set_primary_adjustments(&preferences.primary_adjustments);
    state.adjustment_presets = if preferences.adjustment_presets.len() == 5 {
        preferences.adjustment_presets.clone()
    } else {
        splined_core::settings::default_adjustment_presets()
    };
    state.banner_theme_styles = if preferences.banner_theme_styles.len() == 5 {
        preferences.banner_theme_styles.clone()
    } else {
        splined_core::settings::default_banner_theme_styles()
    };
    state.album_frame_theme_styles = if preferences.album_frame_theme_styles.len() == 3 {
        preferences.album_frame_theme_styles.clone()
    } else {
        splined_core::settings::default_album_frame_theme_styles()
    };
    state.banner_sync_frame_letters = preferences.banner_sync_frame_letters;
    state.banner_random_harmony = preferences.banner_random_harmony;
    state.wheel_hue_shift_degrees = preferences.wheel_hue_shift_degrees.min(359);
    state.wheel_brightness_percent = preferences.wheel_brightness_percent.clamp(30, 170);
    state.banner_theme_baseline_styles = if preferences.banner_theme_baseline_styles.len() == 5 {
        preferences.banner_theme_baseline_styles.clone()
    } else {
        state.banner_theme_styles.clone()
    };
    state.album_frame_theme_baseline_styles =
        if preferences.album_frame_theme_baseline_styles.len() == 3 {
            preferences.album_frame_theme_baseline_styles.clone()
        } else {
            state.album_frame_theme_styles.clone()
        };
    state.appearance_mode = preferences.appearance.clone();
}

fn parse_status_filter(value: &str) -> StatusFilter {
    match value.trim().to_ascii_lowercase().as_str() {
        "unprocessed" => StatusFilter::Unprocessed,
        "incomplete" => StatusFilter::Incomplete,
        "processed" => StatusFilter::Processed,
        "bypassed" => StatusFilter::Bypassed,
        "partial / error" | "partial / timeout" | "partial" | "timeout" => {
            StatusFilter::PartialTimeout
        }
        "artist complete" => StatusFilter::ArtistComplete,
        "artist contains bypass" | "artist bypass" => StatusFilter::ArtistContainsBypass,
        _ => StatusFilter::All,
    }
}

fn synchronize_fallback_album(app: &mut ApplicationState) {
    let browsing_during_run = app.phase != RunPhase::Idle;
    app.synchronize_focused_album();
    // Fallback review is track-scoped. A folder-level cover.* must never leak
    // into the Selected Artwork pane while the operator is deciding which
    // embedded image belongs to an individual track. During an active run,
    // however, these paths belong to the immutable review transaction rather
    // than the library row being browsed.
    if !browsing_during_run {
        app.current_cover_path.clear();
        app.edit_preview_path.clear();
    }
}

fn activate_compilation_track(
    state: &Arc<Mutex<ApplicationState>>,
    weak: &slint::Weak<AppWindow>,
    album_key: &str,
    track_path: &str,
    include_bypassed: bool,
) {
    let preview_request = if let Ok(mut app) = state.lock() {
        let result = app
            .library
            .toggle_compilation_track(album_key, track_path, include_bypassed);
        if result != SelectionResult::NotFound {
            synchronize_fallback_album(&mut app);
        }
        match result {
            SelectionResult::Changed => {
                let normalized_track = track_path.to_ascii_lowercase();
                if app
                    .library
                    .selected_compilation_track_paths
                    .contains(&normalized_track)
                {
                    app.compilation_track_preview_owner = normalized_track;
                    app.compilation_track_preview_path.clear();
                    app.artist_artwork_mode = false;
                    app.operation_status = format!(
                        "Loading COMP track · {} · matching recording and releases…",
                        Path::new(track_path)
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or(track_path)
                    );
                    app.operation_tone = "cyan".to_string();
                    Some((
                        track_path.to_string(),
                        app.config.scan.temporary_cache_dir.clone(),
                    ))
                } else {
                    app.edit_preview_path.clear();
                    app.clear_compilation_track_preview(track_path);
                    app.push_log(format!("Compilation track deselected: {track_path}"));
                    None
                }
            }
            SelectionResult::NeedsBypassOverride(album) => {
                app.push_log(format!(
                    "{album} is bypassed. Enable the one-run bypass override to reopen this track."
                ));
                None
            }
            SelectionResult::BlockedByTimeout(album) => {
                app.push_log(format!(
                    "{album} is protected by its active processing timeout."
                ));
                None
            }
            SelectionResult::NotFound => {
                app.push_log("The compilation track is no longer available.".to_string());
                None
            }
        }
    } else {
        None
    };
    if let Some((track_path, cache_dir)) = preview_request {
        let state = Arc::clone(state);
        let weak = weak.clone();
        // Reading embedded artwork from a NAS can take seconds. Keep it off
        // the Slint event loop so the COMP loading status can repaint first.
        std::thread::spawn(move || {
            let result = embedded_candidate(Path::new(&track_path), Path::new(&cache_dir))
                .map(|candidate| candidate.map(|candidate| candidate.path().to_path_buf()));
            let _ = slint::invoke_from_event_loop(move || {
                if let Ok(mut app) = state.lock() {
                    if app.compilation_track_preview_owner != track_path.to_ascii_lowercase()
                        || app.phase == RunPhase::AwaitingMusicBrainz
                    {
                        return;
                    }
                    match result {
                        Ok(Some(path)) => {
                            app.set_compilation_track_preview(&track_path, &path);
                            app.push_log(format!("Compilation track selected: {track_path}"));
                        }
                        Ok(None) => app.push_log(format!(
                            "Compilation track has no embedded artwork: {track_path}"
                        )),
                        Err(error) => app.push_log(format!(
                            "Unable to preview COMP track {}: {error}",
                            Path::new(&track_path).display()
                        )),
                    }
                }
                refresh_weak(&weak, &state);
            });
        });
    }
}

fn cached_compilation_report_thumb(
    audio_path: &Path,
    cache_dir: &Path,
) -> Result<Option<PathBuf>, String> {
    let metadata = fs::metadata(audio_path)
        .map_err(|error| format!("Unable to inspect COMP track for preview: {error}"))?;
    let audio_modified = metadata.modified().ok();
    let key = format!(
        "{}|{}",
        audio_path.to_string_lossy().to_ascii_lowercase(),
        cache_dir.to_string_lossy().to_ascii_lowercase()
    );
    let cache = COMP_REPORT_THUMB_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(cache) = cache.lock()
        && let Some(cached) = cache.get(&key)
        && cached.audio_len == metadata.len()
        && cached.audio_modified == audio_modified
        && cached.image_path.as_ref().is_none_or(|path| path.is_file())
    {
        cache_compilation_report_thumb_metadata(
            audio_path,
            &metadata,
            cached.image_path.as_deref(),
        );
        return Ok(cached.image_path.clone());
    }
    let image_path =
        embedded_candidate(audio_path, cache_dir)?.map(|candidate| candidate.path().to_path_buf());
    if let Ok(mut cache) = cache.lock() {
        if cache.len() >= 256 {
            cache.clear();
        }
        cache.insert(
            key,
            CachedCompReportThumb {
                audio_len: metadata.len(),
                audio_modified,
                image_path: image_path.clone(),
            },
        );
    }
    cache_compilation_report_thumb_metadata(audio_path, &metadata, image_path.as_deref());
    Ok(image_path)
}

fn cache_compilation_report_thumb_metadata(
    audio_path: &Path,
    audio_metadata: &fs::Metadata,
    image_path: Option<&Path>,
) {
    let dimensions = image_path.and_then(|path| {
        let image = inspect_image(path).ok()?;
        let bytes = fs::metadata(path).ok()?.len();
        Some((image.width, image.height, bytes))
    });
    let modified = audio_metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    if let Ok(mut cache) = EMBEDDED_METADATA_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        cache.insert(
            audio_path.to_string_lossy().into_owned(),
            (audio_metadata.len(), modified, dimensions),
        );
    }
}

fn preview_compilation_report_track(
    weak: &slint::Weak<AppWindow>,
    state: &Arc<Mutex<ApplicationState>>,
    album_key: &str,
    track_path: &str,
) {
    let preview_request = if let Ok(mut app) = state.lock() {
        let fallback_report = app.library.show_tracks
            && app
                .library
                .focused_album_key
                .as_deref()
                .is_some_and(|key| key.eq_ignore_ascii_case(album_key));
        let valid_track = if fallback_report {
            app.library.albums.iter().any(|album| {
                album.snapshot.album_key.eq_ignore_ascii_case(album_key)
                    && album
                        .snapshot
                        .compilation_tracks
                        .iter()
                        .any(|track| track.path.eq_ignore_ascii_case(track_path))
            })
        } else {
            artist_compilation_report_tracks(&app).iter().any(|track| {
                track.album_key.eq_ignore_ascii_case(album_key)
                    && track
                        .path
                        .to_string_lossy()
                        .eq_ignore_ascii_case(track_path)
            })
        };
        if !app.compilation_report_preview_mode
            || (!fallback_report && app.compilation_report_artist.is_empty())
            || app.phase != RunPhase::Idle
            || app.operation_busy
            || !valid_track
        {
            None
        } else {
            // Preview is read-only: do not change album focus or the track
            // selected for processing. On a network library, changing focus
            // also causes an unnecessary scan of every track in that album.
            app.compilation_report_preview_album_key = album_key.to_string();
            app.compilation_report_preview_track_path = track_path.to_string();
            app.compilation_track_preview_owner = track_path.to_ascii_lowercase();
            app.compilation_track_preview_path.clear();
            app.operation_status = "COMP preview · loading saved embedded thumb…".to_string();
            app.operation_tone = "cyan".to_string();
            Some((
                track_path.to_string(),
                app.config.scan.temporary_cache_dir.clone(),
            ))
        }
    } else {
        None
    };
    if let Some((track_path, cache_dir)) = preview_request {
        let state = Arc::clone(state);
        let weak = weak.clone();
        let album_key = album_key.to_string();
        std::thread::spawn(move || {
            let preview =
                cached_compilation_report_thumb(Path::new(&track_path), Path::new(&cache_dir));
            let _ = slint::invoke_from_event_loop(move || {
                if let Ok(mut app) = state.lock() {
                    if !app.compilation_report_preview_mode
                        || !app
                            .compilation_report_preview_album_key
                            .eq_ignore_ascii_case(&album_key)
                        || !app
                            .compilation_report_preview_track_path
                            .eq_ignore_ascii_case(&track_path)
                    {
                        return;
                    }
                    match preview {
                        Ok(Some(path)) => {
                            app.set_compilation_track_preview(&track_path, &path);
                            app.operation_status =
                                "COMP preview · saved embedded thumb".to_string();
                            app.operation_tone = "green".to_string();
                        }
                        Ok(None) => {
                            app.operation_status =
                                "COMP preview · no saved embedded thumb".to_string();
                            app.operation_tone = "orange".to_string();
                        }
                        Err(error) => {
                            app.operation_status = format!("COMP preview unavailable · {error}");
                            app.operation_tone = "orange".to_string();
                        }
                    }
                }
                refresh_compilation_report_preview(&weak, &state);
            });
        });
    }
}

fn activate_and_run_compilation_track(
    state: &Arc<Mutex<ApplicationState>>,
    weak: &slint::Weak<AppWindow>,
    controller: &Arc<ScanController>,
    album_key: &str,
    track_path: &str,
    include_bypassed: bool,
) {
    activate_compilation_track(state, weak, album_key, track_path, include_bypassed);
    let controller_active = controller.is_running();
    if controller_active {
        let switching = state.lock().is_ok_and(|mut app| {
            let selected = app
                .library
                .selected_compilation_track_paths
                .contains(&track_path.to_ascii_lowercase());
            if selected
                && !app
                    .current_compilation_track_path
                    .eq_ignore_ascii_case(track_path)
            {
                app.pending_compilation_track_path = track_path.to_ascii_lowercase();
                app.operation_status =
                    "Switching to the selected COMP track; stopping the prior search…".into();
                app.operation_tone = "cyan".into();
                app.push_log(format!(
                    "COMP track changed; ending the prior review before searching {track_path}."
                ));
                true
            } else {
                false
            }
        });
        if switching {
            controller.stop();
        }
    }
    let should_start = state.lock().is_ok_and(|app| {
        !controller_active
            && app.phase == RunPhase::Idle
            && app
                .library
                .selected_compilation_track_paths
                .contains(&track_path.to_ascii_lowercase())
    });
    if should_start && let Err(error) = controller.start() {
        record_action_result(state, Err(error));
    }
}

type FallbackTrackHydration = (String, String, Vec<MediaTrackSnapshot>);
type ArtistArtworkCandidates = Vec<(String, String)>;

fn hydrate_fallback_album(
    state: &Arc<Mutex<ApplicationState>>,
    weak: slint::Weak<AppWindow>,
    (album_key, album_path, cached): FallbackTrackHydration,
) {
    let state = Arc::clone(state);
    std::thread::spawn(move || {
        let result = fallback_tracks_from_folder(Path::new(&album_path), &cached);
        let _ = slint::invoke_from_event_loop(move || {
            match result {
                Ok(tracks) => {
                    if let Ok(mut app) = state.lock()
                        && (app.library.show_tracks
                            || (app.library.view == LibraryView::Artists
                                && app.library.focused_artist.is_some()))
                        && app
                            .library
                            .focused_album_key
                            .as_deref()
                            .is_some_and(|key| key.eq_ignore_ascii_case(&album_key))
                    {
                        replace_fallback_tracks_and_invalidate(&mut app, &album_key, tracks);
                        let rows = app.library.compilation_track_rows();
                        let pending = rows
                            .iter()
                            .filter(|row| !row.embedded_artwork_recorded)
                            .count();
                        app.operation_status = format!(
                            "COMP ready · {} tracks · {pending} pending · use LAUNCH COMP or select a track",
                            rows.len()
                        );
                        app.operation_tone = if pending > 0 { "cyan" } else { "green" }.into();
                    }
                }
                Err(error) => record_action_result(&state, Err(error)),
            }
            refresh_weak(&weak, &state);
        });
    });
}

struct ArtistArtworkSearchResult {
    candidates: ArtistArtworkCandidates,
    warnings: Vec<String>,
    failed_sources: Vec<String>,
}

fn is_artist_art_source(source: &str) -> bool {
    [
        "local",
        "navidrome",
        "fanarttv",
        "lastfm",
        "deezer",
        "discogs",
        "itunes",
    ]
    .iter()
    .any(|candidate| source.eq_ignore_ascii_case(candidate))
}

fn artist_art_query_sources(enabled_sources: &[String], artist_mbid: &str) -> Vec<String> {
    ["fanarttv", "lastfm", "deezer", "discogs", "itunes"]
        .into_iter()
        .filter(|source| {
            (*source != "fanarttv" || !artist_mbid.trim().is_empty())
                && enabled_sources
                    .iter()
                    .any(|enabled| enabled.eq_ignore_ascii_case(source))
        })
        .map(str::to_string)
        .collect()
}

fn artist_art_candidate_source(label: &str) -> &'static str {
    let label = label.trim().to_ascii_lowercase();
    if label.starts_with("local") {
        "local"
    } else if label.starts_with("fanart") {
        "fanarttv"
    } else if label.starts_with("last.fm") || label.starts_with("lastfm") {
        "lastfm"
    } else if label.starts_with("deezer") {
        "deezer"
    } else if label.starts_with("discogs") {
        "discogs"
    } else if label.starts_with("itunes") {
        "itunes"
    } else {
        "navidrome"
    }
}

fn discover_remote_artist_artwork(
    config: &Config,
    artist: &str,
    artist_mbid: &str,
    enabled_sources: &[String],
) -> Result<ArtistArtworkSearchResult, String> {
    let queried = artist_art_query_sources(enabled_sources, artist_mbid);
    let enabled = |source: &str| queried.iter().any(|value| value == source);
    let cache = DownloadCache::new_persistent(
        &Path::new(&config.scan.temporary_cache_dir).join("artist-art-candidates"),
    )?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("Unable to start the Artist Art search runtime: {error}"))?;
    runtime.block_on(async move {
        let mut references = Vec::<ArtworkReference>::new();
        let mut warnings = Vec::new();
        let mut failed_sources = Vec::new();
        if enabled("fanarttv") {
            let provider = splined_core::credential_store::credential_path(config, "Fanart.tv")
                .and_then(|path| FanartTv::new(&path.to_string_lossy()));
            match provider {
                Ok(provider) => match provider.discover_artist(artist_mbid).await {
                    Ok(found) => {
                        // Fanart lists thumbnails before backgrounds. Reserve a
                        // slot for an actual banner even with many portraits.
                        let mut backgrounds = found
                            .iter()
                            .filter(|reference| {
                                reference
                                    .types
                                    .iter()
                                    .any(|kind| kind == "Artist Background")
                            })
                            .take(2)
                            .cloned()
                            .collect::<Vec<_>>();
                        backgrounds.extend(
                            found
                                .into_iter()
                                .filter(|reference| {
                                    !reference
                                        .types
                                        .iter()
                                        .any(|kind| kind == "Artist Background")
                                })
                                .take(10),
                        );
                        references.extend(backgrounds);
                    }
                    Err(error) => {
                        warnings.push(error);
                        failed_sources.push("fanarttv".to_string());
                    }
                },
                Err(error) => {
                    warnings.push(error);
                    failed_sources.push("fanarttv".to_string());
                }
            }
        }
        if enabled("lastfm") {
            let provider = splined_core::credential_store::credential_path(config, "Last.fm")
                .and_then(|path| LastFm::new(&path.to_string_lossy()));
            match provider {
                Ok(provider) => match provider.discover_artist(artist).await {
                    Ok(found) => references.extend(found.into_iter().take(8)),
                    Err(error) => {
                        warnings.push(error);
                        failed_sources.push("lastfm".to_string());
                    }
                },
                Err(error) => {
                    warnings.push(error);
                    failed_sources.push("lastfm".to_string());
                }
            }
        }
        if enabled("deezer") {
            match Deezer::new() {
                Ok(provider) => match provider.discover_artist(artist).await {
                    Ok(found) => references.extend(found),
                    Err(error) => {
                        warnings.push(error);
                        failed_sources.push("deezer".to_string());
                    }
                },
                Err(error) => {
                    warnings.push(error);
                    failed_sources.push("deezer".to_string());
                }
            }
        }
        if enabled("discogs") {
            let provider = splined_core::credential_store::credential_path(config, "Discogs")
                .and_then(|path| Discogs::new(&path.to_string_lossy()));
            match provider {
                Ok(provider) => match provider.discover_artist(artist).await {
                    Ok(found) => references.extend(found),
                    Err(error) => {
                        warnings.push(error);
                        failed_sources.push("discogs".to_string());
                    }
                },
                Err(error) => {
                    warnings.push(error);
                    failed_sources.push("discogs".to_string());
                }
            }
        }
        if enabled("itunes") {
            match ITunes::new() {
                Ok(provider) => match provider.discover_artist(artist).await {
                    Ok(found) => references.extend(found),
                    Err(error) => {
                        warnings.push(error);
                        failed_sources.push("itunes".to_string());
                    }
                },
                Err(error) => {
                    warnings.push(error);
                    failed_sources.push("itunes".to_string());
                }
            }
        }

        let mut downloaded = Vec::new();
        let mut seen_urls = HashSet::new();
        for (index, reference) in references.into_iter().enumerate() {
            if !seen_urls.insert(reference.url.to_ascii_lowercase()) {
                continue;
            }
            let kind = reference
                .types
                .iter()
                .find(|value| !value.eq_ignore_ascii_case("ArtistImage"))
                .cloned()
                .unwrap_or_else(|| "Artist Image".to_string());
            match cache
                .download_candidate(reference.source.clone(), &reference.url, index)
                .await
            {
                Ok(candidate) => downloaded.push((
                    format!("{} {kind}", source_display_name(&reference.source)),
                    candidate.path().to_string_lossy().into_owned(),
                )),
                Err(error) => {
                    warnings.push(format!(
                        "{} Artist Art download failed: {error}",
                        source_display_name(&reference.source)
                    ));
                    if !failed_sources
                        .iter()
                        .any(|source| source == &reference.source)
                    {
                        failed_sources.push(reference.source);
                    }
                }
            }
        }
        Ok(ArtistArtworkSearchResult {
            candidates: downloaded,
            warnings,
            failed_sources,
        })
    })
}

fn load_artist_art_candidates(
    app: &mut ApplicationState,
    artist: &str,
    queried_sources: &[String],
    failed_sources: &[String],
    artist_mbid: &str,
) -> Result<(), String> {
    if artist.trim().is_empty() {
        return Err("Select an artist before finding Artist Art.".to_string());
    }
    let range = Range {
        min: app.config.range.min,
        ideal: app.config.range.ideal,
        max: app.config.range.max,
        ladder: app.config.range.ladder,
    };
    app.candidates.clear();
    app.artist_artwork_mode = true;
    app.selected_candidate = None;
    app.comparison_candidates.clear();
    app.artist_artwork_draft_path.clear();
    app.artist_portrait_preview_candidate_index = None;
    app.artist_portrait_preview_path.clear();
    app.artist_preview_focus = "portrait".to_string();
    app.artist_last_individual_focus = "portrait".to_string();
    app.artist_portrait_radio_active = false;
    app.artist_banner_radio_active = false;
    app.artist_package_overview_enabled = false;
    app.artist_theme_art_enabled = false;
    app.artist_banner_preview_path.clear();
    app.artist_banner_source_path.clear();
    app.artist_banner_preview_candidate_index = None;
    let artwork_candidates = app.artist_overview_artwork_candidates.clone();
    for (offset, (label, path)) in artwork_candidates.iter().enumerate() {
        let info = match inspect_image(Path::new(path)) {
            Ok(info) => info,
            Err(error) => {
                app.push_log(format!("Skipped unavailable Artist Art {label}: {error}"));
                continue;
            }
        };
        let range_class = format!("{:?}", range.classify(info.width.min(info.height)));
        let source = artist_art_candidate_source(label);
        app.candidates.push(CandidateState {
            index: 800_000 + offset,
            source: source.to_string(),
            dimensions: format!("{} × {}", info.width, info.height),
            width: info.width,
            height: info.height,
            projected_width: info.width,
            projected_height: info.height,
            source_range_class: range_class.clone(),
            range_class,
            policy_status: "accept".to_string(),
            policy_reason: label.clone(),
            image_type: if label.to_ascii_lowercase().contains("background")
                || label.to_ascii_lowercase().contains("banner")
            {
                "ArtistBanner".to_string()
            } else {
                "ArtistImage".to_string()
            },
            cache_path: path.clone(),
            url: String::new(),
            local_origin: if source == "local" {
                path.clone()
            } else {
                String::new()
            },
            acceptable: true,
            strict_override_active: false,
            upscaled: false,
            approved: true,
            recommended: offset == 0,
            selected: false,
        });
    }
    for source in &mut app.sources {
        source.candidate_count = app
            .candidates
            .iter()
            .filter(|candidate| candidate.source.eq_ignore_ascii_case(&source.name))
            .count();
        if source.name.eq_ignore_ascii_case("samples") {
            continue;
        }
        if !source.enabled {
            source.status = "Excluded".to_string();
            continue;
        }
        if !is_artist_art_source(&source.name) {
            source.status = "Album-only".to_string();
            continue;
        }
        if source.candidate_count > 0 {
            source.status = "success".to_string();
        } else if failed_sources
            .iter()
            .any(|failed| failed.eq_ignore_ascii_case(&source.name))
        {
            source.status = "error".to_string();
        } else if source.name.eq_ignore_ascii_case("fanarttv") && artist_mbid.trim().is_empty() {
            source.status = "Needs Artist ID".to_string();
        } else if ["local", "navidrome"].contains(&source.name.as_str())
            || queried_sources
                .iter()
                .any(|queried| queried.eq_ignore_ascii_case(&source.name))
        {
            source.status = "zero".to_string();
        }
    }
    if app.candidates.is_empty() {
        return Err(format!(
            "No usable Artist Art images were found for {artist}."
        ));
    }
    app.artist_artwork_mode = true;
    app.artist_portrait_adjustments = app.adjustments.clone();
    app.artist_banner_adjustments = app.adjustments.clone();
    app.artist_theme_adjustments = app.adjustments.clone();
    app.activate_available_candidate_filters();
    app.operation_status = format!(
        "Found {} Artist Art candidate{} for {artist}",
        app.candidates.len(),
        if app.candidates.len() == 1 { "" } else { "s" }
    );
    app.operation_tone = "green".to_string();
    app.push_log(format!(
        "Artist Art candidates loaded for {artist}; select one in Candidate Artwork."
    ));
    Ok(())
}

fn focused_artist_art_target(
    app: &ApplicationState,
    artist: &str,
) -> Result<ArtistArtTarget, String> {
    artist_art_targets_for_albums(
        app.library.albums.iter().map(|album| {
            (
                album.snapshot.artist.as_str(),
                album.snapshot.path.as_str(),
                album.snapshot.album_mbid.as_str(),
                album.snapshot.artist_mbid.as_str(),
            )
        }),
        artist,
        &configured_shared_artist_art_dir(&app.config),
    )
    .into_iter()
    .next()
    .ok_or_else(|| format!("SPLINED could not resolve an Artist Art destination for {artist}."))
}

fn selected_artist_album_embed_sources(
    app: &ApplicationState,
) -> Result<(String, u64, u32, Vec<ArtistAlbumEmbedTarget>, usize), String> {
    let artist = app
        .library
        .focused_artist
        .as_deref()
        .filter(|artist| !artist.trim().is_empty())
        .ok_or_else(|| "Select an Artist and at least one Artist Album first.".to_string())?;
    let selected = app
        .library
        .albums
        .iter()
        .filter(|album| album.selected && album.snapshot.artist.eq_ignore_ascii_case(artist))
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Err("Select one or more Artist Album cards first.".to_string());
    }
    let mut targets = Vec::new();
    let mut skipped = 0;
    for album in selected {
        let snapshot = &album.snapshot;
        let album_path = PathBuf::from(&snapshot.path);
        let cover_path = PathBuf::from(&snapshot.cover_path);
        let cover_is_local = cover_path.parent().is_some_and(|parent| {
            parent
                .to_string_lossy()
                .eq_ignore_ascii_case(&snapshot.path)
        });
        let cover_is_named = cover_path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.to_ascii_lowercase().starts_with("cover."));
        if snapshot.album_mbid.trim().is_empty() || !cover_is_local || !cover_is_named {
            skipped += 1;
            continue;
        }
        targets.push(ArtistAlbumEmbedTarget {
            title: snapshot.title.clone(),
            album_path,
            cover_path,
            track_count: 0,
        });
    }
    if targets.is_empty() {
        return Err("No selected Artist Album has an album-folder cover.* and a Release ID; COMP fallback albums are excluded.".to_string());
    }
    Ok((
        artist.to_string(),
        app.library.selection_epoch,
        app.config.output.embedded_thumbnail_size,
        targets,
        skipped,
    ))
}

fn plan_artist_album_embeds(
    artist: String,
    selection_epoch: u64,
    side: u32,
    candidates: Vec<ArtistAlbumEmbedTarget>,
    mut skipped: usize,
) -> Result<ArtistAlbumEmbedPlan, String> {
    let mut targets = Vec::new();
    for mut candidate in candidates {
        if !candidate.cover_path.is_file() || !candidate.album_path.is_dir() {
            skipped += 1;
            continue;
        }
        if inspect_image(&candidate.cover_path).is_err() {
            skipped += 1;
            continue;
        }
        let Ok(entries) = fs::read_dir(&candidate.album_path) else {
            skipped += 1;
            continue;
        };
        candidate.track_count = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && splined_core::scan::is_supported_audio_file(path))
            .count();
        if candidate.track_count == 0 {
            skipped += 1;
            continue;
        }
        targets.push(candidate);
    }
    if targets.is_empty() {
        return Err(
            "Selected Artist Albums have no readable cover.* and supported tracks to embed."
                .to_string(),
        );
    }
    Ok(ArtistAlbumEmbedPlan {
        artist,
        selection_epoch,
        side,
        targets,
        skipped,
    })
}

fn activate_library_row(
    state: &Arc<Mutex<ApplicationState>>,
    key: &str,
    include_bypassed: bool,
) -> Option<FallbackTrackHydration> {
    if let Ok(mut app) = state.lock() {
        app.dismiss_intro();
        if app.library.show_tracks {
            let result = app.library.select_album_exclusive(key, include_bypassed);
            if result == SelectionResult::Changed {
                synchronize_fallback_album(&mut app);
                app.operation_status =
                    "COMP fallback selected · loading its tracks for review…".into();
                app.operation_tone = "cyan".into();
                return app.library.fallback_track_hydration_request(key);
            }
            record_library_selection_result(&mut app, result, "fallback album");
            return None;
        }
        let mut focuses_album = true;
        let artist_overview_fallback = app.library.view == LibraryView::Artists
            && app.library.focused_artist.is_some()
            && app.library.albums.iter().any(|album| {
                album.snapshot.album_key.eq_ignore_ascii_case(key)
                    && album.snapshot.album_mbid.trim().is_empty()
            });
        if artist_overview_fallback {
            // An Artist Overview album card is navigation into that artist's
            // fallback appearances, not authority to process the whole
            // Various Artists folder. Expand it without selecting the Album;
            // the filtered track rows below are the only selectable targets.
            if !app.library.toggle_fallback_album_focus(key) {
                app.push_log("The selected fallback album is no longer available.".to_string());
                return None;
            }
            close_artist_compilation_report(&mut app);
            let expanded = app
                .library
                .focused_album_key
                .as_ref()
                .is_some_and(|focused| focused.eq_ignore_ascii_case(key));
            if expanded {
                app.operation_status = format!(
                    "COMP opened for {} · loading matching tracks below this album…",
                    app.library.focused_artist.as_deref().unwrap_or("Artist")
                );
                app.operation_tone = "cyan".to_string();
                let selected_track_paths = app.library.selected_compilation_track_paths.clone();
                if let Some(album) = app
                    .library
                    .albums
                    .iter_mut()
                    .find(|album| album.snapshot.album_key.eq_ignore_ascii_case(key))
                {
                    let has_explicit_track =
                        album.snapshot.compilation_tracks.iter().any(|track| {
                            selected_track_paths.contains(&track.path.to_ascii_lowercase())
                        });
                    if !has_explicit_track {
                        album.selected = false;
                        album.bypass_override = false;
                    }
                }
                app.artist_artwork_mode = false;
                app.artist_artwork_draft_path.clear();
                synchronize_fallback_album(&mut app);
                app.push_log(
                    "Fallback album expanded; choose one of this Artist's tracks below."
                        .to_string(),
                );
                return app.library.fallback_track_hydration_request(key);
            }
            app.synchronize_focused_artist();
            return None;
        }
        let result = if app.library.view == LibraryView::Artists {
            let album_row = app
                .library
                .albums
                .iter()
                .any(|album| album.snapshot.album_key.eq_ignore_ascii_case(key));
            if album_row {
                app.library.select_album_exclusive(key, include_bypassed)
            } else {
                focuses_album = false;
                app.library.select_artist_exclusive(key, include_bypassed)
            }
        } else {
            app.library.select_album_exclusive(key, include_bypassed)
        };
        if focuses_album && result != SelectionResult::NotFound {
            app.artist_artwork_mode = false;
            app.artist_artwork_draft_path.clear();
            if artist_overview_fallback {
                synchronize_fallback_album(&mut app);
            } else {
                app.synchronize_focused_album();
            }
        } else if !focuses_album && result != SelectionResult::NotFound {
            app.synchronize_focused_artist();
        }
        let fallback_selected = artist_overview_fallback && result != SelectionResult::NotFound;
        record_library_selection_result(&mut app, result, "library row");
        if fallback_selected {
            return app.library.fallback_track_hydration_request(key);
        }
    }
    None
}

fn record_library_selection_result(
    app: &mut ApplicationState,
    result: SelectionResult,
    row_kind: &str,
) {
    match result {
        SelectionResult::Changed => {}
        SelectionResult::NeedsBypassOverride(album) => app.push_log(format!(
            "{album} is bypassed. Enable the one-run bypass override to select it."
        )),
        SelectionResult::BlockedByTimeout(album) => app.push_log(format!(
            "{album} is protected by its active processing timeout."
        )),
        SelectionResult::NotFound => {
            app.push_log(format!("The selected {row_kind} is no longer available."))
        }
    }
}

fn overview_artist_name(app: &ApplicationState) -> Option<&str> {
    if app.library.show_tracks {
        return None;
    }
    if app.library.view == LibraryView::Artists {
        return app.library.focused_artist.as_deref();
    }
    if app.library.view == LibraryView::Albums {
        return app.library.focused_album_key.as_deref().and_then(|key| {
            app.library
                .albums
                .iter()
                .find(|album| album.snapshot.album_key.eq_ignore_ascii_case(key))
                .map(|album| album.snapshot.artist.as_str())
        });
    }
    None
}

fn load_cached_artist_overview(app: &mut ApplicationState, artist: &str) {
    app.artist_artwork_draft_path.clear();
    app.artist_portrait_preview_candidate_index = None;
    app.artist_portrait_preview_path.clear();
    app.artist_preview_focus = "portrait".to_string();
    app.artist_last_individual_focus = "portrait".to_string();
    app.artist_portrait_radio_active = false;
    app.artist_banner_radio_active = false;
    app.artist_package_overview_enabled = false;
    app.artist_theme_art_enabled = false;
    app.artist_banner_preview_path.clear();
    app.artist_banner_source_path.clear();
    app.artist_banner_preview_candidate_index = None;
    app.artist_banner_zoom = 1.0;
    app.artist_banner_horizontal = 0.0;
    app.artist_banner_vertical = 0.0;
    if let Some(cached) = app
        .artist_overview_cache
        .get(&artist.trim().to_ascii_lowercase())
        .cloned()
    {
        app.artist_overview_biography = cached.biography;
        app.artist_overview_external_url = cached.external_url;
        app.artist_overview_musicbrainz_id = cached.musicbrainz_id;
        app.artist_overview_banner_path = cached.banner_path;
        app.artist_overview_similar_artists = cached.similar_artists;
        app.artist_overview_genres = cached.genres;
        app.artist_overview_roles = cached.roles;
        app.artist_overview_album_keys = cached.album_keys;
        app.artist_overview_featured_album_keys = cached.featured_album_keys;
        app.artist_overview_track_paths = cached.track_paths_by_album;
        app.artist_overview_album_ratings = cached.album_ratings;
        app.artist_overview_artwork_candidates = cached.artwork_candidates;
        app.artist_overview_top_songs = cached.top_songs;
    } else {
        app.artist_overview_biography.clear();
        app.artist_overview_external_url.clear();
        app.artist_overview_musicbrainz_id.clear();
        app.artist_overview_banner_path.clear();
        app.artist_overview_similar_artists.clear();
        app.artist_overview_genres.clear();
        app.artist_overview_roles.clear();
        app.artist_overview_album_keys.clear();
        app.artist_overview_featured_album_keys.clear();
        app.artist_overview_track_paths.clear();
        app.artist_overview_album_ratings.clear();
        app.artist_overview_artwork_candidates.clear();
        app.artist_overview_top_songs.clear();
    }
    app.artist_overview_top_songs_loading = false;
}

fn select_adjacent_artist_for_overview(
    app: &mut ApplicationState,
    direction: i32,
) -> Option<String> {
    let artist = app.library.adjacent_pending_artist(direction)?;
    if app.library.select_artist_exclusive(&artist, false) == SelectionResult::NotFound {
        return None;
    }

    app.artist_artwork_draft_path.clear();
    app.artist_banner_preview_path.clear();
    app.artist_banner_source_path.clear();
    app.artist_banner_preview_candidate_index = None;
    app.artist_banner_zoom = 1.0;
    app.artist_banner_horizontal = 0.0;
    app.artist_banner_vertical = 0.0;
    app.artist_artwork_mode = false;
    app.artist_overview_show_all = false;
    app.artist_overview_top_songs_loading = false;
    app.synchronize_focused_artist();
    if let Some(cached) = app
        .artist_overview_cache
        .get(&artist.to_ascii_lowercase())
        .cloned()
    {
        app.artist_overview_biography = cached.biography;
        app.artist_overview_external_url = cached.external_url;
        app.artist_overview_musicbrainz_id = cached.musicbrainz_id;
        app.artist_overview_banner_path = cached.banner_path;
        app.artist_overview_similar_artists = cached.similar_artists;
        app.artist_overview_genres = cached.genres;
        app.artist_overview_roles = cached.roles;
        app.artist_overview_album_keys = cached.album_keys;
        app.select_artist_appearances_for_run();
        app.artist_overview_featured_album_keys = cached.featured_album_keys;
        app.artist_overview_track_paths = cached.track_paths_by_album;
        app.artist_overview_album_ratings = cached.album_ratings;
        app.artist_overview_artwork_candidates = cached.artwork_candidates;
        app.artist_overview_top_songs = cached.top_songs;
    } else {
        app.artist_overview_biography.clear();
        app.artist_overview_external_url.clear();
        app.artist_overview_musicbrainz_id.clear();
        app.artist_overview_banner_path.clear();
        app.artist_overview_similar_artists.clear();
        app.artist_overview_genres.clear();
        app.artist_overview_roles.clear();
        app.artist_overview_album_keys.clear();
        app.artist_overview_featured_album_keys.clear();
        app.artist_overview_track_paths.clear();
        app.artist_overview_album_ratings.clear();
        app.artist_overview_artwork_candidates.clear();
        app.artist_overview_top_songs.clear();
    }
    Some(artist)
}

fn activate_library_row_selection(
    state: &Arc<Mutex<ApplicationState>>,
    key: &str,
    include_bypassed: bool,
) -> Option<FallbackTrackHydration> {
    if let Ok(mut app) = state.lock() {
        app.dismiss_intro();
        let previous_artist = app.library.focused_artist.clone();
        let previous_album = app.library.focused_album_key.clone();
        let album_row = app
            .library
            .albums
            .iter()
            .any(|album| album.snapshot.album_key.eq_ignore_ascii_case(key));
        let result = if !album_row && app.library.view == LibraryView::Artists {
            app.library.toggle_artist(key, include_bypassed)
        } else {
            app.library.toggle_album(key, include_bypassed)
        };
        // A direct radio click changes only the selection set. The row body
        // owns focus and expansion, which keeps additive selection from
        // opening every Artist or Fallback Album as the user builds a queue.
        app.library.focused_artist = previous_artist;
        app.library.focused_album_key = previous_album;
        record_library_selection_result(&mut app, result, "library radio selection");
    }
    None
}

fn activate_library_row_expansion(
    state: &Arc<Mutex<ApplicationState>>,
    key: &str,
) -> Option<FallbackTrackHydration> {
    if let Ok(mut app) = state.lock() {
        app.dismiss_intro();
        if app.library.show_tracks {
            if !app.library.toggle_fallback_album_focus(key) {
                app.push_log("The selected fallback album is no longer available.".to_string());
                return None;
            }
            if app.library.focused_album_key.is_some() {
                synchronize_fallback_album(&mut app);
                return app.library.fallback_track_hydration_request(key);
            }
            return None;
        }
        if app.library.view == LibraryView::Artists && !app.library.toggle_artist_scope(key) {
            app.push_log("The selected artist is no longer available.".to_string());
        } else if app.library.view == LibraryView::Artists {
            app.synchronize_focused_artist();
        }
    }
    None
}

fn command_line_operation() -> Option<Result<(), String>> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments
        .iter()
        .any(|value| value == "--slint-library-diagnostics")
    {
        let output = arguments
            .iter()
            .position(|value| value == "--slint-library-diagnostics")
            .and_then(|index| arguments.get(index + 1));
        let result = run_slint_library_diagnostics().and_then(|report| {
            write_diagnostic_report(output, &report, "Slint library diagnostics")
        });
        return Some(result);
    }
    if arguments
        .iter()
        .any(|value| value == "--navidrome-diagnostics")
    {
        let output = arguments
            .iter()
            .position(|value| value == "--navidrome-diagnostics")
            .and_then(|index| arguments.get(index + 1));
        let (report, outcome) = match run_navidrome_diagnostics() {
            Ok(report) => (format!("result=ok\n{report}"), Ok(())),
            Err(error) => (format!("result=error\nmessage={error}\n"), Err(error)),
        };
        let write = write_diagnostic_report(output, &report, "Navidrome diagnostics");
        return Some(write.and(outcome));
    }
    if arguments
        .iter()
        .any(|value| value == "--library-diagnostics")
    {
        let result = run_library_diagnostics().and_then(|report| {
            let output = arguments
                .iter()
                .position(|value| value == "--library-diagnostics")
                .and_then(|index| arguments.get(index + 1));
            write_diagnostic_report(output, &report, "library diagnostics")
        });
        return Some(result);
    }
    if arguments.iter().any(|value| value == "--native-smoke") {
        return Some(
            splined_core::portable::initialize_process_roots().and_then(|layout| {
                match splined_core::config::load_config() {
                    Ok(config) => splined_core::settings::validate_operational_config(&config),
                    Err(_) if !layout.config_file.exists() => Ok(()),
                    Err(error) => Err(error),
                }
            }),
        );
    }
    let index = arguments
        .iter()
        .position(|value| value == "--write-build-info")?;
    let Some(path) = arguments.get(index + 1) else {
        return Some(Err(
            "--write-build-info requires an output filename.".to_string()
        ));
    };
    let path = std::path::PathBuf::from(path);
    let document = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "commit": env!("SPLINED_BUILD_COMMIT"),
        "architecture": "rust-slint",
    });
    Some(
        serde_json::to_vec_pretty(&document)
            .map_err(|error| format!("Unable to serialize build identity: {error}"))
            .and_then(|bytes| {
                std::fs::write(&path, bytes).map_err(|error| {
                    format!("Unable to write build identity {}: {error}", path.display())
                })
            }),
    )
}

fn write_diagnostic_report(
    output: Option<&std::ffi::OsString>,
    report: &str,
    label: &str,
) -> Result<(), String> {
    if let Some(output) = output {
        std::fs::write(output, report).map_err(|error| {
            format!(
                "Unable to write {label} {}: {error}",
                std::path::Path::new(output).display()
            )
        })
    } else {
        print!("{report}");
        Ok(())
    }
}

fn run_navidrome_diagnostics() -> Result<String, String> {
    let layout = splined_core::portable::initialize_process_roots()?;
    let config = splined_core::config::load_config()?;
    splined_core::settings::validate_operational_config(&config)?;
    let credential_path = splined_core::credential_store::credential_path(&config, "Navidrome")?;
    let editor = load_editor(&config, "Navidrome")?;
    let auth_method = editor.auth_method.clone();
    let status = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("Unable to start Navidrome diagnostic runtime: {error}"))?
        .block_on(test_editor(&editor))?;
    Ok(format!(
        "state_root={}\nconfig_v5={}\ncredential_file={}\nauth_method={}\nstatus={}\n",
        layout.root.display(),
        layout.config_file.display(),
        credential_path.display(),
        auth_method,
        status
    ))
}

fn run_slint_library_diagnostics() -> Result<String, String> {
    splined_core::portable::initialize_process_roots()?;
    let config = splined_core::config::load_config()?;
    splined_core::settings::validate_operational_config(&config)?;
    let snapshot = media_snapshot(&config, false)?;
    let state = Arc::new(Mutex::new(ApplicationState::new(config, snapshot)?));
    let (controller, _notices) = ScanController::new(Arc::clone(&state));
    let ui = AppWindow::new().map_err(|error| format!("Unable to create Slint UI: {error}"))?;
    ui.set_setup_required(false);
    sync_ui(&ui, &state);
    let preferences = UiPreferences::default();
    let workspace_layout = Rc::new(RefCell::new(WorkspaceLayout::from_preferences(
        &preferences,
    )));
    let appearance = Rc::new(RefCell::new(normalize_appearance(&preferences.appearance)));
    let main_window_guard = MaximizeGuard::new(false);
    apply_workspace_layout(&ui, &workspace_layout.borrow());
    ui.set_appearance_mode(appearance.borrow().clone().into());
    bind_callbacks(
        &ui,
        Arc::clone(&state),
        Arc::new(controller),
        Rc::clone(&workspace_layout),
        appearance,
        CustomWindowState::from_preferences(&preferences),
        UiWindowDiagnostics {
            maximize_guard: main_window_guard.clone(),
            ui_debug_log: None,
        },
        Arc::new(AtomicBool::new(false)),
    );

    ui.invoke_adjustment_set(SharedString::from("Picture"), 1.0);
    let adjustment_rows = ui.get_adjustment_rows();
    let slider_reaches_configured_maximum = (0..adjustment_rows.row_count())
        .filter_map(|index| adjustment_rows.row_data(index))
        .any(|row| row.name == "Picture" && row.value == row.maximum && row.maximum == 20);
    ui.invoke_adjustment_set(SharedString::from("Picture"), 0.5);

    let visible_artists = ui.get_library_rows().row_count();
    ui.invoke_change_library_view(1);
    let visible_albums = ui.get_library_rows().row_count();
    ui.invoke_change_library_view(2);
    let visible_folders = ui.get_library_rows().row_count();
    ui.invoke_change_library_view(0);

    let artists = ui.get_library_rows();
    let artist = (0..artists.row_count())
        .filter_map(|index| artists.row_data(index))
        .find(|row| row.kind == "artist" && row.eligible)
        .ok_or_else(|| "The Slint Artist model has no selectable row.".to_string())?;
    ui.invoke_library_row_activated(artist.key.clone(), false);
    let artist_click_selected_albums = ui.get_selected_count();
    let artist_click_kept_view = ui.get_library_view_label() == "Artists";
    let artist_overview_album_rows = ui.get_artist_overview_albums().row_count();
    let ten_cc_overview = if state.lock().ok().is_some_and(|app| {
        app.library
            .albums
            .iter()
            .any(|album| album.snapshot.artist == "10cc")
    }) {
        ui.invoke_library_row_activated(SharedString::from("10cc"), false);
        let ui_rows = ui.get_artist_overview_albums().row_count();
        let state_rows = state.lock().ok().map_or(0, |app| {
            app.library
                .artist_overview_album_indices(&app.artist_overview_album_keys, false)
                .len()
        });
        format!(
            "ten_cc_artist_overview_state_rows={state_rows}\nten_cc_artist_overview_ui_rows={ui_rows}\n"
        )
    } else {
        String::new()
    };
    ui.invoke_library_row_activated(artist.key.clone(), false);
    let expanded_rows = ui.get_library_rows();
    let artist_navigation_rows = expanded_rows.row_count();
    let inline_album = (0..expanded_rows.row_count())
        .filter_map(|index| expanded_rows.row_data(index))
        .find(|row| row.kind == "album-child")
        .ok_or_else(|| "The selected Artist produced no inline Album row.".to_string())?;
    let artist_inline_album_rows = artist_navigation_rows > visible_artists;
    ui.invoke_library_row_activated(inline_album.key, false);
    let current_album_populated = !ui.get_current_artist().trim().is_empty()
        && !ui.get_current_album().trim().is_empty()
        && !ui.get_current_path().trim().is_empty();
    ui.invoke_change_library_view(2);
    let folder_rows_after_artist = ui.get_library_rows().row_count();
    ui.invoke_select_none();
    let select_none_count = ui.get_selected_count();
    ui.invoke_set_show_tracks(true);
    let fallback_rows = ui.get_library_rows();
    let fallback_album = (0..fallback_rows.row_count())
        .filter_map(|index| fallback_rows.row_data(index))
        .find(|row| row.kind == "fallback-album" && row.eligible)
        .ok_or_else(|| "The Slint Fallback model has no selectable Album row.".to_string())?;
    ui.invoke_library_row_selection_activated(fallback_album.key, false);
    let fallback_album_enables_launch = ui.get_can_launch();
    let expanded_fallback_rows = ui.get_library_rows();
    let fallback_album_expands_inline = (0..expanded_fallback_rows.row_count())
        .filter_map(|index| expanded_fallback_rows.row_data(index))
        .any(|row| row.kind == "fallback-album" && row.selected && row.expanded);
    let fallback_album_queues_whole_album = state
        .lock()
        .ok()
        .and_then(|app| app.library.queue(ScanScope::Selected).into_iter().next())
        .is_some_and(|target| target.compilation_track_path.is_none());
    ui.invoke_select_none();
    ui.invoke_set_show_tracks(false);

    let media_rows_before_collapse = ui.get_library_rows().row_count();
    ui.invoke_set_region_expanded(SharedString::from("media"), false);
    let media_collapsed = !ui.get_media_selector_expanded();
    ui.invoke_set_region_expanded(SharedString::from("media"), true);
    let media_expanded = ui.get_media_selector_expanded();
    let restored_media_width = logical_dimension(ui.get_media_pane_width(), 0);
    let media_rows_after_expand = ui.get_library_rows().row_count();
    let media_model_restored = media_rows_after_expand == media_rows_before_collapse;
    if !media_collapsed || !media_expanded {
        return Err("The Slint Media Library did not complete collapse/expand.".to_string());
    }
    if restored_media_width < MEDIA_PANE_MIN {
        return Err(format!(
            "The restored Media Library width {restored_media_width} is below {MEDIA_PANE_MIN}."
        ));
    }
    if !media_model_restored {
        return Err(format!(
            "The Slint Media Library model changed across collapse/expand ({media_rows_before_collapse} -> {media_rows_after_expand})."
        ));
    }

    if let Ok(mut app) = state.lock() {
        app.library.clear_artist_scope();
        app.library.set_text_filter("");
        app.library.set_view(LibraryView::Artists);
    }
    sync_ui(&ui, &state);
    let full_library_rows = ui.get_library_rows().row_count();
    let _ = full_library_rows;
    ui.show()
        .map_err(|error| format!("Unable to show the main diagnostic window: {error}"))?;
    ui.window().set_maximized(true);
    ui.invoke_set_region_expanded(SharedString::from("candidate-filter"), false);
    let main_maximized_after_collapse = ui.window().is_maximized();
    ui.invoke_set_region_expanded(SharedString::from("candidate-filter"), true);
    let main_maximized_after_expand = ui.window().is_maximized();
    let _ = ui.hide();

    Ok(format!(
        "visible_artists={visible_artists}\nvisible_albums={visible_albums}\nvisible_folders={visible_folders}\nartist_click_selected_albums={artist_click_selected_albums}\nartist_click_kept_view={artist_click_kept_view}\nartist_overview_album_rows={artist_overview_album_rows}\n{ten_cc_overview}artist_inline_album_rows={artist_inline_album_rows}\nartist_navigation_rows={artist_navigation_rows}\nfolder_rows_after_artist={folder_rows_after_artist}\ncurrent_album_populated={current_album_populated}\nselect_none_count={select_none_count}\nfallback_album_enables_launch={fallback_album_enables_launch}\nfallback_album_expands_inline={fallback_album_expands_inline}\nfallback_album_queues_whole_album={fallback_album_queues_whole_album}\nslider_reaches_configured_maximum={slider_reaches_configured_maximum}\nmedia_collapsed={media_collapsed}\nmedia_expanded={media_expanded}\nmain_maximized_after_collapse={main_maximized_after_collapse}\nmain_maximized_after_expand={main_maximized_after_expand}\nrestored_media_width={restored_media_width}\nrestored_media_width_at_least_min={}\nmedia_rows_before_collapse={media_rows_before_collapse}\nmedia_rows_after_expand={media_rows_after_expand}\nmedia_model_restored={media_model_restored}\n",
        restored_media_width >= MEDIA_PANE_MIN
    ))
}

fn run_library_diagnostics() -> Result<String, String> {
    let layout = splined_core::portable::initialize_process_roots()?;
    let config = splined_core::config::load_config()?;
    splined_core::settings::validate_operational_config(&config)?;
    let snapshot = media_snapshot(&config, false)?;
    let album_count = snapshot.albums.len();
    let artist_count = snapshot
        .albums
        .iter()
        .map(|album| album.artist.trim().to_ascii_lowercase())
        .filter(|artist| !artist.is_empty())
        .collect::<BTreeSet<_>>()
        .len();
    let folder_count = snapshot
        .albums
        .iter()
        .map(|album| album.path.trim().to_ascii_lowercase())
        .filter(|path| !path.is_empty())
        .collect::<BTreeSet<_>>()
        .len();
    let mut state = ApplicationState::new(config.clone(), snapshot)?;
    state.library.set_text_filter("");
    state.library.set_status_filter(StatusFilter::All);
    state.library.clear_artist_scope();
    state.library.set_view(LibraryView::Artists);
    let visible_artists = state.library.rows().len();
    state.library.set_view(LibraryView::Albums);
    let visible_albums = state.library.rows().len();
    state.library.set_view(LibraryView::Folders);
    let visible_folders = state.library.rows().len();
    state.library.set_view(LibraryView::Artists);
    let artist_key = state
        .library
        .rows()
        .into_iter()
        .find(|row| row.eligible)
        .map(|row| row.key)
        .ok_or_else(|| "The real library has no selectable Artist row.".to_string())?;
    let artist_click_changed =
        state.library.toggle_row(&artist_key, false) == SelectionResult::Changed;
    let artist_click_selected_albums = state.library.selected_count();
    state.library.set_view(LibraryView::Albums);
    let artist_navigation_rows = state.library.rows().len();
    let album_key = state
        .library
        .rows()
        .into_iter()
        .next()
        .map(|row| row.key)
        .ok_or_else(|| "The selected Artist produced no visible Album rows.".to_string())?;
    let album_click_changed =
        state.library.toggle_row(&album_key, false) == SelectionResult::Changed;
    let current_album_populated = state.synchronize_focused_album()
        && !state.current_artist.trim().is_empty()
        && !state.current_album.trim().is_empty()
        && !state.current_album_path.trim().is_empty();
    state.library.select_none();
    let select_none_count = state.library.selected_count();

    Ok(format!(
        "state_root={}\nconfig_v5={}\nscan_cache_dir={}\nsqlite={}\nalbums={album_count}\nartists={artist_count}\nfolders={folder_count}\nvisible_artists={visible_artists}\nvisible_albums={visible_albums}\nvisible_folders={visible_folders}\nartist_click_changed={artist_click_changed}\nartist_click_selected_albums={artist_click_selected_albums}\nartist_navigation_rows={artist_navigation_rows}\nalbum_click_changed={album_click_changed}\ncurrent_album_populated={current_album_populated}\nselect_none_count={select_none_count}\n",
        layout.root.display(),
        layout.config_file.display(),
        config.scan.cache_dir,
        splined_core::media_database::database_path(&config.scan.cache_dir).display()
    ))
}

fn restore_request_from_args(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> Option<std::path::PathBuf> {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    let path = match arguments.as_slice() {
        [path] => std::path::PathBuf::from(path),
        [flag, path] if flag == "--restore" => std::path::PathBuf::from(path),
        _ => return None,
    };
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("spl"))
        .then_some(path)
}

fn is_sqlite_backup_path(path: &Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("sqlite") || extension.eq_ignore_ascii_case("db")
        })
}

fn normalize_backup_schedule(value: &str) -> &'static str {
    match value.trim().to_ascii_lowercase().as_str() {
        "daily" => "Daily",
        "weekly" => "Weekly",
        "monthly" => "Monthly",
        _ => "Off",
    }
}

const OVERRIDE_DELAY_CHOICES: [(&str, f64); 8] = [
    ("Always Override", 0.0),
    ("1 Hour Override", 1.0),
    ("3 Hour Override", 3.0),
    ("6 Hour Override", 6.0),
    ("12 Hour Override", 12.0),
    ("1 Day Override", 24.0),
    ("1 Week Override", 168.0),
    ("1 Month Override", 720.0),
];

fn override_delay_label(hours: f64) -> String {
    OVERRIDE_DELAY_CHOICES
        .iter()
        .find(|(_, value)| *value == hours)
        .map(|(label, _)| (*label).to_string())
        .unwrap_or_else(|| format!("Custom: {hours} Hours"))
}

fn override_delay_hours(value: &str) -> Result<f64, String> {
    OVERRIDE_DELAY_CHOICES
        .iter()
        .find(|(label, _)| *label == value)
        .map(|(_, hours)| *hours)
        .or_else(|| {
            value
                .strip_prefix("Custom: ")
                .and_then(|value| value.strip_suffix(" Hours"))
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|hours| hours.is_finite() && *hours >= 0.0)
        })
        .ok_or_else(|| format!("Unsupported Processing Override Delay: {value}"))
}

fn diagnostic_level_label(verbosity: Verbosity) -> &'static str {
    match verbosity {
        Verbosity::Error | Verbosity::Warn => "Error",
        Verbosity::Info => "Current",
        Verbosity::Debug => "Verbose",
        Verbosity::Trace => "Debugger",
    }
}

fn diagnostic_level_verbosity(value: &str) -> Result<Verbosity, String> {
    match value {
        "Current" => Ok(Verbosity::Info),
        "Verbose" => Ok(Verbosity::Debug),
        "Debugger" => Ok(Verbosity::Trace),
        "Error" => Ok(Verbosity::Error),
        _ => Err(format!("Unsupported Diagnostic Log Level: {value}")),
    }
}

fn diagnostic_level_verbosity_for_config(
    value: &str,
    previous: Verbosity,
) -> Result<Verbosity, String> {
    if value == "Error" && previous == Verbosity::Warn {
        Ok(Verbosity::Warn)
    } else {
        diagnostic_level_verbosity(value)
    }
}

fn backup_schedule_interval(value: &str) -> Option<Duration> {
    match normalize_backup_schedule(value) {
        "Daily" => Some(Duration::from_secs(24 * 60 * 60)),
        "Weekly" => Some(Duration::from_secs(7 * 24 * 60 * 60)),
        "Monthly" => Some(Duration::from_secs(30 * 24 * 60 * 60)),
        _ => None,
    }
}

fn backup_file_in_directory(
    directory: &Path,
    stem: &str,
    extension: &str,
) -> Result<PathBuf, String> {
    if directory.as_os_str().is_empty() {
        return Err("Choose a backup directory first.".to_string());
    }
    if directory.exists() && !directory.is_dir() {
        return Err(format!(
            "The backup path is not a directory: {}",
            directory.display()
        ));
    }
    fs::create_dir_all(directory).map_err(|error| {
        format!(
            "Unable to create backup directory {}: {error}",
            directory.display()
        )
    })?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let stem = machine_backup_stem(stem);
    let candidate = directory.join(format!("{stem}-{timestamp}.{extension}"));
    if !candidate.exists() {
        return Ok(candidate);
    }
    for suffix in 1..=999_u16 {
        let candidate = directory.join(format!("{stem}-{timestamp}-{suffix}.{extension}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("Unable to allocate a unique timestamped backup filename.".to_string())
}

fn machine_backup_stem(stem: &str) -> String {
    let machine = std::env::var("COMPUTERNAME").unwrap_or_default();
    backup_stem_for_machine(stem, &machine)
}

fn safe_machine_name(machine: &str) -> String {
    let machine = machine
        .trim()
        .chars()
        .take(64)
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    if machine.is_empty() {
        "UNKNOWN".to_string()
    } else {
        machine
    }
}

fn ui_debug_log_filename(machine: &str, timestamp: u128, pid: u32) -> String {
    let machine = safe_machine_name(machine);
    format!("[{machine}]-Windows-ui-{timestamp}-{pid}.debug.log")
}

fn is_machine_ui_debug_log_filename(name: &str, machine: &str) -> bool {
    if !name.ends_with(".debug.log") {
        return false;
    }
    let machine = safe_machine_name(machine);
    name.starts_with(&format!("[{machine}]-Windows-ui-"))
        || name.starts_with(&format!("windows-ui-[{machine}]-"))
}

fn backup_stem_for_machine(stem: &str, machine: &str) -> String {
    let machine = safe_machine_name(machine);
    if let Some(suffix) = stem.strip_prefix("splined-") {
        format!("splined-{machine}-{suffix}")
    } else {
        format!("{machine}-{stem}")
    }
}

fn archived_backup_files(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut files = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| {
            entry.file_name().to_str().is_some_and(|name| {
                name.starts_with("splined-")
                    && (name.ends_with(".spl") || name.ends_with(".sqlite"))
            })
        })
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    files.sort_by(|left, right| {
        let modified = |path: &PathBuf| {
            path.metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(UNIX_EPOCH)
        };
        modified(right)
            .cmp(&modified(left))
            .then_with(|| right.cmp(left))
    });
    files
}

fn refresh_archived_backups_ui(ui: &AppWindow, select_latest: bool) {
    let files = archived_backup_files(Path::new(ui.get_backup_directory().as_str()));
    if select_latest && let Some(latest) = files.first() {
        let latest = SharedString::from(latest.to_string_lossy().into_owned());
        ui.set_restore_path(latest.clone());
        ui.set_archived_backup_selection(latest);
    }
    ui.set_archived_backup_files(model(
        files
            .iter()
            .map(|path| SharedString::from(path.to_string_lossy().into_owned()))
            .collect(),
    ));
}

fn backup_file_limit(value: &str) -> Option<usize> {
    value
        .trim()
        .parse::<usize>()
        .ok()
        .filter(|limit| *limit > 0)
}

fn prune_timestamped_backups(
    directory: &Path,
    prefix: &str,
    extension: &str,
    limit: usize,
) -> Result<usize, String> {
    if limit == 0 || !directory.is_dir() {
        return Ok(0);
    }
    let canonical_directory = directory
        .canonicalize()
        .map_err(|error| format!("Unable to resolve backup directory: {error}"))?;
    let mut matching = fs::read_dir(directory)
        .map_err(|error| format!("Unable to list backup directory: {error}"))?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| {
            entry.file_name().to_str().is_some_and(|name| {
                name.starts_with(prefix) && name.ends_with(&format!(".{extension}"))
            })
        })
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    matching.sort_by(|left, right| {
        let modified = |path: &PathBuf| {
            path.metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(UNIX_EPOCH)
        };
        modified(right)
            .cmp(&modified(left))
            .then_with(|| right.cmp(left))
    });
    let mut removed = 0;
    for path in matching.into_iter().skip(limit) {
        let resolved = path
            .canonicalize()
            .map_err(|error| format!("Unable to resolve backup {}: {error}", path.display()))?;
        if resolved.parent() != Some(canonical_directory.as_path()) {
            return Err(format!(
                "Refusing to prune a backup outside {}",
                canonical_directory.display()
            ));
        }
        fs::remove_file(&resolved)
            .map_err(|error| format!("Unable to prune backup {}: {error}", resolved.display()))?;
        removed += 1;
    }
    Ok(removed)
}

fn enforce_backup_file_limits(
    directory: &Path,
    preferences: &UiPreferences,
) -> Result<usize, String> {
    let mut removed = 0;
    for (prefix, extension, requested) in [
        (
            "splined-settings-",
            "spl",
            preferences.max_settings_backup_files.as_str(),
        ),
        (
            "splined-database-",
            "sqlite",
            preferences.max_database_backup_files.as_str(),
        ),
        (
            "splined-diagnostics-",
            "json",
            preferences.max_diagnostic_backup_files.as_str(),
        ),
    ] {
        if let Some(limit) = backup_file_limit(requested) {
            removed += prune_timestamped_backups(
                directory,
                &machine_backup_stem(prefix),
                extension,
                limit,
            )?;
        }
    }
    Ok(removed)
}

fn automatic_backup_is_due(directory: &Path, schedule: &str, stem: &str, extension: &str) -> bool {
    let Some(interval) = backup_schedule_interval(schedule) else {
        return false;
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return true;
    };
    let stem = machine_backup_stem(stem);
    let newest = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_name().to_str().is_some_and(|name| {
                name.starts_with(&format!("{stem}-")) && name.ends_with(&format!(".{extension}"))
            })
        })
        .filter_map(|entry| entry.metadata().ok()?.modified().ok())
        .max();
    newest
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_none_or(|elapsed| elapsed >= interval)
}

fn import_settings_from_opened_backup(path: &Path) -> Result<Config, String> {
    splined_core::portable::initialize_process_roots()?;
    let current = splined_core::config::load_config().unwrap_or_default();
    restore_backup(
        path,
        &current,
        BackupSelection {
            settings: true,
            credentials: false,
            database: false,
            interface: false,
            diagnostics: false,
        },
        "",
    )?;
    splined_core::config::load_config().map_err(|error| {
        format!("The opened .spl backup did not provide a usable Config v5: {error}")
    })
}

fn package_identity_smoke_requested() -> bool {
    if std::env::args_os().any(|argument| argument == "--package-identity-smoke") {
        return true;
    }
    let Ok(layout) = splined_core::portable::initialize_process_roots() else {
        return false;
    };
    splined_core::portable::is_packaged_process().unwrap_or(false)
        && layout.root.join("package-identity-smoke.request").is_file()
}

fn package_identity_smoke() {
    let result = splined_core::portable::initialize_process_roots().and_then(|layout| {
        if !splined_core::portable::is_packaged_process()? {
            return Err("The package-identity smoke process has no MSIX identity.".to_string());
        }
        std::fs::create_dir_all(&layout.root).map_err(|error| {
            format!(
                "Unable to create packaged SPLINED state root {}: {error}",
                layout.root.display()
            )
        })?;
        let marker = layout.root.join("package-identity-smoke.ok");
        let request = layout.root.join("package-identity-smoke.request");
        let _ = std::fs::remove_file(request);
        std::fs::write(&marker, b"rust-slint-msix\n").map_err(|error| {
            format!(
                "Unable to write package-identity marker {}: {error}",
                marker.display()
            )
        })
    });
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(2);
    }
}

fn load_application_state() -> (ApplicationState, bool, Option<String>) {
    let config_started = Instant::now();
    let mut first_error = splined_core::portable::initialize_process_roots().err();
    if first_error.is_none()
        && let Err(error) = splined_core::config_migration::migrate_config_if_needed()
    {
        first_error = Some(error);
    }
    let config = match splined_core::config::load_config() {
        Ok(config) => config,
        Err(error) => {
            first_error.get_or_insert(error);
            let mut setup = Config::default();
            setup.library.music_library.clear();
            setup.scan.scan_library_dir.clear();
            if let Ok(layout) = splined_core::portable::app_layout() {
                setup.scan.cache_dir = layout.root.join("_cache").to_string_lossy().into_owned();
                setup.scan.shared_artist_art_dir = layout
                    .root
                    .join("_cache")
                    .join("artist-art")
                    .to_string_lossy()
                    .into_owned();
                setup.scan.temporary_cache_dir =
                    layout.root.join("_cache").to_string_lossy().into_owned();
                setup.scan.log_dir = layout.root.join("_logs").to_string_lossy().into_owned();
                setup.credentials.credential_dir = layout
                    .root
                    .join("credentials")
                    .to_string_lossy()
                    .into_owned();
                setup.samples.sample_dir = layout
                    .root
                    .join("_cache")
                    .join("samples")
                    .to_string_lossy()
                    .into_owned();
            }
            let snapshot = empty_snapshot(&setup);
            let mut state = ApplicationState::new(setup, snapshot)
                .expect("the Rust first-run setup model must remain valid");
            state.service_status = "Setup required".to_string();
            state.log.clear();
            if let Some(error) = first_error.as_ref() {
                state.last_error = Some(error.clone());
                state.push_log(format!("SETUP REQUIRED: {error}"));
            }
            return (state, true, first_error);
        }
    };
    record_startup_timing(format!(
        "startup-config elapsed_ms={}",
        config_started.elapsed().as_millis()
    ));
    if let Err(error) = splined_core::settings::validate_operational_config(&config) {
        let error = format!("Config v5 is not ready for use: {error}");
        first_error.get_or_insert_with(|| error.clone());
        let snapshot = empty_snapshot(&config);
        let mut state = ApplicationState::new(config, snapshot)
            .expect("a parsed Config v5 must initialize the Rust setup model");
        state.service_status = "Setup required".to_string();
        state.last_error = Some(error.clone());
        state.push_log(format!("SETUP REQUIRED: {error}"));
        return (state, true, first_error);
    }
    let cache_cleanup = splined_core::pipeline::cleanup_stale_artwork_on_startup(Path::new(
        &config.scan.temporary_cache_dir,
    ));
    let snapshot_started = Instant::now();
    let snapshot = match media_snapshot(&config, false) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let error = format!("Configured SQLite database could not be loaded: {error}");
            first_error.get_or_insert_with(|| error.clone());
            let snapshot = empty_snapshot(&config);
            let mut state = ApplicationState::new(config, snapshot)
                .expect("a parsed Config v5 must initialize the Rust setup model");
            state.service_status = "Setup required".to_string();
            state.last_error = Some(error.clone());
            state.push_log(format!("SETUP REQUIRED: {error}"));
            return (state, true, first_error);
        }
    };
    record_startup_timing(format!(
        "startup-database elapsed_ms={} albums={} shared={}",
        snapshot_started.elapsed().as_millis(),
        snapshot.albums.len(),
        config.scan.sqlite_shared
    ));
    let diagnostics = startup_diagnostics(&config, &snapshot);
    let model_started = Instant::now();
    let mut state = ApplicationState::new(config, snapshot)
        .expect("a parsed Config v5 and database snapshot must initialize application state");
    record_startup_timing(format!(
        "startup-model elapsed_ms={}",
        model_started.elapsed().as_millis()
    ));
    for diagnostic in diagnostics {
        state.push_log(diagnostic);
    }
    match cache_cleanup {
        Ok((count, bytes)) if count > 0 => state.push_log(format!(
            "Startup cache cleanup removed {count} stale disposable artwork file(s), {:.1} MiB.",
            bytes as f64 / 1_048_576.0
        )),
        Err(error) => state.push_log(format!("WARNING: Startup cache cleanup: {error}")),
        _ => {}
    }
    (state, false, first_error)
}

fn empty_snapshot(config: &Config) -> MediaSnapshot {
    MediaSnapshot {
        schema_version: splined_core::media_database::SCHEMA_VERSION,
        database_path: String::new(),
        canonical_root: config.library.music_library.clone(),
        local_root: config.library.music_library.clone(),
        albums: Vec::new(),
    }
}

fn startup_diagnostics(config: &Config, snapshot: &MediaSnapshot) -> Vec<String> {
    let artists = snapshot
        .albums
        .iter()
        .map(|album| album.artist.trim().to_ascii_lowercase())
        .filter(|artist| !artist.is_empty())
        .collect::<BTreeSet<_>>()
        .len();
    let folders = snapshot
        .albums
        .iter()
        .map(|album| album.path.trim().to_ascii_lowercase())
        .filter(|folder| !folder.is_empty())
        .collect::<BTreeSet<_>>()
        .len();
    vec![
        format!(
            "State root: {}",
            splined_core::portable::state_root()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|error| format!("unavailable ({error})"))
        ),
        format!(
            "Config v5: {}",
            splined_core::config::config_path()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "unavailable".to_string())
        ),
        format!(
            "SQLite: {}",
            splined_core::media_database::database_path(&config.scan.cache_dir).display()
        ),
        format!(
            "Library loaded: {} albums · {artists} artists · {folders} folders",
            snapshot.albums.len()
        ),
    ]
}

fn apply_workspace_layout(ui: &AppWindow, layout: &WorkspaceLayout) {
    ui.set_media_pane_width(layout.media_pane_width as f32);
    ui.set_artwork_pane_width(layout.artwork_pane_width as f32);
    ui.set_candidate_filter_height(layout.candidate_filter_height as f32);
    ui.set_activity_pane_height(layout.activity_pane_height as f32);
}

enum EmbedQueueSource {
    File(PathBuf),
    Preview(Vec<u8>),
    PreparedThumbnail(Vec<u8>),
}

struct EmbedQueueTarget {
    album: PathBuf,
    source: EmbedQueueSource,
    label: String,
    side: u32,
    cover_config: Option<Config>,
    preferred_cover: Option<PathBuf>,
}

fn current_embed_queue_target(app: &ApplicationState) -> Option<(PathBuf, String)> {
    if app.artist_artwork_mode
        || compilation_artwork_context(app)
        || app.current_album_path.trim().is_empty()
    {
        return None;
    }
    let album = PathBuf::from(&app.current_album_path);
    let preview = if app.edit_preview_path.starts_with("memory-preview://") {
        app.edit_preview_path.as_str()
    } else if embedded_thumbnail_ready(app) {
        app.embedded_thumbnail_preview_path.as_str()
    } else {
        ""
    };
    let source = if !preview.is_empty() {
        if !has_edit_preview_bytes(preview) {
            return None;
        }
        preview.to_string()
    } else if let Some(candidate) = app.selected_candidate() {
        candidate.cache_path.clone()
    } else if app.phase == RunPhase::AwaitingMusicBrainz
        && !app.musicbrainz_cover_preview_path.is_empty()
    {
        app.musicbrainz_cover_preview_path.clone()
    } else {
        app.current_cover_path.clone()
    };
    (album.is_dir()
        && (!source.is_empty()
            && (source.starts_with("memory-preview://") || Path::new(&source).is_file()))
        && !app
            .embed_locked_album_paths
            .contains(&app.current_album_path.to_lowercase()))
    .then_some((album, source))
}

fn album_key_embed_locked(app: &ApplicationState, key: &str) -> bool {
    app.library.albums.iter().any(|album| {
        album.snapshot.album_key.eq_ignore_ascii_case(key)
            && app
                .embed_locked_album_paths
                .contains(&album.snapshot.path.to_lowercase())
    })
}

fn sync_embed_queue_locks(
    state: &Arc<Mutex<ApplicationState>>,
    database: &Path,
) -> Result<(), String> {
    let paths = embed_queue::active_album_paths(database)?;
    let mut app = state
        .lock()
        .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
    app.embed_locked_album_paths = paths;
    Ok(())
}

fn sync_embed_queue_window(
    window: &EmbedQueueWindow,
    state: &Arc<Mutex<ApplicationState>>,
    database: &Path,
) -> Result<(), String> {
    if let Ok(app) = state.lock() {
        window.set_appearance_mode(app.appearance_mode.clone().into());
        let ready = current_embed_queue_target(&app);
        let key = ready
            .as_ref()
            .map(|(album, source)| {
                format!(
                    "{}|{}|{}",
                    album.display(),
                    source,
                    app.config.output.embedded_thumbnail_size
                )
            })
            .unwrap_or_default();
        if window.get_current_key().as_str() != key {
            window.set_current_key(key.into());
            window.set_current_label(
                if ready.is_some() {
                    format!(
                        "Awaiting staging · {} — {}",
                        app.current_artist, app.current_album
                    )
                } else {
                    "Select an album cover or upscale preview to stage".to_string()
                }
                .into(),
            );
            let summary = if ready.is_some() {
                let infos = current_embedded_thumbnail_infos(&app);
                let (_, _, before_dimensions, before_size, _) =
                    embedded_artwork_footer(&infos, app.config.output.embedded_thumbnail_size);
                format!(
                    "{} · {} tracks · Before: {} / {} · After: {} × {} each",
                    if ready.as_ref().is_some_and(|(_, source)| source.starts_with("memory-preview://")) {
                        "Edited preview"
                    } else {
                        "Selected cover"
                    },
                    infos.len(),
                    before_dimensions,
                    before_size,
                    app.config.output.embedded_thumbnail_size,
                    app.config.output.embedded_thumbnail_size,
                )
            } else {
                String::new()
            };
            window.set_current_summary(summary.into());
            window.set_current_preview(
                ready
                    .as_ref()
                    .map(|(_, source)| {
                        if source.starts_with("memory-preview://") {
                            load_cached_preview_image(source, &app.current_cover_path)
                        } else {
                            load_scaled_image(source, 160)
                        }
                    })
                    .unwrap_or_default(),
            );
        }
        window.set_current_ready(ready.is_some());
    }
    let jobs = embed_queue::list(database)?;
    window.set_has_running(jobs.iter().any(|job| job.state == "running"));
    let staged_count = jobs.iter().filter(|job| job.state == "staged").count();
    let rows = jobs
        .into_iter()
        .map(|job| {
            let retryable = job.state == "failed" || job.state == "paused";
            let preferred_size = album_embedded_check_cache()
                .lock()
                .ok()
                .and_then(|cache| {
                    cache
                        .get(&album_embedded_check_key(
                            &job.album_path.to_string_lossy(),
                            job.side,
                        ))
                        .copied()
                        .flatten()
                })
                .is_some_and(|check| check.tracks == job.total && !check.needs_attention());
            let key = format!("memory-preview://embed-queue/{}", job.id);
            let preview = if preload_scaled_image_bytes(&key, &job.thumbnail, 128) {
                cached_scaled_image(&key, 128).unwrap_or_default()
            } else {
                slint::Image::default()
            };
            UiEmbedQueueRow {
                id: job.id as i32,
                label: job.label.into(),
                status: if (window.get_paused() || window.get_pause_all()) && job.state == "queued"
                {
                    "paused".into()
                } else {
                    job.state.into()
                },
                detail: job.detail.into(),
                progress: format!(
                    "{}/{} tracks · embedded {} × {}",
                    job.completed, job.total, job.side, job.side
                )
                .into(),
                cover_resolution: format!(
                    "staged cover resolution {} × {}",
                    if job.cover_width == 0 {
                        job.side
                    } else {
                        job.cover_width
                    },
                    if job.cover_height == 0 {
                        job.side
                    } else {
                        job.cover_height
                    }
                )
                .into(),
                preview,
                retryable,
                preferred_size,
            }
        })
        .collect();
    window.set_jobs(model(rows));
    EMBED_MAIN_WINDOW.with(|slot| {
        if let Some(main) = slot.borrow().as_ref().and_then(slint::Weak::upgrade) {
            main.set_embed_panel_jobs(window.get_jobs());
            main.set_embed_panel_current_label(window.get_current_label());
            main.set_embed_panel_current_summary(window.get_current_summary());
            main.set_embed_panel_current_preview(window.get_current_preview());
            main.set_embed_panel_message(window.get_queue_message());
            main.set_embed_panel_status(window.get_queue_status());
            main.set_embed_panel_ready(window.get_current_ready());
            main.set_embed_panel_paused(window.get_paused());
            main.set_embed_panel_pause_all(window.get_pause_all());
            main.set_embed_panel_running(window.get_has_running());
            main.set_embed_panel_staged_count(staged_count as i32);
            main.set_embed_panel_staging_count(window.get_staging_count());
        }
    });
    Ok(())
}

fn snapshot_embed_queue_target_from_app(
    app: &ApplicationState,
) -> Result<EmbedQueueTarget, String> {
    let (album, source) = current_embed_queue_target(app).ok_or_else(|| {
        "Select a readable Artist Album cover or render its upscale preview; COMP fallback stays track-scoped."
            .to_string()
    })?;
    let source = if source.starts_with("memory-preview://") {
        let bytes = full_edit_preview_bytes(&source).ok_or_else(|| {
            "The upscale preview expired; render it again before staging.".to_string()
        })?;
        if source.starts_with("memory-preview://embedded-thumbnail/") {
            EmbedQueueSource::PreparedThumbnail(bytes)
        } else {
            EmbedQueueSource::Preview(bytes)
        }
    } else {
        EmbedQueueSource::File(PathBuf::from(source))
    };
    Ok(EmbedQueueTarget {
        album,
        source,
        label: format!("{} — {}", app.current_artist, app.current_album),
        side: app.config.output.embedded_thumbnail_size,
        cover_config: Some(app.config.clone()),
        preferred_cover: Some(PathBuf::from(&app.current_cover_path)).filter(|cover| {
            cover.is_file()
                && cover.parent().is_some_and(|parent| {
                    parent
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&app.current_album_path)
                })
                && cover
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| {
                        ["jpg", "jpeg", "png"]
                            .iter()
                            .any(|kind| ext.eq_ignore_ascii_case(kind))
                    })
                && cover
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .is_some_and(|stem| stem.eq_ignore_ascii_case(&app.config.output.file_name))
        }),
    })
}

fn snapshot_embed_queue_target(
    state: &Arc<Mutex<ApplicationState>>,
) -> Result<EmbedQueueTarget, String> {
    let app = state
        .lock()
        .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
    snapshot_embed_queue_target_from_app(&app)
}

fn saved_album_queue_target(
    app: &ApplicationState,
    album: &AlbumEntry,
) -> Option<EmbedQueueTarget> {
    let snapshot = &album.snapshot;
    let cover = PathBuf::from(&snapshot.cover_path);
    let cover_is_local = cover.parent().is_some_and(|parent| {
        parent
            .to_string_lossy()
            .eq_ignore_ascii_case(&snapshot.path)
    });
    let cover_is_named = cover
        .file_stem()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(&app.config.output.file_name));
    let supported_cover = cover
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["jpg", "jpeg", "png", "webp"]
                .iter()
                .any(|kind| extension.eq_ignore_ascii_case(kind))
        });
    (!snapshot.album_mbid.trim().is_empty()
        && cover_is_local
        && cover_is_named
        && supported_cover
        && !app
            .embed_locked_album_paths
            .contains(&snapshot.path.to_lowercase()))
    .then(|| EmbedQueueTarget {
        album: PathBuf::from(&snapshot.path),
        source: EmbedQueueSource::File(cover),
        label: format!("{} — {}", snapshot.artist, snapshot.title),
        side: app.config.output.embedded_thumbnail_size,
        cover_config: None,
        preferred_cover: None,
    })
}

fn album_card_queue_target(
    state: &Arc<Mutex<ApplicationState>>,
    key: &str,
) -> Result<EmbedQueueTarget, String> {
    let app = state
        .lock()
        .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
    let album = app
        .library
        .albums
        .iter()
        .find(|album| album.snapshot.album_key.eq_ignore_ascii_case(key))
        .ok_or_else(|| "The selected album is no longer in the library.".to_string())?;
    if album
        .snapshot
        .path
        .eq_ignore_ascii_case(&app.current_album_path)
        && (embedded_thumbnail_ready(&app)
            || app.edit_preview_path.starts_with("memory-preview://"))
    {
        drop(app);
        return snapshot_embed_queue_target(state);
    }
    saved_album_queue_target(&app, album).ok_or_else(|| {
        "This Artist Album needs a saved JPG/PNG/WebP cover and Release ID before it can be staged; COMP fallback stays track-scoped."
            .to_string()
    })
}

fn artist_album_queue_targets(app: &ApplicationState) -> Result<Vec<EmbedQueueTarget>, String> {
    if app.library.view != LibraryView::Artists || app.library.focused_artist.is_none() {
        return Err("Select an Artist before queueing its albums.".to_string());
    }
    let indices = app.library.artist_overview_album_indices(
        &app.artist_overview_album_keys,
        app.artist_overview_show_all,
    );
    let current_in_overview = indices.iter().any(|index| {
        app.library.albums.get(*index).is_some_and(|album| {
            album
                .snapshot
                .path
                .eq_ignore_ascii_case(&app.current_album_path)
                && !album.snapshot.album_mbid.trim().is_empty()
        })
    });
    let mut targets = indices
        .into_iter()
        .filter_map(|index| app.library.albums.get(index))
        .filter(|album| !album.snapshot.album_mbid.trim().is_empty())
        .filter_map(|album| saved_album_queue_target(app, album))
        .collect::<Vec<_>>();
    let current_has_choice = embedded_thumbnail_ready(app)
        || app.edit_preview_path.starts_with("memory-preview://")
        || app.selected_candidate().is_some()
        || (app.phase == RunPhase::AwaitingMusicBrainz
            && !app.musicbrainz_cover_preview_path.is_empty());
    if current_has_choice
        && current_in_overview
        && let Ok(current) = snapshot_embed_queue_target_from_app(app)
    {
        targets.retain(|target| target.album != current.album);
        targets.push(current);
    }
    if targets.is_empty() {
        Err("No Artist Album has a saved JPG/PNG/WebP cover and Release ID; COMP fallback is excluded."
            .to_string())
    } else {
        Ok(targets)
    }
}

fn artist_album_queue_ready(app: &ApplicationState) -> bool {
    if app.library.view != LibraryView::Artists || app.library.focused_artist.is_none() {
        return false;
    }
    let indices = app.library.artist_overview_album_indices(
        &app.artist_overview_album_keys,
        app.artist_overview_show_all,
    );
    indices.into_iter().any(|index| {
        app.library.albums.get(index).is_some_and(|album| {
            !album.snapshot.album_mbid.trim().is_empty()
                && (saved_album_queue_target(app, album).is_some()
                    || (album
                        .snapshot
                        .path
                        .eq_ignore_ascii_case(&app.current_album_path)
                        && current_embed_queue_target(app).is_some()))
        })
    })
}

fn album_overview_queue_target(app: &ApplicationState) -> Result<EmbedQueueTarget, String> {
    if app.library.view != LibraryView::Albums || app.library.show_tracks {
        return Err("Select an Album before staging it.".to_string());
    }
    let key = app
        .library
        .focused_album_key
        .as_deref()
        .ok_or_else(|| "Select an Album before staging it.".to_string())?;
    let album = app
        .library
        .albums
        .iter()
        .find(|album| album.snapshot.album_key.eq_ignore_ascii_case(key))
        .ok_or_else(|| "The selected Album is no longer available.".to_string())?;
    if album.snapshot.album_mbid.trim().is_empty() {
        return Err("COMP fallback tracks must be embedded individually.".to_string());
    }
    if album
        .snapshot
        .path
        .eq_ignore_ascii_case(&app.current_album_path)
        && let Ok(target) = snapshot_embed_queue_target_from_app(app)
    {
        return Ok(target);
    }
    saved_album_queue_target(app, album)
        .ok_or_else(|| "The selected Album needs a readable saved cover.".to_string())
}

fn album_overview_queue_ready(app: &ApplicationState) -> bool {
    if app.library.view != LibraryView::Albums || app.library.show_tracks {
        return false;
    }
    app.library.focused_album_key.as_deref().is_some_and(|key| {
        app.library.albums.iter().any(|album| {
            album.snapshot.album_key.eq_ignore_ascii_case(key)
                && !album.snapshot.album_mbid.trim().is_empty()
                && (saved_album_queue_target(app, album).is_some()
                    || (album
                        .snapshot
                        .path
                        .eq_ignore_ascii_case(&app.current_album_path)
                        && current_embed_queue_target(app).is_some()))
        })
    })
}

struct StagedEmbedAlbum {
    id: i64,
    album: PathBuf,
    written_cover: Option<PathBuf>,
    snapshot: Option<MediaSnapshot>,
}

fn write_staged_album_cover(
    album: &Path,
    source: &[u8],
    config: &Config,
    preferred_cover: Option<&Path>,
) -> Result<PathBuf, String> {
    let image = image::load_from_memory(source)
        .map_err(|error| format!("Unable to decode the selected full-resolution cover: {error}"))?;
    let format = image::guess_format(source)
        .map_err(|error| format!("Unable to identify the selected cover format: {error}"))?;
    if !matches!(
        format,
        image::ImageFormat::Jpeg | image::ImageFormat::Png | image::ImageFormat::WebP
    ) {
        return Err("The selected cover must be JPEG, PNG, or WebP.".to_string());
    }
    let preferred_is_png = preferred_cover.is_some_and(|cover| {
        cover
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
    });
    let target_png =
        preferred_is_png || (preferred_cover.is_none() && format == image::ImageFormat::Png);
    let expected = if target_png {
        StaticFormat::Png
    } else {
        StaticFormat::Jpeg
    };
    let bytes = if (target_png && format == image::ImageFormat::Png)
        || (!target_png && format == image::ImageFormat::Jpeg)
    {
        source.to_vec()
    } else if target_png {
        let mut encoded = Cursor::new(Vec::new());
        image
            .write_to(&mut encoded, image::ImageFormat::Png)
            .map_err(|error| format!("Unable to encode the selected cover as PNG: {error}"))?;
        encoded.into_inner()
    } else {
        let mut encoded = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 92)
            .encode_image(&image)
            .map_err(|error| format!("Unable to encode the selected cover as JPEG: {error}"))?;
        encoded
    };
    let destination = preferred_cover.map(Path::to_path_buf).unwrap_or_else(|| {
        album.join(format!(
            "{}.{}",
            config.output.file_name,
            if target_png { "png" } else { "jpg" }
        ))
    });
    let dimensions = (image.width(), image.height());
    replace_binary_file(&destination, &bytes, "staged album cover", |staged| {
        let info = inspect_image(staged)?;
        if (info.width, info.height, info.format) == (dimensions.0, dimensions.1, expected) {
            Ok(())
        } else {
            Err("The staged cover does not match the selected artwork.".to_string())
        }
    })?;
    let mut completion_config = config.clone();
    completion_config.mode = Mode::Write;
    completion_config.scan.scan_mode_timeout = ScanTimeout(0.0);
    record_album_outcome(&completion_config, album, "installed", Some("embed-queue"))?;
    Ok(destination)
}

fn queue_saved_album(
    database: &Path,
    target: EmbedQueueTarget,
) -> Result<StagedEmbedAlbum, String> {
    let tracks = splined_core::scan::inventory_album_directories(&target.album, &[])?
        .albums
        .into_iter()
        .find(|entry| entry.path == target.album)
        .map(|entry| entry.audio_files.len())
        .unwrap_or_default();
    if tracks == 0 {
        return Err("The selected album has no supported audio tracks.".to_string());
    }
    let (thumbnail, cover_width, cover_height, cover_to_write) = match &target.source {
        EmbedQueueSource::File(cover) => {
            let source = fs::read(&cover).map_err(|error| {
                format!("Unable to read the album cover for the queue: {error}")
            })?;
            let (width, height) = image::image_dimensions(&cover)
                .map_err(|error| format!("Unable to inspect the album cover: {error}"))?;
            (
                prepare_embedded_thumbnail(&source, target.side, StaticFormat::Jpeg)?,
                width,
                height,
                (!cover.parent().is_some_and(|parent| {
                    parent
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&target.album.to_string_lossy())
                }))
                .then_some(source),
            )
        }
        EmbedQueueSource::Preview(source) => {
            let preview = image::load_from_memory(&source)
                .map_err(|error| format!("Unable to inspect the upscale preview: {error}"))?;
            (
                prepare_embedded_thumbnail(&source, target.side, StaticFormat::Jpeg)?,
                preview.width(),
                preview.height(),
                Some(source.clone()),
            )
        }
        EmbedQueueSource::PreparedThumbnail(bytes) => {
            let image = image::load_from_memory(&bytes)
                .map_err(|error| format!("Unable to decode prepared thumbnail: {error}"))?;
            if image.width() != target.side || image.height() != target.side {
                return Err("The prepared thumbnail no longer matches the preferred size.".into());
            }
            (bytes.clone(), image.width(), image.height(), None)
        }
    };
    let id = embed_queue::enqueue(
        database,
        &target.album,
        &target.label,
        &thumbnail,
        target.side,
        cover_width,
        cover_height,
        tracks,
    )?;
    let (written_cover, snapshot) = match (cover_to_write, target.cover_config) {
        (Some(source), Some(config)) => {
            match write_staged_album_cover(
                &target.album,
                &source,
                &config,
                target.preferred_cover.as_deref(),
            )
            .and_then(|path| media_snapshot(&config, false).map(|snapshot| (path, snapshot)))
            {
                Ok((path, snapshot)) => (Some(path), Some(snapshot)),
                Err(error) => {
                    let cleanup = embed_queue::remove(database, id);
                    return Err(match cleanup {
                        Ok(()) => error,
                        Err(cleanup_error) => format!(
                            "{error} The failed queue job could not be removed: {cleanup_error}"
                        ),
                    });
                }
            }
        }
        _ => (None, None),
    };
    Ok(StagedEmbedAlbum {
        id,
        album: target.album,
        written_cover,
        snapshot,
    })
}

fn apply_staged_album_cover(
    state: &Arc<Mutex<ApplicationState>>,
    staged: &mut StagedEmbedAlbum,
) -> Result<bool, String> {
    let Some(snapshot) = staged.snapshot.take() else {
        return Ok(false);
    };
    let mut app = state
        .lock()
        .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
    app.library.replace_snapshot(snapshot)?;
    let selected = Path::new(&app.current_album_path) == staged.album;
    if selected && let Some(cover) = &staged.written_cover {
        app.current_cover_path = cover.to_string_lossy().into_owned();
    }
    let advance = selected
        && matches!(
            app.phase,
            RunPhase::AwaitingCandidate | RunPhase::AwaitingMusicBrainz
        );
    if selected && !advance {
        clear_staged_candidate_review(&mut app);
    }
    Ok(advance)
}

fn clear_staged_candidate_review(app: &mut ApplicationState) {
    app.edit_preview_path.clear();
    app.embedded_thumbnail_preview_path.clear();
    app.embedded_thumbnail_owner_key.clear();
    app.musicbrainz_cover_preview_path.clear();
    app.candidates.clear();
    app.selected_candidate = None;
    app.comparison_candidates.clear();
}

fn ensure_webp_album_still(album: &Path) -> Result<Option<PathBuf>, String> {
    let mut webp = None;
    let mut has_still = false;
    for entry in fs::read_dir(album).map_err(|error| {
        format!(
            "Unable to inspect queued album {}: {error}",
            album.display()
        )
    })? {
        let entry =
            entry.map_err(|error| format!("Unable to inspect queued album entry: {error}"))?;
        if !entry
            .file_type()
            .map_err(|error| format!("Unable to inspect album file type: {error}"))?
            .is_file()
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        match name.as_str() {
            "cover.webp" => webp = Some(entry.path()),
            "cover.jpg" | "cover.jpeg" | "cover.png" => has_still = true,
            _ => {}
        }
    }
    let Some(webp) = webp else { return Ok(None) };
    if has_still {
        return Ok(None);
    }
    let bytes =
        fs::read(&webp).map_err(|error| format!("Unable to read {}: {error}", webp.display()))?;
    let source = image::load_from_memory(&bytes)
        .map_err(|error| format!("Unable to decode {}: {error}", webp.display()))?;
    let dimensions = (source.width(), source.height());
    let mut encoded = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(source.to_rgb8())
        .write_to(&mut encoded, image::ImageFormat::Jpeg)
        .map_err(|error| format!("Unable to create a still from {}: {error}", webp.display()))?;
    let still = album.join("cover.jpg");
    if still.is_file() {
        return Ok(None);
    }
    replace_binary_file(
        &still,
        &encoded.into_inner(),
        "WebP album still",
        |staged| {
            let info = inspect_image(staged)?;
            if (info.width, info.height) == dimensions && info.format == StaticFormat::Jpeg {
                Ok(())
            } else {
                Err("The staged WebP still did not match the original dimensions.".to_string())
            }
        },
    )?;
    Ok(Some(still))
}

fn notify_embed_queue_progress(
    queue_weak: &slint::Weak<EmbedQueueWindow>,
    main_weak: &slint::Weak<AppWindow>,
    state: &Arc<Mutex<ApplicationState>>,
    database: &Path,
    message: String,
    finished: bool,
) {
    let queue_weak = queue_weak.clone();
    let main_weak = main_weak.clone();
    let state = Arc::clone(state);
    let database = database.to_path_buf();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(window) = queue_weak.upgrade() {
            window.set_queue_status(message.clone().into());
            if finished {
                window.set_current_key(SharedString::default());
            }
            let _ = sync_embed_queue_window(&window, &state, &database);
        }
        if finished {
            let _ = sync_embed_queue_locks(&state, &database);
            if let Ok(mut app) = state.lock() {
                app.push_log(message);
            }
            refresh_weak(&main_weak, &state);
        }
    });
}

fn run_embed_queue_job(
    job: &embed_queue::EmbedJob,
    database: &Path,
    queue_weak: &slint::Weak<EmbedQueueWindow>,
    main_weak: &slint::Weak<AppWindow>,
    state: &Arc<Mutex<ApplicationState>>,
    pause_all: &AtomicBool,
) -> Result<bool, String> {
    if let Some(still) = ensure_webp_album_still(&job.album_path)?
        && let Ok(mut app) = state.lock()
    {
        for album in &mut app.library.albums {
            if album
                .snapshot
                .path
                .eq_ignore_ascii_case(&job.album_path.to_string_lossy())
            {
                album
                    .snapshot
                    .local_artwork_files
                    .push(still.to_string_lossy().into_owned());
            }
        }
    }
    let inventory = splined_core::scan::inventory_album_directories(&job.album_path, &[])?;
    let album = inventory
        .albums
        .iter()
        .find(|entry| entry.path == job.album_path)
        .ok_or_else(|| "The queued album no longer has supported audio tracks.".to_string())?;
    let total = album.audio_files.len();
    let mut errors = Vec::new();
    notify_embed_queue_progress(
        queue_weak,
        main_weak,
        state,
        database,
        format!("Embedding {} · 0/{total} tracks", job.label),
        false,
    );
    for (index, track) in album.audio_files.iter().enumerate().skip(job.completed) {
        if pause_all.load(Ordering::Relaxed) {
            if !errors.is_empty() {
                return Err(format!(
                    "{} of {total} tracks failed. First: {}",
                    errors.len(),
                    errors[0]
                ));
            }
            embed_queue::pause_running(database, job.id, index, total)?;
            return Ok(true);
        }
        let track_started = Instant::now();
        let mut phases = Vec::new();
        let embed_result =
            replace_embedded_front_with_trace(track, &job.thumbnail, &mut |phase, elapsed| {
                phases.push(format!("{phase}={}ms", elapsed.as_millis()))
            });
        let embed_ms = track_started.elapsed().as_millis();
        if let Err(error) = &embed_result {
            errors.push(format!("{}: {error}", track.display()));
        }
        let completed = index + 1;
        let progress_started = Instant::now();
        let progress_result = embed_queue::progress(database, job.id, completed, total);
        if let Some(log) = UI_DEBUG_LOG_HANDLE.get() {
            log.record(format!(
                "embed-track job={} index={}/{} embed_ms={} progress_ms={} phases={} embed_result={} progress_result={} track={} error={}",
                job.id,
                completed,
                total,
                embed_ms,
                progress_started.elapsed().as_millis(),
                phases.join(","),
                if embed_result.is_ok() { "ok" } else { "error" },
                if progress_result.is_ok() { "ok" } else { "error" },
                track.display(),
                embed_result
                    .as_ref()
                    .err()
                    .or_else(|| progress_result.as_ref().err())
                    .map(String::as_str)
                    .unwrap_or("")
            ));
        }
        progress_result?;
        notify_embed_queue_progress(
            queue_weak,
            main_weak,
            state,
            database,
            format!("Embedding {} · {completed}/{total} tracks", job.label),
            false,
        );
    }
    if errors.is_empty() {
        let mut completion_config = state
            .lock()
            .map_err(|_| "The SPLINED application state is unavailable.".to_string())?
            .config
            .clone();
        completion_config.mode = Mode::Write;
        completion_config.scan.scan_mode_timeout = ScanTimeout(0.0);
        record_album_outcome(
            &completion_config,
            &job.album_path,
            "installed",
            Some("embed-queue"),
        )?;
        let snapshot = media_snapshot(&completion_config, false)?;
        if let Ok(mut app) = state.lock() {
            app.library.replace_snapshot(snapshot)?;
        }
        invalidate_album_embedded_check(&job.album_path);
        Ok(false)
    } else {
        invalidate_album_embedded_check(&job.album_path);
        Err(format!(
            "{} of {total} tracks failed. First: {}",
            errors.len(),
            errors[0]
        ))
    }
}

fn bind_embed_queue(
    ui: &AppWindow,
    state: Arc<Mutex<ApplicationState>>,
    controller: Arc<ScanController>,
) -> Result<Arc<AtomicBool>, String> {
    let database = embed_queue::local_queue_path();
    embed_queue::recover_interrupted(&database)?;
    sync_embed_queue_locks(&state, &database)?;
    let window = Rc::new(
        EmbedQueueWindow::new()
            .map_err(|error| format!("Unable to create the embed queue window: {error}"))?,
    );
    let queue_weak = window.as_weak();
    EMBED_QUEUE_WINDOW.with(|slot| *slot.borrow_mut() = Some(queue_weak.clone()));
    let main_weak = ui.as_weak();
    EMBED_MAIN_WINDOW.with(|slot| *slot.borrow_mut() = Some(main_weak.clone()));
    let paused = Arc::new(AtomicBool::new(false));
    let pause_all = Arc::new(AtomicBool::new(false));
    let stopped = Arc::new(AtomicBool::new(false));
    let staging_count = Arc::new(AtomicUsize::new(0));
    {
        let window = window.clone();
        let state = Arc::clone(&state);
        let database = database.clone();
        let main_weak = main_weak.clone();
        ui.on_open_embed_queue(move || {
            window.set_current_key(SharedString::default());
            window.set_queue_message(SharedString::default());
            if let Err(error) = sync_embed_queue_window(&window, &state, &database) {
                window.set_queue_status(format!("Embed queue error: {error}").into());
            }
            if let Some(main) = main_weak.upgrade() { main.set_workspace_mode(3); }
            if window.get_current_ready() {
                window.invoke_queue_current();
            }
        });
    }
    {
        let window = window.clone();
        let state = Arc::clone(&state);
        let database = database.clone();
        ui.on_show_embed_queue(move || {
            window.set_current_key(SharedString::default());
            if let Err(error) = sync_embed_queue_window(&window, &state, &database) {
                window.set_queue_status(format!("Embed queue error: {error}").into());
            }
        });
    }
    {
        let window = window.clone();
        ui.on_embed_panel_start_staged(move || window.invoke_start_staged());
    }
    {
        let window = window.clone();
        ui.on_embed_panel_set_paused(move |value| {
            window.set_paused(value);
            window.invoke_set_paused(value);
        });
    }
    {
        let window = window.clone();
        ui.on_embed_panel_set_pause_all(move |value| {
            window.set_pause_all(value);
            window.invoke_set_pause_all(value);
        });
    }
    {
        let window = window.clone();
        ui.on_embed_panel_retry(move |id| window.invoke_retry(id));
    }
    {
        let window = window.clone();
        ui.on_embed_panel_remove(move |id| window.invoke_remove(id));
    }
    {
        let window = window.clone();
        ui.on_embed_panel_remove_all(move || window.invoke_remove_all());
    }
    {
        let window = window.clone();
        let state = Arc::clone(&state);
        let database = database.clone();
        let queue_weak = queue_weak.clone();
        let main_weak = main_weak.clone();
        let staging_count = Arc::clone(&staging_count);
        let controller = Arc::clone(&controller);
        ui.on_queue_artist_albums(move || {
            let targets = state
                .lock()
                .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                .and_then(|app| {
                    if app.library.view == LibraryView::Albums && !app.library.show_tracks {
                        album_overview_queue_target(&app).map(|target| vec![target])
                    } else {
                        artist_album_queue_targets(&app)
                    }
                });
            let targets = match targets {
                Ok(targets) => targets,
                Err(error) => {
                    window.set_queue_message(error.into());
                    if let Some(main) = main_weak.upgrade() { main.set_workspace_mode(3); }
                    let _ = sync_embed_queue_window(&window, &state, &database);
                    return;
                }
            };
            let total = targets.len();
            window.set_current_key(SharedString::default());
            window.set_queue_message(
                format!(
                    "Staging {total} Album{}…",
                    if total == 1 { "" } else { "s" }
                )
                .into(),
            );
            window.set_staging_count(staging_count.fetch_add(1, Ordering::Relaxed) as i32 + 1);
            if let Some(main) = main_weak.upgrade() { main.set_workspace_mode(3); }
            let _ = sync_embed_queue_window(&window, &state, &database);
            let state = Arc::clone(&state);
            let database = database.clone();
            let queue_weak = queue_weak.clone();
            let main_weak = main_weak.clone();
            let staging_count = Arc::clone(&staging_count);
            let controller = Arc::clone(&controller);
            std::thread::spawn(move || {
                let mut added = 0usize;
                let mut skipped = 0usize;
                for (index, target) in targets.into_iter().enumerate() {
                    let mut result = queue_saved_album(&database, target);
                    if result.is_ok() {
                        added += 1;
                    } else {
                        skipped += 1;
                    }
                    let state = Arc::clone(&state);
                    let database = database.clone();
                    let queue_weak = queue_weak.clone();
                    let main_weak = main_weak.clone();
                    let controller = Arc::clone(&controller);
                    let message = format!(
                        "Artist Albums staged: {added}/{total} · {skipped} skipped · checked {}/{}",
                        index + 1,
                        total
                    );
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Ok(job) = &mut result
                            && let Ok(true) = apply_staged_album_cover(&state, job)
                        {
                            let _ = controller
                                .submit_decision(serde_json::json!({"action": "skip_album"}));
                            if let Ok(mut app) = state.lock() {
                                clear_staged_candidate_review(&mut app);
                            }
                        }
                        let _ = sync_embed_queue_locks(&state, &database);
                        if let Some(window) = queue_weak.upgrade() {
                            window.set_queue_message(message.into());
                            let _ = sync_embed_queue_window(&window, &state, &database);
                        }
                        refresh_weak(&main_weak, &state);
                    });
                }
                staging_count.fetch_sub(1, Ordering::Relaxed);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(window) = queue_weak.upgrade() {
                        window.set_staging_count(staging_count.load(Ordering::Relaxed) as i32);
                    }
                });
            });
        });
    }
    {
        let weak = queue_weak.clone();
        window.on_close_queue(move || {
            if let Some(window) = weak.upgrade() {
                let _ = window.hide();
            }
        });
    }
    {
        window
            .window()
            .on_close_requested(move || slint::CloseRequestResponse::HideWindow);
    }
    {
        let paused = Arc::clone(&paused);
        let weak = queue_weak.clone();
        let state = Arc::clone(&state);
        let database = database.clone();
        window.on_set_paused(move |value| {
            paused.store(value, Ordering::Relaxed);
            if let Some(window) = weak.upgrade() {
                window.set_queue_status(if value {
                    "Paused after the current album".into()
                } else {
                    "Embed queue resumed".into()
                });
                let _ = sync_embed_queue_window(&window, &state, &database);
            }
        });
    }
    {
        let pause_all = Arc::clone(&pause_all);
        let weak = queue_weak.clone();
        let state = Arc::clone(&state);
        let database = database.clone();
        window.on_set_pause_all(move |value| {
            pause_all.store(value, Ordering::Relaxed);
            if let Some(window) = weak.upgrade() {
                let message = if value {
                    "Pausing all albums after the current track".to_string()
                } else {
                    match embed_queue::resume_paused(&database) {
                        Ok(count) => format!("Resumed {count} paused album(s)"),
                        Err(error) => error,
                    }
                };
                window.set_queue_status(message.into());
                let _ = sync_embed_queue_window(&window, &state, &database);
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let database = database.clone();
        let weak = queue_weak.clone();
        let main_weak = main_weak.clone();
        let staging_count = Arc::clone(&staging_count);
        let controller = Arc::clone(&controller);
        window.on_queue_current(move || {
            let target = snapshot_embed_queue_target(&state);
            let is_staging = target.is_ok();
            if is_staging {
                let pending = staging_count.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(window) = weak.upgrade() {
                    window.set_staging_count(pending as i32);
                }
            }
            let state = Arc::clone(&state);
            let database = database.clone();
            let weak = weak.clone();
            let main_weak = main_weak.clone();
            let staging_count = Arc::clone(&staging_count);
            let controller = Arc::clone(&controller);
            std::thread::spawn(move || {
                let mut result = target.and_then(|target| queue_saved_album(&database, target));
                if is_staging {
                    staging_count.fetch_sub(1, Ordering::Relaxed);
                }
                let _ = slint::invoke_from_event_loop(move || {
                    if result.is_ok() {
                        let _ = sync_embed_queue_locks(&state, &database);
                    }
                    if let Ok(job) = &mut result {
                        match apply_staged_album_cover(&state, job) {
                            Ok(true) => {
                                match controller.submit_decision(serde_json::json!({"action": "skip_album"})) {
                                    Ok(()) => {
                                        if let Ok(mut app) = state.lock() {
                                            clear_staged_candidate_review(&mut app);
                                        }
                                    }
                                    Err(error) => {
                                        if let Ok(mut app) = state.lock() {
                                            app.push_log(format!("Cover staged; candidate review could not advance: {error}"));
                                        }
                                    }
                                }
                            }
                            Ok(false) => {}
                            Err(error) => {
                                if let Ok(mut app) = state.lock() {
                                    app.push_log(format!("Cover staged; album view did not refresh: {error}"));
                                }
                            }
                        }
                    }
                    if let Ok(mut app) = state.lock() {
                        match &result {
                            Ok(job) => {
                                app.push_log(format!("Album staged for embedding (job {}).", job.id))
                            }
                            Err(error) => app.push_log(format!("Embed queue: {error}")),
                        }
                    }
                    if let Some(window) = weak.upgrade() {
                        window.set_staging_count(staging_count.load(Ordering::Relaxed) as i32);
                        let message = match &result {
                            Ok(job) => {
                                format!("Album staged for embedding (job {}).", job.id)
                            }
                            Err(error) => error.clone(),
                        };
                        window.set_queue_message(message.clone().into());
                        window.set_queue_status(message.into());
                        let _ = sync_embed_queue_window(&window, &state, &database);
                    }
                    if result.is_ok() {
                        refresh_weak(&main_weak, &state);
                    }
                });
            });
        });
    }
    {
        let database = database.clone();
        let state = Arc::clone(&state);
        let weak = queue_weak.clone();
        let main_weak = main_weak.clone();
        let staging_count = Arc::clone(&staging_count);
        let controller = Arc::clone(&controller);
        ui.on_queue_album_card(move |key| {
            let target = album_card_queue_target(&state, key.as_str());
            let is_staging = target.is_ok();
            if is_staging {
                let pending = staging_count.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(window) = weak.upgrade() {
                    window.set_staging_count(pending as i32);
                }
            }
            let state = Arc::clone(&state);
            let database = database.clone();
            let weak = weak.clone();
            let main_weak = main_weak.clone();
            let staging_count = Arc::clone(&staging_count);
            let controller = Arc::clone(&controller);
            std::thread::spawn(move || {
                let mut result = target.and_then(|target| queue_saved_album(&database, target));
                if is_staging {
                    staging_count.fetch_sub(1, Ordering::Relaxed);
                }
                let _ = slint::invoke_from_event_loop(move || {
                    if result.is_ok() {
                        let _ = sync_embed_queue_locks(&state, &database);
                    }
                    if let Ok(job) = &mut result {
                        match apply_staged_album_cover(&state, job) {
                            Ok(true) => {
                                match controller.submit_decision(serde_json::json!({"action": "skip_album"})) {
                                    Ok(()) => {
                                        if let Ok(mut app) = state.lock() {
                                            clear_staged_candidate_review(&mut app);
                                        }
                                    }
                                    Err(error) => {
                                        if let Ok(mut app) = state.lock() {
                                            app.push_log(format!("Cover staged; candidate review could not advance: {error}"));
                                        }
                                    }
                                }
                            }
                            Ok(false) => {}
                            Err(error) => {
                                if let Ok(mut app) = state.lock() {
                                    app.push_log(format!("Cover staged; album view did not refresh: {error}"));
                                }
                            }
                        }
                    }
                    let message = match &result {
                        Ok(job) => format!("Album staged for embedding (job {}).", job.id),
                        Err(error) => error.clone(),
                    };
                    if let Ok(mut app) = state.lock() {
                        app.push_log(message.clone());
                    }
                    if let Some(window) = weak.upgrade() {
                        window.set_staging_count(staging_count.load(Ordering::Relaxed) as i32);
                        window.set_queue_message(message.clone().into());
                        window.set_queue_status(message.into());
                        let _ = sync_embed_queue_window(&window, &state, &database);
                    }
                    refresh_weak(&main_weak, &state);
                });
            });
        });
    }
    {
        let database = database.clone();
        let state = Arc::clone(&state);
        let weak = queue_weak.clone();
        let main_weak = main_weak.clone();
        let staging_count = Arc::clone(&staging_count);
        window.on_start_staged(move || {
            let pending = staging_count.load(Ordering::Relaxed);
            if pending > 0 {
                if let Some(window) = weak.upgrade() {
                    window.set_queue_status(
                        format!("Wait for {pending} album staging task(s) to finish.").into(),
                    );
                }
                return;
            }
            let message = match embed_queue::start_staged(&database) {
                Ok(0) => "No staged albums to start.".to_string(),
                Ok(count) => {
                    format!("Started {count} staged album(s); embedding runs in the background.")
                }
                Err(error) => error,
            };
            if let Ok(mut app) = state.lock() {
                app.push_log(message.clone());
            }
            if let Some(window) = weak.upgrade() {
                window.set_queue_status(message.into());
                let _ = sync_embed_queue_window(&window, &state, &database);
            }
            refresh_weak(&main_weak, &state);
        });
    }
    {
        let database = database.clone();
        let state = Arc::clone(&state);
        let weak = queue_weak.clone();
        let main_weak = main_weak.clone();
        window.on_retry(move |id| {
            if let Some(window) = weak.upgrade() {
                let message = match embed_queue::retry(&database, i64::from(id)) {
                    Ok(()) => format!("Queued job {id} to retry"),
                    Err(error) => error,
                };
                window.set_queue_status(message.into());
                let _ = sync_embed_queue_window(&window, &state, &database);
                let _ = sync_embed_queue_locks(&state, &database);
                refresh_weak(&main_weak, &state);
            }
        });
    }
    {
        let database = database.clone();
        let state = Arc::clone(&state);
        let weak = queue_weak.clone();
        let main_weak = main_weak.clone();
        window.on_remove(move |id| {
            if let Some(window) = weak.upgrade() {
                let message = match embed_queue::remove(&database, i64::from(id)) {
                    Ok(()) => format!("Removed job {id}"),
                    Err(error) => error,
                };
                window.set_queue_status(message.into());
                let _ = sync_embed_queue_window(&window, &state, &database);
                let _ = sync_embed_queue_locks(&state, &database);
                refresh_weak(&main_weak, &state);
            }
        });
    }
    {
        let database = database.clone();
        let state = Arc::clone(&state);
        let weak = queue_weak.clone();
        let main_weak = main_weak.clone();
        window.on_remove_all(move || {
            let message = match embed_queue::remove_all(&database) {
                Ok(count) => format!("Removed {count} embed album(s)."),
                Err(error) => error,
            };
            if let Some(window) = weak.upgrade() {
                window.set_queue_status(message.into());
                let _ = sync_embed_queue_window(&window, &state, &database);
            }
            let _ = sync_embed_queue_locks(&state, &database);
            refresh_weak(&main_weak, &state);
        });
    }
    {
        let stopped = Arc::clone(&stopped);
        let state = Arc::clone(&state);
        let weak = main_weak.clone();
        let queue_weak = queue_weak.clone();
        let database = database.clone();
        std::thread::spawn(move || {
            let mut blink = false;
            let mut last_artist = state
                .lock()
                .ok()
                .and_then(|app| app.library.focused_artist.clone());
            while !stopped.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(600));
                blink = !blink;
                let (active, artist) = state
                    .lock()
                    .map(|app| {
                        (
                            !app.embed_locked_album_paths.is_empty(),
                            app.library.focused_artist.clone(),
                        )
                    })
                    .unwrap_or((false, None));
                let changed_artist = artist.is_some() && artist != last_artist;
                last_artist = artist;
                if changed_artist {
                    let result = embed_queue::remove_completed(&database);
                    let queue_weak = queue_weak.clone();
                    let state = Arc::clone(&state);
                    let database = database.clone();
                    let weak = weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Ok(count) = result {
                            if count > 0
                                && let Some(window) = queue_weak.upgrade()
                            {
                                window.set_queue_status(
                                    format!(
                                        "Removed {count} completed album(s) after artist change."
                                    )
                                    .into(),
                                );
                                let _ = sync_embed_queue_window(&window, &state, &database);
                            }
                        }
                        refresh_weak(&weak, &state);
                    });
                }
                let weak = weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.set_embed_queue_blink(active && blink);
                    }
                });
            }
        });
    }
    {
        let stopped = Arc::clone(&stopped);
        let pause_all = Arc::clone(&pause_all);
        std::thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                if paused.load(Ordering::Relaxed) || pause_all.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(500));
                    continue;
                }
                match embed_queue::claim_next(&database) {
                    Ok(Some(job)) => {
                        let result = run_embed_queue_job(
                            &job,
                            &database,
                            &queue_weak,
                            &main_weak,
                            &state,
                            &pause_all,
                        );
                        let was_paused = matches!(result, Ok(true));
                        let error = result.err();
                        if let Some(reason) = error.as_deref()
                            && let Some(log) = UI_DEBUG_LOG_HANDLE.get()
                        {
                            log.record(format!(
                                "embed-queue-failed album={} job={} error={reason}",
                                job.album_path.display(),
                                job.id
                            ));
                        }
                        if !was_paused {
                            let _ = embed_queue::finish(&database, job.id, error.as_deref());
                        }
                        notify_embed_queue_progress(
                            &queue_weak,
                            &main_weak,
                            &state,
                            &database,
                            match error {
                                Some(error) => {
                                    format!("Embed queue failed · {} · {error}", job.label)
                                }
                                None if was_paused => {
                                    format!("Paused {} · resume this album to continue", job.label)
                                }
                                None => format!("Embedded {} · all tracks complete", job.label),
                            },
                            true,
                        );
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(500)),
                    Err(error) => {
                        notify_embed_queue_progress(
                            &queue_weak,
                            &main_weak,
                            &state,
                            &database,
                            format!("Embed queue error: {error}"),
                            true,
                        );
                        std::thread::sleep(Duration::from_secs(2));
                    }
                }
            }
        });
    }
    Ok(stopped)
}

fn bind_callbacks(
    ui: &AppWindow,
    state: Arc<Mutex<ApplicationState>>,
    controller: Arc<ScanController>,
    workspace_layout: Rc<RefCell<WorkspaceLayout>>,
    appearance: Rc<RefCell<String>>,
    custom_window_state: CustomWindowState,
    diagnostics: UiWindowDiagnostics,
    interface_restore_pending_restart: Arc<AtomicBool>,
) {
    let UiWindowDiagnostics {
        maximize_guard,
        ui_debug_log,
    } = diagnostics;
    let compare_window: Rc<RefCell<Option<CandidateCompareWindow>>> = Rc::new(RefCell::new(None));
    let adjustment_preview_revision = Arc::new(AtomicU64::new(0));
    let adjustment_preview_gate = Arc::new(Mutex::new(()));
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_save_artist_lookup(move |lookup_name, musicbrainz_id| {
            let result = (|| {
                let artist = {
                    let app = state
                        .lock()
                        .map_err(|_| "Application state is unavailable.".to_string())?;
                    overview_artist_name(&app)
                        .map(str::to_owned)
                        .ok_or_else(|| "Select an Artist before saving a lookup.".to_string())?
                };
                let name = lookup_name.trim().to_string();
                let id = musicbrainz_id.trim().to_string();
                if name.is_empty() && id.is_empty() {
                    return Err(
                        "Enter a Navidrome lookup name or an album's MusicBrainz Artist ID.".into(),
                    );
                }
                if !id.is_empty() && !is_valid_mbid(&id) {
                    return Err("Artist ID must be a valid MusicBrainz UUID.".into());
                }
                let hint = ArtistLookupOverride {
                    artist: artist.clone(),
                    lookup_name: name,
                    musicbrainz_id: id,
                };
                let mut preferences = splined_core::settings::load_ui_preferences()?;
                preferences
                    .artist_lookup_overrides
                    .retain(|entry| !entry.artist.eq_ignore_ascii_case(&artist));
                preferences.artist_lookup_overrides.push(hint.clone());
                splined_core::settings::save_ui_preferences(&preferences)?;
                if let Ok(mut app) = state.lock() {
                    app.artist_lookup_overrides
                        .insert(artist.trim().to_lowercase(), hint);
                    app.artist_overview_cache
                        .remove(&artist.trim().to_lowercase());
                    app.operation_status =
                        format!("Saved {artist} Artist lookup · refreshing Navidrome");
                    app.operation_tone = "cyan".into();
                }
                Ok::<_, String>(artist)
            })();
            match result {
                Ok(artist) => refresh_navidrome_thumbnails(&state, weak.clone(), Some(artist)),
                Err(error) => record_action_result(&state, Err(error)),
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let workspace_layout = Rc::clone(&workspace_layout);
        let maximize_guard = maximize_guard.clone();
        let ui_debug_log = ui_debug_log.clone();
        ui.on_window_resized(move |width, height| {
            if let Some(log) = ui_debug_log.as_ref() {
                let maximized = weak.upgrade().is_some_and(|ui| ui.window().is_maximized());
                let viewport_height = weak
                    .upgrade()
                    .map(|ui| ui.get_viewport_height())
                    .unwrap_or_default();
                let content_layout_height = weak
                    .upgrade()
                    .map(|ui| ui.get_content_layout_height())
                    .unwrap_or_default();
                let workspace_page_height = weak
                    .upgrade()
                    .map(|ui| ui.get_workspace_page_height())
                    .unwrap_or_default();
                log.record(format!(
                    "main-window-resized size={:.0}x{:.0} viewport_height={viewport_height:.0} content_layout_height={content_layout_height:.0} workspace_page_height={workspace_page_height:.0} maximized={maximized}",
                    width, height,
                ));
            }
            if let Some(ui) = weak.upgrade() {
                let maximized = ui.window().is_maximized();
                if maximize_guard.maintain(
                    maximized,
                    logical_dimension(width, MIN_WINDOW_WIDTH),
                    logical_dimension(height, MIN_WINDOW_HEIGHT),
                ) {
                    if !maximized {
                        ui.window().set_maximized(true);
                    }
                    return;
                }
            }
            let flags = state.lock().ok().map(|state| {
                (
                    state.media_selector_expanded,
                    state.show_artwork,
                    state.candidate_filter_expanded,
                )
            });
            let Some((media_expanded, artwork_visible, candidate_filter_expanded)) = flags else {
                return;
            };
            let mut layout = workspace_layout.borrow_mut();
            layout.clamp_for_window(
                logical_dimension(width, MIN_WINDOW_WIDTH),
                logical_dimension(height, MIN_WINDOW_HEIGHT),
                media_expanded,
                artwork_visible,
                candidate_filter_expanded,
            );
            if let Some(ui) = weak.upgrade() {
                apply_workspace_layout(&ui, &layout);
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let workspace_layout = Rc::clone(&workspace_layout);
        let maximize_guard = maximize_guard.clone();
        let ui_debug_log = ui_debug_log.clone();
        ui.on_pane_resized(move |region, value| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let (window_width, window_height) = app_logical_window_size(&ui);
            maximize_guard.arm(ui.window().is_maximized(), window_width, window_height);
            let flags = state.lock().ok().map(|state| {
                (
                    state.media_selector_expanded,
                    state.show_artwork,
                    state.candidate_filter_expanded,
                )
            });
            let Some((media_expanded, artwork_visible, candidate_filter_expanded)) = flags else {
                return;
            };
            let requested = if value.is_finite() {
                value.max(0.0).round() as u32
            } else {
                0
            };
            let mut layout = workspace_layout.borrow_mut();
            layout.resize_region(
                region.as_str(),
                requested,
                WorkspaceBounds {
                    window_width,
                    window_height,
                    media_expanded,
                    artwork_visible,
                    candidate_filter_expanded,
                },
            );
            if let Some(log) = ui_debug_log.as_ref() {
                log.record(format!(
                    "main-pane-resized region={} requested={requested} window={window_width}x{window_height} maximized={} media={} artwork={} filter={} activity={}",
                    region.as_str(),
                    ui.window().is_maximized(),
                    layout.media_pane_width,
                    layout.artwork_pane_width,
                    layout.candidate_filter_height,
                    layout.activity_pane_height
                ));
            }
            apply_workspace_layout(&ui, &layout);
        });
    }
    {
        let appearance = Rc::clone(&appearance);
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_set_appearance(move |value| {
            let normalized = normalize_appearance(value.as_str());
            *appearance.borrow_mut() = normalized.clone();
            if let Ok(mut app) = state.lock() {
                app.appearance_mode = normalized.clone();
            }
            if let Some(ui) = weak.upgrade() {
                ui.set_appearance_mode(SharedString::from(normalized.clone()));
            }
            EMBED_QUEUE_WINDOW.with(|slot| {
                if let Some(window) = slot.borrow().as_ref().and_then(|weak| weak.upgrade()) {
                    window.set_appearance_mode(normalized.into());
                }
            });
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let restore_pending = Arc::clone(&interface_restore_pending_restart);
        ui.on_save_interface_settings(move || {
            let result = (|| {
                if restore_pending.load(Ordering::SeqCst) {
                    return Err("Restart SPLINED to finish restoring Interface Settings before saving new changes.".to_string());
                }
                let ui = weak
                    .upgrade()
                    .ok_or_else(|| "The SPLINED Settings window is unavailable.".to_string())?;
                let intensity =
                    normalize_status_bar_intensity(ui.get_settings_status_bar_intensity().as_str());
                let position =
                    normalize_status_bar_position(ui.get_settings_status_bar_position().as_str());
                let size = normalize_status_bar_size(ui.get_settings_status_bar_size().as_str());
                let mut preferences = splined_core::settings::load_ui_preferences()?;
                preferences.status_bar_intensity = intensity.to_string();
                preferences.status_bar_position = position.to_string();
                preferences.status_bar_size = size.to_string();
                let saving_enhancement = ui.get_settings_section() == "Enhancement";
                if saving_enhancement && let Ok(app) = state.lock() {
                    preferences.adjustment_presets = app.adjustment_presets.clone();
                    preferences.banner_theme_styles = app.banner_theme_styles.clone();
                    preferences.album_frame_theme_styles = app.album_frame_theme_styles.clone();
                    preferences.banner_sync_frame_letters = app.banner_sync_frame_letters;
                    preferences.banner_random_harmony = app.banner_random_harmony;
                    preferences.wheel_hue_shift_degrees = app.wheel_hue_shift_degrees;
                    preferences.wheel_brightness_percent = app.wheel_brightness_percent;
                    preferences.banner_theme_baseline_styles =
                        app.banner_theme_baseline_styles.clone();
                    preferences.album_frame_theme_baseline_styles =
                        app.album_frame_theme_baseline_styles.clone();
                }
                splined_core::settings::save_ui_preferences(&preferences)?;
                ui.set_status_bar_intensity(intensity.into());
                ui.set_status_bar_position(position.into());
                ui.set_status_bar_size(size.into());
                if let Ok(mut app) = state.lock() {
                    if saving_enhancement {
                        app.enhancement_settings_dirty = false;
                    }
                    app.service_status = "Saved Interface & Status settings.".to_string();
                }
                Ok(())
            })();
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_change_library_view(move |value| {
            if let Ok(mut app) = state.lock() {
                app.library.set_top_level_view(match value {
                    1 => LibraryView::Albums,
                    2 => LibraryView::Folders,
                    _ => LibraryView::Artists,
                });
            }
            refresh_weak(&weak, &state);
        });
    }
    let media_search_debounce = Rc::new(Timer::default());
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let debounce = Rc::clone(&media_search_debounce);
        ui.on_artist_filter_edited(move |value| {
            if let Ok(mut app) = state.lock()
                && !app.media_filter_locked
            {
                app.library.set_artist_filter(value.as_str());
            }
            let weak = weak.clone();
            let state = Arc::clone(&state);
            debounce.start(
                TimerMode::SingleShot,
                Duration::from_millis(180),
                move || {
                    refresh_weak(&weak, &state);
                },
            );
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let debounce = Rc::clone(&media_search_debounce);
        ui.on_album_filter_edited(move |value| {
            if let Ok(mut app) = state.lock()
                && !app.media_filter_locked
            {
                app.library.set_album_filter(value.as_str());
            }
            let weak = weak.clone();
            let state = Arc::clone(&state);
            debounce.start(
                TimerMode::SingleShot,
                Duration::from_millis(180),
                move || {
                    refresh_weak(&weak, &state);
                },
            );
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_status_filter_toggled(move |value| {
            if let Ok(mut app) = state.lock() {
                if app.media_filter_locked {
                    // A closed lock preserves the current filter selections.
                } else if value.eq_ignore_ascii_case("Embedded Art") {
                    let enabled = !app.library.show_tracks;
                    app.library.set_show_tracks(enabled);
                } else {
                    app.library
                        .toggle_status_filter(parse_status_filter(value.as_str()));
                }
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_filter_lock_changed(move |kind, locked| {
            if let Ok(mut app) = state.lock() {
                match kind.as_str() {
                    "media" => app.media_filter_locked = locked,
                    "candidate" => app.candidate_filter_locked = locked,
                    _ => {}
                }
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_set_show_tracks(move |enabled| {
            if let Ok(mut app) = state.lock() {
                app.library.set_show_tracks(enabled);
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let workspace_layout = Rc::clone(&workspace_layout);
        let maximize_guard = maximize_guard.clone();
        let ui_debug_log = ui_debug_log.clone();
        ui.on_set_region_expanded(move |region, expanded| {
            if matches!(region.as_str(), "candidate-filter" | "artwork-editing") {
                if let Ok(mut app) = state.lock() {
                    if region.as_str() == "candidate-filter" {
                        app.candidate_filter_expanded = expanded;
                    } else {
                        app.artwork_editing_expanded = expanded;
                    }
                }
                if let Some(ui) = weak.upgrade() {
                    if region.as_str() == "candidate-filter" {
                        ui.set_candidate_filter_expanded(expanded);
                    } else {
                        ui.set_artwork_editing_expanded(expanded);
                    }
                }
                if let Some(log) = ui_debug_log.as_ref() {
                    log.record(format!(
                        "main-local-region-expanded region={} expanded={expanded}",
                        region.as_str()
                    ));
                }
                refresh_weak(&weak, &state);
                return;
            }
            let keep_maximized = weak.upgrade().is_some_and(|ui| {
                let (width, height) = app_logical_window_size(&ui);
                maximize_guard.arm(ui.window().is_maximized(), width, height)
            });
            let flags = state.lock().ok().map(|mut app| {
                match region.as_str() {
                    "media" => app.media_selector_expanded = expanded,
                    "candidate-filter" => app.candidate_filter_expanded = expanded,
                    "artwork-editing" => app.artwork_editing_expanded = expanded,
                    "show-artwork" => app.show_artwork = expanded,
                    _ => {}
                }
                (
                    app.media_selector_expanded,
                    app.show_artwork,
                    app.candidate_filter_expanded,
                )
            });
            let Some((media_expanded, artwork_visible, candidate_filter_expanded)) = flags else {
                return;
            };
            if let Some(ui) = weak.upgrade() {
                let (window_width, window_height) = app_logical_window_size(&ui);
                let mut layout = workspace_layout.borrow_mut();
                layout.clamp_for_window(
                    window_width,
                    window_height,
                    media_expanded,
                    artwork_visible,
                    candidate_filter_expanded,
                );
                apply_workspace_layout(&ui, &layout);
                if let Some(log) = ui_debug_log.as_ref() {
                    log.record(format!(
                        "main-region-expanded region={} expanded={expanded} window={window_width}x{window_height} maximized={} media={} artwork={} filter={} activity={}",
                        region.as_str(),
                        ui.window().is_maximized(),
                        layout.media_pane_width,
                        layout.artwork_pane_width,
                        layout.candidate_filter_height,
                        layout.activity_pane_height
                    ));
                }
                if keep_maximized && !ui.window().is_maximized() {
                    ui.window().set_maximized(true);
                }
            }
            refresh_weak(&weak, &state);
            if keep_maximized {
                schedule_main_maximize_repairs(weak.clone(), maximize_guard.clone());
            }
        });
    }
    {
        let ui_debug_log = ui_debug_log.clone();
        ui.on_media_filter_changed(move |expanded| {
            if let Some(log) = ui_debug_log.as_ref() {
                log.record(format!("main-media-filter-expanded expanded={expanded}"));
            }
        });
    }
    {
        let ui_debug_log = ui_debug_log.clone();
        ui.on_transient_region_toggled(move || {
            if let Some(log) = ui_debug_log.as_ref() {
                log.record("main-transient-region-toggled");
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let controller = Arc::clone(&controller);
        ui.on_musicbrainz_leave_unchanged(move || {
            let decision = state
                .lock()
                .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                .and_then(|app| app.musicbrainz_leave_unchanged_decision());
            let result = decision.and_then(|decision| controller.submit_decision(decision));
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let controller = Arc::clone(&controller);
        ui.on_compilation_track_activated(move |album_key, track_path, include_bypassed| {
            activate_and_run_compilation_track(
                &state,
                &weak,
                &controller,
                album_key.as_str(),
                track_path.as_str(),
                include_bypassed,
            );
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let controller = Arc::clone(&controller);
        ui.on_compilation_report_track_activated(move |album_key, track_path, include_bypassed| {
            if state
                .lock()
                .is_ok_and(|app| app.compilation_report_preview_mode)
            {
                preview_compilation_report_track(
                    &weak,
                    &state,
                    album_key.as_str(),
                    track_path.as_str(),
                );
                refresh_compilation_report_preview(&weak, &state);
            } else {
                if let Ok(mut app) = state.lock() {
                    app.compilation_report_preview_track_path = track_path.to_string();
                }
                activate_and_run_compilation_track(
                    &state,
                    &weak,
                    &controller,
                    album_key.as_str(),
                    track_path.as_str(),
                    include_bypassed,
                );
                refresh_weak(&weak, &state);
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_set_compilation_report_preview(move |enabled| {
            if let Ok(mut app) = state.lock() {
                let report_visible = (app.library.show_tracks
                    && app.library.focused_album_key.is_some())
                    || (!app.compilation_report_artist.is_empty()
                        && app.library.focused_artist.as_deref()
                            == Some(app.compilation_report_artist.as_str()));
                if report_visible && app.phase == RunPhase::Idle && !app.operation_busy {
                    app.compilation_report_preview_mode = enabled;
                    app.compilation_report_auto_close_armed = false;
                    app.operation_status = if enabled {
                        "COMP preview on · select a report track to view its saved thumb"
                    } else {
                        "COMP preview off · selected report track remains highlighted"
                    }
                    .to_string();
                    app.operation_tone = "cyan".to_string();
                }
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let controller = Arc::clone(&controller);
        ui.on_compilation_album_launch(move |album_key, include_bypassed| {
            let result = if let Ok(mut app) = state.lock() {
                match app
                    .library
                    .select_pending_compilation_tracks(album_key.as_str(), include_bypassed)
                {
                    Ok(0) => {
                        app.operation_status =
                            "This artist's COMP tracks are already complete.".to_string();
                        app.operation_tone = "green".to_string();
                        false
                    }
                    Ok(count) => {
                        app.artist_artwork_mode = false;
                        app.scan_scope = ScanScope::Selected;
                        app.operation_status =
                            format!("COMP queued · {count} pending artist tracks");
                        app.operation_tone = "cyan".to_string();
                        true
                    }
                    Err(reason) => {
                        app.push_log(format!("Unable to launch COMP tracks: {reason:?}"));
                        false
                    }
                }
            } else {
                false
            };
            if result
                && !controller.is_running()
                && let Err(error) = controller.start()
            {
                record_action_result(&state, Err(error));
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_candidate_source_filter(move |source| {
            if let Ok(mut app) = state.lock() {
                app.set_candidate_source_filter(source.as_str());
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_candidate_range_filter(move |range| {
            if let Ok(mut app) = state.lock() {
                app.set_candidate_range_filter(range.as_str());
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_source_action(move |source, action| {
            let result = (|| {
                let mut app = state
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
                if app.phase != RunPhase::Idle {
                    return Err("Source settings cannot change during an active run.".to_string());
                }
                match action.as_str() {
                    "include" => app.set_source_enabled(source.as_str(), true)?,
                    "exclude" => app.set_source_enabled(source.as_str(), false)?,
                    "up" => app.move_source(source.as_str(), -1)?,
                    "down" => app.move_source(source.as_str(), 1)?,
                    _ => return Err("Unsupported source action.".to_string()),
                }
                app.config = splined_core::settings::save_config(&app.config)?;
                app.push_log(format!("Saved Config v5 source change for {source}."));
                Ok(())
            })();
            if result.is_ok()
                && let (Some(ui), Ok(app)) = (weak.upgrade(), state.lock())
            {
                ui.set_settings_source_order(app.config.sources.cover_sources.join(", ").into());
            }
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    }
    ui.on_make_source_drag_data(|source| {
        slint::DataTransfer::from(SharedString::from(format!("splined-source:{}", source)))
    });
    {
        let weak = ui.as_weak();
        ui.on_can_drop_source(move |data, target| {
            source_name_from_drag(&data).is_some_and(|source| {
                !source.eq_ignore_ascii_case(target.as_str())
                    && weak.upgrade().is_some_and(|ui| {
                        let order = split_values(ui.get_settings_source_order().as_str());
                        order.iter().any(|item| item.eq_ignore_ascii_case(&source))
                            && order
                                .iter()
                                .any(|item| item.eq_ignore_ascii_case(target.as_str()))
                    })
            })
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_drop_source(move |data, target| {
            let result = (|| {
                let ui = weak
                    .upgrade()
                    .ok_or_else(|| "The SPLINED Settings window is unavailable.".to_string())?;
                let source = source_name_from_drag(&data)
                    .ok_or_else(|| "This drag is not an artwork source.".to_string())?;
                let mut order = split_values(ui.get_settings_source_order().as_str());
                let source_index = order
                    .iter()
                    .position(|item| item.eq_ignore_ascii_case(&source))
                    .ok_or_else(|| format!("Unknown artwork source: {source}"))?;
                let target_index = order
                    .iter()
                    .position(|item| item.eq_ignore_ascii_case(target.as_str()))
                    .ok_or_else(|| format!("Unknown artwork source: {target}"))?;
                let moved = order.remove(source_index);
                order.insert(target_index, moved);
                set_settings_source_order_and_cards(&ui, &order);
                Ok(())
            })();
            record_action_result(&state, result);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_settings_source_step(move |source, direction| {
            let result = (|| {
                let ui = weak
                    .upgrade()
                    .ok_or_else(|| "The SPLINED Settings window is unavailable.".to_string())?;
                let mut order = split_values(ui.get_settings_source_order().as_str());
                let source_index = order
                    .iter()
                    .position(|item| item.eq_ignore_ascii_case(source.as_str()))
                    .ok_or_else(|| format!("Unknown artwork source: {source}"))?;
                let target_index = match direction.as_str() {
                    "up" => source_index.saturating_sub(1),
                    "down" => (source_index + 1).min(order.len().saturating_sub(1)),
                    _ => return Err("Unsupported source direction.".to_string()),
                };
                order.swap(source_index, target_index);
                set_settings_source_order_and_cards(&ui, &order);
                Ok(())
            })();
            record_action_result(&state, result);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_search_edited(move |value| {
            if let Ok(mut app) = state.lock() {
                app.library.set_text_filter(value.as_str());
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let revision = Arc::clone(&adjustment_preview_revision);
        let gate = Arc::clone(&adjustment_preview_gate);
        let action_log = ui_debug_log.clone();
        ui.on_set_upscale_preview(move |preview| {
            if !preview {
                return;
            }
            if let Ok(mut app) = state.lock() {
                if app.operation_busy {
                    if let Some(log) = action_log.as_ref() {
                        log.record(format!(
                            "upscale-preview-request accepted=false phase={:?} reason=operation-busy",
                            app.phase
                        ));
                    }
                    return;
                }
                if let Some(log) = action_log.as_ref() {
                    let candidate = app
                        .selected_candidate()
                        .map(|candidate| format!("{}:{}", candidate.source, candidate.index))
                        .or_else(|| {
                            (app.phase == RunPhase::AwaitingMusicBrainz
                                && !app.musicbrainz_cover_preview_path.trim().is_empty())
                            .then(|| {
                                format!(
                                    "musicbrainz:{}",
                                    app.selected_musicbrainz_result.unwrap_or_default()
                                )
                            })
                        })
                        .or_else(|| {
                            app.active_compilation_track_preview()
                                .map(|_| "compilation-track".to_string())
                        })
                        .unwrap_or_else(|| "current-cover".to_string());
                    log.record(format!(
                        "upscale-preview-request accepted=true phase={:?} candidate={candidate}",
                        app.phase
                    ));
                }
                app.upscale_preview_only = true;
                app.adjustments.preserve_source_resolution = false;
                app.adjustments.apply_edit_profile = true;
                app.adjustments.edit_existing_cover = false;
                clear_rendered_preview_for_adjustment(&mut app);
                app.operation_busy = true;
                app.operation_status = "Rendering Upscale Preview in memory…".to_string();
                app.operation_tone = "violet".to_string();
                app.push_log(
                    "Rendering Upscale Preview without changing album files.".to_string(),
                );
            }
            revision.fetch_add(1, Ordering::SeqCst);
            refresh_weak(&weak, &state);
            schedule_adjustment_preview(
                Arc::clone(&state),
                weak.clone(),
                Arc::clone(&revision),
                Arc::clone(&gate),
            );
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_prepare_embedded_thumbnail(move || {
            if let Ok(mut app) = state.lock() {
                if app.operation_busy {
                    return;
                }
                if disable_embedded_thumbnail_preview(&mut app) {
                    app.operation_status =
                        "EMBED Thumbs off · original preview restored · no files changed"
                            .to_string();
                    app.operation_tone = "violet".to_string();
                    drop(app);
                    refresh_weak(&weak, &state);
                    return;
                }
                app.operation_busy = true;
                app.operation_status = "Preparing embedded thumbnail in memory…".to_string();
                app.operation_tone = "violet".to_string();
            }
            refresh_weak(&weak, &state);
            let state = Arc::clone(&state);
            let weak = weak.clone();
            std::thread::spawn(move || {
                let result = prepare_embedded_thumbnail_preview(&state);
                let succeeded = result.is_ok();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Ok(mut app) = state.lock() {
                        app.operation_busy = false;
                        if succeeded {
                            app.operation_status = format!(
                                "Embedded thumbnail ready · {} × {} · no files changed",
                                app.config.output.embedded_thumbnail_size,
                                app.config.output.embedded_thumbnail_size
                            );
                            app.operation_tone = "violet".to_string();
                        }
                    }
                    record_action_result(&state, result);
                    refresh_weak(&weak, &state);
                });
            });
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_save_adjustments(move || {
            let result = (|| {
                let mut app = state
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
                app.persist_adjustments_to_config();
                app.config = splined_core::settings::save_config(&app.config)?;
                app.push_log("Saved artwork adjustment defaults to Config v5.".to_string());
                Ok(())
            })();
            if result.is_ok()
                && let Some(ui) = weak.upgrade()
            {
                ui.set_settings_enhancement_changed(false);
            }
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let pending = Rc::new(RefCell::new(BTreeSet::<(String, String)>::new()));
        let debounce = Rc::new(Timer::default());
        let initial_state = Arc::clone(&state);
        let initial_weak = weak.clone();
        ui.on_library_thumbnail_needed(move |key, kind| {
            if key.trim().is_empty() {
                return;
            }
            pending
                .borrow_mut()
                .insert((key.to_string(), kind.to_string()));
            let state = Arc::clone(&state);
            let weak = weak.clone();
            let pending = Rc::clone(&pending);
            debounce.start(
                TimerMode::SingleShot,
                Duration::from_millis(65),
                move || {
                    let requests = std::mem::take(&mut *pending.borrow_mut())
                        .into_iter()
                        .collect::<Vec<_>>();
                    refresh_visible_navidrome_thumbnails(&state, weak.clone(), requests);
                },
            );
        });
        // Re-publish the model after the callback is connected so the first
        // virtualized ListView delegates request their initial thumbnails.
        refresh_weak(&initial_weak, &initial_state);
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_status_filter_changed(move |value| {
            let filter = match value.as_str() {
                "Unprocessed" => StatusFilter::Unprocessed,
                "Incomplete" => StatusFilter::Incomplete,
                "Processed" => StatusFilter::Processed,
                "Bypassed" => StatusFilter::Bypassed,
                "Partial / Timeout" | "Partial / Error" => StatusFilter::PartialTimeout,
                "Artist Complete" => StatusFilter::ArtistComplete,
                "Artist Contains Bypass" | "Artist Bypass" => StatusFilter::ArtistContainsBypass,
                _ => StatusFilter::All,
            };
            if let Ok(mut app) = state.lock() {
                app.library.set_status_filter(filter);
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let action_log = ui_debug_log.clone();
        let controller = Arc::clone(&controller);
        ui.on_library_row_activated(move |key, include_bypassed| {
            if state
                .lock()
                .is_ok_and(|app| album_key_embed_locked(&app, key.as_str()))
            {
                return;
            }
            let started = Instant::now();
            let prior_artist = state
                .lock()
                .ok()
                .and_then(|app| overview_artist_name(&app).map(str::to_owned));
            let hydration = activate_library_row(&state, key.as_str(), include_bypassed);
            let artist = state.lock().ok().and_then(|app| {
                let activated_album = app
                    .library
                    .albums
                    .iter()
                    .any(|album| album.snapshot.album_key.eq_ignore_ascii_case(key.as_str()));
                if activated_album {
                    (app.library.view == LibraryView::Albums
                        && !app.library.show_tracks
                        && app
                            .library
                            .focused_album_key
                            .as_deref()
                            .is_some_and(|focused| focused.eq_ignore_ascii_case(key.as_str())))
                    .then(|| overview_artist_name(&app).map(str::to_owned))
                    .flatten()
                } else {
                    Some(app.library.focused_artist.clone().unwrap_or_else(|| {
                        if app.current_artist.trim().is_empty() {
                            key.to_string()
                        } else {
                            app.current_artist.clone()
                        }
                    }))
                }
            });
            if let Some(artist) = artist.as_deref()
                && let Ok(mut app) = state.lock()
                && !prior_artist.as_deref().is_some_and(|prior| prior.eq_ignore_ascii_case(artist))
            {
                clear_staged_candidate_review(&mut app);
                app.musicbrainz_cover_preview_path.clear();
                app.hovered_musicbrainz_result = None;
                app.selected_musicbrainz_result = None;
                app.operation_busy = false;
                app.artist_artwork_draft_path.clear();
                load_cached_artist_overview(&mut app, artist);
                if app.library.view == LibraryView::Artists {
                    app.select_artist_appearances_for_run();
                }
                app.artist_overview_show_all = false;
                app.artist_artwork_mode = false;
            }
            if prior_artist.as_deref().is_some_and(|prior| {
                artist.as_deref().is_some_and(|current| !prior.eq_ignore_ascii_case(current))
            }) && controller.is_running() {
                controller.stop();
            }
            if let Some(artist) = artist {
                refresh_navidrome_thumbnails(&state, weak.clone(), Some(artist));
            }
            refresh_weak(&weak, &state);
            if let Some(log) = action_log.as_ref() {
                log.record(format!(
                    "library-row-click key={} elapsed_ms={}",
                    key,
                    started.elapsed().as_millis()
                ));
            }
            if let Some(request) = hydration {
                hydrate_fallback_album(&state, weak.clone(), request);
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_artist_overview_similar_selected(move |artist| {
            let artist = artist.trim().to_string();
            let switched = if let Ok(mut app) = state.lock() {
                let result = app.library.select_artist_exclusive(&artist, false);
                if result == SelectionResult::NotFound {
                    false
                } else {
                    app.artist_artwork_draft_path.clear();
                    app.artist_artwork_mode = false;
                    app.artist_overview_show_all = false;
                    app.artist_overview_top_songs_loading = false;
                    app.synchronize_focused_artist();
                    let cached = app
                        .artist_overview_cache
                        .get(&artist.to_ascii_lowercase())
                        .cloned();
                    if let Some(cached) = cached {
                        app.artist_overview_biography = cached.biography;
                        app.artist_overview_external_url = cached.external_url;
                        app.artist_overview_musicbrainz_id = cached.musicbrainz_id;
                        app.artist_overview_banner_path = cached.banner_path;
                        app.artist_overview_similar_artists = cached.similar_artists;
                        app.artist_overview_genres = cached.genres;
                        app.artist_overview_roles = cached.roles;
                        app.artist_overview_album_keys = cached.album_keys;
                        app.select_artist_appearances_for_run();
                        app.artist_overview_featured_album_keys = cached.featured_album_keys;
                        app.artist_overview_track_paths = cached.track_paths_by_album;
                        app.artist_overview_album_ratings = cached.album_ratings;
                        app.artist_overview_artwork_candidates = cached.artwork_candidates;
                        app.artist_overview_top_songs = cached.top_songs;
                    } else {
                        app.artist_overview_biography.clear();
                        app.artist_overview_external_url.clear();
                        app.artist_overview_musicbrainz_id.clear();
                        app.artist_overview_banner_path.clear();
                        app.artist_overview_similar_artists.clear();
                        app.artist_overview_genres.clear();
                        app.artist_overview_roles.clear();
                        app.artist_overview_album_keys.clear();
                        app.artist_overview_featured_album_keys.clear();
                        app.artist_overview_track_paths.clear();
                        app.artist_overview_album_ratings.clear();
                        app.artist_overview_artwork_candidates.clear();
                        app.artist_overview_top_songs.clear();
                    }
                    true
                }
            } else {
                false
            };
            if switched {
                refresh_navidrome_thumbnails(&state, weak.clone(), Some(artist));
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_artist_overview_navigate(move |direction| {
            let artist = state
                .lock()
                .ok()
                .and_then(|mut app| select_adjacent_artist_for_overview(&mut app, direction));
            if let Some(artist) = artist {
                refresh_navidrome_thumbnails(&state, weak.clone(), Some(artist));
            } else if let Ok(mut app) = state.lock() {
                app.push_log(
                    "No other unprocessed Artist matches the current filters.".to_string(),
                );
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_artist_overview_album_selected(move |key| {
            if state
                .lock()
                .is_ok_and(|app| album_key_embed_locked(&app, key.as_str()))
            {
                return;
            }
            let hydration = activate_library_row(&state, key.as_str(), false).or_else(|| {
                state
                    .lock()
                    .ok()
                    .and_then(|app| app.library.fallback_track_hydration_request(key.as_str()))
            });
            refresh_weak(&weak, &state);
            if let Some((album_key, album_path, cached)) = hydration {
                let state = Arc::clone(&state);
                let weak = weak.clone();
                std::thread::spawn(move || {
                    let result = fallback_tracks_from_folder(Path::new(&album_path), &cached);
                    let _ = slint::invoke_from_event_loop(move || {
                        match result {
                            Ok(tracks) => {
                                if let Ok(mut app) = state.lock() {
                                    replace_fallback_tracks_and_invalidate(
                                        &mut app, &album_key, tracks,
                                    );
                                    let visible = app.library.compilation_track_rows();
                                    let completed_single = visible.len() == 1
                                        && visible[0].embedded_artwork_recorded;
                                    let single_needs_repair = completed_single
                                        && compilation_track_ui_rows(&app).first().is_some_and(
                                            |track| {
                                                track.status_color
                                                    == Color::from_rgb_u8(124, 77, 255)
                                            },
                                        );
                                    app.operation_status = if visible.is_empty() {
                                        "COMP opened · no matching tracks found for this Artist."
                                            .to_string()
                                    } else if single_needs_repair {
                                        "COMP ready · completed track needs thumbnail repair; choose Repair purple COMP."
                                            .to_string()
                                    } else if completed_single {
                                        "COMP ready · completed Artist track; choose the track to review its artwork."
                                            .to_string()
                                    } else {
                                        format!(
                                            "COMP ready · {} matching Artist track{} directly below the album card",
                                            visible.len(),
                                            if visible.len() == 1 { "" } else { "s" }
                                        )
                                    };
                                    app.operation_tone = if visible.is_empty() {
                                        "orange".to_string()
                                    } else if single_needs_repair {
                                        "violet".to_string()
                                    } else if completed_single {
                                        "green".to_string()
                                    } else {
                                        "cyan".to_string()
                                    };
                                }
                            }
                            Err(error) => record_action_result(&state, Err(error)),
                        }
                        refresh_weak(&weak, &state);
                    });
                });
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_browse_artist_artwork(move || {
            let prepared = state.lock().ok().and_then(|mut app| {
                if app.operation_busy {
                    return None;
                }
                let artist = overview_artist_name(&app)
                    .map(str::to_owned)
                    .or_else(|| app.library.focused_artist.clone())
                    .filter(|value| !value.trim().is_empty())?;
                let artist_mbid = if !app.artist_overview_musicbrainz_id.trim().is_empty() {
                    app.artist_overview_musicbrainz_id.clone()
                } else {
                    app.library
                        .albums
                        .iter()
                        .find(|album| {
                            album.snapshot.artist.trim().eq_ignore_ascii_case(artist.trim())
                                && !album.snapshot.artist_mbid.trim().is_empty()
                        })
                        .map(|album| album.snapshot.artist_mbid.clone())
                        .unwrap_or_default()
                };
                let enabled_sources = app
                    .sources
                    .iter()
                    .filter(|source| source.enabled)
                    .map(|source| source.name.clone())
                    .collect::<Vec<_>>();
                let queried_sources = artist_art_query_sources(&enabled_sources, &artist_mbid);
                let existing = app.artist_overview_artwork_candidates.clone();
                let artist_album_paths = app
                    .library
                    .albums
                    .iter()
                    .map(|album| {
                        (
                            album.snapshot.artist.clone(),
                            album.snapshot.path.clone(),
                            album.snapshot.album_mbid.clone(),
                            album.snapshot.artist_mbid.clone(),
                        )
                    })
                    .collect::<Vec<_>>();
                // The Artist Art workspace must be visible while remote
                // candidates load. Otherwise the tab opens the album editor
                // for several seconds and can retain its album preview.
                app.artist_artwork_mode = true;
                app.candidates.clear();
                app.selected_candidate = None;
                app.comparison_candidates.clear();
                app.edit_preview_path.clear();
                app.artist_artwork_draft_path.clear();
                app.artist_portrait_preview_candidate_index = None;
                app.artist_portrait_preview_path.clear();
                app.artist_banner_source_path.clear();
                app.artist_banner_preview_candidate_index = None;
                app.artist_banner_preview_path.clear();
                app.artist_theme_portrait_preview_path.clear();
                app.artist_theme_banner_preview_path.clear();
                app.artist_current_radio_active = false;
                app.artist_portrait_radio_active = false;
                app.artist_banner_radio_active = false;
                app.artist_package_overview_enabled = false;
                app.artist_theme_art_enabled = false;
                for source in &mut app.sources {
                    if source.enabled
                        && queried_sources
                            .iter()
                            .any(|queried| queried.eq_ignore_ascii_case(&source.name))
                    {
                        source.candidate_count = 0;
                        source.status = "Searching…".to_string();
                    } else if source.enabled && !is_artist_art_source(&source.name) {
                        source.status = "Album-only".to_string();
                    } else if source.enabled
                        && source.name.eq_ignore_ascii_case("fanarttv")
                        && artist_mbid.trim().is_empty()
                    {
                        source.status = "Needs Artist ID".to_string();
                    }
                }
                app.operation_busy = true;
                app.operation_status = format!(
                    "Searching Artist Art for {artist}: {}…",
                    if queried_sources.is_empty() {
                        "Navidrome and local files".to_string()
                    } else {
                        queried_sources
                            .iter()
                            .map(|source| source_display_name(source))
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                );
                app.operation_tone = "cyan".to_string();
                app.last_error = None;
                Some((
                    app.config.clone(),
                    artist,
                    artist_mbid,
                    enabled_sources,
                    queried_sources,
                    existing,
                    artist_album_paths,
                    app.library.selection_epoch,
                ))
            });
            let Some((config, artist, artist_mbid, enabled_sources, queried_sources, existing, artist_album_paths, selection_epoch)) = prepared else {
                record_action_result(
                    &state,
                    Err("Select an artist before finding Artist Art, or wait for the current operation to finish.".to_string()),
                );
                refresh_weak(&weak, &state);
                return;
            };
            if let Some(log) = UI_DEBUG_LOG_HANDLE.get() {
                log.record(format!(
                    "artist-art-search-start artist={} mbid={} sources={}",
                    artist,
                    artist_mbid,
                    queried_sources.join(",")
                ));
            }
            refresh_weak(&weak, &state);
            let state = Arc::clone(&state);
            let weak = weak.clone();
            std::thread::spawn(move || {
                let started = Instant::now();
                let local = local_artist_art_candidates(
                    artist_album_paths
                        .iter()
                        .map(|(album_artist, path, album_mbid, artist_mbid)| {
                            (
                                album_artist.as_str(),
                                path.as_str(),
                                album_mbid.as_str(),
                                artist_mbid.as_str(),
                            )
                        }),
                    &artist,
                    &configured_shared_artist_art_dir(&config),
                );
                let remote = discover_remote_artist_artwork(
                    &config,
                    &artist,
                    &artist_mbid,
                    &enabled_sources,
                );
                if let Some(log) = UI_DEBUG_LOG_HANDLE.get() {
                    log.record(format!(
                        "artist-art-search-finished artist={} elapsed_ms={} queried={} failed={} downloaded={} runtime_ok={}",
                        artist,
                        started.elapsed().as_millis(),
                        queried_sources.join(","),
                        remote
                            .as_ref()
                            .map_or_else(|_| queried_sources.join(","), |result| result.failed_sources.join(",")),
                        remote.as_ref().map_or(0, |result| result.candidates.len()),
                        remote.is_ok(),
                    ));
                }
                let _ = slint::invoke_from_event_loop(move || {
                    let result = state
                        .lock()
                        .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                        .and_then(|mut app| {
                            if app.library.selection_epoch != selection_epoch
                                || !overview_artist_name(&app).is_some_and(|focused| {
                                focused.trim().eq_ignore_ascii_case(artist.trim())
                            }) {
                                return Ok(());
                            }
                            app.operation_busy = false;
                            let (remote_candidates, failed_sources) = match remote {
                                Ok(result) => {
                                    for warning in result.warnings {
                                        app.push_log(format!("Artist Art source warning: {warning}"));
                                    }
                                    (result.candidates, result.failed_sources)
                                }
                                Err(error) => {
                                    app.push_log(format!(
                                        "Remote Artist Art search warning: {error}"
                                    ));
                                    (Vec::new(), queried_sources.clone())
                                }
                            };
                            let artwork_candidates =
                                merge_artist_art_candidates(local, existing, remote_candidates);
                            app.artist_overview_artwork_candidates = artwork_candidates.clone();
                            if let Some(cache) = app
                                .artist_overview_cache
                                .get_mut(&artist.trim().to_ascii_lowercase())
                            {
                                cache.artwork_candidates = artwork_candidates;
                            }
                            load_artist_art_candidates(
                                &mut app,
                                &artist,
                                &queried_sources,
                                &failed_sources,
                                &artist_mbid,
                            )
                        });
                    record_action_result(&state, result);
                    refresh_weak(&weak, &state);
                });
            });
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_set_artist_overview_show_all(move |enabled| {
            if let Ok(mut app) = state.lock() {
                app.artist_overview_show_all = enabled;
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_load_artist_top_songs(move || {
            let prepared = state.lock().ok().and_then(|mut app| {
                let artist = overview_artist_name(&app)?.to_string();
                let artist_key = artist.trim().to_ascii_lowercase();
                if let Some(cached) = app.artist_top_songs_cache.get(&artist_key).cloned() {
                    app.artist_overview_top_songs_loading = cached.is_none();
                    if let Some(songs) = cached {
                        app.artist_overview_top_songs = songs;
                    }
                    return None;
                }
                app.artist_top_songs_cache.insert(artist_key.clone(), None);
                app.artist_overview_top_songs_loading = true;
                Some((app.config.clone(), artist, artist_key))
            });
            let Some((config, artist, artist_key)) = prepared else {
                return;
            };
            refresh_weak(&weak, &state);
            let state = Arc::clone(&state);
            let weak = weak.clone();
            std::thread::spawn(move || {
                let result = (|| -> Result<Vec<String>, String> {
                    let credential_path =
                        splined_core::credential_store::credential_path(&config, "Navidrome")?;
                    let credential = NavidromeCredential::load(&credential_path)?;
                    let companion = NavidromeCompanion::new(credential)?;
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|error| {
                            format!("Unable to start the top-songs runtime: {error}")
                        })?;
                    runtime.block_on(companion.artist_top_songs(&artist, 12))
                })();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Ok(mut app) = state.lock() {
                        let focused = overview_artist_name(&app)
                            .is_some_and(|name| name.trim().eq_ignore_ascii_case(&artist));
                        match &result {
                            Ok(songs) => {
                                app.artist_top_songs_cache
                                    .insert(artist_key.clone(), Some(songs.clone()));
                                if let Some(cache) = app.artist_overview_cache.get_mut(&artist_key)
                                {
                                    cache.top_songs.clone_from(songs);
                                }
                                if focused {
                                    app.artist_overview_top_songs.clone_from(songs);
                                    app.artist_overview_top_songs_loading = false;
                                }
                            }
                            Err(error) => {
                                app.artist_top_songs_cache.remove(&artist_key);
                                if focused {
                                    app.artist_overview_top_songs_loading = false;
                                }
                                if let Some(log) = UI_DEBUG_LOG_HANDLE.get() {
                                    log.record(format!(
                                        "artist-top-songs artist={artist} error={error}"
                                    ));
                                }
                            }
                        }
                    }
                    refresh_weak(&weak, &state);
                });
            });
        });
    }
    {
        let state = Arc::clone(&state);
        ui.on_open_artist_external(move || {
            let result = state
                .lock()
                .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                .and_then(|app| {
                    let url = app.artist_overview_external_url.trim();
                    if !matches!(url.split_once("://"), Some(("http" | "https", _))) {
                        return Err("Navidrome did not provide a valid artist information link."
                            .to_string());
                    }
                    open_external_url(url)
                });
            record_action_result(&state, result);
        });
    }
    {
        let state = Arc::clone(&state);
        ui.on_open_artist_musicbrainz(move || {
            let result = state
                .lock()
                .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                .and_then(|app| {
                    let mbid = app.artist_overview_musicbrainz_id.trim();
                    if mbid.is_empty() {
                        return Err(
                            "This artist does not have a MusicBrainz artist ID.".to_string()
                        );
                    }
                    open_external_url(&format!("https://musicbrainz.org/artist/{mbid}"))
                });
            record_action_result(&state, result);
        });
    }
    {
        let pending = Arc::new(Mutex::new(None::<CompilationThumbRepairPlan>));
        let launch_after_repair = Arc::new(Mutex::new(None::<bool>));
        let state_for_prepare = Arc::clone(&state);
        let weak_for_prepare = ui.as_weak();
        let controller_for_prepare = Arc::clone(&controller);
        let pending_for_prepare = Arc::clone(&pending);
        ui.on_prepare_compilation_repair(move |album_key| {
            let prepared = (|| {
                let mut app = state_for_prepare
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
                if app.operation_busy || app.phase != RunPhase::Idle || controller_for_prepare.is_running() {
                    return Err("Finish or SKIP the active review before repairing COMP thumbs.".to_string());
                }
                let plan = compilation_thumb_repair_plan(&app, album_key.as_str())?;
                close_artist_compilation_report(&mut app);
                app.operation_busy = true;
                app.operation_status = "Checking completed COMP track thumbnails…".into();
                app.operation_tone = "violet".into();
                Ok::<_, String>(plan)
            })();
            let plan = match prepared {
                Ok(plan) => plan,
                Err(error) => {
                    record_action_result(&state_for_prepare, Err(error));
                    refresh_weak(&weak_for_prepare, &state_for_prepare);
                    return;
                }
            };
            if let Ok(mut slot) = pending_for_prepare.lock() {
                *slot = None;
            }
            if let Some(ui) = weak_for_prepare.upgrade() {
                ui.set_confirm_compilation_repair(false);
            }
            refresh_weak(&weak_for_prepare, &state_for_prepare);
            let state = Arc::clone(&state_for_prepare);
            let weak = weak_for_prepare.clone();
            let pending = Arc::clone(&pending_for_prepare);
            std::thread::spawn(move || {
                let result = inspect_compilation_thumb_repair(plan);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Ok(mut app) = state.lock() {
                        app.operation_busy = false;
                    }
                    match result {
                        Ok(plan) => {
                            let summary = format!(
                                "Resize the existing embedded art in {} completed {} COMP track{} to {} × {}? No MusicBrainz search or album cover change. {} missing/unreadable track{} skipped.",
                                plan.targets.len(), plan.artist,
                                if plan.targets.len() == 1 { "" } else { "s" },
                                plan.side, plan.side, plan.missing,
                                if plan.missing == 1 { "" } else { "s" }
                            );
                            if let Ok(mut slot) = pending.lock() {
                                *slot = Some(plan);
                            }
                            if let Some(ui) = weak.upgrade() {
                                ui.set_compilation_repair_summary(summary.into());
                                ui.set_confirm_compilation_repair(true);
                            }
                        }
                        Err(error) => record_action_result(&state, Err(error)),
                    }
                    refresh_weak(&weak, &state);
                });
            });
        });

        let state_for_report = Arc::clone(&state);
        let weak_for_report = ui.as_weak();
        let controller_for_report = Arc::clone(&controller);
        let pending_for_report = Arc::clone(&pending);
        let launch_for_report = Arc::clone(&launch_after_repair);
        ui.on_prepare_artist_compilation_repair(move || {
            let prepared = (|| {
                let mut app = state_for_report
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
                if app.operation_busy
                    || app.phase != RunPhase::Idle
                    || controller_for_report.is_running()
                {
                    return Err("Finish or SKIP the active review before compiling the Artist COMP report.".into());
                }
                let (plan, report) = artist_compilation_thumb_repair_plan(&app)?;
                app.operation_busy = true;
                app.operation_status = format!(
                    "Compiling {} COMP report · checking {} track{} across {} album{}…",
                    plan.artist,
                    report.len(),
                    if report.len() == 1 { "" } else { "s" },
                    plan.album_paths.len(),
                    if plan.album_paths.len() == 1 { "" } else { "s" }
                );
                app.operation_tone = "violet".into();
                Ok::<_, String>((plan, report))
            })();
            let (mut plan, report) = match prepared {
                Ok(value) => value,
                Err(error) => {
                    record_action_result(&state_for_report, Err(error));
                    refresh_weak(&weak_for_report, &state_for_report);
                    return;
                }
            };
            if let Ok(mut slot) = pending_for_report.lock() {
                *slot = None;
            }
            if let Ok(mut slot) = launch_for_report.lock() {
                *slot = None;
            }
            if let Some(ui) = weak_for_report.upgrade() {
                ui.set_confirm_compilation_repair(false);
            }
            refresh_weak(&weak_for_report, &state_for_report);
            let state = Arc::clone(&state_for_report);
            let weak = weak_for_report.clone();
            let pending = Arc::clone(&pending_for_report);
            std::thread::spawn(move || {
                let mut repairable = Vec::new();
                let mut missing = 0usize;
                let mut blue = 0usize;
                for track in &report {
                    if !track.completed {
                        blue += 1;
                        continue;
                    }
                    match cached_embedded_front_metadata(&track.path) {
                        Some((width, height, _)) if width != plan.side || height != plan.side => {
                            repairable.push(track.path.clone());
                        }
                        None => missing += 1,
                        _ => {}
                    }
                }
                let purple = repairable.len();
                plan.targets = repairable;
                plan.missing = missing;
                let green = report.len().saturating_sub(blue + purple + missing);
                let _ = slint::invoke_from_event_loop(move || {
                    let still_selected = if let Ok(mut app) = state.lock() {
                        app.operation_busy = false;
                        let same = app.library.selection_epoch == plan.selection_epoch
                            && app.library.focused_artist.as_deref()
                                == Some(plan.artist.as_str());
                        if same {
                            let albums = artist_overview_ui_model(&app);
                            let unfinished_album = (0..albums.row_count()).any(|index| {
                                albums.row_data(index).is_some_and(|album| !album.fully_complete)
                            });
                            app.compilation_report_collapsed_albums.clear();
                            app.compilation_report_preview_mode = false;
                            app.compilation_report_preview_album_key.clear();
                            app.compilation_report_preview_track_path.clear();
                            app.compilation_report_auto_close_armed = unfinished_album
                                || purple > 0 || blue > 0 || missing > 0;
                            app.compilation_report_close_pending = false;
                            app.compilation_report_generation =
                                app.compilation_report_generation.wrapping_add(1);
                            app.compilation_report_artist = plan.artist.clone();
                            app.compilation_report_summary = format!(
                                "{} album{} · {} tracks: {green} green, {purple} repairable, {blue} need artwork, {missing} unreadable",
                                plan.album_paths.len(),
                                if plan.album_paths.len() == 1 { "" } else { "s" },
                                report.len(),
                            );
                            app.operation_status = format!(
                                "COMP report ready · {green} green · {purple} thumb repair{} · {blue} need artwork · {missing} unreadable",
                                if purple == 1 { "" } else { "s" }
                            );
                            app.operation_tone = if purple > 0 || missing > 0 {
                                "violet"
                            } else if blue > 0 {
                                "orange"
                            } else {
                                "green"
                            }
                            .into();
                        }
                        same
                    } else {
                        false
                    };
                    if still_selected && purple > 0 {
                        let summary = format!(
                            "Resize {purple} completed {} COMP thumb{} to {} × {}? {} unreadable skipped; cover.* untouched.",
                            plan.artist,
                            if purple == 1 { "" } else { "s" },
                            plan.side,
                            plan.side,
                            missing
                        );
                        if let Ok(mut slot) = pending.lock() {
                            *slot = Some(plan);
                        }
                        if let Some(ui) = weak.upgrade() {
                            ui.set_compilation_repair_summary(summary.into());
                        }
                    }
                    refresh_weak(&weak, &state);
                });
            });
        });

        let state_for_toggle = Arc::clone(&state);
        let weak_for_toggle = ui.as_weak();
        ui.on_compilation_report_album_toggled(move |album_key| {
            if let Ok(mut app) = state_for_toggle.lock()
                && app.library.focused_artist.as_deref()
                    == Some(app.compilation_report_artist.as_str())
            {
                let key = album_key.to_ascii_lowercase();
                if app.compilation_report_collapsed_albums.contains(&key) {
                    app.compilation_report_collapsed_albums.remove(&key);
                } else {
                    app.compilation_report_collapsed_albums.insert(key);
                }
            }
            refresh_weak(&weak_for_toggle, &state_for_toggle);
        });

        let state_for_repair_request = Arc::clone(&state);
        let weak_for_repair_request = ui.as_weak();
        let pending_for_repair_request = Arc::clone(&pending);
        ui.on_request_artist_compilation_repair(move || {
            let ready = state_for_repair_request.lock().ok().is_some_and(|app| {
                app.library.focused_artist.as_deref()
                    == Some(app.compilation_report_artist.as_str())
                    && !app.compilation_report_artist.is_empty()
                    && app.phase == RunPhase::Idle
                    && !app.operation_busy
            });
            if !ready {
                record_action_result(
                    &state_for_repair_request,
                    Err("Open the current Artist's COMP report before repairing it.".into()),
                );
            } else if pending_for_repair_request
                .lock()
                .ok()
                .and_then(|slot| slot.as_ref().map(|plan| !plan.targets.is_empty()))
                .unwrap_or(false)
            {
                if let Some(ui) = weak_for_repair_request.upgrade() {
                    ui.set_confirm_compilation_repair(true);
                }
            } else if let Ok(mut app) = state_for_repair_request.lock() {
                app.operation_status = "No purple COMP thumbnails need repair.".into();
                app.operation_tone = "green".into();
            }
            refresh_weak(&weak_for_repair_request, &state_for_repair_request);
        });

        let state_for_report_launch = Arc::clone(&state);
        let weak_for_report_launch = ui.as_weak();
        let pending_for_report_launch = Arc::clone(&pending);
        let launch_for_report_launch = Arc::clone(&launch_after_repair);
        let controller_for_report_launch = Arc::clone(&controller);
        ui.on_launch_artist_compilation_report(move |include_bypassed| {
            let repair_pending = pending_for_report_launch
                .lock()
                .ok()
                .and_then(|slot| slot.as_ref().map(|plan| !plan.targets.is_empty()))
                .unwrap_or(false);
            if repair_pending {
                if let Ok(mut slot) = launch_for_report_launch.lock() {
                    *slot = Some(include_bypassed);
                }
                if let Some(ui) = weak_for_report_launch.upgrade() {
                    ui.set_confirm_compilation_repair(false);
                    ui.invoke_confirm_compilation_repair_action(true);
                }
            } else if let Err(error) = launch_artist_compilation_report_tracks(
                &state_for_report_launch,
                &controller_for_report_launch,
                include_bypassed,
            ) {
                record_action_result(&state_for_report_launch, Err(error));
            }
            refresh_weak(&weak_for_report_launch, &state_for_report_launch);
        });

        let state_for_close = Arc::clone(&state);
        let weak_for_close = ui.as_weak();
        let pending_for_close = Arc::clone(&pending);
        let launch_for_close = Arc::clone(&launch_after_repair);
        ui.on_close_compilation_report(move || {
            if let Ok(mut slot) = pending_for_close.lock() {
                *slot = None;
            }
            if let Ok(mut slot) = launch_for_close.lock() {
                *slot = None;
            }
            if let Ok(mut app) = state_for_close.lock() {
                close_artist_compilation_report(&mut app);
            }
            if let Some(ui) = weak_for_close.upgrade() {
                ui.set_confirm_compilation_repair(false);
            }
            refresh_weak(&weak_for_close, &state_for_close);
        });

        let state_for_confirm = Arc::clone(&state);
        let weak_for_confirm = ui.as_weak();
        let controller_for_confirm = Arc::clone(&controller);
        let launch_for_confirm = Arc::clone(&launch_after_repair);
        ui.on_confirm_compilation_repair_action(move |confirmed| {
            if !confirmed {
                if let Ok(mut slot) = launch_for_confirm.lock() {
                    *slot = None;
                }
                if let Ok(mut app) = state_for_confirm.lock() {
                    app.operation_status = "COMP thumb repair cancelled · no files changed".into();
                    app.operation_tone = "violet".into();
                }
                refresh_weak(&weak_for_confirm, &state_for_confirm);
                return;
            }
            let plan = pending.lock().ok().and_then(|mut slot| slot.take());
            let prepared = (|| {
                let plan = plan.ok_or_else(|| "No COMP thumb repair is pending.".to_string())?;
                let mut app = state_for_confirm
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
                if app.operation_busy || app.phase != RunPhase::Idle || controller_for_confirm.is_running()
                    || app.library.selection_epoch != plan.selection_epoch
                    || !(if app.library.show_tracks && !plan.artist_wide {
                        app.library.focused_album_key.as_deref().is_some_and(|key| key.eq_ignore_ascii_case(&plan.album_key))
                    } else {
                        app.library.focused_artist.as_deref() == Some(plan.artist.as_str())
                    })
                    || app.config.output.embedded_thumbnail_size != plan.side
                    || !(if plan.artist_wide {
                        plan.album_paths.iter().all(|path| app.library.albums.iter().any(|album| {
                            album.snapshot.album_mbid.trim().is_empty()
                                && Path::new(&album.snapshot.path) == path
                        }))
                    } else {
                        app.library.albums.iter().any(|album| {
                            album.snapshot.album_key.eq_ignore_ascii_case(&plan.album_key)
                                && Path::new(&album.snapshot.path) == plan.album_path
                        })
                    })
                {
                    return Err("The Artist, selection, or preference changed. Open COMP status and retry the repair.".to_string());
                }
                app.operation_busy = true;
                app.operation_status = format!("Repairing completed COMP thumbs · 0/{}", plan.targets.len());
                app.operation_tone = "violet".into();
                Ok::<_, String>(plan)
            })();
            let plan = match prepared {
                Ok(plan) => plan,
                Err(error) => {
                    if let Ok(mut requested) = launch_for_confirm.lock() {
                        *requested = None;
                    }
                    record_action_result(&state_for_confirm, Err(error));
                    refresh_weak(&weak_for_confirm, &state_for_confirm);
                    return;
                }
            };
            refresh_weak(&weak_for_confirm, &state_for_confirm);
            let state = Arc::clone(&state_for_confirm);
            let weak = weak_for_confirm.clone();
            let launch = Arc::clone(&launch_for_confirm);
            let controller = Arc::clone(&controller_for_confirm);
            std::thread::spawn(move || {
                let total = plan.targets.len();
                let mut repaired = 0usize;
                let mut errors = Vec::new();
                let mut aborted = false;
                for track in &plan.targets {
                    let still_selected = state.lock().ok().is_some_and(|app| {
                        app.library.selection_epoch == plan.selection_epoch
                            && (if app.library.show_tracks && !plan.artist_wide {
                                app.library.focused_album_key.as_deref().is_some_and(|key| key.eq_ignore_ascii_case(&plan.album_key))
                            } else {
                                app.library.focused_artist.as_deref() == Some(plan.artist.as_str())
                            })
                    });
                    if !still_selected {
                        aborted = true;
                        break;
                    }
                    match repair_compilation_thumb_from_embedded(track, &plan.cache_dir, plan.side) {
                        Ok(true) => repaired += 1,
                        Ok(false) => {}
                        Err(error) => errors.push(error),
                    }
                    invalidate_embedded_track_metadata(track);
                    cached_embedded_front_metadata(track);
                }
                for album_path in &plan.album_paths {
                    invalidate_album_embedded_check(album_path);
                }
                let all_single_tracks_green = if !plan.artist_wide && errors.is_empty() && !aborted {
                    let scope = state.lock().ok().and_then(|app| {
                        app.library.albums.iter().find(|album| {
                            album.snapshot.album_key.eq_ignore_ascii_case(&plan.album_key)
                        }).map(|album| {
                            (if app.library.show_tracks {
                                album.snapshot.compilation_tracks.iter()
                                    .map(|track| PathBuf::from(&track.path)).collect::<Vec<_>>()
                            } else {
                                artist_fallback_embedded_track_paths(&app, album)
                            })
                                .into_iter()
                                .map(|path| {
                                    let completed = album.snapshot.compilation_tracks.iter().any(|track| {
                                        track.path.eq_ignore_ascii_case(path.to_string_lossy().as_ref())
                                            && track.embedded_artwork_recorded
                                    });
                                    (path, completed)
                                })
                                .collect::<Vec<_>>()
                        })
                    });
                    scope.is_some_and(|tracks| {
                        !tracks.is_empty() && tracks.into_iter().all(|(path, completed)| {
                            completed && cached_embedded_front_metadata(&path).is_some_and(
                                |(width, height, _)| width == plan.side && height == plan.side
                            )
                        })
                    })
                } else {
                    false
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Ok(mut app) = state.lock() {
                        app.operation_busy = false;
                        app.operation_tone = if errors.is_empty() && !aborted
                            && (plan.artist_wide || all_single_tracks_green) { "green" } else { "orange" }.into();
                        app.operation_status = format!(
                            "COMP thumbs · {repaired}/{total} repaired to {} × {}{}{}{}",
                            plan.side, plan.side,
                            if aborted { " · selection changed; remaining tracks skipped" } else { "" },
                            if errors.is_empty() { "" } else { " · see debug for errors" },
                            if !plan.artist_wide && !all_single_tracks_green { " · other tracks still need attention" } else { "" }
                        );
                        app.completion_dialog_visible = false;
                        if plan.artist_wide {
                            let rows = artist_compilation_report_track_ui_rows(&app);
                            let green = rows.iter().filter(|row| row.status.as_str() == "Green").count();
                            let purple = rows.iter().filter(|row| row.status.as_str() == "Purple").count();
                            let blue = rows.len().saturating_sub(green + purple);
                            if purple > 0 || blue > 0 {
                                app.operation_tone = if purple > 0 { "violet" } else { "orange" }.into();
                            }
                            app.compilation_report_summary = format!(
                                "{} albums · {} tracks: {green} green, {purple} thumb check, {blue} need artwork",
                                plan.album_paths.len(), rows.len()
                            );
                        }
                        for error in &errors {
                            app.push_log(format!("COMP thumb repair: {error}"));
                        }
                    }
                    refresh_weak(&weak, &state);
                    let requested_launch = launch.lock().ok().and_then(|mut slot| slot.take());
                    if let Some(include_bypassed) = requested_launch {
                        if !aborted && errors.is_empty() {
                            if let Err(error) = launch_artist_compilation_report_tracks(
                                &state,
                                &controller,
                                include_bypassed,
                            ) {
                                record_action_result(&state, Err(error));
                            }
                        } else if let Ok(mut app) = state.lock() {
                            app.operation_status =
                                "COMP repair stopped or failed; remaining tracks were not launched. See debug."
                                    .into();
                            app.operation_tone = "red".into();
                        }
                        refresh_weak(&weak, &state);
                    }
                    if all_single_tracks_green {
                        let state = Arc::clone(&state);
                        let weak = weak.clone();
                        let artist = plan.artist.clone();
                        let album_key = plan.album_key.clone();
                        let epoch = plan.selection_epoch;
                        slint::Timer::single_shot(Duration::from_secs(2), move || {
                            let collapsed = if let Ok(mut app) = state.lock()
                                && app.library.selection_epoch == epoch
                                && app.library.focused_artist.as_deref() == Some(artist.as_str())
                                && app.library.focused_album_key.as_deref().is_some_and(|key| key.eq_ignore_ascii_case(&album_key))
                                && app.phase == RunPhase::Idle {
                                    app.library.focused_album_key = None;
                                    app.operation_status = format!("COMP complete · {artist} thumbnails verified at the preferred size");
                                    app.operation_tone = "green".into();
                                    true
                                } else {
                                    false
                                };
                            if collapsed {
                                refresh_weak(&weak, &state);
                            }
                        });
                    }
                });
            });
        });
    }
    {
        let pending = Arc::new(Mutex::new(None::<ArtistAlbumEmbedPlan>));
        let state_for_prepare = Arc::clone(&state);
        let weak_for_prepare = ui.as_weak();
        let pending_for_prepare = Arc::clone(&pending);
        ui.on_prepare_artist_embed(move || {
            let prepared = (|| {
                let mut app = state_for_prepare
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
                if app.operation_busy {
                    return Err("Another SPLINED operation is still in progress.".to_string());
                }
                let sources = selected_artist_album_embed_sources(&app)?;
                app.operation_busy = true;
                app.operation_status = "Checking selected Artist Album covers and tracks…".to_string();
                app.operation_tone = "violet".to_string();
                Ok::<_, String>(sources)
            })();
            let (artist, epoch, side, candidates, skipped) = match prepared {
                Ok(sources) => sources,
                Err(error) => {
                    record_action_result(&state_for_prepare, Err(error));
                    refresh_weak(&weak_for_prepare, &state_for_prepare);
                    return;
                }
            };
            refresh_weak(&weak_for_prepare, &state_for_prepare);
            let state = Arc::clone(&state_for_prepare);
            let weak = weak_for_prepare.clone();
            let pending = Arc::clone(&pending_for_prepare);
            std::thread::spawn(move || {
                let result = plan_artist_album_embeds(artist, epoch, side, candidates, skipped);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Ok(mut app) = state.lock() {
                        app.operation_busy = false;
                    }
                    match result {
                        Ok(plan) => {
                            let albums = plan.targets.len();
                            let tracks: usize = plan.targets.iter().map(|target| target.track_count).sum();
                            let summary = format!(
                                "Embed {} × {} cover.* thumbnails into {tracks} tracks across {albums} selected Artist Album{}? {} selected album{} will be skipped. cover.* remains unchanged.",
                                plan.side, plan.side, if albums == 1 { "" } else { "s" },
                                plan.skipped, if plan.skipped == 1 { "" } else { "s" }
                            );
                            if let Ok(mut slot) = pending.lock() {
                                *slot = Some(plan);
                            }
                            if let Some(ui) = weak.upgrade() {
                                ui.set_artist_embed_summary(summary.into());
                                ui.set_confirm_artist_embed(true);
                            }
                        }
                        Err(error) => record_action_result(&state, Err(error)),
                    }
                    refresh_weak(&weak, &state);
                });
            });
        });

        let state_for_confirm = Arc::clone(&state);
        let weak_for_confirm = ui.as_weak();
        ui.on_confirm_artist_embed_action(move |confirmed| {
            let plan = pending.lock().ok().and_then(|mut slot| slot.take());
            if !confirmed {
                if let Ok(mut app) = state_for_confirm.lock() {
                    app.operation_status = "Artist Album embedding cancelled · no files changed".to_string();
                    app.operation_tone = "violet".to_string();
                }
                refresh_weak(&weak_for_confirm, &state_for_confirm);
                return;
            }
            let prepared = (|| {
                let plan = plan.ok_or_else(|| "No Artist Album embed confirmation is pending.".to_string())?;
                let mut app = state_for_confirm.lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
                if app.operation_busy || app.library.selection_epoch != plan.selection_epoch
                    || app.library.focused_artist.as_deref() != Some(plan.artist.as_str())
                    || app.config.output.embedded_thumbnail_size != plan.side
                {
                    return Err("The Artist Album selection or thumbnail preference changed. Press EMBED Artist Art again.".to_string());
                }
                app.operation_busy = true;
                app.operation_status = format!("Embedding selected Artist Albums · 0/{}", plan.targets.len());
                app.operation_tone = "violet".to_string();
                Ok::<_, String>((plan, app.config.clone()))
            })();
            let (plan, config) = match prepared {
                Ok(value) => value,
                Err(error) => {
                    record_action_result(&state_for_confirm, Err(error));
                    refresh_weak(&weak_for_confirm, &state_for_confirm);
                    return;
                }
            };
            refresh_weak(&weak_for_confirm, &state_for_confirm);
            let state = Arc::clone(&state_for_confirm);
            let weak = weak_for_confirm.clone();
            std::thread::spawn(move || {
                let total = plan.targets.len();
                let mut changed_tracks = 0usize;
                let mut completed_albums = 0usize;
                let mut errors = Vec::new();
                let mut aborted = false;
                for target in plan.targets {
                    let still_selected = state.lock().ok().is_some_and(|app| {
                        app.library.selection_epoch == plan.selection_epoch
                            && app.library.focused_artist.as_deref() == Some(plan.artist.as_str())
                    });
                    if !still_selected {
                        aborted = true;
                        break;
                    }
                    let result = fs::read(&target.cover_path)
                        .map_err(|error| format!("{}: unable to read cover.*: {error}", target.title))
                        .and_then(|cover| embed_album_thumbnails_from_cover(&target.album_path, &cover, &config));
                    match result {
                        Ok(written) => {
                            changed_tracks += written;
                            completed_albums += 1;
                            invalidate_album_embedded_check(&target.album_path);
                        }
                        Err(error) => {
                            invalidate_album_embedded_check(&target.album_path);
                            errors.push(error);
                        }
                    }
                    let state_for_update = Arc::clone(&state);
                    let weak_for_update = weak.clone();
                    let done = completed_albums + errors.len();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Ok(mut app) = state_for_update.lock() {
                            app.operation_status = format!("Embedding selected Artist Albums · {done}/{total}");
                        }
                        refresh_weak(&weak_for_update, &state_for_update);
                    });
                }
                let _ = slint::invoke_from_event_loop(move || {
                    if let Ok(mut app) = state.lock() {
                        app.operation_busy = false;
                        app.operation_tone = if errors.is_empty() { "green" } else { "orange" }.to_string();
                        app.operation_status = format!(
                            "Artist Album thumbs · {completed_albums}/{total} albums · {changed_tracks} tracks updated{}{}",
                            if aborted { " · selection changed; remaining albums skipped" } else { "" },
                            if errors.is_empty() { "" } else { " · see debug for errors" }
                        );
                        for error in &errors {
                            app.push_log(format!("Artist Album embed: {error}"));
                        }
                    }
                    refresh_weak(&weak, &state);
                });
            });
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let action_log = ui_debug_log.clone();
        ui.on_approve_banner(move || {
            let prepared = state
                .lock()
                .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                .and_then(|mut app| {
                    if app.operation_busy {
                        return Err("Another SPLINED operation is still in progress.".to_string());
                    }
                    let artist = overview_artist_name(&app)
                        .filter(|name| !name.trim().is_empty())
                        .ok_or_else(|| "Select an artist before approving a banner.".to_string())?
                        .to_string();
                    let source = app.artist_banner_source_path.clone();
                    if source.is_empty() || !Path::new(&source).is_file() {
                        return Err("Preview an available banner before approving it.".to_string());
                    }
                    let (directory, stem) = artist_banner_targets_for_albums(
                        app.library.albums.iter().map(|album| {
                            (
                                album.snapshot.artist.as_str(),
                                album.snapshot.path.as_str(),
                                album.snapshot.album_mbid.as_str(),
                                album.snapshot.artist_mbid.as_str(),
                            )
                        }),
                        &artist,
                        &configured_shared_artist_art_dir(&app.config),
                    )
                    .into_iter()
                    .next()
                    .ok_or_else(|| {
                        "This artist has no eligible album for a saved banner.".to_string()
                    })?;
                    let target = newest_local_artwork_file(&directory, &[&stem], false)
                        .unwrap_or_else(|| directory.join(format!("{stem}.png")));
                    let mut config = app.config.clone();
                    let adjustments = if app.artist_preview_focus == "banner" {
                        &app.adjustments
                    } else {
                        &app.artist_banner_adjustments
                    };
                    project_adjustment_state(adjustments, &mut config);
                    let theme_output = if app.artist_theme_art_enabled {
                        let mut theme = app.config.clone();
                        project_adjustments(&app, &mut theme);
                        Some(theme.output)
                    } else {
                        None
                    };
                    let cache = Path::new(&app.config.scan.temporary_cache_dir)
                        .join("navidrome-thumbnails")
                        .join("artist-banners");
                    let zoom = app.artist_banner_zoom;
                    let horizontal = app.artist_banner_horizontal;
                    let vertical = app.artist_banner_vertical;
                    app.operation_busy = true;
                    app.operation_status = "Saving approved Artist Banner…".to_string();
                    app.operation_tone = "orange".to_string();
                    Ok((
                        artist,
                        source,
                        target,
                        cache,
                        config.output,
                        theme_output,
                        zoom,
                        horizontal,
                        vertical,
                    ))
                });
            let (artist, source, target, cache, output, theme_output, zoom, horizontal, vertical) =
                match prepared {
                    Ok(value) => value,
                    Err(error) => {
                        record_action_result(&state, Err(error));
                        refresh_weak(&weak, &state);
                        return;
                    }
                };
            refresh_weak(&weak, &state);
            let state = Arc::clone(&state);
            let weak = weak.clone();
            let action_log = action_log.clone();
            std::thread::spawn(move || {
                let result = (|| -> Result<PathBuf, String> {
                    let source_bytes = fs::read(&source)
                        .map_err(|error| format!("Unable to read banner source: {error}"))?;
                    let composed = enhanced_artist_banner_with_position(&source_bytes, zoom, horizontal, vertical, &output)?;
                    let composed = if let Some(theme) = &theme_output {
                        apply_selected_image_adjustments(composed, theme)
                    } else {
                        composed
                    };
                    let format =
                        if target
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .is_some_and(|ext| {
                                matches!(ext.to_ascii_lowercase().as_str(), "jpg" | "jpeg")
                            })
                        {
                            image::ImageFormat::Jpeg
                        } else {
                            image::ImageFormat::Png
                        };
                    let composed = if format == image::ImageFormat::Jpeg {
                        image::DynamicImage::ImageRgb8(composed.to_rgb8())
                    } else {
                        composed
                    };
                    let mut encoded = Cursor::new(Vec::new());
                    composed
                        .write_to(&mut encoded, format)
                        .map_err(|error| format!("Unable to encode approved banner: {error}"))?;
                    let bytes = encoded.into_inner();
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent).map_err(|error| {
                            format!("Unable to create banner directory: {error}")
                        })?;
                    }
                    replace_binary_file(&target, &bytes, "approved artist banner", |staged| {
                        inspect_image(staged).map(|_| ())
                    })?;
                    cached_artist_banner_with_bottom_blur(&target, &cache, &artist)
                })();
                if let Some(log) = action_log.as_ref() {
                    log.record(format!(
                        "artist-banner-approval artist={} target={} success={}",
                        artist,
                        target.display(),
                        result.is_ok()
                    ));
                }
                let _ = slint::invoke_from_event_loop(move || {
                    if let Ok(mut app) = state.lock() {
                        app.operation_busy = false;
                        if let Ok(path) = &result {
                            let banner = path.to_string_lossy().into_owned();
                            if overview_artist_name(&app)
                                .is_some_and(|name| name.eq_ignore_ascii_case(&artist))
                            {
                                app.artist_overview_banner_path = banner.clone();
                                app.artist_banner_preview_path.clear();
                                app.artist_banner_source_path.clear();
                                app.artist_banner_preview_candidate_index = None;
                                app.artist_theme_art_enabled = false;
                                app.artist_theme_portrait_preview_path.clear();
                                app.artist_theme_banner_preview_path.clear();
                                app.artist_portrait_preview_candidate_index = None;
                                app.artist_portrait_preview_path.clear();
                                app.artist_artwork_draft_path.clear();
                                app.artist_artwork_mode = false;
                            }
                            if let Some(cached) = app
                                .artist_overview_cache
                                .get_mut(&artist.trim().to_ascii_lowercase())
                            {
                                cached.banner_path = banner;
                                cached.banner_uses_artist_art = false;
                            }
                            app.operation_status = "Artist Banner approved and saved".to_string();
                            app.operation_tone = "green".to_string();
                        } else {
                            app.operation_status = "Artist Banner approval failed".to_string();
                            app.operation_tone = "red".to_string();
                        }
                    }
                    record_action_result(&state, result.map(|_| ()));
                    refresh_weak(&weak, &state);
                });
            });
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let action_log = ui_debug_log.clone();
        ui.on_write_artist_artwork(move || {
            if state.lock().is_ok_and(|app| {
                app.artist_artwork_mode
                    && !app.artist_portrait_radio_active
                    && app.artist_banner_radio_active
                    && !app.artist_banner_source_path.is_empty()
            }) {
                if let Some(ui) = weak.upgrade() {
                    ui.invoke_approve_banner();
                }
                return;
            }
            let prepared = (|| -> Result<(String, String, ArtistArtTarget, bool, PathBuf, bool, bool, splined_core::config::OutputConfig, Option<splined_core::config::OutputConfig>, (f32, f32, f32)), String> {
                let mut app = state
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
                if app.operation_busy {
                    return Err("Another SPLINED operation is still in progress.".to_string());
                }
                if app.artist_artwork_mode && !app.artist_portrait_radio_active {
                    return Err("Select the Artist image radio before writing artist.*.".to_string());
                }
                let artist = app
                    .library
                    .focused_artist
                    .clone()
                    .or_else(|| overview_artist_name(&app).map(str::to_owned))
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| "Select a Navidrome-linked artist first.".to_string())?;
                let selected = app.artist_artwork_draft_path.trim().to_string();
                if selected.is_empty() {
                    return Err("Select an artist artwork image before writing.".to_string());
                }
                inspect_image(Path::new(&selected))?;
                let artist_target = focused_artist_art_target(&app, &artist)?;
                let preserve_file = app.config.output.preserve_file;
                let banner_cache = Path::new(&app.config.scan.temporary_cache_dir)
                    .join("navidrome-thumbnails")
                    .join("artist-banners");
                let refresh_fallback_banner = app.artist_overview_cache
                    .get(&artist.trim().to_ascii_lowercase())
                    .is_none_or(|entry| entry.banner_uses_artist_art || entry.banner_path.is_empty());
                let write_banner_after = app.artist_banner_radio_active
                    && !app.artist_banner_source_path.is_empty();
                let mut config = app.config.clone();
                let adjustments = if app.artist_preview_focus == "portrait" {
                    &app.adjustments
                } else {
                    &app.artist_portrait_adjustments
                };
                project_adjustment_state(adjustments, &mut config);
                let theme_output = if app.artist_theme_art_enabled {
                    let mut theme = app.config.clone();
                    project_adjustments(&app, &mut theme);
                    Some(theme.output)
                } else {
                    None
                };
                app.operation_busy = true;
                app.operation_status = "Saving artist.*…".to_string();
                app.operation_tone = "orange".to_string();
                app.last_error = None;
                Ok((artist, selected, artist_target, preserve_file, banner_cache, refresh_fallback_banner, write_banner_after, config.output, theme_output, (app.artist_portrait_zoom, app.artist_portrait_horizontal, app.artist_portrait_vertical)))
            })();
            let (artist, selected, artist_target, preserve_file, banner_cache, refresh_fallback_banner, write_banner_after, output, theme_output, portrait_fit) = match prepared {
                Ok(value) => value,
                Err(error) => {
                    record_action_result(&state, Err(error));
                    refresh_weak(&weak, &state);
                    return;
                }
            };
            if let Some(log) = action_log.as_ref() {
                log.record(format!(
                    "artist-art-write-start artist={} selected={} directory={} stem={} managed={} preserve_file={}",
                    artist,
                    selected,
                    artist_target.directory.display(),
                    artist_target.stem,
                    artist_target.managed,
                    preserve_file
                ));
            }
            refresh_weak(&weak, &state);
            let state = Arc::clone(&state);
            let weak = weak.clone();
            let action_log = action_log.clone();
            std::thread::spawn(move || {
                let result = (|| -> Result<(PathBuf, bool, Vec<String>, Option<PathBuf>), String> {
                    let source_bytes = fs::read(&selected).map_err(|error| {
                        format!("Unable to read selected artist artwork {selected}: {error}")
                    })?;
                    let prepared_bytes = prepare_artist_art_png_with_fit(&source_bytes, Some(&output), portrait_fit.0, portrait_fit.1, portrait_fit.2)?;
                    let prepared_bytes = if let Some(theme) = &theme_output {
                        let image = image::load_from_memory(&prepared_bytes)
                            .map_err(|error| format!("Unable to decode Artist Art for Theme Art: {error}"))?;
                        let image = apply_selected_image_adjustments(image, theme);
                        let mut encoded = Cursor::new(Vec::new());
                        image.write_to(&mut encoded, image::ImageFormat::Png)
                            .map_err(|error| format!("Unable to encode themed Artist Art: {error}"))?;
                        encoded.into_inner()
                    } else {
                        prepared_bytes
                    };
                    let (target, unchanged, mut warnings) = write_prepared_artist_art_file_with_stem(
                        &artist_target.directory,
                        &artist_target.stem,
                        &prepared_bytes,
                        preserve_file,
                    )?;
                    let banner = if refresh_fallback_banner {
                        match cached_artist_banner_with_bottom_blur(&target, &banner_cache, &artist) {
                            Ok(path) => Some(path),
                            Err(error) => {
                                warnings.push(format!("Artist Art saved, but fallback banner refresh failed: {error}"));
                                None
                            }
                        }
                    } else { None };
                    Ok((target, unchanged, warnings, banner))
                })();
                let succeeded = result.is_ok();
                if let Some(log) = action_log.as_ref() {
                    log.record(format!(
                        "artist-art-write-finished artist={} target={} success={} preserve_file={}",
                        artist,
                        result
                            .as_ref()
                            .map(|(path, _, _, _)| path.display().to_string())
                            .unwrap_or_default(),
                        succeeded,
                        preserve_file
                    ));
                }
                let _ = slint::invoke_from_event_loop(move || {
                    if let Ok(mut app) = state.lock() {
                        app.operation_busy = false;
                        if let Ok((target, unchanged, warnings, banner)) = &result {
                            app.library.set_artist_artwork_path(&artist, target);
                            if let Some(banner) = banner {
                                let banner_path = banner.to_string_lossy().into_owned();
                                if overview_artist_name(&app).is_some_and(|current| current.eq_ignore_ascii_case(&artist)) {
                                    app.artist_overview_banner_path = banner_path.clone();
                                }
                                for (name, cached) in &mut app.artist_overview_cache {
                                    if name.eq_ignore_ascii_case(&artist) {
                                        cached.banner_path = banner_path.clone();
                                        cached.banner_uses_artist_art = true;
                                    }
                                }
                            }
                            app.artist_artwork_draft_path.clear();
                            app.artist_portrait_preview_candidate_index = None;
                            app.artist_portrait_preview_path.clear();
                            app.edit_preview_path.clear();
                            app.artist_artwork_mode = false;
                            let file_name = target
                                .file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or("artist.*");
                            app.operation_status = if *unchanged {
                                format!("Artist Art unchanged · {file_name}")
                            } else if warnings.is_empty() {
                                format!("Saved {file_name}")
                            } else {
                                format!(
                                    "Saved {file_name}; some older Artist Art could not be removed"
                                )
                            };
                            app.operation_tone = if warnings.is_empty() {
                                "green"
                            } else {
                                "orange"
                            }
                            .to_string();
                            for warning in warnings {
                                app.push_log(warning.clone());
                            }
                            app.push_log(format!(
                                "Wrote {artist} artist artwork to {}.",
                                target.display()
                            ));
                        } else {
                            app.operation_status = "Artist artwork write failed".to_string();
                            app.operation_tone = "red".to_string();
                        }
                    }
                    record_action_result(&state, result.map(|_| ()));
                    refresh_weak(&weak, &state);
                    if succeeded && write_banner_after
                        && let Some(ui) = weak.upgrade()
                    {
                        ui.invoke_approve_banner();
                    }
                });
            });
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let action_log = ui_debug_log.clone();
        ui.on_library_row_selection_activated(move |key, include_bypassed| {
            if state
                .lock()
                .is_ok_and(|app| album_key_embed_locked(&app, key.as_str()))
            {
                return;
            }
            let started = Instant::now();
            let hydration = activate_library_row_selection(&state, key.as_str(), include_bypassed);
            refresh_weak(&weak, &state);
            if let Some(log) = action_log.as_ref() {
                log.record(format!(
                    "library-radio-click key={} elapsed_ms={}",
                    key,
                    started.elapsed().as_millis()
                ));
            }
            if let Some((album_key, album_path, cached)) = hydration {
                let weak = weak.clone();
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    let result = fallback_tracks_from_folder(Path::new(&album_path), &cached);
                    let _ = slint::invoke_from_event_loop(move || {
                        match result {
                            Ok(tracks) => {
                                if let Ok(mut app) = state.lock() {
                                    replace_fallback_tracks_and_invalidate(
                                        &mut app, &album_key, tracks,
                                    );
                                }
                            }
                            Err(error) => record_action_result(&state, Err(error)),
                        }
                        refresh_weak(&weak, &state);
                    });
                });
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let action_log = ui_debug_log.clone();
        ui.on_library_row_expansion_activated(move |key| {
            let started = Instant::now();
            let hydration = activate_library_row_expansion(&state, key.as_str());
            refresh_weak(&weak, &state);
            if let Some(log) = action_log.as_ref() {
                log.record(format!(
                    "library-expand-click key={} elapsed_ms={}",
                    key,
                    started.elapsed().as_millis()
                ));
            }
            if let Some((album_key, album_path, cached)) = hydration {
                let weak = weak.clone();
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    let result = fallback_tracks_from_folder(Path::new(&album_path), &cached);
                    let _ = slint::invoke_from_event_loop(move || {
                        match result {
                            Ok(tracks) => {
                                if let Ok(mut app) = state.lock() {
                                    replace_fallback_tracks_and_invalidate(
                                        &mut app, &album_key, tracks,
                                    );
                                }
                            }
                            Err(error) => record_action_result(&state, Err(error)),
                        }
                        refresh_weak(&weak, &state);
                    });
                });
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_select_all(move || {
            let result = state
                .lock()
                .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                .map(|mut app| {
                    if app.library.show_tracks && app.library.focused_album_key.is_some() {
                        app.scan_scope = ScanScope::Selected;
                    }
                    app.library.select_all_for_review();
                });
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_select_filtered(move || {
            let result = state
                .lock()
                .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                .and_then(|mut app| app.library.select_filtered_for_review().map(|_| ()));
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    }
    bind_simple_library_action(ui, &state, |app| app.library.select_none(), "none");
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_clear_artist_scope(move || {
            if let Ok(mut app) = state.lock() {
                app.library.clear_artist_scope();
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let controller = Arc::clone(&controller);
        ui.on_refresh_library(move |rebuild| {
            if controller.is_running()
                || weak
                    .upgrade()
                    .is_some_and(|ui| ui.get_media_refresh_running())
            {
                return;
            }
            NEW_MEDIA_CHECK_REVISION.fetch_add(1, Ordering::SeqCst);
            if let Ok(mut app) = state.lock() {
                app.operation_busy = true;
                app.operation_status =
                    "Refreshing music folders and representative Album tags…".into();
                app.operation_tone = "orange".into();
                app.push_log("Refresh DB started; music files will not be changed.".into());
            }
            if let Some(ui) = weak.upgrade() {
                ui.set_media_refresh_running(true);
            }
            refresh_weak(&weak, &state);
            let controller = Arc::clone(&controller);
            let state = Arc::clone(&state);
            let weak = weak.clone();
            std::thread::spawn(move || {
                let result = controller.refresh_library(rebuild);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Ok(mut app) = state.lock() {
                        app.operation_busy = false;
                        match &result {
                            Ok(()) => {
                                app.operation_status =
                                    "Music library index refreshed · no music files changed".into();
                                app.operation_tone = "green".into();
                                app.push_log("Refresh DB completed.".into());
                            }
                            Err(error) => {
                                app.operation_status = format!("Refresh DB failed · {error}");
                                app.operation_tone = "red".into();
                                app.last_error = Some(error.clone());
                                app.push_log(format!("ERROR: {error}"));
                            }
                        }
                    }
                    if let Some(ui) = weak.upgrade() {
                        ui.set_media_refresh_running(false);
                        if result.is_ok() {
                            ui.set_new_media_warning_visible(false);
                        }
                    }
                    refresh_weak(&weak, &state);
                });
            });
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_set_auto_scan(move |all, enabled| {
            if let Ok(mut app) = state.lock() {
                app.auto_scan_enabled = enabled;
                if enabled {
                    app.scan_scope = if all {
                        ScanScope::All
                    } else {
                        ScanScope::Selected
                    };
                }
                app.push_log(if enabled {
                    format!(
                        "Auto Scan [{}] enabled; only Ideal candidates are accepted unattended.",
                        if all { "All" } else { "Selected" }
                    )
                } else {
                    "Auto Scan disabled; operator review remains active.".to_string()
                });
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_set_scan_scope(move |currently_selected| {
            if let Ok(mut app) = state.lock() {
                app.scan_scope = if currently_selected {
                    ScanScope::All
                } else {
                    ScanScope::Selected
                };
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_set_launch_mode(move |currently_read| {
            let result = (|| {
                let mut app = state
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
                if app.phase != RunPhase::Idle {
                    return Err("Processing mode cannot change during an active run.".to_string());
                }
                let mode = if currently_read {
                    LaunchMode::LiveWrite
                } else {
                    LaunchMode::Read
                };
                let mut config = app.config.clone();
                config.mode = mode.config_mode();
                app.config = splined_core::settings::save_config(&config)?;
                app.launch_mode = mode;
                app.push_log(format!(
                    "Saved {} processing mode to Config v5.",
                    if mode == LaunchMode::LiveWrite {
                        "LIVE WRITE"
                    } else {
                        "READ"
                    }
                ));
                Ok(())
            })();
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let controller = Arc::clone(&controller);
        let workspace_layout = Rc::clone(&workspace_layout);
        ui.on_launch(move || {
            if let Some(ui) = weak.upgrade() {
                let (media_expanded, artwork_visible, candidate_filter_expanded) = state
                    .lock()
                    .map(|app| {
                        (
                            app.media_selector_expanded,
                            app.show_artwork,
                            app.candidate_filter_expanded,
                        )
                    })
                    .unwrap_or((true, true, true));
                let (window_width, window_height) = app_logical_window_size(&ui);
                let mut layout = workspace_layout.borrow_mut();
                layout.clamp_for_window(
                    window_width,
                    window_height,
                    media_expanded,
                    artwork_visible,
                    candidate_filter_expanded,
                );
                apply_workspace_layout(&ui, &layout);
            }
            if let Err(error) = controller.start()
                && let Ok(mut app) = state.lock()
            {
                app.last_error = Some(error.clone());
                app.push_log(format!("ERROR: {error}"));
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let weak = ui.as_weak();
        let state = Arc::clone(&state);
        let controller = Arc::clone(&controller);
        ui.on_stop(move || {
            controller.stop();
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_candidate_selected(move |index| {
            if let Ok(mut app) = state.lock()
                && app.select_candidate(index.max(0) as usize)
                && app.artist_artwork_mode
            {
                app.operation_status =
                    "Candidate selected · use blue/pink preview radios to stage Artist Art"
                        .to_string();
                app.operation_tone = "orange".to_string();
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_candidate_portrait_preview(move |index| {
            if let Some(ui) = weak.upgrade() { ui.set_artist_preview_reset_active(false); }
            if let Ok(mut app) = state.lock()
                && app.artist_artwork_mode
            {
                let index = index.max(0) as usize;
                let portrait = app
                    .candidates
                    .iter()
                    .find(|candidate| candidate.index == index)
                    .filter(|candidate| candidate.image_type != "ArtistBanner")
                    .map(|candidate| candidate.cache_path.clone());
                if let Some(path) = portrait {
                    app.artist_current_radio_active = false;
                    app.artist_theme_art_enabled = false;
                    app.artist_theme_portrait_preview_path.clear();
                    app.artist_theme_banner_preview_path.clear();
                    if app.artist_portrait_preview_candidate_index == Some(index) {
                        app.artist_portrait_preview_candidate_index = None;
                        app.artist_artwork_draft_path.clear();
                        app.artist_portrait_preview_path.clear();
                        app.artist_portrait_radio_active = false;
                    } else {
                        app.artist_portrait_preview_candidate_index = Some(index);
                        app.artist_artwork_draft_path = path.clone();
                        app.artist_portrait_preview_path = path;
                        app.artist_portrait_zoom = 1.0;
                        app.artist_portrait_horizontal = 0.0;
                        app.artist_portrait_vertical = 0.0;
                        app.artist_portrait_radio_active = true;
                    }
                    update_artist_preview_activation(&mut app, "portrait");
                    if app.artist_portrait_preview_candidate_index.is_some() {
                        app.tint_palette_source.clear();
                        app.adjustments.tint_percent = 0;
                    }
                    app.operation_status =
                        "Artist portrait preview changed · no files written".to_string();
                    app.operation_tone = "cyan".to_string();
                }
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_artist_current_package_selected(move || {
            if let Some(ui) = weak.upgrade() { ui.set_artist_preview_reset_active(false); }
            if let Ok(mut app) = state.lock() && app.artist_artwork_mode {
                if app.artist_current_radio_active {
                    app.artist_current_radio_active = false;
                    app.artist_theme_art_enabled = false;
                    app.artist_artwork_draft_path.clear();
                    app.artist_portrait_preview_path.clear();
                    app.artist_banner_source_path.clear();
                    app.artist_banner_preview_path.clear();
                    app.artist_portrait_radio_active = false;
                    app.artist_banner_radio_active = false;
                    update_artist_preview_activation(&mut app, "portrait");
                } else {
                    let artist = overview_artist_name(&app).unwrap_or_default().to_ascii_lowercase();
                    let portrait = app.library.artist_artwork_paths.get(&artist).cloned().unwrap_or_default();
                    let banner = if Path::new(&app.artist_overview_banner_path).is_file() {
                        app.artist_overview_banner_path.clone()
                    } else {
                        portrait.clone()
                    };
                    if Path::new(&portrait).is_file() && Path::new(&banner).is_file() {
                        app.artist_current_radio_active = true;
                        app.artist_artwork_draft_path = portrait.clone();
                        app.artist_portrait_preview_path = portrait;
                        app.artist_portrait_preview_candidate_index = None;
                        app.artist_banner_source_path = banner.clone();
                        app.artist_banner_preview_path = banner;
                        app.artist_banner_preview_candidate_index = None;
                        app.artist_portrait_zoom = 1.0;
                        app.artist_portrait_horizontal = 0.0;
                        app.artist_portrait_vertical = 0.0;
                        app.artist_banner_zoom = auto_artist_banner_zoom(Path::new(&app.artist_banner_source_path));
                        app.artist_banner_horizontal = 0.0;
                        app.artist_banner_vertical = 0.0;
                        app.artist_portrait_radio_active = true;
                        app.artist_banner_radio_active = true;
                        app.artist_theme_art_enabled = true;
                        update_artist_preview_activation(&mut app, "portrait");
                        app.operation_status = "Current Artist Art package loaded for editing; saved files are unchanged".to_string();
                        app.operation_tone = "green".to_string();
                    } else {
                        app.operation_status = "Current Artist Art or banner is unavailable".to_string();
                        app.operation_tone = "orange".to_string();
                    }
                }
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_artist_preview_pane_selected(move |focus| {
            if let Ok(mut app) = state.lock()
                && app.artist_artwork_mode
            {
                let focus = focus.as_str();
                app.artist_current_radio_active = false;
                if focus == "portrait" && !app.artist_artwork_draft_path.is_empty() {
                    app.artist_portrait_radio_active = !app.artist_portrait_radio_active;
                    update_artist_preview_activation(&mut app, focus);
                } else if focus == "banner" && !app.artist_banner_source_path.is_empty() {
                    app.artist_banner_radio_active = !app.artist_banner_radio_active;
                    update_artist_preview_activation(&mut app, focus);
                }
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let revision = Arc::clone(&adjustment_preview_revision);
        let gate = Arc::clone(&adjustment_preview_gate);
        ui.on_artist_theme_art_toggle(move || {
            let mut render_theme = false;
            if let Ok(mut app) = state.lock()
                && app.artist_artwork_mode
            {
                if !app.artist_package_overview_enabled
                    || app.artist_artwork_draft_path.is_empty()
                    || app.artist_banner_source_path.is_empty()
                {
                    app.operation_status =
                        "Select both Artist image and banner radios before Theme Art".to_string();
                    app.operation_tone = "orange".to_string();
                } else {
                    app.artist_theme_art_enabled = !app.artist_theme_art_enabled;
                    if app.artist_theme_art_enabled {
                        switch_artist_preview_focus(&mut app, "theme");
                        render_theme = true;
                        app.operation_status =
                            "Theme Art enabled for the Artist Art package".to_string();
                    } else {
                        let focus = app.artist_last_individual_focus.clone();
                        switch_artist_preview_focus(&mut app, &focus);
                        app.operation_status =
                            "Theme Art disabled; individual adjustments restored".to_string();
                    }
                    app.operation_tone = "cyan".to_string();
                }
            }
            refresh_weak(&weak, &state);
            if render_theme {
                schedule_adjustment_preview(
                    Arc::clone(&state),
                    weak.clone(),
                    Arc::clone(&revision),
                    Arc::clone(&gate),
                );
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_candidate_banner_preview(move |index| {
            if let Some(ui) = weak.upgrade() { ui.set_artist_preview_reset_active(false); }
            if let Ok(mut app) = state.lock() {
                let candidate = app
                    .candidates
                    .iter()
                    .find(|candidate| candidate.index == index.max(0) as usize)
                    .and_then(|candidate| {
                        associated_banner_candidate(candidate, &app.candidates)
                            .map(|banner| banner.cache_path.clone())
                    });
                if app.artist_artwork_mode
                    && let Some(path) = candidate
                {
                    app.artist_current_radio_active = false;
                    app.artist_theme_art_enabled = false;
                    app.artist_theme_portrait_preview_path.clear();
                    app.artist_theme_banner_preview_path.clear();
                    if app.artist_banner_preview_candidate_index == Some(index.max(0) as usize) {
                        app.artist_banner_source_path.clear();
                        app.artist_banner_preview_path.clear();
                        if app
                            .edit_preview_path
                            .starts_with("memory-preview://artist-banner/")
                        {
                            app.edit_preview_path.clear();
                        }
                        app.artist_banner_preview_candidate_index = None;
                        app.artist_banner_radio_active = false;
                        app.operation_status =
                            "Banner preview closed · showing saved Artist Overview".to_string();
                    } else {
                        app.artist_banner_source_path = path.clone();
                        app.artist_banner_preview_path = path.clone();
                        app.artist_banner_preview_candidate_index = Some(index.max(0) as usize);
                        app.artist_banner_zoom = auto_artist_banner_zoom(Path::new(&path));
                        app.artist_banner_horizontal = 0.0;
                        app.artist_banner_vertical = 0.0;
                        app.artist_banner_radio_active = true;
                        app.operation_status =
                            "Banner preview · framing only; no image was saved".to_string();
                    }
                    update_artist_preview_activation(&mut app, "banner");
                    if app.artist_banner_preview_candidate_index.is_some() {
                        app.tint_palette_source.clear();
                        app.adjustments.tint_percent = 0;
                    }
                    app.operation_tone = "cyan".to_string();
                }
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let revision = Arc::clone(&adjustment_preview_revision);
        let gate = Arc::clone(&adjustment_preview_gate);
        ui.on_banner_framing(move |zoom, vertical| {
            let mut render_theme = false;
            if let Ok(mut app) = state.lock()
                && !app.artist_banner_source_path.is_empty()
            {
                app.artist_banner_zoom = zoom.clamp(1.0, 8.0);
                app.artist_banner_vertical = vertical.clamp(-0.4, 0.4);
                app.artist_theme_banner_preview_path.clear();
                // The adjusted frame is a preview only; clicking Upscale Preview
                // renders the same framing in memory without writing a banner.
                app.artist_banner_preview_path = app.artist_banner_source_path.clone();
                if app
                    .edit_preview_path
                    .starts_with("memory-preview://artist-banner/")
                {
                    app.edit_preview_path.clear();
                }
                render_theme = app.artist_package_overview_enabled && app.artist_theme_art_enabled;
            }
            refresh_weak(&weak, &state);
            if render_theme {
                schedule_adjustment_preview(
                    Arc::clone(&state),
                    weak.clone(),
                    Arc::clone(&revision),
                    Arc::clone(&gate),
                );
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_banner_framing_pan(move |zoom, horizontal, vertical| {
            if let Some(ui) = weak.upgrade() { ui.set_artist_preview_reset_active(false); }
            if let Ok(mut app) = state.lock() && !app.artist_banner_source_path.is_empty() {
                app.artist_banner_zoom = zoom.clamp(1.0, 8.0);
                app.artist_banner_horizontal = horizontal.clamp(-0.4, 0.4);
                app.artist_banner_vertical = vertical.clamp(-0.4, 0.4);
                app.artist_banner_preview_path = app.artist_banner_source_path.clone();
                app.artist_theme_banner_preview_path.clear();
                app.edit_preview_path.clear();
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_portrait_framing(move |zoom, horizontal, vertical| {
            if let Some(ui) = weak.upgrade() { ui.set_artist_preview_reset_active(false); }
            if let Ok(mut app) = state.lock() && !app.artist_artwork_draft_path.is_empty() {
                app.artist_portrait_zoom = zoom.clamp(1.0, 8.0);
                app.artist_portrait_horizontal = horizontal.clamp(-0.4, 0.4);
                app.artist_portrait_vertical = vertical.clamp(-0.4, 0.4);
                app.artist_portrait_preview_path = app.artist_artwork_draft_path.clone();
                app.artist_theme_portrait_preview_path.clear();
                app.edit_preview_path.clear();
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_reset_artist_preview(move || {
            if let Ok(mut app) = state.lock() && app.artist_artwork_mode {
                reset_enhancement_preview_state(&mut app);
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let revision = Arc::clone(&adjustment_preview_revision);
        let gate = Arc::clone(&adjustment_preview_gate);
        ui.on_auto_banner_framing(move || {
            let mut render_theme = false;
            if let Ok(mut app) = state.lock()
                && !app.artist_banner_source_path.is_empty()
            {
                app.artist_banner_zoom =
                    auto_artist_banner_zoom(Path::new(&app.artist_banner_source_path));
                app.artist_banner_horizontal = 0.0;
                app.artist_banner_vertical = 0.0;
                app.artist_theme_banner_preview_path.clear();
                app.artist_banner_preview_path = app.artist_banner_source_path.clone();
                app.edit_preview_path.clear();
                render_theme = app.artist_package_overview_enabled && app.artist_theme_art_enabled;
            }
            refresh_weak(&weak, &state);
            if render_theme {
                schedule_adjustment_preview(
                    Arc::clone(&state),
                    weak.clone(),
                    Arc::clone(&revision),
                    Arc::clone(&gate),
                );
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_candidate_filter_cycle(move |dimension, key| {
            let result = state
                .lock()
                .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                .and_then(|mut app| {
                    if app.candidate_filter_locked {
                        Ok(())
                    } else {
                        app.cycle_candidate_filter(dimension.as_str(), key.as_str())
                            .map(|_| ())
                    }
                });
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_clear_candidate_filters(move || {
            if let Ok(mut app) = state.lock()
                && !app.candidate_filter_locked
            {
                app.activate_available_candidate_filters();
                app.push_log("Candidate filters restored for the current results.".to_string());
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_candidate_compare_toggle(move |index| {
            if let Ok(mut app) = state.lock() {
                app.toggle_candidate_comparison(index.max(0) as usize);
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let compare_window = Rc::clone(&compare_window);
        ui.on_open_candidate_compare(move || {
            if compare_window.borrow().is_none() {
                match CandidateCompareWindow::new() {
                    Ok(window) => {
                        let compare_weak = window.as_weak();
                        let state_for_choose = Arc::clone(&state);
                        let main_weak = weak.clone();
                        window.on_choose(move |index| {
                            if let Ok(mut app) = state_for_choose.lock() {
                                app.select_candidate(index.max(0) as usize);
                            }
                            if let Some(window) = compare_weak.upgrade() {
                                let _ = window.hide();
                            }
                            refresh_weak(&main_weak, &state_for_choose);
                        });
                        *compare_window.borrow_mut() = Some(window);
                    }
                    Err(error) => {
                        record_action_result(
                            &state,
                            Err(format!("Unable to open candidate comparison: {error}")),
                        );
                    }
                }
            }
            if let Some(window) = compare_window.borrow().as_ref() {
                sync_candidate_compare(window, &state);
                let _ = window.show();
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_candidate_hover(move |index, active| {
            // Candidate cards are destroyed when MusicBrainz replaces the normal
            // workspace. Slint can emit this hover change synchronously from
            // sync_ui while that function owns the state mutex, so never wait on
            // the same mutex from the UI event loop.
            if let Ok(mut app) = state.try_lock() {
                app.set_candidate_hover(index.max(0) as usize, active);
                if let Some(ui) = weak.upgrade() {
                    let preview_path = app
                        .preview_candidate()
                        .map(|candidate| candidate.cache_path.as_str())
                        .unwrap_or(&app.current_cover_path);
                    ui.set_current_preview(load_image(preview_path));
                }
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_set_hover_enabled(move |enabled| {
            if let Ok(mut app) = state.lock() {
                app.hover_enabled = enabled;
                if !enabled {
                    app.hovered_candidate = None;
                }
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let controller = Arc::clone(&controller);
        ui.on_fallback_retry(move |artist, album| {
            let decision = state
                .lock()
                .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                .and_then(|app| app.fallback_retry_decision(artist.as_str(), album.as_str()));
            let result = decision.and_then(|decision| controller.submit_decision(decision));
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let controller = Arc::clone(&controller);
        ui.on_candidate_action(move |action| {
            let result = if action.as_str() == "retry_musicbrainz"
                || action.as_str() == "back_to_musicbrainz"
            {
                controller.open_musicbrainz()
            } else if action.as_str() == "write_musicbrainz" {
                let decision = state
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                    .and_then(|app| {
                        require_active_compilation_write_target(&app)?;
                        let mut decision = app.write_musicbrainz_preview_decision()?;
                        add_embedded_thumbnail_decision(&app, &mut decision);
                        Ok(decision)
                    });
                decision.and_then(|decision| controller.write_selected_musicbrainz(decision))
            } else if action.as_str() == "write_existing_musicbrainz" {
                let decision = state
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                    .and_then(|app| {
                        require_active_compilation_write_target(&app)?;
                        let mut decision = app.musicbrainz_existing_decision()?;
                        add_embedded_thumbnail_decision(&app, &mut decision);
                        Ok(decision)
                    });
                decision.and_then(|decision| controller.write_selected_musicbrainz(decision))
            } else {
                let decision = state
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                    .and_then(|app| {
                        if action.as_str() == "write" {
                            require_active_compilation_write_target(&app)?;
                        }
                        let mut decision = app.candidate_decision(action.as_str())?;
                        add_embedded_thumbnail_decision(&app, &mut decision);
                        Ok(decision)
                    });
                decision.and_then(|decision| controller.submit_decision(decision))
            };
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_clear_current_bypass(move || {
            let result = (|| {
                let (config, album_path, album_key) = {
                    let app = state
                        .lock()
                        .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
                    let album_key = app
                        .library
                        .focused_album_key
                        .clone()
                        .ok_or_else(|| "Select a bypassed album first.".to_string())?;
                    let album = app
                        .library
                        .albums
                        .iter()
                        .find(|album| album.snapshot.album_key.eq_ignore_ascii_case(&album_key))
                        .ok_or_else(|| "The selected album is no longer available.".to_string())?;
                    (app.config.clone(), album.snapshot.path.clone(), album_key)
                };
                if !clear_album_bypass_from_runtime(
                    &config,
                    Path::new(&album_path),
                    None,
                    Some(&album_key),
                )? {
                    return Err("The selected album is not currently bypassed.".to_string());
                }
                let snapshot = media_snapshot(&config, false)?;
                let mut app = state
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
                app.library.replace_snapshot(snapshot)?;
                app.synchronize_focused_album();
                app.push_log("Album bypass removed; status returned to Unprocessed.".to_string());
                Ok(())
            })();
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_musicbrainz_filter(move |artist, release_type| {
            if artist.trim().is_empty() || release_type.trim().is_empty() {
                return;
            }
            // ComboBox can emit `selected` while sync_ui updates its model. That
            // update already holds this mutex, so a blocking lock here freezes
            // the entire Slint event loop before the user can make a decision.
            if let Ok(mut app) = state.try_lock() {
                let requested_artist = (artist.as_str() != "All").then_some(artist.as_str());
                let requested_type =
                    (release_type.as_str() != "All").then_some(release_type.as_str());
                if app.musicbrainz_artist_filter.as_deref() == requested_artist
                    && app.musicbrainz_release_type_filter.as_deref() == requested_type
                {
                    return;
                }
                app.set_musicbrainz_artist_filter(artist.as_str());
                app.set_musicbrainz_release_type_filter(release_type.as_str());
                drop(app);
                refresh_weak(&weak, &state);
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_open_musicbrainz_info(move || {
            let target = state
                .lock()
                .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                .map(|app| musicbrainz_failed_url(&app));
            let result = target.and_then(|url| open_external_url(&url));
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_open_musicbrainz_page(move |index| {
            let target = state
                .lock()
                .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                .and_then(|mut app| {
                    let selected_index = if index < 0 {
                        app.selected_musicbrainz_result.ok_or_else(|| {
                            "Select a different MusicBrainz album first.".to_string()
                        })?
                    } else {
                        index as usize
                    };
                    let authority_release_mbid = app.musicbrainz_authority.release_mbid.clone();
                    let item = app
                        .musicbrainz_results
                        .iter_mut()
                        .find(|item| item.index == selected_index)
                        .ok_or_else(|| {
                            "The selected MusicBrainz result is unavailable.".to_string()
                        })?;
                    if index < 0
                        && (item.current
                            || item
                                .release_mbid
                                .eq_ignore_ascii_case(&authority_release_mbid))
                    {
                        return Err("Select a different MusicBrainz album first.".to_string());
                    }
                    let release_mbid = item.release_mbid.clone();
                    if !is_valid_mbid(&release_mbid) {
                        return Err(
                            "A valid MusicBrainz release ID is required before opening its page."
                                .to_string(),
                        );
                    }
                    item.visited = true;
                    Ok(format!("https://musicbrainz.org/release/{release_mbid}"))
                });
            let result = target.and_then(|url| open_external_url(&url));
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_musicbrainz_selected(move |index| {
            let selected_index = index.max(0) as usize;
            let mut load_cover = false;
            let mut selected = false;
            if let Ok(mut app) = state.lock()
                && app.select_musicbrainz_result(selected_index)
            {
                selected = true;
                let cached_hover_path = app
                    .musicbrainz_preview_cache
                    .get(&selected_index)
                    .cloned()
                    .or_else(|| {
                        (app.musicbrainz_hover_preview_index == Some(selected_index))
                            .then(|| app.musicbrainz_hover_preview_path.trim().to_string())
                            .filter(|path| !path.is_empty())
                    });
                let hover_loading = app
                    .musicbrainz_preview_inflight
                    .contains(&selected_index);
                let promoted_hover = if let Some(path) = cached_hover_path {
                    app.musicbrainz_cover_preview_path.clone_from(&path);
                    app.edit_preview_path = path;
                    app.hovered_musicbrainz_result = None;
                    if let Some(item) = app
                        .musicbrainz_results
                        .iter()
                        .find(|item| item.index == selected_index)
                    {
                        app.operation_status = format!(
                            "Selected MusicBrainz release · {} — {} · cover ready",
                            item.release_artist, item.release_title
                        );
                        app.operation_tone = "green".to_string();
                    }
                    true
                } else {
                    false
                };
                load_cover = !promoted_hover
                    && !hover_loading
                    && !app.musicbrainz_missing_cover.contains(&selected_index)
                    && app.musicbrainz_cover_preview_path.trim().is_empty()
                    && app
                    .musicbrainz_results
                    .iter()
                    .find(|item| item.index == selected_index)
                    .is_some_and(|item| !item.current);
                if !promoted_hover {
                    // Never leave the prior embedded/local image in view while
                    // a different MusicBrainz cover is loading from its URL.
                    // A hover preview for this exact row is already the chosen
                    // cover, so preserve it instead of flashing the pane empty.
                    app.edit_preview_path.clear();
                    app.musicbrainz_cover_preview_path.clear();
                    if app.musicbrainz_missing_cover.contains(&selected_index) {
                        app.operation_status = "Selected MusicBrainz release has no direct cover · Generate Candidate Artwork searches its artist and album title".to_string();
                        app.operation_tone = "orange".to_string();
                    }
                }
            }
            if let Some(log) = UI_DEBUG_LOG_HANDLE.get() {
                log.record(format!(
                    "musicbrainz-selection index={selected_index} accepted={selected} load_cover={load_cover}"
                ));
            }
            refresh_weak(&weak, &state);
            if load_cover {
                refresh_musicbrainz_cover_preview(
                    Arc::clone(&state),
                    weak.clone(),
                    selected_index,
                    false,
                );
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let hover_exit_debounce = Rc::new(Timer::default());
        let cached_hover_debounce = Rc::new(Timer::default());
        ui.on_musicbrainz_hovered(move |index, entered| {
            let selected_index = index.max(0) as usize;
            let mut load_cover = false;
            let mut cached_hover = false;
            let mut refresh_after_exit = false;
            if let Ok(mut app) = state.lock() {
                if entered {
                    hover_exit_debounce.stop();
                    cached_hover_debounce.stop();
                    app.hovered_musicbrainz_result = Some(selected_index);
                    let cached_path = app
                        .musicbrainz_preview_cache
                        .get(&selected_index)
                        .cloned()
                        .or_else(|| {
                            (app.musicbrainz_hover_preview_index == Some(selected_index))
                                .then(|| app.musicbrainz_hover_preview_path.trim().to_string())
                                .filter(|path| !path.is_empty())
                        });
                    if let Some(path) = cached_path {
                        app.musicbrainz_hover_preview_path = path;
                        app.musicbrainz_hover_preview_index = Some(selected_index);
                        cached_hover = true;
                    }
                    let loading = app.musicbrainz_preview_inflight.contains(&selected_index);
                    load_cover = !cached_hover
                        && !loading
                        && app
                            .musicbrainz_results
                            .iter()
                            .any(|item| item.index == selected_index && !item.current);
                } else if app.hovered_musicbrainz_result == Some(selected_index) {
                    app.hovered_musicbrainz_result = None;
                    refresh_after_exit = true;
                }
            }
            // Do not rebuild the results model on hover entry. Replacing the
            // TouchArea beneath a pressed pointer prevents its click from
            // completing and previously launched a storm of duplicate cover
            // requests. The downloaded preview performs the required sync.
            if cached_hover {
                let hover_state = Arc::clone(&state);
                let hover_weak = weak.clone();
                cached_hover_debounce.start(
                    TimerMode::SingleShot,
                    Duration::from_millis(90),
                    move || {
                        let still_hovered = hover_state.lock().ok().is_some_and(|app| {
                            app.hovered_musicbrainz_result == Some(selected_index)
                        });
                        if still_hovered {
                            refresh_weak(&hover_weak, &hover_state);
                        }
                    },
                );
            }
            if refresh_after_exit {
                let exit_state = Arc::clone(&state);
                let exit_weak = weak.clone();
                hover_exit_debounce.start(
                    TimerMode::SingleShot,
                    Duration::from_millis(90),
                    move || {
                        let still_outside = exit_state
                            .lock()
                            .ok()
                            .is_some_and(|app| app.hovered_musicbrainz_result.is_none());
                        if still_outside {
                            refresh_weak(&exit_weak, &exit_state);
                        }
                    },
                );
            }
            if load_cover {
                refresh_musicbrainz_cover_preview(
                    Arc::clone(&state),
                    weak.clone(),
                    selected_index,
                    true,
                );
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_musicbrainz_use_existing(move || {
            if let Ok(mut app) = state.lock() {
                let index = app.selected_musicbrainz_existing.unwrap_or(0);
                app.select_musicbrainz_existing(index);
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_musicbrainz_existing_selected(move |index| {
            if let Ok(mut app) = state.lock() {
                app.select_musicbrainz_existing(index as usize);
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_musicbrainz_existing_hovered(move |index, hovered| {
            if let Ok(mut app) = state.lock() {
                if hovered {
                    app.hovered_musicbrainz_existing = Some(index as usize);
                } else if app.hovered_musicbrainz_existing == Some(index as usize) {
                    app.hovered_musicbrainz_existing = None;
                }
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let controller = Arc::clone(&controller);
        ui.on_musicbrainz_use_selected(move || {
            let decision = state
                .lock()
                .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                .and_then(|mut app| app.selected_musicbrainz_result_decision());
            let result = decision.and_then(|decision| controller.submit_decision(decision));
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let controller = Arc::clone(&controller);
        ui.on_musicbrainz_apply_authority(move |recording, artists| {
            let decision = state
                .lock()
                .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                .and_then(|mut app| {
                    // Keep the operator's rejected values in the editor. The
                    // asynchronous validator will replace them only after a
                    // successful MusicBrainz response.
                    app.musicbrainz_authority.recording_mbid = recording.to_string();
                    app.musicbrainz_authority.artist_mbids = artists.to_string();
                    app.musicbrainz_recording_status = "pending".to_string();
                    app.musicbrainz_artist_status = "pending".to_string();
                    app.musicbrainz_authority_decision_with_ids(
                        recording.as_str(),
                        artists.as_str(),
                        "",
                    )
                });
            let result = decision.and_then(|decision| controller.submit_decision(decision));
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let revision = Arc::clone(&adjustment_preview_revision);
        let gate = Arc::clone(&adjustment_preview_gate);
        ui.on_adjustment_step(move |name, delta| {
            if let Some(ui) = weak.upgrade() { ui.set_artist_preview_reset_active(false); }
            if let Ok(mut app) = state.lock() {
                if name == "Tint" && weak.upgrade().is_some_and(|ui| ui.get_page_index() == 1) {
                    app.tint_palette_source.clear();
                }
                if name == "Tint" && weak.upgrade().is_some_and(|ui| ui.get_page_index() == 0) {
                    ensure_active_tint_palette(&mut app);
                }
                step_adjustment(&mut app, name.as_str(), delta);
                app.upscale_preview_only = true;
                clear_rendered_preview_for_adjustment(&mut app);
            }
            revision.fetch_add(1, Ordering::SeqCst);
            if let Some(ui) = weak.upgrade()
                && let Ok(app) = state.lock()
            {
                set_settings_adjustment_ui(&ui, &app, true);
            }
            refresh_weak(&weak, &state);
            if weak.upgrade().is_some_and(|ui| ui.get_page_index() == 1) {
                schedule_settings_adjustment_preview(
                    Arc::clone(&state),
                    weak.clone(),
                    Arc::clone(&revision),
                    Arc::clone(&gate),
                );
            } else {
                schedule_adjustment_preview(
                    Arc::clone(&state),
                    weak.clone(),
                    Arc::clone(&revision),
                    Arc::clone(&gate),
                );
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let revision = Arc::clone(&adjustment_preview_revision);
        let gate = Arc::clone(&adjustment_preview_gate);
        ui.on_filter_intensity_step(move |name, delta| {
            let result = state.lock().ok().and_then(|mut app| {
                let index = FILTER_MENU_NAMES.iter().position(|label| *label == name.as_str())?;
                let target = active_filter_levels(&app)[index] + delta;
                change_filter_level(&mut app, name.as_str(), target)
                    .then(|| app.adjustments.slider_values())
            });
            if let Some(values) = result {
                apply_adjustment_values(&state, &weak, &revision, &gate, values, None);
            } else { refresh_weak(&weak, &state); }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let revision = Arc::clone(&adjustment_preview_revision);
        let gate = Arc::clone(&adjustment_preview_gate);
        ui.on_filter_intensity_set(move |name, position| {
            let target = if position.is_finite() { (-5.0 + position.clamp(0.0, 1.0) * 10.0).round() as i32 } else { 0 };
            let result = state.lock().ok().and_then(|mut app| {
                change_filter_level(&mut app, name.as_str(), target)
                    .then(|| app.adjustments.slider_values())
            });
            if let Some(values) = result {
                apply_adjustment_values(&state, &weak, &revision, &gate, values, None);
            } else { refresh_weak(&weak, &state); }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let revision = Arc::clone(&adjustment_preview_revision);
        let gate = Arc::clone(&adjustment_preview_gate);
        ui.on_activate_detailed_adjustments(move || {
            let values = state.lock().ok().map(|mut app| {
                ensure_active_tint_palette(&mut app);
                let saved = AdjustmentState::from_config(&app.config).slider_values();
                for (name, amount) in [("Hue", 5), ("Tint", 5), ("Gamma", 2), ("Picture", 2), ("Exposure", 2), ("Brightness", 2)] {
                    let index = ["Picture", "Sharpen", "Softness", "Contrast", "Exposure", "Brightness", "Gamma", "Color", "Hue", "Darkness", "Tint"]
                        .iter().position(|candidate| *candidate == name).unwrap_or_default();
                    if app.adjustments.slider_values()[index] == saved[index] { step_adjustment(&mut app, name, amount); }
                }
                app.adjustments.slider_values()
            });
            if let Some(values) = values { apply_adjustment_values(&state, &weak, &revision, &gate, values, None); }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let revision = Arc::clone(&adjustment_preview_revision);
        ui.on_reset_enhancement_preview(move || {
            if let Ok(mut app) = state.lock() { reset_enhancement_preview_state(&mut app); }
            revision.fetch_add(1, Ordering::SeqCst);
            refresh_weak(&weak, &state);
            let weak_reset = weak.clone();
            slint::Timer::single_shot(std::time::Duration::from_millis(550), move || {
                if let Some(ui) = weak_reset.upgrade() { ui.set_enhancement_reset_active(false); }
            });
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let revision = Arc::clone(&adjustment_preview_revision);
        let gate = Arc::clone(&adjustment_preview_gate);
        ui.on_adjustment_set(move |name, position| {
            if let Some(ui) = weak.upgrade() { ui.set_artist_preview_reset_active(false); }
            if let Ok(mut app) = state.lock() {
                if name == "Tint" && weak.upgrade().is_some_and(|ui| ui.get_page_index() == 1) {
                    app.tint_palette_source.clear();
                }
                if name == "Tint" && weak.upgrade().is_some_and(|ui| ui.get_page_index() == 0) {
                    ensure_active_tint_palette(&mut app);
                }
                set_adjustment_position(&mut app, name.as_str(), position);
                app.upscale_preview_only = true;
                clear_rendered_preview_for_adjustment(&mut app);
            }
            revision.fetch_add(1, Ordering::SeqCst);
            if let Some(ui) = weak.upgrade()
                && let Ok(app) = state.lock()
            {
                set_settings_adjustment_ui(&ui, &app, true);
            }
            refresh_weak(&weak, &state);
            if weak.upgrade().is_some_and(|ui| ui.get_page_index() == 1) {
                schedule_settings_adjustment_preview(
                    Arc::clone(&state),
                    weak.clone(),
                    Arc::clone(&revision),
                    Arc::clone(&gate),
                );
            } else {
                schedule_adjustment_preview(
                    Arc::clone(&state),
                    weak.clone(),
                    Arc::clone(&revision),
                    Arc::clone(&gate),
                );
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_adjustment_relative_set(move |name, position| {
            let absolute = state.lock().ok().map(|app| {
                relative_adjustment_position(&app, name.as_str(), position)
            });
            if let (Some(ui), Some(absolute)) = (weak.upgrade(), absolute) {
                ui.invoke_adjustment_set(name, absolute);
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let revision = Arc::clone(&adjustment_preview_revision);
        let gate = Arc::clone(&adjustment_preview_gate);
        ui.on_tint_color_changed(move |color| {
            let selected = color.trim();
            if selected.len() != 7
                || !selected.starts_with('#')
                || !selected[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return;
            }
            if let Ok(mut app) = state.lock() {
                app.tint_palette_source.clear();
                app.adjustments.tint_color = selected.to_ascii_uppercase();
                app.upscale_preview_only = true;
                clear_rendered_preview_for_adjustment(&mut app);
            }
            revision.fetch_add(1, Ordering::SeqCst);
            if let Some(ui) = weak.upgrade()
                && let Ok(app) = state.lock()
            {
                set_settings_adjustment_ui(&ui, &app, true);
            }
            refresh_weak(&weak, &state);
            if weak.upgrade().is_some_and(|ui| ui.get_page_index() == 1) {
                schedule_settings_adjustment_preview(
                    Arc::clone(&state),
                    weak.clone(),
                    Arc::clone(&revision),
                    Arc::clone(&gate),
                );
            } else {
                schedule_adjustment_preview(
                    Arc::clone(&state),
                    weak.clone(),
                    Arc::clone(&revision),
                    Arc::clone(&gate),
                );
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let revision = Arc::clone(&adjustment_preview_revision);
        let gate = Arc::clone(&adjustment_preview_gate);
        ui.on_adjustment_reset(move |name| {
            if let Ok(mut app) = state.lock() {
                reset_adjustment(&mut app, name.as_str());
                app.upscale_preview_only = true;
                clear_rendered_preview_for_adjustment(&mut app);
            }
            revision.fetch_add(1, Ordering::SeqCst);
            if let Some(ui) = weak.upgrade()
                && let Ok(app) = state.lock()
            {
                set_settings_adjustment_ui(&ui, &app, true);
            }
            refresh_weak(&weak, &state);
            if weak.upgrade().is_some_and(|ui| ui.get_page_index() == 1) {
                schedule_settings_adjustment_preview(
                    Arc::clone(&state),
                    weak.clone(),
                    Arc::clone(&revision),
                    Arc::clone(&gate),
                );
            } else {
                schedule_adjustment_preview(
                    Arc::clone(&state),
                    weak.clone(),
                    Arc::clone(&revision),
                    Arc::clone(&gate),
                );
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_primary_adjustment_move(move |name, direction| {
            if let Ok(mut app) = state.lock() {
                app.move_primary_adjustment(name.as_str(), direction as isize);
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_primary_adjustment_promote(move |name| {
            if let Ok(mut app) = state.lock() {
                app.promote_primary_adjustment(name.as_str());
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let revision = Arc::clone(&adjustment_preview_revision);
        let gate = Arc::clone(&adjustment_preview_gate);
        ui.on_activate_all_adjustments(move || {
            if let Ok(mut app) = state.lock() {
                app.activate_all_adjustments();
                app.upscale_preview_only = true;
                clear_rendered_preview_for_adjustment(&mut app);
            }
            revision.fetch_add(1, Ordering::SeqCst);
            if let Some(ui) = weak.upgrade()
                && let Ok(app) = state.lock()
            {
                set_settings_adjustment_ui(&ui, &app, true);
            }
            refresh_weak(&weak, &state);
            if weak.upgrade().is_some_and(|ui| ui.get_page_index() == 1) {
                schedule_settings_adjustment_preview(
                    Arc::clone(&state),
                    weak.clone(),
                    Arc::clone(&revision),
                    Arc::clone(&gate),
                );
            } else {
                schedule_adjustment_preview(
                    Arc::clone(&state),
                    weak.clone(),
                    Arc::clone(&revision),
                    Arc::clone(&gate),
                );
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let revision = Arc::clone(&adjustment_preview_revision);
        let gate = Arc::clone(&adjustment_preview_gate);
        ui.on_apply_enhancement_defaults(move || {
            if let Ok(mut app) = state.lock() {
                app.adjustments.picture_percent = 2;
                app.adjustments.sharpen_percent = 4;
                app.adjustments.softness_percent = 0;
                app.adjustments.contrast_percent = 10;
                app.adjustments.exposure_percent = -5;
                app.adjustments.brightness_percent = -1;
                app.adjustments.gamma_percent = -6;
                app.adjustments.color_temperature = 0;
                app.adjustments.hue_degrees = 0;
                app.adjustments.darkness_percent = 0;
                app.adjustments.tint_percent = 0;
                app.adjustments.tint_color = "#FFB36B".to_string();
                app.upscale_preview_only = true;
                clear_rendered_preview_for_adjustment(&mut app);
                app.push_log("Restored the suggested enhancement defaults.".to_string());
            }
            if let Some(ui) = weak.upgrade()
                && let Ok(app) = state.lock()
            {
                set_settings_adjustment_ui(&ui, &app, true);
            }
            refresh_weak(&weak, &state);
            if weak.upgrade().is_some_and(|ui| ui.get_page_index() == 1) {
                schedule_settings_adjustment_preview(
                    Arc::clone(&state),
                    weak.clone(),
                    Arc::clone(&revision),
                    Arc::clone(&gate),
                );
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let revision = Arc::clone(&adjustment_preview_revision);
        let gate = Arc::clone(&adjustment_preview_gate);
        ui.on_apply_built_in_filter(move |index| {
            let increment = match index {
                0 => [4, 12, 0, 8, 0, 0, 0, 0, 0, 0, 0],
                1 => [2, 0, 12, -3, 2, 3, 0, 0, 0, 0, 0],
                2 => [2, 3, 0, 2, 0, 1, 0, 12, 0, 0, 0],
                3 => [6, 8, 0, 12, 2, 3, 0, 4, 0, 0, 0],
                4 => [3, 5, 4, 3, 8, 12, 5, 3, 0, 0, 0],
                _ => return,
            };
            let values = state.lock().ok().map_or(increment, |app| {
                stack_adjustment_values(app.adjustments.slider_values(), increment)
            });
            apply_adjustment_values(&state, &weak, &revision, &gate, values, None);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_adjustment_preset_pressed(move |index| {
            let Ok(mut app) = state.lock() else {
                return;
            };
            let Some(preset) = app.adjustment_presets.get(index as usize).cloned() else {
                return;
            };
            if preset.saved {
                drop(app);
                if let Some(ui) = weak.upgrade() {
                    ui.set_preset_confirm_index(index);
                }
            } else {
                let values = app.adjustments.slider_values();
                let tint_color = app.adjustments.tint_color.clone();
                if let Some(preset) = app.adjustment_presets.get_mut(index as usize) {
                    preset.values = values;
                    preset.tint_color = tint_color;
                    preset.saved = true;
                    app.enhancement_settings_dirty = true;
                    app.service_status =
                        "Preset captured. Save Enhancement Settings to keep it.".to_string();
                }
                drop(app);
                refresh_weak(&weak, &state);
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let revision = Arc::clone(&adjustment_preview_revision);
        let gate = Arc::clone(&adjustment_preview_gate);
        ui.on_adjustment_preset_confirmed(move |index, action| {
            if action.as_str() == "overwrite" {
                if let Ok(mut app) = state.lock() {
                    let values = app.adjustments.slider_values();
                    let tint_color = app.adjustments.tint_color.clone();
                    if let Some(preset) = app.adjustment_presets.get_mut(index as usize) {
                        preset.values = values;
                        preset.tint_color = tint_color;
                        preset.saved = true;
                        app.enhancement_settings_dirty = true;
                        app.service_status =
                            "Preset overwritten. Save Enhancement Settings to keep it.".to_string();
                    }
                }
                refresh_weak(&weak, &state);
            } else if action.as_str() == "apply" {
                let values = state.lock().ok().and_then(|app| {
                    app.adjustment_presets
                        .get(index as usize)
                        .filter(|preset| preset.saved)
                        .map(|preset| (preset.values, preset.tint_color.clone()))
                });
                if let Some((values, tint_color)) = values {
                    let values = if index > 0 {
                        state.lock().ok().map_or(values, |app| {
                            stack_adjustment_values(app.adjustments.slider_values(), values)
                        })
                    } else {
                        values
                    };
                    apply_adjustment_values(
                        &state,
                        &weak,
                        &revision,
                        &gate,
                        values,
                        Some(tint_color),
                    );
                }
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_adjustment_preset_name_changed(move |index, name| {
            let trimmed = name.trim().chars().take(18).collect::<String>();
            if trimmed.is_empty() {
                return;
            }
            if let Ok(mut app) = state.lock() {
                if let Some(preset) = app.adjustment_presets.get_mut(index as usize) {
                    preset.name = trimmed;
                    app.enhancement_settings_dirty = true;
                }
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_adjustment_preset_save(move |index| {
            if let Ok(mut app) = state.lock() {
                let values = app.adjustments.slider_values();
                let tint_color = app.adjustments.tint_color.clone();
                if let Some(preset) = app.adjustment_presets.get_mut(index as usize) {
                    preset.values = values;
                    preset.tint_color = tint_color;
                    preset.saved = true;
                    app.enhancement_settings_dirty = true;
                }
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let revision = Arc::clone(&adjustment_preview_revision);
        let gate = Arc::clone(&adjustment_preview_gate);
        ui.on_adjustment_preset_apply(move |index| {
            let values = state.lock().ok().and_then(|app| {
                app.adjustment_presets
                    .get(index as usize)
                    .filter(|preset| preset.saved)
                    .map(|preset| (preset.values, preset.tint_color.clone()))
            });
            if let Some((values, tint_color)) = values {
                apply_adjustment_values(&state, &weak, &revision, &gate, values, Some(tint_color));
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let revision = Arc::clone(&adjustment_preview_revision);
        let gate = Arc::clone(&adjustment_preview_gate);
        ui.on_settings_preset_select(move |index| {
            let values = state.lock().ok().and_then(|app| {
                app.adjustment_presets
                    .get(index as usize)
                    .filter(|preset| preset.saved)
                    .map(|preset| (preset.values, preset.tint_color.clone()))
            });
            if let Some((values, tint_color)) = values {
                apply_adjustment_values(&state, &weak, &revision, &gate, values, Some(tint_color));
            }
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_banner_theme_sync_changed(move |enabled| {
            if let Ok(mut app) = state.lock()
                && app.banner_sync_frame_letters != enabled
            {
                app.banner_sync_frame_letters = enabled;
                app.enhancement_settings_dirty = true;
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_banner_theme_random_changed(move |enabled| {
            if let Ok(mut app) = state.lock()
                && app.banner_random_harmony != enabled
            {
                app.banner_random_harmony = enabled;
                app.enhancement_settings_dirty = true;
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_theme_color_target_changed(move |target| {
            if let Some(ui) = weak.upgrade() {
                ui.set_theme_color_target(target);
                ui.set_theme_wheel_marker_visible(false);
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let last_drag = Rc::new(Cell::new(Instant::now() - Duration::from_millis(50)));
        ui.on_theme_color_picked(move |x, y, scroll, dragging| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            if dragging && last_drag.get().elapsed() < Duration::from_millis(35) {
                return;
            }
            if dragging {
                last_drag.set(Instant::now());
            }
            if scroll.abs() <= f32::EPSILON {
                if !wheel_contains(x, y) {
                    return;
                }
                ui.set_theme_wheel_marker_x(x);
                ui.set_theme_wheel_marker_y(y);
                ui.set_theme_wheel_marker_visible(true);
            }
            let target = ui.get_theme_color_target().to_string();
            if let Ok(mut app) = state.lock() {
                let current = theme_target_hex(&app, &target)
                    .unwrap_or("#3478FF")
                    .to_string();
                let selected = wheel_rgb(
                    x,
                    y,
                    scroll,
                    &current,
                    app.wheel_hue_shift_degrees,
                    app.wheel_brightness_percent,
                );
                let seed = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_nanos() as u64)
                    .unwrap_or(1);
                apply_theme_selected_color(&mut app, &target, selected, seed);
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let last_drag = Rc::new(Cell::new(Instant::now() - Duration::from_millis(80)));
        ui.on_theme_wheel_range_changed(move |range, position, dragging| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            if dragging && last_drag.get().elapsed() < Duration::from_millis(55) {
                return;
            }
            if dragging {
                last_drag.set(Instant::now());
            }
            let position = position.clamp(0.0, 1.0);
            let target = ui.get_theme_color_target().to_string();
            if let Ok(mut app) = state.lock() {
                match range.as_str() {
                    "hue" => app.wheel_hue_shift_degrees = (position * 359.0).round() as u16,
                    "brightness" => {
                        app.wheel_brightness_percent = (30.0 + position * 140.0).round() as u8
                    }
                    _ => return,
                }
                app.enhancement_settings_dirty = true;
                if ui.get_theme_wheel_marker_visible() {
                    let x = ui.get_theme_wheel_marker_x();
                    let y = ui.get_theme_wheel_marker_y();
                    let current = theme_target_hex(&app, &target).unwrap_or("#3478FF");
                    let selected = wheel_rgb(
                        x,
                        y,
                        0.0,
                        current,
                        app.wheel_hue_shift_degrees,
                        app.wheel_brightness_percent,
                    );
                    let seed = u64::from(x.to_bits()) << 32 | u64::from(y.to_bits());
                    apply_theme_selected_color(&mut app, &target, selected, seed);
                }
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_banner_theme_name_changed(move |key, name| {
            let trimmed = name.trim().chars().take(10).collect::<String>();
            if trimmed.is_empty() {
                return;
            }
            if let Ok(mut app) = state.lock() {
                if let Some(style) = app
                    .banner_theme_styles
                    .iter_mut()
                    .find(|style| style.key == key.as_str())
                {
                    style.name = trimmed;
                    app.enhancement_settings_dirty = true;
                }
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_theme_style_reset(move |target| {
            if let Ok(mut app) = state.lock() {
                let target = target.as_str();
                let restored = if let Some(key) = target.strip_prefix("frame.") {
                    app.album_frame_theme_baseline_styles
                        .iter()
                        .find(|style| style.key == key)
                        .cloned()
                        .and_then(|baseline| {
                            app.album_frame_theme_styles
                                .iter_mut()
                                .find(|style| style.key == key)
                                .map(|current| *current = baseline)
                        })
                        .is_some()
                } else {
                    app.banner_theme_baseline_styles
                        .iter()
                        .find(|style| style.key == target)
                        .cloned()
                        .and_then(|baseline| {
                            app.banner_theme_styles
                                .iter_mut()
                                .find(|style| style.key == target)
                                .map(|current| *current = baseline)
                        })
                        .is_some()
                };
                if restored {
                    app.enhancement_settings_dirty = true;
                    app.service_status =
                        "Baseline restored for this row. Save Enhancement Settings to keep it."
                            .to_string();
                }
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_completion_acknowledged(move || {
            if let Ok(mut app) = state.lock() {
                app.completion_dialog_visible = false;
                app.completion_message.clear();
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_apply_selected_sample(move || {
            if let Ok(mut app) = state.lock() {
                if app.operation_busy {
                    return;
                }
                app.operation_busy = true;
                app.operation_status = "Saving the selected sample to cover.*…".to_string();
                app.operation_tone = "magenta".to_string();
            }
            refresh_weak(&weak, &state);
            let state = Arc::clone(&state);
            let weak = weak.clone();
            std::thread::spawn(move || {
                let result = apply_selected_sample(&state);
                let succeeded = result.is_ok();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Ok(mut app) = state.lock() {
                        app.operation_busy = false;
                        app.operation_status = if succeeded {
                            "Saved selected sample to cover.*".to_string()
                        } else {
                            "Unable to save selected sample".to_string()
                        };
                        app.operation_tone = if succeeded {
                            "green".to_string()
                        } else {
                            "red".to_string()
                        };
                    }
                    record_action_result(&state, result);
                    refresh_weak(&weak, &state);
                });
            });
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_browse_setting_directory(move |target| {
            let target = target.to_string();
            let current = weak
                .upgrade()
                .map(|ui| match target.as_str() {
                    "music" => ui.get_settings_music_library().to_string(),
                    "scan" => ui.get_settings_scan_library().to_string(),
                    "database" => ui.get_settings_database_dir().to_string(),
                    "artist-art" => ui.get_settings_shared_artist_art_dir().to_string(),
                    "temporary" => ui.get_settings_temporary_cache_dir().to_string(),
                    "logs" => ui.get_settings_log_dir().to_string(),
                    "credentials" => ui.get_settings_credential_dir().to_string(),
                    _ => String::new(),
                })
                .unwrap_or_default();
            let description = match target.as_str() {
                "music" => "Choose the existing SPLINED Media Library",
                "scan" => "Choose the Media Library scan folder",
                "database" => "Choose the SPLINED database directory",
                "artist-art" => "Choose the V/A / OST Artist images database directory",
                "temporary" => "Choose the SPLINED temporary cache directory",
                "logs" => "Choose the SPLINED log directory",
                "credentials" => "Choose the SPLINED provider credential directory",
                _ => "Choose a SPLINED directory",
            }
            .to_string();
            let weak = weak.clone();
            std::thread::spawn(move || {
                let selected = browse_for_folder(&current, &description);
                let _ = slint::invoke_from_event_loop(move || {
                    let (Some(path), Some(ui)) = (selected, weak.upgrade()) else {
                        return;
                    };
                    match target.as_str() {
                        "music" => ui.set_settings_music_library(path.into()),
                        "scan" => ui.set_settings_scan_library(path.into()),
                        "database" => ui.set_settings_database_dir(path.into()),
                        "artist-art" => ui.set_settings_shared_artist_art_dir(path.into()),
                        "temporary" => ui.set_settings_temporary_cache_dir(path.into()),
                        "logs" => ui.set_settings_log_dir(path.into()),
                        "credentials" => ui.set_settings_credential_dir(path.into()),
                        _ => {}
                    }
                });
            });
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_browse_sample_directory(move || {
            let current = weak
                .upgrade()
                .map(|ui| ui.get_settings_sample_dir().to_string())
                .unwrap_or_default();
            let weak = weak.clone();
            std::thread::spawn(move || {
                let selected =
                    browse_for_folder(&current, "Choose the SPLINED sample images directory");
                let _ = slint::invoke_from_event_loop(move || {
                    if let (Some(path), Some(ui)) = (selected, weak.upgrade()) {
                        ui.set_settings_sample_dir(path.into());
                    }
                });
            });
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_browse_backup_directory(move || {
            let current = weak
                .upgrade()
                .map(|ui| ui.get_backup_directory().to_string())
                .unwrap_or_default();
            let weak = weak.clone();
            std::thread::spawn(move || {
                let selected = browse_for_folder(&current, "Choose the SPLINED backup directory");
                let _ = slint::invoke_from_event_loop(move || {
                    if let (Some(path), Some(ui)) = (selected, weak.upgrade()) {
                        ui.set_backup_directory(path.into());
                        refresh_archived_backups_ui(&ui, true);
                    }
                });
            });
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_refresh_archived_backups(move || {
            if let Some(ui) = weak.upgrade() {
                refresh_archived_backups_ui(&ui, true);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_browse_restore_file(move || {
            let current = weak
                .upgrade()
                .map(|ui| ui.get_restore_path().to_string())
                .unwrap_or_default();
            let weak = weak.clone();
            std::thread::spawn(move || {
                let selected = browse_for_backup_file(&current, false);
                let _ = slint::invoke_from_event_loop(move || {
                    if let (Some(path), Some(ui)) = (selected, weak.upgrade()) {
                        ui.set_restore_path(path.into());
                    }
                });
            });
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_browse_setup_settings_file(move || {
            let current = weak
                .upgrade()
                .map(|ui| ui.get_restore_path().to_string())
                .unwrap_or_default();
            let weak = weak.clone();
            std::thread::spawn(move || {
                let selected = browse_for_backup_file(&current, true);
                let _ = slint::invoke_from_event_loop(move || {
                    if let (Some(path), Some(ui)) = (selected, weak.upgrade()) {
                        ui.set_restore_path(path.into());
                        ui.invoke_load_setup_settings_file();
                    }
                });
            });
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_edit_existing_cover(move |apply| {
            if let Ok(mut app) = state.lock() {
                if app.operation_busy {
                    return;
                }
                app.operation_busy = true;
                app.operation_status = if apply {
                    "Saving full-resolution cover.*…".to_string()
                } else {
                    "Rendering Upscale Preview in memory…".to_string()
                };
                app.operation_tone = if apply {
                    "magenta".to_string()
                } else {
                    "violet".to_string()
                };
            }
            refresh_weak(&weak, &state);
            let state = Arc::clone(&state);
            let weak = weak.clone();
            std::thread::spawn(move || {
                let result = edit_existing_cover(&state, apply, None);
                let succeeded = result.is_ok();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Ok(mut app) = state.lock() {
                        app.operation_busy = false;
                        app.operation_status = if succeeded {
                            if apply {
                                "Cover saved to cover.* · EMBED Batch is available".to_string()
                            } else {
                                "Upscale Preview ready".to_string()
                            }
                        } else if apply {
                            "Unable to save Upscale".to_string()
                        } else {
                            "Upscale Preview failed".to_string()
                        };
                        app.operation_tone = if succeeded {
                            "green".to_string()
                        } else {
                            "red".to_string()
                        };
                    }
                    record_action_result(&state, result);
                    refresh_weak(&weak, &state);
                });
            });
        });
    }
    bind_service_callbacks(
        ui,
        state,
        workspace_layout,
        appearance,
        custom_window_state,
        interface_restore_pending_restart,
    );
}

fn bind_service_callbacks(
    ui: &AppWindow,
    state: Arc<Mutex<ApplicationState>>,
    workspace_layout: Rc<RefCell<WorkspaceLayout>>,
    appearance: Rc<RefCell<String>>,
    custom_window_state: CustomWindowState,
    interface_restore_pending_restart: Arc<AtomicBool>,
) {
    let settings_draft = Arc::new(Mutex::new(
        state
            .lock()
            .map(|app| app.config.clone())
            .unwrap_or_default(),
    ));
    {
        let weak = ui.as_weak();
        let state = Arc::clone(&state);
        let settings_draft = Arc::clone(&settings_draft);
        ui.on_load_settings(move || {
            if let Some(ui) = weak.upgrade() {
                match state.lock() {
                    Ok(app) => {
                        if let Ok(mut draft) = settings_draft.lock() {
                            *draft = app.config.clone();
                        }
                        set_settings_ui(&ui, &app.config);
                        ui.set_settings_animate_webp_preview(app.animate_webp_preview);
                        ui.set_settings_source_rows(ui.get_source_rows());
                        ui.set_settings_status_bar_intensity(ui.get_status_bar_intensity());
                        ui.set_settings_status_bar_position(ui.get_status_bar_position());
                        ui.set_settings_status_bar_size(ui.get_status_bar_size());
                    }
                    Err(_) => record_action_result(
                        &state,
                        Err("The SPLINED application state is unavailable.".to_string()),
                    ),
                }
            }
        });
    }
    {
        let weak = ui.as_weak();
        let state = Arc::clone(&state);
        let settings_draft = Arc::clone(&settings_draft);
        ui.on_load_source_policy(move |source| {
            let result = settings_draft
                .lock()
                .map_err(|_| "The structured Settings draft is unavailable.".to_string())
                .map(|draft| draft.clone());
            if let (Ok(draft), Some(ui)) = (result, weak.upgrade()) {
                set_source_policy_ui(&ui, &draft, source.as_str());
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let weak = ui.as_weak();
        let state = Arc::clone(&state);
        let settings_draft = Arc::clone(&settings_draft);
        ui.on_update_source_policy(move || {
            let result = weak
                .upgrade()
                .ok_or_else(|| "The SPLINED Settings window is unavailable.".to_string())
                .and_then(|ui| {
                    let mut draft = settings_draft
                        .lock()
                        .map_err(|_| "The structured Settings draft is unavailable.".to_string())?;
                    draft.sources.cover_sources =
                        split_values(ui.get_settings_source_order().as_str());
                    draft.sources.exclude_cover_sources =
                        split_values(ui.get_settings_source_exclusions().as_str());
                    update_source_policy_from_ui(&ui, &mut draft)?;
                    set_settings_source_order_and_cards(&ui, &draft.sources.cover_sources);
                    ui.set_settings_source_exclusions(
                        draft.sources.exclude_cover_sources.join(", ").into(),
                    );
                    Ok(())
                });
            if result.is_ok()
                && let Ok(mut app) = state.lock()
            {
                app.service_status =
                    "Source policy updated in the Settings draft. Save to apply.".to_string();
            }
            let failed = result.is_err();
            record_action_result(&state, result);
            // Keep the edited source order visible in Settings until Save.
            // A full refresh here rehydrates fields from the last saved
            // config and hides the draft change made above.
            if failed {
                refresh_weak(&weak, &state);
            }
        });
    }
    {
        let weak = ui.as_weak();
        let state = Arc::clone(&state);
        let settings_draft = Arc::clone(&settings_draft);
        let restore_pending = Arc::clone(&interface_restore_pending_restart);
        ui.on_save_settings(move || {
            let result = (|| {
                if restore_pending.load(Ordering::SeqCst) {
                    return Err("Restart SPLINED to finish restoring Interface Settings before saving new changes.".to_string());
                }
                let ui = weak
                    .upgrade()
                    .ok_or_else(|| "The SPLINED Settings window is unavailable.".to_string())?;
                if ui.get_media_refresh_running() {
                    return Err("Wait for Refresh DB to finish before saving Settings.".to_string());
                }
                {
                    let app = state
                        .lock()
                        .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
                    if app.phase != RunPhase::Idle {
                        return Err("Finish the active run before saving Settings.".to_string());
                    }
                }
                let section = ui.get_settings_section().to_string();
                let mut config = state
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())?
                    .config
                    .clone();
                if section == "Sources" {
                    config.source_policies = settings_draft
                        .lock()
                        .map_err(|_| "The structured Settings draft is unavailable.".to_string())?
                        .source_policies
                        .clone();
                }
                apply_settings_section_ui(&ui, &mut config, &section)?;
                let config = splined_core::settings::save_config(&config)?;
                let snapshot = media_snapshot(&config, false)?;
                let diagnostics = startup_diagnostics(&config, &snapshot);
                let (intro_dismissed, _, _) = state
                    .lock()
                    .map(|app| {
                        (
                            app.intro_dismissed,
                            app.disable_intro_message,
                            app.disable_confirmation_messages,
                        )
                    })
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
                let mut replacement = ApplicationState::new(config.clone(), snapshot)?;
                let prior_preferences = state
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
                replacement.disable_intro_message = if section == "Library" {
                    ui.get_settings_disable_intro_message()
                } else {
                    prior_preferences.disable_intro_message
                };
                replacement.disable_confirmation_messages = if section == "Library" {
                    ui.get_settings_disable_confirmation_messages()
                } else {
                    prior_preferences.disable_confirmation_messages
                };
                replacement.adjustment_presets = prior_preferences.adjustment_presets.clone();
                replacement.banner_theme_styles = prior_preferences.banner_theme_styles.clone();
                replacement.album_frame_theme_styles =
                    prior_preferences.album_frame_theme_styles.clone();
                replacement.banner_sync_frame_letters = prior_preferences.banner_sync_frame_letters;
                replacement.banner_random_harmony = prior_preferences.banner_random_harmony;
                replacement.wheel_hue_shift_degrees = prior_preferences.wheel_hue_shift_degrees;
                replacement.wheel_brightness_percent = prior_preferences.wheel_brightness_percent;
                replacement.banner_theme_baseline_styles =
                    prior_preferences.banner_theme_baseline_styles.clone();
                replacement.album_frame_theme_baseline_styles =
                    prior_preferences.album_frame_theme_baseline_styles.clone();
                replacement.appearance_mode = prior_preferences.appearance_mode.clone();
                preserve_filter_state(&mut replacement, &prior_preferences);
                replacement.animate_webp_preview = if section == "Artwork" {
                    ui.get_settings_animate_webp_preview()
                } else {
                    prior_preferences.animate_webp_preview
                };
                replacement.enhancement_settings_dirty =
                    prior_preferences.enhancement_settings_dirty;
                if section == "Enhancement" && ui.get_settings_preview_preset_index() != 0 {
                    replacement.adjustments = prior_preferences.adjustments.clone();
                }
                drop(prior_preferences);
                if section == "Enhancement" {
                    let mut preferences = splined_core::settings::load_ui_preferences()?;
                    preferences.adjustment_presets = replacement.adjustment_presets.clone();
                    preferences.banner_theme_styles = replacement.banner_theme_styles.clone();
                    preferences.album_frame_theme_styles =
                        replacement.album_frame_theme_styles.clone();
                    preferences.banner_sync_frame_letters = replacement.banner_sync_frame_letters;
                    preferences.banner_random_harmony = replacement.banner_random_harmony;
                    preferences.wheel_hue_shift_degrees = replacement.wheel_hue_shift_degrees;
                    preferences.wheel_brightness_percent = replacement.wheel_brightness_percent;
                    preferences.banner_theme_baseline_styles =
                        replacement.banner_theme_baseline_styles.clone();
                    preferences.album_frame_theme_baseline_styles =
                        replacement.album_frame_theme_baseline_styles.clone();
                    splined_core::settings::save_ui_preferences(&preferences)?;
                    replacement.enhancement_settings_dirty = false;
                }
                if section == "Artwork" {
                    let mut preferences = splined_core::settings::load_ui_preferences()?;
                    preferences.animate_webp_preview = replacement.animate_webp_preview;
                    splined_core::settings::save_ui_preferences(&preferences)?;
                }
                replacement.intro_dismissed = intro_dismissed || replacement.disable_intro_message;
                replacement.push_log(format!("Validated and saved {section} settings."));
                for diagnostic in diagnostics {
                    replacement.push_log(diagnostic);
                }
                *state
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())? =
                    replacement;
                if section == "Sources" {
                    *settings_draft.lock().map_err(|_| {
                        "The structured Settings draft is unavailable.".to_string()
                    })? = config.clone();
                    set_settings_source_order_and_cards(&ui, &config.sources.cover_sources);
                    ui.set_settings_source_exclusions(
                        config.sources.exclude_cover_sources.join(", ").into(),
                    );
                }
                Ok(())
            })();
            let saved_sources = result.is_ok()
                && weak
                    .upgrade()
                    .is_some_and(|ui| ui.get_settings_section() == "Sources");
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
            if saved_sources && let Some(ui) = weak.upgrade() {
                ui.set_settings_source_rows(ui.get_source_rows());
            }
        });
    }
    {
        let weak = ui.as_weak();
        let state = Arc::clone(&state);
        let settings_draft = Arc::clone(&settings_draft);
        let restore_pending = Arc::clone(&interface_restore_pending_restart);
        ui.on_reset_advanced_settings(move || {
            let result = (|| {
                if restore_pending.load(Ordering::SeqCst) {
                    return Err("Restart SPLINED before resetting settings after an Interface Settings restore.".to_string());
                }
                let ui = weak
                    .upgrade()
                    .ok_or_else(|| "The SPLINED Settings window is unavailable.".to_string())?;
                if !ui.get_reset_settings_preview_visible()
                    || !ui.get_reset_settings_confirm_visible()
                    || ui.get_reset_settings_confirmation_text() != "RESET"
                {
                    return Err(
                        "Reset requires the red preview and typed RESET confirmation.".to_string(),
                    );
                }
                let (current, intro_dismissed, disable_intro, disable_confirmation) = {
                    let app = state
                        .lock()
                        .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
                    if app.phase != RunPhase::Idle {
                        return Err("Settings cannot reset during an active run.".to_string());
                    }
                    (
                        app.config.clone(),
                        app.intro_dismissed,
                        app.disable_intro_message,
                        app.disable_confirmation_messages,
                    )
                };
                let reset = safe_reset_config(&current);
                splined_core::settings::validate_operational_config(&reset)?;
                let snapshot = media_snapshot(&reset, false)?;
                let backup = backup_config_before_reset()?;
                let saved = splined_core::settings::save_config(&reset)?;
                let mut replacement = ApplicationState::new(saved.clone(), snapshot)?;
                replacement.intro_dismissed = intro_dismissed;
                replacement.disable_intro_message = disable_intro;
                replacement.disable_confirmation_messages = disable_confirmation;
                replacement.push_log(format!(
                    "Reset Config v5 to safe defaults; previous config: {}",
                    backup.display()
                ));
                *state
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())? =
                    replacement;
                *settings_draft
                    .lock()
                    .map_err(|_| "The structured Settings draft is unavailable.".to_string())? =
                    saved.clone();
                set_settings_ui(&ui, &saved);
                ui.set_reset_settings_preview_visible(false);
                ui.set_reset_settings_confirm_visible(false);
                ui.set_reset_settings_confirmation_text(SharedString::default());
                Ok(())
            })();
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
            if let Some(ui) = weak.upgrade() {
                ui.set_settings_source_rows(ui.get_source_rows());
            }
        });
    }
    {
        let weak = ui.as_weak();
        let state = Arc::clone(&state);
        let settings_draft = Arc::clone(&settings_draft);
        ui.on_choose_import_client_lock(move |enable| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            ui.set_import_client_lock_prompt_visible(false);
            if !enable {
                if let Ok(mut app) = state.lock() {
                    app.service_status = "Extra .client-lock remains off; SQLite locking is still active.".to_string();
                }
                refresh_weak(&weak, &state);
                return;
            }
            if ui.get_setup_required() {
                ui.set_settings_windows_client_lock(true);
                if let Ok(mut draft) = settings_draft.lock() {
                    draft.scan.windows_client_lock = true;
                }
                ui.set_setup_message("Extra .client-lock selected. Review the paths, then Save & Open Settings; you can turn it off here if this PC cannot acquire it.".into());
                return;
            }
            let candidate = match state.lock() {
                Ok(app) => {
                    let mut config = app.config.clone();
                    config.scan.windows_client_lock = true;
                    config
                }
                Err(_) => return,
            };
            if let Ok(mut app) = state.lock() {
                app.service_status = "Checking the extra shared-database lock…".to_string();
            }
            refresh_weak(&weak, &state);
            let weak = weak.clone();
            let state = Arc::clone(&state);
            let settings_draft = Arc::clone(&settings_draft);
            std::thread::spawn(move || {
                let result = media_snapshot(&candidate, false)
                    .and_then(|_| splined_core::settings::save_config(&candidate));
                let _ = slint::invoke_from_event_loop(move || {
                    match result {
                        Ok(saved) => {
                            if let Ok(mut app) = state.lock() {
                                app.config = saved.clone();
                                app.service_status = "Extra .client-lock enabled for this installation.".to_string();
                            }
                            if let Ok(mut draft) = settings_draft.lock() {
                                *draft = saved.clone();
                            }
                            if let Some(ui) = weak.upgrade() {
                                ui.set_settings_windows_client_lock(true);
                            }
                        }
                        Err(error) => {
                            record_action_result(
                                &state,
                                Err(format!("Could not enable extra .client-lock; SPLINED continues with it off: {error}")),
                            );
                        }
                    }
                    refresh_weak(&weak, &state);
                });
            });
        });
    }
    {
        let weak = ui.as_weak();
        let state = Arc::clone(&state);
        let settings_draft = Arc::clone(&settings_draft);
        ui.on_load_setup_settings_file(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            if ui.get_setup_saving() {
                return;
            }
            let path = PathBuf::from(ui.get_restore_path().as_str());
            if !path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|extension| extension.eq_ignore_ascii_case("spl"))
            {
                record_action_result(
                    &state,
                    Err("Choose an existing .spl Settings Archive first.".to_string()),
                );
                refresh_weak(&weak, &state);
                return;
            }
            let password = ui.get_backup_password().to_string();
            ui.set_setup_saving(true);
            ui.set_setup_loaded_archive_path(SharedString::default());
            ui.set_setup_message("Reading Settings Archive…".into());
            let weak = weak.clone();
            let state = Arc::clone(&state);
            let settings_draft = Arc::clone(&settings_draft);
            std::thread::spawn(move || {
                let result = read_backup_settings(&path, &password);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.set_setup_saving(false);
                        match result {
                            Ok(config) => {
                                if let Ok(mut draft) = settings_draft.lock() {
                                    *draft = config.clone();
                                }
                                set_settings_ui(&ui, &config);
                                ui.set_backup_password(SharedString::default());
                                ui.set_setup_loaded_archive_path(SharedString::from(
                                    path.to_string_lossy().into_owned(),
                                ));
                                ui.set_setup_message("Settings loaded from the .spl archive. Review every path and the shared SQLite options, then choose Save & Open Settings. No settings have been saved yet.".into());
                                if let Ok(mut app) = state.lock() {
                                    app.last_error = None;
                                }
                            }
                            Err(error) => {
                                ui.set_setup_message("Settings Archive could not be loaded. Check the file and password, or enter paths below.".into());
                                record_action_result(&state, Err(error));
                            }
                        }
                    }
                    refresh_weak(&weak, &state);
                });
            });
        });
    }
    {
        let weak = ui.as_weak();
        let state = Arc::clone(&state);
        let settings_draft = Arc::clone(&settings_draft);
        ui.on_save_setup(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            if ui.get_setup_saving() {
                return;
            }
            let prepared = (|| {
                let mut config = settings_draft
                    .lock()
                    .map_err(|_| "The structured Settings draft is unavailable.".to_string())?
                    .clone();
                apply_settings_ui(&ui, &mut config)?;
                if config.scan.scan_library_dir.trim().is_empty() {
                    config.scan.scan_library_dir = config.library.music_library.clone();
                }
                Ok::<_, String>(config)
            })();
            let config = match prepared {
                Ok(prepared) => prepared,
                Err(error) => {
                    record_action_result(&state, Err(error));
                    refresh_weak(&weak, &state);
                    return;
                }
            };
            ui.set_setup_saving(true);
            ui.set_setup_message("Saving settings and loading the music library… This may take a while on a new library.".into());
            let weak = weak.clone();
            let state = Arc::clone(&state);
            let settings_draft = Arc::clone(&settings_draft);
            std::thread::spawn(move || {
                let result = (|| {
                    let config = splined_core::settings::save_config(&config)?;
                    let snapshot = media_snapshot(&config, false)?;
                    let diagnostics = startup_diagnostics(&config, &snapshot);
                    let mut replacement = ApplicationState::new(config.clone(), snapshot)?;
                    replacement.push_log("First-run Setup completed.".to_string());
                    for diagnostic in diagnostics {
                        replacement.push_log(diagnostic);
                    }
                    Ok::<_, String>((config, replacement))
                })();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.set_setup_saving(false);
                        match result {
                            Ok((config, replacement)) => {
                                if let Ok(mut app) = state.lock() {
                                    let mut replacement = replacement;
                                    replacement.embed_locked_album_paths =
                                        app.embed_locked_album_paths.clone();
                                    *app = replacement;
                                }
                                if let Ok(mut draft) = settings_draft.lock() {
                                    *draft = config.clone();
                                }
                                ui.set_setup_required(false);
                                ui.set_page_index(1);
                                set_settings_ui(&ui, &config);
                                if let Ok(editor) = load_editor(&config, "Navidrome") {
                                    set_credential_ui(&ui, &editor);
                                }
                            }
                            Err(error) => {
                                ui.set_setup_message("Setup could not finish. Correct the problem below and try Save & Open Settings again.".into());
                                record_action_result(&state, Err(error));
                            }
                        }
                    }
                    refresh_weak(&weak, &state);
                });
            });
        });
    }
    {
        let weak = ui.as_weak();
        let state = Arc::clone(&state);
        ui.on_load_credential(move |provider| {
            let config = state.lock().ok().map(|app| app.config.clone());
            if let (Some(config), Some(ui)) = (config, weak.upgrade()) {
                match load_editor(&config, provider.as_str()) {
                    Ok(editor) => set_credential_ui(&ui, &editor),
                    Err(error) => {
                        ui.set_credential_status(SharedString::from(error.clone()));
                        record_action_result(&state, Err(error));
                    }
                }
            }
        });
    }
    {
        let weak = ui.as_weak();
        let state = Arc::clone(&state);
        ui.on_save_credential(
            move |provider, server_name, server, auth, username, identifier, secret| {
                let editor = credential_from_fields(
                    provider,
                    server_name,
                    server,
                    auth,
                    username,
                    identifier,
                    secret,
                );
                let result = state
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                    .and_then(|app| save_editor(&app.config, &editor));
                let protection_warning =
                    result.is_ok() && take_credential_protection_warning();
                if let Some(ui) = weak.upgrade() {
                    ui.set_credential_status(SharedString::from(if protection_warning {
                        "Saved, but user-specific filesystem protection is unavailable on this storage location.".to_string()
                    } else if result.is_ok() {
                        "Saved securely. Use Test to verify live access.".to_string()
                    } else {
                        result.clone().unwrap_err()
                    }));
                }
                if protection_warning
                    && let Ok(mut app) = state.lock()
                {
                    app.push_log(
                        "WARNING: credential file protection is unavailable on this storage location."
                            .to_string(),
                    );
                }
                record_action_result(&state, result);
                refresh_weak(&weak, &state);
            },
        );
    }
    {
        let weak = ui.as_weak();
        let state = Arc::clone(&state);
        ui.on_test_credential(
            move |provider, server_name, server, auth, username, identifier, secret| {
                let editor = credential_from_fields(
                    provider,
                    server_name,
                    server,
                    auth,
                    username,
                    identifier,
                    secret,
                );
                if let Some(ui) = weak.upgrade() {
                    ui.set_credential_status(SharedString::from("Testing live access…"));
                }
                let weak = weak.clone();
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    let result = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|error| {
                            format!("Unable to start credential test runtime: {error}")
                        })
                        .and_then(|runtime| runtime.block_on(test_editor(&editor)));
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_credential_status(SharedString::from(
                                result.clone().unwrap_or_else(|error| error),
                            ));
                        }
                        record_action_result(&state, result.map(|_| ()));
                    });
                });
            },
        );
    }
    {
        let weak = ui.as_weak();
        let state = Arc::clone(&state);
        let settings_draft = Arc::clone(&settings_draft);
        let restore_pending = Arc::clone(&interface_restore_pending_restart);
        let backup_layout = Rc::clone(&workspace_layout);
        let backup_appearance = Rc::clone(&appearance);
        let backup_custom_window = custom_window_state.clone();
        ui.on_backup_action(
            move |action,
                  path,
                  password,
                  settings,
                  credentials,
                  database,
                  interface,
                  diagnostics| {
                if restore_pending.load(Ordering::SeqCst) {
                    if let Some(ui) = weak.upgrade() {
                        ui.set_service_status("Restart SPLINED to finish restoring Interface Settings before another backup or restore.".into());
                    }
                    return;
                }
                if let Some(ui) = weak.upgrade()
                    && ui.get_media_refresh_running()
                {
                    ui.set_service_status(
                        "Wait for Refresh DB to finish before backing up or restoring.".into(),
                    );
                    return;
                }
                if action.as_str() == "backup-settings"
                    && (weak.upgrade().is_some_and(|ui| ui.get_settings_section_dirty())
                        || state.lock().is_ok_and(|app| {
                            app.enhancement_settings_dirty || unsaved_adjustment_defaults(&app)
                        }))
                {
                    if let Some(ui) = weak.upgrade() {
                        ui.set_service_status("Save Enhancement slider defaults and the changed Settings section before creating a Settings Archive.".into());
                    }
                    return;
                }
                if action.as_str() == "backup-settings" {
                    let Some(ui) = weak.upgrade() else {
                        return;
                    };
                    let result = capture_ui_preferences(
                        &ui,
                        &state,
                        &backup_layout,
                        &backup_appearance,
                        &backup_custom_window,
                    )
                    .ok_or_else(|| "Unable to snapshot the current Interface Settings.".to_string())
                    .and_then(|preferences| splined_core::settings::save_ui_preferences(&preferences));
                    if let Err(error) = result {
                        ui.set_service_status(format!("Settings Archive not created: {error}").into());
                        return;
                    }
                }
                let selection = BackupSelection {
                    settings,
                    credentials,
                    database,
                    interface,
                    diagnostics,
                };
                let path = std::path::PathBuf::from(path.as_str());
                let password = password.to_string();
                let backup_artist_images = weak.upgrade().is_some_and(|ui| ui.get_backup_artist_images());
                let restore_artist_images = weak.upgrade().is_some_and(|ui| ui.get_restore_artist_images());
                let config = state.lock().ok().map(|app| app.config.clone());
                let setup_required = weak.upgrade().is_some_and(|ui| ui.get_setup_required());
                // Arm the guard on the UI thread, before the worker starts. The
                // preference timer also runs here and must not race the restore.
                if action.as_str() == "restore"
                    && selection.interface
                    && config.is_some()
                    && (!setup_required || selection.settings)
                    && path
                        .extension()
                        .and_then(std::ffi::OsStr::to_str)
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("spl"))
                {
                    restore_pending.store(true, Ordering::SeqCst);
                }
                let weak = weak.clone();
                let state = Arc::clone(&state);
                let settings_draft = Arc::clone(&settings_draft);
                let restore_pending = Arc::clone(&restore_pending);
                std::thread::spawn(move || {
                    let result: Result<(String, Option<(Config, ApplicationState)>), String> =
                        match (action.as_str(), config) {
                        (_, None) => {
                            Err("The SPLINED application state is unavailable.".to_string())
                        }
                        ("backup-settings", Some(config)) => {
                            let settings_selection = BackupSelection {
                                database: false,
                                diagnostics: false,
                                ..selection
                            };
                            backup_file_in_directory(&path, "splined-settings", "spl")
                                .and_then(|path| {
                                    export_backup(
                                        &path,
                                        &config,
                                        settings_selection,
                                        &password,
                                    )
                                })
                                .map(|saved| {
                                    (format!("Settings backup created: {}", saved.display()), None)
                                })
                        }
                        ("backup-database", Some(config)) => {
                            backup_file_in_directory(&path, "splined-database", "sqlite")
                                .and_then(|path| export_database_backup_with_artist_art(&path, &config, backup_artist_images))
                                .map(|saved| {
                                    (format!("SQLite backup created: {}", saved.display()), None)
                                })
                        }
                        ("backup-diagnostics", Some(config)) => {
                            backup_file_in_directory(&path, "splined-diagnostics", "json")
                                .and_then(|path| export_diagnostic_summary(&path, &config))
                                .map(|saved| {
                                    (
                                        format!(
                                            "Diagnostic summary created: {}",
                                            saved.display()
                                        ),
                                        None,
                                    )
                                })
                        }
                        ("restore", _)
                            if setup_required
                                && !is_sqlite_backup_path(&path)
                                && !selection.settings =>
                        {
                            Err(
                            "Select Config v5 settings when restoring before Setup is complete."
                                .to_string(),
                            )
                        }
                        ("restore", Some(config)) if is_sqlite_backup_path(&path) => {
                            restore_database_backup_with_artist_art(&path, &config, restore_artist_images).and_then(|restored_path| {
                                let snapshot = media_snapshot(&config, false)?;
                                let diagnostics = startup_diagnostics(&config, &snapshot);
                                let mut replacement =
                                    ApplicationState::new(config.clone(), snapshot)?;
                                replacement.push_log(format!(
                                    "Standalone SQLite backup restored: {}",
                                    restored_path.display()
                                ));
                                for diagnostic in diagnostics {
                                    replacement.push_log(diagnostic);
                                }
                                Ok((
                                    "SQLite backup restored and Media Library reloaded."
                                        .to_string(),
                                    Some((config, replacement)),
                                ))
                            })
                        }
                        ("restore", Some(config)) => {
                            if path
                                .extension()
                                .and_then(std::ffi::OsStr::to_str)
                                .is_some_and(|extension| extension.eq_ignore_ascii_case("spl"))
                            {
                                let restored = restore_backup(&path, &config, selection, &password);
                                if restored.is_err() {
                                    restore_pending.store(false, Ordering::SeqCst);
                                }
                                restored.and_then(
                                    |restored| {
                                        let config = restored.config;
                                        let snapshot = media_snapshot(&config, false)?;
                                        let diagnostics = startup_diagnostics(&config, &snapshot);
                                        let mut replacement =
                                            ApplicationState::new(config.clone(), snapshot)?;
                                        replacement.push_log(
                                            "Backup restored and Config v5 loaded.".to_string(),
                                        );
                                        for diagnostic in diagnostics {
                                            replacement.push_log(diagnostic);
                                        }
                                        Ok((
                                            if restored.interface_restored {
                                                "Settings restored. Close and reopen SPLINED to load restored theme, panes, locks, presets, and colors. Do not make other changes before restarting.".to_string()
                                            } else {
                                                "Backup restored and loaded.".to_string()
                                            },
                                            Some((config, replacement)),
                                        ))
                                    },
                                )
                            } else {
                                Err(
                                    "Choose a .spl settings archive or .sqlite database backup."
                                        .to_string(),
                                )
                            }
                        }
                        _ => Err("Unsupported backup action.".to_string()),
                    };
                    let _ = slint::invoke_from_event_loop(move || {
                        match result {
                            Ok((message, Some((config, mut replacement)))) => {
                                replacement.service_status = message;
                                if let Ok(mut app) = state.lock() {
                                    replacement.embed_locked_album_paths =
                                        app.embed_locked_album_paths.clone();
                                    *app = replacement;
                                }
                                if let Ok(mut draft) = settings_draft.lock() {
                                    *draft = config.clone();
                                }
                                if let Some(ui) = weak.upgrade() {
                                    ui.set_setup_required(false);
                                    ui.set_page_index(0);
                                    ui.set_backup_password(SharedString::default());
                                    ui.set_restore_password_prompt_visible(false);
                                    set_settings_ui(&ui, &config);
                                }
                            }
                            Ok((message, None)) => {
                                if let Ok(mut app) = state.lock() {
                                    app.service_status = message;
                                }
                                if let Some(ui) = weak.upgrade() {
                                    refresh_archived_backups_ui(&ui, true);
                                    ui.set_backup_password(SharedString::default());
                                }
                            }
                            Err(error) => {
                                let error = if restore_pending.load(Ordering::SeqCst) {
                                    format!("{error} Interface Settings may have been restored; close and reopen SPLINED before making other changes.")
                                } else {
                                    error
                                };
                                if let Some(ui) = weak.upgrade()
                                    && (error.contains("requires a password")
                                        || error.contains("password is incorrect"))
                                {
                                    ui.set_restore_password_prompt_visible(true);
                                }
                                if let Ok(mut app) = state.lock() {
                                    app.service_status = error.clone();
                                }
                                record_action_result(&state, Err(error));
                            }
                        }
                        refresh_weak(&weak, &state);
                    });
                });
            },
        );
    }
    {
        let weak = ui.as_weak();
        let state = Arc::clone(&state);
        ui.on_check_update(move || {
            let window_handle = active_window_handle();
            if let Ok(mut app) = state.lock() {
                app.service_status = "Checking the official stable update channel…".to_string();
            }
            refresh_weak(&weak, &state);
            let weak = weak.clone();
            let state = Arc::clone(&state);
            std::thread::spawn(move || {
                let result = splined_core::portable::is_packaged_process().and_then(|packaged| {
                    tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|error| format!("Unable to start update runtime: {error}"))
                        .and_then(|runtime| {
                            if packaged {
                                runtime
                                    .block_on(splined_core::update::check_store_updates(window_handle))
                                    .map(|count| match count {
                                        0 => "Microsoft Store reports no available SPLINED update."
                                            .to_string(),
                                        1 => "Microsoft Store reports one available SPLINED update. Open Library > Get updates to install it."
                                            .to_string(),
                                        count => format!(
                                            "Microsoft Store reports {count} application updates, including any available SPLINED update. Open Library > Get updates to install them."
                                        ),
                                    })
                            } else {
                                runtime
                                    .block_on(splined_core::update::check_portable_update())
                                    .map(|info| info.message)
                            }
                        })
                });
                let _ = slint::invoke_from_event_loop(move || {
                    if let Ok(mut app) = state.lock() {
                        app.service_status = result.clone().unwrap_or_else(|error| error.clone());
                    }
                    record_action_result(&state, result.map(|_| ()));
                    refresh_weak(&weak, &state);
                });
            });
        });
    }
    {
        let weak = ui.as_weak();
        let state = Arc::clone(&state);
        ui.on_install_store_update(move || {
            let window_handle = active_window_handle();
            if let Ok(mut app) = state.lock() {
                app.service_status = "Requesting the Microsoft Store update…".to_string();
            }
            refresh_weak(&weak, &state);
            let weak = weak.clone();
            let state = Arc::clone(&state);
            std::thread::spawn(move || {
                let result = splined_core::portable::is_packaged_process().and_then(|packaged| {
                    if !packaged {
                        return Err(
                            "Store installation is available only to the Microsoft Store package."
                                .to_string(),
                        );
                    }
                    tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|error| format!("Unable to start update runtime: {error}"))?
                        .block_on(splined_core::update::install_store_updates(window_handle))
                });
                let _ = slint::invoke_from_event_loop(move || {
                    if let Ok(mut app) = state.lock() {
                        app.service_status = result.clone().unwrap_or_else(|error| error.clone());
                    }
                    record_action_result(&state, result.map(|_| ()));
                    refresh_weak(&weak, &state);
                });
            });
        });
    }
    {
        let state = Arc::clone(&state);
        ui.on_open_about_link(move |target| {
            let url = match target.as_str() {
                "store" => Some("https://apps.microsoft.com/detail/9p8g4gmbbvbs?hl=en-US&gl=US"),
                "source" => Some("https://github.com/scottia/S-P-L-I-N-E-D"),
                "releases" => Some("https://github.com/scottia/S-P-L-I-N-E-D/releases/latest"),
                "guide" => {
                    Some("https://github.com/scottia/S-P-L-I-N-E-D/blob/main/docs/windows-guide.md")
                }
                "shared-db" => Some(
                    "https://github.com/scottia/S-P-L-I-N-E-D/blob/main/docs/splined-media-database.md",
                ),
                _ => None,
            };
            let result = url
                .ok_or_else(|| "The About link is unavailable.".to_string())
                .and_then(open_external_url);
            record_action_result(&state, result);
        });
    }
}

fn ensure_settings_preview_source() -> Result<PathBuf, String> {
    let directory = std::env::temp_dir().join("splined-settings-preview");
    std::fs::create_dir_all(&directory).map_err(|error| {
        format!(
            "Unable to create the Settings preview directory {}: {error}",
            directory.display()
        )
    })?;
    let path = directory.join("enhancement-source.png");
    if path.is_file() {
        return Ok(path);
    }
    let image = image::RgbaImage::from_fn(640, 640, |x, y| {
        let fx = x as f32 / 639.0;
        let fy = y as f32 / 639.0;
        let wave = (((x as f32 * 0.043).sin() + (y as f32 * 0.031).cos()) * 20.0) as i32;
        let center = (((x as i32 - 320).pow(2) + (y as i32 - 300).pow(2)) as f32)
            .sqrt()
            .min(430.0);
        let glow = ((430.0 - center) / 430.0 * 92.0) as i32;
        let diagonal = if ((x + y) / 74).is_multiple_of(2) {
            22
        } else {
            -10
        };
        let red = (42 + (196.0 * fx) as i32 + glow + wave / 2).clamp(0, 255) as u8;
        let green = (35 + (170.0 * (1.0 - fy)) as i32 + glow / 2 + diagonal).clamp(0, 255) as u8;
        let blue =
            (74 + (156.0 * fy) as i32 + (80.0 * (1.0 - fx)) as i32 - wave / 3).clamp(0, 255) as u8;
        image::Rgba([red, green, blue, 255])
    });
    image
        .save_with_format(&path, image::ImageFormat::Png)
        .map_err(|error| {
            format!(
                "Unable to create the Settings enhancement sample {}: {error}",
                path.display()
            )
        })?;
    Ok(path)
}

fn render_settings_preview(config: &Config) -> Result<(String, String), String> {
    let source_path = ensure_settings_preview_source()?;
    let candidate = Candidate::from_file("settings", &source_path, 0)?;
    let range = Range {
        min: 640,
        ideal: 640,
        max: 640,
        ladder: 640,
    };
    let prepared = prepare_existing_cover_edit(
        &candidate,
        &source_path,
        &range,
        StaticFormat::Png,
        &config.output,
        true,
    )?;
    let adjusted_path = source_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("enhancement-adjusted.png");
    replace_binary_file(
        &adjusted_path,
        &prepared.bytes,
        "Settings enhancement preview",
        |staged| inspect_image(staged).map(|_| ()),
    )?;
    let source_text = source_path.to_string_lossy().into_owned();
    let adjusted_text = adjusted_path.to_string_lossy().into_owned();
    if !preload_scaled_image(&source_text, PREVIEW_CACHE_DIMENSION)
        || !preload_scaled_image(&adjusted_text, PREVIEW_CACHE_DIMENSION)
    {
        let _ = std::fs::remove_file(&adjusted_path);
        return Err("Unable to load the Settings enhancement comparison.".to_string());
    }
    Ok((source_text, adjusted_text))
}

fn set_settings_preview_ui(ui: &AppWindow, config: &Config) -> Result<(), String> {
    let (source, adjusted) = render_settings_preview(config)?;
    ui.set_settings_original_preview(load_cached_preview_image(&source, ""));
    ui.set_settings_current_preview(load_cached_preview_image(&adjusted, &source));
    Ok(())
}

fn set_settings_adjustment_ui(ui: &AppWindow, app: &ApplicationState, changed: bool) {
    ui.set_settings_upscale_picture(app.adjustments.picture_percent.to_string().into());
    ui.set_settings_upscale_sharpen(app.adjustments.sharpen_percent.to_string().into());
    ui.set_settings_upscale_softness(app.adjustments.softness_percent.to_string().into());
    ui.set_settings_upscale_contrast(app.adjustments.contrast_percent.to_string().into());
    ui.set_settings_upscale_exposure(app.adjustments.exposure_percent.to_string().into());
    ui.set_settings_upscale_brightness(app.adjustments.brightness_percent.to_string().into());
    ui.set_settings_upscale_gamma(app.adjustments.gamma_percent.to_string().into());
    ui.set_settings_upscale_color(app.adjustments.color_temperature.to_string().into());
    ui.set_settings_upscale_hue(app.adjustments.hue_degrees.to_string().into());
    ui.set_settings_upscale_darkness(app.adjustments.darkness_percent.to_string().into());
    ui.set_settings_upscale_tint(app.adjustments.tint_percent.to_string().into());
    ui.set_settings_upscale_tint_color(app.adjustments.tint_color.clone().into());
    ui.set_settings_enhancement_changed(changed);
}

fn apply_adjustment_values(
    state: &Arc<Mutex<ApplicationState>>,
    weak: &slint::Weak<AppWindow>,
    revision: &Arc<AtomicU64>,
    gate: &Arc<Mutex<()>>,
    values: [i32; 11],
    tint_color: Option<String>,
) {
    if let Some(ui) = weak.upgrade() { ui.set_artist_preview_reset_active(false); }
    if let Ok(mut app) = state.lock() {
        app.adjustments.apply_slider_values(values);
        if let Some(tint_color) = tint_color {
            app.adjustments.tint_color = tint_color;
        } else if app.adjustments.tint_percent != 0
            && weak.upgrade().is_some_and(|ui| ui.get_page_index() == 0)
        {
            ensure_active_tint_palette(&mut app);
        }
        app.upscale_preview_only = true;
        clear_rendered_preview_for_adjustment(&mut app);
    }
    if let Some(ui) = weak.upgrade() {
        if let Ok(app) = state.lock() {
            set_settings_adjustment_ui(&ui, &app, true);
        }
        ui.set_settings_section_dirty(true);
    }
    refresh_weak(weak, state);
    if weak.upgrade().is_some_and(|ui| ui.get_page_index() == 1) {
        schedule_settings_adjustment_preview(
            Arc::clone(state),
            weak.clone(),
            Arc::clone(revision),
            Arc::clone(gate),
        );
    } else {
        schedule_adjustment_preview(
            Arc::clone(state),
            weak.clone(),
            Arc::clone(revision),
            Arc::clone(gate),
        );
    }
}

fn stack_adjustment_values(current: [i32; 11], increment: [i32; 11]) -> [i32; 11] {
    let names = ["Picture", "Sharpen", "Softness", "Contrast", "Exposure", "Brightness", "Gamma", "Color", "Hue", "Darkness", "Tint"];
    std::array::from_fn(|index| {
        let (minimum, maximum) = adjustment_bounds(names[index]);
        current[index].saturating_add(increment[index]).clamp(minimum, maximum)
    })
}

const FILTER_MENU_NAMES: [&str; 14] = [
    "Default", "Clarity", "Vivid Color", "Warm/Cool Color", "Low/Dark Light",
    "Soft/Sharper", "High/Low Contrast", "Portrait Mode", "Blur Background",
    "Custom A", "Custom B", "Custom C", "Custom D", "Detailed Default",
];

fn filter_increment(app: &ApplicationState, index: usize) -> [i32; 11] {
    let saved_default = || {
        AdjustmentState::from_config(&app.config).slider_values().map(|value| {
            if value == 0 { 0 } else { value.signum() * (value.abs() / 5).max(1) }
        })
    };
    match index {
        0 => saved_default(),
        1 => [0, 5, 0, 3, 0, 0, 0, 0, 0, 0, 0],
        2 => [6, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0],
        3 => [0, 0, 0, 0, 0, 0, 0, 12, 0, 0, 0],
        4 => [0, 0, 0, 0, 5, 4, 0, 0, 0, -4, 0],
        5 => [0, 6, -6, 0, 0, 0, 0, 0, 0, 0, 0],
        6 => [0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0],
        7 => [3, 0, 4, -2, 0, 2, 0, 0, 0, 0, 0],
        8 => [0, 0, 6, 0, 0, 0, 0, 0, 0, 0, 0],
        9..=12 => app.adjustment_presets.get(index - 8).filter(|preset| preset.saved)
            .map(|preset| preset.values.map(|value| if value == 0 { 0 } else { value.signum() * (value.abs() / 5).max(1) }))
            .unwrap_or([0; 11]),
        13 => {
            let mut values = saved_default();
            for index in [1, 2, 3, 7, 9] { values[index] = 0; }
            values
        }
        _ => [0; 11],
    }
}

fn active_filter_levels(app: &ApplicationState) -> [i32; 14] {
    if !app.artist_artwork_mode { return app.album_filter_levels; }
    match app.artist_preview_focus.as_str() {
        "banner" => app.artist_banner_filter_levels,
        "theme" => app.artist_theme_filter_levels,
        _ => app.artist_portrait_filter_levels,
    }
}

fn set_active_filter_levels(app: &mut ApplicationState, levels: [i32; 14]) {
    if !app.artist_artwork_mode { app.album_filter_levels = levels; return; }
    match app.artist_preview_focus.as_str() {
        "banner" => app.artist_banner_filter_levels = levels,
        "theme" => app.artist_theme_filter_levels = levels,
        _ => app.artist_portrait_filter_levels = levels,
    }
}

fn change_filter_level(app: &mut ApplicationState, name: &str, target: i32) -> bool {
    let Some(index) = FILTER_MENU_NAMES.iter().position(|label| *label == name) else { return false; };
    let target = target.clamp(-5, 5);
    let mut levels = active_filter_levels(app);
    let difference = target - levels[index];
    if difference == 0 { return false; }
    let increment = filter_increment(app, index).map(|value| value.saturating_mul(difference));
    if increment == [0; 11] && (9..=12).contains(&index) {
        app.operation_status = format!("Save {} in Settings before adjusting its intensity", name);
        app.operation_tone = "orange".to_string();
        return false;
    }
    if increment == [0; 11] && (index == 0 || index == 13) {
        app.operation_status = "Set enhancement defaults in Settings before changing Default intensity".to_string();
        app.operation_tone = "orange".to_string();
        return false;
    }
    let values = stack_adjustment_values(app.adjustments.slider_values(), increment);
    app.adjustments.apply_slider_values(values);
    levels[index] = target;
    set_active_filter_levels(app, levels);
    app.upscale_preview_only = true;
    clear_rendered_preview_for_adjustment(app);
    true
}

fn sync_enhancement_menu_rows(existing: ModelRc<UiAdjustmentRow>, rows: Vec<UiAdjustmentRow>) -> bool {
    if existing.row_count() != rows.len() {
        return false;
    }
    // Replacing the model destroys the active TouchArea and releases a mouse
    // drag after its first value change. Keep the row instances alive.
    for (index, row) in rows.into_iter().enumerate() {
        if existing.row_data(index).as_ref() != Some(&row) {
            existing.set_row_data(index, row);
        }
    }
    true
}

fn active_tint_source(app: &ApplicationState) -> Option<&str> {
    if app.artist_artwork_mode {
        return match app.artist_preview_focus.as_str() {
            "banner" => Some(app.artist_banner_source_path.as_str()),
            _ => Some(app.artist_artwork_draft_path.as_str()),
        }
        .filter(|path| !path.trim().is_empty());
    }
    app.selected_candidate()
        .map(|candidate| candidate.cache_path.as_str())
        .filter(|path| !path.trim().is_empty())
        .or_else(|| (!app.musicbrainz_cover_preview_path.is_empty()).then_some(app.musicbrainz_cover_preview_path.as_str()))
        .or_else(|| (!app.current_cover_path.is_empty()).then_some(app.current_cover_path.as_str()))
}

fn image_derived_tint_color(bytes: &[u8]) -> Option<String> {
    let pixels = image::load_from_memory(bytes).ok()?.thumbnail(96, 96).to_rgb8();
    let mut bins = [(0.0_f32, [0.0_f32; 3]); 24];
    for pixel in pixels.pixels() {
        let rgb = pixel.0.map(|value| f32::from(value) / 255.0);
        let maximum = rgb.iter().copied().fold(0.0_f32, f32::max);
        let minimum = rgb.iter().copied().fold(1.0_f32, f32::min);
        let chroma = maximum - minimum;
        if maximum < 0.12 || chroma < 0.12 {
            continue;
        }
        let hue = if maximum == rgb[0] {
            ((rgb[1] - rgb[2]) / chroma).rem_euclid(6.0)
        } else if maximum == rgb[1] {
            (rgb[2] - rgb[0]) / chroma + 2.0
        } else {
            (rgb[0] - rgb[1]) / chroma + 4.0
        };
        let index = ((hue * 4.0).floor() as usize).min(23);
        let weight = chroma * maximum;
        bins[index].0 += weight;
        for channel in 0..3 {
            bins[index].1[channel] += rgb[channel] * weight;
        }
    }
    let (weight, totals) = bins.into_iter().max_by(|left, right| left.0.total_cmp(&right.0))?;
    if weight <= f32::EPSILON {
        return None;
    }
    let [red, green, blue] = totals.map(|sum| (sum / weight * 255.0).round().clamp(0.0, 255.0) as u8);
    Some(format!("#{red:02X}{green:02X}{blue:02X}"))
}

fn ensure_active_tint_palette(app: &mut ApplicationState) {
    let Some(source) = active_tint_source(app).map(str::to_owned) else { return; };
    if app.tint_palette_source == source {
        return;
    }
    let bytes = if source.starts_with("memory-preview://") {
        full_edit_preview_bytes(&source)
    } else {
        fs::read(&source).ok()
    };
    if let Some(bytes) = bytes {
        app.adjustments.tint_color = image_derived_tint_color(&bytes)
            .unwrap_or_else(|| "#808080".to_string());
        app.tint_palette_source = source;
    }
}

fn set_settings_ui(ui: &AppWindow, config: &Config) {
    ui.set_settings_music_library(config.library.music_library.clone().into());
    ui.set_settings_scan_library(config.scan.scan_library_dir.clone().into());
    ui.set_settings_database_dir(config.scan.cache_dir.clone().into());
    ui.set_settings_shared_artist_art_dir(
        configured_shared_artist_art_dir(config)
            .to_string_lossy()
            .into_owned()
            .into(),
    );
    ui.set_settings_temporary_cache_dir(config.scan.temporary_cache_dir.clone().into());
    ui.set_settings_log_dir(config.scan.log_dir.clone().into());
    ui.set_settings_credential_dir(config.credentials.credential_dir.clone().into());
    ui.set_settings_ignored_subs(config.library.ignored_subs.join(", ").into());
    ui.set_settings_mode(
        match config.mode {
            Mode::Read => "Read",
            Mode::Write => "Write",
        }
        .into(),
    );
    ui.set_settings_verbosity(diagnostic_level_label(config.verbosity).into());
    ui.set_settings_scan_mode(config.scan.scan_mode);
    ui.set_settings_library_scan(config.scan.library_scan);
    let override_label = override_delay_label(config.scan.scan_mode_timeout.0);
    let mut override_options = OVERRIDE_DELAY_CHOICES
        .iter()
        .map(|(label, _)| SharedString::from(*label))
        .collect::<Vec<_>>();
    if override_label.starts_with("Custom: ") {
        override_options.push(SharedString::from(override_label.clone()));
    }
    ui.set_settings_override_options(model(override_options));
    ui.set_settings_scan_timeout(override_label.into());
    ui.set_settings_sqlite_shared(config.scan.sqlite_shared);
    ui.set_settings_windows_client_lock(config.scan.windows_client_lock);
    ui.set_settings_sample_write(config.samples.sample_write);
    ui.set_settings_sample_dir(
        configured_samples_dir(config)
            .to_string_lossy()
            .into_owned()
            .into(),
    );
    ui.set_settings_file_formats(config.output.file_formats.join(", ").into());
    ui.set_settings_file_name(config.output.file_name.clone().into());
    ui.set_settings_preserve_file(config.output.preserve_file);
    ui.set_settings_square(config.output.square);
    ui.set_settings_square_mode(
        if config.output.square_mode.eq_ignore_ascii_case("crop") {
            "Crop"
        } else {
            "Off"
        }
        .into(),
    );
    ui.set_settings_square_round_to(config.output.square_round_to.to_string().into());
    ui.set_settings_embedded_thumbnail_size(
        config.output.embedded_thumbnail_size.to_string().into(),
    );
    ui.set_settings_upscale_below_ideal(config.output.upscale_below_ideal);
    ui.set_settings_upscale_max_percent(config.output.upscale_max_percent.to_string().into());
    ui.set_settings_upscale_adaptive(config.output.upscale_adaptive_defaults);
    ui.set_settings_upscale_picture(config.output.upscale_picture_percent.to_string().into());
    ui.set_settings_upscale_sharpen(config.output.upscale_sharpen_percent.to_string().into());
    ui.set_settings_upscale_softness(config.output.upscale_softness_percent.to_string().into());
    ui.set_settings_upscale_contrast(config.output.upscale_contrast_percent.to_string().into());
    ui.set_settings_upscale_exposure(config.output.upscale_exposure_percent.to_string().into());
    ui.set_settings_upscale_brightness(config.output.upscale_brightness_percent.to_string().into());
    ui.set_settings_upscale_gamma(config.output.upscale_gamma_percent.to_string().into());
    ui.set_settings_upscale_color(config.output.upscale_color_temperature.to_string().into());
    ui.set_settings_upscale_hue(config.output.upscale_hue_degrees.to_string().into());
    ui.set_settings_upscale_darkness(config.output.upscale_darkness_percent.to_string().into());
    ui.set_settings_upscale_tint(config.output.upscale_tint_percent.to_string().into());
    ui.set_settings_upscale_tint_color(config.output.upscale_tint_color.clone().into());
    ui.set_settings_enhancement_changed(false);
    ui.set_settings_evaluate_final(config.output.evaluate_final_image);
    ui.set_settings_range_min(config.range.min.to_string().into());
    ui.set_settings_range_ideal(config.range.ideal.to_string().into());
    ui.set_settings_range_max(config.range.max.to_string().into());
    ui.set_settings_range_ladder(config.range.ladder.to_string().into());
    ui.set_settings_source_order(config.sources.cover_sources.join(", ").into());
    ui.set_settings_source_exclusions(config.sources.exclude_cover_sources.join(", ").into());
    ui.set_settings_history_enabled(config.history.enabled);
    ui.set_settings_history_retention(config.history.retention_days.to_string().into());
    ui.set_settings_log_retention(config.logging.retention_days.to_string().into());
    ui.set_settings_ai_enabled(config.aisplined.enabled);
    ui.set_settings_ai_endpoint(config.aisplined.endpoint.clone().into());
    ui.set_settings_ai_minimum(config.aisplined.minimum_short_side.to_string().into());
    ui.set_settings_ai_allow_below(config.aisplined.allow_below_minimum_override);
    let source = ui.get_settings_policy_source().to_string();
    let source = if splined_core::config::SUPPORTED_SOURCE_POLICIES.contains(&source.as_str()) {
        source
    } else {
        splined_core::config::SUPPORTED_SOURCE_POLICIES[0].to_string()
    };
    set_source_policy_ui(ui, config, &source);
    let _ = set_settings_preview_ui(ui, config);
}

fn set_source_policy_ui(ui: &AppWindow, config: &Config, source: &str) {
    let source = source.trim().to_ascii_lowercase();
    let policy = config
        .source_policies
        .get(&source)
        .cloned()
        .unwrap_or_else(|| SourcePolicyConfig {
            enabled: config
                .sources
                .cover_sources
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(&source))
                && !config
                    .sources
                    .exclude_cover_sources
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(&source)),
            ..SourcePolicyConfig::default()
        });
    ui.set_settings_policy_source(source.clone().into());
    // A provider is usable only when it is in the artwork-source order, is
    // not excluded, and its policy is enabled. Present that effective state
    // instead of a policy flag that can disagree with Candidate Findings.
    let available = config
        .sources
        .cover_sources
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(&source))
        && !config
            .sources
            .exclude_cover_sources
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(&source));
    ui.set_settings_policy_enabled(policy.enabled && available);
    ui.set_settings_policy_strict(policy.strict_override);
    ui.set_settings_policy_override(policy.source_override);
    ui.set_settings_policy_minimum_range(minimum_range_label(policy.minimum_range_type).into());
    ui.set_settings_policy_fallback(policy.allow_below_minimum_fallback);
    ui.set_settings_policy_minimum_short(optional_number(policy.minimum_short_side).into());
    ui.set_settings_policy_maximum_short(optional_number(policy.maximum_short_side).into());
    ui.set_settings_policy_minimum_width(optional_number(policy.minimum_width).into());
    ui.set_settings_policy_minimum_height(optional_number(policy.minimum_height).into());
    ui.set_settings_policy_primary_only(policy.primary_image_only);
}

fn apply_settings_ui(ui: &AppWindow, config: &mut Config) -> Result<(), String> {
    config.library.music_library = ui.get_settings_music_library().trim().to_string();
    config.scan.scan_library_dir = ui.get_settings_scan_library().trim().to_string();
    config.scan.cache_dir = ui.get_settings_database_dir().trim().to_string();
    config.scan.shared_artist_art_dir = ui.get_settings_shared_artist_art_dir().trim().to_string();
    if config.scan.shared_artist_art_dir.is_empty() {
        return Err("V/A / OST Artist images database is required.".to_string());
    }
    config.scan.temporary_cache_dir = ui.get_settings_temporary_cache_dir().trim().to_string();
    config.scan.log_dir = ui.get_settings_log_dir().trim().to_string();
    config.credentials.credential_dir = ui.get_settings_credential_dir().trim().to_string();
    config.library.ignored_subs = split_values(ui.get_settings_ignored_subs().as_str());
    config.mode = match ui.get_settings_mode().as_str() {
        "Write" => Mode::Write,
        "Read" => Mode::Read,
        value => return Err(format!("Unsupported mode: {value}")),
    };
    config.verbosity = diagnostic_level_verbosity_for_config(
        ui.get_settings_verbosity().as_str(),
        config.verbosity,
    )?;
    config.scan.scan_mode = ui.get_settings_scan_mode();
    config.scan.library_scan = ui.get_settings_library_scan();
    config.scan.scan_mode_timeout.0 =
        override_delay_hours(ui.get_settings_scan_timeout().as_str())?;
    config.scan.sqlite_shared = ui.get_settings_sqlite_shared();
    config.scan.windows_client_lock = ui.get_settings_windows_client_lock();
    config.samples.sample_write = ui.get_settings_sample_write();
    config.samples.sample_dir = ui.get_settings_sample_dir().trim().to_string();
    config.output.file_formats = split_values(ui.get_settings_file_formats().as_str());
    config.output.file_name = ui.get_settings_file_name().trim().to_string();
    config.output.preserve_file = ui.get_settings_preserve_file();
    config.output.square = ui.get_settings_square();
    config.output.square_mode = ui.get_settings_square_mode().trim().to_ascii_lowercase();
    config.output.square_round_to =
        parse_setting(&ui.get_settings_square_round_to(), "Square round-to")?;
    config.output.embedded_thumbnail_size = parse_setting(
        &ui.get_settings_embedded_thumbnail_size(),
        "Embedded Thumbnail Preference",
    )?;
    config.output.upscale_below_ideal = ui.get_settings_upscale_below_ideal();
    config.output.upscale_max_percent = parse_setting(
        &ui.get_settings_upscale_max_percent(),
        "Maximum upscale percent",
    )?;
    config.output.upscale_adaptive_defaults = ui.get_settings_upscale_adaptive();
    config.output.upscale_picture_percent =
        parse_setting(&ui.get_settings_upscale_picture(), "Picture adjustment")?;
    config.output.upscale_sharpen_percent =
        parse_setting(&ui.get_settings_upscale_sharpen(), "Sharpen adjustment")?;
    config.output.upscale_softness_percent =
        parse_setting(&ui.get_settings_upscale_softness(), "Softness adjustment")?;
    config.output.upscale_contrast_percent =
        parse_setting(&ui.get_settings_upscale_contrast(), "Contrast adjustment")?;
    config.output.upscale_exposure_percent =
        parse_setting(&ui.get_settings_upscale_exposure(), "Exposure adjustment")?;
    config.output.upscale_brightness_percent = parse_setting(
        &ui.get_settings_upscale_brightness(),
        "Brightness adjustment",
    )?;
    config.output.upscale_gamma_percent =
        parse_setting(&ui.get_settings_upscale_gamma(), "Gamma adjustment")?;
    config.output.upscale_color_temperature =
        parse_setting(&ui.get_settings_upscale_color(), "Color temperature")?;
    config.output.upscale_hue_degrees = parse_setting(&ui.get_settings_upscale_hue(), "Hue")?;
    config.output.upscale_darkness_percent =
        parse_setting(&ui.get_settings_upscale_darkness(), "Darkness")?;
    config.output.upscale_tint_percent = parse_setting(&ui.get_settings_upscale_tint(), "Tint")?;
    config.output.upscale_tint_color = ui.get_settings_upscale_tint_color().trim().to_string();
    config.output.evaluate_final_image = ui.get_settings_evaluate_final();
    config.range.min = parse_setting(&ui.get_settings_range_min(), "Range minimum")?;
    config.range.ideal = parse_setting(&ui.get_settings_range_ideal(), "Range ideal")?;
    config.range.max = parse_setting(&ui.get_settings_range_max(), "Range maximum")?;
    config.range.ladder = parse_setting(&ui.get_settings_range_ladder(), "Range ladder")?;
    config.sources.cover_sources = split_values(ui.get_settings_source_order().as_str());
    config.sources.exclude_cover_sources =
        split_values(ui.get_settings_source_exclusions().as_str());
    config.history.enabled = ui.get_settings_history_enabled();
    config.history.retention_days =
        parse_setting(&ui.get_settings_history_retention(), "History retention")?;
    config.logging.retention_days =
        parse_setting(&ui.get_settings_log_retention(), "Log retention")?;
    config.aisplined.enabled = ui.get_settings_ai_enabled();
    config.aisplined.endpoint = ui.get_settings_ai_endpoint().trim().to_string();
    config.aisplined.minimum_short_side = parse_setting(
        &ui.get_settings_ai_minimum(),
        "AISPLINED minimum short side",
    )?;
    config.aisplined.allow_below_minimum_override = ui.get_settings_ai_allow_below();
    update_source_policy_from_ui(ui, config)?;
    Ok(())
}

fn apply_settings_section_ui(
    ui: &AppWindow,
    config: &mut Config,
    section: &str,
) -> Result<(), String> {
    match section {
        "Library" => {
            config.library.music_library = ui.get_settings_music_library().trim().to_string();
            config.scan.scan_library_dir = ui.get_settings_scan_library().trim().to_string();
            config.scan.cache_dir = ui.get_settings_database_dir().trim().to_string();
            config.scan.shared_artist_art_dir =
                ui.get_settings_shared_artist_art_dir().trim().to_string();
            if config.scan.shared_artist_art_dir.is_empty() {
                return Err("V/A / OST Artist images database is required.".to_string());
            }
            config.scan.temporary_cache_dir =
                ui.get_settings_temporary_cache_dir().trim().to_string();
            config.scan.log_dir = ui.get_settings_log_dir().trim().to_string();
            config.credentials.credential_dir = ui.get_settings_credential_dir().trim().to_string();
            config.library.ignored_subs = split_values(ui.get_settings_ignored_subs().as_str());
        }
        "Artwork" => {
            config.scan.shared_artist_art_dir =
                ui.get_settings_shared_artist_art_dir().trim().to_string();
            if config.scan.shared_artist_art_dir.is_empty() {
                return Err("V/A / OST Artist images database is required.".to_string());
            }
            config.output.file_formats = split_values(ui.get_settings_file_formats().as_str());
            config.output.file_name = ui.get_settings_file_name().trim().to_string();
            config.output.preserve_file = ui.get_settings_preserve_file();
            config.output.square = ui.get_settings_square();
            config.output.square_mode = ui.get_settings_square_mode().trim().to_ascii_lowercase();
            config.output.square_round_to =
                parse_setting(&ui.get_settings_square_round_to(), "Square round-to")?;
            config.output.embedded_thumbnail_size = parse_setting(
                &ui.get_settings_embedded_thumbnail_size(),
                "Embedded Thumbnail Preference",
            )?;
            config.samples.sample_write = ui.get_settings_sample_write();
            config.samples.sample_dir = ui.get_settings_sample_dir().trim().to_string();
        }
        "Enhancement" => {
            config.output.upscale_below_ideal = ui.get_settings_upscale_below_ideal();
            config.output.upscale_max_percent = parse_setting(
                &ui.get_settings_upscale_max_percent(),
                "Maximum upscale percent",
            )?;
            config.output.upscale_adaptive_defaults = ui.get_settings_upscale_adaptive();
            // A Custom A–D radio previews that slot in the existing comparison
            // area. Saving its name/values must not silently replace the global
            // enhancement defaults in Config v5.
            if ui.get_settings_preview_preset_index() == 0 {
                config.output.upscale_picture_percent =
                    parse_setting(&ui.get_settings_upscale_picture(), "Picture adjustment")?;
                config.output.upscale_sharpen_percent =
                    parse_setting(&ui.get_settings_upscale_sharpen(), "Sharpen adjustment")?;
                config.output.upscale_softness_percent =
                    parse_setting(&ui.get_settings_upscale_softness(), "Softness adjustment")?;
                config.output.upscale_contrast_percent =
                    parse_setting(&ui.get_settings_upscale_contrast(), "Contrast adjustment")?;
                config.output.upscale_exposure_percent =
                    parse_setting(&ui.get_settings_upscale_exposure(), "Exposure adjustment")?;
                config.output.upscale_brightness_percent = parse_setting(
                    &ui.get_settings_upscale_brightness(),
                    "Brightness adjustment",
                )?;
                config.output.upscale_gamma_percent =
                    parse_setting(&ui.get_settings_upscale_gamma(), "Gamma adjustment")?;
                config.output.upscale_color_temperature =
                    parse_setting(&ui.get_settings_upscale_color(), "Color temperature")?;
                config.output.upscale_hue_degrees =
                    parse_setting(&ui.get_settings_upscale_hue(), "Hue")?;
                config.output.upscale_darkness_percent =
                    parse_setting(&ui.get_settings_upscale_darkness(), "Darkness")?;
                config.output.upscale_tint_percent =
                    parse_setting(&ui.get_settings_upscale_tint(), "Tint")?;
                config.output.upscale_tint_color =
                    ui.get_settings_upscale_tint_color().trim().to_string();
            }
            config.output.evaluate_final_image = ui.get_settings_evaluate_final();
            config.range.min = parse_setting(&ui.get_settings_range_min(), "Range minimum")?;
            config.range.ideal = parse_setting(&ui.get_settings_range_ideal(), "Range ideal")?;
            config.range.max = parse_setting(&ui.get_settings_range_max(), "Range maximum")?;
            config.range.ladder = parse_setting(&ui.get_settings_range_ladder(), "Range ladder")?;
        }
        "Sources" => {
            config.sources.cover_sources = split_values(ui.get_settings_source_order().as_str());
            config.sources.exclude_cover_sources =
                split_values(ui.get_settings_source_exclusions().as_str());
            update_source_policy_from_ui(ui, config)?;
        }
        "Runtime" => {
            config.mode = match ui.get_settings_mode().as_str() {
                "Write" => Mode::Write,
                "Read" => Mode::Read,
                value => return Err(format!("Unsupported mode: {value}")),
            };
            config.verbosity = diagnostic_level_verbosity_for_config(
                ui.get_settings_verbosity().as_str(),
                config.verbosity,
            )?;
            config.scan.scan_mode = ui.get_settings_scan_mode();
            config.scan.library_scan = ui.get_settings_library_scan();
            config.scan.scan_mode_timeout.0 =
                override_delay_hours(ui.get_settings_scan_timeout().as_str())?;
            config.scan.sqlite_shared = ui.get_settings_sqlite_shared();
            config.scan.windows_client_lock = ui.get_settings_windows_client_lock();
            config.history.enabled = ui.get_settings_history_enabled();
            config.history.retention_days =
                parse_setting(&ui.get_settings_history_retention(), "History retention")?;
            config.logging.retention_days =
                parse_setting(&ui.get_settings_log_retention(), "Log retention")?;
        }
        "Connections" => {
            config.aisplined.enabled = ui.get_settings_ai_enabled();
            config.aisplined.endpoint = ui.get_settings_ai_endpoint().trim().to_string();
            config.aisplined.minimum_short_side = parse_setting(
                &ui.get_settings_ai_minimum(),
                "AISPLINED minimum short side",
            )?;
            config.aisplined.allow_below_minimum_override = ui.get_settings_ai_allow_below();
        }
        _ => {
            return Err(format!(
                "{section} does not have Config v5 settings to save."
            ));
        }
    }
    Ok(())
}

fn safe_reset_config(current: &Config) -> Config {
    let mut reset = Config::default();
    reset.library.music_library = current.library.music_library.clone();
    reset.library.ignored_subs = current.library.ignored_subs.clone();
    reset.scan.scan_library_dir = current.scan.scan_library_dir.clone();
    reset.scan.cache_dir = current.scan.cache_dir.clone();
    reset.scan.temporary_cache_dir = current.scan.temporary_cache_dir.clone();
    reset.scan.log_dir = current.scan.log_dir.clone();
    reset.scan.sqlite_shared = current.scan.sqlite_shared;
    reset.scan.windows_client_lock = current.scan.windows_client_lock;
    reset.credentials = current.credentials.clone();
    reset
}

fn backup_config_before_reset() -> Result<PathBuf, String> {
    let layout = splined_core::portable::app_layout()?;
    let backup_dir = layout.root.join("backups");
    fs::create_dir_all(&backup_dir).map_err(|error| {
        format!(
            "Unable to create config backup directory {}: {error}",
            backup_dir.display()
        )
    })?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("Unable to timestamp config backup: {error}"))?
        .as_millis();
    let backup = backup_dir.join(format!("config.toml.pre-reset-{stamp}.bak"));
    fs::copy(&layout.config_file, &backup).map_err(|error| {
        format!(
            "Unable to back up {}: {error}",
            layout.config_file.display()
        )
    })?;
    Ok(backup)
}

fn update_source_policy_from_ui(ui: &AppWindow, config: &mut Config) -> Result<(), String> {
    let source = ui.get_settings_policy_source().trim().to_ascii_lowercase();
    if !splined_core::config::SUPPORTED_SOURCE_POLICIES.contains(&source.as_str()) {
        return Err(format!("Unsupported source policy provider: {source}"));
    }
    let policy = SourcePolicyConfig {
        enabled: ui.get_settings_policy_enabled(),
        strict_override: ui.get_settings_policy_strict(),
        source_override: ui.get_settings_policy_override(),
        minimum_range_type: parse_minimum_range(ui.get_settings_policy_minimum_range().as_str())?,
        allow_below_minimum_fallback: ui.get_settings_policy_fallback(),
        minimum_short_side: parse_optional_setting(
            &ui.get_settings_policy_minimum_short(),
            "Minimum short side",
        )?,
        maximum_short_side: parse_optional_setting(
            &ui.get_settings_policy_maximum_short(),
            "Maximum short side",
        )?,
        minimum_width: parse_optional_setting(
            &ui.get_settings_policy_minimum_width(),
            "Minimum width",
        )?,
        minimum_height: parse_optional_setting(
            &ui.get_settings_policy_minimum_height(),
            "Minimum height",
        )?,
        primary_image_only: ui.get_settings_policy_primary_only(),
    };
    policy.validate()?;
    if policy.enabled {
        include_artwork_source(config, &source);
    }
    config.source_policies.insert(source, policy);
    Ok(())
}

fn include_artwork_source(config: &mut Config, source: &str) {
    if !config
        .sources
        .cover_sources
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(source))
    {
        config.sources.cover_sources.push(source.to_string());
    }
    config
        .sources
        .exclude_cover_sources
        .retain(|candidate| !candidate.eq_ignore_ascii_case(source));
}

fn source_name_from_drag(data: &slint::DataTransfer) -> Option<String> {
    data.plain_text()
        .ok()?
        .strip_prefix("splined-source:")
        .filter(|source| !source.is_empty() && !matches!(*source, "samples" | "local"))
        .map(str::to_string)
}

fn set_settings_source_order_and_cards(ui: &AppWindow, order: &[String]) {
    ui.set_settings_source_order(order.join(", ").into());
    let mut rows = ui.get_settings_source_rows().iter().collect::<Vec<_>>();
    rows.sort_by_key(|row| match row.name.as_str() {
        "samples" => 0,
        "local" => 1,
        name => order
            .iter()
            .position(|source| source.eq_ignore_ascii_case(name))
            .map(|index| index + 2)
            .unwrap_or(usize::MAX),
    });
    ui.set_settings_source_rows(model(rows));
}

fn split_values(value: &str) -> Vec<String> {
    let value = value.trim();
    let value = if (value.starts_with("[\"") && value.ends_with("\"]"))
        || (value.starts_with("['") && value.ends_with("']"))
    {
        &value[1..value.len() - 1]
    } else {
        value
    };
    value
        .split([',', '\n', '\r'])
        .map(|value| value.trim().trim_matches(['"', '\'']).trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_setting<T>(value: &SharedString, label: &str) -> Result<T, String>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value
        .trim()
        .parse::<T>()
        .map_err(|error| format!("Invalid {label}: {error}"))
}

fn parse_optional_setting<T>(value: &SharedString, label: &str) -> Result<Option<T>, String>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let value = value.trim();
    if value.is_empty() {
        Ok(None)
    } else {
        value
            .parse::<T>()
            .map(Some)
            .map_err(|error| format!("Invalid {label}: {error}"))
    }
}

fn parse_minimum_range(value: &str) -> Result<MinimumRangeType, String> {
    match value.trim() {
        "BelowMinimum" => Ok(MinimumRangeType::BelowMinimum),
        "LowerRange" => Ok(MinimumRangeType::LowerRange),
        "Ideal" => Ok(MinimumRangeType::Ideal),
        "UpperRange" => Ok(MinimumRangeType::UpperRange),
        "Ladder" => Ok(MinimumRangeType::Ladder),
        "AboveLadder" => Ok(MinimumRangeType::AboveLadder),
        value => Err(format!("Unsupported minimum range type: {value}")),
    }
}

fn minimum_range_label(value: MinimumRangeType) -> &'static str {
    match value {
        MinimumRangeType::BelowMinimum => "BelowMinimum",
        MinimumRangeType::LowerRange => "LowerRange",
        MinimumRangeType::Ideal => "Ideal",
        MinimumRangeType::UpperRange => "UpperRange",
        MinimumRangeType::Ladder => "Ladder",
        MinimumRangeType::AboveLadder => "AboveLadder",
    }
}

fn optional_number(value: Option<u32>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn credential_from_fields(
    provider: SharedString,
    server_name: SharedString,
    server: SharedString,
    auth: SharedString,
    username: SharedString,
    identifier: SharedString,
    secret: SharedString,
) -> CredentialEditor {
    CredentialEditor {
        provider: provider.to_string(),
        server_name: server_name.to_string(),
        server_url: server.to_string(),
        auth_method: auth.to_string(),
        username: username.to_string(),
        identifier: identifier.to_string(),
        secret: secret.to_string(),
        status: String::new(),
    }
}

fn set_credential_ui(ui: &AppWindow, editor: &CredentialEditor) {
    ui.set_credential_provider(SharedString::from(editor.provider.clone()));
    ui.set_credential_server_name(SharedString::from(editor.server_name.clone()));
    ui.set_credential_server(SharedString::from(editor.server_url.clone()));
    ui.set_credential_auth(SharedString::from(editor.auth_method.clone()));
    ui.set_credential_username(SharedString::from(editor.username.clone()));
    ui.set_credential_identifier(SharedString::from(editor.identifier.clone()));
    ui.set_credential_secret(SharedString::from(editor.secret.clone()));
    ui.set_credential_status(SharedString::from(editor.status.clone()));
}

fn initialize_service_ui(ui: &AppWindow, state: &Arc<Mutex<ApplicationState>>) {
    ui.set_version_label(SharedString::from(format!(
        "v{} · {}",
        env!("CARGO_PKG_VERSION"),
        env!("SPLINED_BUILD_COMMIT")
    )));
    ui.set_packaged_build(splined_core::portable::is_packaged_process().unwrap_or(false));
    if let Ok(app) = state.lock() {
        set_settings_ui(ui, &app.config);
        if !ui.get_setup_required()
            && let Ok(editor) = load_editor(&app.config, "Navidrome")
        {
            set_credential_ui(ui, &editor);
        }
    }
}

#[cfg(windows)]
fn active_window_handle() -> usize {
    let handle = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
    handle.0 as usize
}

#[cfg(not(windows))]
fn active_window_handle() -> usize {
    0
}

fn bind_simple_library_action(
    ui: &AppWindow,
    state: &Arc<Mutex<ApplicationState>>,
    action: fn(&mut ApplicationState),
    which: &'static str,
) {
    let state = Arc::clone(state);
    let weak = ui.as_weak();
    let callback = move || {
        if let Ok(mut app) = state.lock() {
            action(&mut app);
        }
        refresh_weak(&weak, &state);
    };
    match which {
        "all" => ui.on_select_all(callback),
        "filtered" => ui.on_select_filtered(callback),
        _ => ui.on_select_none(callback),
    }
}

fn record_action_result(state: &Arc<Mutex<ApplicationState>>, result: Result<(), String>) {
    if let Err(error) = result
        && let Ok(mut app) = state.lock()
    {
        app.last_error = Some(error.clone());
        app.operation_status = error.clone();
        app.operation_tone = "red".to_string();
        app.push_log(format!("ERROR: {error}"));
    }
}

fn step_adjustment(app: &mut ApplicationState, name: &str, delta: i32) {
    let clamp = |value: &mut i32, min, max| *value = (*value + delta).clamp(min, max);
    match name {
        "Picture" => clamp(&mut app.adjustments.picture_percent, -20, 20),
        "Sharpen" => clamp(&mut app.adjustments.sharpen_percent, -20, 20),
        "Softness" => clamp(&mut app.adjustments.softness_percent, -20, 20),
        "Contrast" => clamp(&mut app.adjustments.contrast_percent, -20, 20),
        "Exposure" => clamp(&mut app.adjustments.exposure_percent, -20, 20),
        "Brightness" => clamp(&mut app.adjustments.brightness_percent, -20, 20),
        "Gamma" => clamp(&mut app.adjustments.gamma_percent, -20, 20),
        "Color" => clamp(&mut app.adjustments.color_temperature, -100, 100),
        "Hue" => clamp(&mut app.adjustments.hue_degrees, -180, 180),
        "Darkness" => clamp(&mut app.adjustments.darkness_percent, 0, 100),
        "Tint" => clamp(&mut app.adjustments.tint_percent, -100, 100),
        _ => {}
    }
}

fn adjustment_bounds(name: &str) -> (i32, i32) {
    match name {
        "Color" => (-100, 100),
        "Hue" => (-180, 180),
        "Darkness" => (0, 100),
        "Tint" => (-100, 100),
        _ => (-20, 20),
    }
}

fn set_adjustment_position(app: &mut ApplicationState, name: &str, position: f32) {
    let (minimum, maximum) = adjustment_bounds(name);
    let position = if position.is_finite() {
        position.clamp(0.0, 1.0)
    } else {
        0.5
    };
    let value = (minimum as f32 + position * (maximum - minimum) as f32).round() as i32;
    match name {
        "Picture" => app.adjustments.picture_percent = value,
        "Sharpen" => app.adjustments.sharpen_percent = value,
        "Softness" => app.adjustments.softness_percent = value,
        "Contrast" => app.adjustments.contrast_percent = value,
        "Exposure" => app.adjustments.exposure_percent = value,
        "Brightness" => app.adjustments.brightness_percent = value,
        "Gamma" => app.adjustments.gamma_percent = value,
        "Color" => app.adjustments.color_temperature = value,
        "Hue" => app.adjustments.hue_degrees = value,
        "Darkness" => app.adjustments.darkness_percent = value,
        "Tint" => app.adjustments.tint_percent = value,
        _ => {}
    }
}

fn relative_adjustment_position(app: &ApplicationState, name: &str, position: f32) -> f32 {
    let names = ["Picture", "Sharpen", "Softness", "Contrast", "Exposure", "Brightness", "Gamma", "Color", "Hue", "Darkness", "Tint"];
    let Some(index) = names.iter().position(|candidate| *candidate == name) else { return 0.5; };
    let (minimum, maximum) = adjustment_bounds(name);
    let span = minimum.abs().max(maximum.abs());
    let baseline = AdjustmentState::from_config(&app.config).slider_values()[index];
    let position = if position.is_finite() { position.clamp(0.0, 1.0) } else { 0.5 };
    let offset = ((position * 2.0 - 1.0) * span as f32).round() as i32;
    let target = (baseline + offset).clamp(minimum, maximum);
    (target - minimum) as f32 / (maximum - minimum) as f32
}

fn reset_adjustment(app: &mut ApplicationState, name: &str) {
    match name {
        "Picture" => app.adjustments.picture_percent = 0,
        "Sharpen" => app.adjustments.sharpen_percent = 0,
        "Softness" => app.adjustments.softness_percent = 0,
        "Contrast" => app.adjustments.contrast_percent = 0,
        "Exposure" => app.adjustments.exposure_percent = 0,
        "Brightness" => app.adjustments.brightness_percent = 0,
        "Gamma" => app.adjustments.gamma_percent = 0,
        "Color" => app.adjustments.color_temperature = 0,
        "Hue" => app.adjustments.hue_degrees = 0,
        "Darkness" => app.adjustments.darkness_percent = 0,
        "Tint" => app.adjustments.tint_percent = 0,
        _ => {}
    }
}

fn reset_enhancement_preview_state(app: &mut ApplicationState) {
    let baseline = AdjustmentState::from_config(&app.config);
    if app.artist_artwork_mode {
        let reset_both = app.artist_theme_art_enabled && app.artist_package_overview_enabled;
        let reset_portrait = reset_both || app.artist_preview_focus == "portrait";
        let reset_banner = reset_both || app.artist_preview_focus == "banner";
        if reset_portrait {
            app.artist_portrait_adjustments = baseline.clone();
            app.artist_portrait_filter_levels = [0; 14];
            app.artist_portrait_preview_path = app.artist_artwork_draft_path.clone();
            app.artist_portrait_zoom = 1.0;
            app.artist_portrait_horizontal = 0.0;
            app.artist_portrait_vertical = 0.0;
        }
        if reset_banner {
            app.artist_banner_adjustments = baseline.clone();
            app.artist_banner_filter_levels = [0; 14];
            app.artist_banner_preview_path = app.artist_banner_source_path.clone();
            app.artist_banner_zoom = auto_artist_banner_zoom(Path::new(&app.artist_banner_source_path));
            app.artist_banner_horizontal = 0.0;
            app.artist_banner_vertical = 0.0;
        }
        if reset_both { app.artist_theme_adjustments = baseline.clone(); app.artist_theme_filter_levels = [0; 14]; app.artist_theme_portrait_preview_path.clear(); app.artist_theme_banner_preview_path.clear(); }
        app.operation_status = "Artist Art preview reset; current saved package is unchanged".to_string();
    } else {
        app.album_filter_levels = [0; 14];
        app.operation_status = "Album Art enhancement preview reset".to_string();
    }
    app.adjustments = baseline;
    app.tint_palette_source.clear();
    app.edit_preview_path.clear();
    app.upscale_preview_only = false;
    app.operation_tone = "green".to_string();
}

fn clear_rendered_preview_for_adjustment(_app: &mut ApplicationState) {
    // A slider drag can queue many renders. Keep the last completed image in
    // all four preview panes until the newest result replaces it. Clearing the
    // path here made the image flash back to its original on every mouse move.
}

fn update_artist_preview_activation(app: &mut ApplicationState, changed: &str) {
    app.artist_portrait_radio_active &= !app.artist_artwork_draft_path.is_empty();
    app.artist_banner_radio_active &= !app.artist_banner_source_path.is_empty();
    app.artist_package_overview_enabled =
        app.artist_portrait_radio_active && app.artist_banner_radio_active;
    if !app.artist_package_overview_enabled {
        app.artist_theme_art_enabled = false;
        app.artist_theme_portrait_preview_path.clear();
        app.artist_theme_banner_preview_path.clear();
        let focus = if app.artist_banner_radio_active {
            "banner"
        } else {
            "portrait"
        };
        switch_artist_preview_focus(app, focus);
    } else if app.artist_theme_art_enabled {
        switch_artist_preview_focus(app, "theme");
    } else {
        switch_artist_preview_focus(app, changed);
    }
}

fn switch_artist_preview_focus(app: &mut ApplicationState, focus: &str) {
    if !app.artist_artwork_mode || app.artist_preview_focus == focus {
        return;
    }
    match app.artist_preview_focus.as_str() {
        "portrait" => app.artist_portrait_adjustments = app.adjustments.clone(),
        "banner" => app.artist_banner_adjustments = app.adjustments.clone(),
        "theme" => app.artist_theme_adjustments = app.adjustments.clone(),
        _ => {}
    }
    app.artist_preview_focus = focus.to_string();
    if focus == "portrait" || focus == "banner" {
        app.artist_last_individual_focus = focus.to_string();
    }
    app.adjustments = match focus {
        "banner" => app.artist_banner_adjustments.clone(),
        "theme" => app.artist_theme_adjustments.clone(),
        _ => app.artist_portrait_adjustments.clone(),
    };
    let preview = match focus {
        "banner" => app.artist_banner_preview_path.clone(),
        "portrait" => app.artist_portrait_preview_path.clone(),
        _ => String::new(),
    };
    app.edit_preview_path = if preview.starts_with("memory-preview://") {
        preview
    } else {
        String::new()
    };
}

fn prepare_embedded_thumbnail_preview(state: &Arc<Mutex<ApplicationState>>) -> Result<(), String> {
    let (source, use_upscale, phase, compilation_track, config, owner) = {
        let app = state
            .lock()
            .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
        if app.artist_artwork_mode {
            return Err("Embedded thumbnails are for album artwork, not Artist Art.".to_string());
        }
        let source = if app.phase == RunPhase::AwaitingMusicBrainz {
            if app.selected_musicbrainz_result.is_some() {
                app.musicbrainz_cover_preview_path.as_str()
            } else {
                app.musicbrainz_existing_artwork_path.as_str()
            }
        } else if let Some(candidate) = app.selected_candidate() {
            candidate.cache_path.as_str()
        } else {
            app.current_cover_path.as_str()
        };
        if source.is_empty() || !Path::new(source).is_file() {
            return Err(
                "Select artwork with a loaded preview before preparing EMBED Thumbs.".to_string(),
            );
        }
        let mut config = app.config.clone();
        project_adjustments(&app, &mut config);
        (
            source.to_string(),
            app.edit_preview_path.starts_with("memory-preview://"),
            app.phase,
            !app.current_compilation_track_path.trim().is_empty(),
            config,
            embedded_thumbnail_owner_key(&app),
        )
    };
    let path = Path::new(&source);
    let candidate = Candidate::from_file("thumbnail", path, 0)?;
    let full_resolution = if use_upscale || phase == RunPhase::AwaitingCandidate {
        let range = Range {
            min: config.range.min,
            ideal: config.range.ideal,
            max: config.range.max,
            ladder: config.range.ladder,
        };
        let mut output = config.output.clone();
        if compilation_track && !use_upscale {
            output.upscale_below_ideal = false;
        }
        if use_upscale {
            prepare_selected_artwork_edit(
                &candidate,
                path,
                &range,
                candidate.format,
                &output,
                true,
            )?
            .bytes
        } else {
            prepare_configured_artwork(&candidate, path, &range, candidate.format, &output, true)?
                .bytes
        }
    } else {
        fs::read(path).map_err(|error| format!("Unable to read selected artwork: {error}"))?
    };
    // Artist Album writes always embed JPEG thumbnails. Preview the same
    // encoding so the projected album total reflects the bytes we will write.
    let format = if compilation_track && candidate.format == StaticFormat::Png {
        StaticFormat::Png
    } else {
        StaticFormat::Jpeg
    };
    let prepared = prepare_embedded_thumbnail(
        &full_resolution,
        config.output.embedded_thumbnail_size,
        format,
    )?;
    let render = PREVIEW_RENDER_REVISION.fetch_add(1, Ordering::SeqCst) + 1;
    let key = format!("memory-preview://embedded-thumbnail/{render}");
    if !preload_scaled_image_bytes(&key, &prepared, PREVIEW_CACHE_DIMENSION) {
        return Err("Unable to load the prepared embedded thumbnail preview.".to_string());
    }
    if let Ok(mut app) = state.lock() {
        if embedded_thumbnail_owner_key(&app) != owner {
            return Ok(());
        }
        store_edit_preview_bytes(&key, &prepared);
        app.embedded_thumbnail_preview_path = key;
        app.embedded_thumbnail_owner_key = owner;
        app.push_log(format!(
            "Prepared an embedded-only {} × {} thumbnail in memory from {}.",
            config.output.embedded_thumbnail_size, config.output.embedded_thumbnail_size, source
        ));
    }
    Ok(())
}

fn embed_album_thumbnails_from_cover(
    album_path: &Path,
    full_resolution: &[u8],
    config: &Config,
) -> Result<usize, String> {
    let inventory = splined_core::scan::inventory_album_directories(album_path, &[])?;
    let album = inventory
        .albums
        .iter()
        .find(|album| album.path == album_path)
        .ok_or_else(|| "The selected Artist Album has no supported audio tracks.".to_string())?;
    let thumbnail = prepare_embedded_thumbnail(
        full_resolution,
        config.output.embedded_thumbnail_size,
        StaticFormat::Jpeg,
    )?;
    let mut written = 0;
    for track in &album.audio_files {
        if !embedded_front_matches(track, &thumbnail).unwrap_or(false) {
            replace_embedded_front(track, &thumbnail)?;
            written += 1;
        }
    }
    invalidate_album_embedded_check(album_path);
    Ok(written)
}

fn edit_existing_cover(
    state: &Arc<Mutex<ApplicationState>>,
    apply: bool,
    guard: Option<(&AtomicU64, u64)>,
) -> Result<(), String> {
    let (path, config, context, artist, album, album_path) = {
        let app = state
            .lock()
            .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
        if apply && compilation_artwork_context(&app) {
            return Err(
                "COMP fallback artwork must be embedded in the selected track; no album cover.* will be written."
                    .to_string(),
            );
        }
        if app.current_cover_path.trim().is_empty() {
            return Err("The current album has no local cover to edit.".to_string());
        }
        let mut config = app.config.clone();
        config.mode = if apply { Mode::Write } else { Mode::Read };
        project_adjustments(&app, &mut config);
        (
            app.current_cover_path.clone(),
            config,
            preview_context(&app),
            app.current_artist.clone(),
            app.current_album.clone(),
            app.current_album_path.clone(),
        )
    };
    if !preview_request_is_current(guard) {
        return Ok(());
    }
    let path = Path::new(&path);
    let candidate = Candidate::from_file("local", path, 0)?;
    // cover.webp is source material, not the target of a direct WRITE Cover.
    // Preserve animation and install a static JPEG beside it even before LAUNCH.
    let target_format = if candidate.format == StaticFormat::Webp {
        StaticFormat::Jpeg
    } else {
        candidate.format
    };
    let destination = if candidate.format == StaticFormat::Webp {
        path.with_extension("jpg")
    } else {
        path.to_path_buf()
    };
    let range = Range {
        min: config.range.min,
        ideal: config.range.ideal,
        max: config.range.max,
        ladder: config.range.ladder,
    };
    let prepared = prepare_existing_cover_edit(
        &candidate,
        path,
        &range,
        target_format,
        &config.output,
        true,
    )?;
    let preview_key = if apply {
        if fs::read(&destination).ok().as_deref() != Some(prepared.bytes.as_slice()) {
            install_prepared_artwork(&destination, &prepared)?;
        }
        remove_saved_sample_album(&config, &artist, &album)?;
        let album_path = Path::new(&album_path);
        if !album_path.is_dir() {
            return Err("The selected Album directory is unavailable.".to_string());
        }
        let mut completion_config = config.clone();
        completion_config.scan.scan_mode_timeout = ScanTimeout(0.0);
        // Refresh this Album's file inventory after a direct write. A runtime
        // outcome alone updates cover_path but leaves cover.webp absent from
        // older local_art_json snapshots, hiding the persistent blue dot.
        record_album_outcome(&completion_config, album_path, "installed", Some("local"))?;
        let path_text = destination.to_string_lossy().into_owned();
        if !preload_scaled_image(&path_text, PREVIEW_CACHE_DIMENSION) {
            return Err(format!(
                "Unable to load the saved existing cover {}.",
                destination.display()
            ));
        }
        path_text
    } else {
        let render = PREVIEW_RENDER_REVISION.fetch_add(1, Ordering::SeqCst) + 1;
        let preview_key = format!("memory-preview://existing-cover/{render}");
        if !preload_scaled_image_bytes(&preview_key, &prepared.bytes, PREVIEW_CACHE_DIMENSION) {
            return Err(
                "Unable to decode the prepared existing-cover preview in memory.".to_string(),
            );
        }
        preview_key
    };
    if !preview_request_is_current(guard) {
        return Ok(());
    }
    let refreshed_snapshot = apply.then(|| media_snapshot(&config, false)).transpose()?;
    if let Ok(mut app) = state.lock() {
        if preview_context(&app) != context || !preview_request_is_current(guard) {
            return Ok(());
        }
        if let Some(snapshot) = refreshed_snapshot {
            app.library.replace_snapshot(snapshot)?;
            app.service_status = "Cover written; Album marked Processed.".to_string();
            app.current_cover_path = destination.to_string_lossy().into_owned();
        }
        // The saved file is now the selected album's cover. Drop the transient
        // edit preview so an enabled animated WebP can resume immediately.
        if !apply {
            store_edit_preview_bytes(&preview_key, &prepared.bytes);
        }
        app.edit_preview_path = if apply { String::new() } else { preview_key };
        if apply {
            app.embedded_thumbnail_preview_path.clear();
            app.embedded_thumbnail_owner_key.clear();
        }
        app.push_log(if apply {
            format!(
                "Edited existing cover and marked the Album Processed: {}",
                destination.display()
            )
        } else {
            "Prepared existing-cover preview in memory without changing album files.".to_string()
        });
    }
    Ok(())
}

fn schedule_adjustment_preview(
    state: Arc<Mutex<ApplicationState>>,
    weak: slint::Weak<AppWindow>,
    revision: Arc<AtomicU64>,
    gate: Arc<Mutex<()>>,
) {
    let ticket = revision.fetch_add(1, Ordering::SeqCst) + 1;
    let thumbnail_mode = state.lock().is_ok_and(|app| embedded_thumbnail_ready(&app));
    if let Ok(mut app) = state.lock() {
        app.operation_busy = true;
        app.operation_status = "Rendering Upscale Preview in memory…".to_string();
        app.operation_tone = "violet".to_string();
    }
    refresh_weak(&weak, &state);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        if revision.load(Ordering::SeqCst) != ticket {
            return;
        }
        let Ok(_guard) = gate.lock() else {
            return;
        };
        if revision.load(Ordering::SeqCst) != ticket {
            return;
        }
        let preview_source = state.lock().ok().map(|app| {
            if app.artist_artwork_mode && app.artist_preview_focus == "theme" {
                "artist-theme"
            } else if app.artist_artwork_mode {
                "candidate"
            } else if app.phase == RunPhase::AwaitingMusicBrainz
                && app.selected_musicbrainz_result.is_some()
                && !app.musicbrainz_cover_preview_path.trim().is_empty()
            {
                "musicbrainz"
            } else if app.phase == RunPhase::AwaitingMusicBrainz
                && app.selected_musicbrainz_result.is_none()
                && !app.musicbrainz_existing_artwork_path.trim().is_empty()
            {
                "musicbrainz-existing"
            } else if app.selected_candidate().is_some() {
                "candidate"
            } else if app.active_compilation_track_preview().is_some() {
                "compilation-track"
            } else {
                "current-cover"
            }
        });
        let mut result = match preview_source {
            Some("artist-theme") => preview_artist_theme_package(&state, Some((&revision, ticket))),
            Some("candidate") => preview_selected_candidate(&state, Some((&revision, ticket))),
            Some("musicbrainz") => preview_musicbrainz_cover(&state, Some((&revision, ticket))),
            Some("musicbrainz-existing") => {
                preview_musicbrainz_existing(&state, Some((&revision, ticket)))
            }
            Some("compilation-track") => {
                preview_compilation_track(&state, Some((&revision, ticket)))
            }
            _ => edit_existing_cover(&state, false, Some((&revision, ticket))),
        };
        if result.is_ok() && thumbnail_mode && revision.load(Ordering::SeqCst) == ticket {
            result = prepare_embedded_thumbnail_preview(&state);
        }
        if revision.load(Ordering::SeqCst) != ticket {
            return;
        }
        let succeeded = result.is_ok();
        let _ = slint::invoke_from_event_loop(move || {
            if let Ok(mut app) = state.lock() {
                app.operation_busy = false;
                if succeeded {
                    app.upscale_preview_only = false;
                    app.adjustments.preserve_source_resolution = false;
                    app.adjustments.apply_edit_profile = true;
                    app.operation_status = if thumbnail_mode {
                        "Upscale Preview applied to EMBED Thumbs · ready to write".to_string()
                    } else {
                        "Upscale Preview ready".to_string()
                    };
                    app.operation_tone = "green".to_string();
                    app.push_log(
                        "Upscale Preview is ready for the selected write action.".to_string(),
                    );
                } else {
                    app.operation_status = "Upscale Preview failed".to_string();
                    app.operation_tone = "red".to_string();
                }
            }
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    });
}

fn schedule_settings_adjustment_preview(
    state: Arc<Mutex<ApplicationState>>,
    weak: slint::Weak<AppWindow>,
    revision: Arc<AtomicU64>,
    gate: Arc<Mutex<()>>,
) {
    let ticket = revision.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        if revision.load(Ordering::SeqCst) != ticket {
            return;
        }
        let Ok(_guard) = gate.lock() else {
            return;
        };
        if revision.load(Ordering::SeqCst) != ticket {
            return;
        }
        let config = state.lock().map(|app| {
            let mut config = app.config.clone();
            project_adjustments(&app, &mut config);
            config
        });
        let result = config
            .map_err(|_| "The SPLINED application state is unavailable.".to_string())
            .and_then(|config| render_settings_preview(&config));
        if revision.load(Ordering::SeqCst) != ticket {
            return;
        }
        let _ = slint::invoke_from_event_loop(move || match result {
            Ok((source, adjusted)) => {
                if let Some(ui) = weak.upgrade() {
                    ui.set_settings_original_preview(load_cached_preview_image(&source, ""));
                    ui.set_settings_current_preview(load_cached_preview_image(&adjusted, &source));
                }
            }
            Err(error) => {
                record_action_result(&state, Err(error));
                refresh_weak(&weak, &state);
            }
        });
    });
}

fn preview_artist_theme_package(
    state: &Arc<Mutex<ApplicationState>>,
    guard: Option<(&AtomicU64, u64)>,
) -> Result<(), String> {
    let (portrait, banner, portrait_output, banner_output, theme_output, portrait_fit, banner_fit, context) = {
        let app = state
            .lock()
            .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
        if app.artist_artwork_draft_path.is_empty() || app.artist_banner_source_path.is_empty() {
            return Err("Select both an Artist image and banner for Theme Art.".to_string());
        }
        let mut portrait_config = app.config.clone();
        let mut banner_config = app.config.clone();
        let mut theme_config = app.config.clone();
        project_adjustment_state(&app.artist_portrait_adjustments, &mut portrait_config);
        project_adjustment_state(&app.artist_banner_adjustments, &mut banner_config);
        project_adjustments(&app, &mut theme_config);
        (
            app.artist_artwork_draft_path.clone(),
            app.artist_banner_source_path.clone(),
            portrait_config.output,
            banner_config.output,
            theme_config.output,
            (app.artist_portrait_zoom, app.artist_portrait_horizontal, app.artist_portrait_vertical),
            (app.artist_banner_zoom, app.artist_banner_horizontal, app.artist_banner_vertical),
            preview_context(&app),
        )
    };
    if !preview_request_is_current(guard) {
        return Ok(());
    }
    let portrait_bytes =
        fs::read(&portrait).map_err(|error| format!("Unable to read Artist image: {error}"))?;
    let banner_bytes =
        fs::read(&banner).map_err(|error| format!("Unable to read Artist banner: {error}"))?;
    let portrait_base = prepare_artist_art_png_with_fit(&portrait_bytes, Some(&portrait_output), portrait_fit.0, portrait_fit.1, portrait_fit.2)?;
    let portrait_image = image::load_from_memory(&portrait_base)
        .map_err(|error| format!("Unable to decode Artist image: {error}"))?;
    let portrait_image = apply_selected_image_adjustments(portrait_image, &theme_output);
    let banner_image = enhanced_artist_banner_with_position(&banner_bytes, banner_fit.0, banner_fit.1, banner_fit.2, &banner_output)?;
    let banner_image = apply_selected_image_adjustments(banner_image, &theme_output);
    let mut portrait_encoded = Cursor::new(Vec::new());
    portrait_image
        .write_to(&mut portrait_encoded, image::ImageFormat::Png)
        .map_err(|error| format!("Unable to render themed Artist image: {error}"))?;
    let mut banner_encoded = Cursor::new(Vec::new());
    banner_image
        .write_to(&mut banner_encoded, image::ImageFormat::Png)
        .map_err(|error| format!("Unable to render themed Artist banner: {error}"))?;
    let render = PREVIEW_RENDER_REVISION.fetch_add(1, Ordering::SeqCst) + 1;
    let portrait_key = format!("memory-preview://artist-theme/portrait/{render}");
    let banner_key = format!("memory-preview://artist-theme/banner/{render}");
    let portrait_bytes = portrait_encoded.into_inner();
    let banner_bytes = banner_encoded.into_inner();
    // The two images form one package. Pin both keys before inserting either
    // bitmap so the bounded cache cannot evict the portrait on banner insert.
    if let Ok(mut active) = active_preview_cache_keys().lock() {
        active.insert(format!("{portrait_key}\u{1f}{PREVIEW_CACHE_DIMENSION}"));
        active.insert(format!("{banner_key}\u{1f}{PREVIEW_CACHE_DIMENSION}"));
    }
    if !preload_scaled_image_bytes(&portrait_key, &portrait_bytes, PREVIEW_CACHE_DIMENSION)
        || !preload_scaled_image_bytes(&banner_key, &banner_bytes, PREVIEW_CACHE_DIMENSION)
    {
        return Err("Unable to decode the themed Artist Art package.".to_string());
    }
    if let Ok(mut app) = state.lock() {
        if preview_context(&app) != context || !preview_request_is_current(guard) {
            return Ok(());
        }
        store_edit_preview_bytes(&portrait_key, &portrait_bytes);
        store_edit_preview_bytes(&banner_key, &banner_bytes);
        app.artist_theme_portrait_preview_path = portrait_key;
        app.artist_theme_banner_preview_path = banner_key;
        app.push_log("Rendered themed Artist Art package in memory; no files changed.".to_string());
    }
    Ok(())
}

fn preview_selected_candidate(
    state: &Arc<Mutex<ApplicationState>>,
    guard: Option<(&AtomicU64, u64)>,
) -> Result<(), String> {
    let (index, source, source_path, config, context, banner_framing, portrait_framing, artist_artwork_mode) = {
        let app = state
            .lock()
            .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
        let selected = app.selected_candidate();
        let (index, source, source_path, banner_framing) = if app.artist_artwork_mode {
            if app.artist_preview_focus == "banner" {
                if app.artist_banner_source_path.is_empty() {
                    return Err("Preview a banner before adjusting it.".to_string());
                }
                (
                    app.artist_banner_preview_candidate_index
                        .unwrap_or_default(),
                    "ArtistBanner".to_string(),
                    app.artist_banner_source_path.clone(),
                    Some((app.artist_banner_zoom, app.artist_banner_horizontal, app.artist_banner_vertical)),
                )
            } else {
                if app.artist_artwork_draft_path.is_empty() {
                    return Err("Preview an artist image before adjusting it.".to_string());
                }
                (
                    app.artist_portrait_preview_candidate_index
                        .unwrap_or_default(),
                    "ArtistImage".to_string(),
                    app.artist_artwork_draft_path.clone(),
                    None,
                )
            }
        } else {
            let selected = selected.ok_or_else(|| {
                "Select an artwork candidate before requesting a preview.".to_string()
            })?;
            if selected.cache_path.trim().is_empty() {
                return Err("The selected artwork candidate has no cached image.".to_string());
            }
            (
                selected.index,
                selected.source.clone(),
                selected.cache_path.clone(),
                None,
            )
        };
        let mut config = app.config.clone();
        config.mode = Mode::Read;
        project_adjustments(&app, &mut config);
        (
            index,
            source.clone(),
            source_path,
            config,
            preview_context(&app),
            banner_framing.or_else(|| {
                selected
                    .filter(|selected| selected.image_type == "ArtistBanner")
                    .map(|_| (app.artist_banner_zoom, app.artist_banner_horizontal, app.artist_banner_vertical))
            }),
            (app.artist_portrait_zoom, app.artist_portrait_horizontal, app.artist_portrait_vertical),
            app.artist_artwork_mode,
        )
    };
    if !preview_request_is_current(guard) {
        return Ok(());
    }
    let source_path = Path::new(&source_path);
    if let Some((zoom, horizontal, vertical)) = banner_framing {
        let source_bytes = fs::read(source_path)
            .map_err(|error| format!("Unable to read the banner preview: {error}"))?;
        let mut encoded = Cursor::new(Vec::new());
        enhanced_artist_banner_with_position(&source_bytes, zoom, horizontal, vertical, &config.output)?
            .write_to(&mut encoded, image::ImageFormat::Png)
            .map_err(|error| format!("Unable to render the banner preview: {error}"))?;
        let bytes = encoded.into_inner();
        let render = PREVIEW_RENDER_REVISION.fetch_add(1, Ordering::SeqCst) + 1;
        let preview_key = format!("memory-preview://artist-banner/{index}/{render}");
        if !preload_scaled_image_bytes(&preview_key, &bytes, PREVIEW_CACHE_DIMENSION) {
            return Err("Unable to decode the banner Upscale Preview in memory.".to_string());
        }
        if let Ok(mut app) = state.lock() {
            if preview_context(&app) != context || !preview_request_is_current(guard) {
                return Ok(());
            }
            store_edit_preview_bytes(&preview_key, &bytes);
            app.edit_preview_path = preview_key.clone();
            if !app.artist_banner_source_path.is_empty() {
                app.artist_banner_preview_path = preview_key;
            }
            app.push_log(format!(
                "Rendered Artist Banner preview at 1400 × 600 (zoom {zoom:.2}, vertical {vertical:.2}); no banner file changed."
            ));
        }
        return Ok(());
    }
    if artist_artwork_mode {
        let source_bytes = fs::read(source_path)
            .map_err(|error| format!("Unable to read Artist Art preview: {error}"))?;
        let bytes = prepare_artist_art_png_with_fit(&source_bytes, Some(&config.output), portrait_framing.0, portrait_framing.1, portrait_framing.2)?;
        let render = PREVIEW_RENDER_REVISION.fetch_add(1, Ordering::SeqCst) + 1;
        let preview_key = format!("memory-preview://artist-art/{index}/{render}");
        if !preload_scaled_image_bytes(&preview_key, &bytes, PREVIEW_CACHE_DIMENSION) {
            return Err("Unable to decode the Artist Art preview in memory.".to_string());
        }
        if let Ok(mut app) = state.lock() {
            if preview_context(&app) != context || !preview_request_is_current(guard) {
                return Ok(());
            }
            store_edit_preview_bytes(&preview_key, &bytes);
            app.edit_preview_path = preview_key.clone();
            app.artist_portrait_preview_path = preview_key;
            app.push_log(format!(
                "Rendered enhanced Artist Art candidate {index} without changing saved artwork."
            ));
        }
        return Ok(());
    }
    let candidate = Candidate::from_file(source, source_path, index)?;
    let range = Range {
        min: config.range.min,
        ideal: config.range.ideal,
        max: config.range.max,
        ladder: config.range.ladder,
    };
    let prepared = prepare_selected_artwork_edit(
        &candidate,
        source_path,
        &range,
        candidate.format,
        &config.output,
        true,
    )?;
    let render = PREVIEW_RENDER_REVISION.fetch_add(1, Ordering::SeqCst) + 1;
    let preview_key = format!("memory-preview://candidate/{index}/{render}");
    if !preload_scaled_image_bytes(&preview_key, &prepared.bytes, PREVIEW_CACHE_DIMENSION) {
        return Err("Unable to decode the candidate Upscale Preview in memory.".to_string());
    }
    if !preview_request_is_current(guard) {
        return Ok(());
    }
    if let Ok(mut app) = state.lock() {
        if preview_context(&app) != context || !preview_request_is_current(guard) {
            return Ok(());
        }
        store_edit_preview_bytes(&preview_key, &prepared.bytes);
        app.edit_preview_path = preview_key;
        app.push_log(format!(
            "Rendered candidate {index} preview in memory at {} × {} without changing album files.",
            prepared.info.width, prepared.info.height
        ));
    }
    Ok(())
}

fn preview_compilation_track(
    state: &Arc<Mutex<ApplicationState>>,
    guard: Option<(&AtomicU64, u64)>,
) -> Result<(), String> {
    let (source_path, config, context) = {
        let app = state
            .lock()
            .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
        let source_path = app
            .active_compilation_track_preview()
            .ok_or_else(|| "The selected compilation track has no embedded artwork.".to_string())?
            .to_string();
        let mut config = app.config.clone();
        config.mode = Mode::Read;
        project_adjustments(&app, &mut config);
        (source_path, config, preview_context(&app))
    };
    if !preview_request_is_current(guard) {
        return Ok(());
    }
    let source_path = Path::new(&source_path);
    let candidate = Candidate::from_file("embedded", source_path, 0)?;
    let range = Range {
        min: config.range.min,
        ideal: config.range.ideal,
        max: config.range.max,
        ladder: config.range.ladder,
    };
    let prepared = prepare_selected_artwork_edit(
        &candidate,
        source_path,
        &range,
        candidate.format,
        &config.output,
        true,
    )?;
    let render = PREVIEW_RENDER_REVISION.fetch_add(1, Ordering::SeqCst) + 1;
    let preview_key = format!("memory-preview://compilation-track/{render}");
    if !preload_scaled_image_bytes(&preview_key, &prepared.bytes, PREVIEW_CACHE_DIMENSION) {
        return Err(
            "Unable to decode the compilation-track Upscale Preview in memory.".to_string(),
        );
    }
    if !preview_request_is_current(guard) {
        return Ok(());
    }
    if let Ok(mut app) = state.lock() {
        if preview_context(&app) != context || !preview_request_is_current(guard) {
            return Ok(());
        }
        store_edit_preview_bytes(&preview_key, &prepared.bytes);
        app.edit_preview_path = preview_key;
        app.push_log(format!(
            "Rendered the selected compilation track preview in memory at {} × {} without changing album files.",
            prepared.info.width, prepared.info.height
        ));
    }
    Ok(())
}

fn preview_musicbrainz_cover(
    state: &Arc<Mutex<ApplicationState>>,
    guard: Option<(&AtomicU64, u64)>,
) -> Result<(), String> {
    let (index, source_path, config, context) = {
        let app = state
            .lock()
            .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
        if app.phase != RunPhase::AwaitingMusicBrainz {
            return Err("Select a MusicBrainz release before requesting a preview.".to_string());
        }
        let index = app.selected_musicbrainz_result.ok_or_else(|| {
            "Select a MusicBrainz release before requesting a preview.".to_string()
        })?;
        let source_path = app.musicbrainz_cover_preview_path.trim().to_string();
        if source_path.is_empty() {
            return Err("The selected MusicBrainz cover is still loading.".to_string());
        }
        let mut config = app.config.clone();
        config.mode = Mode::Read;
        project_adjustments(&app, &mut config);
        (index, source_path, config, preview_context(&app))
    };
    if !preview_request_is_current(guard) {
        return Ok(());
    }
    let source_path = Path::new(&source_path);
    let candidate = Candidate::from_file("musicbrainz", source_path, index)?;
    let range = Range {
        min: config.range.min,
        ideal: config.range.ideal,
        max: config.range.max,
        ladder: config.range.ladder,
    };
    let prepared = prepare_selected_artwork_edit(
        &candidate,
        source_path,
        &range,
        candidate.format,
        &config.output,
        true,
    )?;
    let render = PREVIEW_RENDER_REVISION.fetch_add(1, Ordering::SeqCst) + 1;
    let preview_key = format!("memory-preview://musicbrainz/{index}/{render}");
    if !preload_scaled_image_bytes(&preview_key, &prepared.bytes, PREVIEW_CACHE_DIMENSION) {
        return Err("Unable to decode the MusicBrainz Upscale Preview in memory.".to_string());
    }
    if !preview_request_is_current(guard) {
        return Ok(());
    }
    if let Ok(mut app) = state.lock() {
        if preview_context(&app) != context || !preview_request_is_current(guard) {
            return Ok(());
        }
        store_edit_preview_bytes(&preview_key, &prepared.bytes);
        app.edit_preview_path = preview_key;
        app.push_log(format!(
            "Rendered MusicBrainz release {index} preview in memory at {} × {} without changing album files.",
            prepared.info.width, prepared.info.height
        ));
    }
    Ok(())
}

fn preview_musicbrainz_existing(
    state: &Arc<Mutex<ApplicationState>>,
    guard: Option<(&AtomicU64, u64)>,
) -> Result<(), String> {
    let (source_path, config, context) = {
        let app = state
            .lock()
            .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
        if app.phase != RunPhase::AwaitingMusicBrainz
            || app.selected_musicbrainz_result.is_some()
            || app.musicbrainz_existing_artwork_path.trim().is_empty()
        {
            return Err(
                "Select Existing artwork before requesting an Upscale Preview.".to_string(),
            );
        }
        let mut config = app.config.clone();
        config.mode = Mode::Read;
        project_adjustments(&app, &mut config);
        (
            app.musicbrainz_existing_artwork_path.clone(),
            config,
            preview_context(&app),
        )
    };
    if !preview_request_is_current(guard) {
        return Ok(());
    }
    let source_path = Path::new(&source_path);
    let candidate = Candidate::from_file("existing", source_path, 0)?;
    let range = Range {
        min: config.range.min,
        ideal: config.range.ideal,
        max: config.range.max,
        ladder: config.range.ladder,
    };
    let prepared = prepare_selected_artwork_edit(
        &candidate,
        source_path,
        &range,
        candidate.format,
        &config.output,
        true,
    )?;
    let render = PREVIEW_RENDER_REVISION.fetch_add(1, Ordering::SeqCst) + 1;
    let preview_key = format!("memory-preview://musicbrainz-existing/{render}");
    if !preload_scaled_image_bytes(&preview_key, &prepared.bytes, PREVIEW_CACHE_DIMENSION) {
        return Err("Unable to decode the Existing Upscale Preview in memory.".to_string());
    }
    if !preview_request_is_current(guard) {
        return Ok(());
    }
    if let Ok(mut app) = state.lock() {
        if preview_context(&app) != context || !preview_request_is_current(guard) {
            return Ok(());
        }
        store_edit_preview_bytes(&preview_key, &prepared.bytes);
        app.edit_preview_path = preview_key;
        app.push_log(format!(
            "Rendered Existing artwork preview in memory at {} × {} without changing album files.",
            prepared.info.width, prepared.info.height
        ));
    }
    Ok(())
}

fn apply_selected_sample(state: &Arc<Mutex<ApplicationState>>) -> Result<(), String> {
    let (
        config,
        source_path,
        album_path,
        current_cover,
        sample_album_dir,
        artist,
        album,
        album_key,
    ) = {
        let app = state
            .lock()
            .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
        let selected = app
            .selected_candidate()
            .filter(|candidate| candidate.source.eq_ignore_ascii_case("samples"))
            .ok_or_else(|| "Select a saved sample before applying it.".to_string())?;
        let source_path = PathBuf::from(&selected.cache_path);
        let sample_album_dir = source_path
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "The selected sample has no album directory.".to_string())?;
        let mut config = app.config.clone();
        config.mode = Mode::Write;
        project_adjustments(&app, &mut config);
        (
            config,
            source_path,
            PathBuf::from(&app.current_album_path),
            PathBuf::from(&app.current_cover_path),
            sample_album_dir,
            app.current_artist.clone(),
            app.current_album.clone(),
            app.library.focused_album_key.clone(),
        )
    };
    if !source_path.is_file() {
        return Err(format!(
            "The selected sample no longer exists: {}",
            source_path.display()
        ));
    }
    if !album_path.is_dir() {
        return Err("The selected album directory is unavailable.".to_string());
    }
    let sample_root = configured_samples_dir(&config)
        .canonicalize()
        .map_err(|error| format!("Unable to verify the sample directory: {error}"))?;
    let verified_album_dir = sample_album_dir
        .canonicalize()
        .map_err(|error| format!("Unable to verify the album sample directory: {error}"))?;
    if !verified_album_dir.starts_with(&sample_root) || verified_album_dir == sample_root {
        return Err(
            "Refusing to tidy a sample directory outside the configured Samples root.".to_string(),
        );
    }

    let candidate = Candidate::from_file("samples", &source_path, 0)?;
    let target_format = if current_cover.is_file() {
        inspect_image(&current_cover)
            .map(|info| info.format)
            .unwrap_or(candidate.format)
    } else {
        candidate.format
    };
    let range = Range {
        min: config.range.min,
        ideal: config.range.ideal,
        max: config.range.max,
        ladder: config.range.ladder,
    };
    let prepared = prepare_selected_artwork_edit(
        &candidate,
        &source_path,
        &range,
        target_format,
        &config.output,
        true,
    )?;
    let destination = if current_cover.is_file() {
        current_cover
    } else {
        let extension = match prepared.info.format {
            StaticFormat::Jpeg => "jpg",
            StaticFormat::Png => "png",
            StaticFormat::Webp => "webp",
        };
        album_path.join(format!("{}.{}", config.output.file_name, extension))
    };
    install_prepared_artwork(&destination, &prepared)?;
    remove_saved_sample_album(&config, &artist, &album)?;
    let material = RuntimeArtworkMaterial {
        path: destination.clone(),
        format: format!("{:?}", prepared.info.format).to_ascii_uppercase(),
        width: prepared.info.width,
        height: prepared.info.height,
    };
    let mut completion_config = config.clone();
    completion_config.scan.scan_mode_timeout = ScanTimeout(0.0);
    record_album_outcome_from_runtime(
        &completion_config,
        &album_path,
        Some(&album_path),
        album_key.as_deref(),
        "installed",
        Some("samples"),
        Some(&material),
    )?;

    let snapshot = media_snapshot(&config, false)?;
    if let Ok(mut app) = state.lock() {
        app.library.replace_snapshot(snapshot)?;
        app.current_cover_path = destination.to_string_lossy().into_owned();
        app.edit_preview_path.clear();
        app.candidates.clear();
        app.selected_candidate = None;
        app.comparison_candidates.clear();
        app.upscale_preview_only = true;
        if let Some(source) = app
            .sources
            .iter_mut()
            .find(|source| source.name.eq_ignore_ascii_case("samples"))
        {
            source.candidate_count = 0;
            source.status = "No images".to_string();
        }
        app.push_log(format!(
            "Applied saved sample to {} and removed the remaining samples for this album.",
            destination.display()
        ));
        app.service_status = "Saved sample applied; Album marked Processed.".to_string();
    }
    Ok(())
}

fn remove_saved_sample_album(config: &Config, artist: &str, album: &str) -> Result<bool, String> {
    let sample_root = configured_samples_dir(config);
    let album_dir = sample_root
        .join(sanitize_sample_component(artist))
        .join(sanitize_sample_component(album));
    if !album_dir.exists() {
        return Ok(false);
    }
    let verified_root = sample_root.canonicalize().map_err(|error| {
        format!(
            "Unable to verify the configured Samples root {}: {error}",
            sample_root.display()
        )
    })?;
    let verified_album = album_dir.canonicalize().map_err(|error| {
        format!(
            "Unable to verify the album Samples directory {}: {error}",
            album_dir.display()
        )
    })?;
    if verified_album == verified_root || !verified_album.starts_with(&verified_root) {
        return Err(
            "Refusing to remove an album Samples directory outside the configured root."
                .to_string(),
        );
    }
    let artist_dir = verified_album.parent().map(Path::to_path_buf);
    fs::remove_dir_all(&verified_album).map_err(|error| {
        format!(
            "Unable to remove album Samples directory {}: {error}",
            verified_album.display()
        )
    })?;
    if let Some(artist_dir) = artist_dir
        && artist_dir != verified_root
    {
        let _ = fs::remove_dir(&artist_dir);
    }
    Ok(true)
}

fn sanitize_sample_component(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());
    for character in value.trim().chars() {
        if character.is_control()
            || matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            )
        {
            sanitized.push('_');
        } else {
            sanitized.push(character);
        }
    }
    let sanitized = sanitized.trim().trim_end_matches([' ', '.']).to_string();
    if sanitized.is_empty() {
        "unknown".to_string()
    } else {
        sanitized
    }
}

fn project_adjustments(app: &ApplicationState, config: &mut Config) {
    project_adjustment_state(&app.adjustments, config);
}

fn project_adjustment_state(adjustments: &AdjustmentState, config: &mut Config) {
    config.output.upscale_adaptive_defaults = adjustments.adaptive_defaults;
    config.output.upscale_picture_percent = adjustments.picture_percent;
    config.output.upscale_sharpen_percent = adjustments.sharpen_percent;
    config.output.upscale_softness_percent = adjustments.softness_percent;
    config.output.upscale_contrast_percent = adjustments.contrast_percent;
    config.output.upscale_exposure_percent = adjustments.exposure_percent;
    config.output.upscale_brightness_percent = adjustments.brightness_percent;
    config.output.upscale_gamma_percent = adjustments.gamma_percent;
    config.output.upscale_color_temperature = adjustments.color_temperature;
    config.output.upscale_hue_degrees = adjustments.hue_degrees;
    config.output.upscale_darkness_percent = adjustments.darkness_percent;
    config.output.upscale_tint_percent = adjustments.tint_percent;
    config.output.upscale_tint_color = adjustments.tint_color.clone();
}

fn unsaved_adjustment_defaults(app: &ApplicationState) -> bool {
    let mut projected = app.config.clone();
    project_adjustments(app, &mut projected);
    projected.output != app.config.output
}

fn associated_banner_candidate<'a>(
    candidate: &CandidateState,
    candidates: &'a [CandidateState],
) -> Option<&'a CandidateState> {
    if candidate.image_type == "ArtistBanner" {
        return candidates.iter().find(|row| row.index == candidate.index);
    }
    candidates.iter().find(|row| {
        row.image_type == "ArtistBanner" && row.source.eq_ignore_ascii_case(&candidate.source)
    })
}

fn candidate_ui_row(candidate: &CandidateState, compared: bool) -> UiCandidateRow {
    UiCandidateRow {
        index: candidate.index as i32,
        source: SharedString::from(source_display_name(&candidate.source)),
        dimensions: SharedString::from(candidate.dimensions.clone()),
        range_class: SharedString::from(candidate.range_class.clone()),
        policy: SharedString::from(if candidate.policy_reason.is_empty() {
            candidate.policy_status.clone()
        } else {
            format!("{} · {}", candidate.policy_status, candidate.policy_reason)
        }),
        image_type: SharedString::from(candidate.image_type.clone()),
        selected: candidate.selected,
        recommended: candidate.recommended,
        compared,
        portrait_available: candidate.image_type != "ArtistBanner",
        portrait_active: false,
        banner_available: candidate.image_type == "ArtistBanner",
        banner_active: false,
        preview: load_scaled_image(&candidate.cache_path, 320),
    }
}

fn sync_candidate_compare(window: &CandidateCompareWindow, state: &Arc<Mutex<ApplicationState>>) {
    let Ok(app) = state.lock() else {
        return;
    };
    window.set_candidate_rows(model(
        app.comparison_candidates()
            .into_iter()
            .map(|candidate| candidate_ui_row(candidate, true))
            .collect(),
    ));
}

fn is_valid_mbid(value: &str) -> bool {
    let bytes = value.trim().as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

fn percent_encode_query(value: &str) -> String {
    use std::fmt::Write as _;
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(*byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn open_external_url(url: &str) -> Result<(), String> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{PCWSTR, w};
    if !(url.starts_with("https://") || url.starts_with("http://"))
        || url.chars().any(char::is_control)
    {
        return Err("The external page URL is invalid.".to_string());
    }
    let wide_url = url
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // ShellExecute uses the registered URL protocol handler. explorer.exe
    // interprets encoded MusicBrainz search URLs as file paths on Windows.
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide_url.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if (result.0 as isize) <= 32 {
        Err(format!(
            "Windows could not open the external page (shell error {}).",
            result.0 as isize
        ))
    } else {
        Ok(())
    }
}

fn musicbrainz_failed_url(app: &ApplicationState) -> String {
    let artist_id = app.musicbrainz_focused_artist_mbid.trim();
    if app.musicbrainz_artist_status == "valid" && is_valid_mbid(artist_id) {
        return format!("https://musicbrainz.org/artist/{artist_id}");
    }
    let artist = (!app.fallback_artist.trim().is_empty())
        .then_some(app.fallback_artist.as_str())
        .or_else(|| {
            app.library
                .focused_artist
                .as_deref()
                .filter(|name| !name.trim().is_empty())
        })
        .or_else(|| {
            (!app.musicbrainz_friendly_artist.trim().is_empty())
                .then_some(app.musicbrainz_friendly_artist.as_str())
        })
        .unwrap_or("");
    format!(
        "https://musicbrainz.org/search?query={}&type=artist&method=indexed",
        percent_encode_query(artist)
    )
}

fn begin_musicbrainz_preview_request(
    app: &mut ApplicationState,
    selected_index: usize,
    hover_preview: bool,
) -> Option<String> {
    if app.musicbrainz_preview_cache.contains_key(&selected_index)
        || app.musicbrainz_missing_cover.contains(&selected_index)
        || app.musicbrainz_preview_inflight.contains(&selected_index)
    {
        return None;
    }
    let release_mbid = app
        .musicbrainz_results
        .iter()
        .find(|item| item.index == selected_index && !item.current)
        .map(|item| item.release_mbid.clone())
        .filter(|value| is_valid_mbid(value))?;
    app.musicbrainz_preview_inflight.insert(selected_index);
    if hover_preview {
        app.musicbrainz_hover_preview_loading = Some(selected_index);
    }
    Some(release_mbid)
}

fn refresh_musicbrainz_cover_preview(
    state: Arc<Mutex<ApplicationState>>,
    weak: slint::Weak<AppWindow>,
    selected_index: usize,
    hover_preview: bool,
) {
    let release_mbid = {
        let Ok(mut app) = state.lock() else {
            return;
        };
        begin_musicbrainz_preview_request(&mut app, selected_index, hover_preview)
    };
    let Some(release_mbid) = release_mbid else {
        return;
    };

    if let Some(log) = UI_DEBUG_LOG_HANDLE.get() {
        log.record(format!(
            "musicbrainz-preview-start index={selected_index} release={release_mbid}"
        ));
    }

    std::thread::spawn(move || {
        let started = Instant::now();
        let preview = (|| -> Result<PathBuf, String> {
            let layout = splined_core::portable::app_layout()?;
            let cache = DownloadCache::new_persistent(
                &layout
                    .root
                    .join("_cache")
                    .join("musicbrainz-cover-previews"),
            )?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| {
                    format!("Unable to start the MusicBrainz preview runtime: {error}")
                })?;
            let front_500 = format!("https://coverartarchive.org/release/{release_mbid}/front-500");
            let front = format!("https://coverartarchive.org/release/{release_mbid}/front");
            let downloaded = runtime.block_on(async {
                match cache
                    .download_candidate("coverartarchive", &front_500, 0)
                    .await
                {
                    Ok(candidate) => Ok(candidate),
                    Err(_) => cache.download_candidate("coverartarchive", &front, 0).await,
                }
            })?;
            Ok(downloaded.path().to_path_buf())
        })();

        if let Some(log) = UI_DEBUG_LOG_HANDLE.get() {
            log.record(format!(
                "musicbrainz-preview-finished index={selected_index} elapsed_ms={} success={}",
                started.elapsed().as_millis(),
                preview.is_ok()
            ));
        }

        let _ = slint::invoke_from_event_loop(move || {
            if let Ok(mut app) = state.lock() {
                app.musicbrainz_preview_inflight.remove(&selected_index);
                let selected_same_result = app.selected_musicbrainz_result == Some(selected_index);
                let still_requested = if hover_preview {
                    app.hovered_musicbrainz_result == Some(selected_index) || selected_same_result
                } else {
                    selected_same_result
                };
                match preview {
                    Ok(path) => {
                        app.musicbrainz_missing_cover.remove(&selected_index);
                        let path = path.to_string_lossy().into_owned();
                        app.musicbrainz_preview_cache
                            .insert(selected_index, path.clone());
                        if hover_preview && still_requested {
                            app.musicbrainz_hover_preview_path.clone_from(&path);
                            app.musicbrainz_hover_preview_index = Some(selected_index);
                            if selected_same_result {
                                app.musicbrainz_cover_preview_path.clone_from(&path);
                                app.edit_preview_path = path;
                                app.hovered_musicbrainz_result = None;
                                if let Some(item) = app
                                    .musicbrainz_results
                                    .iter()
                                    .find(|item| item.index == selected_index)
                                {
                                    app.operation_status = format!(
                                        "Selected MusicBrainz release · {} — {} · cover ready",
                                        item.release_artist, item.release_title
                                    );
                                    app.operation_tone = "green".to_string();
                                }
                            }
                        } else if still_requested {
                            app.musicbrainz_cover_preview_path = path.clone();
                            app.edit_preview_path = path;
                            if let Some(item) = app
                                .musicbrainz_results
                                .iter()
                                .find(|item| item.index == selected_index)
                            {
                                app.operation_status = format!(
                                    "Selected MusicBrainz release · {} — {} · cover ready",
                                    item.release_artist, item.release_title
                                );
                                app.operation_tone = "green".to_string();
                            }
                        }
                        if still_requested {
                            app.push_log(format!(
                                "MusicBrainz cover preview loaded for release {release_mbid}."
                            ));
                        }
                    }
                    Err(error) => {
                        if error.contains("404") {
                            app.musicbrainz_missing_cover.insert(selected_index);
                        }
                        if selected_same_result && (!hover_preview || error.contains("404")) {
                            app.operation_status = if error.contains("404") {
                                "Selected MusicBrainz release has no direct cover · Generate Candidate Artwork searches its artist and album title".to_string()
                            } else {
                                format!(
                                    "MusicBrainz release selected · cover preview unavailable · {error}"
                                )
                            };
                            app.operation_tone = "orange".to_string();
                        }
                        if still_requested {
                            app.push_log(format!(
                                "MusicBrainz cover preview unavailable for release {release_mbid}: {error}"
                            ));
                        }
                    }
                }
                if hover_preview && app.musicbrainz_hover_preview_loading == Some(selected_index) {
                    app.musicbrainz_hover_preview_loading = None;
                }
            }
            refresh_weak(&weak, &state);
        });
    });
}

#[track_caller]
fn refresh_weak(weak: &slint::Weak<AppWindow>, state: &Arc<Mutex<ApplicationState>>) {
    let count = UI_REFRESH_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    let caller = std::panic::Location::caller();
    let started = Instant::now();
    schedule_preview_image_loads(weak, state);
    schedule_artist_album_embedded_checks(weak, state);
    update_artist_bulk_completion(state);
    schedule_visible_compilation_track_checks(weak, state);
    if let Some(ui) = weak.upgrade() {
        sync_ui(&ui, state);
        schedule_completed_compilation_report_close(weak, state, &ui);
    }
    let elapsed_ms = started.elapsed().as_millis();
    if (count <= 8 || count & 63 == 0 || elapsed_ms >= 75)
        && let Some(log) = UI_DEBUG_LOG_HANDLE.get()
    {
        log.record(format!(
            "ui-refresh count={count} elapsed_ms={elapsed_ms} image_memory_hits={} image_file_decodes={} image_stale_invalidations={} image_preload_hits={} caller={}:{}",
            IMAGE_MEMORY_HITS.load(Ordering::Relaxed),
            IMAGE_FILE_DECODES.load(Ordering::Relaxed),
            IMAGE_STALE_INVALIDATIONS.load(Ordering::Relaxed),
            IMAGE_PRELOAD_HITS.load(Ordering::Relaxed),
            caller.file(),
            caller.line()
        ));
    }
}

fn refresh_compilation_report_preview(
    weak: &slint::Weak<AppWindow>,
    state: &Arc<Mutex<ApplicationState>>,
) {
    // A saved-thumb preview does not change processing state. Keep the image
    // preload, but do not start artist/album-wide embedded-art checks for a
    // single report-track click.
    schedule_preview_image_loads(weak, state);
    if let Some(ui) = weak.upgrade() {
        sync_ui(&ui, state);
    }
}

fn schedule_completed_compilation_report_close(
    weak: &slint::Weak<AppWindow>,
    state: &Arc<Mutex<ApplicationState>>,
    ui: &AppWindow,
) {
    let albums = ui.get_artist_overview_albums();
    let all_green = albums.row_count() > 0
        && (0..albums.row_count()).all(|index| {
            albums
                .row_data(index)
                .is_some_and(|album| album.fully_complete)
        });
    if !all_green {
        return;
    }
    let schedule = state.lock().ok().and_then(|mut app| {
        if !app.compilation_report_auto_close_armed
            || app.compilation_report_close_pending
            || app.compilation_report_artist.is_empty()
            || app.library.focused_artist.as_deref() != Some(app.compilation_report_artist.as_str())
            || app.operation_busy
            || app.phase != RunPhase::Idle
        {
            return None;
        }
        app.compilation_report_close_pending = true;
        Some((
            app.compilation_report_artist.clone(),
            app.compilation_report_generation,
            app.library.selection_epoch,
        ))
    });
    let Some((artist, generation, epoch)) = schedule else {
        return;
    };
    let weak = weak.clone();
    let state = Arc::clone(state);
    Timer::single_shot(Duration::from_secs(2), move || {
        let all_still_green = weak.upgrade().is_some_and(|ui| {
            let albums = ui.get_artist_overview_albums();
            albums.row_count() > 0
                && (0..albums.row_count()).all(|index| {
                    albums
                        .row_data(index)
                        .is_some_and(|album| album.fully_complete)
                })
        });
        let closed = if let Ok(mut app) = state.lock() {
            if app.compilation_report_generation != generation
                || app.compilation_report_artist != artist
                || app.library.focused_artist.as_deref() != Some(artist.as_str())
            {
                false
            } else if app.library.selection_epoch != epoch {
                app.compilation_report_close_pending = false;
                app.compilation_report_auto_close_armed = false;
                false
            } else if all_still_green && !app.operation_busy && app.phase == RunPhase::Idle {
                close_artist_compilation_report(&mut app);
                app.completion_dialog_visible = false;
                app.operation_status =
                    format!("Artist complete · {artist} COMP report closed after verification");
                app.operation_tone = "green".into();
                true
            } else {
                app.compilation_report_close_pending = false;
                false
            }
        } else {
            false
        };
        if closed {
            refresh_weak(&weak, &state);
        }
    });
}

fn schedule_preview_image_loads(
    weak: &slint::Weak<AppWindow>,
    state: &Arc<Mutex<ApplicationState>>,
) {
    let paths = state
        .lock()
        .ok()
        .map(|app| preview_image_source_paths(&app))
        .unwrap_or_default();
    let preview_paths = paths
        .into_iter()
        .map(|path| {
            let cache_key = format!("{path}\u{1f}{PREVIEW_CACHE_DIMENSION}");
            (path, cache_key)
        })
        .collect::<Vec<_>>();
    if let Ok(mut active) = active_preview_cache_keys().lock() {
        active.clear();
        active.extend(preview_paths.iter().map(|(_, cache_key)| cache_key.clone()));
    }
    for (path, cache_key) in preview_paths {
        if raw_image_cache()
            .lock()
            .ok()
            .is_some_and(|cache| cache.contains_key(&cache_key))
        {
            continue;
        }
        let queued = preview_image_loads()
            .lock()
            .ok()
            .is_some_and(|mut loads| loads.insert(cache_key.clone()));
        if !queued {
            continue;
        }
        let weak = weak.clone();
        let state = Arc::clone(state);
        std::thread::spawn(move || {
            let loaded = preload_scaled_image(&path, PREVIEW_CACHE_DIMENSION);
            if let Ok(mut loads) = preview_image_loads().lock() {
                loads.remove(&cache_key);
            }
            if loaded {
                let _ = slint::invoke_from_event_loop(move || {
                    if state.lock().is_ok_and(|app| {
                        app.compilation_report_preview_mode
                            && !app.compilation_report_preview_track_path.is_empty()
                    }) {
                        refresh_compilation_report_preview(&weak, &state);
                    } else {
                        refresh_weak(&weak, &state);
                    }
                });
            }
        });
    }
}

fn preview_image_source_paths(app: &ApplicationState) -> BTreeSet<String> {
    if app.compilation_report_preview_mode
        && !app.compilation_report_preview_track_path.trim().is_empty()
    {
        return app
            .active_compilation_track_preview()
            .map(str::to_string)
            .into_iter()
            .collect();
    }
    let candidate = app
        .preview_candidate()
        .map(|candidate| candidate.cache_path.as_str());
    let compilation = app.active_compilation_track_preview();
    let hovered_musicbrainz = app
        .hovered_musicbrainz_result
        .filter(|index| app.musicbrainz_hover_preview_index == Some(*index))
        .map(|_| app.musicbrainz_hover_preview_path.as_str());
    [
        app.current_cover_path.as_str(),
        app.edit_preview_path.as_str(),
        app.artist_portrait_preview_path.as_str(),
        app.artist_banner_preview_path.as_str(),
        app.artist_theme_portrait_preview_path.as_str(),
        app.artist_theme_banner_preview_path.as_str(),
        app.musicbrainz_cover_preview_path.as_str(),
        app.musicbrainz_existing_artwork_path.as_str(),
        hovered_musicbrainz.unwrap_or_default(),
        candidate.unwrap_or_default(),
        compilation.unwrap_or_default(),
    ]
    .into_iter()
    .filter(|path| !path.trim().is_empty())
    .map(str::to_string)
    .chain(
        app.musicbrainz_existing_artworks
            .iter()
            .map(|item| item.artwork_path.clone()),
    )
    .collect()
}

fn fallback_compilation_cover_suppressed(app: &ApplicationState, fallback_album: bool) -> bool {
    app.library.show_tracks
        || fallback_album
        || !app.current_compilation_track_path.trim().is_empty()
        || !app.library.selected_compilation_track_paths.is_empty()
}

fn compilation_artwork_context(app: &ApplicationState) -> bool {
    if !app.current_compilation_track_path.trim().is_empty()
        || !app.library.selected_compilation_track_paths.is_empty()
    {
        return true;
    }
    if app.phase != RunPhase::Idle && !app.current_album_path.trim().is_empty() {
        return app.library.albums.iter().any(|album| {
            album
                .snapshot
                .path
                .eq_ignore_ascii_case(&app.current_album_path)
                && album.snapshot.album_mbid.trim().is_empty()
        });
    }
    app.library
        .focused_album_key
        .as_ref()
        .is_some_and(|focused| {
            app.library.albums.iter().any(|album| {
                album.snapshot.album_key.eq_ignore_ascii_case(focused)
                    && album.snapshot.album_mbid.trim().is_empty()
            })
        })
}

fn require_active_compilation_write_target(app: &ApplicationState) -> Result<(), String> {
    if compilation_artwork_context(app) && app.current_compilation_track_path.trim().is_empty() {
        return Err(
            "Select a COMP fallback track and start its review before embedding artwork."
                .to_string(),
        );
    }
    Ok(())
}

fn selected_artwork_write_label(
    artist_artwork: bool,
    compilation: bool,
    existing: bool,
    thumbnail_ready: bool,
) -> &'static str {
    if artist_artwork {
        "WRITE Artist Art"
    } else if compilation {
        if existing && !thumbnail_ready {
            "Embed Existing"
        } else {
            "EMBED Comp"
        }
    } else {
        "WRITE Cover"
    }
}

fn selected_artwork_preview_type(thumbnail_ready: bool, source_type: &str) -> &str {
    if thumbnail_ready {
        "Embedded Thumb"
    } else {
        source_type
    }
}

fn musicbrainz_existing_preview_path(
    app: &ApplicationState,
    active_musicbrainz_hover_preview: bool,
) -> Option<&str> {
    (app.phase == RunPhase::AwaitingMusicBrainz
        && !active_musicbrainz_hover_preview
        && app.selected_musicbrainz_result.is_none())
    .then(|| {
        app.hovered_musicbrainz_existing
            .and_then(|index| app.musicbrainz_existing_artworks.get(index))
            .map(|item| item.artwork_path.as_str())
            .unwrap_or(app.musicbrainz_existing_artwork_path.as_str())
    })
    .filter(|path| !path.trim().is_empty())
}

fn sync_ui(ui: &AppWindow, shared: &Arc<Mutex<ApplicationState>>) {
    let Ok(state) = shared.lock() else {
        return;
    };

    ui.set_library_rows(if state.media_selector_expanded {
        library_ui_model(&state)
    } else {
        model(Vec::new())
    });
    let artist_overview_active = !state.library.show_tracks
        && state.library.view == LibraryView::Artists
        && state.library.focused_artist.is_some();
    let single_album_mode = state.library.focused_album_key.is_some()
        && (state.library.show_tracks || state.library.view == LibraryView::Albums);
    let overview_mode = if artist_overview_active {
        "Artist"
    } else if single_album_mode && state.library.show_tracks {
        "Fallback"
    } else if single_album_mode {
        "Album"
    } else {
        ""
    };
    ui.set_overview_mode(overview_mode.into());
    ui.set_artist_overview_active(artist_overview_active || single_album_mode);
    ui.set_artist_overview_embed_ready(
        (artist_overview_active && artist_album_queue_ready(&state))
            || (overview_mode == "Album" && album_overview_queue_ready(&state)),
    );
    let overview_artist = overview_artist_name(&state);
    ui.set_artist_overview_name(overview_artist.unwrap_or_default().into());
    let artist_artwork_path = overview_artist
        .and_then(|artist| {
            state
                .library
                .artist_artwork_paths
                .get(&artist.trim().to_ascii_lowercase())
        })
        .cloned()
        .unwrap_or_default();
    ui.set_artist_overview_has_artwork(!artist_artwork_path.trim().is_empty());
    ui.set_artist_overview_artwork(load_scaled_image(&artist_artwork_path, 320));
    ui.set_selected_artwork_artist_art(load_scaled_image(&artist_artwork_path, 1400));
    let artist_artwork_draft = state.artist_artwork_draft_path.clone();
    ui.set_artist_overview_draft_selected(!artist_artwork_draft.trim().is_empty());
    ui.set_artist_overview_draft_artwork(load_scaled_image(&artist_artwork_draft, 320));
    ui.set_artist_overview_navidrome_linked(overview_artist.is_some_and(|artist| {
        state
            .library
            .artist_navidrome_ids
            .contains_key(&artist.trim().to_ascii_lowercase())
    }));
    ui.set_artist_overview_biography(SharedString::from(state.artist_overview_biography.clone()));
    ui.set_artist_overview_external_url(SharedString::from(
        state.artist_overview_external_url.clone(),
    ));
    let indexed_artist_mbid = overview_artist
        .and_then(|artist| {
            state.library.albums.iter().find_map(|album| {
                (album.snapshot.artist.trim().eq_ignore_ascii_case(artist)
                    && !album.snapshot.artist_mbid.trim().is_empty())
                .then(|| album.snapshot.artist_mbid.trim().to_string())
            })
        })
        .unwrap_or_default();
    ui.set_artist_edit_indexed_id(indexed_artist_mbid.clone().into());
    let artist_album_evidence = overview_artist
        .and_then(|artist| {
            state
                .library
                .albums
                .iter()
                .find(|album| {
                    album.snapshot.artist.trim().eq_ignore_ascii_case(artist)
                        && !album.snapshot.artist_mbid.trim().is_empty()
                })
                .map(|album| {
                    format!(
                        "Album evidence: {} · Artist ID {}",
                        album.snapshot.title, album.snapshot.artist_mbid
                    )
                })
        })
        .unwrap_or_else(|| {
            "No album Artist ID indexed; enter a lookup name to match Navidrome.".to_string()
        });
    ui.set_artist_edit_album_hint(artist_album_evidence.into());
    let lookup_override = overview_artist.and_then(|artist| {
        state
            .artist_lookup_overrides
            .get(&artist.trim().to_lowercase())
    });
    let missing_album_match = overview_artist.is_some_and(|artist| {
        state.library.albums.iter().any(|album| {
            album.snapshot.artist.trim().eq_ignore_ascii_case(artist)
                && !album.snapshot.album_mbid.trim().is_empty()
                && !state
                    .artist_overview_album_keys
                    .contains(&album.snapshot.album_key.to_ascii_lowercase())
        })
    });
    ui.set_artist_edit_available(
        overview_artist.is_some()
            && (state.artist_overview_biography.trim().is_empty() || missing_album_match),
    );
    ui.set_artist_edit_saved_name(SharedString::from(
        lookup_override
            .map(|hint| hint.lookup_name.clone())
            .unwrap_or_default(),
    ));
    ui.set_artist_edit_saved_id(SharedString::from(
        lookup_override
            .map(|hint| hint.musicbrainz_id.clone())
            .unwrap_or_default(),
    ));
    ui.set_artist_overview_musicbrainz_id(SharedString::from(
        if let Some(hint) = lookup_override
            && !hint.musicbrainz_id.trim().is_empty()
        {
            hint.musicbrainz_id.clone()
        } else if state.artist_overview_musicbrainz_id.trim().is_empty() {
            indexed_artist_mbid
        } else {
            state.artist_overview_musicbrainz_id.clone()
        },
    ));
    let overview_banner_path = if state.artist_overview_banner_path.trim().is_empty() {
        artist_artwork_path.as_str()
    } else {
        state.artist_overview_banner_path.as_str()
    };
    ui.set_artist_overview_banner(load_scaled_image(overview_banner_path, 1400));
    ui.set_artist_banner_preview_active(!state.artist_banner_source_path.is_empty());
    let package_active = state.artist_artwork_mode
        && state.artist_package_overview_enabled
        && !state.artist_artwork_draft_path.is_empty()
        && !state.artist_banner_source_path.is_empty();
    let portrait_preview_path = if package_active
        && state.artist_theme_art_enabled
        && !state.artist_theme_portrait_preview_path.is_empty()
    {
        &state.artist_theme_portrait_preview_path
    } else {
        &state.artist_portrait_preview_path
    };
    let banner_preview_path = if package_active
        && state.artist_theme_art_enabled
        && !state.artist_theme_banner_preview_path.is_empty()
    {
        &state.artist_theme_banner_preview_path
    } else {
        &state.artist_banner_preview_path
    };
    ui.set_artist_banner_preview(load_scaled_image(
        &state.artist_banner_preview_path,
        PREVIEW_CACHE_DIMENSION,
    ));
    ui.set_artist_selected_banner_preview(load_scaled_image(
        banner_preview_path,
        PREVIEW_CACHE_DIMENSION,
    ));
    ui.set_artist_portrait_preview_active(!state.artist_artwork_draft_path.is_empty());
    ui.set_artist_portrait_radio_active(state.artist_portrait_radio_active);
    ui.set_artist_banner_radio_active(state.artist_banner_radio_active);
    ui.set_artist_current_radio_active(state.artist_current_radio_active);
    ui.set_artist_portrait_preview(load_scaled_image(
        &state.artist_portrait_preview_path,
        PREVIEW_CACHE_DIMENSION,
    ));
    ui.set_artist_selected_portrait_preview(load_scaled_image(
        portrait_preview_path,
        PREVIEW_CACHE_DIMENSION,
    ));
    ui.set_artist_package_preview_active(package_active);
    ui.set_artist_theme_art_enabled(state.artist_theme_art_enabled);
    ui.set_artist_preview_focus(state.artist_preview_focus.clone().into());
    ui.set_artist_banner_preview_rendered(
        state
            .artist_banner_preview_path
            .starts_with("memory-preview://"),
    );
    ui.set_artist_portrait_preview_rendered(
        state.artist_portrait_preview_path.starts_with("memory-preview://"),
    );
    ui.set_artist_selected_banner_rendered(banner_preview_path.starts_with("memory-preview://"));
    ui.set_artist_selected_portrait_rendered(portrait_preview_path.starts_with("memory-preview://"));
    ui.set_artist_banner_zoom(state.artist_banner_zoom);
    ui.set_artist_banner_horizontal(state.artist_banner_horizontal);
    ui.set_artist_banner_vertical(state.artist_banner_vertical);
    ui.set_artist_portrait_zoom(state.artist_portrait_zoom);
    ui.set_artist_portrait_horizontal(state.artist_portrait_horizontal);
    ui.set_artist_portrait_vertical(state.artist_portrait_vertical);
    ui.set_artist_overview_similar_artists(model(
        state
            .artist_overview_similar_artists
            .iter()
            .map(|(name, artwork_path)| UiSimilarArtistRow {
                name: SharedString::from(name.clone()),
                artwork: load_scaled_image(artwork_path, 128),
                available: state.library.albums.iter().any(|album| {
                    album
                        .snapshot
                        .artist
                        .trim()
                        .eq_ignore_ascii_case(name.trim())
                }),
            })
            .collect(),
    ));
    ui.set_artist_overview_genres(SharedString::from(state.artist_overview_genres.join(" · ")));
    ui.set_artist_overview_roles(SharedString::from(state.artist_overview_roles.join(" · ")));
    ui.set_artist_overview_top_songs(SharedString::from(
        state
            .artist_overview_top_songs
            .iter()
            .enumerate()
            .map(|(index, title)| format!("{}. {title}", index + 1))
            .collect::<Vec<_>>()
            .join("  ·  "),
    ));
    ui.set_artist_overview_top_songs_loading(state.artist_overview_top_songs_loading);
    ui.set_artist_overview_show_all(state.artist_overview_show_all);
    let overview_albums = artist_overview_ui_model(&state);
    let compilation_tracks = compilation_track_ui_rows(&state);
    ui.set_artist_overview_legend_counts(artist_overview_legend_counts(
        &overview_albums,
        &compilation_tracks,
    ));
    let compilation_index = compilation_tracks.first().and_then(|track| {
        (0..overview_albums.row_count()).find(|&index| {
            overview_albums.row_data(index).is_some_and(|album| {
                album
                    .key
                    .as_str()
                    .eq_ignore_ascii_case(track.album_key.as_str())
            })
        })
    });
    ui.set_artist_overview_compilation_index(
        compilation_index.map(|index| index as i32).unwrap_or(-1),
    );
    ui.set_artist_overview_albums(overview_albums);
    ui.set_overview_album_key(
        state
            .library
            .focused_album_key
            .clone()
            .unwrap_or_default()
            .into(),
    );
    ui.set_library_view_label(SharedString::from(state.library.view.label()));
    ui.set_media_status_scope_label(SharedString::from(state.library.state_scope_label()));
    ui.set_compilation_track_rows(model(compilation_tracks));
    let artist_report_visible = state.library.view == LibraryView::Artists
        && !state.library.show_tracks
        && state
            .library
            .focused_artist
            .as_deref()
            .is_some_and(|artist| {
                !state.compilation_report_artist.is_empty()
                    && artist.eq_ignore_ascii_case(&state.compilation_report_artist)
            });
    let fallback_report_visible = state.library.show_tracks
        && state.library.focused_album_key.as_ref().is_some_and(|key| {
            state.library.albums.iter().any(|album| {
                album.snapshot.album_key.eq_ignore_ascii_case(key)
                    && album.snapshot.album_mbid.trim().is_empty()
            })
        });
    let report_visible = artist_report_visible || fallback_report_visible;
    let report_preview_active = report_visible && state.compilation_report_preview_mode;
    ui.set_compilation_report_visible(report_visible);
    ui.set_compilation_report_preview_mode(report_preview_active);
    ui.set_compilation_report_preview_track_path(
        if report_visible {
            state.compilation_report_preview_track_path.as_str()
        } else {
            ""
        }
        .into(),
    );
    let fallback_report_tracks = if fallback_report_visible {
        fallback_album_report_track_ui_rows(&state)
    } else {
        Vec::new()
    };
    let report_rows = if fallback_report_visible {
        group_compilation_report_rows(
            &fallback_report_tracks,
            &state.compilation_report_collapsed_albums,
        )
    } else if artist_report_visible {
        artist_compilation_report_ui_rows(&state)
    } else {
        Vec::new()
    };
    let fallback_report_summary = if fallback_report_visible {
        let tracks = fallback_report_tracks.iter().collect::<Vec<_>>();
        format!(
            "{} tracks · {} green · {} thumb check · {} need artwork",
            tracks.len(),
            tracks
                .iter()
                .filter(|row| row.status.as_str() == "Green")
                .count(),
            tracks
                .iter()
                .filter(|row| row.status.as_str() == "Purple")
                .count(),
            tracks
                .iter()
                .filter(|row| row.status.as_str() == "Blue")
                .count(),
        )
    } else {
        state.compilation_report_summary.clone()
    };
    ui.set_compilation_report_rows(model(report_rows));
    ui.set_compilation_report_summary(fallback_report_summary.into());

    let sources = state
        .sources
        .iter()
        .map(|source| UiSourceRow {
            name: SharedString::from(source.name.clone()),
            label: SharedString::from(source_display_name(&source.name)),
            icon: SharedString::from(source_icon(&source.name)),
            count: source.candidate_count as i32,
            status: SharedString::from(source.status.clone()),
            enabled: source.enabled,
        })
        .collect::<Vec<_>>();
    ui.set_source_rows(model(sources));
    ui.set_source_options(model(
        std::iter::once(SharedString::from("All"))
            .chain(
                state
                    .sources
                    .iter()
                    .map(|source| SharedString::from(source.name.clone())),
            )
            .collect(),
    ));

    let candidates = state
        .visible_candidates()
        .into_iter()
        .map(|candidate| UiCandidateRow {
            index: candidate.index as i32,
            source: SharedString::from(source_display_name(&candidate.source)),
            dimensions: SharedString::from(candidate.dimensions.clone()),
            range_class: SharedString::from(candidate.range_class.clone()),
            policy: SharedString::from(if candidate.policy_reason.is_empty() {
                candidate.policy_status.clone()
            } else {
                format!("{} · {}", candidate.policy_status, candidate.policy_reason)
            }),
            image_type: SharedString::from(candidate.image_type.clone()),
            selected: candidate.selected,
            recommended: candidate.recommended,
            compared: state.comparison_candidates.contains(&candidate.index),
            portrait_available: candidate.image_type != "ArtistBanner",
            portrait_active: state.artist_portrait_preview_candidate_index == Some(candidate.index),
            banner_available: associated_banner_candidate(candidate, &state.candidates).is_some(),
            banner_active: state.artist_banner_preview_candidate_index == Some(candidate.index),
            preview: load_scaled_image(&candidate.cache_path, 320),
        })
        .collect::<Vec<_>>();
    ui.set_candidate_rows(model(candidates));

    let mut candidate_filters = Vec::new();
    for source in &state.sources {
        candidate_filters.push(candidate_filter_row(
            &state,
            "source",
            &source.name,
            &source.name,
        ));
    }
    let candidate_type_filters = [
        ("Recommended", "Recommended"),
        ("Upscalable", "Upscalable"),
        ("Rejected", "Rejected"),
        ("Local", "Local"),
        ("Samples", "Samples"),
        ("FrontCover", "Front Cover"),
        ("BackCover", "Back Cover"),
        ("ArtistImage", "Artist Image"),
        ("MediaBooklet", "Media / Booklet"),
        ("Other", "Other"),
    ]
    .into_iter()
    .map(|(key, label)| candidate_filter_row(&state, "type", key, label))
    .collect::<Vec<_>>();
    let candidate_policy_filters = ["Acceptable", "Strict"]
        .into_iter()
        .map(|key| candidate_filter_row(&state, "policy", key, key))
        .collect::<Vec<_>>();
    let candidate_range_filters = [
        ("Ideal", "Ideal"),
        ("UpperRange", "Upper range"),
        ("Ladder", "Ladder"),
        ("AboveLadder", "Above ladder"),
        ("LowerRange", "Lower range"),
        ("BelowMinimum", "Below minimum"),
    ]
    .into_iter()
    .map(|(key, label)| candidate_filter_row(&state, "range", key, label))
    .collect::<Vec<_>>();
    candidate_filters.extend(candidate_type_filters.iter().cloned());
    candidate_filters.extend(candidate_policy_filters.iter().cloned());
    candidate_filters.extend(candidate_range_filters.iter().cloned());
    ui.set_candidate_filter_rows(model(candidate_filters));
    ui.set_candidate_type_filter_rows(model(candidate_type_filters));
    ui.set_candidate_policy_filter_rows(model(candidate_policy_filters));
    ui.set_candidate_range_filter_rows(model(candidate_range_filters));

    ui.set_status_filter_rows(model(status_filter_ui_rows(&state)));

    let mut visible_musicbrainz = musicbrainz_visible_deduplicated_results(&state);
    let preferred_families = visible_musicbrainz
        .iter()
        .filter(|item| musicbrainz_artist_album_preferred(item))
        .map(|item| musicbrainz_group_key(item))
        .collect::<HashSet<_>>();
    let mut family_scores = HashMap::<String, u16>::new();
    for item in &visible_musicbrainz {
        let score = family_scores
            .entry(musicbrainz_group_key(item))
            .or_default();
        *score = (*score).max(item.score);
    }
    // Filtering an Existing edition can change its family's highest visible
    // percentage. Re-rank the displayed families so the section and order
    // still agree with the percentages the user can actually see.
    visible_musicbrainz.sort_by(|left, right| {
        let left_group = musicbrainz_group_key(left);
        let right_group = musicbrainz_group_key(right);
        let left_score = family_scores[&left_group];
        let right_score = family_scores[&right_group];
        musicbrainz_match_section(left_score, preferred_families.contains(&left_group))
            .cmp(&musicbrainz_match_section(
                right_score,
                preferred_families.contains(&right_group),
            ))
            .then_with(|| right_score.cmp(&left_score))
            .then_with(|| left_group.cmp(&right_group))
            .then_with(|| right.score.cmp(&left.score))
            .then_with(|| left.release_mbid.cmp(&right.release_mbid))
    });
    let mut previous_musicbrainz_group = String::new();
    let mut previous_match_section = None;
    let musicbrainz = visible_musicbrainz
        .into_iter()
        .map(|item| {
            let group_key = musicbrainz_group_key(item);
            let group = musicbrainz_group_label(item);
            let group_start = group_key != previous_musicbrainz_group;
            let artist_priority = preferred_families.contains(&group_key);
            let section_kind = musicbrainz_match_section(
                family_scores.get(&group_key).copied().unwrap_or_default(),
                artist_priority,
            );
            let section = if section_kind == 0 && !state.musicbrainz_existing_artworks.is_empty() {
                ""
            } else if previous_match_section != Some(section_kind) {
                match section_kind {
                    0 => "BEST MATCH · 90–100%",
                    1 => "ARTIST RELEASES · Same-artist albums below 90%",
                    _ => "OTHER MATCHING RELEASES · Soundtracks, compilations & appearances",
                }
            } else {
                ""
            };
            previous_match_section = Some(section_kind);
            previous_musicbrainz_group = group_key;
            UiMusicBrainzRow {
                index: item.index as i32,
                title: SharedString::from(item.release_title.clone()),
                artist: SharedString::from(item.release_artist.clone()),
                detail: SharedString::from(format!(
                    "{} · {} · {} · {} · {}",
                    item.country,
                    item.release_date,
                    item.release_class,
                    item.resolution,
                    item.release_mbid
                )),
                group: SharedString::from(group),
                section: SharedString::from(section),
                group_start,
                current: item.current,
                visited: item.visited,
                selected: state.selected_musicbrainz_result == Some(item.index),
                score: i32::from(item.score),
            }
        })
        .collect::<Vec<_>>();
    let selected_musicbrainz_visible = musicbrainz.iter().any(|row| row.selected);
    // Replacing an unchanged result model destroys and recreates every row's
    // TouchArea. A hover-preview-only refresh then emits a synthetic hover
    // exit and makes the newly loaded cover disappear. Preserve the existing
    // model when its visible data is unchanged; selection/filter changes still
    // replace it normally.
    let current_musicbrainz = ui.get_musicbrainz_rows();
    if !musicbrainz_ui_rows_equal(&current_musicbrainz, &musicbrainz) {
        ui.set_musicbrainz_rows(model(musicbrainz));
    }
    ui.set_musicbrainz_selected_different(
        selected_musicbrainz_visible
            && state
                .selected_musicbrainz_result
                .and_then(|index| {
                    state
                        .musicbrainz_results
                        .iter()
                        .find(|item| item.index == index)
                })
                .is_some_and(|item| {
                    !item.current
                        && !item
                            .release_mbid
                            .eq_ignore_ascii_case(&state.musicbrainz_authority.release_mbid)
                }),
    );
    ui.set_musicbrainz_selected_cover_missing(
        state
            .selected_musicbrainz_result
            .is_some_and(|index| state.musicbrainz_missing_cover.contains(&index)),
    );
    ui.set_musicbrainz_existing_available(
        state.phase == RunPhase::AwaitingMusicBrainz
            && !state.musicbrainz_existing_artwork_path.is_empty(),
    );
    ui.set_musicbrainz_existing_preview(load_cached_preview_image(
        &state.musicbrainz_existing_artwork_path,
        "",
    ));
    ui.set_musicbrainz_existing_detail(SharedString::from(
        if state.musicbrainz_existing_artwork_kind == "artist-album-cover" {
            format!(
                "{} · Artist Album cover · Recording + Artist IDs",
                state.musicbrainz_existing_artwork_title
            )
        } else {
            format!(
                "{} · Verified embedded cover · Recording + Artist IDs",
                state.musicbrainz_existing_artwork_title
            )
        },
    ));
    let existing_rows = state
        .musicbrainz_existing_artworks
        .iter()
        .enumerate()
        .map(|(index, item)| UiExistingArtworkRow {
            index: index as i32,
            title: SharedString::from(item.title.clone()),
            detail: SharedString::from(if item.kind == "artist-album-cover-title" {
                "Artist Album cover · Artist ID + exact title (different Recording ID)"
            } else if item.kind == "artist-album-cover" {
                "Artist Album cover · Recording + Artist IDs"
            } else {
                "Verified embedded cover · Recording + Artist IDs"
            }),
            source_path: SharedString::from(item.artwork_path.clone()),
            preview: load_cached_preview_image(&item.artwork_path, ""),
            loaded: cached_preview_metadata(&item.artwork_path).is_some(),
            selected: state.selected_musicbrainz_existing == Some(index),
        })
        .collect::<Vec<_>>();
    if !existing_ui_rows_equal(&ui.get_musicbrainz_existing_rows(), &existing_rows) {
        ui.set_musicbrainz_existing_rows(model(existing_rows));
    }
    ui.set_musicbrainz_artist_options(model(
        std::iter::once(SharedString::from("All"))
            .chain(
                musicbrainz_visible_deduplicated_results(&state)
                    .into_iter()
                    .map(|item| item.release_artist.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .map(SharedString::from),
            )
            .collect(),
    ));
    ui.set_musicbrainz_release_type_options(model(
        std::iter::once(SharedString::from("All"))
            .chain(
                musicbrainz_visible_deduplicated_results(&state)
                    .into_iter()
                    .map(|item| item.release_class.clone())
                    .filter(|value| !value.is_empty())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .map(SharedString::from),
            )
            .collect(),
    ));

    let adjustment = &state.adjustments;
    let preset_rows = state
        .adjustment_presets
        .iter()
        .enumerate()
        .map(|(index, preset)| UiAdjustmentPresetRow {
            index: index as i32,
            name: preset.name.clone().into(),
            summary: if preset.saved {
                preset
                    .values
                    .iter()
                    .map(i32::to_string)
                    .collect::<Vec<_>>()
                    .join(" · ")
            } else {
                "Not saved yet".to_string()
            }
            .into(),
            saved: preset.saved,
        })
        .collect::<Vec<_>>();
    ui.set_default_adjustment_preset(preset_rows[0].clone());
    ui.set_custom_adjustment_presets(model(preset_rows[1..].to_vec()));
    ui.set_adjustment_preset_rows(model(preset_rows));
    ui.set_theme_banner_sync_frame_letters(state.banner_sync_frame_letters);
    ui.set_theme_banner_random_harmony(state.banner_random_harmony);
    ui.set_theme_wheel_hue_shift(i32::from(state.wheel_hue_shift_degrees));
    ui.set_theme_wheel_brightness(i32::from(state.wheel_brightness_percent));
    ui.set_theme_wheel_image(theme_wheel_image(
        state.wheel_hue_shift_degrees,
        state.wheel_brightness_percent,
    ));
    ui.set_theme_banner_rows(model(
        state
            .banner_theme_styles
            .iter()
            .map(|style| UiBannerThemeRow {
                key: style.key.clone().into(),
                name: style.name.clone().into(),
                background: themed_color(&style.background, &state.appearance_mode, true),
                frame: themed_color(&style.frame, &state.appearance_mode, false),
                letters: themed_color(&style.letters, &state.appearance_mode, false),
                at_baseline: state
                    .banner_theme_baseline_styles
                    .iter()
                    .any(|baseline| baseline.key == style.key && baseline == style),
            })
            .collect(),
    ));
    ui.set_theme_frame_rows(model(
        state
            .album_frame_theme_styles
            .iter()
            .map(|frame| UiAlbumFrameThemeRow {
                key: frame.key.clone().into(),
                name: frame.name.clone().into(),
                color: themed_color(&frame.color, &state.appearance_mode, false),
                at_baseline: state
                    .album_frame_theme_baseline_styles
                    .iter()
                    .any(|baseline| baseline.key == frame.key && baseline == frame),
            })
            .collect(),
    ));
    let color_target = ui.get_theme_color_target().to_string();
    let selected_hex = theme_target_hex(&state, &color_target).unwrap_or("#3478FF");
    ui.set_theme_selected_color(themed_color(selected_hex, &state.appearance_mode, false));
    ui.set_theme_selected_hex(selected_hex.into());
    ui.set_theme_preview_album(theme_preview_album_row(&state, &color_target));
    let preview_frame = color_target
        .strip_prefix("frame.")
        .and_then(|key| {
            state
                .album_frame_theme_styles
                .iter()
                .find(|frame| frame.key == key)
        })
        .or_else(|| {
            state
                .album_frame_theme_styles
                .iter()
                .find(|frame| frame.key == "complete")
        });
    if let Some(frame) = preview_frame {
        ui.set_theme_preview_frame_color(themed_color(&frame.color, &state.appearance_mode, false));
        ui.set_theme_preview_frame_label(frame.name.clone().into());
    }
    let adjustment_rows = vec![
        adjustment_row(&state, "Picture", adjustment.picture_percent, "%"),
        adjustment_row(&state, "Sharpen", adjustment.sharpen_percent, "%"),
        adjustment_row(&state, "Softness", adjustment.softness_percent, "%"),
        adjustment_row(&state, "Contrast", adjustment.contrast_percent, "%"),
        adjustment_row(&state, "Exposure", adjustment.exposure_percent, "%"),
        adjustment_row(&state, "Brightness", adjustment.brightness_percent, "%"),
        adjustment_row(&state, "Gamma", adjustment.gamma_percent, "%"),
        adjustment_row(&state, "Color", adjustment.color_temperature, ""),
        adjustment_row(&state, "Hue", adjustment.hue_degrees, "°"),
        adjustment_row(&state, "Darkness", adjustment.darkness_percent, "%"),
        adjustment_row(&state, "Tint", adjustment.tint_percent, "%"),
    ];
    ui.set_detail_adjustment_rows(model(adjustment_rows[0..3].to_vec()));
    ui.set_light_adjustment_rows(model(vec![
        adjustment_rows[3].clone(),
        adjustment_rows[4].clone(),
        adjustment_rows[5].clone(),
        adjustment_rows[6].clone(),
        adjustment_rows[9].clone(),
    ]));
    ui.set_color_adjustment_rows(model(vec![
        adjustment_rows[7].clone(),
        adjustment_rows[8].clone(),
        adjustment_rows[10].clone(),
    ]));
    let filter_levels = active_filter_levels(&state);
    let filter_menu_rows = FILTER_MENU_NAMES[..13].iter().enumerate().map(|(index, name)| UiAdjustmentRow {
            name: (*name).into(), value: filter_levels[index], minimum: -5, maximum: 5,
            unit: "".into(), primary: false,
        }).collect::<Vec<_>>();
    if !sync_enhancement_menu_rows(ui.get_filter_menu_rows(), filter_menu_rows.clone()) {
        ui.set_filter_menu_rows(model(filter_menu_rows));
    }
    let mut advanced_menu_rows = vec![UiAdjustmentRow {
        name: "Default".into(), value: filter_levels[13], minimum: -5, maximum: 5,
        unit: "".into(), primary: false,
    }];
    let saved_values = AdjustmentState::from_config(&state.config).slider_values();
    advanced_menu_rows.extend([8, 10, 6, 0, 4, 5].into_iter().map(|index| {
        let mut row = adjustment_rows[index].clone();
        let (minimum, maximum) = adjustment_bounds(row.name.as_str());
        let span = minimum.abs().max(maximum.abs());
        row.value -= saved_values[index];
        row.minimum = -span;
        row.maximum = span;
        row
    }));
    if !sync_enhancement_menu_rows(ui.get_advanced_menu_rows(), advanced_menu_rows.clone()) {
        ui.set_advanced_menu_rows(model(advanced_menu_rows));
    }
    let tint_palette_ready = active_tint_source(&state)
        .is_some_and(|source| source == state.tint_palette_source);
    let tint_hex = state.adjustments.tint_color.trim_start_matches('#');
    let tint_bytes = if tint_hex.len() == 6 {
        [0, 2, 4].map(|offset| tint_hex.get(offset..offset + 2)
            .and_then(|part| u8::from_str_radix(part, 16).ok()))
    } else {
        [None; 3]
    };
    let tint_color = if tint_palette_ready {
        if let [Some(red), Some(green), Some(blue)] = tint_bytes {
            Color::from_rgb_u8(red, green, blue)
        } else {
            Color::from_rgb_u8(128, 128, 128)
        }
    } else {
        Color::from_rgb_u8(128, 128, 128)
    };
    ui.set_tint_palette_color(tint_color);
    ui.set_tint_palette_ready(tint_palette_ready);
    ui.set_adjustment_rows(model(adjustment_rows));

    let report_previewing = state.compilation_report_preview_mode
        && !state
            .compilation_report_preview_track_path
            .trim()
            .is_empty();
    let focus_changed_during_review = state.phase != RunPhase::Idle
        && overview_artist.is_some_and(|artist| {
            !state.current_artist.trim().is_empty()
                && !artist.trim().eq_ignore_ascii_case(state.current_artist.trim())
        });
    let preview_candidate = if report_previewing {
        None
    } else {
        state.preview_candidate()
    };
    let has_edit_preview = !report_previewing && !state.edit_preview_path.is_empty();
    let thumbnail_ready = !report_previewing && embedded_thumbnail_ready(&state);
    let thumbnail_preview =
        thumbnail_ready.then_some(state.embedded_thumbnail_preview_path.as_str());
    let compilation_track_preview = state.active_compilation_track_preview();
    let has_compilation_track_preview = compilation_track_preview.is_some();
    let active_musicbrainz_hover_preview = state
        .hovered_musicbrainz_result
        .filter(|index| state.musicbrainz_hover_preview_index == Some(*index))
        .is_some()
        && !state.musicbrainz_hover_preview_path.trim().is_empty();
    let musicbrainz_cover_preview = has_edit_preview
        && state.phase == RunPhase::AwaitingMusicBrainz
        && state.selected_musicbrainz_result.is_some_and(|selected| {
            state
                .musicbrainz_results
                .iter()
                .find(|item| item.index == selected)
                .is_some_and(|item| !item.current)
        })
        || (state.phase == RunPhase::AwaitingMusicBrainz && active_musicbrainz_hover_preview);
    let artist_overview_fallback_album = artist_overview_active
        && state
            .library
            .focused_album_key
            .as_ref()
            .is_some_and(|focused| {
                state.library.albums.iter().any(|album| {
                    album.snapshot.album_key.eq_ignore_ascii_case(focused)
                        && album.snapshot.album_mbid.trim().is_empty()
                })
            });
    let selected_artwork_artist_overview = artist_overview_active
        && state.phase != RunPhase::AwaitingMusicBrainz
        && (state.current_album.trim().is_empty()
            || artist_overview_fallback_album
            || focus_changed_during_review)
        && !has_edit_preview
        && !has_compilation_track_preview
        && !report_previewing
        && preview_candidate.is_none();
    ui.set_selected_artwork_artist_overview(selected_artwork_artist_overview);
    ui.set_selected_artwork_artist_has_art(!artist_artwork_path.trim().is_empty());
    let suppress_fallback_cover =
        fallback_compilation_cover_suppressed(&state, artist_overview_fallback_album);
    let current_cover_path = if suppress_fallback_cover
        || report_previewing
        || focus_changed_during_review
    {
        ""
    } else {
        state.current_cover_path.as_str()
    };
    let musicbrainz_preview_source = (state.phase == RunPhase::AwaitingMusicBrainz
        && active_musicbrainz_hover_preview)
        .then_some(state.musicbrainz_hover_preview_path.as_str())
        .filter(|path| !path.trim().is_empty())
        .or_else(|| {
            (state.phase == RunPhase::AwaitingMusicBrainz
                && state.selected_musicbrainz_result.is_some())
            .then_some(state.musicbrainz_cover_preview_path.as_str())
            .filter(|path| !path.trim().is_empty())
        });
    let existing_preview_source =
        musicbrainz_existing_preview_path(&state, active_musicbrainz_hover_preview);
    let awaiting_musicbrainz_result_preview = state.phase == RunPhase::AwaitingMusicBrainz
        && (state.hovered_musicbrainz_result.is_some()
            || state.selected_musicbrainz_result.is_some())
        && musicbrainz_preview_source.is_none();
    let original_preview_path = musicbrainz_preview_source
        .or(existing_preview_source)
        .or_else(|| {
            preview_candidate
                .map(|candidate| candidate.cache_path.as_str())
                .filter(|path| !path.is_empty())
        })
        .or_else(|| {
            (!awaiting_musicbrainz_result_preview)
                .then_some(compilation_track_preview)
                .flatten()
        })
        .unwrap_or(current_cover_path);
    let focused_album_thumbnail = if suppress_fallback_cover {
        ""
    } else {
        state
            .library
            .focused_album_key
            .as_ref()
            .and_then(|key| {
                state
                    .library
                    .album_artwork_paths
                    .get(&key.to_ascii_lowercase())
            })
            .map(String::as_str)
            .unwrap_or_default()
    };
    ui.set_original_preview(load_cached_preview_image(
        original_preview_path,
        focused_album_thumbnail,
    ));
    let preview_path = thumbnail_preview
        .or((has_edit_preview
            && (state
                .edit_preview_path
                .starts_with("memory-preview://musicbrainz/")
                || (existing_preview_source.is_some()
                    && state
                        .edit_preview_path
                        .starts_with("memory-preview://musicbrainz-existing/")))
            && !active_musicbrainz_hover_preview)
            .then_some(state.edit_preview_path.as_str()))
        .or(musicbrainz_preview_source)
        .or(existing_preview_source)
        .or_else(|| has_edit_preview.then_some(state.edit_preview_path.as_str()))
        .or_else(|| {
            preview_candidate
                .map(|candidate| candidate.cache_path.as_str())
                .filter(|path| !path.is_empty())
        })
        .or_else(|| {
            (!awaiting_musicbrainz_result_preview)
                .then_some(compilation_track_preview)
                .flatten()
        })
        .unwrap_or(current_cover_path);
    // The main artwork pane must not inherit a 320px candidate-card cache entry.
    // Decode the selected candidate at preview size once; subsequent refreshes
    // reuse the validated high-resolution cache entry.
    let current_preview =
        if preview_candidate.is_some_and(|candidate| candidate.cache_path == preview_path) {
            load_scaled_image(preview_path, PREVIEW_CACHE_DIMENSION)
        } else {
            load_cached_preview_image(preview_path, focused_album_thumbnail)
        };
    ui.set_current_preview(current_preview);
    ui.set_selected_artwork_source(SharedString::from(if selected_artwork_artist_overview {
        "Navidrome".to_string()
    } else if musicbrainz_cover_preview {
        "MusicBrainz".to_string()
    } else if existing_preview_source.is_some() {
        "Existing".to_string()
    } else if has_edit_preview {
        "Enhancement".to_string()
    } else if preview_candidate.is_none() && has_compilation_track_preview {
        "Embedded".to_string()
    } else {
        preview_candidate
            .map(|candidate| source_display_name(&candidate.source))
            .unwrap_or_else(|| "Local".to_string())
    }));
    let selected_artwork_type = if selected_artwork_artist_overview {
        "Artist Image"
    } else if musicbrainz_cover_preview || existing_preview_source.is_some() {
        "Front Cover"
    } else if has_edit_preview {
        if state.upscale_preview_only {
            "Upscale Preview"
        } else {
            "Upscale"
        }
    } else if preview_candidate.is_none() && has_compilation_track_preview {
        "Track Artwork"
    } else {
        preview_candidate
            .map(|candidate| candidate.image_type.as_str())
            .unwrap_or("Current Cover")
    };
    ui.set_selected_artwork_type(SharedString::from(selected_artwork_preview_type(
        thumbnail_ready,
        selected_artwork_type,
    )));
    let metadata_path = if thumbnail_ready {
        state.embedded_thumbnail_preview_path.as_str()
    } else if selected_artwork_artist_overview {
        artist_artwork_path.as_str()
    } else {
        preview_path
    };
    let cached_preview = cached_preview_metadata(metadata_path);
    let preview_dimensions =
        if thumbnail_ready || selected_artwork_artist_overview || has_edit_preview {
            cached_preview.map(|(width, height, _)| format!("{width} × {height}"))
        } else {
            preview_candidate
                .map(|candidate| candidate.dimensions.clone())
                .filter(|value| !value.trim().is_empty())
                .or_else(|| cached_preview.map(|(width, height, _)| format!("{width} × {height}")))
        }
        .unwrap_or_else(|| "Resolution unavailable".to_string());
    ui.set_selected_artwork_dimensions(SharedString::from(preview_dimensions));
    ui.set_selected_artwork_quality(SharedString::from(if selected_artwork_artist_overview {
        "Artist"
    } else if existing_preview_source.is_some() {
        "Existing"
    } else if has_edit_preview {
        "Preview"
    } else {
        preview_candidate
            .map(|candidate| candidate.range_class.as_str())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("Current")
    }));
    ui.set_selected_artwork_is_sample(
        preview_candidate.is_some_and(|candidate| candidate.source.eq_ignore_ascii_case("samples")),
    );
    ui.set_artist_artwork_mode(state.artist_artwork_mode);
    let compilation_context = !state.artist_artwork_mode && compilation_artwork_context(&state);
    let current_album_queue_locked = state
        .embed_locked_album_paths
        .contains(&state.current_album_path.to_lowercase());
    ui.set_selected_artwork_queue_locked(current_album_queue_locked);
    let selected_artwork_write_ready = if state.artist_artwork_mode {
        (state.artist_portrait_radio_active && !state.artist_artwork_draft_path.trim().is_empty())
            || (state.artist_banner_radio_active
                && !state.artist_banner_source_path.trim().is_empty())
    } else if state.phase == RunPhase::AwaitingMusicBrainz {
        if let Some(path) = existing_preview_source {
            std::path::Path::new(path).is_file()
        } else {
            state.selected_musicbrainz_result.is_some()
                && !state.musicbrainz_cover_preview_path.trim().is_empty()
                && std::path::Path::new(&state.musicbrainz_cover_preview_path).is_file()
        }
    } else if state.phase == RunPhase::AwaitingCandidate {
        state.selected_candidate().is_some()
    } else {
        !current_cover_path.trim().is_empty()
            || state
                .selected_candidate()
                .is_some_and(|candidate| candidate.source.eq_ignore_ascii_case("samples"))
    };
    let selected_artwork_write_ready = selected_artwork_write_ready
        && !current_album_queue_locked
        && !report_preview_active
        && (!compilation_context || !state.current_compilation_track_path.trim().is_empty());
    ui.set_selected_artwork_write_ready(selected_artwork_write_ready);
    ui.set_selected_artwork_thumbnail_ready(thumbnail_ready && selected_artwork_write_ready);
    ui.set_selected_artwork_comp(compilation_context);
    ui.set_selected_artwork_embedded_target(if compilation_context {
        format!(
            "COMP embed target: {0} × {0} · current track thumb shown below",
            state.config.output.embedded_thumbnail_size
        )
        .into()
    } else {
        SharedString::default()
    });
    ui.set_selected_artwork_write_label(
        selected_artwork_write_label(
            state.artist_artwork_mode,
            compilation_context,
            existing_preview_source.is_some()
                || (state.phase == RunPhase::AwaitingCandidate
                    && state.selected_candidate().is_some_and(|candidate| {
                        candidate.source.eq_ignore_ascii_case("existing")
                    })),
            thumbnail_ready && selected_artwork_write_ready,
        )
        .into(),
    );
    let embed_available = !state.artist_artwork_mode
        && !compilation_context
        && !report_preview_active
        && !state.operation_busy
        && !current_album_queue_locked
        && !state.current_album_path.trim().is_empty()
        && (thumbnail_ready
            || state.edit_preview_path.starts_with("memory-preview://")
            || !state.current_cover_path.trim().is_empty()
            || state.selected_candidate().is_some())
        && !state
            .selected_candidate()
            .is_some_and(|candidate| candidate.source.eq_ignore_ascii_case("samples"));
    ui.set_selected_artwork_embed_available(embed_available);
    let (has_webp, webp_needs_still) = state
        .library
        .focused_album_key
        .as_ref()
        .and_then(|key| {
            state
                .library
                .albums
                .iter()
                .find(|album| album.snapshot.album_key.eq_ignore_ascii_case(key))
        })
        .map(|album| canonical_webp_status(&album.snapshot))
        .unwrap_or_default();
    ui.set_selected_artwork_webp_needs_still(webp_needs_still);
    ui.set_selected_artwork_webp_static_footer(has_webp && state.phase == RunPhase::Idle);
    let embedded_infos = current_embedded_thumbnail_infos(&state);
    ui.set_selected_artwork_embedded_info(if embed_available {
        format_existing_embedded_info(&embedded_infos).into()
    } else {
        SharedString::default()
    });
    let (embedded_source, embedded_type, embedded_dimensions, embedded_size, embedded_color) =
        embedded_artwork_footer(&embedded_infos, state.config.output.embedded_thumbnail_size);
    let preview_size = cached_preview
        .map(|(_, _, bytes)| byte_size_label(bytes))
        .unwrap_or_else(|| "Size unavailable".to_string());
    let size_comparison = embedded_artwork_size_comparison(
        thumbnail_ready,
        cached_preview.map(|(_, _, bytes)| bytes),
        &embedded_infos,
    );
    let comparing_sizes = size_comparison.is_some();
    ui.set_selected_artwork_size_comparison(comparing_sizes);
    let origin_path = if has_edit_preview {
        state.edit_preview_path.as_str()
    } else {
        original_preview_path
    };
    let origin_metadata = cached_preview_metadata(origin_path);
    let origin_dimensions = if has_edit_preview {
        origin_metadata.map(|(width, height, _)| format!("{width} × {height}"))
    } else {
        preview_candidate
            .map(|candidate| candidate.dimensions.clone())
            .filter(|value| !value.trim().is_empty())
            .or_else(|| origin_metadata.map(|(width, height, _)| format!("{width} × {height}")))
    }
    .unwrap_or_else(|| "Resolution unavailable".to_string());
    let origin_bytes = origin_metadata.map(|(_, _, bytes)| bytes).or_else(|| {
        (!origin_path.starts_with("memory-preview://"))
            .then(|| {
                std::fs::metadata(origin_path)
                    .ok()
                    .map(|metadata| metadata.len())
            })
            .flatten()
    });
    ui.set_selected_artwork_origin_source(ui.get_selected_artwork_source());
    ui.set_selected_artwork_origin_type(selected_artwork_type.into());
    ui.set_selected_artwork_origin_dimensions(origin_dimensions.into());
    ui.set_selected_artwork_origin_size(
        origin_bytes
            .map(byte_size_label)
            .unwrap_or_else(|| "Size unavailable".to_string())
            .into(),
    );
    let (preview_size, embedded_size) = size_comparison.unwrap_or((preview_size, embedded_size));
    if comparing_sizes {
        ui.set_selected_artwork_source("After".into());
    }
    ui.set_selected_artwork_size(preview_size.into());
    ui.set_selected_artwork_embedded_source(
        if comparing_sizes {
            "Before".to_string()
        } else {
            embedded_source
        }
        .into(),
    );
    ui.set_selected_artwork_embedded_type(
        if comparing_sizes {
            format!("Track(s) ({})", embedded_infos.len())
        } else {
            embedded_type
        }
        .into(),
    );
    ui.set_selected_artwork_embedded_dimensions(embedded_dimensions.into());
    ui.set_selected_artwork_embedded_size(embedded_size.into());
    ui.set_selected_artwork_embedded_dimensions_color(embedded_color);
    ui.set_selected_artwork_existing(
        !state.artist_artwork_mode
            && (existing_preview_source.is_some()
                || (state.phase == RunPhase::AwaitingCandidate
                    && state.selected_candidate().is_some_and(|candidate| {
                        candidate.source.eq_ignore_ascii_case("existing")
                    }))),
    );
    ui.set_selected_count(state.library.selected_count() as i32);
    ui.set_status_filter_label(SharedString::from(
        if state.library.status_filter_enabled(StatusFilter::All) {
            "All".to_string()
        } else if state.library.status_filters.is_empty() {
            "None".to_string()
        } else {
            state
                .library
                .status_filters
                .iter()
                .map(|value| value.label())
                .collect::<Vec<_>>()
                .join(", ")
        },
    ));
    ui.set_database_label(SharedString::from(
        if state.library.database_path.is_empty() {
            "No media database loaded".to_string()
        } else {
            state.library.database_path.clone()
        },
    ));
    ui.set_current_artist(SharedString::from(state.current_artist.clone()));
    ui.set_current_album(SharedString::from(state.current_album.clone()));
    ui.set_current_path(SharedString::from(preview_path));
    ui.set_current_status(SharedString::from(match state.phase {
        RunPhase::Idle if ui.get_setup_required() => "Setup required",
        RunPhase::Idle => "Ready",
        RunPhase::Running => "Processing",
        RunPhase::AwaitingCandidate => "Awaiting artwork decision",
        RunPhase::AwaitingMusicBrainz => "Awaiting MusicBrainz authority decision",
        RunPhase::Stopping => "Stopping",
    }));
    ui.set_log_text(SharedString::from(state.log.join("\n")));
    ui.set_error_text(SharedString::from(
        state.last_error.clone().unwrap_or_default(),
    ));
    ui.set_service_status(SharedString::from(state.service_status.clone()));
    ui.set_operation_busy(state.operation_busy);
    ui.set_artist_overview_loading(state.artist_overview_loading);
    ui.set_operation_status(SharedString::from(state.operation_status.clone()));
    ui.set_operation_tone(SharedString::from(state.operation_tone.clone()));
    let intro_active = !state.disable_intro_message && !state.intro_dismissed;
    ui.set_intro_active(intro_active);
    ui.set_confirmation_messages_disabled(state.disable_confirmation_messages);
    ui.set_can_launch(
        !ui.get_setup_required()
            && !intro_active
            && !state.operation_busy
            && !report_preview_active
            && state.can_launch(),
    );
    ui.set_running(state.phase != RunPhase::Idle);
    ui.set_auto_scan_enabled(state.auto_scan_enabled);
    ui.set_live_write(state.launch_mode == LaunchMode::LiveWrite);
    ui.set_awaiting_candidate(state.phase == RunPhase::AwaitingCandidate);
    ui.set_awaiting_musicbrainz(state.phase == RunPhase::AwaitingMusicBrainz);
    ui.set_current_album_bypassed(state.library.focused_album_is_bypassed());
    ui.set_launch_mode_label(SharedString::from(match state.launch_mode {
        LaunchMode::Read => "READ",
        LaunchMode::LiveWrite => "LIVE WRITE",
    }));
    ui.set_scan_scope_label(SharedString::from(match state.scan_scope {
        ScanScope::All => "ALL",
        ScanScope::Selected => "SELECTED",
    }));
    ui.set_authority_artist(SharedString::from(
        state.musicbrainz_authority.artist.clone(),
    ));
    ui.set_authority_album(SharedString::from(
        state.musicbrainz_authority.album.clone(),
    ));
    ui.set_authority_release_id(SharedString::from(
        state.musicbrainz_authority.release_mbid.clone(),
    ));
    ui.set_authority_recording_id(SharedString::from(
        state.musicbrainz_authority.recording_mbid.clone(),
    ));
    ui.set_authority_artist_ids(SharedString::from(
        state.musicbrainz_authority.artist_mbids.clone(),
    ));
    ui.set_musicbrainz_identity_status(SharedString::from(
        state.musicbrainz_identity_status.clone(),
    ));
    ui.set_musicbrainz_identity_message(SharedString::from(
        state.musicbrainz_identity_message.clone(),
    ));
    ui.set_musicbrainz_recording_status(SharedString::from(
        state.musicbrainz_recording_status.clone(),
    ));
    ui.set_musicbrainz_artist_status(SharedString::from(state.musicbrainz_artist_status.clone()));
    ui.set_musicbrainz_friendly_artist(SharedString::from(
        state.musicbrainz_friendly_artist.clone(),
    ));
    ui.set_musicbrainz_friendly_title(SharedString::from(state.musicbrainz_friendly_title.clone()));
    ui.set_musicbrainz_used_fuzzy_search(state.musicbrainz_used_fuzzy_search);
    ui.set_show_tracks(state.library.show_tracks);
    ui.set_media_selector_expanded(state.media_selector_expanded);
    ui.set_candidate_filter_expanded(state.candidate_filter_expanded);
    ui.set_media_filter_locked(state.media_filter_locked);
    ui.set_candidate_filter_locked(state.candidate_filter_locked);
    ui.set_artwork_editing_expanded(state.artwork_editing_expanded);
    ui.set_show_artwork(state.show_artwork);
    ui.set_hover_enabled(state.hover_enabled);
    ui.set_upscale_preview_only(state.upscale_preview_only);
    ui.set_completion_dialog_visible(state.completion_dialog_visible);
    ui.set_completion_message(SharedString::from(state.completion_message.clone()));
    ui.set_settings_disable_intro_message(state.disable_intro_message);
    ui.set_settings_disable_confirmation_messages(state.disable_confirmation_messages);
    // Reassigning a bound LineEdit on every keystroke steals its caret/focus
    // on some Windows backends and produces the system error sound.
    if ui.get_artist_filter_text().as_str() != state.library.artist_filter.as_str() {
        ui.set_artist_filter_text(SharedString::from(state.library.artist_filter.clone()));
    }
    if ui.get_album_filter_text().as_str() != state.library.album_filter.as_str() {
        ui.set_album_filter_text(SharedString::from(state.library.album_filter.clone()));
    }
    ui.set_fallback_artist(SharedString::from(state.fallback_artist.clone()));
    ui.set_fallback_album(SharedString::from(state.fallback_album.clone()));
    ui.set_fallback_reason(SharedString::from(state.fallback_reason.clone()));
    ui.set_fallback_active(state.fallback_active);
    ui.set_musicbrainz_retry_available(state.musicbrainz_retry_available);
    ui.set_musicbrainz_matches_available(state.musicbrainz_matches_available);
    ui.set_musicbrainz_back_available(state.musicbrainz_back_available);
    ui.set_selected_musicbrainz_artist(SharedString::from(
        state
            .musicbrainz_artist_filter
            .clone()
            .unwrap_or_else(|| "All".to_string()),
    ));
    ui.set_selected_musicbrainz_release_type(SharedString::from(
        state
            .musicbrainz_release_type_filter
            .clone()
            .unwrap_or_else(|| "All".to_string()),
    ));
    ui.set_run_total(state.run_total as i32);
    ui.set_run_completed(state.run_completed as i32);
    ui.set_run_failed(state.run_failed as i32);
}

fn library_ui_model(state: &ApplicationState) -> ModelRc<UiLibraryRow> {
    let rows = if state.library.show_tracks {
        state.library.fallback_album_rows()
    } else {
        state.library.rows()
    };
    Rc::new(LibraryUiModel {
        rows,
        preview_size: 64,
        embed_locked_keys: state
            .library
            .albums
            .iter()
            .filter(|album| {
                state
                    .embed_locked_album_paths
                    .contains(&album.snapshot.path.to_lowercase())
            })
            .map(|album| album.snapshot.album_key.to_lowercase())
            .collect(),
    })
    .into()
}

fn artist_album_card_display_title(title: &str) -> String {
    const MAX_CHARS: usize = 12;
    if title.chars().count() <= MAX_CHARS {
        return title.to_string();
    }
    let mut display = title
        .chars()
        .take(MAX_CHARS - 2)
        .collect::<String>()
        .trim_end()
        .to_string();
    display.push_str("..");
    display
}

fn rgb_from_hex(value: &str) -> [u8; 3] {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() == 6
        && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
        && let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&hex[0..2], 16),
            u8::from_str_radix(&hex[2..4], 16),
            u8::from_str_radix(&hex[4..6], 16),
        )
    {
        return [r, g, b];
    }
    [52, 120, 255]
}

fn themed_color(value: &str, appearance: &str, background: bool) -> Color {
    let mut channels = rgb_from_hex(value);
    for channel in &mut channels {
        let base = f32::from(*channel);
        let adjusted = if appearance.eq_ignore_ascii_case("Chalk") {
            base * 0.78 + 132.0 * 0.22
        } else if appearance.eq_ignore_ascii_case("Light") {
            if background {
                base * 0.84 + 255.0 * 0.16
            } else {
                base * 0.72
            }
        } else {
            base
        };
        *channel = adjusted.round().clamp(0.0, 255.0) as u8;
    }
    Color::from_argb_u8(
        if background { 128 } else { 255 },
        channels[0],
        channels[1],
        channels[2],
    )
}

fn theme_target_hex<'a>(state: &'a ApplicationState, target: &str) -> Option<&'a str> {
    if let Some(key) = target.strip_prefix("frame.") {
        return state
            .album_frame_theme_styles
            .iter()
            .find(|style| style.key == key)
            .map(|style| style.color.as_str());
    }
    let (key, part) = target.split_once('.')?;
    let style = state
        .banner_theme_styles
        .iter()
        .find(|style| style.key == key)?;
    match part {
        "background" => Some(style.background.as_str()),
        "frame" => Some(style.frame.as_str()),
        "letters" => Some(style.letters.as_str()),
        _ => None,
    }
}

fn theme_preview_album_row(state: &ApplicationState, target: &str) -> UiArtistAlbumRow {
    let banner_key = target
        .split_once('.')
        .map(|(key, _)| key)
        .filter(|key| *key != "frame")
        .unwrap_or("artist");
    let banner = state
        .banner_theme_styles
        .iter()
        .find(|style| style.key == banner_key)
        .or_else(|| {
            state
                .banner_theme_styles
                .iter()
                .find(|style| style.key == "artist")
        });
    let banner_color = |part: &str, fallback: &str, background: bool| {
        let hex = banner
            .map(|style| match part {
                "background" => style.background.as_str(),
                "frame" => style.frame.as_str(),
                _ => style.letters.as_str(),
            })
            .unwrap_or(fallback);
        themed_color(hex, &state.appearance_mode, background)
    };
    let frame_color = |key: &str, fallback: &str| {
        themed_color(
            state
                .album_frame_theme_styles
                .iter()
                .find(|style| style.key == key)
                .map(|style| style.color.as_str())
                .unwrap_or(fallback),
            &state.appearance_mode,
            false,
        )
    };
    let selected_frame = target.strip_prefix("frame.").unwrap_or("complete");
    let banner_display = banner
        .map(|style| style.name.as_str())
        .unwrap_or("ARTIST")
        .chars()
        .take(10)
        .map(|character| character.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    UiArtistAlbumRow {
        key: "theme-preview".into(),
        title: "Album Card".into(),
        display_title: "Album Card".into(),
        year: "2026".into(),
        artwork: slint::Image::default(),
        status_color: frame_color(selected_frame, "#32DDA0"),
        selected: false,
        bullets: model(Vec::<UiAlbumStatusBullet>::new()),
        check_embedded: selected_frame == "thumbnail",
        validating: false,
        comp_loading: false,
        processed: selected_frame == "in_process",
        fully_complete: selected_frame == "complete",
        fallback: false,
        fallback_complete: false,
        featured: banner_key == "featuring",
        soundtrack: banner_key == "soundtrack",
        various_artists: banner_key == "various",
        separator_before: false,
        banner_display: banner_display.into(),
        banner_background: banner_color("background", "#006B3C", true),
        banner_frame: banner_color("frame", "#D9AD43", false),
        banner_letters: banner_color("letters", "#D9AD43", false),
        complete_frame: frame_color("complete", "#32DDA0"),
        pending_frame: frame_color("in_process", "#EFA54E"),
        attention_frame: frame_color("thumbnail", "#7C4DFF"),
        queue_locked: false,
    }
}

fn set_banner_theme_color(
    style: &mut BannerThemeStyle,
    part: &str,
    selected: String,
    synchronize_frame_letters: bool,
) -> bool {
    match part {
        "background" => style.background = selected.clone(),
        "frame" if !synchronize_frame_letters => style.frame = selected.clone(),
        "letters" if !synchronize_frame_letters => style.letters = selected.clone(),
        "frame" | "letters" => {}
        _ => return false,
    }
    if synchronize_frame_letters {
        style.frame = selected.clone();
        style.letters = selected;
    }
    true
}

fn apply_theme_selected_color(
    app: &mut ApplicationState,
    target: &str,
    selected: String,
    seed: u64,
) -> bool {
    if let Some(key) = target.strip_prefix("frame.") {
        if let Some(style) = app
            .album_frame_theme_styles
            .iter_mut()
            .find(|style| style.key == key)
        {
            style.color = selected;
            app.enhancement_settings_dirty = true;
            return true;
        }
        return false;
    }
    let Some((key, part)) = target.split_once('.') else {
        return false;
    };
    let synchronize = app.banner_sync_frame_letters;
    let random = app.banner_random_harmony;
    let light = app.appearance_mode.eq_ignore_ascii_case("light");
    if let Some(style) = app
        .banner_theme_styles
        .iter_mut()
        .find(|style| style.key == key)
    {
        if random {
            let (background, frame, letters) = banner_harmony(&selected, seed, light, synchronize);
            style.background = background;
            style.frame = frame;
            style.letters = letters;
        } else if !set_banner_theme_color(style, part, selected, synchronize) {
            return false;
        }
        app.enhancement_settings_dirty = true;
        return true;
    }
    false
}

fn rgb_to_hsv(rgb: [u8; 3]) -> (f32, f32, f32) {
    let [red, green, blue] = rgb.map(|channel| f32::from(channel) / 255.0);
    let maximum = red.max(green).max(blue);
    let minimum = red.min(green).min(blue);
    let difference = maximum - minimum;
    let hue = if difference <= f32::EPSILON {
        0.0
    } else if maximum == red {
        60.0 * ((green - blue) / difference).rem_euclid(6.0)
    } else if maximum == green {
        60.0 * ((blue - red) / difference + 2.0)
    } else {
        60.0 * ((red - green) / difference + 4.0)
    };
    let saturation = if maximum <= f32::EPSILON {
        0.0
    } else {
        difference / maximum
    };
    (hue, saturation, maximum)
}

fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> [u8; 3] {
    let hue = hue.rem_euclid(360.0);
    let saturation = saturation.clamp(0.0, 1.0);
    let value = value.clamp(0.0, 1.0);
    let chroma = value * saturation;
    let section = hue / 60.0;
    let secondary = chroma * (1.0 - (section.rem_euclid(2.0) - 1.0).abs());
    let (red, green, blue) = match section as u8 {
        0 => (chroma, secondary, 0.0),
        1 => (secondary, chroma, 0.0),
        2 => (0.0, chroma, secondary),
        3 => (0.0, secondary, chroma),
        4 => (secondary, 0.0, chroma),
        _ => (chroma, 0.0, secondary),
    };
    let offset = value - chroma;
    [red, green, blue].map(|channel| ((channel + offset) * 255.0).round() as u8)
}

fn hex_from_rgb(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

fn next_harmony_random(seed: &mut u64) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    (*seed % 10_000) as f32 / 10_000.0
}

fn banner_harmony(
    anchor: &str,
    seed: u64,
    light: bool,
    synchronize: bool,
) -> (String, String, String) {
    let (hue, saturation, value) = rgb_to_hsv(rgb_from_hex(anchor));
    let mut seed = seed.max(1);
    let direction = if next_harmony_random(&mut seed) < 0.5 {
        -1.0
    } else {
        1.0
    };
    let base_hue = hue + (next_harmony_random(&mut seed) - 0.5) * 24.0;
    let accent_hue = base_hue + direction * (15.0 + next_harmony_random(&mut seed) * 20.0);
    let background = hex_from_rgb(hsv_to_rgb(
        base_hue,
        saturation.max(0.56),
        if light {
            value.max(0.72)
        } else {
            value.clamp(0.32, 0.55)
        },
    ));
    let frame = hex_from_rgb(hsv_to_rgb(
        accent_hue,
        saturation.max(0.65),
        if light { 0.44 } else { 0.95 },
    ));
    let letters = if synchronize {
        frame.clone()
    } else {
        hex_from_rgb(hsv_to_rgb(
            accent_hue + direction * (5.0 + next_harmony_random(&mut seed) * 10.0),
            saturation.max(0.58),
            if light { 0.28 } else { 1.0 },
        ))
    };
    (background, frame, letters)
}

fn wheel_source() -> &'static image::RgbaImage {
    static WHEEL: OnceLock<image::RgbaImage> = OnceLock::new();
    WHEEL.get_or_init(|| {
        let decoded = image::load_from_memory(include_bytes!("../gui/oled-chroma-wheel.png"))
            .expect("the bundled OLED chroma wheel must decode")
            .to_rgba8();
        image::imageops::resize(&decoded, 384, 384, image::imageops::FilterType::Triangle)
    })
}

fn transformed_wheel_rgb(rgb: [u8; 3], hue_shift: u16, brightness: u8) -> [u8; 3] {
    let (hue, saturation, value) = rgb_to_hsv(rgb);
    hsv_to_rgb(
        hue + f32::from(hue_shift),
        saturation,
        value * f32::from(brightness) / 100.0,
    )
}

thread_local! {
    static THEME_WHEEL_RENDER_CACHE: RefCell<Option<((u16, u8), slint::Image)>> = const { RefCell::new(None) };
}

fn theme_wheel_image(hue_shift: u16, brightness: u8) -> slint::Image {
    THEME_WHEEL_RENDER_CACHE.with(|cache| {
        if let Some(((old_hue, old_brightness), image)) = cache.borrow().as_ref()
            && (*old_hue, *old_brightness) == (hue_shift, brightness)
        {
            return image.clone();
        }
        let source = wheel_source();
        let mut pixels = Vec::with_capacity((source.width() * source.height() * 4) as usize);
        for pixel in source.pixels() {
            let [red, green, blue] =
                transformed_wheel_rgb([pixel[0], pixel[1], pixel[2]], hue_shift, brightness);
            pixels.extend_from_slice(&[red, green, blue, pixel[3]]);
        }
        let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
            &pixels,
            source.width(),
            source.height(),
        );
        let image = slint::Image::from_rgba8(buffer);
        *cache.borrow_mut() = Some(((hue_shift, brightness), image.clone()));
        image
    })
}

fn wheel_contains(x: f32, y: f32) -> bool {
    (x - 0.5).powi(2) + (y - 0.5).powi(2) <= 0.44_f32.powi(2)
}

fn wheel_rgb(
    x: f32,
    y: f32,
    scroll_delta: f32,
    current: &str,
    hue_shift: u16,
    brightness: u8,
) -> String {
    if scroll_delta.abs() > f32::EPSILON {
        let mut rgb = rgb_from_hex(current);
        let multiplier = if scroll_delta > 0.0 { 1.07 } else { 0.93 };
        for channel in &mut rgb {
            *channel = (f32::from(*channel) * multiplier).round().clamp(0.0, 255.0) as u8;
        }
        return format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
    }
    if !wheel_contains(x, y) {
        return current.to_string();
    }
    let wheel = wheel_source();
    let pixel = wheel.get_pixel(
        (x.clamp(0.0, 1.0) * (wheel.width() - 1) as f32) as u32,
        (y.clamp(0.0, 1.0) * (wheel.height() - 1) as f32) as u32,
    );
    hex_from_rgb(transformed_wheel_rgb(
        [pixel[0], pixel[1], pixel[2]],
        hue_shift,
        brightness,
    ))
}

fn canonical_webp_status(snapshot: &MediaAlbumSnapshot) -> (bool, bool) {
    let has_name = |expected: &str| {
        snapshot.local_artwork_files.iter().any(|file| {
            Path::new(file)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case(expected))
        })
    };
    let webp = has_name("cover.webp") || snapshot.cover_name.eq_ignore_ascii_case("cover.webp");
    // Runtime writes update cover_name immediately; the cached local-art list
    // can lag until the next folder inventory.
    let still = has_name("cover.jpg")
        || has_name("cover.jpeg")
        || has_name("cover.png")
        || ["cover.jpg", "cover.jpeg", "cover.png"]
            .iter()
            .any(|name| snapshot.cover_name.eq_ignore_ascii_case(name));
    (webp, webp && !still)
}

/// A COMP card may show its folder cover for orientation, but the folder
/// image must not become the selected artwork for a track-scoped review.
fn artist_album_card_artwork_path(
    state: &ApplicationState,
    snapshot: &MediaAlbumSnapshot,
) -> String {
    if let Some(path) = state
        .library
        .album_artwork_paths
        .get(&snapshot.album_key.to_ascii_lowercase())
        .filter(|path| !path.trim().is_empty())
    {
        return path.clone();
    }
    if !snapshot.album_mbid.trim().is_empty() {
        return String::new();
    }
    let album_path = Path::new(&snapshot.path);
    let configured = state.config.output.file_name.trim();
    let mut names = vec![
        "cover.jpg".to_string(),
        "cover.png".to_string(),
        "cover.jpeg".to_string(),
        "cover.webp".to_string(),
    ];
    if !configured.is_empty() && !configured.eq_ignore_ascii_case("cover") {
        names.extend([
            format!("{configured}.jpg"),
            format!("{configured}.png"),
            format!("{configured}.jpeg"),
            format!("{configured}.webp"),
        ]);
    }
    names
        .into_iter()
        .map(|name| album_path.join(name))
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn artist_overview_ui_model(state: &ApplicationState) -> ModelRc<UiArtistAlbumRow> {
    let artist_mode = !state.library.show_tracks
        && state.library.view == LibraryView::Artists
        && state.library.focused_artist.is_some();
    let single_album_mode = state.library.focused_album_key.is_some()
        && (state.library.show_tracks || state.library.view == LibraryView::Albums);
    if !artist_mode && !single_album_mode {
        return model(Vec::new());
    }
    let overview_artist = state.library.focused_artist.as_deref().or_else(|| {
        state.library.focused_album_key.as_deref().and_then(|key| {
            state
                .library
                .albums
                .iter()
                .find(|album| album.snapshot.album_key.eq_ignore_ascii_case(key))
                .map(|album| album.snapshot.artist.as_str())
        })
    });
    let has_artist_art = overview_artist
        .and_then(|artist| {
            state
                .library
                .artist_artwork_paths
                .get(&artist.trim().to_ascii_lowercase())
        })
        .is_some_and(|path| !path.trim().is_empty());
    let mut indices = if single_album_mode && !artist_mode {
        state
            .library
            .focused_album_key
            .as_ref()
            .map(|key| {
                state
                    .library
                    .albums
                    .iter()
                    .enumerate()
                    .filter(|(_, album)| album.snapshot.album_key.eq_ignore_ascii_case(key))
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    } else {
        state.library.artist_overview_album_indices(
            &state.artist_overview_album_keys,
            state.artist_overview_show_all,
        )
    };
    indices.sort_by(|left, right| {
        let left_album = &state.library.albums[*left];
        let right_album = &state.library.albums[*right];
        artist_overview_sort_group(state, left_album)
            .cmp(&artist_overview_sort_group(state, right_album))
            .then_with(|| {
                artist_overview_release_year(&right_album.snapshot.release_year).cmp(
                    &artist_overview_release_year(&left_album.snapshot.release_year),
                )
            })
            .then_with(|| {
                left_album
                    .snapshot
                    .title
                    .to_ascii_lowercase()
                    .cmp(&right_album.snapshot.title.to_ascii_lowercase())
            })
            .then_with(|| {
                left_album
                    .snapshot
                    .album_key
                    .cmp(&right_album.snapshot.album_key)
            })
    });
    let mut previous_group = None;
    let rows = indices
        .into_iter()
        .filter_map(|index| state.library.albums.get(index))
        .map(|album| {
            let snapshot = &album.snapshot;
            let group = artist_overview_sort_group(state, album);
            let separator_before = previous_group.is_some_and(|previous| previous != group);
            previous_group = Some(group);
            let fallback = snapshot.album_mbid.trim().is_empty();
            let comp_loading = fallback
                && state.phase == RunPhase::Running
                && snapshot
                    .path
                    .eq_ignore_ascii_case(&state.current_album_path);
            let fallback_progress = if state.library.show_tracks && fallback {
                Some((
                    snapshot
                        .compilation_tracks
                        .iter()
                        .filter(|track| track.embedded_artwork_recorded)
                        .count(),
                    snapshot.compilation_tracks.len(),
                ))
            } else {
                state.artist_fallback_track_progress(album)
            };
            let fallback_complete =
                fallback_progress.is_some_and(|(completed, total)| total > 0 && completed == total);
            let featured = artist_overview_is_featured(state, album);
            let bypassed = album.state == splined_core::application::AlbumState::Bypassed;
            // A fallback album intentionally has no Release ID and does not
            // use its folder cover. Neither is an actionable red status for
            // the selected artist's track-scoped COMP workflow.
            let missing_album_art = !fallback && !snapshot.has_local_artwork;
            let incomplete = !fallback
                && matches!(
                    album.state,
                    splined_core::application::AlbumState::Incomplete
                        | splined_core::application::AlbumState::TimeoutActive
                );
            let processed = album.state == splined_core::application::AlbumState::Processed;
            let complete_art = !fallback && processed && has_artist_art && !missing_album_art;
            let (soundtrack, various_artists) = artist_overview_release_banners(snapshot);
            let banner_key = if featured {
                "featuring"
            } else if fallback {
                "compiled"
            } else if soundtrack {
                "soundtrack"
            } else if various_artists {
                "various"
            } else {
                "artist"
            };
            let banner_style = state
                .banner_theme_styles
                .iter()
                .find(|style| style.key == banner_key);
            let banner_text = banner_style
                .map(|style| style.name.as_str())
                .unwrap_or("ARTIST");
            let banner_display = banner_text
                .chars()
                .take(10)
                .map(|character| character.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            let banner_background = themed_color(
                banner_style
                    .map(|style| style.background.as_str())
                    .unwrap_or("#006B3C"),
                &state.appearance_mode,
                true,
            );
            let banner_frame = themed_color(
                banner_style
                    .map(|style| style.frame.as_str())
                    .unwrap_or("#D9AD43"),
                &state.appearance_mode,
                false,
            );
            let banner_letters = themed_color(
                banner_style
                    .map(|style| style.letters.as_str())
                    .unwrap_or("#D9AD43"),
                &state.appearance_mode,
                false,
            );
            let frame_color = |key: &str, fallback: &str| {
                themed_color(
                    state
                        .album_frame_theme_styles
                        .iter()
                        .find(|style| style.key == key)
                        .map(|style| style.color.as_str())
                        .unwrap_or(fallback),
                    &state.appearance_mode,
                    false,
                )
            };
            let embedded_status = album_embedded_check_cache().lock().ok().and_then(|cache| {
                cache
                    .get(&if fallback {
                        artist_fallback_embedded_check_key(
                            &snapshot.path,
                            state.config.output.embedded_thumbnail_size,
                            state.library.focused_artist.as_deref().unwrap_or_default(),
                        )
                    } else {
                        album_embedded_check_key(
                            &snapshot.path,
                            state.config.output.embedded_thumbnail_size,
                        )
                    })
                    .copied()
            });
            let embedded_check = if fallback && state.library.show_tracks {
                fallback_album_embedded_check_from_cache(
                    snapshot,
                    state.config.output.embedded_thumbnail_size,
                )
            } else {
                embedded_status.flatten()
            };
            let checking_embedded = embedded_check.is_none()
                && (processed || fallback_progress.is_some_and(|(completed, _)| completed > 0));
            // An unknown check is not evidence that an otherwise complete
            // album has regressed. Keep it out of the In Process count until
            // the background thumbnail validation returns a real result.
            let validating = embedded_check.is_none()
                && if fallback {
                    fallback_complete
                } else {
                    processed && snapshot.has_local_artwork
                };
            let check_embedded = embedded_check.is_some_and(AlbumEmbeddedCheck::needs_attention)
                || (checking_embedded && !validating);
            let fully_complete = artist_overview_is_fully_complete(
                fallback,
                fallback_complete,
                processed,
                snapshot.has_local_artwork,
                embedded_check,
            );
            let mut bullets = Vec::new();
            let mut add_bullet = |applicable: bool, color: Color, tip: String| {
                if applicable {
                    bullets.push(UiAlbumStatusBullet {
                        color,
                        tip: SharedString::from(tip),
                    });
                }
            };
            add_bullet(
                !has_artist_art,
                Color::from_rgb_u8(244, 204, 77),
                "Missing Artist Art".to_string(),
            );
            add_bullet(
                bypassed,
                Color::from_rgb_u8(244, 204, 77),
                "Bypassed Album".to_string(),
            );
            add_bullet(
                missing_album_art,
                Color::from_rgb_u8(255, 82, 104),
                "Missing Album Art".to_string(),
            );
            add_bullet(
                incomplete
                    || (fallback && fallback_progress.is_some_and(|(done, total)| done < total)),
                Color::from_rgb_u8(255, 82, 104),
                if fallback {
                    "Selected Artist's COMP tracks incomplete"
                } else {
                    "Incomplete / timed-out Album"
                }
                .to_string(),
            );
            add_bullet(
                processed && !fully_complete && !validating,
                Color::from_rgb_u8(239, 165, 78),
                "Processed Album".to_string(),
            );
            add_bullet(
                complete_art || fallback_complete,
                Color::from_rgb_u8(50, 221, 160),
                if fallback_complete {
                    "Selected Artist's COMP tracks complete"
                } else {
                    "Completed Artist & Album Art"
                }
                .to_string(),
            );
            if let Some(check) = embedded_check {
                add_bullet(
                    check.needs_attention(),
                    Color::from_rgb_u8(124, 77, 255),
                    format!(
                        "Check Embedded · {} missing, {} wrong size; preference {} × {}",
                        check.missing,
                        check.wrong_size,
                        state.config.output.embedded_thumbnail_size,
                        state.config.output.embedded_thumbnail_size
                    ),
                );
            } else if checking_embedded && !validating {
                add_bullet(
                    true,
                    Color::from_rgb_u8(124, 77, 255),
                    "Checking Embedded Art against the thumbnail preference…".to_string(),
                );
            }
            let (has_webp, needs_still) = canonical_webp_status(snapshot);
            add_bullet(
                has_webp,
                Color::from_rgb_u8(52, 120, 255),
                if needs_still {
                    "cover.webp preserved · WRITE Cover to create cover.jpg and embed a still"
                } else {
                    "cover.webp preserved alongside a still cover"
                }
                .to_string(),
            );
            UiArtistAlbumRow {
                key: SharedString::from(snapshot.album_key.clone()),
                title: SharedString::from(snapshot.title.clone()),
                display_title: SharedString::from(artist_album_card_display_title(&snapshot.title)),
                year: SharedString::from(snapshot.release_year.clone()),
                artwork: load_scaled_image(&artist_album_card_artwork_path(state, snapshot), 192),
                status_color: if fully_complete {
                    Color::from_rgb_u8(50, 221, 160)
                } else if featured && fallback_complete {
                    Color::from_rgb_u8(255, 151, 56)
                } else {
                    library_status_color(album.state.label())
                },
                selected: album.selected
                    && !state
                        .embed_locked_album_paths
                        .contains(&snapshot.path.to_lowercase()),
                bullets: model(bullets),
                check_embedded,
                validating,
                comp_loading,
                processed,
                fully_complete,
                fallback,
                fallback_complete,
                featured,
                soundtrack,
                various_artists,
                separator_before,
                banner_display: banner_display.into(),
                banner_background,
                banner_frame,
                banner_letters,
                complete_frame: frame_color("complete", "#32DDA0"),
                pending_frame: frame_color("in_process", "#EFA54E"),
                attention_frame: frame_color("thumbnail", "#7C4DFF"),
                queue_locked: state
                    .embed_locked_album_paths
                    .contains(&snapshot.path.to_lowercase()),
            }
        })
        .collect::<Vec<_>>();
    model(rows)
}

fn artist_overview_legend_counts(
    albums: &ModelRc<UiArtistAlbumRow>,
    tracks: &[UiCompilationTrackRow],
) -> UiArtistOverviewLegendCounts {
    let mut counts = UiArtistOverviewLegendCounts::default();
    for index in 0..albums.row_count() {
        let Some(album) = albums.row_data(index) else {
            continue;
        };
        counts.shown += 1;
        if album.validating {
            counts.validating += 1;
        } else if album.fully_complete {
            counts.completed += 1;
            counts.green_frames += 1;
        } else if album.check_embedded
            && (album.processed || (album.fallback && album.fallback_complete))
        {
            if album.fallback && album.fallback_complete {
                counts.green_purple_frames += 1;
            } else {
                counts.orange_purple_frames += 1;
            }
        }
        for bullet_index in 0..album.bullets.row_count() {
            let Some(bullet) = album.bullets.row_data(bullet_index) else {
                continue;
            };
            match bullet.color {
                color if color == Color::from_rgb_u8(255, 82, 104) => counts.red_dots += 1,
                color if color == Color::from_rgb_u8(244, 204, 77) => counts.yellow_dots += 1,
                color if color == Color::from_rgb_u8(239, 165, 78) => counts.orange_dots += 1,
                color if color == Color::from_rgb_u8(50, 221, 160) => counts.green_dots += 1,
                color if color == Color::from_rgb_u8(124, 77, 255) => counts.purple_dots += 1,
                color if color == Color::from_rgb_u8(52, 120, 255) => counts.blue_dots += 1,
                _ => {}
            }
        }
        // Mirror the visible banner's priority, not merely the underlying
        // metadata: a secondary credit replaces the album-type banner.
        if album.featured {
            counts.feat += 1;
        } else if album.fallback {
            if album.fallback_complete {
                counts.comp_complete += 1;
            } else {
                counts.comp_pending += 1;
            }
        } else if album.soundtrack {
            counts.ost += 1;
        } else if album.various_artists {
            counts.various += 1;
        } else {
            counts.artist += 1;
        }
    }
    counts.in_process = counts.shown - counts.completed - counts.validating;
    for track in tracks {
        if track.status_color == Color::from_rgb_u8(124, 77, 255) {
            counts.track_purple += 1;
        } else if track.completed {
            counts.track_green += 1;
        } else {
            counts.track_blue += 1;
        }
    }
    counts
}

// Card order is presentation-only: do not change the library's selection or
// LAUNCH order when grouping the Artist Overview by album type.
fn artist_overview_sort_group(state: &ApplicationState, album: &AlbumEntry) -> u8 {
    if artist_overview_is_featured(state, album) {
        return 4; // FEAT is its own group after V/A, even on another album type.
    }
    if album.snapshot.album_mbid.trim().is_empty() {
        return 5; // COMP fallback, even if its album status is unprocessed.
    }
    let (soundtrack, various_artists) = artist_overview_release_banners(&album.snapshot);
    if soundtrack {
        2
    } else if various_artists {
        3
    } else if album.state == AlbumState::New {
        0
    } else {
        1
    }
}

fn artist_overview_is_featured(state: &ApplicationState, album: &AlbumEntry) -> bool {
    state
        .artist_overview_featured_album_keys
        .contains(&album.snapshot.album_key.to_ascii_lowercase())
        || state
            .library
            .focused_artist
            .as_deref()
            .is_some_and(|artist| {
                splined_core::application::LibraryState::artist_fallback_is_featured(album, artist)
            })
}

fn artist_overview_release_year(value: &str) -> i32 {
    value
        .trim()
        .chars()
        .take(4)
        .collect::<String>()
        .parse()
        .unwrap_or_default()
}

fn artist_overview_release_banners(
    snapshot: &splined_core::media_database::MediaAlbumSnapshot,
) -> (bool, bool) {
    if snapshot.album_mbid.trim().is_empty() {
        return (false, false);
    }
    let path_is_soundtrack = snapshot.path.split(['/', '\\']).any(|part| {
        part.eq_ignore_ascii_case("[Soundtracks]") || part.eq_ignore_ascii_case("Soundtracks")
    });
    let title = snapshot.title.to_ascii_lowercase();
    let soundtrack = path_is_soundtrack
        || title.contains("soundtrack")
        || title.contains("original motion picture")
        || title.contains("music from the motion picture")
        || title.ends_with(" ost");
    let various_artists = !soundtrack
        && snapshot
            .artist
            .trim()
            .trim_matches(['[', ']'])
            .eq_ignore_ascii_case("Various Artists");
    (soundtrack, various_artists)
}

fn artist_overview_is_fully_complete(
    fallback: bool,
    fallback_complete: bool,
    processed: bool,
    has_local_artwork: bool,
    embedded_check: Option<AlbumEmbeddedCheck>,
) -> bool {
    embedded_check.is_some_and(|check| check.tracks > 0 && !check.needs_attention())
        && if fallback {
            fallback_complete
        } else {
            processed && has_local_artwork
        }
}

fn library_status_color(status: &str) -> Color {
    match status.trim().to_ascii_lowercase().as_str() {
        "processed" => status_filter_color(StatusFilter::Processed),
        "bypassed" => status_filter_color(StatusFilter::Bypassed),
        "partial / error" | "partial / timeout" => {
            status_filter_color(StatusFilter::PartialTimeout)
        }
        "incomplete" => status_filter_color(StatusFilter::Incomplete),
        "artist complete" => status_filter_color(StatusFilter::ArtistComplete),
        "artist bypass" | "artist contains bypass" => {
            status_filter_color(StatusFilter::ArtistContainsBypass)
        }
        _ => status_filter_color(StatusFilter::Unprocessed),
    }
}

fn artist_is_secondary_credit(track_artist: &str, selected_artist: &str) -> bool {
    splined_core::application::LibraryState::artist_credit_is_secondary(
        track_artist,
        selected_artist,
    )
}

fn compilation_track_ui_rows(state: &ApplicationState) -> Vec<UiCompilationTrackRow> {
    let side = state.config.output.embedded_thumbnail_size;
    let metadata = EMBEDDED_METADATA_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok();
    state
        .library
        .compilation_track_rows()
        .into_iter()
        .map(|track| {
            let recorded = metadata
                .as_ref()
                .and_then(|cache| cache.get(&track.path).map(|(_, _, dimensions)| *dimensions));
            let needs_attention = track.embedded_artwork_recorded
                && completed_compilation_track_needs_attention(recorded, side);
            UiCompilationTrackRow {
                album_key: SharedString::from(track.album_key),
                path: SharedString::from(track.path),
                file_name: SharedString::from(track.file_name),
                status_color: if needs_attention {
                    Color::from_rgb_u8(124, 77, 255)
                } else {
                    status_filter_color(if track.embedded_artwork_recorded {
                        StatusFilter::ArtistComplete
                    } else {
                        StatusFilter::Incomplete
                    })
                },
                completed: track.embedded_artwork_recorded,
                selected: track.selected,
            }
        })
        .collect()
}

fn completed_compilation_track_needs_attention(
    metadata: Option<Option<(u32, u32, u64)>>,
    side: u32,
) -> bool {
    metadata.is_some_and(|dimensions| {
        dimensions.is_none_or(|(width, height, _)| width != side || height != side)
    })
}

fn status_filter_ui_rows(state: &ApplicationState) -> Vec<UiStatusFilterRow> {
    [
        StatusFilter::All,
        StatusFilter::Unprocessed,
        StatusFilter::Incomplete,
        StatusFilter::Processed,
        StatusFilter::Bypassed,
        StatusFilter::PartialTimeout,
        StatusFilter::ArtistComplete,
        StatusFilter::ArtistContainsBypass,
    ]
    .into_iter()
    .map(|filter| UiStatusFilterRow {
        key: SharedString::from(filter.label()),
        label: SharedString::from(state.library.status_label(filter)),
        count: state.library.focused_status_count(filter) as i32,
        enabled: state.library.status_filter_enabled(filter),
        semantic_color: status_filter_color(filter),
    })
    .collect()
}

fn byte_size_label(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn current_embedded_thumbnail_infos(app: &ApplicationState) -> Vec<Option<(u32, u32, u64)>> {
    if app.compilation_report_preview_mode
        && !app.compilation_report_preview_track_path.trim().is_empty()
    {
        // This runs on the UI thread. The preview worker fills the metadata
        // cache from the extracted thumb; never reopen the audio file here.
        let info = EMBEDDED_METADATA_CACHE
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .ok()
            .and_then(|cache| {
                cache
                    .get(&app.compilation_report_preview_track_path)
                    .map(|(_, _, dimensions)| *dimensions)
            });
        return info.map(|dimensions| vec![dimensions]).unwrap_or_default();
    }
    let tracks = if !app.current_compilation_track_path.trim().is_empty() {
        vec![PathBuf::from(&app.current_compilation_track_path)]
    } else if !app.current_album_path.trim().is_empty() {
        fs::read_dir(&app.current_album_path)
            .ok()
            .map(|entries| {
                let mut tracks = entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| {
                        path.is_file() && splined_core::scan::is_supported_audio_file(path)
                    })
                    .collect::<Vec<_>>();
                tracks.sort();
                tracks
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    tracks
        .iter()
        .map(|track| cached_embedded_front_metadata(track))
        .collect()
}

fn embedded_artwork_footer(
    infos: &[Option<(u32, u32, u64)>],
    preference: u32,
) -> (String, String, String, String, Color) {
    if infos.is_empty() {
        return (
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            Color::from_rgb_u8(150, 175, 195),
        );
    }
    let dimensions = infos
        .iter()
        .flatten()
        .map(|(width, height, _)| (*width, *height))
        .collect::<Vec<_>>();
    let below_preference = infos.iter().any(|info| {
        info.is_none_or(|(width, height, _)| width < preference || height < preference)
    });
    let varying = dimensions
        .first()
        .is_some_and(|first| dimensions.iter().any(|dimension| dimension != first));
    let resolution = if dimensions.is_empty() {
        "No embedded art".to_string()
    } else if varying {
        "Varying Size".to_string()
    } else {
        format!("{} × {}", dimensions[0].0, dimensions[0].1)
    };
    let color = if below_preference {
        Color::from_rgb_u8(255, 82, 104)
    } else if varying {
        Color::from_rgb_u8(239, 165, 78)
    } else {
        Color::from_rgb_u8(50, 221, 160)
    };
    (
        format!("Track(s) ({})", infos.len()),
        "Embedded Track(s)".to_string(),
        resolution,
        byte_size_label(infos.iter().flatten().map(|(_, _, bytes)| *bytes).sum()),
        color,
    )
}

fn embedded_artwork_size_comparison(
    thumbnail_ready: bool,
    prepared_bytes: Option<u64>,
    infos: &[Option<(u32, u32, u64)>],
) -> Option<(String, String)> {
    if !thumbnail_ready || infos.is_empty() {
        return None;
    }
    let prepared_bytes = prepared_bytes?;
    let after_total = prepared_bytes.saturating_mul(infos.len() as u64);
    let before_total = infos
        .iter()
        .flatten()
        .map(|(_, _, bytes)| *bytes)
        .sum::<u64>();
    Some((
        format!("~{}", byte_size_label(after_total)),
        byte_size_label(before_total),
    ))
}

fn format_existing_embedded_info(infos: &[Option<(u32, u32, u64)>]) -> String {
    let largest = infos
        .iter()
        .flatten()
        .max_by_key(|(width, height, bytes)| (u64::from(*width) * u64::from(*height), *bytes));
    let Some((width, height, bytes)) = largest else {
        return "No embedded thumb".to_string();
    };
    let varying = infos
        .iter()
        .any(|info| info.as_ref() != Some(&(*width, *height, *bytes)));
    format!(
        "Embedded Art (now): {width} × {height} · {}{}",
        byte_size_label(*bytes),
        if varying { " · Varying Size" } else { "" }
    )
}

fn cached_embedded_front_metadata(track: &Path) -> Option<(u32, u32, u64)> {
    let Ok(metadata) = fs::metadata(track) else {
        return None;
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    let key = track.to_string_lossy().into_owned();
    let cache = EMBEDDED_METADATA_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let cached = cache.lock().ok().and_then(|cache| {
        cache.get(&key).and_then(|(size, time, info)| {
            (*size == metadata.len() && *time == modified).then_some(*info)
        })
    });
    if let Some(info) = cached {
        info
    } else {
        let info = embedded_front_metadata(track).ok().flatten();
        if let Ok(mut cache) = cache.lock() {
            cache.insert(key, (metadata.len(), modified, info));
        }
        info
    }
}

fn invalidate_embedded_track_metadata(track: &Path) {
    if let Some(cache) = EMBEDDED_METADATA_CACHE.get()
        && let Ok(mut cache) = cache.lock()
    {
        cache.remove(track.to_string_lossy().as_ref());
    }
}

fn schedule_visible_compilation_track_checks(
    weak: &slint::Weak<AppWindow>,
    state: &Arc<Mutex<ApplicationState>>,
) {
    let visible = state
        .lock()
        .ok()
        .map(|app| {
            let mut paths = app
                .library
                .compilation_track_rows()
                .into_iter()
                .filter(|track| track.embedded_artwork_recorded)
                .map(|track| track.path)
                .collect::<Vec<_>>();
            if app.library.show_tracks
                && let Some(key) = app.library.focused_album_key.as_deref()
                && let Some(album) = app.library.albums.iter().find(|album| {
                    album.snapshot.album_key.eq_ignore_ascii_case(key)
                        && album.snapshot.album_mbid.trim().is_empty()
                })
            {
                paths.extend(
                    album
                        .snapshot
                        .compilation_tracks
                        .iter()
                        .filter(|track| track.embedded_artwork_recorded)
                        .map(|track| track.path.clone()),
                );
                paths.sort();
                paths.dedup();
            }
            paths
        })
        .unwrap_or_default();
    if visible.is_empty() {
        return;
    }
    let cache = EMBEDDED_METADATA_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let uncached = cache
        .lock()
        .ok()
        .map(|cache| {
            visible
                .into_iter()
                .filter(|path| !cache.contains_key(path))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let pending = VISIBLE_COMPILATION_CHECKS_PENDING.get_or_init(|| Mutex::new(HashSet::new()));
    let targets = pending
        .lock()
        .ok()
        .map(|mut pending| {
            uncached
                .into_iter()
                .filter(|path| pending.insert(path.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if targets.is_empty() {
        return;
    }
    let weak = weak.clone();
    let state = Arc::clone(state);
    std::thread::spawn(move || {
        for path in targets {
            let file = Path::new(&path);
            if file.is_file() {
                cached_embedded_front_metadata(file);
            } else if let Some(cache) = EMBEDDED_METADATA_CACHE.get()
                && let Ok(mut cache) = cache.lock()
            {
                cache.insert(path.clone(), (0, 0, None));
            }
            if let Some(pending) = VISIBLE_COMPILATION_CHECKS_PENDING.get()
                && let Ok(mut pending) = pending.lock()
            {
                pending.remove(&path);
            }
        }
        let _ = slint::invoke_from_event_loop(move || refresh_weak(&weak, &state));
    });
}

fn album_embedded_check_key(path: &str, side: u32) -> String {
    format!("{}|{side}", path.to_ascii_lowercase())
}

fn artist_fallback_embedded_check_key(path: &str, side: u32, artist: &str) -> String {
    format!(
        "{}|{side}|{}",
        path.to_ascii_lowercase(),
        artist.trim().to_ascii_lowercase()
    )
}

fn artist_fallback_embedded_track_paths(
    state: &ApplicationState,
    album: &splined_core::application::AlbumEntry,
) -> Vec<PathBuf> {
    let artist = state.library.focused_artist.as_deref().unwrap_or_default();
    let mut paths = album
        .snapshot
        .compilation_tracks
        .iter()
        .filter(|track| {
            track
                .artist
                .split(['\0', ';'])
                .any(|credit| credit.trim().eq_ignore_ascii_case(artist))
        })
        .map(|track| PathBuf::from(&track.path))
        .collect::<Vec<_>>();
    let album_key = album.snapshot.album_key.to_ascii_lowercase();
    if let Some(remote) = state.artist_overview_track_paths.get(&album_key) {
        for path in remote {
            if !paths
                .iter()
                .any(|local: &PathBuf| local.to_string_lossy().eq_ignore_ascii_case(path))
            {
                paths.push(PathBuf::from(path));
            }
        }
    }
    paths
}

fn artist_compilation_report_tracks(app: &ApplicationState) -> Vec<ArtistCompReportTrack> {
    if app.library.view != LibraryView::Artists || app.library.focused_artist.is_none() {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    app.library
        .artist_overview_album_indices(&app.artist_overview_album_keys, true)
        .into_iter()
        .filter_map(|index| app.library.albums.get(index))
        .filter(|album| album.snapshot.album_mbid.trim().is_empty())
        .flat_map(|album| {
            let allowed = artist_fallback_embedded_track_paths(app, album)
                .into_iter()
                .map(|path| path.to_string_lossy().to_ascii_lowercase())
                .collect::<HashSet<_>>();
            album
                .snapshot
                .compilation_tracks
                .iter()
                .filter(move |track| allowed.contains(&track.path.to_ascii_lowercase()))
                .map(move |track| ArtistCompReportTrack {
                    album_key: album.snapshot.album_key.clone(),
                    album_title: album.snapshot.title.clone(),
                    album_path: PathBuf::from(&album.snapshot.path),
                    path: PathBuf::from(&track.path),
                    completed: track.embedded_artwork_recorded,
                })
        })
        .filter(|track| seen.insert(track.path.to_string_lossy().to_ascii_lowercase()))
        .collect()
}

fn select_pending_artist_compilation_report_tracks(
    app: &mut ApplicationState,
    include_bypassed: bool,
) -> Result<usize, String> {
    let artist = app
        .library
        .focused_artist
        .as_deref()
        .filter(|artist| artist.eq_ignore_ascii_case(&app.compilation_report_artist))
        .ok_or_else(|| "Open the current Artist's COMP report before launching it.".to_string())?;
    let artist = artist.to_string();
    let non_green_paths = artist_compilation_report_track_ui_rows(app)
        .into_iter()
        .filter(|row| row.status.as_str() != "Green")
        .map(|row| row.path.as_str().to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let tracks = artist_compilation_report_tracks(app)
        .into_iter()
        .filter(|track| {
            non_green_paths.contains(&track.path.to_string_lossy().to_ascii_lowercase())
        })
        .collect::<Vec<_>>();
    if tracks.is_empty() {
        app.operation_status = "All reported COMP tracks are green.".into();
        app.operation_tone = "green".into();
        return Ok(0);
    }
    let mut album_indices = Vec::with_capacity(tracks.len());
    for track in &tracks {
        let index = app
            .library
            .albums
            .iter()
            .position(|album| {
                album
                    .snapshot
                    .album_key
                    .eq_ignore_ascii_case(&track.album_key)
                    && Path::new(&album.snapshot.path) == track.album_path
                    && album.snapshot.album_mbid.trim().is_empty()
            })
            .ok_or_else(|| {
                format!(
                    "COMP album {} changed; reopen the report.",
                    track.album_title
                )
            })?;
        let album = &app.library.albums[index];
        if album.state == AlbumState::TimeoutActive {
            return Err(format!(
                "COMP album {} is in timeout; reopen it later.",
                track.album_title
            ));
        }
        if album.state == AlbumState::Bypassed && !include_bypassed {
            return Err(format!(
                "COMP album {} is bypassed; enable bypass override first.",
                track.album_title
            ));
        }
        album_indices.push(index);
    }
    app.library.select_none();
    for (track, index) in tracks.iter().zip(album_indices) {
        let album = &mut app.library.albums[index];
        album.selected = true;
        album.bypass_override = album.state == AlbumState::Bypassed;
        app.library
            .selected_compilation_track_paths
            .insert(track.path.to_string_lossy().to_ascii_lowercase());
    }
    app.artist_artwork_mode = false;
    app.scan_scope = ScanScope::Selected;
    app.operation_status = format!(
        "COMP queued · {} non-green {artist} track{} across the report",
        tracks.len(),
        if tracks.len() == 1 { "" } else { "s" }
    );
    app.operation_tone = "cyan".into();
    Ok(tracks.len())
}

fn launch_artist_compilation_report_tracks(
    state: &Arc<Mutex<ApplicationState>>,
    controller: &Arc<ScanController>,
    include_bypassed: bool,
) -> Result<(), String> {
    if controller.is_running() {
        return Err("Finish the active review before launching COMP tracks.".into());
    }
    let count = {
        let mut app = state
            .lock()
            .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
        if app.operation_busy || app.phase != RunPhase::Idle {
            return Err("Finish the active operation before launching COMP tracks.".into());
        }
        select_pending_artist_compilation_report_tracks(&mut app, include_bypassed)?
    };
    if count > 0 {
        controller.start()?;
    }
    Ok(())
}

fn artist_compilation_thumb_repair_plan(
    app: &ApplicationState,
) -> Result<(CompilationThumbRepairPlan, Vec<ArtistCompReportTrack>), String> {
    let artist = app
        .library
        .focused_artist
        .as_deref()
        .ok_or_else(|| "Select an Artist before compiling the COMP report.".to_string())?;
    let report = artist_compilation_report_tracks(app);
    if report.is_empty() {
        return Err("This Artist has no indexed COMP tracks to report or repair.".into());
    }
    let mut album_paths = Vec::new();
    for track in &report {
        if !album_paths.contains(&track.album_path) {
            album_paths.push(track.album_path.clone());
        }
    }
    let targets = report
        .iter()
        .filter(|track| track.completed)
        .map(|track| track.path.clone())
        .collect();
    Ok((
        CompilationThumbRepairPlan {
            album_key: String::new(),
            album_path: PathBuf::new(),
            album_paths,
            artist_wide: true,
            artist: artist.to_string(),
            selection_epoch: app.library.selection_epoch,
            side: app.config.output.embedded_thumbnail_size,
            cache_dir: PathBuf::from(&app.config.scan.temporary_cache_dir),
            targets,
            missing: 0,
        },
        report,
    ))
}

fn artist_compilation_report_track_ui_rows(app: &ApplicationState) -> Vec<UiCompilationReportRow> {
    if !app
        .library
        .focused_artist
        .as_deref()
        .is_some_and(|artist| artist.eq_ignore_ascii_case(&app.compilation_report_artist))
    {
        return Vec::new();
    }
    let side = app.config.output.embedded_thumbnail_size;
    let metadata = EMBEDDED_METADATA_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok();
    artist_compilation_report_tracks(app)
        .into_iter()
        .map(|track| {
            let recorded = metadata.as_ref().and_then(|cache| {
                cache
                    .get(track.path.to_string_lossy().as_ref())
                    .map(|(_, _, dimensions)| *dimensions)
            });
            let status = if !track.completed {
                "Blue"
            } else if recorded.is_some_and(|dimensions| {
                dimensions.is_some_and(|(width, height, _)| width == side && height == side)
            }) {
                "Green"
            } else {
                "Purple"
            };
            UiCompilationReportRow {
                album_key: track.album_key.into(),
                album_title: track.album_title.into(),
                file_name: track
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .into(),
                path: track.path.to_string_lossy().into_owned().into(),
                status: status.into(),
                album_header: false,
                expanded: false,
            }
        })
        .collect()
}

fn artist_compilation_report_ui_rows(app: &ApplicationState) -> Vec<UiCompilationReportRow> {
    let tracks = artist_compilation_report_track_ui_rows(app);
    group_compilation_report_rows(&tracks, &app.compilation_report_collapsed_albums)
}

fn fallback_album_report_track_ui_rows(app: &ApplicationState) -> Vec<UiCompilationReportRow> {
    let Some(key) = app.library.focused_album_key.as_deref() else {
        return Vec::new();
    };
    let Some(album) = app.library.albums.iter().find(|album| {
        album.snapshot.album_key.eq_ignore_ascii_case(key)
            && album.snapshot.album_mbid.trim().is_empty()
    }) else {
        return Vec::new();
    };
    let side = app.config.output.embedded_thumbnail_size;
    let metadata = EMBEDDED_METADATA_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok();
    let tracks = album
        .snapshot
        .compilation_tracks
        .iter()
        .map(|track| {
            let recorded = metadata
                .as_ref()
                .and_then(|cache| cache.get(&track.path).map(|(_, _, dimensions)| *dimensions));
            let status = if !track.embedded_artwork_recorded {
                "Blue"
            } else if recorded.is_some_and(|dimensions| {
                dimensions.is_some_and(|(width, height, _)| width == side && height == side)
            }) {
                "Green"
            } else {
                "Purple"
            };
            UiCompilationReportRow {
                album_key: key.into(),
                album_title: album.snapshot.title.clone().into(),
                path: track.path.clone().into(),
                file_name: Path::new(&track.path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(&track.path)
                    .into(),
                status: status.into(),
                album_header: false,
                expanded: false,
            }
        })
        .collect::<Vec<_>>();
    tracks
}

fn group_compilation_report_rows(
    tracks: &[UiCompilationReportRow],
    collapsed_albums: &HashSet<String>,
) -> Vec<UiCompilationReportRow> {
    let mut rows = Vec::with_capacity(tracks.len() * 2);
    let mut start = 0;
    while start < tracks.len() {
        let album_key = tracks[start].album_key.clone();
        let mut end = start + 1;
        while end < tracks.len() && tracks[end].album_key == album_key {
            end += 1;
        }
        let group = &tracks[start..end];
        let purple = group
            .iter()
            .filter(|row| row.status.as_str() == "Purple")
            .count();
        let blue = group
            .iter()
            .filter(|row| row.status.as_str() == "Blue")
            .count();
        let expanded = !collapsed_albums.contains(&album_key.to_ascii_lowercase());
        rows.push(UiCompilationReportRow {
            album_key: album_key.clone(),
            album_title: group[0].album_title.clone(),
            path: SharedString::default(),
            file_name: format!(
                "{} track{} · {} green{}{}",
                group.len(),
                if group.len() == 1 { "" } else { "s" },
                group.len() - purple - blue,
                if purple > 0 {
                    format!(", {purple} thumb check")
                } else {
                    String::new()
                },
                if blue > 0 {
                    format!(", {blue} need artwork")
                } else {
                    String::new()
                },
            )
            .into(),
            status: if purple > 0 {
                "Purple"
            } else if blue > 0 {
                "Blue"
            } else {
                "Green"
            }
            .into(),
            album_header: true,
            expanded,
        });
        if expanded {
            rows.extend(group.iter().cloned().map(|mut track| {
                track.album_title = SharedString::default();
                track
            }));
        }
        start = end;
    }
    rows
}

fn close_artist_compilation_report(app: &mut ApplicationState) {
    app.compilation_report_artist.clear();
    app.compilation_report_summary.clear();
    app.compilation_report_preview_mode = false;
    app.compilation_report_preview_album_key.clear();
    app.compilation_report_preview_track_path.clear();
    app.compilation_report_collapsed_albums.clear();
    app.compilation_report_auto_close_armed = false;
    app.compilation_report_close_pending = false;
    app.compilation_report_generation = app.compilation_report_generation.wrapping_add(1);
}

fn compilation_thumb_repair_plan(
    app: &ApplicationState,
    album_key: &str,
) -> Result<CompilationThumbRepairPlan, String> {
    if app.library.view != LibraryView::Artists && !app.library.show_tracks {
        return Err("Open the COMP album before repairing its thumbs.".into());
    }
    let album = app
        .library
        .albums
        .iter()
        .find(|album| album.snapshot.album_key.eq_ignore_ascii_case(album_key))
        .ok_or_else(|| "The selected COMP album is no longer available.".to_string())?;
    if !album.snapshot.album_mbid.trim().is_empty() {
        return Err("Only release-ID-free COMP albums use this track repair.".into());
    }
    let artist = if app.library.show_tracks {
        album.snapshot.artist.as_str()
    } else {
        app.library
            .focused_artist
            .as_deref()
            .ok_or_else(|| "Select an Artist before repairing COMP thumbs.".to_string())?
    };
    let complete = album
        .snapshot
        .compilation_tracks
        .iter()
        .filter(|track| track.embedded_artwork_recorded)
        .map(|track| track.path.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let target_paths = if app.library.show_tracks {
        album
            .snapshot
            .compilation_tracks
            .iter()
            .map(|track| PathBuf::from(&track.path))
            .collect::<Vec<_>>()
    } else {
        artist_fallback_embedded_track_paths(app, album)
    };
    let targets = target_paths
        .into_iter()
        .filter(|path| complete.contains(&path.to_string_lossy().to_ascii_lowercase()))
        .collect();
    Ok(CompilationThumbRepairPlan {
        album_key: album.snapshot.album_key.clone(),
        album_path: PathBuf::from(&album.snapshot.path),
        album_paths: vec![PathBuf::from(&album.snapshot.path)],
        artist_wide: false,
        artist: artist.to_string(),
        selection_epoch: app.library.selection_epoch,
        side: app.config.output.embedded_thumbnail_size,
        cache_dir: PathBuf::from(&app.config.scan.temporary_cache_dir),
        targets,
        missing: 0,
    })
}

fn inspect_compilation_thumb_repair(
    mut plan: CompilationThumbRepairPlan,
) -> Result<CompilationThumbRepairPlan, String> {
    let mut wrong_size = Vec::new();
    for track in &plan.targets {
        match embedded_front_metadata(track) {
            Ok(Some((width, height, _))) if width != plan.side || height != plan.side => {
                wrong_size.push(track.clone());
            }
            Ok(None) | Err(_) => plan.missing += 1,
            _ => {}
        }
    }
    plan.targets = wrong_size;
    if plan.targets.is_empty() {
        return Err(if plan.missing > 0 {
            "No completed COMP thumb can be resized; missing/unreadable art requires an artwork search."
                .into()
        } else {
            "Completed COMP tracks already match the Embedded Thumbnail Preference.".into()
        });
    }
    Ok(plan)
}

fn repair_compilation_thumb_from_embedded(
    track: &Path,
    cache_dir: &Path,
    side: u32,
) -> Result<bool, String> {
    match embedded_front_metadata(track)? {
        Some((width, height, _)) if width == side && height == side => return Ok(false),
        None => return Err(format!("{} has no embedded front art", track.display())),
        _ => {}
    }
    let candidate = embedded_candidate(track, cache_dir)?
        .ok_or_else(|| format!("{} has no usable embedded artwork", track.display()))?;
    let bytes = fs::read(candidate.path())
        .map_err(|error| format!("Unable to read current embedded art: {error}"))?;
    let format = match candidate.candidate.format {
        StaticFormat::Png => StaticFormat::Png,
        _ => StaticFormat::Jpeg,
    };
    let thumbnail = prepare_embedded_thumbnail(&bytes, side, format)?;
    replace_embedded_front(track, &thumbnail)?;
    if !embedded_front_matches(track, &thumbnail)?
        || !embedded_front_metadata(track)?
            .is_some_and(|(width, height, _)| width == side && height == side)
    {
        return Err(format!(
            "The repaired embedded art could not be verified for {}",
            track.display()
        ));
    }
    Ok(true)
}

fn artist_fallback_embedded_check(paths: &[PathBuf], side: u32) -> AlbumEmbeddedCheck {
    let mut result = AlbumEmbeddedCheck::default();
    for track in paths {
        result.tracks += 1;
        match cached_embedded_front_metadata(track) {
            None => result.missing += 1,
            Some((width, height, _)) if width != side || height != side => {
                result.wrong_size += 1;
            }
            _ => {}
        }
    }
    result
}

fn album_embedded_check(path: &Path, side: u32) -> AlbumEmbeddedCheck {
    let mut result = AlbumEmbeddedCheck::default();
    let Ok(entries) = fs::read_dir(path) else {
        return result;
    };
    for track in entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|track| track.is_file() && splined_core::scan::is_supported_audio_file(track))
    {
        result.tracks += 1;
        match cached_embedded_front_metadata(&track) {
            None => result.missing += 1,
            Some((width, height, _)) if width != side || height != side => {
                result.wrong_size += 1;
            }
            _ => {}
        }
    }
    result
}

fn fallback_album_embedded_check_from_cache(
    snapshot: &MediaAlbumSnapshot,
    side: u32,
) -> Option<AlbumEmbeddedCheck> {
    if snapshot.compilation_tracks.is_empty() {
        return None;
    }
    let cache = EMBEDDED_METADATA_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok()?;
    let mut result = AlbumEmbeddedCheck::default();
    for track in &snapshot.compilation_tracks {
        let (_, _, dimensions) = cache.get(&track.path)?;
        result.tracks += 1;
        match dimensions {
            None => result.missing += 1,
            Some((width, height, _)) if *width != side || *height != side => {
                result.wrong_size += 1;
            }
            _ => {}
        }
    }
    Some(result)
}

fn album_embedded_check_cache() -> &'static Mutex<HashMap<String, Option<AlbumEmbeddedCheck>>> {
    ALBUM_EMBEDDED_CHECKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn invalidate_album_embedded_check(path: &Path) {
    let prefix = format!("{}|", path.to_string_lossy().to_ascii_lowercase());
    if let Ok(mut cache) = album_embedded_check_cache().lock() {
        cache.retain(|key, _| !key.starts_with(&prefix));
    }
}

fn replace_fallback_tracks_and_invalidate(
    app: &mut ApplicationState,
    album_key: &str,
    tracks: Vec<MediaTrackSnapshot>,
) {
    let album_path = app
        .library
        .albums
        .iter()
        .find(|album| album.snapshot.album_key.eq_ignore_ascii_case(album_key))
        .map(|album| PathBuf::from(&album.snapshot.path));
    if app.library.replace_fallback_tracks(album_key, tracks)
        && let Some(path) = album_path
    {
        invalidate_album_embedded_check(&path);
    }
}

fn schedule_artist_album_embedded_checks(
    weak: &slint::Weak<AppWindow>,
    state: &Arc<Mutex<ApplicationState>>,
) {
    let targets = {
        let Ok(app) = state.lock() else { return };
        let artist_mode = !app.library.show_tracks
            && app.library.view == LibraryView::Artists
            && app.library.focused_artist.is_some();
        let album_mode =
            app.library.view == LibraryView::Albums && app.library.focused_album_key.is_some();
        if !artist_mode && !album_mode {
            return;
        }
        let side = app.config.output.embedded_thumbnail_size;
        let mut indices = if artist_mode {
            app.library.artist_overview_album_indices(
                &app.artist_overview_album_keys,
                app.artist_overview_show_all,
            )
        } else {
            app.library
                .focused_album_key
                .as_deref()
                .map(|key| {
                    app.library
                        .albums
                        .iter()
                        .enumerate()
                        .filter(|(_, album)| album.snapshot.album_key.eq_ignore_ascii_case(key))
                        .map(|(index, _)| index)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };
        if artist_mode
            && app.library.artist_run_scope_active
            && app.library.focused_album_key.is_none()
        {
            for (index, album) in app.library.albums.iter().enumerate() {
                if album.selected
                    && !album.snapshot.album_mbid.trim().is_empty()
                    && album.state == AlbumState::Processed
                    && !indices.contains(&index)
                {
                    indices.push(index);
                }
            }
        }
        indices
            .into_iter()
            .filter_map(|index| app.library.albums.get(index))
            .map(|album| {
                let path = album.snapshot.path.clone();
                if album.snapshot.album_mbid.trim().is_empty() {
                    let artist = app.library.focused_artist.as_deref().unwrap_or_default();
                    (
                        artist_fallback_embedded_check_key(&path, side, artist),
                        path,
                        Some(artist_fallback_embedded_track_paths(&app, album)),
                        side,
                    )
                } else {
                    (album_embedded_check_key(&path, side), path, None, side)
                }
            })
            .collect::<Vec<_>>()
    };
    let pending = {
        let Ok(mut cache) = album_embedded_check_cache().lock() else {
            return;
        };
        targets
            .into_iter()
            .filter(|(key, _, _, _)| {
                if let std::collections::hash_map::Entry::Vacant(entry) = cache.entry(key.clone()) {
                    entry.insert(None);
                    true
                } else {
                    false
                }
            })
            .collect::<Vec<_>>()
    };
    if pending.is_empty() {
        return;
    }
    let weak = weak.clone();
    let state = Arc::clone(state);
    std::thread::spawn(move || {
        for (key, path, scoped_tracks, side) in pending {
            let check = if let Some(tracks) = scoped_tracks {
                artist_fallback_embedded_check(&tracks, side)
            } else {
                album_embedded_check(Path::new(&path), side)
            };
            if let Some(log) = UI_DEBUG_LOG_HANDLE.get() {
                log.record(format!(
                    "artist-album-embedded-check path={} tracks={} missing={} wrong_size={} side={}",
                    path, check.tracks, check.missing, check.wrong_size, side
                ));
            }
            if let Ok(mut cache) = album_embedded_check_cache().lock() {
                cache.insert(key, Some(check));
            }
        }
        let _ = slint::invoke_from_event_loop(move || refresh_weak(&weak, &state));
    });
}

fn update_artist_bulk_completion(state: &Arc<Mutex<ApplicationState>>) {
    let Ok(mut app) = state.lock() else { return };
    let was_pending = app.artist_bulk_validation_pending;
    app.artist_bulk_complete_keys.clear();
    app.artist_bulk_validation_pending = false;
    if app.library.view != LibraryView::Artists
        || !app.library.artist_run_scope_active
        || app.library.focused_album_key.is_some()
    {
        return;
    }
    let Ok(cache) = album_embedded_check_cache().lock() else {
        app.artist_bulk_validation_pending = true;
        return;
    };
    let side = app.config.output.embedded_thumbnail_size;
    let mut complete = BTreeSet::new();
    let mut pending = false;
    for album in app.library.albums.iter().filter(|album| {
        album.selected
            && !album.snapshot.album_mbid.trim().is_empty()
            && album.state == AlbumState::Processed
            && album.snapshot.has_local_artwork
    }) {
        let key = album_embedded_check_key(&album.snapshot.path, side);
        match cache.get(&key).copied().flatten() {
            Some(check) if check.tracks > 0 && !check.needs_attention() => {
                complete.insert(album.snapshot.album_key.to_ascii_lowercase());
            }
            None => pending = true,
            _ => {}
        }
    }
    app.artist_bulk_complete_keys = complete;
    app.artist_bulk_validation_pending = pending;
    const VALIDATING: &str = "Checking artist albums for cover and embedded-art completion…";
    if !app.artist_overview_loading && app.phase == RunPhase::Idle && !app.operation_busy {
        if pending {
            app.operation_status = VALIDATING.to_string();
            app.operation_tone = "cyan".to_string();
        } else if was_pending && app.operation_status == VALIDATING {
            let remaining = app.launch_targets().len();
            app.operation_status = if remaining == 0 {
                "Artist albums already complete · select an album card to reprocess".to_string()
            } else {
                format!("Artist album selection ready · {remaining} albums need artwork")
            };
            app.operation_tone = if remaining == 0 { "green" } else { "cyan" }.to_string();
        }
    }
}

#[cfg(windows)]
fn browse_for_folder(initial: &str, description: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let script = r#"
$shell = New-Object -ComObject Shell.Application
$initial = if ($env:SPLINED_BROWSE_INITIAL -and (Test-Path -LiteralPath $env:SPLINED_BROWSE_INITIAL -PathType Container)) { $env:SPLINED_BROWSE_INITIAL } else { 0 }
$folder = $shell.BrowseForFolder(0, $env:SPLINED_BROWSE_DESCRIPTION, 81, $initial)
if ($null -ne $folder) {
    [Console]::Out.Write($folder.Self.Path)
}
"#;
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-STA", "-Command", script])
        .env("SPLINED_BROWSE_INITIAL", initial)
        .env("SPLINED_BROWSE_DESCRIPTION", description)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let selected = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!selected.is_empty()).then_some(selected)
}

#[cfg(not(windows))]
fn browse_for_folder(_initial: &str, _description: &str) -> Option<String> {
    None
}

#[cfg(windows)]
fn browse_for_backup_file(initial: &str, settings_only: bool) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let script = r#"
Add-Type -AssemblyName PresentationFramework
$dialog = New-Object Microsoft.Win32.OpenFileDialog
if ($env:SPLINED_SETTINGS_ARCHIVE_ONLY -eq '1') {
    $dialog.Title = 'Choose a SPLINED Settings Archive'
    $dialog.Filter = 'Settings archives (*.spl)|*.spl'
} else {
    $dialog.Title = 'Choose a SPLINED backup to restore'
    $dialog.Filter = 'SPLINED backups (*.spl;*.sqlite)|*.spl;*.sqlite|Settings archives (*.spl)|*.spl|SQLite snapshots (*.sqlite)|*.sqlite|All files (*.*)|*.*'
}
$dialog.CheckFileExists = $true
$dialog.Multiselect = $false
if ($env:SPLINED_RESTORE_INITIAL) {
    if (Test-Path -LiteralPath $env:SPLINED_RESTORE_INITIAL -PathType Leaf) {
        $dialog.InitialDirectory = [System.IO.Path]::GetDirectoryName($env:SPLINED_RESTORE_INITIAL)
        $dialog.FileName = [System.IO.Path]::GetFileName($env:SPLINED_RESTORE_INITIAL)
    } elseif (Test-Path -LiteralPath $env:SPLINED_RESTORE_INITIAL -PathType Container) {
        $dialog.InitialDirectory = $env:SPLINED_RESTORE_INITIAL
    }
}
if ($dialog.ShowDialog()) {
    [Console]::Out.Write($dialog.FileName)
}
"#;
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-STA", "-Command", script])
        .env("SPLINED_RESTORE_INITIAL", initial)
        .env(
            "SPLINED_SETTINGS_ARCHIVE_ONLY",
            if settings_only { "1" } else { "0" },
        )
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let selected = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!selected.is_empty()).then_some(selected)
}

#[cfg(not(windows))]
fn browse_for_backup_file(_initial: &str, _settings_only: bool) -> Option<String> {
    None
}

fn source_display_name(source: &str) -> String {
    match source.trim().to_ascii_lowercase().as_str() {
        "samples" => "Samples".to_string(),
        "existing" => "Existing".to_string(),
        "local" | "webpstill" => "Local Files".to_string(),
        "navidrome" => "Navidrome".to_string(),
        "deezer" => "Deezer".to_string(),
        "itunes" | "apple" => "iTunes".to_string(),
        "fanarttv" | "fanart.tv" => "Fanart.tv".to_string(),
        "lastfm" | "last.fm" => "Last.fm".to_string(),
        "musicbrainz" => "MusicBrainz".to_string(),
        "coverartarchive" | "cover art archive" => "Cover Art Archive".to_string(),
        "discogs" => "Discogs".to_string(),
        "amazon" => "Amazon".to_string(),
        _ => source.trim().to_string(),
    }
}

fn musicbrainz_visible_deduplicated_results(state: &ApplicationState) -> Vec<&MusicBrainzResult> {
    // Keep 90-100% MB matches as alternatives to local artwork in Best Match.
    // In the two lower sections, suppress only an identical release ID;
    // different editions may share a title and must remain visible.
    let existing_release_ids = state
        .musicbrainz_existing_artworks
        .iter()
        .map(|item| item.release_mbid.trim().to_ascii_lowercase())
        .filter(|id| !id.is_empty())
        .collect::<HashSet<_>>();
    let visible = state.visible_musicbrainz_results();
    let mut family_scores = HashMap::<String, u16>::new();
    let mut preferred_families = HashSet::<String>::new();
    for item in &visible {
        let family = musicbrainz_group_key(item);
        let score = family_scores.entry(family.clone()).or_default();
        *score = (*score).max(item.score);
        if musicbrainz_artist_album_preferred(item) {
            preferred_families.insert(family);
        }
    }
    visible
        .into_iter()
        .filter(|item| {
            let family = musicbrainz_group_key(item);
            let best_match = musicbrainz_match_section(
                family_scores.get(&family).copied().unwrap_or_default(),
                preferred_families.contains(&family),
            ) == 0;
            best_match
                || item.release_mbid.trim().is_empty()
                || !existing_release_ids.contains(&item.release_mbid.to_ascii_lowercase())
        })
        .collect()
}

fn musicbrainz_group_label(item: &MusicBrainzResult) -> String {
    let title = if item.release_group_title.trim().is_empty() {
        item.release_title.trim()
    } else {
        item.release_group_title.trim()
    };
    format!("Release Family · {title}")
}

fn musicbrainz_group_key(item: &MusicBrainzResult) -> String {
    if item.release_group_mbid.trim().is_empty() {
        item.release_mbid.to_ascii_lowercase()
    } else {
        item.release_group_mbid.to_ascii_lowercase()
    }
}

fn musicbrainz_ui_rows_equal(
    current: &ModelRc<UiMusicBrainzRow>,
    next: &[UiMusicBrainzRow],
) -> bool {
    current.row_count() == next.len()
        && next.iter().enumerate().all(|(index, expected)| {
            current.row_data(index).is_some_and(|actual| {
                actual.index == expected.index
                    && actual.title == expected.title
                    && actual.artist == expected.artist
                    && actual.detail == expected.detail
                    && actual.group == expected.group
                    && actual.section == expected.section
                    && actual.group_start == expected.group_start
                    && actual.current == expected.current
                    && actual.visited == expected.visited
                    && actual.selected == expected.selected
                    && actual.score == expected.score
            })
        })
}

fn existing_ui_rows_equal(
    current: &ModelRc<UiExistingArtworkRow>,
    next: &[UiExistingArtworkRow],
) -> bool {
    current.row_count() == next.len()
        && next.iter().enumerate().all(|(index, expected)| {
            current.row_data(index).is_some_and(|actual| {
                actual.index == expected.index
                    && actual.title == expected.title
                    && actual.detail == expected.detail
                    && actual.source_path == expected.source_path
                    && actual.loaded == expected.loaded
                    && actual.selected == expected.selected
            })
        })
}

fn source_icon(source: &str) -> &'static str {
    match source.trim().to_ascii_lowercase().as_str() {
        "samples" => "🧪",
        "local" | "webpstill" => "📁",
        "navidrome" => "🧑‍🎨",
        "deezer" => "🎧",
        "itunes" | "apple" => "🍎",
        "fanarttv" | "fanart.tv" => "🎨",
        "lastfm" | "last.fm" => "📻",
        "musicbrainz" => "🧠",
        "coverartarchive" | "cover art archive" => "🖼️",
        "discogs" => "💿",
        "amazon" => "📦",
        _ => "✨",
    }
}

fn model<T: Clone + 'static>(values: Vec<T>) -> ModelRc<T> {
    Rc::new(VecModel::from(values)).into()
}

fn candidate_filter_row(
    state: &ApplicationState,
    dimension: &str,
    key: &str,
    label: &str,
) -> UiCandidateFilterRow {
    UiCandidateFilterRow {
        dimension: SharedString::from(dimension),
        key: SharedString::from(key),
        label: SharedString::from(label),
        count: state.candidate_filter_count(dimension, key) as i32,
        state: SharedString::from(state.candidate_filter_disposition(dimension, key).label()),
    }
}

fn status_filter_color(filter: StatusFilter) -> Color {
    let (red, green, blue) = match filter {
        StatusFilter::All | StatusFilter::Unprocessed => (238, 248, 255),
        StatusFilter::Processed => (239, 165, 78),
        StatusFilter::Bypassed => (255, 92, 102),
        StatusFilter::PartialTimeout => (181, 92, 228),
        StatusFilter::Incomplete => (80, 185, 255),
        StatusFilter::ArtistComplete => (76, 217, 137),
        StatusFilter::ArtistContainsBypass => (64, 153, 255),
    };
    Color::from_rgb_u8(red, green, blue)
}

fn adjustment_row(state: &ApplicationState, name: &str, value: i32, unit: &str) -> UiAdjustmentRow {
    let (minimum, maximum) = adjustment_bounds(name);
    UiAdjustmentRow {
        name: SharedString::from(name),
        value,
        minimum,
        maximum,
        unit: SharedString::from(unit),
        primary: state
            .primary_adjustments
            .iter()
            .any(|value| value.eq_ignore_ascii_case(name)),
    }
}

fn load_image(path: &str) -> slint::Image {
    load_image_with_maximum(path, None)
}

fn load_scaled_image(path: &str, maximum_dimension: u32) -> slint::Image {
    load_image_with_maximum(path, Some(maximum_dimension.max(1)))
}

fn load_cached_preview_image(path: &str, fallback_path: &str) -> slint::Image {
    [PREVIEW_CACHE_DIMENSION, 320, 64]
        .into_iter()
        .find_map(|maximum| cached_scaled_image(path, maximum))
        .or_else(|| {
            [320, 64]
                .into_iter()
                .find_map(|maximum| cached_scaled_image(fallback_path, maximum))
        })
        .unwrap_or_default()
}

fn cached_preview_metadata(path: &str) -> Option<(u32, u32, u64)> {
    let cache_key = format!("{path}\u{1f}{PREVIEW_CACHE_DIMENSION}");
    let cached = raw_image_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(&cache_key).cloned())?;
    if !path.starts_with("memory-preview://") {
        let identity = std::fs::metadata(path)
            .ok()
            .map(|metadata| (metadata.len(), metadata.modified().ok()));
        if identity != Some(cached.identity) {
            if let Ok(mut cache) = raw_image_cache().lock() {
                cache.remove(&cache_key);
            }
            return None;
        }
    }
    Some((cached.source_width, cached.source_height, cached.identity.0))
}

fn cached_scaled_image(path: &str, maximum_dimension: u32) -> Option<slint::Image> {
    if path.trim().is_empty() {
        return None;
    }
    let cache_key = format!("{path}\u{1f}{maximum_dimension}");
    let cached_pixels = raw_image_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(&cache_key).cloned())?;
    // In-memory previews have no backing file. Every file-backed cache hit,
    // however, must still prove that the file has not changed. Otherwise a
    // rewritten cover.* can keep displaying the old pixels indefinitely.
    if !path.starts_with("memory-preview://") {
        let identity = std::fs::metadata(path)
            .ok()
            .map(|metadata| (metadata.len(), metadata.modified().ok()));
        if identity != Some(cached_pixels.identity) {
            IMAGE_STALE_INVALIDATIONS.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut cache) = raw_image_cache().lock() {
                cache.remove(&cache_key);
            }
            IMAGE_CACHE.with(|cache| {
                cache.borrow_mut().remove(&cache_key);
            });
            return None;
        }
    }
    if let Some(image) = IMAGE_CACHE.with(|cache| {
        cache
            .borrow()
            .get(&cache_key)
            .filter(|cached| cached.identity == cached_pixels.identity)
            .map(|cached| cached.image.clone())
    }) {
        IMAGE_MEMORY_HITS.fetch_add(1, Ordering::Relaxed);
        return Some(image);
    }
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        cached_pixels.pixels.as_ref(),
        cached_pixels.width,
        cached_pixels.height,
    );
    let image = slint::Image::from_rgba8(buffer);
    cache_slint_image(
        cache_key,
        maximum_dimension,
        CachedImage {
            identity: cached_pixels.identity,
            image: image.clone(),
        },
    );
    IMAGE_MEMORY_HITS.fetch_add(1, Ordering::Relaxed);
    Some(image)
}

fn cache_slint_image(cache_key: String, maximum_dimension: u32, cached: CachedImage) {
    IMAGE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if maximum_dimension >= 1024 {
            let large_suffix = format!("\u{1f}{PREVIEW_CACHE_DIMENSION}");
            let large_keys = cache
                .keys()
                .filter(|key| key.ends_with(&large_suffix))
                .cloned()
                .collect::<Vec<_>>();
            let remove_count = large_keys.len().saturating_sub(3);
            for key in large_keys.into_iter().take(remove_count) {
                cache.remove(&key);
            }
        } else if cache.len() >= 8192
            && let Some(oldest_key) = cache.keys().next().cloned()
        {
            cache.remove(&oldest_key);
        }
        cache.insert(cache_key, cached);
    });
}

fn load_image_with_maximum(path: &str, maximum_dimension: Option<u32>) -> slint::Image {
    if path.trim().is_empty() {
        return slint::Image::default();
    }
    let cache_key = maximum_dimension
        .map(|maximum| format!("{path}\u{1f}{maximum}"))
        .unwrap_or_else(|| path.to_string());
    if let Some(maximum) = maximum_dimension
        && let Some(image) = cached_scaled_image(path, maximum)
    {
        return image;
    }
    let Ok(metadata) = std::fs::metadata(path) else {
        IMAGE_CACHE.with(|cache| cache.borrow_mut().remove(&cache_key));
        return slint::Image::default();
    };
    let identity = (metadata.len(), metadata.modified().ok());
    if let Some(image) = IMAGE_CACHE.with(|cache| {
        cache
            .borrow()
            .get(&cache_key)
            .filter(|cached| cached.identity == identity)
            .map(|cached| cached.image.clone())
    }) {
        IMAGE_MEMORY_HITS.fetch_add(1, Ordering::Relaxed);
        return image;
    }
    let cached_pixels = maximum_dimension
        .and_then(|_| {
            raw_image_cache().lock().ok().and_then(|cache| {
                cache
                    .get(&cache_key)
                    .filter(|cached| cached.identity == identity)
                    .cloned()
            })
        })
        .or_else(|| decode_scaled_pixels(path, maximum_dimension));
    let Some(cached_pixels) = cached_pixels else {
        return slint::Image::default();
    };
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        cached_pixels.pixels.as_ref(),
        cached_pixels.width,
        cached_pixels.height,
    );
    let image = slint::Image::from_rgba8(buffer);
    cache_slint_image(
        cache_key,
        maximum_dimension.unwrap_or_default(),
        CachedImage {
            identity,
            image: image.clone(),
        },
    );
    image
}

fn decode_scaled_pixels(path: &str, maximum_dimension: Option<u32>) -> Option<CachedPixels> {
    let metadata = std::fs::metadata(path).ok()?;
    let identity = (metadata.len(), metadata.modified().ok());
    let bytes = std::fs::read(path).ok()?;
    let decoded = image::load_from_memory(&bytes).ok()?;
    IMAGE_FILE_DECODES.fetch_add(1, Ordering::Relaxed);
    let source_width = decoded.width();
    let source_height = decoded.height();
    let decoded = if let Some(maximum) = maximum_dimension
        .filter(|maximum| decoded.width() > *maximum || decoded.height() > *maximum)
    {
        decoded.thumbnail(maximum, maximum)
    } else {
        decoded
    };
    let rgba = decoded.into_rgba8();
    let width = rgba.width();
    let height = rgba.height();
    Some(CachedPixels {
        identity,
        pixels: Arc::<[u8]>::from(rgba.into_raw()),
        width,
        height,
        source_width,
        source_height,
    })
}

fn preload_scaled_image_bytes(cache_id: &str, bytes: &[u8], maximum_dimension: u32) -> bool {
    let maximum_dimension = maximum_dimension.max(1);
    let Ok(decoded) = image::load_from_memory(bytes) else {
        return false;
    };
    let source_width = decoded.width();
    let source_height = decoded.height();
    let decoded = if decoded.width() > maximum_dimension || decoded.height() > maximum_dimension {
        decoded.thumbnail(maximum_dimension, maximum_dimension)
    } else {
        decoded
    };
    let rgba = decoded.into_rgba8();
    let pixels = CachedPixels {
        identity: (bytes.len() as u64, None),
        width: rgba.width(),
        height: rgba.height(),
        pixels: Arc::<[u8]>::from(rgba.into_raw()),
        source_width,
        source_height,
    };
    let cache_key = format!("{cache_id}\u{1f}{maximum_dimension}");
    if let Ok(mut cache) = raw_image_cache().lock() {
        insert_raw_image_pixels(&mut cache, cache_key, pixels, maximum_dimension);
        return true;
    }
    false
}

fn preload_scaled_image(path: &str, maximum_dimension: u32) -> bool {
    let maximum_dimension = maximum_dimension.max(1);
    let cache_key = format!("{path}\u{1f}{maximum_dimension}");
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    let identity = (metadata.len(), metadata.modified().ok());
    if raw_image_cache().lock().ok().is_some_and(|cache| {
        cache
            .get(&cache_key)
            .is_some_and(|cached| cached.identity == identity)
    }) {
        IMAGE_PRELOAD_HITS.fetch_add(1, Ordering::Relaxed);
        return true;
    }
    let Some(pixels) = decode_scaled_pixels(path, Some(maximum_dimension)) else {
        return false;
    };
    if let Ok(mut cache) = raw_image_cache().lock() {
        insert_raw_image_pixels(&mut cache, cache_key, pixels, maximum_dimension);
        return true;
    }
    false
}

fn insert_raw_image_pixels(
    cache: &mut HashMap<String, CachedPixels>,
    cache_key: String,
    pixels: CachedPixels,
    maximum_dimension: u32,
) {
    cache.insert(cache_key.clone(), pixels);
    if maximum_dimension >= 1024 {
        let mut protected = active_preview_cache_keys()
            .lock()
            .map(|keys| keys.clone())
            .unwrap_or_default();
        protected.insert(cache_key);
        trim_large_raw_image_cache(cache, &protected, LARGE_RAW_IMAGE_CACHE_LIMIT);
    } else if cache.len() > 8192
        && let Some(oldest_key) = cache.keys().next().cloned()
    {
        cache.remove(&oldest_key);
    }
}

fn trim_large_raw_image_cache(
    cache: &mut HashMap<String, CachedPixels>,
    protected: &HashSet<String>,
    limit: usize,
) {
    let large_suffix = format!("\u{1f}{PREVIEW_CACHE_DIMENSION}");
    let large_count = cache
        .keys()
        .filter(|key| key.ends_with(&large_suffix))
        .count();
    let removable = cache
        .keys()
        .filter(|key| key.ends_with(&large_suffix) && !protected.contains(*key))
        .cloned()
        .collect::<Vec<_>>();
    for key in removable
        .into_iter()
        .take(large_count.saturating_sub(limit))
    {
        cache.remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AlbumEmbeddedCheck, ArtistAlbumEmbedTarget, CachedPixels, EmbedQueueSource,
        EmbedQueueTarget, MaximizeGuard, PREVIEW_CACHE_DIMENSION, ThumbnailAlbumTarget,
        UiCompilationReportRow, UiMusicBrainzRow, WorkspaceLayout, add_embedded_thumbnail_decision,
        album_embedded_check, album_embedded_check_cache, album_embedded_check_key,
        album_overview_queue_target, apply_ui_preferences, artist_album_card_artwork_path,
        artist_album_card_display_title, artist_album_queue_targets, artist_art_candidate_source,
        artist_art_query_sources, artist_art_targets_for_albums,
        artist_compilation_thumb_repair_plan, artist_fallback_embedded_check_key,
        artist_fallback_embedded_track_paths, artist_is_secondary_credit,
        artist_overview_is_fully_complete, artist_overview_legend_counts,
        artist_overview_release_banners, artist_overview_ui_model, automatic_backup_is_due,
        backup_file_in_directory, backup_schedule_interval, begin_musicbrainz_preview_request,
        cached_artist_banner_with_bottom_blur, cached_circular_artist_thumbnail,
        canonical_webp_status, circular_artist_art_png, clear_rendered_preview_for_adjustment,
        compilation_artwork_context, compilation_thumb_repair_plan,
        completed_compilation_track_needs_attention, current_embedded_thumbnail_infos,
        decode_webp_animation, disable_embedded_thumbnail_preview, edit_existing_cover,
        embedded_artwork_footer, embedded_artwork_size_comparison, embedded_thumbnail_owner_key,
        embedded_thumbnail_ready, ensure_webp_album_still, fallback_compilation_cover_suppressed,
        format_existing_embedded_info, group_compilation_report_rows, include_artwork_source,
        is_sqlite_backup_path, is_valid_mbid, load_scaled_image, local_album_art_file,
        load_artist_art_candidates, local_artist_art_candidates, local_artist_art_file,
        local_artist_art_for_albums,
        logical_dimension, match_artist_overview_album, merge_artist_art_candidates, model,
        musicbrainz_existing_preview_path, musicbrainz_failed_url, musicbrainz_ui_rows_equal,
        musicbrainz_visible_deduplicated_results, navidrome_artist_for_lookup,
        normalize_backup_schedule, normalize_status_bar_intensity, normalize_status_bar_position,
        normalize_status_bar_size, overview_artist_name, plan_artist_album_embeds,
        preload_scaled_image, preload_scaled_image_bytes, prepare_embedded_thumbnail_preview,
        preserve_filter_state, preview_compilation_track, preview_image_source_paths,
        preview_musicbrainz_cover, preview_musicbrainz_existing, preview_request_is_current,
        primary_credit_biography_artist, queue_saved_album, raw_image_cache,
        require_active_compilation_write_target, restore_request_from_args, safe_reset_config,
        select_pending_artist_compilation_report_tracks, selected_artist_album_embed_sources,
        selected_artwork_preview_type, selected_artwork_write_label, split_values,
        switch_artist_preview_focus, synchronize_fallback_album, trim_large_raw_image_cache,
        update_artist_bulk_completion, update_artist_preview_activation, webp_animation_metadata,
        write_artist_art_file, write_artist_art_file_with_stem,
    };
    use serde_json::json;

    #[test]
    fn tint_palette_follows_each_image_without_persistent_storage() {
        fn png(rgb: [u8; 3]) -> Vec<u8> {
            let image = image::DynamicImage::ImageRgba8(image::ImageBuffer::from_pixel(
                8, 8, image::Rgba([rgb[0], rgb[1], rgb[2], 255]),
            ));
            let mut output = std::io::Cursor::new(Vec::new());
            image.write_to(&mut output, image::ImageFormat::Png).unwrap();
            output.into_inner()
        }
        assert_eq!(super::image_derived_tint_color(&png([20, 40, 210])).as_deref(), Some("#1428D2"));
        assert_eq!(super::image_derived_tint_color(&png([0, 0, 255])).as_deref(), Some("#0000FF"));
        assert_eq!(super::image_derived_tint_color(&png([210, 30, 40])).as_deref(), Some("#D21E28"));
    }

    #[test]
    fn enhancement_default_intensity_uses_saved_settings() {
        let mut config = Config::default();
        config.output.upscale_picture_percent = 10;
        config.output.upscale_contrast_percent = 15;
        config.output.upscale_hue_degrees = -20;
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(), canonical_root: String::new(),
            local_root: String::new(), albums: Vec::new(),
        };
        let app = ApplicationState::new(config, snapshot).unwrap();
        let filter = super::filter_increment(&app, 0);
        assert_eq!(filter[0], 2);
        assert_eq!(filter[3], 3);
        assert_eq!(filter[8], -4);
        let detailed = super::filter_increment(&app, 13);
        assert_eq!(detailed[0], 2);
        assert_eq!(detailed[3], 0);
        assert_eq!(detailed[8], -4);
    }

    #[test]
    fn advanced_slider_center_resolves_to_saved_default() {
        let mut config = Config::default();
        config.output.upscale_gamma_percent = -6;
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(), canonical_root: String::new(),
            local_root: String::new(), albums: Vec::new(),
        };
        let mut app = ApplicationState::new(config, snapshot).unwrap();
        let position = super::relative_adjustment_position(&app, "Gamma", 0.5);
        super::set_adjustment_position(&mut app, "Gamma", position);
        assert_eq!(app.adjustments.gamma_percent, -6);
        let position = super::relative_adjustment_position(&app, "Gamma", 0.75);
        super::set_adjustment_position(&mut app, "Gamma", position);
        assert_eq!(app.adjustments.gamma_percent, 4);
    }
    use slint::{Color, Model};
    use splined_core::application::{
        ApplicationState, LaunchMode, LibraryView, RunPhase, ScanScope, StatusFilter,
    };
    use splined_core::candidate::StaticFormat;
    use splined_core::config::{Config, Mode};
    use splined_core::embed_queue;
    use splined_core::inspect::inspect_image;
    use splined_core::media_database::{
        MediaAlbumSnapshot, MediaSnapshot, MediaTrackSnapshot, media_snapshot,
    };
    use splined_core::settings::UiPreferences;
    use splined_core::source::navidrome::{NavidromeAlbum, NavidromeArtist};
    use std::collections::{HashMap, HashSet};
    use std::ffi::OsString;
    use std::fs;
    use std::io::Cursor;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    #[test]
    fn album_view_artist_art_search_loads_downloaded_candidates() {
        let directory = tempfile::tempdir().unwrap();
        let portrait = directory.path().join("portrait.png");
        image::ImageBuffer::from_pixel(16, 16, image::Rgba([20u8, 40, 80, 255]))
            .save(&portrait)
            .unwrap();
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut app = ApplicationState::new(Config::default(), snapshot).unwrap();
        app.library.view = LibraryView::Albums;
        assert!(app.library.focused_artist.is_none());
        app.artist_overview_artwork_candidates = vec![(
            "Fanart.tv Artist".to_string(),
            portrait.to_string_lossy().into_owned(),
        )];

        load_artist_art_candidates(&mut app, "3 Doors Down", &[], &[], "").unwrap();

        assert_eq!(app.candidates.len(), 1);
        assert!(app.artist_artwork_mode);
        assert!(app.operation_status.contains("3 Doors Down"));
    }

    #[test]
    fn slider_preview_replaces_pixels_without_clearing_the_visible_image() {
        let directory = tempfile::TempDir::new().unwrap();
        let cover = directory.path().join("cover.png");
        image::RgbaImage::from_pixel(32, 32, image::Rgba([30, 80, 170, 255]))
            .save_with_format(&cover, image::ImageFormat::Png).unwrap();
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(), canonical_root: String::new(),
            local_root: String::new(), albums: Vec::new(),
        };
        let mut config = Config::default();
        config.range.min = 16;
        config.range.ideal = 32;
        config.range.max = 64;
        config.range.ladder = 64;
        config.output.upscale_adaptive_defaults = false;
        let mut app = ApplicationState::new(config, snapshot).unwrap();
        app.current_cover_path = cover.to_string_lossy().into_owned();
        let state = Arc::new(Mutex::new(app));
        edit_existing_cover(&state, false, None).unwrap();
        let first_path = state.lock().unwrap().edit_preview_path.clone();
        let first = super::full_edit_preview_bytes(&first_path).unwrap();
        {
            let mut app = state.lock().unwrap();
            app.adjustments.picture_percent = 20;
            clear_rendered_preview_for_adjustment(&mut app);
            assert_eq!(app.edit_preview_path, first_path);
        }
        edit_existing_cover(&state, false, None).unwrap();
        let second_path = state.lock().unwrap().edit_preview_path.clone();
        let second = super::full_edit_preview_bytes(&second_path).unwrap();
        assert_ne!(image::load_from_memory(&first).unwrap().to_rgba8(),
                   image::load_from_memory(&second).unwrap().to_rgba8());
    }

    #[test]
    fn artist_portrait_and_banner_adjustments_survive_pane_switches() {
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut app = ApplicationState::new(Config::default(), snapshot).unwrap();
        app.artist_artwork_mode = true;
        app.artist_artwork_draft_path = "portrait.png".to_string();
        app.artist_portrait_preview_path = "memory-preview://artist-art/1".to_string();
        app.artist_banner_source_path = "banner.png".to_string();
        app.artist_banner_preview_path = "memory-preview://artist-banner/2".to_string();
        app.adjustments.hue_degrees = 12;
        switch_artist_preview_focus(&mut app, "banner");
        assert_eq!(app.artist_portrait_adjustments.hue_degrees, 12);
        assert_eq!(app.edit_preview_path, "memory-preview://artist-banner/2");
        app.adjustments.darkness_percent = 24;
        switch_artist_preview_focus(&mut app, "portrait");
        assert_eq!(app.adjustments.hue_degrees, 12);
        assert_eq!(app.artist_banner_adjustments.darkness_percent, 24);
        assert_eq!(app.edit_preview_path, "memory-preview://artist-art/1");
    }

    #[test]
    fn artist_radios_select_individual_or_package_without_manual_package_button() {
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut app = ApplicationState::new(Config::default(), snapshot).unwrap();
        app.artist_artwork_mode = true;
        app.artist_artwork_draft_path = "portrait.png".to_string();
        app.artist_portrait_preview_path = "portrait.png".to_string();
        app.artist_banner_source_path = "banner.png".to_string();
        app.artist_banner_preview_path = "banner.png".to_string();
        app.artist_portrait_radio_active = true;
        update_artist_preview_activation(&mut app, "portrait");
        assert!(!app.artist_package_overview_enabled);
        assert_eq!(app.artist_preview_focus, "portrait");
        app.artist_banner_radio_active = true;
        update_artist_preview_activation(&mut app, "banner");
        assert!(app.artist_package_overview_enabled);
        app.artist_theme_art_enabled = true;
        switch_artist_preview_focus(&mut app, "theme");
        app.artist_portrait_radio_active = false;
        update_artist_preview_activation(&mut app, "portrait");
        assert!(!app.artist_package_overview_enabled);
        assert!(!app.artist_theme_art_enabled);
        assert_eq!(app.artist_preview_focus, "banner");
    }

    #[test]
    fn existing_musicbrainz_release_is_hidden_only_below_best_match() {
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut app = ApplicationState::new(Config::default(), snapshot).unwrap();
        app.apply_runtime_event(&json!({
            "event": "musicbrainz_matches",
            "existing_artwork_items": [{
                "track_path": "C:/Music/Gold/song.mp3",
                "artwork_path": "C:/Music/Gold/cover.jpg",
                "kind": "artist-album-cover",
                "title": "Gold",
                "release_mbid": "EXISTING-RELEASE"
            }, {
                "track_path": "C:/Music/Best/song.mp3",
                "artwork_path": "C:/Music/Best/cover.jpg",
                "kind": "artist-album-cover",
                "title": "Best",
                "release_mbid": "BEST-RELEASE"
            }],
            "items": [
                {"index": 0, "release_title": "Gold", "release_mbid": "existing-release", "release_group_mbid": "family", "score": 65},
                {"index": 1, "release_title": "Gold", "release_mbid": "another-edition", "release_group_mbid": "family", "score": 85},
                {"index": 2, "release_title": "Gold", "release_mbid": "", "release_group_mbid": "", "score": 70},
                {"index": 3, "release_title": "Best", "release_mbid": "best-release", "release_group_mbid": "best-family", "score": 99}
            ]
        }));
        let visible = musicbrainz_visible_deduplicated_results(&app);
        assert_eq!(visible.len(), 3);
        assert!(
            visible
                .iter()
                .any(|item| item.release_mbid == "another-edition")
        );
        assert!(visible.iter().any(|item| item.release_mbid.is_empty()));
        assert!(
            visible
                .iter()
                .any(|item| item.release_mbid == "best-release")
        );
    }

    #[test]
    fn webp_frame_count_follows_riff_chunks() {
        let mut bytes = b"RIFF\0\0\0\0WEBP".to_vec();
        bytes.extend_from_slice(b"VP8X");
        bytes.extend_from_slice(&10_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&[0xff, 0x03, 0]);
        bytes.extend_from_slice(&[0xff, 0x03, 0]);
        let mut frame = b"ANMF\x10\0\0\0".to_vec();
        frame.extend_from_slice(&[0; 12]);
        frame.extend_from_slice(&[0xe8, 0x03, 0x00, 0]);
        bytes.extend_from_slice(&frame);
        bytes.extend_from_slice(&frame);
        let metadata = webp_animation_metadata(&bytes).unwrap();
        assert_eq!(metadata.frame_count, 2);
        assert_eq!((metadata.width, metadata.height), (1024, 1024));
        assert_eq!(metadata.duration_ms, 2000);
        assert!(metadata.suits_preview());
        assert!(webp_animation_metadata(b"not a WebP").is_none());
        bytes[28] = 0xff;
        bytes[29] = 0xff;
        bytes[30] = 0x00;
        assert!(!webp_animation_metadata(&bytes).unwrap().suits_preview());
    }

    #[test]
    fn configured_webp_animation_preview_decodes_with_bounded_frames() {
        let Some(path) = std::env::var_os("SPLINED_TEST_WEBP") else {
            return;
        };
        let started = std::time::Instant::now();
        let frames = decode_webp_animation(std::path::Path::new(&path)).unwrap();
        let metadata = webp_animation_metadata(&fs::read(&path).unwrap()).unwrap();
        eprintln!(
            "Decoded {} preview frames in {:.2}s ({} frames, {} ms, {}×{})",
            frames.len(),
            started.elapsed().as_secs_f64(),
            metadata.frame_count,
            metadata.duration_ms,
            metadata.width,
            metadata.height,
        );
        if !metadata.suits_preview() {
            assert!(frames.is_empty());
        }
        assert!(frames.len() <= 24);
        let pixels: u64 = frames
            .iter()
            .map(|frame| u64::from(frame.pixels.width()) * u64::from(frame.pixels.height()))
            .sum();
        assert!(pixels <= 32_000_000);
    }

    #[test]
    fn album_card_title_uses_two_dots_without_changing_short_titles() {
        assert_eq!(artist_album_card_display_title("Classics"), "Classics");
        assert_eq!(
            artist_album_card_display_title("The Grand Tour"),
            "The Grand.."
        );
        assert_eq!(
            artist_album_card_display_title("Billboard Hot 100 Singles of 1994"),
            "Billboard.."
        );
        assert_eq!(
            artist_album_card_display_title("Étoile Étendue Album"),
            "Étoile Éte.."
        );
    }

    #[test]
    fn status_bar_preferences_reject_unknown_values_without_changing_legacy_defaults() {
        assert_eq!(normalize_status_bar_intensity("unexpected"), "Full");
        assert_eq!(normalize_status_bar_position("unexpected"), "Top");
        assert_eq!(normalize_status_bar_size("unexpected"), "Small");
        assert_eq!(normalize_status_bar_intensity("Off"), "Off");
        assert_eq!(normalize_status_bar_position("Middle"), "Middle");
        assert_eq!(normalize_status_bar_size("Large"), "Large");
    }

    #[test]
    fn safe_reset_preserves_library_database_and_credential_locations() {
        let mut current = Config {
            mode: Mode::Write,
            ..Config::default()
        };
        current.library.music_library = r"E:\Music".to_string();
        current.library.ignored_subs = vec!["_junk".to_string()];
        current.scan.scan_library_dir = r"E:\Music".to_string();
        current.scan.cache_dir = r"E:\SPLINED\data".to_string();
        current.scan.temporary_cache_dir = r"E:\SPLINED\temp".to_string();
        current.scan.log_dir = r"E:\SPLINED\logs".to_string();
        current.scan.sqlite_shared = true;
        current.scan.windows_client_lock = true;
        current.credentials.credential_dir = r"E:\SPLINED\credentials".to_string();
        current.output.file_name = "custom-cover".to_string();
        let reset = safe_reset_config(&current);
        assert_eq!(reset.mode, Mode::Read);
        assert_eq!(reset.output, Config::default().output);
        assert_eq!(reset.library, current.library);
        assert_eq!(reset.scan.scan_library_dir, current.scan.scan_library_dir);
        assert_eq!(reset.scan.cache_dir, current.scan.cache_dir);
        assert_eq!(
            reset.scan.temporary_cache_dir,
            current.scan.temporary_cache_dir
        );
        assert_eq!(reset.scan.log_dir, current.scan.log_dir);
        assert!(reset.scan.sqlite_shared);
        assert!(reset.scan.windows_client_lock);
        assert_eq!(reset.credentials, current.credentials);
    }

    #[test]
    fn album_embedded_info_uses_largest_image_and_marks_varying_sizes() {
        let line = format_existing_embedded_info(&[
            Some((600, 600, 90_000)),
            Some((1400, 1400, 210_000)),
            None,
        ]);
        assert!(line.contains("1400 × 1400"));
        assert!(line.starts_with("Embedded Art (now):"));
        assert!(line.contains("Varying Size"));
        assert!(
            !format_existing_embedded_info(&[
                Some((1200, 1200, 90_000)),
                Some((1200, 1200, 90_000)),
            ])
            .contains("Varying Size")
        );
    }

    #[test]
    fn embedded_footer_reports_entire_album_and_color_precedence() {
        let uniform = embedded_artwork_footer(
            &[Some((1200, 1200, 100_000)), Some((1200, 1200, 150_000))],
            1200,
        );
        assert_eq!(uniform.0, "Track(s) (2)");
        assert_eq!(uniform.2, "1200 × 1200");
        assert_eq!(uniform.4, Color::from_rgb_u8(50, 221, 160));
        let varied = embedded_artwork_footer(
            &[Some((1200, 1200, 100_000)), Some((1400, 1400, 150_000))],
            1200,
        );
        assert_eq!(varied.2, "Varying Size");
        assert_eq!(varied.4, Color::from_rgb_u8(239, 165, 78));
        let undersized = embedded_artwork_footer(
            &[Some((1200, 1200, 100_000)), Some((900, 900, 50_000))],
            1200,
        );
        assert_eq!(undersized.4, Color::from_rgb_u8(255, 82, 104));
        assert_eq!(embedded_artwork_footer(&[None], 1200).4, undersized.4);
    }

    #[test]
    fn prepared_thumbnail_size_compares_totals_for_the_same_tracks() {
        let tracks = [
            Some((1500, 1500, 187 * 1024)),
            Some((1500, 1500, 200 * 1024)),
            None,
        ];
        assert_eq!(
            embedded_artwork_size_comparison(true, Some(120 * 1024), &tracks),
            Some(("~360 KB".into(), "387 KB".into()))
        );
        assert_eq!(
            embedded_artwork_size_comparison(false, Some(120), &tracks),
            None
        );
        assert_eq!(embedded_artwork_size_comparison(true, None, &tracks), None);
        assert_eq!(embedded_artwork_size_comparison(true, Some(120), &[]), None);
    }

    #[test]
    fn featured_credit_detects_nonprimary_appearance_on_regular_album() {
        assert!(artist_is_secondary_credit(
            "Mariah Carey & Joe & 98°",
            "98°"
        ));
        assert!(artist_is_secondary_credit("Mariah Carey;Joe;98°", "98°"));
        assert!(!artist_is_secondary_credit("98°", "98°"));
        assert!(!artist_is_secondary_credit("98° & Mariah Carey", "98°"));
        assert!(artist_is_secondary_credit(
            "George Jones & Merle Haggard",
            "Merle Haggard"
        ));
        assert!(!artist_is_secondary_credit(
            "George Jones & Merle Haggard",
            "George Jones"
        ));
        assert!(artist_is_secondary_credit(
            "Merle Haggard with George Jones",
            "George Jones"
        ));
    }

    #[test]
    fn cover_write_keeps_upscaled_preview_without_implicit_album_embedding() {
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut app = ApplicationState::new(Config::default(), snapshot).unwrap();
        app.edit_preview_path = "memory-preview://candidate/1".into();
        app.embedded_thumbnail_preview_path = "memory-preview://embedded-thumbnail/1".into();
        app.embedded_thumbnail_owner_key = embedded_thumbnail_owner_key(&app);
        let mut decision = json!({
            "action": "use", "preserve_source_resolution": true,
            "apply_edit_profile": false,
        });
        add_embedded_thumbnail_decision(&app, &mut decision);
        assert!(decision.get("embed_thumbnails").is_none());
        assert_eq!(decision["preserve_source_resolution"], false);
        assert_eq!(decision["apply_edit_profile"], true);
        assert_eq!(decision["use_upscaled_preview"], true);
    }

    #[test]
    fn comp_write_uses_upscaled_preview_without_requiring_thumb_preview() {
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut app = ApplicationState::new(Config::default(), snapshot).unwrap();
        app.current_compilation_track_path = "selected-comp-track.mp3".into();
        app.edit_preview_path = "memory-preview://candidate/1/2".into();
        let mut decision = json!({
            "action": "use", "preserve_source_resolution": true,
            "apply_edit_profile": false,
        });
        add_embedded_thumbnail_decision(&app, &mut decision);
        assert_eq!(decision["use_upscaled_preview"], true);
        assert_eq!(decision["preserve_source_resolution"], false);
        assert_eq!(decision["apply_edit_profile"], true);
        assert!(decision.get("embed_thumbnails").is_none());

        app.edit_preview_path.clear();
        let mut plain = json!({"action": "use"});
        add_embedded_thumbnail_decision(&app, &mut plain);
        assert!(plain.get("use_upscaled_preview").is_none());

        app.current_compilation_track_path.clear();
        app.edit_preview_path = "memory-preview://candidate/1/2".into();
        let mut artist_album = json!({"action": "use"});
        add_embedded_thumbnail_decision(&app, &mut artist_album);
        assert!(artist_album.get("use_upscaled_preview").is_none());
    }

    #[test]
    fn embedded_album_check_flags_missing_front_art_without_treating_other_files_as_tracks() {
        let directory = tempfile::TempDir::new().unwrap();
        fs::write(directory.path().join("song.mp3"), b"not tagged").unwrap();
        fs::write(directory.path().join("notes.txt"), b"not audio").unwrap();
        let check = album_embedded_check(directory.path(), 1200);
        assert_eq!(check.tracks, 1);
        assert_eq!(check.missing, 1);
        assert_eq!(check.wrong_size, 0);
        assert!(check.needs_attention());
    }

    #[test]
    fn artist_overview_green_outline_requires_cover_and_preferred_embedded_sizes() {
        let good = Some(AlbumEmbeddedCheck {
            tracks: 2,
            missing: 0,
            wrong_size: 0,
        });
        let wrong = Some(AlbumEmbeddedCheck {
            tracks: 2,
            missing: 0,
            wrong_size: 1,
        });
        assert!(artist_overview_is_fully_complete(
            false, false, true, true, good
        ));
        assert!(!artist_overview_is_fully_complete(
            false, false, true, false, good
        ));
        assert!(!artist_overview_is_fully_complete(
            false, false, true, true, wrong
        ));
        assert!(!artist_overview_is_fully_complete(
            false, false, true, true, None
        ));
        assert!(artist_overview_is_fully_complete(
            true, true, false, false, good
        ));
        assert!(!artist_overview_is_fully_complete(
            true, false, true, false, good
        ));
        assert!(!artist_overview_is_fully_complete(
            true, true, true, false, wrong
        ));
        assert_ne!(
            artist_fallback_embedded_check_key("C:/comp", 1200, "98°"),
            artist_fallback_embedded_check_key("C:/comp", 1200, "Mariah Carey"),
        );
    }

    #[test]
    fn artist_bulk_launch_waits_for_validation_and_skips_only_green_albums() {
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().to_string_lossy().into_owned();
        let album = MediaAlbumSnapshot {
            album_key: "album-one".into(),
            artist: "Alpha".into(),
            tagged_artist: "Alpha".into(),
            title: "Album One".into(),
            path: path.clone(),
            representative_file: String::new(),
            status: "processed".into(),
            compilation: false,
            compilation_track_art_eligible: false,
            compilation_tracks: Vec::new(),
            track_count: 1,
            compilation_completed_tracks: 0,
            release_year: String::new(),
            artist_mbid: String::new(),
            album_mbid: "release-one".into(),
            release_group_mbid: String::new(),
            has_local_artwork: true,
            local_artwork_files: Vec::new(),
            cover_path: String::new(),
            cover_name: String::new(),
            cover_format: String::new(),
            cover_width: 0,
            cover_height: 0,
            root_files: 0,
            cover_files: 0,
            processed_at: None,
            timeout_until: None,
            selected_source: None,
        };
        let mut app = ApplicationState::new(
            Config::default(),
            MediaSnapshot {
                schema_version: splined_core::media_database::SCHEMA_VERSION,
                database_path: String::new(),
                canonical_root: String::new(),
                local_root: String::new(),
                albums: vec![album],
            },
        )
        .unwrap();
        app.library.set_view(LibraryView::Artists);
        app.library.select_artist_exclusive("Alpha", false);
        let key = album_embedded_check_key(&path, app.config.output.embedded_thumbnail_size);
        let shared = Arc::new(Mutex::new(app));
        album_embedded_check_cache()
            .lock()
            .unwrap()
            .insert(key.clone(), None);
        update_artist_bulk_completion(&shared);
        assert!(shared.lock().unwrap().artist_bulk_validation_pending);
        assert!(shared.lock().unwrap().launch_targets().is_empty());

        album_embedded_check_cache().lock().unwrap().insert(
            key.clone(),
            Some(AlbumEmbeddedCheck {
                tracks: 1,
                missing: 0,
                wrong_size: 0,
            }),
        );
        update_artist_bulk_completion(&shared);
        assert!(!shared.lock().unwrap().artist_bulk_validation_pending);
        assert!(
            shared
                .lock()
                .unwrap()
                .artist_bulk_complete_keys
                .contains("album-one")
        );
        assert!(shared.lock().unwrap().launch_targets().is_empty());

        album_embedded_check_cache().lock().unwrap().insert(
            key.clone(),
            Some(AlbumEmbeddedCheck {
                tracks: 1,
                missing: 1,
                wrong_size: 0,
            }),
        );
        update_artist_bulk_completion(&shared);
        assert!(
            !shared
                .lock()
                .unwrap()
                .artist_bulk_complete_keys
                .contains("album-one")
        );
        assert_eq!(shared.lock().unwrap().launch_targets().len(), 1);
        album_embedded_check_cache().lock().unwrap().remove(&key);
    }

    #[test]
    fn completed_comp_track_turns_purple_only_for_known_embedded_art_issues() {
        assert!(!completed_compilation_track_needs_attention(None, 1200));
        assert!(!completed_compilation_track_needs_attention(
            Some(Some((1200, 1200, 90_000))),
            1200
        ));
        assert!(completed_compilation_track_needs_attention(
            Some(None),
            1200
        ));
        assert!(completed_compilation_track_needs_attention(
            Some(Some((900, 900, 90_000))),
            1200
        ));
    }

    #[test]
    fn comp_thumb_repair_targets_only_green_tracks_for_the_focused_artist() {
        let album = MediaAlbumSnapshot {
            album_key: "comp".into(),
            artist: "Various Artists".into(),
            tagged_artist: "Various Artists".into(),
            title: "COMP".into(),
            path: "C:/Music/COMP".into(),
            representative_file: String::new(),
            status: "processed".into(),
            compilation: true,
            compilation_track_art_eligible: true,
            compilation_tracks: vec![
                MediaTrackSnapshot {
                    path: "C:/Music/COMP/one.mp3".into(),
                    title: "One".into(),
                    artist: "98°".into(),
                    embedded_artwork_recorded: true,
                },
                MediaTrackSnapshot {
                    path: "C:/Music/COMP/two.mp3".into(),
                    title: "Two".into(),
                    artist: "98°".into(),
                    embedded_artwork_recorded: false,
                },
                MediaTrackSnapshot {
                    path: "C:/Music/COMP/three.mp3".into(),
                    title: "Three".into(),
                    artist: "Other".into(),
                    embedded_artwork_recorded: true,
                },
            ],
            track_count: 3,
            compilation_completed_tracks: 2,
            release_year: String::new(),
            artist_mbid: String::new(),
            album_mbid: String::new(),
            release_group_mbid: String::new(),
            has_local_artwork: false,
            local_artwork_files: Vec::new(),
            cover_path: String::new(),
            cover_name: String::new(),
            cover_format: String::new(),
            cover_width: 0,
            cover_height: 0,
            root_files: 0,
            cover_files: 0,
            processed_at: None,
            timeout_until: None,
            selected_source: None,
        };
        let mut second_album = album.clone();
        second_album.album_key = "second-comp".into();
        second_album.title = "Second COMP".into();
        second_album.path = "C:/Music/Second COMP".into();
        second_album.compilation_tracks = vec![MediaTrackSnapshot {
            path: "C:/Music/Second COMP/four.mp3".into(),
            title: "Four".into(),
            artist: "98°".into(),
            embedded_artwork_recorded: true,
        }];
        let mut app = ApplicationState::new(
            Config::default(),
            MediaSnapshot {
                schema_version: splined_core::media_database::SCHEMA_VERSION,
                database_path: String::new(),
                canonical_root: String::new(),
                local_root: String::new(),
                albums: vec![album, second_album],
            },
        )
        .unwrap();
        app.library.view = LibraryView::Artists;
        app.library.focused_artist = Some("98°".into());
        app.artist_overview_track_paths
            .insert("comp".into(), vec!["C:/Music/COMP/one.mp3".into()]);
        assert_eq!(
            app.artist_fallback_track_progress(&app.library.albums[0]),
            Some((1, 2))
        );
        let scoped = artist_fallback_embedded_track_paths(&app, &app.library.albums[0]);
        assert_eq!(scoped.len(), 2);
        assert!(scoped.contains(&PathBuf::from("C:/Music/COMP/two.mp3")));
        let plan = compilation_thumb_repair_plan(&app, "comp").unwrap();
        assert_eq!(plan.targets, vec![PathBuf::from("C:/Music/COMP/one.mp3")]);
        let (all_plan, report) = artist_compilation_thumb_repair_plan(&app).unwrap();
        assert!(all_plan.artist_wide);
        assert_eq!(all_plan.album_paths.len(), 2);
        assert_eq!(all_plan.targets.len(), 2);
        assert_eq!(report.len(), 3);
        assert_eq!(report.iter().filter(|track| !track.completed).count(), 1);
        app.compilation_report_artist = "98°".into();
        app.library.albums[1].snapshot.compilation_tracks[0].embedded_artwork_recorded = false;
        assert_eq!(
            select_pending_artist_compilation_report_tracks(&mut app, false),
            Ok(3)
        );
        let queued = app.library.queue(ScanScope::Selected);
        assert_eq!(queued.len(), 3);
        assert!(
            queued
                .iter()
                .all(|target| target.compilation_track_path.is_some())
        );
        assert!(queued.iter().any(|target| target.album_key == "comp"));
        assert!(
            queued
                .iter()
                .any(|target| target.album_key == "second-comp")
        );
    }

    #[test]
    fn compilation_report_groups_duplicate_albums_and_collapses_independently() {
        let row = |album: &str, title: &str, file: &str, status: &str| UiCompilationReportRow {
            album_key: album.into(),
            album_title: title.into(),
            path: format!("C:/Music/{album}/{file}").into(),
            file_name: file.into(),
            status: status.into(),
            album_header: false,
            expanded: false,
        };
        let tracks = vec![
            row("first", "First Album", "one.mp3", "Green"),
            row("first", "First Album", "two.mp3", "Purple"),
            row("second", "Second Album", "three.mp3", "Blue"),
        ];
        let open = group_compilation_report_rows(&tracks, &HashSet::new());
        assert_eq!(open.len(), 5);
        assert!(open[0].album_header && open[0].expanded);
        assert_eq!(
            open[0].file_name.as_str(),
            "2 tracks · 1 green, 1 thumb check"
        );
        assert!(!open[1].album_header && !open[2].album_header);
        assert_eq!(open[3].album_title.as_str(), "Second Album");

        let collapsed = group_compilation_report_rows(&tracks, &HashSet::from(["first".into()]));
        assert_eq!(collapsed.len(), 3);
        assert!(collapsed[0].album_header && !collapsed[0].expanded);
        assert_eq!(collapsed[1].album_title.as_str(), "Second Album");
        assert_eq!(collapsed[2].file_name.as_str(), "three.mp3");
    }

    #[test]
    fn release_id_soundtrack_and_various_artists_get_distinct_overview_banners() {
        let mut album = MediaAlbumSnapshot {
            album_key: "soundtrack".into(),
            artist: "Various Artists".into(),
            tagged_artist: "Various Artists".into(),
            title: "Notting Hill Music From the Motion Picture".into(),
            path: r"N:\media\music\[Soundtracks]\Notting Hill Music From the Motion Picture".into(),
            representative_file: String::new(),
            status: "processed".into(),
            compilation: true,
            compilation_track_art_eligible: false,
            compilation_tracks: Vec::new(),
            track_count: 1,
            compilation_completed_tracks: 0,
            release_year: "1999".into(),
            artist_mbid: String::new(),
            album_mbid: "release-id".into(),
            release_group_mbid: String::new(),
            has_local_artwork: true,
            local_artwork_files: Vec::new(),
            cover_path: String::new(),
            cover_name: String::new(),
            cover_format: String::new(),
            cover_width: 0,
            cover_height: 0,
            root_files: 0,
            cover_files: 0,
            processed_at: None,
            timeout_until: None,
            selected_source: None,
        };
        assert_eq!(artist_overview_release_banners(&album), (true, false));
        album.path = r"N:\media\music\[Various Artists]\Greatest Hits".into();
        album.title = "Greatest Hits".into();
        assert_eq!(artist_overview_release_banners(&album), (false, true));
        album.album_mbid.clear();
        assert_eq!(artist_overview_release_banners(&album), (false, false));
    }

    #[test]
    fn artist_overview_cards_group_by_album_type_then_newest_year_with_separators() {
        let base = MediaAlbumSnapshot {
            album_key: String::new(),
            artist: "Mariah Carey".into(),
            tagged_artist: "Mariah Carey".into(),
            title: String::new(),
            path: String::new(),
            representative_file: String::new(),
            status: "processed".into(),
            compilation: false,
            compilation_track_art_eligible: false,
            compilation_tracks: Vec::new(),
            track_count: 1,
            compilation_completed_tracks: 0,
            release_year: String::new(),
            artist_mbid: String::new(),
            album_mbid: "release-id".into(),
            release_group_mbid: String::new(),
            has_local_artwork: true,
            local_artwork_files: Vec::new(),
            cover_path: String::new(),
            cover_name: String::new(),
            cover_format: String::new(),
            cover_width: 0,
            cover_height: 0,
            root_files: 0,
            cover_files: 0,
            processed_at: None,
            timeout_until: None,
            selected_source: None,
        };
        let specs = [
            (
                "va-old",
                "VA Old",
                "1998",
                "processed",
                "[Various Artists]",
                true,
            ),
            (
                "artist-old",
                "Artist Old",
                "1999",
                "processed",
                "Mariah Carey",
                false,
            ),
            (
                "comp-new",
                "COMP New",
                "2023",
                "processed",
                "[Various Artists]",
                true,
            ),
            (
                "ost-new",
                "OST New",
                "2010",
                "processed",
                "[Soundtracks]",
                true,
            ),
            (
                "new-old",
                "New Old",
                "2020",
                "unprocessed",
                "Mariah Carey",
                false,
            ),
            (
                "va-new",
                "VA New",
                "2018",
                "processed",
                "[Various Artists]",
                true,
            ),
            (
                "feat-old",
                "FEAT Old",
                "2001",
                "processed",
                "[Various Artists]",
                true,
            ),
            (
                "comp-old",
                "COMP Old",
                "2013",
                "unprocessed",
                "[Various Artists]",
                true,
            ),
            (
                "artist-new",
                "Artist New",
                "2025",
                "processed",
                "Mariah Carey",
                false,
            ),
            (
                "feat-new",
                "FEAT New",
                "2022",
                "processed",
                "Mariah Carey",
                false,
            ),
            (
                "ost-old",
                "OST Old",
                "2000",
                "processed",
                "[Soundtracks]",
                true,
            ),
            (
                "new-new",
                "New New",
                "2024",
                "unprocessed",
                "Mariah Carey",
                false,
            ),
        ];
        let albums = specs
            .into_iter()
            .map(|(key, title, year, status, folder, appearance)| {
                let mut album = base.clone();
                album.album_key = key.into();
                album.title = title.into();
                album.release_year = year.into();
                album.status = status.into();
                album.path = format!(r"N:\media\music\{folder}\{title}");
                if appearance {
                    album.artist = "Various Artists".into();
                    album.tagged_artist = album.artist.clone();
                }
                if key.starts_with("comp-") {
                    album.album_mbid.clear();
                    album.compilation = true;
                }
                album
            })
            .collect();
        let mut app = ApplicationState::new(
            Config::default(),
            MediaSnapshot {
                schema_version: splined_core::media_database::SCHEMA_VERSION,
                database_path: String::new(),
                canonical_root: String::new(),
                local_root: String::new(),
                albums,
            },
        )
        .unwrap();
        app.library.focused_artist = Some("Mariah Carey".into());
        app.artist_overview_show_all = true;
        app.artist_overview_album_keys = [
            "va-old", "va-new", "comp-new", "comp-old", "ost-new", "ost-old", "feat-new",
            "feat-old",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        app.artist_overview_featured_album_keys = ["feat-new", "feat-old"]
            .into_iter()
            .map(str::to_string)
            .collect();

        let cards = artist_overview_ui_model(&app);
        let actual = (0..cards.row_count())
            .map(|index| {
                let card = cards.row_data(index).unwrap();
                (card.key.to_string(), card.separator_before)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            [
                ("new-new", false),
                ("new-old", false),
                ("artist-new", true),
                ("artist-old", false),
                ("ost-new", true),
                ("ost-old", false),
                ("va-new", true),
                ("va-old", false),
                ("feat-new", true),
                ("feat-old", false),
                ("comp-new", true),
                ("comp-old", false),
            ]
            .into_iter()
            .map(|(key, separator)| (key.to_string(), separator))
            .collect::<Vec<_>>()
        );
        let counts = artist_overview_legend_counts(&cards, &[]);
        assert_eq!(counts.shown, 12);
        assert_eq!(counts.completed, 0);
        assert_eq!(counts.in_process, 4);
        assert_eq!(counts.validating, 8);
        assert_eq!(counts.feat, 2);
        assert_eq!(counts.various, 2);
        assert_eq!(counts.ost, 2);
        assert_eq!(counts.artist, 4);
        for album in &mut app.library.albums {
            album.snapshot.cover_path = format!("{}\\cover.jpg", album.snapshot.path);
        }
        let targets = artist_album_queue_targets(&app).unwrap();
        assert!(
            targets
                .iter()
                .any(|target| target.label.contains("FEAT New"))
        );
        assert!(
            !targets
                .iter()
                .any(|target| target.label.contains("COMP New"))
        );
        app.library.view = LibraryView::Albums;
        app.library.focused_album_key = Some("feat-new".into());
        assert_eq!(overview_artist_name(&app), Some("Mariah Carey"));
        let album_only = artist_overview_ui_model(&app);
        assert_eq!(album_only.row_count(), 1);
        assert_eq!(album_only.row_data(0).unwrap().key, "feat-new");
        assert!(
            album_overview_queue_target(&app)
                .unwrap()
                .label
                .contains("FEAT New")
        );
        app.library.show_tracks = true;
        app.library.focused_album_key = Some("comp-new".into());
        let fallback_only = artist_overview_ui_model(&app);
        assert_eq!(fallback_only.row_count(), 1);
        assert_eq!(fallback_only.row_data(0).unwrap().key, "comp-new");
        app.library.view = LibraryView::Artists;
        app.library.show_tracks = false;
        app.library.focused_album_key = None;
        assert_eq!(counts.comp_pending + counts.comp_complete, 2);

        // A processed artist's three physical albums and one completed COMP
        // appearance must not acquire red dots merely because the COMP folder
        // has no Release ID/usable folder cover or remains globally incomplete.
        app.library.albums.retain(|album| {
            ["artist-new", "artist-old", "new-new", "comp-new"]
                .contains(&album.snapshot.album_key.as_str())
        });
        app.library.artist_artwork_paths.insert(
            "mariah carey".into(),
            "C:/Music/Mariah Carey/artist.jpg".into(),
        );
        for album in &mut app.library.albums {
            if album.snapshot.album_key == "comp-new" {
                album.state = splined_core::application::AlbumState::Incomplete;
                album.snapshot.status = "incomplete".into();
                album.snapshot.has_local_artwork = false;
                album.snapshot.track_count = 1;
                album.snapshot.compilation_tracks = vec![MediaTrackSnapshot {
                    path: "comp-track.mp3".into(),
                    title: "Because the Night".into(),
                    artist: "Mariah Carey".into(),
                    embedded_artwork_recorded: true,
                }];
            } else {
                album.state = splined_core::application::AlbumState::Processed;
                album.snapshot.status = "processed".into();
            }
        }
        app.artist_overview_track_paths
            .insert("comp-new".into(), vec!["comp-track.mp3".into()]);
        let side = app.config.output.embedded_thumbnail_size;
        for album in &app.library.albums {
            let key = if album.snapshot.album_mbid.is_empty() {
                artist_fallback_embedded_check_key(&album.snapshot.path, side, "Mariah Carey")
            } else {
                album_embedded_check_key(&album.snapshot.path, side)
            };
            album_embedded_check_cache().lock().unwrap().remove(&key);
        }
        let cards = artist_overview_ui_model(&app);
        let counts = artist_overview_legend_counts(&cards, &[]);
        assert_eq!(counts.shown, 4);
        assert_eq!(counts.validating, 4);
        assert_eq!(counts.in_process, 0);
        assert_eq!(counts.orange_dots, 0);
        assert_eq!(counts.purple_dots, 0);
        assert!((0..cards.row_count()).all(|index| {
            cards
                .row_data(index)
                .is_some_and(|card| card.validating && !card.check_embedded)
        }));
        for album in &app.library.albums {
            let key = if album.snapshot.album_mbid.is_empty() {
                artist_fallback_embedded_check_key(&album.snapshot.path, side, "Mariah Carey")
            } else {
                album_embedded_check_key(&album.snapshot.path, side)
            };
            album_embedded_check_cache().lock().unwrap().insert(
                key,
                Some(AlbumEmbeddedCheck {
                    tracks: 1,
                    missing: 0,
                    wrong_size: 0,
                }),
            );
        }
        let cards = artist_overview_ui_model(&app);
        let counts = artist_overview_legend_counts(&cards, &[]);
        assert_eq!(counts.shown, 4);
        assert_eq!(counts.completed, 4);
        assert_eq!(counts.green_frames, 4);
        assert_eq!(counts.green_dots, 4);
        assert_eq!(counts.red_dots, 0);
        assert_eq!(counts.orange_dots, 0);
        assert_eq!(counts.purple_dots, 0);
        assert_eq!(counts.comp_complete, 1);
    }

    #[test]
    fn selected_artist_album_embed_plan_counts_only_readable_artist_album_targets() {
        let directory = tempfile::TempDir::new().unwrap();
        let valid = directory.path().join("Artist").join("Album");
        fs::create_dir_all(&valid).unwrap();
        let cover = valid.join("cover.png");
        image::RgbImage::from_pixel(700, 700, image::Rgb([30, 70, 130]))
            .save_with_format(&cover, image::ImageFormat::Png)
            .unwrap();
        fs::write(valid.join("song.mp3"), b"audio test fixture").unwrap();
        fs::write(valid.join("notes.txt"), b"ignored").unwrap();
        let plan = plan_artist_album_embeds(
            "Artist".to_string(),
            4,
            1200,
            vec![
                ArtistAlbumEmbedTarget {
                    title: "Album".to_string(),
                    album_path: valid.clone(),
                    cover_path: cover.clone(),
                    track_count: 0,
                },
                ArtistAlbumEmbedTarget {
                    title: "Missing".to_string(),
                    album_path: directory.path().join("Missing"),
                    cover_path: directory.path().join("Missing").join("cover.jpg"),
                    track_count: 0,
                },
            ],
            1,
        )
        .unwrap();
        assert_eq!(plan.artist, "Artist");
        assert_eq!(plan.selection_epoch, 4);
        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.targets[0].track_count, 1);
        assert_eq!(plan.skipped, 2);
        assert!(!fs::read(&cover).unwrap().is_empty());
    }

    #[test]
    fn artist_album_embed_scope_uses_selected_albums_and_excludes_fallback_without_release_id() {
        let artist_album = MediaAlbumSnapshot {
            album_key: "artist-album".to_string(),
            artist: "Artist".to_string(),
            tagged_artist: "Artist".to_string(),
            title: "Album".to_string(),
            path: r"C:\Music\Artist\Album".to_string(),
            representative_file: String::new(),
            status: "processed".to_string(),
            compilation: false,
            compilation_track_art_eligible: false,
            compilation_tracks: Vec::new(),
            track_count: 3,
            compilation_completed_tracks: 0,
            release_year: String::new(),
            artist_mbid: String::new(),
            album_mbid: "release-id".to_string(),
            release_group_mbid: String::new(),
            has_local_artwork: true,
            local_artwork_files: Vec::new(),
            cover_path: r"C:\Music\Artist\Album\cover.jpg".to_string(),
            cover_name: "cover.jpg".to_string(),
            cover_format: "JPEG".to_string(),
            cover_width: 1200,
            cover_height: 1200,
            root_files: 0,
            cover_files: 1,
            processed_at: None,
            timeout_until: None,
            selected_source: None,
        };
        let mut fallback = artist_album.clone();
        fallback.album_key = "fallback".to_string();
        fallback.title = "COMP fallback".to_string();
        fallback.album_mbid.clear();
        fallback.compilation = true;
        let mut unselected = artist_album.clone();
        unselected.album_key = "other".to_string();
        unselected.title = "Other Artist Album".to_string();
        let mut app = ApplicationState::new(
            Config::default(),
            MediaSnapshot {
                schema_version: splined_core::media_database::SCHEMA_VERSION,
                database_path: String::new(),
                canonical_root: String::new(),
                local_root: String::new(),
                albums: vec![artist_album, fallback, unselected],
            },
        )
        .unwrap();
        app.library.focused_artist = Some("Artist".to_string());
        app.library.albums[0].selected = true;
        app.library.albums[1].selected = true;
        let (_, _, _, targets, skipped) = selected_artist_album_embed_sources(&app).unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].title, "Album");
        assert_eq!(skipped, 1);
        app.artist_overview_show_all = true;
        album_embedded_check_cache().lock().unwrap().insert(
            album_embedded_check_key(&app.library.albums[0].snapshot.path, 1200),
            Some(AlbumEmbeddedCheck {
                tracks: 3,
                missing: 1,
                wrong_size: 1,
            }),
        );
        let cards = artist_overview_ui_model(&app);
        let card = (0..cards.row_count())
            .filter_map(|index| cards.row_data(index))
            .find(|card| card.key == "artist-album")
            .unwrap();
        assert!(card.processed);
        assert!(card.check_embedded);
        assert!((0..card.bullets.row_count()).any(|index| {
            card.bullets
                .row_data(index)
                .is_some_and(|bullet| bullet.tip == "Processed Album")
        }));
        assert!((0..card.bullets.row_count()).any(|index| {
            card.bullets
                .row_data(index)
                .is_some_and(|bullet| bullet.tip.contains("Check Embedded"))
        }));
    }

    #[test]
    fn artist_art_queries_only_artist_image_providers() {
        let enabled = [
            "local",
            "navidrome",
            "fanarttv",
            "lastfm",
            "deezer",
            "discogs",
            "itunes",
            "amazon",
            "musicbrainz",
            "coverartarchive",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
        assert_eq!(
            artist_art_query_sources(&enabled, "valid-artist-id"),
            ["fanarttv", "lastfm", "deezer", "discogs", "itunes"]
        );
        assert_eq!(
            artist_art_query_sources(&enabled, ""),
            ["lastfm", "deezer", "discogs", "itunes"]
        );
        assert_eq!(artist_art_candidate_source("Deezer Artist Image"), "deezer");
        assert_eq!(
            artist_art_candidate_source("Discogs Artist Image"),
            "discogs"
        );
        assert_eq!(artist_art_candidate_source("iTunes Artist Image"), "itunes");
    }

    #[test]
    fn dotted_artist_lookup_requires_a_unique_navidrome_alias() {
        let dotted = NavidromeArtist {
            id: "dotted".into(),
            name: ".38 Special".into(),
            ..NavidromeArtist::default()
        };
        assert_eq!(
            navidrome_artist_for_lookup(&[dotted.clone()], "38 Special", None)
                .map(|artist| artist.id.as_str()),
            Some("dotted")
        );
        let exact = NavidromeArtist {
            id: "exact".into(),
            name: "38 Special".into(),
            ..NavidromeArtist::default()
        };
        assert_eq!(
            navidrome_artist_for_lookup(&[dotted.clone(), exact], "38 Special", None)
                .map(|artist| artist.id.as_str()),
            Some("exact")
        );
        let ambiguous = NavidromeArtist {
            id: "ambiguous".into(),
            name: "..38 Special".into(),
            ..NavidromeArtist::default()
        };
        assert!(navidrome_artist_for_lookup(&[dotted, ambiguous], "38 Special", None).is_none());
    }

    #[test]
    fn collaboration_biography_falls_back_only_to_the_first_credited_mbid() {
        let duo_id = "8063db1a-2126-45c2-9fae-b50c628ed102";
        let tina_id = "54d991d8-045a-4078-9fc5-4f6d150ec4a9";
        let combined = "George Jones & Tammy Wynette & Tina Byrd";
        let album = ThumbnailAlbumTarget {
            album_key: "collaboration".to_string(),
            artist: combined.to_string(),
            artist_mbid: format!("{duo_id}\\{tina_id}"),
            title: "George & Tammy & Tina".to_string(),
            path: String::new(),
            album_mbid: String::new(),
            release_year: String::new(),
            cover_path: String::new(),
        };
        let navidrome_artists = [
            NavidromeArtist {
                id: "nav-tina".to_string(),
                name: "Tina Byrd".to_string(),
                musicbrainz_id: tina_id.to_string(),
                ..NavidromeArtist::default()
            },
            NavidromeArtist {
                id: "nav-duo".to_string(),
                name: "George Jones & Tammy Wynette".to_string(),
                musicbrainz_id: duo_id.to_string(),
                ..NavidromeArtist::default()
            },
        ];
        assert_eq!(
            primary_credit_biography_artist(
                std::slice::from_ref(&album),
                combined,
                &navidrome_artists
            )
            .map(|artist| artist.id.as_str()),
            Some("nav-duo")
        );
        let mut single = album.clone();
        single.artist_mbid = duo_id.to_string();
        assert!(primary_credit_biography_artist(&[single], combined, &navidrome_artists).is_none());
        let mut conflict = album.clone();
        conflict.artist_mbid = format!("{tina_id};{duo_id}");
        assert!(
            primary_credit_biography_artist(
                &[album.clone(), conflict],
                combined,
                &navidrome_artists
            )
            .is_none()
        );
        assert!(
            primary_credit_biography_artist(&[album], "Other Artist", &navidrome_artists).is_none()
        );
    }

    #[test]
    fn artist_art_search_sees_a_file_created_after_overview_load() {
        let directory = tempfile::TempDir::new().unwrap();
        let artist = directory.path().join("New Artist");
        let album = artist.join("New Album");
        fs::create_dir_all(&album).unwrap();
        let albums = [("New Artist", album.to_str().unwrap(), "album-id", "")];
        let cache_dir = directory.path().join("db");
        assert!(local_artist_art_candidates(albums, "New Artist", &cache_dir).is_empty());

        let image = artist.join("artist.png");
        image::RgbImage::from_pixel(24, 24, image::Rgb([20, 50, 80]))
            .save_with_format(&image, image::ImageFormat::Png)
            .unwrap();
        let local = local_artist_art_candidates(albums, "New Artist", &cache_dir);
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].1, image.to_string_lossy());

        let merged = merge_artist_art_candidates(
            local,
            vec![(
                "Navidrome current".to_string(),
                "cached-image.png".to_string(),
            )],
            vec![(
                "Local artist file".to_string(),
                image.to_string_lossy().into_owned(),
            )],
        );
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].1, image.to_string_lossy());
    }

    #[test]
    fn writing_artist_art_produces_a_round_transparent_png() {
        let source = image::RgbaImage::from_pixel(640, 480, image::Rgba([80, 120, 200, 255]));
        let mut input = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(source)
            .write_to(&mut input, image::ImageFormat::Png)
            .unwrap();
        let written = circular_artist_art_png(input.get_ref()).unwrap();
        let decoded = image::load_from_memory(&written).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), (1024, 1024));
        assert_eq!(decoded.get_pixel(0, 0).0[3], 0);
        assert_eq!(decoded.get_pixel(512, 512).0[3], 255);
        assert_eq!(
            image::guess_format(&written).unwrap(),
            image::ImageFormat::Png
        );
    }

    #[test]
    fn artist_art_write_obeys_preserve_and_overwrite_policy() {
        let directory = tempfile::TempDir::new().unwrap();
        let artist = directory.path();
        let source = artist.join("source.png");
        let old = artist.join("artist.jpg");
        image::RgbImage::from_pixel(24, 24, image::Rgb([25, 50, 75]))
            .save_with_format(&old, image::ImageFormat::Jpeg)
            .unwrap();
        image::RgbaImage::from_pixel(24, 24, image::Rgba([210, 40, 90, 255]))
            .save_with_format(&source, image::ImageFormat::Png)
            .unwrap();
        let input = fs::read(&source).unwrap();

        let (target, unchanged, warnings) = write_artist_art_file(artist, &input, false).unwrap();
        assert_eq!(target, artist.join("artist.png"));
        assert!(!unchanged);
        assert!(warnings.is_empty());
        assert!(!old.exists());
        let original = fs::read(&target).unwrap();

        image::RgbaImage::from_pixel(24, 24, image::Rgba([20, 220, 100, 255]))
            .save_with_format(&source, image::ImageFormat::Png)
            .unwrap();
        let replacement = fs::read(&source).unwrap();
        let (same_target, unchanged, warnings) =
            write_artist_art_file(artist, &replacement, false).unwrap();
        assert_eq!(same_target, target);
        assert!(!unchanged);
        assert!(warnings.is_empty());
        assert_ne!(fs::read(&target).unwrap(), original);

        let (numbered, unchanged, warnings) = write_artist_art_file(artist, &input, true).unwrap();
        assert_eq!(numbered, artist.join("artist-(2).png"));
        assert!(!unchanged);
        assert!(warnings.is_empty());
        assert!(target.is_file());
        let (same_numbered, unchanged, _) = write_artist_art_file(artist, &input, true).unwrap();
        assert_eq!(same_numbered, numbered);
        assert!(unchanged);
    }

    #[test]
    fn searched_artist_banner_precedes_saved_portrait_fallback() {
        let portrait = PathBuf::from("artist.png");
        let background = PathBuf::from("searched-background.png");
        assert_eq!(
            super::choose_artist_banner(
                Some(background.clone()),
                None,
                Some(portrait.clone()),
                None
            ),
            (Some(background.clone()), false),
        );
        assert_eq!(
            super::choose_artist_banner(
                None,
                Some(background.clone()),
                Some(portrait.clone()),
                None
            ),
            (Some(background), false),
        );
        assert_eq!(
            super::choose_artist_banner(None, None, Some(portrait.clone()), None),
            (Some(portrait), true),
        );
    }

    #[test]
    fn soundtrack_artists_use_separate_persistent_art_not_shared_artist_file() {
        let root = tempfile::TempDir::new().unwrap();
        let soundtracks = root.path().join("[Soundtracks]");
        let first_album = soundtracks.join("The Virgin Suicides");
        let second_album = soundtracks.join("Other Soundtrack");
        fs::create_dir_all(&first_album).unwrap();
        fs::create_dir_all(&second_album).unwrap();
        let cache_dir = root.path().join("database-state");
        let paths = [
            (
                "Air",
                first_album.to_str().unwrap(),
                "album-air",
                "cb67438a-7f50-4f2b-a6f1-2bb2729fd538",
            ),
            (
                "Alan Silvestri",
                second_album.to_str().unwrap(),
                "album-alan",
                "f4c53778-e48d-4da4-9ddd-cc007f715d64",
            ),
            (
                "Air",
                second_album.to_str().unwrap(),
                "album-air-2",
                "cb67438a-7f50-4f2b-a6f1-2bb2729fd538",
            ),
        ];
        let air_target = artist_art_targets_for_albums(paths, "Air", &cache_dir.join("artist-art"))
            .into_iter()
            .next()
            .unwrap();
        let alan_target =
            artist_art_targets_for_albums(paths, "Alan Silvestri", &cache_dir.join("artist-art"))
                .into_iter()
                .next()
                .unwrap();
        assert!(air_target.managed);
        assert_eq!(air_target.directory, cache_dir.join("artist-art"));
        assert_eq!(
            air_target.stem,
            "artist-mbid-cb67438a-7f50-4f2b-a6f1-2bb2729fd538"
        );
        assert_eq!(alan_target.directory, air_target.directory);
        assert_ne!(air_target.stem, alan_target.stem);

        let legacy_shared = soundtracks.join("artist.png");
        image::RgbImage::from_pixel(24, 24, image::Rgb([30, 40, 50]))
            .save_with_format(&legacy_shared, image::ImageFormat::Png)
            .unwrap();
        let legacy_bytes = fs::read(&legacy_shared).unwrap();
        let air_input = root.path().join("air.png");
        let alan_input = root.path().join("alan.png");
        image::RgbImage::from_pixel(24, 24, image::Rgb([220, 50, 70]))
            .save_with_format(&air_input, image::ImageFormat::Png)
            .unwrap();
        image::RgbImage::from_pixel(24, 24, image::Rgb([40, 80, 220]))
            .save_with_format(&alan_input, image::ImageFormat::Png)
            .unwrap();
        let (air_art, _, _) = write_artist_art_file_with_stem(
            &air_target.directory,
            &air_target.stem,
            &fs::read(&air_input).unwrap(),
            false,
        )
        .unwrap();
        let (alan_art, _, _) = write_artist_art_file_with_stem(
            &alan_target.directory,
            &alan_target.stem,
            &fs::read(&alan_input).unwrap(),
            false,
        )
        .unwrap();
        assert!(air_art.is_file());
        assert!(alan_art.is_file());
        assert_ne!(air_art, alan_art);
        assert_eq!(fs::read(&legacy_shared).unwrap(), legacy_bytes);

        let albums = paths
            .iter()
            .map(
                |(artist, path, album_mbid, artist_mbid)| ThumbnailAlbumTarget {
                    album_key: format!("{artist}:{path}"),
                    artist: (*artist).to_string(),
                    artist_mbid: (*artist_mbid).to_string(),
                    title: "Soundtrack".to_string(),
                    path: (*path).to_string(),
                    album_mbid: (*album_mbid).to_string(),
                    release_year: String::new(),
                    cover_path: String::new(),
                },
            )
            .collect::<Vec<_>>();
        assert_eq!(
            local_artist_art_for_albums(&albums, "Air", &cache_dir.join("artist-art")),
            Some(air_art.clone())
        );
        assert_eq!(
            local_artist_art_for_albums(&albums, "Alan Silvestri", &cache_dir.join("artist-art")),
            Some(alan_art)
        );
        assert_eq!(
            local_artist_art_for_albums(&albums, "Benj Pasek", &cache_dir.join("artist-art")),
            None
        );
        let air_candidates =
            local_artist_art_candidates(paths, "Air", &cache_dir.join("artist-art"));
        assert_eq!(air_candidates.len(), 1);
        assert_eq!(air_candidates[0].1, air_art.to_string_lossy());
    }

    #[test]
    fn artist_banner_prefers_artist_folder_and_scopes_soundtrack_fallback() {
        let root = tempfile::TempDir::new().unwrap();
        let artist_dir = root.path().join("Beyoncé");
        let artist_album = artist_dir.join("Lemonade");
        let soundtrack_album = root.path().join("[Soundtracks]").join("Soundtrack");
        fs::create_dir_all(&artist_album).unwrap();
        fs::create_dir_all(&soundtrack_album).unwrap();
        let shared = root.path().join("artist-state");
        let rows = [
            ("Beyoncé", artist_album.to_str().unwrap(), "album-id", ""),
            (
                "Beyoncé",
                soundtrack_album.to_str().unwrap(),
                "soundtrack-id",
                "",
            ),
        ];
        let targets = super::artist_banner_targets_for_albums(rows, "Beyoncé", &shared);
        assert_eq!(targets[0], (artist_dir.clone(), "banner".to_string()));
        assert_eq!(targets[1].0, shared);
        assert!(targets[1].1.starts_with("banner-name-"));

        let local_banner = artist_dir.join("banner.jpg");
        image::RgbImage::from_pixel(24, 12, image::Rgb([30, 40, 50]))
            .save_with_format(&local_banner, image::ImageFormat::Jpeg)
            .unwrap();
        let local_portrait = artist_dir.join("artist..jpg");
        image::RgbImage::from_pixel(12, 12, image::Rgb([60, 70, 80]))
            .save_with_format(&local_portrait, image::ImageFormat::Jpeg)
            .unwrap();
        let albums =
            rows.map(
                |(artist, path, album_mbid, artist_mbid)| super::ThumbnailAlbumTarget {
                    album_key: path.to_string(),
                    artist: artist.to_string(),
                    path: path.to_string(),
                    album_mbid: album_mbid.to_string(),
                    artist_mbid: artist_mbid.to_string(),
                    title: String::new(),
                    release_year: String::new(),
                    cover_path: String::new(),
                },
            );
        assert_eq!(
            super::local_artist_banner_for_albums(
                &albums,
                "Beyoncé",
                &root.path().join("artist-state")
            ),
            Some(local_banner)
        );
        assert_eq!(
            super::local_artist_art_for_albums(
                &albums,
                "Beyoncé",
                &root.path().join("artist-state")
            ),
            Some(local_portrait.clone())
        );
        let local_candidates =
            super::local_artist_art_candidates(rows, "Beyoncé", &root.path().join("artist-state"));
        assert!(local_candidates.iter().any(|(label, path)| {
            label == "Local artist banner"
                && path == artist_dir.join("banner.jpg").to_string_lossy().as_ref()
        }));
        assert!(local_candidates.iter().any(|(label, path)| {
            label == "Local artist file" && path == local_portrait.to_string_lossy().as_ref()
        }));
    }

    #[test]
    fn saved_artist_package_loads_from_managed_directory_without_navidrome() {
        let root = tempfile::TempDir::new().unwrap();
        let managed = root.path().join("artist-state");
        fs::create_dir_all(&managed).unwrap();
        let artist_id = "c2d25856-a09a-4d15-b404-77dd19c19e63";
        let portrait = managed.join(format!("artist-mbid-{artist_id}.png"));
        let banner = managed.join(format!("banner-mbid-{artist_id}.png"));
        image::RgbImage::from_pixel(24, 24, image::Rgb([30, 40, 50]))
            .save(&portrait)
            .unwrap();
        image::RgbImage::from_pixel(48, 24, image::Rgb([60, 70, 80]))
            .save(&banner)
            .unwrap();
        let albums = [super::ThumbnailAlbumTarget {
            album_key: "soundtrack".to_string(),
            artist: "Air".to_string(),
            artist_mbid: artist_id.to_string(),
            title: "The Virgin Suicides".to_string(),
            path: root
                .path()
                .join("[Soundtracks]")
                .join("The Virgin Suicides")
                .to_string_lossy()
                .into_owned(),
            album_mbid: "a release id".to_string(),
            release_year: String::new(),
            cover_path: String::new(),
        }];
        let package = super::saved_artist_package_for_albums(&albums, "Air", &managed);
        assert_eq!(package.portrait, Some(portrait));
        assert_eq!(package.banner, Some(banner));
    }

    #[test]
    fn various_artists_use_artist_scoped_art_but_comp_fallback_does_not() {
        let root = tempfile::TempDir::new().unwrap();
        let various = root.path().join("Various Artists");
        let album = various.join("A Valid Compilation");
        let fallback = various.join("COMP Fallback");
        fs::create_dir_all(&album).unwrap();
        fs::create_dir_all(&fallback).unwrap();
        let cache_dir = root.path().join("database-state");
        let rows = [
            ("Beyoncé", album.to_str().unwrap(), "album-id", ""),
            ("Prince", album.to_str().unwrap(), "album-id", ""),
            ("Track Credit", fallback.to_str().unwrap(), "", ""),
        ];
        let beyonce = artist_art_targets_for_albums(rows, "Beyoncé", &cache_dir.join("artist-art"));
        let prince = artist_art_targets_for_albums(rows, "Prince", &cache_dir.join("artist-art"));
        assert_eq!(beyonce.len(), 1);
        assert_eq!(prince.len(), 1);
        assert!(beyonce[0].managed);
        assert_eq!(beyonce[0].directory, cache_dir.join("artist-art"));
        assert_ne!(beyonce[0].stem, prince[0].stem);
        assert!(
            artist_art_targets_for_albums(rows, "Track Credit", &cache_dir.join("artist-art"))
                .is_empty()
        );
        assert!(
            local_artist_art_candidates(rows, "Track Credit", &cache_dir.join("artist-art"))
                .is_empty()
        );
    }

    #[test]
    fn local_thumbnails_prefer_saved_artist_and_album_art_after_restart() {
        let directory = tempfile::TempDir::new().unwrap();
        let artist = directory.path().join("Artist");
        let album_dir = artist.join("Album");
        fs::create_dir_all(&album_dir).unwrap();
        let old_artist = artist.join("artist.jpg");
        let new_artist = artist.join("artist.png");
        image::RgbImage::from_pixel(24, 24, image::Rgb([20, 50, 80]))
            .save_with_format(&old_artist, image::ImageFormat::Jpeg)
            .unwrap();
        let old_cover = album_dir.join("cover.jpg");
        image::RgbImage::from_pixel(24, 24, image::Rgb([20, 50, 80]))
            .save_with_format(&old_cover, image::ImageFormat::Jpeg)
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        image::RgbaImage::from_pixel(24, 24, image::Rgba([80, 50, 20, 255]))
            .save_with_format(&new_artist, image::ImageFormat::Png)
            .unwrap();
        let new_cover = album_dir.join("cover.png");
        image::RgbaImage::from_pixel(24, 24, image::Rgba([80, 50, 20, 255]))
            .save_with_format(&new_cover, image::ImageFormat::Png)
            .unwrap();
        assert_eq!(local_artist_art_file(&artist), Some(new_artist));
        let mut album = ThumbnailAlbumTarget {
            album_key: "album".to_string(),
            artist: "Artist".to_string(),
            artist_mbid: String::new(),
            title: "Album".to_string(),
            path: album_dir.to_string_lossy().into_owned(),
            album_mbid: "release-id".to_string(),
            release_year: String::new(),
            cover_path: old_cover.to_string_lossy().into_owned(),
        };
        assert_eq!(local_album_art_file(&album, "cover"), Some(new_cover));
        album.album_mbid.clear();
        assert_eq!(local_album_art_file(&album, "cover"), None);
    }

    #[test]
    fn comp_card_completion_follows_the_focused_artist_tracks() {
        let mut app = ApplicationState::new(
            Config::default(),
            MediaSnapshot {
                schema_version: splined_core::media_database::SCHEMA_VERSION,
                database_path: "splined.db".to_string(),
                canonical_root: "/music".to_string(),
                local_root: r"C:\Music".to_string(),
                albums: vec![MediaAlbumSnapshot {
                    album_key: "comp".to_string(),
                    artist: "Various Artists".to_string(),
                    tagged_artist: "Various Artists".to_string(),
                    title: "Jaar Radio Top 2000".to_string(),
                    path: r"C:\Music\Various Artists\Jaar Radio Top 2000".to_string(),
                    representative_file: String::new(),
                    status: "processed".to_string(),
                    compilation: true,
                    compilation_track_art_eligible: true,
                    compilation_tracks: vec![
                        MediaTrackSnapshot {
                            path: "wallstreet.mp3".to_string(),
                            title: "Wallstreet Shuffle".to_string(),
                            artist: "10cc".to_string(),
                            embedded_artwork_recorded: true,
                        },
                        MediaTrackSnapshot {
                            path: "other.mp3".to_string(),
                            title: "Other".to_string(),
                            artist: "Other Artist".to_string(),
                            embedded_artwork_recorded: false,
                        },
                    ],
                    track_count: 3,
                    compilation_completed_tracks: 0,
                    release_year: "2024".to_string(),
                    artist_mbid: String::new(),
                    album_mbid: String::new(),
                    release_group_mbid: String::new(),
                    has_local_artwork: false,
                    local_artwork_files: Vec::new(),
                    cover_path: String::new(),
                    cover_name: String::new(),
                    cover_format: String::new(),
                    cover_width: 0,
                    cover_height: 0,
                    root_files: 0,
                    cover_files: 0,
                    processed_at: None,
                    timeout_until: None,
                    selected_source: Some("embedded-compilation".to_string()),
                }],
            },
        )
        .unwrap();
        app.library.focused_artist = Some("10cc".to_string());
        app.artist_overview_track_paths.insert(
            "comp".to_string(),
            vec!["wallstreet.mp3".to_string(), "donna.mp3".to_string()],
        );
        let card = artist_overview_ui_model(&app).row_data(0).unwrap();
        assert!(card.fallback);
        assert!(!card.fallback_complete);
        assert!(card.check_embedded);
        assert!((0..card.bullets.row_count()).any(|index| {
            card.bullets.row_data(index).is_some_and(|bullet| {
                bullet.tip == "Checking Embedded Art against the thumbnail preference…"
            })
        }));
        assert!((0..card.bullets.row_count()).any(|index| {
            card.bullets
                .row_data(index)
                .is_some_and(|bullet| bullet.tip == "Selected Artist's COMP tracks incomplete")
        }));
        assert!(!(0..card.bullets.row_count()).any(|index| {
            card.bullets
                .row_data(index)
                .is_some_and(|bullet| bullet.tip == "Missing MusicBrainz Release ID")
        }));
        assert!(!(0..card.bullets.row_count()).any(|index| {
            card.bullets
                .row_data(index)
                .is_some_and(|bullet| bullet.tip == "Selected Artist's COMP tracks complete")
        }));

        app.library.albums[0]
            .snapshot
            .compilation_tracks
            .push(MediaTrackSnapshot {
                path: "donna.mp3".to_string(),
                title: String::new(),
                artist: String::new(),
                embedded_artwork_recorded: true,
            });
        let card = artist_overview_ui_model(&app).row_data(0).unwrap();
        assert!(card.fallback_complete);
        assert!(!(0..card.bullets.row_count()).any(|index| {
            card.bullets
                .row_data(index)
                .is_some_and(|bullet| bullet.tip == "Missing MusicBrainz Release ID")
        }));
        assert!((0..card.bullets.row_count()).any(|index| {
            card.bullets
                .row_data(index)
                .is_some_and(|bullet| bullet.tip == "Selected Artist's COMP tracks complete")
        }));
    }

    #[test]
    fn superseded_preview_render_cannot_commit() {
        let revision = AtomicU64::new(41);
        assert!(preview_request_is_current(Some((&revision, 41))));
        revision.store(42, Ordering::SeqCst);
        assert!(!preview_request_is_current(Some((&revision, 41))));
        assert!(preview_request_is_current(None));
    }

    #[test]
    fn upscale_preview_pixels_stay_in_the_bounded_memory_cache() {
        let image = image::RgbaImage::from_pixel(8, 6, image::Rgba([15, 70, 210, 255]));
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let cache_id = "memory-preview://test/upscale";
        assert!(preload_scaled_image_bytes(
            cache_id,
            encoded.get_ref(),
            PREVIEW_CACHE_DIMENSION
        ));
        let key = format!("{cache_id}\u{1f}{PREVIEW_CACHE_DIMENSION}");
        let cached = raw_image_cache().lock().unwrap().remove(&key).unwrap();
        assert_eq!((cached.source_width, cached.source_height), (8, 6));
        assert_eq!(cached.identity.0, encoded.get_ref().len() as u64);
    }

    #[test]
    fn repeated_file_preload_reuses_the_decoded_memory_cache_entry() {
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("thumbnail.png");
        image::RgbaImage::from_pixel(32, 24, image::Rgba([15, 70, 210, 255]))
            .save_with_format(&path, image::ImageFormat::Png)
            .unwrap();
        let path = path.to_string_lossy().into_owned();
        let key = format!("{path}\u{1f}160");

        assert!(preload_scaled_image(&path, 160));
        let first = raw_image_cache()
            .lock()
            .unwrap()
            .get(&key)
            .unwrap()
            .pixels
            .clone();
        assert!(preload_scaled_image(&path, 160));
        let second = raw_image_cache()
            .lock()
            .unwrap()
            .remove(&key)
            .unwrap()
            .pixels;

        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn displayed_file_cache_reloads_when_cover_is_replaced_at_same_path() {
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("cover.png");
        image::RgbaImage::from_pixel(32, 24, image::Rgba([15, 70, 210, 255]))
            .save_with_format(&path, image::ImageFormat::Png)
            .unwrap();
        let path = path.to_string_lossy().into_owned();
        assert!(preload_scaled_image(&path, 160));
        assert_eq!(load_scaled_image(&path, 160).size().width, 32);

        image::RgbaImage::from_pixel(64, 40, image::Rgba([210, 70, 15, 255]))
            .save_with_format(&path, image::ImageFormat::Png)
            .unwrap();
        assert_eq!(load_scaled_image(&path, 160).size().width, 64);

        fs::remove_file(&path).unwrap();
        assert_eq!(load_scaled_image(&path, 160).size().width, 0);
    }

    #[test]
    fn enabling_provider_in_settings_also_includes_it_in_artwork_search() {
        let mut config = Config::default();
        config
            .sources
            .cover_sources
            .retain(|source| source != "navidrome");
        config
            .sources
            .exclude_cover_sources
            .push("navidrome".into());
        include_artwork_source(&mut config, "navidrome");
        include_artwork_source(&mut config, "navidrome");
        assert_eq!(
            config
                .sources
                .cover_sources
                .iter()
                .filter(|source| source.as_str() == "navidrome")
                .count(),
            1
        );
        assert!(
            !config
                .sources
                .exclude_cover_sources
                .contains(&"navidrome".into())
        );
    }

    #[test]
    fn unchanged_musicbrainz_rows_keep_the_existing_touch_areas() {
        let row = UiMusicBrainzRow {
            index: 7,
            title: "Album".into(),
            artist: "Artist".into(),
            detail: "US · 1977 · album".into(),
            group: "Release Family · Album".into(),
            section: "ARTIST RELEASES · Same-artist album priority".into(),
            group_start: true,
            current: false,
            visited: false,
            selected: false,
            score: 100,
        };
        let current = model(vec![row.clone()]);

        assert!(musicbrainz_ui_rows_equal(
            &current,
            std::slice::from_ref(&row)
        ));

        let mut selected = row;
        selected.selected = true;
        assert!(!musicbrainz_ui_rows_equal(&current, &[selected]));
    }

    #[test]
    fn musicbrainz_preview_requests_are_single_flight_and_page_cached() {
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut app = ApplicationState::new(Config::default(), snapshot).unwrap();
        app.apply_runtime_event(&json!({
            "event": "musicbrainz_matches",
            "album_artist": "10cc",
            "album": "The Things We Do for Love",
            "items": [{
                "index": 7,
                "release_title": "Deceptive Bends",
                "release_artist": "10cc",
                "release_mbid": "11111111-1111-1111-1111-111111111111"
            }]
        }));

        assert_eq!(
            begin_musicbrainz_preview_request(&mut app, 7, true).as_deref(),
            Some("11111111-1111-1111-1111-111111111111")
        );
        assert!(app.musicbrainz_preview_inflight.contains(&7));
        assert!(begin_musicbrainz_preview_request(&mut app, 7, true).is_none());

        app.musicbrainz_preview_inflight.remove(&7);
        app.musicbrainz_preview_cache
            .insert(7, r"C:\Cache\musicbrainz.jpg".to_string());
        assert!(begin_musicbrainz_preview_request(&mut app, 7, false).is_none());

        app.musicbrainz_preview_cache.clear();
        app.musicbrainz_missing_cover.insert(7);
        assert!(begin_musicbrainz_preview_request(&mut app, 7, false).is_none());
        app.apply_runtime_event(&json!({
            "event": "musicbrainz_matches",
            "items": [{
                "index": 7,
                "release_title": "Another Release",
                "release_artist": "10cc",
                "release_mbid": "22222222-2222-2222-2222-222222222222"
            }]
        }));
        assert!(!app.musicbrainz_missing_cover.contains(&7));
        assert_eq!(
            begin_musicbrainz_preview_request(&mut app, 7, false).as_deref(),
            Some("22222222-2222-2222-2222-222222222222")
        );
    }

    #[test]
    fn musicbrainz_upscale_preview_uses_the_selected_release_cover() {
        let directory = tempfile::TempDir::new().unwrap();
        let cover = directory.path().join("musicbrainz.png");
        image::RgbaImage::from_pixel(96, 96, image::Rgba([42, 90, 170, 255]))
            .save_with_format(&cover, image::ImageFormat::Png)
            .unwrap();
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut app = ApplicationState::new(Config::default(), snapshot).unwrap();
        app.phase = RunPhase::AwaitingMusicBrainz;
        app.selected_musicbrainz_result = Some(4);
        app.current_album_path = directory.path().to_string_lossy().into_owned();
        app.musicbrainz_cover_preview_path = cover.to_string_lossy().into_owned();
        app.edit_preview_path = app.musicbrainz_cover_preview_path.clone();
        let state = Arc::new(Mutex::new(app));

        preview_musicbrainz_cover(&state, None).unwrap();

        let app = state.lock().unwrap();
        assert!(
            app.edit_preview_path
                .starts_with("memory-preview://musicbrainz/4/")
        );
        assert_eq!(app.musicbrainz_cover_preview_path, cover.to_string_lossy());
    }

    #[test]
    fn embedded_thumbnail_preview_is_in_memory_and_invalidates_on_selection_change() {
        let directory = tempfile::TempDir::new().unwrap();
        let cover = directory.path().join("existing.png");
        image::RgbaImage::from_pixel(96, 96, image::Rgba([90, 40, 170, 255]))
            .save_with_format(&cover, image::ImageFormat::Png)
            .unwrap();
        let original = fs::read(&cover).unwrap();
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut config = Config::default();
        config.output.embedded_thumbnail_size = 300;
        let mut app = ApplicationState::new(config, snapshot).unwrap();
        app.phase = RunPhase::AwaitingMusicBrainz;
        app.musicbrainz_existing_artwork_path = cover.to_string_lossy().into_owned();
        let state = Arc::new(Mutex::new(app));
        prepare_embedded_thumbnail_preview(&state).unwrap();
        let mut app = state.lock().unwrap();
        assert!(embedded_thumbnail_ready(&app));
        assert_eq!(
            super::cached_preview_metadata(&app.embedded_thumbnail_preview_path)
                .map(|(width, height, _)| (width, height)),
            Some((300, 300))
        );
        app.edit_preview_path = "memory-preview://upscaled-cover".to_string();
        app.embedded_thumbnail_owner_key = embedded_thumbnail_owner_key(&app);
        clear_rendered_preview_for_adjustment(&mut app);
        assert_eq!(app.edit_preview_path, "memory-preview://upscaled-cover");
        assert!(embedded_thumbnail_ready(&app));
        let mut decision = json!({"action": "use_existing_compilation_artwork"});
        add_embedded_thumbnail_decision(&app, &mut decision);
        assert!(decision.get("embed_thumbnails").is_none());
        assert_eq!(decision["use_upscaled_preview"], true);
        assert_eq!(fs::read(&cover).unwrap(), original);
        assert!(disable_embedded_thumbnail_preview(&mut app));
        assert!(!embedded_thumbnail_ready(&app));
        assert_eq!(
            app.musicbrainz_existing_artwork_path,
            cover.to_string_lossy()
        );
        assert_eq!(fs::read(&cover).unwrap(), original);
        drop(app);
        prepare_embedded_thumbnail_preview(&state).unwrap();
        let mut app = state.lock().unwrap();
        assert!(embedded_thumbnail_ready(&app));
        app.selected_musicbrainz_result = Some(4);
        assert!(!embedded_thumbnail_ready(&app));
    }

    #[test]
    fn comp_thumbnail_preview_is_derived_from_upscaled_full_resolution_art() {
        let directory = tempfile::TempDir::new().unwrap();
        let cover = directory.path().join("musicbrainz.png");
        image::RgbaImage::from_fn(320, 320, |x, y| {
            image::Rgba([x as u8, y as u8, (x ^ y) as u8, 255])
        })
        .save_with_format(&cover, image::ImageFormat::Png)
        .unwrap();
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut config = Config::default();
        config.range.min = 300;
        config.range.ideal = 640;
        config.range.max = 1280;
        config.range.ladder = 1280;
        config.output.upscale_below_ideal = true;
        config.output.upscale_max_percent = 200;
        config.output.embedded_thumbnail_size = 300;
        let mut app = ApplicationState::new(config.clone(), snapshot).unwrap();
        app.phase = RunPhase::AwaitingMusicBrainz;
        app.selected_musicbrainz_result = Some(1);
        app.musicbrainz_cover_preview_path = cover.to_string_lossy().into_owned();
        app.current_compilation_track_path = "selected-comp-track.mp3".into();
        app.edit_preview_path = "memory-preview://musicbrainz/1/1".into();
        let state = Arc::new(Mutex::new(app));
        prepare_embedded_thumbnail_preview(&state).unwrap();
        let app = state.lock().unwrap();
        assert!(embedded_thumbnail_ready(&app));
        let cache_key = format!(
            "{}\u{1f}{}",
            app.embedded_thumbnail_preview_path,
            super::PREVIEW_CACHE_DIMENSION
        );
        let actual = raw_image_cache()
            .lock()
            .unwrap()
            .get(&cache_key)
            .unwrap()
            .pixels
            .clone();
        drop(app);

        let candidate = splined_core::candidate::Candidate::from_file("test", &cover, 0).unwrap();
        let range = splined_core::range::Range {
            min: config.range.min,
            ideal: config.range.ideal,
            max: config.range.max,
            ladder: config.range.ladder,
        };
        let upscaled = splined_core::final_artwork::prepare_selected_artwork_edit(
            &candidate,
            &cover,
            &range,
            splined_core::candidate::StaticFormat::Png,
            &config.output,
            true,
        )
        .unwrap();
        assert_eq!(upscaled.info.width, 640);
        let expected = splined_core::final_artwork::prepare_embedded_thumbnail(
            &upscaled.bytes,
            config.output.embedded_thumbnail_size,
            splined_core::candidate::StaticFormat::Png,
        )
        .unwrap();
        assert_eq!(
            actual.as_ref(),
            image::load_from_memory(&expected)
                .unwrap()
                .into_rgba8()
                .into_raw()
        );
    }

    #[test]
    fn existing_musicbrainz_upscale_preview_uses_existing_art_without_requiring_it() {
        let directory = tempfile::TempDir::new().unwrap();
        let cover = directory.path().join("existing.png");
        image::RgbaImage::from_pixel(96, 96, image::Rgba([90, 40, 170, 255]))
            .save_with_format(&cover, image::ImageFormat::Png)
            .unwrap();
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut app = ApplicationState::new(Config::default(), snapshot).unwrap();
        app.phase = RunPhase::AwaitingMusicBrainz;
        app.musicbrainz_existing_artwork_path = cover.to_string_lossy().into_owned();
        app.current_cover_path = directory
            .path()
            .join("wrong-comp-cover.png")
            .to_string_lossy()
            .into_owned();
        let state = Arc::new(Mutex::new(app));

        preview_musicbrainz_existing(&state, None).unwrap();

        let app = state.lock().unwrap();
        assert!(
            app.edit_preview_path
                .starts_with("memory-preview://musicbrainz-existing/")
        );
        assert_eq!(
            app.musicbrainz_existing_artwork_path,
            cover.to_string_lossy()
        );
    }

    #[test]
    fn compilation_track_upscale_uses_its_embedded_preview_not_folder_cover() {
        let directory = tempfile::TempDir::new().unwrap();
        let embedded = directory.path().join("embedded.png");
        image::RgbaImage::from_pixel(96, 96, image::Rgba([170, 60, 125, 255]))
            .save_with_format(&embedded, image::ImageFormat::Png)
            .unwrap();
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut app = ApplicationState::new(Config::default(), snapshot).unwrap();
        let track = directory.path().join("01.mp3");
        let track = track.to_string_lossy().into_owned();
        app.library
            .selected_compilation_track_paths
            .insert(track.to_ascii_lowercase());
        app.set_compilation_track_preview(&track, &embedded);
        app.current_cover_path.clear();
        assert!(
            preview_image_source_paths(&app).contains(embedded.to_string_lossy().as_ref()),
            "the dedicated track preview must be queued for pixel decoding"
        );
        let state = Arc::new(Mutex::new(app));

        preview_compilation_track(&state, None).unwrap();

        let app = state.lock().unwrap();
        assert!(
            app.edit_preview_path
                .starts_with("memory-preview://compilation-track/")
        );
        assert_eq!(
            app.active_compilation_track_preview(),
            Some(embedded.to_string_lossy().as_ref())
        );
        assert!(app.current_cover_path.is_empty());
    }

    #[test]
    fn comp_report_preview_checks_only_the_clicked_track_not_the_whole_album() {
        let directory = tempfile::TempDir::new().unwrap();
        let first = directory.path().join("01.mp3");
        let second = directory.path().join("02.mp3");
        fs::write(&first, []).unwrap();
        fs::write(&second, []).unwrap();
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut app = ApplicationState::new(Config::default(), snapshot).unwrap();
        app.current_album_path = directory.path().to_string_lossy().into_owned();
        assert_eq!(current_embedded_thumbnail_infos(&app).len(), 2);
        super::EMBEDDED_METADATA_CACHE
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .remove(first.to_string_lossy().as_ref());
        app.compilation_report_preview_mode = true;
        app.compilation_report_preview_track_path = first.to_string_lossy().into_owned();
        assert!(current_embedded_thumbnail_infos(&app).is_empty());
        super::cache_compilation_report_thumb_metadata(
            &first,
            &fs::metadata(&first).unwrap(),
            None,
        );
        assert_eq!(current_embedded_thumbnail_infos(&app).len(), 1);
    }

    #[test]
    fn comp_report_preview_preloads_only_the_saved_thumb() {
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut app = ApplicationState::new(Config::default(), snapshot).unwrap();
        app.library.show_tracks = true;
        app.library.focused_album_key = Some("comp-a".into());
        app.compilation_report_preview_mode = true;
        app.compilation_report_preview_album_key = "comp-a".into();
        app.compilation_report_preview_track_path = "C:/music/comp/01.mp3".into();
        app.current_cover_path = "C:/music/comp/cover.jpg".into();
        app.edit_preview_path = "C:/cache/old-edit.jpg".into();
        app.set_compilation_track_preview(
            "C:/music/comp/01.mp3",
            std::path::Path::new("C:/cache/embedded.jpg"),
        );
        let paths = preview_image_source_paths(&app);
        assert_eq!(paths.len(), 1);
        assert!(paths.contains("C:/cache/embedded.jpg"));
    }

    #[test]
    fn compilation_track_never_falls_back_to_album_folder_cover() {
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut app = ApplicationState::new(Config::default(), snapshot).unwrap();
        app.current_cover_path = "C:/music/Various Artists/Compilation/cover.jpg".to_string();
        assert!(!fallback_compilation_cover_suppressed(&app, false));

        app.current_compilation_track_path =
            "c:/music/various artists/compilation/01.mp3".to_string();
        assert!(fallback_compilation_cover_suppressed(&app, false));
        app.current_compilation_track_path.clear();
        app.library
            .selected_compilation_track_paths
            .insert("c:/music/various artists/compilation/01.mp3".to_string());
        assert!(fallback_compilation_cover_suppressed(&app, false));
    }

    #[test]
    fn compilation_write_action_never_claims_to_write_cover() {
        assert_eq!(
            selected_artwork_write_label(false, true, false, false),
            "EMBED Comp"
        );
        assert_eq!(
            selected_artwork_write_label(false, true, false, true),
            "EMBED Comp"
        );
        assert_eq!(
            selected_artwork_write_label(false, true, true, false),
            "Embed Existing"
        );
        assert_eq!(
            selected_artwork_write_label(false, true, true, true),
            "EMBED Comp"
        );
        assert_eq!(
            selected_artwork_write_label(false, false, false, true),
            "WRITE Cover"
        );
        assert_eq!(
            selected_artwork_write_label(false, false, true, true),
            "WRITE Cover"
        );
    }

    #[test]
    fn prepared_existing_art_is_labeled_as_an_embedded_thumb() {
        assert_eq!(
            selected_artwork_preview_type(true, "Front Cover"),
            "Embedded Thumb"
        );
        assert_eq!(
            selected_artwork_preview_type(false, "Front Cover"),
            "Front Cover"
        );
    }

    #[test]
    fn selected_compilation_track_cannot_edit_album_cover() {
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut app = ApplicationState::new(Config::default(), snapshot).unwrap();
        app.current_cover_path = "C:/music/Various Artists/Compilation/cover.jpg".into();
        app.library
            .selected_compilation_track_paths
            .insert("c:/music/various artists/compilation/01.mp3".into());
        assert!(compilation_artwork_context(&app));
        assert!(require_active_compilation_write_target(&app).is_err());
        app.current_compilation_track_path = "c:/music/various artists/compilation/01.mp3".into();
        assert!(require_active_compilation_write_target(&app).is_ok());
        let state = Arc::new(Mutex::new(app));
        assert!(
            edit_existing_cover(&state, true, None)
                .unwrap_err()
                .contains("COMP fallback artwork")
        );
    }

    #[test]
    fn musicbrainz_existing_art_is_default_preview_until_a_release_is_selected() {
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut app = ApplicationState::new(Config::default(), snapshot).unwrap();
        app.phase = RunPhase::AwaitingMusicBrainz;
        app.musicbrainz_existing_artwork_path = "C:/cache/music-box.jpg".to_string();
        assert_eq!(
            musicbrainz_existing_preview_path(&app, false),
            Some("C:/cache/music-box.jpg")
        );
        assert_eq!(musicbrainz_existing_preview_path(&app, true), None);
        app.selected_musicbrainz_result = Some(3);
        assert_eq!(musicbrainz_existing_preview_path(&app, false), None);
    }

    #[test]
    fn large_preview_cache_never_evicts_currently_displayed_images() {
        let key = |name: &str| format!("{name}\u{1f}{PREVIEW_CACHE_DIMENSION}");
        let pixels = || CachedPixels {
            identity: (4, None),
            pixels: Arc::<[u8]>::from([0, 0, 0, 0]),
            width: 1,
            height: 1,
            source_width: 1,
            source_height: 1,
        };
        let mut cache = HashMap::new();
        for name in [
            "stale-a", "stale-b", "stale-c", "stale-d", "active-a", "active-b",
        ] {
            cache.insert(key(name), pixels());
        }
        let active = HashSet::from([key("active-a"), key("active-b")]);

        trim_large_raw_image_cache(&mut cache, &active, 3);

        assert!(cache.contains_key(&key("active-a")));
        assert!(cache.contains_key(&key("active-b")));
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn themed_artist_package_keeps_both_preview_images_during_refresh() {
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut app = ApplicationState::new(Config::default(), snapshot).unwrap();
        app.artist_artwork_mode = true;
        app.artist_preview_focus = "theme".to_string();
        app.artist_theme_portrait_preview_path =
            "memory-preview://artist-theme/portrait/1".to_string();
        app.artist_theme_banner_preview_path = "memory-preview://artist-theme/banner/1".to_string();

        clear_rendered_preview_for_adjustment(&mut app);

        assert!(preview_image_source_paths(&app).contains(&app.artist_theme_portrait_preview_path));
        assert!(preview_image_source_paths(&app).contains(&app.artist_theme_banner_preview_path));
    }

    #[test]
    fn direct_write_cover_marks_the_focused_album_processed_without_launch() {
        let directory = tempfile::TempDir::new().unwrap();
        let library = directory.path().join("music");
        let album_directory = library.join("Artist").join("Album");
        fs::create_dir_all(&album_directory).unwrap();
        fs::write(album_directory.join("01.mp3"), []).unwrap();
        let cover = album_directory.join("cover.png");
        image::RgbaImage::from_pixel(96, 96, image::Rgba([42, 90, 170, 255]))
            .save_with_format(&cover, image::ImageFormat::Png)
            .unwrap();

        let mut config = Config {
            mode: Mode::Write,
            ..Config::default()
        };
        config.library.music_library = library.to_string_lossy().into_owned();
        config.scan.scan_library_dir = config.library.music_library.clone();
        config.scan.cache_dir = directory
            .path()
            .join("database")
            .to_string_lossy()
            .into_owned();
        config.scan.temporary_cache_dir = directory
            .path()
            .join("temporary")
            .to_string_lossy()
            .into_owned();

        let mut snapshot = media_snapshot(&config, false).unwrap();
        assert_eq!(snapshot.albums.len(), 1);
        assert_eq!(snapshot.albums[0].status, "unprocessed");
        snapshot.albums[0].album_mbid = "artist-album-release-id".to_string();
        let album_key = snapshot.albums[0].album_key.clone();
        let mut app = ApplicationState::new(config, snapshot).unwrap();
        app.library.focused_album_key = Some(album_key.clone());
        app.current_artist = "Artist".to_string();
        app.current_album = "Album".to_string();
        app.current_album_path = album_directory.to_string_lossy().into_owned();
        app.current_cover_path = cover.to_string_lossy().into_owned();
        let state = Arc::new(Mutex::new(app));

        edit_existing_cover(&state, true, None).unwrap();

        let app = state.lock().unwrap();
        let album = app
            .library
            .albums
            .iter()
            .find(|album| album.snapshot.album_key == album_key)
            .unwrap();
        assert_eq!(album.snapshot.status, "processed");
        assert!(app.service_status.contains("marked Processed"));
    }

    #[test]
    fn embed_batch_staging_writes_selected_cover_before_tracks_start() {
        let directory = tempfile::TempDir::new().unwrap();
        let library = directory.path().join("music");
        let album = library.join("Artist").join("Album");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("01.mp3"), []).unwrap();
        let cover = album.join("cover.jpg");
        image::RgbImage::from_pixel(96, 96, image::Rgb([10, 20, 30]))
            .save_with_format(&cover, image::ImageFormat::Jpeg)
            .unwrap();
        let preview_file = directory.path().join("preview.png");
        image::RgbaImage::from_pixel(144, 144, image::Rgba([180, 90, 40, 255]))
            .save_with_format(&preview_file, image::ImageFormat::Png)
            .unwrap();
        let preview = fs::read(&preview_file).unwrap();
        let mut config = Config {
            mode: Mode::Write,
            ..Config::default()
        };
        config.library.music_library = library.to_string_lossy().into_owned();
        config.scan.scan_library_dir = config.library.music_library.clone();
        config.scan.cache_dir = directory
            .path()
            .join("database")
            .to_string_lossy()
            .into_owned();
        config.scan.temporary_cache_dir = directory
            .path()
            .join("temporary")
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            media_snapshot(&config, false).unwrap().albums[0].status,
            "unprocessed"
        );
        let queue_db = directory.path().join("embed-queue.sqlite");
        let mut staged = queue_saved_album(
            &queue_db,
            EmbedQueueTarget {
                album: album.clone(),
                source: EmbedQueueSource::Preview(preview),
                label: "Artist — Album".into(),
                side: config.output.embedded_thumbnail_size,
                cover_config: Some(config),
                preferred_cover: Some(cover.clone()),
            },
        )
        .unwrap();
        assert_eq!(staged.written_cover.as_deref(), Some(cover.as_path()));
        let info = inspect_image(&cover).unwrap();
        assert_eq!(
            (info.width, info.height, info.format),
            (144, 144, StaticFormat::Jpeg)
        );
        assert!(!album.join("cover.png").exists());
        assert_eq!(
            staged.snapshot.take().unwrap().albums[0].status,
            "processed"
        );
        let jobs = embed_queue::list(&queue_db).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].state, "staged");
        assert_eq!(jobs[0].completed, 0);
    }

    #[test]
    fn failed_cover_write_does_not_leave_a_staged_embed_job() {
        let directory = tempfile::TempDir::new().unwrap();
        let album = directory.path().join("Artist").join("Album");
        fs::create_dir_all(album.join("cover.jpg")).unwrap();
        fs::write(album.join("01.mp3"), []).unwrap();
        let preview_file = directory.path().join("preview.jpg");
        image::RgbImage::from_pixel(96, 96, image::Rgb([20, 100, 180]))
            .save_with_format(&preview_file, image::ImageFormat::Jpeg)
            .unwrap();
        let queue_db = directory.path().join("embed-queue.sqlite");
        let result = queue_saved_album(
            &queue_db,
            EmbedQueueTarget {
                album,
                source: EmbedQueueSource::Preview(fs::read(preview_file).unwrap()),
                label: "Artist — Album".into(),
                side: 1200,
                cover_config: Some(Config::default()),
                preferred_cover: None,
            },
        );
        assert!(result.is_err());
        assert!(embed_queue::list(&queue_db).unwrap().is_empty());
    }

    #[test]
    fn direct_write_from_webp_preserves_animation_and_creates_jpeg_still() {
        let directory = tempfile::TempDir::new().unwrap();
        let library = directory.path().join("music");
        let album_directory = library.join("Artist").join("Album");
        fs::create_dir_all(&album_directory).unwrap();
        fs::write(album_directory.join("01.mp3"), []).unwrap();
        let webp = album_directory.join("cover.webp");
        image::RgbaImage::from_pixel(96, 96, image::Rgba([42, 90, 170, 255]))
            .save_with_format(&webp, image::ImageFormat::WebP)
            .unwrap();
        let original = fs::read(&webp).unwrap();
        let mut config = Config {
            mode: Mode::Write,
            ..Config::default()
        };
        config.library.music_library = library.to_string_lossy().into_owned();
        config.scan.scan_library_dir = config.library.music_library.clone();
        config.scan.cache_dir = directory
            .path()
            .join("database")
            .to_string_lossy()
            .into_owned();
        config.scan.temporary_cache_dir = directory
            .path()
            .join("temporary")
            .to_string_lossy()
            .into_owned();
        let initial = media_snapshot(&config, false).unwrap();
        let connection = rusqlite::Connection::open(&initial.database_path).unwrap();
        connection
            .execute(
                "UPDATE albums SET musicbrainz_albumid = 'artist-album-release-id'",
                [],
            )
            .unwrap();
        drop(connection);
        let mut snapshot = media_snapshot(&config, false).unwrap();
        snapshot.albums[0].local_artwork_files = vec![webp.to_string_lossy().into_owned()];
        snapshot.albums[0].cover_name = "cover.webp".to_string();
        snapshot.albums[0].cover_path = webp.to_string_lossy().into_owned();
        assert_eq!(canonical_webp_status(&snapshot.albums[0]), (true, true));
        let key = snapshot.albums[0].album_key.clone();
        let mut app = ApplicationState::new(config, snapshot).unwrap();
        app.library.focused_album_key = Some(key);
        app.current_artist = "Artist".to_string();
        app.current_album = "Album".to_string();
        app.current_album_path = album_directory.to_string_lossy().into_owned();
        app.current_cover_path = webp.to_string_lossy().into_owned();
        let state = Arc::new(Mutex::new(app));

        edit_existing_cover(&state, true, None).unwrap();

        let jpg = album_directory.join("cover.jpg");
        assert!(jpg.is_file());
        assert_eq!(
            image::ImageFormat::from_path(&jpg).unwrap(),
            image::ImageFormat::Jpeg
        );
        assert_eq!(fs::read(&webp).unwrap(), original);
        let app = state.lock().unwrap();
        assert_eq!(app.current_cover_path, jpg.to_string_lossy());
        let album = &app.library.albums[0].snapshot;
        assert_eq!(canonical_webp_status(album), (true, false));
    }

    #[test]
    fn embed_queue_creates_a_still_only_for_webp_without_one() {
        let directory = tempfile::TempDir::new().unwrap();
        let album = directory.path();
        let webp = album.join("cover.webp");
        image::RgbaImage::from_pixel(32, 24, image::Rgba([42, 90, 170, 255]))
            .save_with_format(&webp, image::ImageFormat::WebP)
            .unwrap();
        let original = fs::read(&webp).unwrap();
        let still = ensure_webp_album_still(album).unwrap().unwrap();
        assert_eq!(still.file_name().unwrap(), "cover.jpg");
        assert_eq!(fs::read(&webp).unwrap(), original);
        let decoded = image::open(&still).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (32, 24));
        assert!(ensure_webp_album_still(album).unwrap().is_none());

        fs::remove_file(&still).unwrap();
        let png = album.join("cover.png");
        fs::write(&png, b"existing still").unwrap();
        assert!(ensure_webp_album_still(album).unwrap().is_none());
        assert!(!still.exists());
    }

    #[test]
    fn settings_rebuild_keeps_locked_filter_authority() {
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut previous = ApplicationState::new(Config::default(), snapshot.clone()).unwrap();
        previous.media_filter_locked = true;
        previous.candidate_filter_locked = true;
        previous.library.set_artist_filter("Brenton Wood");
        previous.library.set_album_filter("Oogum Boogum");
        previous.library.set_status_filter(StatusFilter::Processed);
        previous
            .candidate_included_types
            .insert("FrontCover".to_string());
        let mut replacement = ApplicationState::new(Config::default(), snapshot).unwrap();
        preserve_filter_state(&mut replacement, &previous);
        assert!(replacement.media_filter_locked);
        assert!(replacement.candidate_filter_locked);
        assert_eq!(replacement.library.artist_filter, "Brenton Wood");
        assert_eq!(replacement.library.album_filter, "Oogum Boogum");
        assert!(
            replacement
                .library
                .status_filters
                .contains(&StatusFilter::Processed)
        );
        assert!(replacement.candidate_included_types.contains("FrontCover"));
    }

    #[test]
    fn fallback_album_focus_never_projects_folder_cover_art() {
        let directory = tempfile::TempDir::new().unwrap();
        let library = directory.path().join("music");
        let album_directory = library.join("Various Artists").join("Soundtrack");
        fs::create_dir_all(&album_directory).unwrap();
        fs::write(album_directory.join("01.mp3"), []).unwrap();
        let card_cover = album_directory.join("cover.png");
        image::RgbaImage::from_pixel(96, 96, image::Rgba([180, 40, 90, 255]))
            .save_with_format(&card_cover, image::ImageFormat::Png)
            .unwrap();

        let mut config = Config::default();
        config.library.music_library = library.to_string_lossy().into_owned();
        config.scan.scan_library_dir = config.library.music_library.clone();
        config.scan.cache_dir = directory
            .path()
            .join("database")
            .to_string_lossy()
            .into_owned();
        let snapshot = media_snapshot(&config, false).unwrap();
        let album_key = snapshot.albums[0].album_key.clone();
        let mut app = ApplicationState::new(config, snapshot).unwrap();
        assert_eq!(
            artist_album_card_artwork_path(&app, &app.library.albums[0].snapshot),
            card_cover.to_string_lossy()
        );
        assert!(!app.library.album_artwork_paths.contains_key(&album_key));
        app.library.set_show_tracks(true);
        app.library.focused_album_key = Some(album_key);
        app.edit_preview_path = "stale-preview".to_string();

        synchronize_fallback_album(&mut app);

        assert_eq!(app.current_album, "Soundtrack");
        assert!(app.current_cover_path.is_empty());
        assert!(app.edit_preview_path.is_empty());
        assert!(compilation_artwork_context(&app));
        assert!(app.current_compilation_track_path.is_empty());
        assert!(require_active_compilation_write_target(&app).is_err());
        app.library.view = LibraryView::Artists;
        app.library.set_show_tracks(false);
        assert!(compilation_artwork_context(&app));
        assert!(require_active_compilation_write_target(&app).is_err());
    }

    #[test]
    fn artist_banner_is_a_bounded_blurred_runtime_derivative() {
        let directory = tempfile::TempDir::new().unwrap();
        let source = directory.path().join("artist.png");
        image::RgbaImage::from_pixel(320, 480, image::Rgba([80, 140, 220, 255]))
            .save_with_format(&source, image::ImageFormat::Png)
            .unwrap();
        let banner =
            cached_artist_banner_with_bottom_blur(&source, directory.path(), "artist-id").unwrap();
        let inspected = splined_core::inspect::inspect_image(&banner).unwrap();
        assert_eq!((inspected.width, inspected.height), (1400, 600));
        assert_eq!(
            banner,
            cached_artist_banner_with_bottom_blur(&source, directory.path(), "artist-id").unwrap()
        );
        image::RgbaImage::from_pixel(320, 480, image::Rgba([220, 80, 80, 255]))
            .save_with_format(&source, image::ImageFormat::Png)
            .unwrap();
        let refreshed =
            cached_artist_banner_with_bottom_blur(&source, directory.path(), "artist-id").unwrap();
        assert_ne!(banner, refreshed);
        assert_eq!(
            splined_core::inspect::inspect_image(&refreshed)
                .unwrap()
                .width,
            1400
        );
    }

    #[test]
    fn auto_banner_fit_fills_the_target_and_enhancement_changes_approved_pixels() {
        let directory = tempfile::TempDir::new().unwrap();
        let source = directory.path().join("portrait.png");
        image::RgbaImage::from_pixel(320, 480, image::Rgba([80, 140, 220, 255]))
            .save_with_format(&source, image::ImageFormat::Png)
            .unwrap();
        let zoom = super::auto_artist_banner_zoom(&source);
        assert!(zoom > 3.0);
        let bytes = std::fs::read(&source).unwrap();
        let mut output = splined_core::config::OutputConfig::default();
        output.upscale_adaptive_defaults = false;
        output.upscale_hue_degrees = 90;
        let enhanced = super::enhanced_artist_banner(&bytes, zoom, 0.0, &output)
            .unwrap()
            .to_rgba8();
        assert_eq!(enhanced.dimensions(), (1400, 600));
        output.upscale_hue_degrees = 0;
        let original = super::enhanced_artist_banner(&bytes, zoom, 0.0, &output)
            .unwrap()
            .to_rgba8();
        assert_ne!(enhanced.get_pixel(700, 300), original.get_pixel(700, 300));
    }

    #[test]
    fn artist_fitment_moves_the_written_round_portrait_without_changing_saved_source() {
        let mut source = image::RgbaImage::new(128, 128);
        for (x, _, pixel) in source.enumerate_pixels_mut() {
            *pixel = if x < 64 { image::Rgba([240, 20, 20, 255]) } else { image::Rgba([20, 40, 240, 255]) };
        }
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(source).write_to(&mut encoded, image::ImageFormat::Png).unwrap();
        let left = image::load_from_memory(&super::prepare_artist_art_png_with_fit(encoded.get_ref(), None, 2.0, 0.4, 0.0).unwrap()).unwrap().to_rgba8();
        let right = image::load_from_memory(&super::prepare_artist_art_png_with_fit(encoded.get_ref(), None, 2.0, -0.4, 0.0).unwrap()).unwrap().to_rgba8();
        assert!(left.get_pixel(512, 512).0[0] > left.get_pixel(512, 512).0[2]);
        assert!(right.get_pixel(512, 512).0[2] > right.get_pixel(512, 512).0[0]);
        assert_eq!(left.get_pixel(0, 0).0[3], 0);
    }

    #[test]
    fn repeated_filters_stack_and_stop_at_slider_limits() {
        let clarity = [4, 12, 0, 8, 0, 0, 0, 0, 0, 0, 0];
        let twice = super::stack_adjustment_values(clarity, clarity);
        assert_eq!(twice[0], 8);
        assert_eq!(twice[1], 20);
        assert_eq!(super::stack_adjustment_values(twice, clarity)[1], 20);
    }

    #[test]
    fn enhancement_filter_intensity_is_separate_for_album_portrait_and_banner() {
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(), canonical_root: String::new(), local_root: String::new(),
            albums: Vec::new(),
        };
        let mut app = ApplicationState::new(Config::default(), snapshot).unwrap();
        assert!(super::change_filter_level(&mut app, "Clarity", 3));
        assert_eq!(app.album_filter_levels[1], 3);
        app.artist_artwork_mode = true;
        app.artist_preview_focus = "portrait".to_string();
        assert_eq!(super::active_filter_levels(&app)[1], 0);
        assert!(super::change_filter_level(&mut app, "Clarity", -2));
        assert_eq!(app.artist_portrait_filter_levels[1], -2);
        app.artist_preview_focus = "banner".to_string();
        assert_eq!(super::active_filter_levels(&app)[1], 0);
        assert!(super::change_filter_level(&mut app, "Clarity", 1));
        super::reset_enhancement_preview_state(&mut app);
        assert_eq!(app.artist_banner_filter_levels[1], 0);
        assert_eq!(app.artist_portrait_filter_levels[1], -2);
        assert_eq!(app.album_filter_levels[1], 3);
    }

    #[test]
    fn similar_artist_thumbnail_has_real_transparent_round_corners() {
        let directory = tempfile::TempDir::new().unwrap();
        // Navidrome cache files use the generic .img extension even when the
        // response body is PNG or JPEG, so the decoder must inspect content.
        let source = directory.path().join("similar.img");
        image::RgbaImage::from_pixel(240, 180, image::Rgba([210, 90, 45, 255]))
            .save_with_format(&source, image::ImageFormat::Png)
            .unwrap();
        let thumbnail =
            cached_circular_artist_thumbnail(&source, directory.path(), "artist-id").unwrap();
        let decoded = image::open(thumbnail).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), (160, 160));
        assert_eq!(decoded.get_pixel(0, 0).0[3], 0);
        assert_eq!(decoded.get_pixel(80, 80).0[3], 255);
    }

    #[test]
    fn artist_overview_never_reassigns_an_authoritative_album_mbid_by_title() {
        let bob_seger_release = NavidromeAlbum {
            id: "bob-greatest-hits".to_string(),
            artist_id: "bob-seger".to_string(),
            artist: "Bob Seger & the Silver Bullet Band".to_string(),
            title: "Greatest Hits".to_string(),
            cover_art_id: "bob-cover".to_string(),
            musicbrainz_id: "bob-release-mbid".to_string(),
            year: "1994".to_string(),
            song_count: 14,
            user_rating: 4,
        };
        let suzy_bogguss_release = ThumbnailAlbumTarget {
            album_key: "suzy-greatest-hits".to_string(),
            artist: "Suzy Bogguss".to_string(),
            artist_mbid: String::new(),
            title: "Greatest Hits".to_string(),
            path: String::new(),
            album_mbid: "suzy-release-mbid".to_string(),
            release_year: "1994".to_string(),
            cover_path: String::new(),
        };

        assert!(match_artist_overview_album(&suzy_bogguss_release, [&bob_seger_release]).is_none());

        let mut untagged_suzy = suzy_bogguss_release;
        untagged_suzy.album_mbid.clear();
        assert!(match_artist_overview_album(&untagged_suzy, [&bob_seger_release]).is_none());

        let untagged_bob = ThumbnailAlbumTarget {
            album_key: "bob-greatest-hits".to_string(),
            artist: "Bob Seger & the Silver Bullet Band".to_string(),
            artist_mbid: String::new(),
            title: "Greatest Hits".to_string(),
            path: String::new(),
            album_mbid: String::new(),
            release_year: "1994".to_string(),
            cover_path: String::new(),
        };
        assert_eq!(
            match_artist_overview_album(&untagged_bob, [&bob_seger_release]).map(|album| &album.id),
            Some(&bob_seger_release.id)
        );

        let compilation_release = NavidromeAlbum {
            id: "billboard-1975".to_string(),
            artist: "Various Artists".to_string(),
            title: "Billboard Hot 100 Singles of 1975".to_string(),
            year: "1975".to_string(),
            ..NavidromeAlbum::default()
        };
        let local_compilation = ThumbnailAlbumTarget {
            album_key: "fallback-1975".to_string(),
            artist: "[Various Artists]".to_string(),
            artist_mbid: String::new(),
            title: "Billboard Hot 100 Singles of 1975".to_string(),
            path: String::new(),
            album_mbid: String::new(),
            release_year: "1975".to_string(),
            cover_path: String::new(),
        };
        assert_eq!(
            match_artist_overview_album(&local_compilation, [&compilation_release])
                .map(|album| album.id.as_str()),
            Some("billboard-1975")
        );
    }

    #[test]
    fn spl_file_association_and_explicit_restore_resolve_the_backup_request() {
        let backup = OsString::from(r"C:\Backups\library.spl");
        assert_eq!(
            restore_request_from_args([backup.clone()]),
            Some(PathBuf::from(&backup))
        );
        assert_eq!(
            restore_request_from_args([OsString::from("--restore"), backup.clone()]),
            Some(PathBuf::from(&backup))
        );
        assert_eq!(
            restore_request_from_args([OsString::from("notes.txt")]),
            None
        );
    }

    #[test]
    fn portable_backup_schedule_is_exclusive_and_uses_timestamped_files() {
        assert_eq!(normalize_backup_schedule("daily"), "Daily");
        assert_eq!(normalize_backup_schedule("WEEKLY"), "Weekly");
        assert_eq!(normalize_backup_schedule("unsupported"), "Off");
        assert!(backup_schedule_interval("Monthly").is_some());
        assert!(backup_schedule_interval("Off").is_none());

        let directory = tempfile::TempDir::new().unwrap();
        assert!(automatic_backup_is_due(
            directory.path(),
            "Daily",
            "splined-database-auto",
            "sqlite"
        ));
        let path =
            backup_file_in_directory(directory.path(), "splined-database-auto", "sqlite").unwrap();
        std::fs::write(&path, b"fixture").unwrap();
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(&format!(
                    "{}-",
                    super::machine_backup_stem("splined-database-auto")
                ))
        );
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("sqlite")
        );
        assert!(!automatic_backup_is_due(
            directory.path(),
            "Daily",
            "splined-database-auto",
            "sqlite"
        ));
        assert!(automatic_backup_is_due(
            directory.path(),
            "Daily",
            "splined-settings-auto",
            "spl"
        ));
        assert!(is_sqlite_backup_path(
            PathBuf::from("snapshot.sqlite").as_path()
        ));
        assert!(!is_sqlite_backup_path(
            PathBuf::from("settings.spl").as_path()
        ));
    }

    #[test]
    fn processing_override_choices_round_trip_without_changing_zero_semantics() {
        for (label, hours) in [
            ("Always Override", 0.0),
            ("1 Hour Override", 1.0),
            ("3 Hour Override", 3.0),
            ("6 Hour Override", 6.0),
            ("12 Hour Override", 12.0),
            ("1 Day Override", 24.0),
            ("1 Week Override", 168.0),
            ("1 Month Override", 720.0),
        ] {
            assert_eq!(super::override_delay_hours(label).unwrap(), hours);
            assert_eq!(super::override_delay_label(hours), label);
        }
        assert!(super::override_delay_hours("Unlimited").is_err());
    }

    #[test]
    fn diagnostic_choices_separate_current_verbose_debugger_and_error() {
        use splined_core::config::Verbosity;
        assert_eq!(
            super::diagnostic_level_verbosity("Current").unwrap(),
            Verbosity::Info
        );
        assert_eq!(
            super::diagnostic_level_verbosity("Verbose").unwrap(),
            Verbosity::Debug
        );
        assert_eq!(
            super::diagnostic_level_verbosity("Debugger").unwrap(),
            Verbosity::Trace
        );
        assert_eq!(
            super::diagnostic_level_verbosity("Error").unwrap(),
            Verbosity::Error
        );
        assert_eq!(super::diagnostic_level_label(Verbosity::Warn), "Error");
    }

    #[test]
    fn backup_file_limits_prune_only_the_selected_timestamped_type() {
        let directory = tempfile::TempDir::new().unwrap();
        for name in [
            "splined-MEDIAPC-settings-1.spl",
            "splined-MEDIAPC-settings-2.spl",
            "splined-MEDIAPC-settings-3.spl",
            "splined-PSYCOTIX-MINI-settings-1.spl",
            "splined-settings-legacy.spl",
            "splined-MEDIAPC-database-1.sqlite",
            "other-settings-1.spl",
        ] {
            std::fs::write(directory.path().join(name), b"fixture").unwrap();
        }
        assert_eq!(
            super::prune_timestamped_backups(
                directory.path(),
                &super::backup_stem_for_machine("splined-settings-", "MEDIAPC"),
                "spl",
                1,
            )
            .unwrap(),
            2
        );
        assert_eq!(super::archived_backup_files(directory.path()).len(), 4);
        assert!(
            directory
                .path()
                .join("splined-PSYCOTIX-MINI-settings-1.spl")
                .exists()
        );
        assert!(
            directory
                .path()
                .join("splined-settings-legacy.spl")
                .exists()
        );
        assert!(
            directory
                .path()
                .join("splined-MEDIAPC-database-1.sqlite")
                .exists()
        );
        assert!(directory.path().join("other-settings-1.spl").exists());
        assert_eq!(super::backup_file_limit("0"), None);
        assert_eq!(super::backup_file_limit("invalid"), None);
    }

    #[test]
    fn backup_stem_places_safe_machine_name_before_backup_type() {
        assert_eq!(super::safe_machine_name("PSYCOTIX-MINI"), "PSYCOTIX-MINI");
        assert_eq!(super::safe_machine_name("MEDIA/PC"), "MEDIA-PC");
        assert_eq!(super::safe_machine_name("  "), "UNKNOWN");
        assert_eq!(
            super::backup_stem_for_machine("splined-settings-auto", "PSYCOTIX-MINI"),
            "splined-PSYCOTIX-MINI-settings-auto"
        );
        assert_eq!(
            super::backup_stem_for_machine("splined-database", "MEDIA/PC"),
            "splined-MEDIA-PC-database"
        );
    }

    #[test]
    fn ui_debug_logs_start_with_machine_and_retention_stays_machine_scoped() {
        let filename = super::ui_debug_log_filename("MEDIA/PC", 123, 45);
        assert_eq!(filename, "[MEDIA-PC]-Windows-ui-123-45.debug.log");
        assert!(super::is_machine_ui_debug_log_filename(
            &filename, "MEDIA/PC"
        ));
        assert!(super::is_machine_ui_debug_log_filename(
            "windows-ui-[MEDIA-PC]-123-45.debug.log",
            "MEDIA/PC"
        ));
        assert!(!super::is_machine_ui_debug_log_filename(
            "[OTHER-PC]-Windows-ui-123-45.debug.log",
            "MEDIA/PC"
        ));
        assert!(!super::is_machine_ui_debug_log_filename(
            "windows-ui-123-45.debug.log",
            "MEDIA/PC"
        ));
    }

    #[test]
    fn structured_list_fields_accept_plain_and_pasted_array_values() {
        assert_eq!(
            split_values(r#""[Octo-Fiesta]", "[Artist Singles]", [videos]", _noinfo"#),
            vec!["[Octo-Fiesta]", "[Artist Singles]", "[videos]", "_noinfo"]
        );
        assert_eq!(
            split_values(r#"["deezer", "itunes", "navidrome"]"#),
            vec!["deezer", "itunes", "navidrome"]
        );
    }

    #[test]
    fn musicbrainz_page_links_require_release_uuid_shape() {
        assert!(is_valid_mbid("4f725973-aaf1-4d0d-a775-0a90ed2a0757"));
        assert!(is_valid_mbid("4F725973-AAF1-4D0D-A775-0A90ED2A0757"));
        assert!(!is_valid_mbid("not-a-release-id"));
        assert!(!is_valid_mbid("4f725973-aaf1-4d0d-a775-0a90ed2a075z"));
    }

    #[test]
    fn mb_failed_opens_validated_focused_artist_or_focused_name_search() {
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut state = ApplicationState::new(Config::default(), snapshot).unwrap();
        state.library.focused_artist = Some("2 Chainz".to_string());
        state.musicbrainz_focused_artist_mbid = "dff0d392-4cd5-4052-9fbb-f485df3891e5".to_string();
        state.musicbrainz_artist_status = "valid".to_string();
        assert_eq!(
            musicbrainz_failed_url(&state),
            "https://musicbrainz.org/artist/dff0d392-4cd5-4052-9fbb-f485df3891e5"
        );
        state.musicbrainz_artist_status = "invalid".to_string();
        assert!(musicbrainz_failed_url(&state).contains("query=2%20Chainz"));
    }

    #[test]
    fn workspace_geometry_uses_logical_dimensions_and_per_monitor_v2() {
        assert_eq!(logical_dimension(1440.4, 960), 1440);
        assert_eq!(logical_dimension(720.0, 960), 960);
        assert_eq!(logical_dimension(f32::NAN, 960), 960);

        let manifest = include_str!("../gui/app.manifest");
        assert!(manifest.contains(">PerMonitorV2</dpiAwareness>"));
        assert!(manifest.contains(">true/pm</dpiAware>"));
    }

    #[test]
    fn maximized_guard_preserves_maximized_state_during_layout_changes() {
        let guard = MaximizeGuard::new(false);
        assert!(guard.arm(true, 3440, 1392));
        assert!(guard.maintain(true, 3440, 765));
        assert!(guard.maintain(true, 3440, 1392));
    }

    #[test]
    fn config_mode_remains_authoritative_over_legacy_ui_preferences() {
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut config = Config {
            mode: Mode::Write,
            ..Config::default()
        };
        let mut state = ApplicationState::new(config.clone(), snapshot.clone()).unwrap();
        let read_preference = UiPreferences {
            launch_mode: "read".to_string(),
            ..UiPreferences::default()
        };
        apply_ui_preferences(&mut state, &read_preference);
        assert_eq!(state.launch_mode, LaunchMode::LiveWrite);

        config.mode = Mode::Read;
        let mut state = ApplicationState::new(config, snapshot).unwrap();
        let write_preference = UiPreferences {
            launch_mode: "write".to_string(),
            ..UiPreferences::default()
        };
        apply_ui_preferences(&mut state, &write_preference);
        assert_eq!(state.launch_mode, LaunchMode::Read);
    }

    #[test]
    fn restored_interface_rehydrates_locked_filters_palette_and_pane_sizes() {
        let snapshot = MediaSnapshot {
            schema_version: splined_core::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: Vec::new(),
        };
        let mut state = ApplicationState::new(Config::default(), snapshot).unwrap();
        let mut preferences = UiPreferences {
            appearance: "Chalk".to_string(),
            media_filter_locked: true,
            candidate_filter_locked: true,
            banner_sync_frame_letters: true,
            banner_random_harmony: true,
            wheel_hue_shift_degrees: 42,
            wheel_brightness_percent: 130,
            candidate_excluded_types: vec!["BackCover".to_string()],
            candidate_included_sources: vec!["Navidrome".to_string()],
            media_selector_expanded: false,
            media_pane_width: 390,
            media_pane_expanded_width: 390,
            artwork_pane_width: 510,
            artwork_pane_expanded_width: 510,
            ..UiPreferences::default()
        };
        preferences.banner_theme_baseline_styles = preferences.banner_theme_styles.clone();
        preferences.album_frame_theme_baseline_styles =
            preferences.album_frame_theme_styles.clone();
        preferences.banner_theme_styles[0].background = "#123ABC".to_string();
        preferences.album_frame_theme_styles[0].color = "#20BFA0".to_string();
        preferences.adjustment_presets[1].name = "Bright".to_string();
        preferences.adjustment_presets[1].values = [1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0];
        preferences.adjustment_presets[1].saved = true;

        apply_ui_preferences(&mut state, &preferences);
        let layout = WorkspaceLayout::from_preferences(&preferences);
        assert!(state.media_filter_locked);
        assert!(state.candidate_filter_locked);
        assert!(!state.media_selector_expanded);
        assert!(state.banner_sync_frame_letters);
        assert!(state.banner_random_harmony);
        assert_eq!(state.wheel_hue_shift_degrees, 42);
        assert_eq!(state.wheel_brightness_percent, 130);
        assert!(state.candidate_excluded_types.contains("BackCover"));
        assert!(state.candidate_included_sources.contains("Navidrome"));
        assert_eq!(state.appearance_mode, "Chalk");
        assert_eq!(state.adjustment_presets[1].name, "Bright");
        assert_eq!(state.banner_theme_styles[0].background, "#123ABC");
        assert_eq!(state.banner_theme_baseline_styles[0].background, "#3478FF");
        assert_eq!(state.album_frame_theme_styles[0].color, "#20BFA0");
        assert_eq!(layout.media_pane_width, 390);
        assert_eq!(layout.artwork_pane_width, 510);

        preferences.banner_theme_baseline_styles.clear();
        preferences.album_frame_theme_baseline_styles.clear();
        apply_ui_preferences(&mut state, &preferences);
        assert_eq!(state.banner_theme_baseline_styles[0].background, "#123ABC");
        assert_eq!(state.album_frame_theme_baseline_styles[0].color, "#20BFA0");

        preferences.candidate_filter_locked = false;
        apply_ui_preferences(&mut state, &preferences);
        assert!(state.candidate_excluded_types.is_empty());
    }

    #[test]
    fn banner_color_sync_links_frame_and_letters_only_when_enabled() {
        let mut style = splined_core::settings::default_banner_theme_styles().remove(0);
        let original_background = style.background.clone();
        assert!(super::set_banner_theme_color(
            &mut style,
            "frame",
            "#D6A42A".to_string(),
            true,
        ));
        assert_eq!(style.background, original_background);
        assert_eq!(style.frame, "#D6A42A");
        assert_eq!(style.letters, "#D6A42A");
        assert!(super::set_banner_theme_color(
            &mut style,
            "background",
            "#C38412".to_string(),
            true,
        ));
        assert_eq!(style.background, "#C38412");
        assert_eq!(style.frame, "#C38412");
        assert_eq!(style.letters, "#C38412");
        assert!(super::set_banner_theme_color(
            &mut style,
            "letters",
            "#3178B1".to_string(),
            false,
        ));
        assert_eq!(style.frame, "#C38412");
        assert_eq!(style.letters, "#3178B1");
    }

    #[test]
    fn wheel_has_warm_gold_and_rejects_outside_clicks() {
        let _base = super::theme_wheel_image(0, 100);
        let _cached = super::theme_wheel_image(0, 100);
        let _shifted = super::theme_wheel_image(180, 70);
        let gold = super::wheel_rgb(0.19, 0.39, 0.0, "#000000", 0, 100);
        let [red, green, blue] = super::rgb_from_hex(&gold);
        assert!(red > 220 && green > 110 && blue < 60, "{gold}");
        assert!(!super::wheel_contains(0.02, 0.02));
        assert_eq!(
            super::wheel_rgb(0.02, 0.02, 0.0, "#123ABC", 0, 100),
            "#123ABC"
        );
        let dark = super::wheel_rgb(0.19, 0.39, 0.0, "#000000", 0, 50);
        assert!(super::rgb_from_hex(&dark)[0] < red);
        let rotated = super::wheel_rgb(0.19, 0.39, 0.0, "#000000", 180, 100);
        assert_ne!(rotated, gold);
    }

    #[test]
    fn random_banner_harmony_is_coordinated_and_reproducible() {
        let first = super::banner_harmony("#D8A321", 42, false, false);
        let second = super::banner_harmony("#D8A321", 42, false, false);
        assert_eq!(first, second);
        assert_ne!(first.0, first.1);
        assert_ne!(first.1, first.2);
        let synced = super::banner_harmony("#D8A321", 43, false, true);
        assert_eq!(synced.1, synced.2);
    }
}

#[derive(Clone)]
struct CachedImage {
    identity: (u64, Option<std::time::SystemTime>),
    image: slint::Image,
}

#[derive(Clone)]
struct CachedPixels {
    identity: (u64, Option<std::time::SystemTime>),
    pixels: Arc<[u8]>,
    width: u32,
    height: u32,
    source_width: u32,
    source_height: u32,
}

thread_local! {
    static IMAGE_CACHE: RefCell<HashMap<String, CachedImage>> = RefCell::new(HashMap::new());
}

static RAW_IMAGE_CACHE: OnceLock<Mutex<HashMap<String, CachedPixels>>> = OnceLock::new();
// Keep the exact full-resolution edit result, not only its 1600px UI bitmap.
// Staging an album freezes these bytes into the durable embed queue.
static EDIT_PREVIEW_BYTES: OnceLock<Mutex<Option<(String, Vec<u8>)>>> = OnceLock::new();
static PREVIEW_IMAGE_LOADS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static ACTIVE_PREVIEW_CACHE_KEYS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn raw_image_cache() -> &'static Mutex<HashMap<String, CachedPixels>> {
    RAW_IMAGE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn store_edit_preview_bytes(key: &str, bytes: &[u8]) {
    if let Ok(mut cached) = EDIT_PREVIEW_BYTES.get_or_init(|| Mutex::new(None)).lock() {
        *cached = Some((key.to_string(), bytes.to_vec()));
    }
}

fn full_edit_preview_bytes(key: &str) -> Option<Vec<u8>> {
    EDIT_PREVIEW_BYTES
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|cached| {
            cached
                .as_ref()
                .filter(|(cached_key, _)| cached_key == key)
                .map(|(_, bytes)| bytes.clone())
        })
}

fn has_edit_preview_bytes(key: &str) -> bool {
    EDIT_PREVIEW_BYTES
        .get_or_init(|| Mutex::new(None))
        .lock()
        .is_ok_and(|cached| {
            cached
                .as_ref()
                .is_some_and(|(cached_key, _)| cached_key == key)
        })
}

fn preview_image_loads() -> &'static Mutex<HashSet<String>> {
    PREVIEW_IMAGE_LOADS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn active_preview_cache_keys() -> &'static Mutex<HashSet<String>> {
    ACTIVE_PREVIEW_CACHE_KEYS.get_or_init(|| Mutex::new(HashSet::new()))
}
