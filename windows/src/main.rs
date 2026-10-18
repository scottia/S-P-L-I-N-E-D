#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use futures_util::{StreamExt, stream};
use slint::{
    Color, ComponentHandle, Model, ModelRc, ModelTracker, SharedString, Timer, TimerMode, VecModel,
};
use splined_core::application::{
    ApplicationState, ArtistOverviewCacheEntry, CandidateState, LaunchMode, LibraryRow,
    LibraryView, MusicBrainzResult, RunPhase, ScanScope, SelectionResult, StatusFilter,
};
use splined_core::backup::{
    BackupSelection, export_backup, export_database_backup, export_diagnostic_summary,
    restore_backup, restore_database_backup,
};
use splined_core::candidate::{Candidate, StaticFormat};
use splined_core::config::{Config, Mode, ScanTimeout, Verbosity, configured_samples_dir};
use splined_core::credential_store::{CredentialEditor, load_editor, save_editor, test_editor};
use splined_core::download::DownloadCache;
use splined_core::final_artwork::{
    install_prepared_artwork, prepare_existing_cover_edit, prepare_selected_artwork_edit,
};
use splined_core::inspect::inspect_image;
use splined_core::local_artwork::embedded_candidate;
use splined_core::media_database::{
    MediaSnapshot, MediaTrackSnapshot, RuntimeArtworkMaterial, clear_album_bypass_from_runtime,
    fallback_tracks_from_folder, media_snapshot, record_album_outcome_from_runtime,
};
use splined_core::range::Range;
use splined_core::safe_write::{replace_binary_file, take_credential_protection_warning};
use splined_core::scan_controller::{ProcessingDebugSink, ScanController};
use splined_core::settings::{
    MEDIA_PANE_MIN, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, UiPreferences, WorkspaceBounds,
    WorkspaceLayout, normalize_appearance,
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
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

slint::include_modules!();

static FULL_THUMBNAIL_REFRESH_ACTIVE: AtomicBool = AtomicBool::new(false);
static FOCUSED_THUMBNAIL_REFRESH_REVISION: AtomicU64 = AtomicU64::new(0);
static PREVIEW_RENDER_REVISION: AtomicU64 = AtomicU64::new(0);
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
    compilation_track_preview: Option<(String, String)>,
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
        compilation_track_preview: app.active_compilation_track_preview().map(|path| {
            (
                app.compilation_track_preview_owner.clone(),
                path.to_string(),
            )
        }),
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
    let path = run_dir.join(format!(
        "windows-ui-{timestamp}-{}.debug.log",
        std::process::id()
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
        "ui-debug-start version={} pid={} log_dir={}",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        config.scan.log_dir
    ));
    let _ = UI_DEBUG_LOG_HANDLE.set(log.clone());
    Some(log)
}

#[derive(Clone)]
struct ThumbnailAlbumTarget {
    album_key: String,
    artist: String,
    title: String,
    path: String,
    album_mbid: String,
    release_year: String,
    cover_path: String,
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
    similar_artists: Vec<(String, String)>,
    genres: Vec<String>,
    roles: Vec<String>,
    album_keys: BTreeSet<String>,
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
            selected: row.selected,
            eligible: row.eligible,
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
                app.library
                    .replace_fallback_tracks(album_key, tracks.clone());
            }
            let cached = ArtistOverviewCacheEntry {
                biography: metadata.biography,
                external_url: metadata.external_url,
                musicbrainz_id: metadata.musicbrainz_id,
                banner_path: metadata.banner_path,
                similar_artists: metadata.similar_artists,
                genres: metadata.genres,
                roles: metadata.roles,
                album_keys: metadata.album_keys,
                track_paths_by_album: metadata.track_paths_by_album,
                album_ratings: metadata.album_ratings,
                artwork_candidates: metadata.artwork_candidates,
                top_songs: Vec::new(),
            };
            app.artist_overview_biography = cached.biography.clone();
            app.artist_overview_external_url = cached.external_url.clone();
            app.artist_overview_musicbrainz_id = cached.musicbrainz_id.clone();
            app.artist_overview_banner_path = cached.banner_path.clone();
            app.artist_overview_similar_artists = cached.similar_artists.clone();
            app.artist_overview_genres = cached.genres.clone();
            app.artist_overview_roles = cached.roles.clone();
            app.artist_overview_album_keys = cached.album_keys.clone();
            app.artist_overview_track_paths = cached.track_paths_by_album.clone();
            app.artist_overview_album_ratings = cached.album_ratings.clone();
            app.artist_overview_artwork_candidates = cached.artwork_candidates.clone();
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
    Some(UiPreferences {
        workspace_layout_revision: splined_core::settings::CURRENT_WORKSPACE_LAYOUT_REVISION,
        appearance: appearance.borrow().clone(),
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
        artwork_editing_expanded: state.artwork_editing_expanded,
        show_artwork: state.show_artwork,
        hover_enabled: state.hover_enabled,
        // Preview/apply is transient workflow state, never a persisted setting.
        upscale_preview_only: true,
        disable_intro_message: state.disable_intro_message,
        disable_confirmation_messages: state.disable_confirmation_messages,
        backup_directory: ui.get_backup_directory().to_string(),
        restore_path: ui.get_restore_path().to_string(),
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
        backup_interface: ui.get_backup_interface(),
        backup_diagnostics: ui.get_backup_diagnostics(),
        artist_filter: state.library.artist_filter.clone(),
        album_filter: state.library.album_filter.clone(),
        // Candidate filters are per-run review controls. A fresh candidate set
        // re-enables every available category, so stale exclusions must never
        // survive an application restart.
        candidate_included_sources: Vec::new(),
        candidate_included_types: Vec::new(),
        candidate_included_policies: Vec::new(),
        candidate_included_ranges: Vec::new(),
        candidate_excluded_sources: Vec::new(),
        candidate_excluded_types: Vec::new(),
        candidate_excluded_policies: Vec::new(),
        candidate_excluded_ranges: Vec::new(),
        primary_adjustments: state.primary_adjustments.clone(),
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
    let artists = cell
        .get_or_try_init(|| async { companion.artists().await.map(Arc::new) })
        .await?;
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
    let albums = cell
        .get_or_try_init(|| async { companion.albums().await.map(Arc::new) })
        .await?;
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
    let Some((config, albums, needs_refresh)) = state.lock().ok().map(|mut app| {
        let albums = app
            .library
            .albums
            .iter()
            .map(|album| ThumbnailAlbumTarget {
                album_key: album.snapshot.album_key.clone(),
                artist: album.snapshot.artist.clone(),
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
        (app.config.clone(), albums, needs_refresh)
    }) else {
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
                        |value| value == &normalized,
                    )
                }) {
                    artist_ids.push((artist.name.clone(), artist.id.clone()));
                    let current_artist_art = companion
                        .cache_artist_art(artist, &artist_cache, 192)
                        .await
                        .ok()
                        .flatten();
                    let library_artist_art = local_artist_art_for_albums(&albums, &artist.name)
                        .or_else(|| {
                            current_artist_art.as_deref().and_then(|source| {
                                cached_circular_artist_thumbnail(source, &artist_cache, &artist.id)
                                    .ok()
                            })
                        })
                        .or_else(|| current_artist_art.clone());
                    if let Some(path) = library_artist_art {
                        artist_paths.push((artist.name.clone(), path));
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
                        let raw_banner = match companion
                            .cache_artist_info_art(&info, &artist_banner_cache, 1500)
                            .await
                            .ok()
                            .flatten()
                        {
                            Some(path) => Some(path),
                            None => companion
                                .cache_artist_art(artist, &artist_banner_cache, 1500)
                                .await
                                .ok()
                                .flatten(),
                        };
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
                                if local.album_mbid.trim().is_empty()
                                    && let Some(paths) = artist_library
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
                                        track_paths_by_album
                                            .insert(key, local_paths.into_iter().collect());
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
                                "Navidrome metadata".to_string(),
                                path.to_string_lossy().into_owned(),
                            ));
                        }
                        artwork_candidates
                            .extend(local_artist_art_candidates(&albums, &artist.name));
                        let artist_key = artist.name.trim().to_ascii_lowercase();
                        artist_metadata = Some(ArtistOverviewMetadata {
                            artist_key,
                            biography: info.biography,
                            external_url: info.external_url,
                            musicbrainz_id: if info.musicbrainz_id.trim().is_empty() {
                                artist.musicbrainz_id.clone()
                            } else {
                                info.musicbrainz_id
                            },
                            banner_path,
                            similar_artists,
                            genres: artist_library.genres,
                            roles: artist_library.roles,
                            album_keys,
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
                Ok((artist_paths, album_paths, artist_ids, artist_metadata))
            })
        })();
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
    let Some((config, artist_names, artist_directories, album_targets, hydration_request_keys)) =
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
                        title: album.snapshot.title.clone(),
                        path: album.snapshot.path.clone(),
                        album_mbid: album.snapshot.album_mbid.clone(),
                        release_year: album.snapshot.release_year.clone(),
                        cover_path: album.snapshot.cover_path.clone(),
                    });
                }
            }
            let artist_directories = artist_names
                .iter()
                .filter_map(|artist| {
                    focused_artist_directory(&app, artist)
                        .ok()
                        .map(|directory| (artist.clone(), directory))
                })
                .collect::<Vec<_>>();
            (
                app.config.clone(),
                artist_names,
                artist_directories,
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
        let result =
            (|| -> Result<(ThumbnailPaths, ThumbnailPaths, ArtistNavidromeIds), String> {
                let mut artist_paths = artist_directories
                    .iter()
                    .filter_map(|(artist, directory)| {
                        local_artist_art_file(directory).map(|path| (artist.clone(), path))
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
    let name = name.to_ascii_lowercase();
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

fn local_artist_art_for_albums(albums: &[ThumbnailAlbumTarget], artist: &str) -> Option<PathBuf> {
    let mut directories = HashMap::<PathBuf, usize>::new();
    for album in albums
        .iter()
        .filter(|album| album.artist.trim().eq_ignore_ascii_case(artist.trim()))
    {
        if let Some(parent) = Path::new(&album.path).parent() {
            *directories.entry(parent.to_path_buf()).or_default() += 1;
        }
    }
    directories
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .and_then(|(directory, _)| local_artist_art_file(&directory))
}

fn local_artist_art_candidates(
    albums: &[ThumbnailAlbumTarget],
    artist: &str,
) -> Vec<(String, String)> {
    let mut directories = BTreeSet::new();
    for album in albums
        .iter()
        .filter(|album| album.artist.trim().eq_ignore_ascii_case(artist.trim()))
    {
        if let Some(parent) = Path::new(&album.path).parent() {
            directories.insert(parent.to_path_buf());
        }
    }
    let mut result = Vec::new();
    for directory in directories {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
            let is_artist_art = path
                .file_stem()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("artist"));
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
    const SIZE: u32 = 1024;
    let decoded = image::load_from_memory(source)
        .map_err(|error| format!("Unable to decode selected Artist Art: {error}"))?;
    let mut thumbnail = decoded
        .resize_to_fill(SIZE, SIZE, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
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
    bytes: &[u8],
    preserve_file: bool,
) -> Result<(PathBuf, bool), String> {
    let canonical = directory.join("artist.png");
    if !preserve_file {
        return Ok((canonical, false));
    }
    for number in 1..=1_000_000usize {
        let candidate = if number == 1 {
            canonical.clone()
        } else {
            directory.join(format!("artist-({number}).png"))
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

fn remove_superseded_artist_art(directory: &Path, keep: &Path) -> Vec<String> {
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
        if path == keep || !is_artwork_stem(&path, "artist") {
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
    let bytes = circular_artist_art_png(source)?;
    let (target, unchanged) = artist_art_destination(directory, &bytes, preserve_file)?;
    if !unchanged {
        replace_binary_file(&target, &bytes, "artist artwork", |staged| {
            inspect_image(staged).map(|_| ())
        })?;
    }
    let warnings = if preserve_file {
        Vec::new()
    } else {
        remove_superseded_artist_art(directory, &target)
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
    let source_identity = source
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("artist");
    let path = cache_directory.join(format!(
        "banner-v2-{}-{}.png",
        sanitize_sample_component(artist_id),
        sanitize_sample_component(source_identity)
    ));
    if path.is_file() && inspect_image(&path).is_ok() {
        return Ok(path);
    }
    let decoded = image::ImageReader::open(source)
        .map_err(|error| {
            format!(
                "Unable to open Navidrome artist artwork {}: {error}",
                source.display()
            )
        })?
        .with_guessed_format()
        .map_err(|error| {
            format!(
                "Unable to identify Navidrome artist artwork {}: {error}",
                source.display()
            )
        })?
        .decode()
        .map_err(|error| {
            format!(
                "Unable to decode Navidrome artist artwork {}: {error}",
                source.display()
            )
        })?;
    let mut banner = decoded
        .resize_to_fill(1400, 360, image::imageops::FilterType::Triangle)
        .to_rgba8();
    let blurred = image::DynamicImage::ImageRgba8(banner.clone())
        .blur(18.0)
        .to_rgba8();
    let blur_start = 250_u32;
    let blur_span = (banner.height() - blur_start).max(1);
    for y in blur_start..banner.height() {
        let blend = ((y - blur_start) as f32 / blur_span as f32 * 0.82).clamp(0.0, 0.82);
        for x in 0..banner.width() {
            let original = banner.get_pixel(x, y).0;
            let softened = blurred.get_pixel(x, y).0;
            let mixed = std::array::from_fn(|index| {
                if index == 3 {
                    original[index]
                } else {
                    ((original[index] as f32 * (1.0 - blend)) + (softened[index] as f32 * blend))
                        .round() as u8
                }
            });
            banner.put_pixel(x, y, image::Rgba(mixed));
        }
    }
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
    let association_warning =
        splined_core::file_association::register_portable_backup_association().err();
    let (mut state, setup_required, startup_error) = load_application_state();
    let ui_debug_log = initialize_ui_debug_log(&state.config);
    if let Some(log) = ui_debug_log.as_ref() {
        state.push_log(format!("Windows UI debug log: {}", log.display_path()));
    }
    let mut preferences = splined_core::settings::load_ui_preferences().unwrap_or_default();
    preferences.migrate_workspace_layout();
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
    let ui = AppWindow::new()?;
    ui.window().set_maximized(true);
    ui.set_setup_required(setup_required);
    if setup_required {
        ui.set_page_index(3);
        ui.set_setup_message(SharedString::from(startup_error.clone().unwrap_or_else(
            || "Config v5 was not found. Complete SPLINED Setup.".to_string(),
        )));
    }
    ui.set_include_bypassed(preferences.include_bypassed);
    ui.set_media_filter_expanded(preferences.media_filter_expanded);
    ui.set_musicbrainz_id_editor_expanded(preferences.musicbrainz_id_editor_expanded);
    ui.set_selected_source_name(SharedString::from(preferences.selected_source_name.clone()));
    let default_backup_directory = splined_core::portable::app_layout()
        .map(|layout| layout.root.join("backups").to_string_lossy().into_owned())
        .unwrap_or_else(|_| "backups".to_string());
    ui.set_backup_directory(SharedString::from(
        if preferences.backup_directory.trim().is_empty() {
            default_backup_directory
        } else {
            preferences.backup_directory.clone()
        },
    ));
    ui.set_restore_path(SharedString::from(preferences.restore_path.clone()));
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
    ui.set_backup_interface(preferences.backup_interface);
    ui.set_backup_diagnostics(preferences.backup_diagnostics);
    if preferences.window_width >= 960 && preferences.window_height >= 640 {
        ui.window().set_size(slint::LogicalSize::new(
            preferences.window_width as f32,
            preferences.window_height as f32,
        ));
    }
    ui.set_appearance_mode(SharedString::from(appearance.borrow().clone()));
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
    {
        ui.set_page_index(2);
        ui.set_backup_mode(SharedString::from("Restore"));
        ui.set_restore_path(SharedString::from(path.to_string_lossy().into_owned()));
        ui.set_backup_settings(true);
        ui.set_backup_credentials(false);
        ui.set_backup_database(false);
        ui.set_backup_interface(false);
        ui.set_backup_diagnostics(false);
        if let Ok(mut app) = state.lock() {
            app.service_status = format!(
                "Automatic Config v5 import needs attention: {error} Enter the backup password if required, then choose Restore."
            );
        }
    }
    sync_ui(&ui, &state);
    bind_callbacks(
        &ui,
        Arc::clone(&state),
        Arc::clone(&controller),
        Rc::clone(&workspace_layout),
        Rc::clone(&appearance),
        UiWindowDiagnostics {
            maximize_guard: main_window_guard.clone(),
            ui_debug_log: ui_debug_log.clone(),
        },
    );
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
            let (artwork_visible, candidate_filter_expanded) = startup_reflow_state
                .lock()
                .map(|mut app| {
                    app.media_selector_expanded = true;
                    (app.show_artwork, app.candidate_filter_expanded)
                })
                .unwrap_or((true, true));
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
                    true,
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
    let notice_timer = Timer::default();
    notice_timer.start(TimerMode::Repeated, Duration::from_millis(75), move || {
        let mut changed = false;
        while notices.try_recv().is_ok() {
            changed = true;
        }
        if changed && let Some(ui) = weak.upgrade() {
            sync_ui(&ui, &timer_state);
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
    preference_timer.start(TimerMode::Repeated, Duration::from_millis(600), move || {
        let Some(ui) = preference_weak.upgrade() else {
            return;
        };
        if ui.get_setup_required() {
            return;
        }
        let Some(current) = capture_ui_preferences(
            &ui,
            &preference_state,
            &preference_layout,
            &preference_appearance,
            &preference_custom_window,
        ) else {
            return;
        };
        if current != *timer_last_preferences.borrow()
            && splined_core::settings::save_ui_preferences(&current).is_ok()
        {
            *timer_last_preferences.borrow_mut() = current;
        }
    });

    let scheduled_backup_timer = Timer::default();
    let scheduled_backup_weak = ui.as_weak();
    let scheduled_backup_state = Arc::clone(&state);
    let scheduled_backup_active = Arc::new(AtomicBool::new(false));
    let scheduled_backup_guard = Arc::clone(&scheduled_backup_active);
    scheduled_backup_timer.start(TimerMode::Repeated, Duration::from_secs(60), move || {
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
        let diagnostics_schedule =
            normalize_backup_schedule(ui.get_diagnostics_backup_schedule().as_str());
        let settings_due = automatic_backup_is_due(
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
                    .and_then(|path| export_database_backup(&path, &config))
                {
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

    let result = ui.run();
    let (window_width, window_height) = app_logical_window_size(&ui);
    workspace_layout.borrow_mut().record_normal_window(
        window_width,
        window_height,
        ui.window().is_maximized(),
    );
    if !ui.get_setup_required()
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

fn apply_ui_preferences(state: &mut ApplicationState, preferences: &UiPreferences) {
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
    // Always launch with the library visible. Restoring a collapsed rail while
    // Windows was still applying per-monitor DPI produced a misleading
    // half-screen workspace that only recovered after extra clicks.
    state.media_selector_expanded = true;
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
    state.set_primary_adjustments(&preferences.primary_adjustments);
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
        match embedded_candidate(Path::new(&track_path), Path::new(&cache_dir)) {
            Ok(Some(candidate)) => {
                if let Ok(mut app) = state.lock() {
                    app.set_compilation_track_preview(&track_path, candidate.path());
                    app.push_log(format!("Compilation track selected: {track_path}"));
                }
            }
            Ok(None) => {
                if let Ok(mut app) = state.lock() {
                    app.push_log(format!(
                        "Compilation track has no embedded artwork: {track_path}"
                    ));
                }
            }
            Err(error) => record_action_result(state, Err(error)),
        }
    }
}

type FallbackTrackHydration = (String, String, Vec<MediaTrackSnapshot>);
type ArtistArtworkCandidates = Vec<(String, String)>;

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
                    Ok(found) => references.extend(found.into_iter().take(12)),
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
    queried_sources: &[String],
    failed_sources: &[String],
    artist_mbid: &str,
) -> Result<(), String> {
    let artist = app
        .library
        .focused_artist
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Select an artist before finding Artist Art.".to_string())?;
    let range = Range {
        min: app.config.range.min,
        ideal: app.config.range.ideal,
        max: app.config.range.max,
        ladder: app.config.range.ladder,
    };
    app.candidates.clear();
    app.artist_artwork_mode = false;
    app.selected_candidate = None;
    app.comparison_candidates.clear();
    app.artist_artwork_draft_path.clear();
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
            image_type: "ArtistImage".to_string(),
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

fn focused_artist_directory(app: &ApplicationState, artist: &str) -> Result<PathBuf, String> {
    let mut directories = HashMap::<String, (PathBuf, usize)>::new();
    for album in app.library.albums.iter().filter(|album| {
        album
            .snapshot
            .artist
            .trim()
            .eq_ignore_ascii_case(artist.trim())
    }) {
        let Some(parent) = Path::new(&album.snapshot.path).parent() else {
            continue;
        };
        let key = parent.to_string_lossy().to_ascii_lowercase();
        let entry = directories.entry(key).or_insert((parent.to_path_buf(), 0));
        entry.1 += 1;
    }
    directories
        .into_values()
        .max_by_key(|(_, count)| *count)
        .map(|(path, _)| path)
        .ok_or_else(|| {
            format!(
                "SPLINED could not resolve a local Artist folder for {artist}; compilation appearances alone do not define a safe artist.* destination."
            )
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

fn select_adjacent_artist_for_overview(
    app: &mut ApplicationState,
    direction: i32,
) -> Option<String> {
    let artist = app.library.adjacent_pending_artist(direction)?;
    if app.library.select_artist_exclusive(&artist, false) == SelectionResult::NotFound {
        return None;
    }

    app.artist_artwork_draft_path.clear();
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
        UiWindowDiagnostics {
            maximize_guard: main_window_guard.clone(),
            ui_debug_log: None,
        },
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

fn automatic_backup_is_due(directory: &Path, schedule: &str, stem: &str, extension: &str) -> bool {
    let Some(interval) = backup_schedule_interval(schedule) else {
        return false;
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return true;
    };
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
    let diagnostics = startup_diagnostics(&config, &snapshot);
    let mut state = ApplicationState::new(config, snapshot)
        .expect("a parsed Config v5 and database snapshot must initialize application state");
    for diagnostic in diagnostics {
        state.push_log(diagnostic);
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

fn bind_callbacks(
    ui: &AppWindow,
    state: Arc<Mutex<ApplicationState>>,
    controller: Arc<ScanController>,
    workspace_layout: Rc<RefCell<WorkspaceLayout>>,
    appearance: Rc<RefCell<String>>,
    diagnostics: UiWindowDiagnostics,
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
        let weak = ui.as_weak();
        ui.on_set_appearance(move |value| {
            let normalized = normalize_appearance(value.as_str());
            *appearance.borrow_mut() = normalized.clone();
            if let Some(ui) = weak.upgrade() {
                ui.set_appearance_mode(SharedString::from(normalized));
            }
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
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_artist_filter_edited(move |value| {
            if let Ok(mut app) = state.lock() {
                app.library.set_artist_filter(value.as_str());
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_album_filter_edited(move |value| {
            if let Ok(mut app) = state.lock() {
                app.library.set_album_filter(value.as_str());
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_status_filter_toggled(move |value| {
            if let Ok(mut app) = state.lock() {
                if value.eq_ignore_ascii_case("Embedded Art") {
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
            activate_compilation_track(
                &state,
                album_key.as_str(),
                track_path.as_str(),
                include_bypassed,
            );
            let controller_active = controller.is_running();
            if controller_active
                && let Ok(mut app) = state.lock()
                && app
                    .library
                    .selected_compilation_track_paths
                    .contains(&track_path.as_str().to_ascii_lowercase())
            {
                if app.last_error.as_deref() == Some("A SPLINED run is already active.") {
                    app.last_error = None;
                }
                app.operation_status =
                    "Embedded track preview ready · finish or SKIP the active album before searching this track."
                        .to_string();
                app.operation_tone = "orange".to_string();
                app.push_log(
                    "Previewed a fallback track while another Album remains active; no second run was started."
                        .to_string(),
                );
            }
            let should_start = state.lock().is_ok_and(|app| {
                !controller_active
                    && app.phase == RunPhase::Idle
                    && app
                        .library
                        .selected_compilation_track_paths
                        .contains(&track_path.as_str().to_ascii_lowercase())
            });
            if should_start && let Err(error) = controller.start() {
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
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
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
        ui.on_library_row_activated(move |key, include_bypassed| {
            let started = Instant::now();
            let prior_artist = state
                .lock()
                .ok()
                .and_then(|app| app.library.focused_artist.clone());
            let hydration = activate_library_row(&state, key.as_str(), include_bypassed);
            let artist = state.lock().ok().and_then(|app| {
                let activated_album = app
                    .library
                    .albums
                    .iter()
                    .any(|album| album.snapshot.album_key.eq_ignore_ascii_case(key.as_str()));
                (!activated_album).then(|| {
                    app.library.focused_artist.clone().unwrap_or_else(|| {
                        if app.current_artist.trim().is_empty() {
                            key.to_string()
                        } else {
                            app.current_artist.clone()
                        }
                    })
                })
            });
            if artist.is_some()
                && let Ok(mut app) = state.lock()
                && prior_artist != app.library.focused_artist
            {
                app.artist_artwork_draft_path.clear();
                let cached = app
                    .library
                    .focused_artist
                    .as_ref()
                    .and_then(|artist| {
                        app.artist_overview_cache
                            .get(&artist.trim().to_ascii_lowercase())
                    })
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
                    app.artist_overview_track_paths.clear();
                    app.artist_overview_album_ratings.clear();
                    app.artist_overview_artwork_candidates.clear();
                    app.artist_overview_top_songs.clear();
                }
                app.artist_overview_top_songs_loading = false;
                app.artist_overview_show_all = false;
                app.artist_artwork_mode = false;
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
            if let Some((album_key, album_path, cached)) = hydration {
                let weak = weak.clone();
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    let result = fallback_tracks_from_folder(Path::new(&album_path), &cached);
                    let _ = slint::invoke_from_event_loop(move || {
                        match result {
                            Ok(tracks) => {
                                if let Ok(mut app) = state.lock() {
                                    app.library.replace_fallback_tracks(&album_key, tracks);
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
        let controller = Arc::clone(&controller);
        ui.on_artist_overview_album_selected(move |key| {
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
                let controller = Arc::clone(&controller);
                std::thread::spawn(move || {
                    let result = fallback_tracks_from_folder(Path::new(&album_path), &cached);
                    let _ = slint::invoke_from_event_loop(move || {
                        match result {
                            Ok(tracks) => {
                                let mut single_track = None;
                                if let Ok(mut app) = state.lock() {
                                    app.library.replace_fallback_tracks(&album_key, tracks);
                                    let visible = app.library.compilation_track_rows();
                                    app.operation_status = if visible.is_empty() {
                                        "COMP opened · no matching tracks found for this Artist."
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
                                    } else {
                                        "cyan".to_string()
                                    };
                                    if visible.len() == 1 {
                                        single_track =
                                            visible.first().map(|track| track.path.clone());
                                    }
                                }
                                if let Some(track_path) = single_track {
                                    activate_compilation_track(
                                        &state,
                                        &album_key,
                                        &track_path,
                                        false,
                                    );
                                    if controller.is_running() {
                                        if let Ok(mut app) = state.lock() {
                                            app.operation_status =
                                                "Embedded track preview ready · finish or SKIP the active album before searching this track."
                                                    .to_string();
                                            app.operation_tone = "orange".to_string();
                                            app.push_log(
                                                "Expanded a one-track fallback album while another Album remains active; no second run was started."
                                                    .to_string(),
                                            );
                                        }
                                    } else if let Err(error) = controller.start() {
                                        record_action_result(&state, Err(error));
                                    }
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
                let artist = app
                    .library
                    .focused_artist
                    .clone()
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
                ))
            });
            let Some((config, artist, artist_mbid, enabled_sources, queried_sources, existing)) = prepared else {
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
                            app.operation_busy = false;
                            if !app.library.focused_artist.as_ref().is_some_and(|focused| {
                                focused.trim().eq_ignore_ascii_case(artist.trim())
                            }) {
                                return Ok(());
                            }
                            let mut artwork_candidates = existing;
                            let failed_sources = match remote {
                                Ok(result) => {
                                    for warning in result.warnings {
                                        app.push_log(format!("Artist Art source warning: {warning}"));
                                    }
                                    for candidate in result.candidates {
                                        if !artwork_candidates.iter().any(|(_, path)| {
                                            path.eq_ignore_ascii_case(&candidate.1)
                                        }) {
                                            artwork_candidates.push(candidate);
                                        }
                                    }
                                    result.failed_sources
                                }
                                Err(error) => {
                                    app.push_log(format!(
                                        "Remote Artist Art search warning: {error}"
                                    ));
                                    queried_sources.clone()
                                }
                            };
                            app.artist_overview_artwork_candidates = artwork_candidates.clone();
                            if let Some(cache) = app
                                .artist_overview_cache
                                .get_mut(&artist.trim().to_ascii_lowercase())
                            {
                                cache.artwork_candidates = artwork_candidates;
                            }
                            load_artist_art_candidates(
                                &mut app,
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
                if app.artist_overview_top_songs_loading {
                    return None;
                }
                let artist = app.library.focused_artist.clone()?;
                app.artist_overview_top_songs_loading = true;
                app.operation_status = format!("Loading {artist} top songs from Navidrome…");
                app.operation_tone = "cyan".to_string();
                Some((app.config.clone(), artist))
            });
            let Some((config, artist)) = prepared else {
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
                        app.artist_overview_top_songs_loading = false;
                        match &result {
                            Ok(songs) => {
                                app.artist_overview_top_songs.clone_from(songs);
                                if let Some(cache) = app
                                    .artist_overview_cache
                                    .get_mut(&artist.trim().to_ascii_lowercase())
                                {
                                    cache.top_songs.clone_from(songs);
                                }
                                app.operation_status = format!(
                                    "Loaded {} Navidrome top song{} for {artist}",
                                    songs.len(),
                                    if songs.len() == 1 { "" } else { "s" }
                                );
                                app.operation_tone = "green".to_string();
                            }
                            Err(_) => {
                                app.operation_status = "Top songs could not be loaded".to_string();
                                app.operation_tone = "red".to_string();
                            }
                        }
                    }
                    if let Err(error) = result {
                        record_action_result(&state, Err(error));
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
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let action_log = ui_debug_log.clone();
        ui.on_write_artist_artwork(move || {
            let prepared = (|| -> Result<(String, String, PathBuf, bool), String> {
                let mut app = state
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
                if app.operation_busy {
                    return Err("Another SPLINED operation is still in progress.".to_string());
                }
                let artist = app
                    .library
                    .focused_artist
                    .clone()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| "Select a Navidrome-linked artist first.".to_string())?;
                let selected = app.artist_artwork_draft_path.trim().to_string();
                if selected.is_empty() {
                    return Err("Select an artist artwork image before writing.".to_string());
                }
                inspect_image(Path::new(&selected))?;
                let artist_directory = focused_artist_directory(&app, &artist)?;
                let preserve_file = app.config.output.preserve_file;
                app.operation_busy = true;
                app.operation_status = "Saving artist.*…".to_string();
                app.operation_tone = "orange".to_string();
                app.last_error = None;
                Ok((artist, selected, artist_directory, preserve_file))
            })();
            let (artist, selected, artist_directory, preserve_file) = match prepared {
                Ok(value) => value,
                Err(error) => {
                    record_action_result(&state, Err(error));
                    refresh_weak(&weak, &state);
                    return;
                }
            };
            if let Some(log) = action_log.as_ref() {
                log.record(format!(
                    "artist-art-write-start artist={} selected={} directory={} preserve_file={}",
                    artist,
                    selected,
                    artist_directory.display(),
                    preserve_file
                ));
            }
            refresh_weak(&weak, &state);
            let state = Arc::clone(&state);
            let weak = weak.clone();
            let action_log = action_log.clone();
            std::thread::spawn(move || {
                let result = (|| -> Result<(PathBuf, bool, Vec<String>), String> {
                    let source_bytes = fs::read(&selected).map_err(|error| {
                        format!("Unable to read selected artist artwork {selected}: {error}")
                    })?;
                    write_artist_art_file(&artist_directory, &source_bytes, preserve_file)
                })();
                let succeeded = result.is_ok();
                if let Some(log) = action_log.as_ref() {
                    log.record(format!(
                        "artist-art-write-finished artist={} target={} success={} preserve_file={}",
                        artist,
                        result
                            .as_ref()
                            .map(|(path, _, _)| path.display().to_string())
                            .unwrap_or_default(),
                        succeeded,
                        preserve_file
                    ));
                }
                let _ = slint::invoke_from_event_loop(move || {
                    if let Ok(mut app) = state.lock() {
                        app.operation_busy = false;
                        if let Ok((target, unchanged, warnings)) = &result {
                            app.library.set_artist_artwork_path(&artist, target);
                            app.artist_artwork_draft_path.clear();
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
                });
            });
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let action_log = ui_debug_log.clone();
        ui.on_library_row_selection_activated(move |key, include_bypassed| {
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
                                    app.library.replace_fallback_tracks(&album_key, tracks);
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
                                    app.library.replace_fallback_tracks(&album_key, tracks);
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
            if let Err(error) = controller.refresh_library(rebuild)
                && let Ok(mut app) = state.lock()
            {
                app.last_error = Some(error.clone());
                app.push_log(format!("ERROR: {error}"));
            }
            refresh_weak(&weak, &state);
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
                let (artwork_visible, candidate_filter_expanded) = state
                    .lock()
                    .map(|mut app| {
                        app.media_selector_expanded = true;
                        (app.show_artwork, app.candidate_filter_expanded)
                    })
                    .unwrap_or((true, true));
                let (window_width, window_height) = app_logical_window_size(&ui);
                let mut layout = workspace_layout.borrow_mut();
                layout.clamp_for_window(
                    window_width,
                    window_height,
                    true,
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
                app.artist_artwork_draft_path = app
                    .selected_candidate()
                    .map(|candidate| candidate.cache_path.clone())
                    .unwrap_or_default();
                app.operation_status = "Artist Art selected — ready to WRITE Artist".to_string();
                app.operation_tone = "orange".to_string();
                if let Some(log) = UI_DEBUG_LOG_HANDLE.get() {
                    log.record(format!(
                        "artist-art-selected index={} path={}",
                        index, app.artist_artwork_draft_path
                    ));
                }
            }
            refresh_weak(&weak, &state);
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
                    app.cycle_candidate_filter(dimension.as_str(), key.as_str())
                        .map(|_| ())
                });
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_clear_candidate_filters(move || {
            if let Ok(mut app) = state.lock() {
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
                    .and_then(|mut app| app.selected_musicbrainz_result_decision());
                decision.and_then(|decision| controller.write_selected_musicbrainz(decision))
            } else {
                let decision = state
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                    .and_then(|app| app.candidate_decision(action.as_str()));
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
            if let Ok(mut app) = state.lock() {
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
        ui.on_adjustment_set(move |name, position| {
            if let Ok(mut app) = state.lock() {
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
                    }
                });
            });
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
                let selected = browse_for_backup_file(&current);
                let _ = slint::invoke_from_event_loop(move || {
                    if let (Some(path), Some(ui)) = (selected, weak.upgrade()) {
                        ui.set_restore_path(path.into());
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
                    "Saving Upscale to cover.*…".to_string()
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
                                "Upscale saved to cover.*".to_string()
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
    bind_service_callbacks(ui, state);
}

fn bind_service_callbacks(ui: &AppWindow, state: Arc<Mutex<ApplicationState>>) {
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
                    update_source_policy_from_ui(&ui, &mut draft)
                });
            if result.is_ok()
                && let Ok(mut app) = state.lock()
            {
                app.service_status =
                    "Source policy updated in the Settings draft. Save to apply.".to_string();
            }
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    }
    {
        let weak = ui.as_weak();
        let state = Arc::clone(&state);
        let settings_draft = Arc::clone(&settings_draft);
        ui.on_save_settings(move || {
            let result = (|| {
                let ui = weak
                    .upgrade()
                    .ok_or_else(|| "The SPLINED Settings window is unavailable.".to_string())?;
                if state
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())?
                    .phase
                    != RunPhase::Idle
                {
                    return Err("Config v5 cannot be saved during an active run.".to_string());
                }
                let mut config = settings_draft
                    .lock()
                    .map_err(|_| "The structured Settings draft is unavailable.".to_string())?
                    .clone();
                apply_settings_ui(&ui, &mut config)?;
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
                replacement.disable_intro_message = ui.get_settings_disable_intro_message();
                replacement.disable_confirmation_messages =
                    ui.get_settings_disable_confirmation_messages();
                replacement.intro_dismissed = intro_dismissed || replacement.disable_intro_message;
                replacement.push_log("Validated and atomically saved Config v5.".to_string());
                for diagnostic in diagnostics {
                    replacement.push_log(diagnostic);
                }
                *state
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())? =
                    replacement;
                *settings_draft
                    .lock()
                    .map_err(|_| "The structured Settings draft is unavailable.".to_string())? =
                    config.clone();
                set_settings_ui(&ui, &config);
                if let Ok(editor) = load_editor(&config, "Navidrome") {
                    set_credential_ui(&ui, &editor);
                }
                Ok(())
            })();
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    }
    {
        let weak = ui.as_weak();
        let state = Arc::clone(&state);
        let settings_draft = Arc::clone(&settings_draft);
        ui.on_save_setup(move || {
            let result =
                (|| {
                    let ui = weak
                        .upgrade()
                        .ok_or_else(|| "The SPLINED Setup window is unavailable.".to_string())?;
                    let mut config = settings_draft
                        .lock()
                        .map_err(|_| "The structured Settings draft is unavailable.".to_string())?
                        .clone();
                    apply_settings_ui(&ui, &mut config)?;
                    if config.scan.scan_library_dir.trim().is_empty() {
                        config.scan.scan_library_dir = config.library.music_library.clone();
                    }
                    let config = splined_core::settings::save_config(&config)?;
                    let backup_directory = PathBuf::from(ui.get_backup_directory().as_str());
                    if !backup_directory.as_os_str().is_empty() {
                        fs::create_dir_all(&backup_directory).map_err(|error| {
                            format!(
                                "Unable to create the portable backup directory {}: {error}",
                                backup_directory.display()
                            )
                        })?;
                    }
                    let snapshot = media_snapshot(&config, false)?;
                    let diagnostics = startup_diagnostics(&config, &snapshot);
                    let mut replacement = ApplicationState::new(config.clone(), snapshot)?;
                    replacement.push_log("First-run Setup completed.".to_string());
                    for diagnostic in diagnostics {
                        replacement.push_log(diagnostic);
                    }
                    *state.lock().map_err(|_| {
                        "The SPLINED application state is unavailable.".to_string()
                    })? = replacement;
                    *settings_draft.lock().map_err(|_| {
                        "The structured Settings draft is unavailable.".to_string()
                    })? = config.clone();
                    ui.set_setup_required(false);
                    ui.set_page_index(1);
                    set_settings_ui(&ui, &config);
                    splined_core::settings::save_ui_preferences(&UiPreferences::default())?;
                    if let Ok(editor) = load_editor(&config, "Navidrome") {
                        set_credential_ui(&ui, &editor);
                    }
                    Ok(())
                })();
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
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
        ui.on_backup_action(
            move |action,
                  path,
                  password,
                  settings,
                  credentials,
                  database,
                  interface,
                  diagnostics| {
                let selection = BackupSelection {
                    settings,
                    credentials,
                    database,
                    interface,
                    diagnostics,
                };
                let path = std::path::PathBuf::from(path.as_str());
                let password = password.to_string();
                let config = state.lock().ok().map(|app| app.config.clone());
                let setup_required = weak.upgrade().is_some_and(|ui| ui.get_setup_required());
                let weak = weak.clone();
                let state = Arc::clone(&state);
                let settings_draft = Arc::clone(&settings_draft);
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
                                .and_then(|path| export_database_backup(&path, &config))
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
                            restore_database_backup(&path, &config).and_then(|restored_path| {
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
                                restore_backup(&path, &config, selection, &password).and_then(
                                    |restored| {
                                        let snapshot = media_snapshot(&restored, false)?;
                                        let diagnostics = startup_diagnostics(&restored, &snapshot);
                                        let mut replacement =
                                            ApplicationState::new(restored.clone(), snapshot)?;
                                        replacement.push_log(
                                            "Backup restored and Config v5 loaded.".to_string(),
                                        );
                                        for diagnostic in diagnostics {
                                            replacement.push_log(diagnostic);
                                        }
                                        Ok((
                                            "Backup restored and loaded.".to_string(),
                                            Some((restored, replacement)),
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
                                    *app = replacement;
                                }
                                if let Ok(mut draft) = settings_draft.lock() {
                                    *draft = config.clone();
                                }
                                if let Some(ui) = weak.upgrade() {
                                    ui.set_setup_required(false);
                                    ui.set_page_index(0);
                                    set_settings_ui(&ui, &config);
                                }
                            }
                            Ok((message, None)) => {
                                if let Ok(mut app) = state.lock() {
                                    app.service_status = message;
                                }
                            }
                            Err(error) => {
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
    ui.set_settings_enhancement_changed(changed);
}

fn set_settings_ui(ui: &AppWindow, config: &Config) {
    ui.set_settings_music_library(config.library.music_library.clone().into());
    ui.set_settings_scan_library(config.scan.scan_library_dir.clone().into());
    ui.set_settings_database_dir(config.scan.cache_dir.clone().into());
    ui.set_settings_temporary_cache_dir(config.scan.temporary_cache_dir.clone().into());
    ui.set_settings_log_dir(config.scan.log_dir.clone().into());
    ui.set_settings_credential_dir(config.credentials.credential_dir.clone().into());
    ui.set_settings_ignored_subs(config.library.ignored_subs.join(", ").into());
    ui.set_settings_mode(
        match config.mode {
            Mode::Read => "read",
            Mode::Write => "write",
        }
        .into(),
    );
    ui.set_settings_verbosity(
        match config.verbosity {
            Verbosity::Error => "error",
            Verbosity::Warn => "warn",
            Verbosity::Info => "info",
            Verbosity::Debug => "debug",
            Verbosity::Trace => "trace",
        }
        .into(),
    );
    ui.set_settings_scan_mode(config.scan.scan_mode);
    ui.set_settings_library_scan(config.scan.library_scan);
    ui.set_settings_scan_timeout(config.scan.scan_mode_timeout.0.to_string().into());
    ui.set_settings_sqlite_shared(config.scan.sqlite_shared);
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
    ui.set_settings_square_mode(config.output.square_mode.clone().into());
    ui.set_settings_square_round_to(config.output.square_round_to.to_string().into());
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
    ui.set_settings_policy_source(source.into());
    ui.set_settings_policy_enabled(policy.enabled);
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
    config.scan.temporary_cache_dir = ui.get_settings_temporary_cache_dir().trim().to_string();
    config.scan.log_dir = ui.get_settings_log_dir().trim().to_string();
    config.credentials.credential_dir = ui.get_settings_credential_dir().trim().to_string();
    config.library.ignored_subs = split_values(ui.get_settings_ignored_subs().as_str());
    config.mode = match ui.get_settings_mode().as_str() {
        "write" => Mode::Write,
        "read" => Mode::Read,
        value => return Err(format!("Unsupported mode: {value}")),
    };
    config.verbosity = match ui.get_settings_verbosity().as_str() {
        "error" => Verbosity::Error,
        "warn" => Verbosity::Warn,
        "info" => Verbosity::Info,
        "debug" => Verbosity::Debug,
        "trace" => Verbosity::Trace,
        value => return Err(format!("Unsupported verbosity: {value}")),
    };
    config.scan.scan_mode = ui.get_settings_scan_mode();
    config.scan.library_scan = ui.get_settings_library_scan();
    config.scan.scan_mode_timeout.0 =
        parse_setting(&ui.get_settings_scan_timeout(), "Scan timeout")?;
    config.scan.sqlite_shared = ui.get_settings_sqlite_shared();
    config.samples.sample_write = ui.get_settings_sample_write();
    config.samples.sample_dir = ui.get_settings_sample_dir().trim().to_string();
    config.output.file_formats = split_values(ui.get_settings_file_formats().as_str());
    config.output.file_name = ui.get_settings_file_name().trim().to_string();
    config.output.preserve_file = ui.get_settings_preserve_file();
    config.output.square = ui.get_settings_square();
    config.output.square_mode = ui.get_settings_square_mode().trim().to_string();
    config.output.square_round_to =
        parse_setting(&ui.get_settings_square_round_to(), "Square round-to")?;
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
    config.source_policies.insert(source, policy);
    Ok(())
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
        _ => {}
    }
}

fn adjustment_bounds(name: &str) -> (i32, i32) {
    if name == "Color" {
        (-100, 100)
    } else {
        (-20, 20)
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
        _ => {}
    }
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
        _ => {}
    }
}

fn clear_rendered_preview_for_adjustment(app: &mut ApplicationState) {
    // Keep the selected MusicBrainz cover visible while a replacement preview
    // is rendered. Its stable source path is separate from edit_preview_path,
    // so repeated adjustments never fall through to an absent album cover.*.
    if app.phase != RunPhase::AwaitingMusicBrainz
        || app.musicbrainz_cover_preview_path.trim().is_empty()
    {
        app.edit_preview_path.clear();
    }
}

fn edit_existing_cover(
    state: &Arc<Mutex<ApplicationState>>,
    apply: bool,
    guard: Option<(&AtomicU64, u64)>,
) -> Result<(), String> {
    let (path, config, context, artist, album, album_path, album_key) = {
        let app = state
            .lock()
            .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
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
            app.library.focused_album_key.clone(),
        )
    };
    if !preview_request_is_current(guard) {
        return Ok(());
    }
    let path = Path::new(&path);
    let candidate = Candidate::from_file("local", path, 0)?;
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
        candidate.format,
        &config.output,
        true,
    )?;
    let preview_key = if apply {
        install_prepared_artwork(path, &prepared)?;
        remove_saved_sample_album(&config, &artist, &album)?;
        let album_path = Path::new(&album_path);
        if !album_path.is_dir() {
            return Err("The selected Album directory is unavailable.".to_string());
        }
        let material = RuntimeArtworkMaterial {
            path: path.to_path_buf(),
            format: format!("{:?}", prepared.info.format).to_ascii_uppercase(),
            width: prepared.info.width,
            height: prepared.info.height,
        };
        let mut completion_config = config.clone();
        completion_config.scan.scan_mode_timeout = ScanTimeout(0.0);
        record_album_outcome_from_runtime(
            &completion_config,
            album_path,
            Some(album_path),
            album_key.as_deref(),
            "installed",
            Some("local"),
            Some(&material),
        )?;
        let path_text = path.to_string_lossy().into_owned();
        if !preload_scaled_image(&path_text, PREVIEW_CACHE_DIMENSION) {
            return Err(format!(
                "Unable to load the saved existing cover {}.",
                path.display()
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
        }
        app.edit_preview_path = preview_key;
        app.push_log(if apply {
            format!(
                "Edited existing cover and marked the Album Processed: {}",
                path.display()
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
            if app.selected_candidate().is_some() {
                "candidate"
            } else if app.phase == RunPhase::AwaitingMusicBrainz
                && app.selected_musicbrainz_result.is_some()
                && !app.musicbrainz_cover_preview_path.trim().is_empty()
            {
                "musicbrainz"
            } else if app.active_compilation_track_preview().is_some() {
                "compilation-track"
            } else {
                "current-cover"
            }
        });
        let result = match preview_source {
            Some("candidate") => preview_selected_candidate(&state, Some((&revision, ticket))),
            Some("musicbrainz") => preview_musicbrainz_cover(&state, Some((&revision, ticket))),
            Some("compilation-track") => {
                preview_compilation_track(&state, Some((&revision, ticket)))
            }
            _ => edit_existing_cover(&state, false, Some((&revision, ticket))),
        };
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
                    app.operation_status = "Upscale Preview ready".to_string();
                    app.operation_tone = "green".to_string();
                    app.push_log(
                        "Upscale Preview is ready. Apply Upscale is now available.".to_string(),
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

fn preview_selected_candidate(
    state: &Arc<Mutex<ApplicationState>>,
    guard: Option<(&AtomicU64, u64)>,
) -> Result<(), String> {
    let (index, source, source_path, config, context) = {
        let app = state
            .lock()
            .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
        let selected = app.selected_candidate().ok_or_else(|| {
            "Select an artwork candidate before requesting a preview.".to_string()
        })?;
        if selected.cache_path.trim().is_empty() {
            return Err("The selected artwork candidate has no cached image.".to_string());
        }
        let mut config = app.config.clone();
        config.mode = Mode::Read;
        project_adjustments(&app, &mut config);
        (
            selected.index,
            selected.source.clone(),
            selected.cache_path.clone(),
            config,
            preview_context(&app),
        )
    };
    if !preview_request_is_current(guard) {
        return Ok(());
    }
    let source_path = Path::new(&source_path);
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
        app.edit_preview_path = preview_key;
        app.push_log(format!(
            "Rendered MusicBrainz release {index} preview in memory at {} × {} without changing album files.",
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
    config.output.upscale_adaptive_defaults = app.adjustments.adaptive_defaults;
    config.output.upscale_picture_percent = app.adjustments.picture_percent;
    config.output.upscale_sharpen_percent = app.adjustments.sharpen_percent;
    config.output.upscale_softness_percent = app.adjustments.softness_percent;
    config.output.upscale_contrast_percent = app.adjustments.contrast_percent;
    config.output.upscale_exposure_percent = app.adjustments.exposure_percent;
    config.output.upscale_brightness_percent = app.adjustments.brightness_percent;
    config.output.upscale_gamma_percent = app.adjustments.gamma_percent;
    config.output.upscale_color_temperature = app.adjustments.color_temperature;
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
                        if !hover_preview && selected_same_result {
                            app.operation_status = format!(
                                "MusicBrainz release selected · cover preview unavailable · {error}"
                            );
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
    if let Some(ui) = weak.upgrade() {
        sync_ui(&ui, state);
    }
    let elapsed_ms = started.elapsed().as_millis();
    if (count <= 8 || count & 63 == 0 || elapsed_ms >= 75)
        && let Some(log) = UI_DEBUG_LOG_HANDLE.get()
    {
        log.record(format!(
            "ui-refresh count={count} elapsed_ms={elapsed_ms} caller={}:{}",
            caller.file(),
            caller.line()
        ));
    }
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
                let _ = slint::invoke_from_event_loop(move || refresh_weak(&weak, &state));
            }
        });
    }
}

fn preview_image_source_paths(app: &ApplicationState) -> BTreeSet<String> {
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
        app.musicbrainz_cover_preview_path.as_str(),
        hovered_musicbrainz.unwrap_or_default(),
        candidate.unwrap_or_default(),
        compilation.unwrap_or_default(),
    ]
    .into_iter()
    .filter(|path| !path.trim().is_empty())
    .map(str::to_string)
    .collect()
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
    ui.set_artist_overview_active(artist_overview_active);
    ui.set_artist_overview_name(SharedString::from(
        state.library.focused_artist.clone().unwrap_or_default(),
    ));
    let artist_artwork_path = state
        .library
        .focused_artist
        .as_ref()
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
    ui.set_artist_overview_navidrome_linked(state.library.focused_artist.as_ref().is_some_and(
        |artist| {
            state
                .library
                .artist_navidrome_ids
                .contains_key(&artist.trim().to_ascii_lowercase())
        },
    ));
    ui.set_artist_overview_biography(SharedString::from(state.artist_overview_biography.clone()));
    ui.set_artist_overview_external_url(SharedString::from(
        state.artist_overview_external_url.clone(),
    ));
    let indexed_artist_mbid = state
        .library
        .focused_artist
        .as_ref()
        .and_then(|artist| {
            state.library.albums.iter().find_map(|album| {
                (album.snapshot.artist.trim().eq_ignore_ascii_case(artist)
                    && !album.snapshot.artist_mbid.trim().is_empty())
                .then(|| album.snapshot.artist_mbid.trim().to_string())
            })
        })
        .unwrap_or_default();
    ui.set_artist_overview_musicbrainz_id(SharedString::from(
        if state.artist_overview_musicbrainz_id.trim().is_empty() {
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
    ui.set_library_view_label(SharedString::from(state.library.view.label()));
    ui.set_compilation_track_rows(model(compilation_tracks));

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

    let mut previous_musicbrainz_group = String::new();
    let musicbrainz = state
        .visible_musicbrainz_results()
        .into_iter()
        .map(|item| {
            let group_key = musicbrainz_group_key(item);
            let group = musicbrainz_group_label(item);
            let group_start = group_key != previous_musicbrainz_group;
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
    ui.set_musicbrainz_artist_options(model(
        std::iter::once(SharedString::from("All"))
            .chain(
                state
                    .musicbrainz_results
                    .iter()
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
                state
                    .musicbrainz_results
                    .iter()
                    .map(|item| item.release_class.clone())
                    .filter(|value| !value.is_empty())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .map(SharedString::from),
            )
            .collect(),
    ));

    let adjustment = &state.adjustments;
    ui.set_adjustment_rows(model(vec![
        adjustment_row(&state, "Picture", adjustment.picture_percent, "%"),
        adjustment_row(&state, "Sharpen", adjustment.sharpen_percent, "%"),
        adjustment_row(&state, "Softness", adjustment.softness_percent, "%"),
        adjustment_row(&state, "Contrast", adjustment.contrast_percent, "%"),
        adjustment_row(&state, "Exposure", adjustment.exposure_percent, "%"),
        adjustment_row(&state, "Brightness", adjustment.brightness_percent, "%"),
        adjustment_row(&state, "Gamma", adjustment.gamma_percent, "%"),
        adjustment_row(&state, "Color", adjustment.color_temperature, ""),
    ]));

    let preview_candidate = state.preview_candidate();
    let has_edit_preview = !state.edit_preview_path.is_empty();
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
        && (state.current_album.trim().is_empty() || artist_overview_fallback_album)
        && !has_edit_preview
        && !has_compilation_track_preview
        && preview_candidate.is_none();
    ui.set_selected_artwork_artist_overview(selected_artwork_artist_overview);
    ui.set_selected_artwork_artist_has_art(!artist_artwork_path.trim().is_empty());
    let current_cover_path = if state.library.show_tracks || artist_overview_fallback_album {
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
    let original_preview_path = musicbrainz_preview_source
        .or_else(|| {
            preview_candidate
                .map(|candidate| candidate.cache_path.as_str())
                .filter(|path| !path.is_empty())
        })
        .or(compilation_track_preview)
        .unwrap_or(current_cover_path);
    let focused_album_thumbnail = if state.library.show_tracks || artist_overview_fallback_album {
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
    let preview_path = musicbrainz_preview_source
        .or_else(|| has_edit_preview.then_some(state.edit_preview_path.as_str()))
        .or_else(|| {
            preview_candidate
                .map(|candidate| candidate.cache_path.as_str())
                .filter(|path| !path.is_empty())
        })
        .or(compilation_track_preview)
        .unwrap_or(current_cover_path);
    ui.set_current_preview(load_cached_preview_image(
        preview_path,
        focused_album_thumbnail,
    ));
    ui.set_selected_artwork_source(SharedString::from(if selected_artwork_artist_overview {
        "Navidrome".to_string()
    } else if musicbrainz_cover_preview {
        "MusicBrainz".to_string()
    } else if has_edit_preview {
        "Enhancement".to_string()
    } else if preview_candidate.is_none() && has_compilation_track_preview {
        "Embedded".to_string()
    } else {
        preview_candidate
            .map(|candidate| source_display_name(&candidate.source))
            .unwrap_or_else(|| "Local".to_string())
    }));
    ui.set_selected_artwork_type(SharedString::from(if selected_artwork_artist_overview {
        "Artist Image"
    } else if musicbrainz_cover_preview {
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
    }));
    let metadata_path = if selected_artwork_artist_overview {
        artist_artwork_path.as_str()
    } else {
        preview_path
    };
    let cached_preview = cached_preview_metadata(metadata_path);
    let preview_dimensions = if selected_artwork_artist_overview || has_edit_preview {
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
    } else if has_edit_preview {
        "Preview"
    } else {
        preview_candidate
            .map(|candidate| candidate.range_class.as_str())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("Current")
    }));
    ui.set_selected_artwork_size(SharedString::from(
        cached_preview
            .map(|(_, _, bytes)| byte_size_label(bytes))
            .unwrap_or_else(|| "Size unavailable".to_string()),
    ));
    ui.set_selected_artwork_is_sample(
        preview_candidate.is_some_and(|candidate| candidate.source.eq_ignore_ascii_case("samples")),
    );
    ui.set_artist_artwork_mode(state.artist_artwork_mode);
    let selected_artwork_write_ready = if state.artist_artwork_mode {
        !state.artist_artwork_draft_path.trim().is_empty()
    } else if state.phase == RunPhase::AwaitingMusicBrainz {
        state.selected_musicbrainz_result.is_some()
            && !state.musicbrainz_cover_preview_path.trim().is_empty()
    } else if state.phase == RunPhase::AwaitingCandidate {
        state.selected_candidate().is_some()
    } else {
        !current_cover_path.trim().is_empty()
            || state
                .selected_candidate()
                .is_some_and(|candidate| candidate.source.eq_ignore_ascii_case("samples"))
    };
    ui.set_selected_artwork_write_ready(selected_artwork_write_ready);
    ui.set_selected_artwork_existing(
        !state.artist_artwork_mode
            && state.phase == RunPhase::AwaitingCandidate
            && state
                .selected_candidate()
                .is_some_and(|candidate| candidate.source.eq_ignore_ascii_case("existing")),
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
        !ui.get_setup_required() && !intro_active && !state.operation_busy && state.can_launch(),
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
    ui.set_artwork_editing_expanded(state.artwork_editing_expanded);
    ui.set_show_artwork(state.show_artwork);
    ui.set_hover_enabled(state.hover_enabled);
    ui.set_upscale_preview_only(state.upscale_preview_only);
    ui.set_completion_dialog_visible(state.completion_dialog_visible);
    ui.set_completion_message(SharedString::from(state.completion_message.clone()));
    ui.set_settings_disable_intro_message(state.disable_intro_message);
    ui.set_settings_disable_confirmation_messages(state.disable_confirmation_messages);
    ui.set_artist_filter_text(SharedString::from(state.library.artist_filter.clone()));
    ui.set_album_filter_text(SharedString::from(state.library.album_filter.clone()));
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
    })
    .into()
}

fn artist_overview_ui_model(state: &ApplicationState) -> ModelRc<UiArtistAlbumRow> {
    if state.library.view != LibraryView::Artists || state.library.focused_artist.is_none() {
        return model(Vec::new());
    }
    let has_artist_art = state
        .library
        .focused_artist
        .as_ref()
        .and_then(|artist| {
            state
                .library
                .artist_artwork_paths
                .get(&artist.trim().to_ascii_lowercase())
        })
        .is_some_and(|path| !path.trim().is_empty());
    let rows = state
        .library
        .artist_overview_album_indices(
            &state.artist_overview_album_keys,
            state.artist_overview_show_all,
        )
        .into_iter()
        .filter_map(|index| state.library.albums.get(index))
        .map(|album| {
            let snapshot = &album.snapshot;
            let album_key = snapshot.album_key.to_ascii_lowercase();
            let fallback_progress = state.artist_fallback_track_progress(album);
            let fallback_complete =
                fallback_progress.is_some_and(|(completed, total)| completed == total);
            let featured = state
                .library
                .focused_artist
                .as_deref()
                .is_some_and(|artist| {
                    splined_core::application::LibraryState::artist_fallback_is_featured(
                        album, artist,
                    )
                });
            let bypassed = album.state == splined_core::application::AlbumState::Bypassed;
            let missing_album_art = !snapshot.has_local_artwork;
            let incomplete = matches!(
                album.state,
                splined_core::application::AlbumState::Incomplete
                    | splined_core::application::AlbumState::TimeoutActive
            );
            let yellow_active = !has_artist_art || bypassed;
            let red_active =
                snapshot.album_mbid.trim().is_empty() || missing_album_art || incomplete;
            let green_active = album.state == splined_core::application::AlbumState::Processed
                && has_artist_art
                && !missing_album_art;
            let yellow_tip = [
                (!has_artist_art).then_some("Missing Artist Art"),
                bypassed.then_some("Bypassed Album"),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
            let red_tip = [
                snapshot
                    .album_mbid
                    .trim()
                    .is_empty()
                    .then_some("Missing MBID"),
                missing_album_art.then_some("Missing Album Art"),
                incomplete.then_some("Incomplete Album"),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
            let rating = state
                .artist_overview_album_ratings
                .get(&album_key)
                .copied()
                .unwrap_or_default()
                .min(5);
            UiArtistAlbumRow {
                key: SharedString::from(snapshot.album_key.clone()),
                title: SharedString::from(snapshot.title.clone()),
                year: SharedString::from(snapshot.release_year.clone()),
                artwork: load_scaled_image(
                    state
                        .library
                        .album_artwork_paths
                        .get(&album_key)
                        .map(String::as_str)
                        .unwrap_or_default(),
                    192,
                ),
                status_color: if featured && fallback_complete {
                    Color::from_rgb_u8(255, 151, 56)
                } else {
                    library_status_color(album.state.label())
                },
                selected: album.selected,
                rating_stars: SharedString::from(format!(
                    "{}{}",
                    "★".repeat(rating as usize),
                    "☆".repeat(5_usize.saturating_sub(rating as usize))
                )),
                yellow_active,
                red_active,
                green_active,
                yellow_tip: SharedString::from(if yellow_tip.is_empty() {
                    "No yellow data-quality warning"
                } else {
                    yellow_tip.as_str()
                }),
                red_tip: SharedString::from(if red_tip.is_empty() {
                    "No red data-quality warning"
                } else {
                    red_tip.as_str()
                }),
                green_tip: SharedString::from(if green_active {
                    "Completed Artist & Album Art"
                } else {
                    "Artist and Album art are not both complete"
                }),
                fallback: snapshot.album_mbid.trim().is_empty(),
                fallback_complete,
                featured,
            }
        })
        .collect::<Vec<_>>();
    model(rows)
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

fn compilation_track_ui_rows(state: &ApplicationState) -> Vec<UiCompilationTrackRow> {
    state
        .library
        .compilation_track_rows()
        .into_iter()
        .map(|track| UiCompilationTrackRow {
            album_key: SharedString::from(track.album_key),
            path: SharedString::from(track.path),
            file_name: SharedString::from(track.file_name),
            status_color: status_filter_color(if track.embedded_artwork_recorded {
                StatusFilter::ArtistComplete
            } else {
                StatusFilter::Incomplete
            }),
            completed: track.embedded_artwork_recorded,
            selected: track.selected,
        })
        .collect()
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
        count: state.library.status_count(filter) as i32,
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
fn browse_for_backup_file(initial: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let script = r#"
Add-Type -AssemblyName PresentationFramework
$dialog = New-Object Microsoft.Win32.OpenFileDialog
$dialog.Title = 'Choose a SPLINED backup to restore'
$dialog.Filter = 'SPLINED backups (*.spl;*.sqlite)|*.spl;*.sqlite|Settings archives (*.spl)|*.spl|SQLite snapshots (*.sqlite)|*.sqlite|All files (*.*)|*.*'
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
fn browse_for_backup_file(_initial: &str) -> Option<String> {
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
                    && actual.group_start == expected.group_start
                    && actual.current == expected.current
                    && actual.visited == expected.visited
                    && actual.selected == expected.selected
                    && actual.score == expected.score
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
    raw_image_cache().lock().ok().and_then(|cache| {
        cache
            .get(&cache_key)
            .map(|cached| (cached.source_width, cached.source_height, cached.identity.0))
    })
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
    if let Some(image) = IMAGE_CACHE.with(|cache| {
        cache
            .borrow()
            .get(&cache_key)
            .filter(|cached| cached.identity == cached_pixels.identity)
            .map(|cached| cached.image.clone())
    }) {
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
        CachedPixels, MaximizeGuard, PREVIEW_CACHE_DIMENSION, ThumbnailAlbumTarget,
        UiMusicBrainzRow, apply_ui_preferences, artist_art_candidate_source,
        artist_art_query_sources, artist_overview_ui_model, automatic_backup_is_due,
        backup_file_in_directory, backup_schedule_interval, begin_musicbrainz_preview_request,
        cached_artist_banner_with_bottom_blur, cached_circular_artist_thumbnail,
        circular_artist_art_png, edit_existing_cover, is_sqlite_backup_path, is_valid_mbid,
        local_album_art_file, local_artist_art_file, logical_dimension,
        match_artist_overview_album, model, musicbrainz_failed_url, musicbrainz_ui_rows_equal,
        normalize_backup_schedule, preload_scaled_image, preload_scaled_image_bytes,
        preview_compilation_track, preview_image_source_paths, preview_musicbrainz_cover,
        preview_request_is_current, raw_image_cache, restore_request_from_args, split_values,
        synchronize_fallback_album, trim_large_raw_image_cache, write_artist_art_file,
    };
    use serde_json::json;
    use slint::Model;
    use splined_core::application::{ApplicationState, LaunchMode, RunPhase};
    use splined_core::config::{Config, Mode};
    use splined_core::media_database::{
        MediaAlbumSnapshot, MediaSnapshot, MediaTrackSnapshot, media_snapshot,
    };
    use splined_core::settings::UiPreferences;
    use splined_core::source::navidrome::NavidromeAlbum;
    use std::collections::{HashMap, HashSet};
    use std::ffi::OsString;
    use std::fs;
    use std::io::Cursor;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

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
        assert!(card.red_active);
        assert!(!card.green_active);

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
        assert!(card.red_active);
        assert!(!card.green_active);
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
    fn unchanged_musicbrainz_rows_keep_the_existing_touch_areas() {
        let row = UiMusicBrainzRow {
            index: 7,
            title: "Album".into(),
            artist: "Artist".into(),
            detail: "US · 1977 · album".into(),
            group: "Release Family · Album".into(),
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

        let snapshot = media_snapshot(&config, false).unwrap();
        assert_eq!(snapshot.albums.len(), 1);
        assert_eq!(snapshot.albums[0].status, "unprocessed");
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
    fn fallback_album_focus_never_projects_folder_cover_art() {
        let directory = tempfile::TempDir::new().unwrap();
        let library = directory.path().join("music");
        let album_directory = library.join("Various Artists").join("Soundtrack");
        fs::create_dir_all(&album_directory).unwrap();
        fs::write(album_directory.join("01.mp3"), []).unwrap();
        image::RgbaImage::from_pixel(96, 96, image::Rgba([180, 40, 90, 255]))
            .save_with_format(album_directory.join("cover.png"), image::ImageFormat::Png)
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
        app.library.set_show_tracks(true);
        app.library.focused_album_key = Some(album_key);
        app.edit_preview_path = "stale-preview".to_string();

        synchronize_fallback_album(&mut app);

        assert_eq!(app.current_album, "Soundtrack");
        assert!(app.current_cover_path.is_empty());
        assert!(app.edit_preview_path.is_empty());
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
        assert_eq!((inspected.width, inspected.height), (1400, 360));
        assert_eq!(
            banner,
            cached_artist_banner_with_bottom_blur(&source, directory.path(), "artist-id").unwrap()
        );
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
static PREVIEW_IMAGE_LOADS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static ACTIVE_PREVIEW_CACHE_KEYS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn raw_image_cache() -> &'static Mutex<HashMap<String, CachedPixels>> {
    RAW_IMAGE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn preview_image_loads() -> &'static Mutex<HashSet<String>> {
    PREVIEW_IMAGE_LOADS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn active_preview_cache_keys() -> &'static Mutex<HashSet<String>> {
    ACTIVE_PREVIEW_CACHE_KEYS.get_or_init(|| Mutex::new(HashSet::new()))
}
