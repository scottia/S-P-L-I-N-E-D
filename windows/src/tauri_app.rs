use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use splined::config::{Mode, load_config, resolve_sources};
use splined::gui_events::{self, ScanBridgeContext};
use splined::local_artwork::{embedded_candidate, embedded_preview_cache_dir};
use splined::media_database::{MediaSnapshot, media_snapshot};
use splined::musicbrainz::{AuthorizationSession, MusicBrainzClient};
use splined::portable::app_layout;
use splined::safe_write::replace_text_file;
use splined::scan_runtime::run_scan_library_read_report;
use splined::source::lastfm::{LastFm, LastFmAuthorization};
use splined::windows_backup::{
    BackupSelection, export_backup as export_spl_backup, read_backup,
    restore_backup as restore_spl_backup,
};
use splined::windows_state::{
    PortableState, UiState, load_portable_state, save_config_text, save_ui_state, ui_state_to_text,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::UpdaterExt;

const MAX_PREVIEW_BYTES: u64 = 32 * 1024 * 1024;
const CREDENTIAL_FILES: [&str; 4] = [
    "musicbrainz.json",
    "lastfm.json",
    "fanarttv.json",
    "discogs.json",
];

struct DesktopState {
    scan_running: Arc<AtomicBool>,
    musicbrainz_authorization: Mutex<Option<AuthorizationSession>>,
    lastfm_authorization: Mutex<Option<LastFmAuthorization>>,
}

impl Default for DesktopState {
    fn default() -> Self {
        Self {
            scan_running: Arc::new(AtomicBool::new(false)),
            musicbrainz_authorization: Mutex::new(None),
            lastfm_authorization: Mutex::new(None),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScanRequest {
    album_paths: Vec<String>,
    mode: String,
    review_required: bool,
    auto_ideal: bool,
    compilation_track_path: Option<String>,
    fallback_album: Option<String>,
    fallback_artist: Option<String>,
    indexed_album_path: Option<String>,
    indexed_album_key: Option<String>,
    #[serde(default)]
    bypass_overrides: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateStatus {
    available: bool,
    version: Option<String>,
    current_version: String,
    notes: Option<String>,
    published_at: Option<String>,
    signed_install_enabled: bool,
}

fn updater_public_key() -> Option<&'static str> {
    option_env!("SPLINED_UPDATER_PUBLIC_KEY")
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

#[tauri::command]
fn bootstrap() -> Result<PortableState, String> {
    load_portable_state()
}

#[tauri::command]
fn save_config(config_text: String) -> Result<PortableState, String> {
    save_config_text(&config_text)?;
    load_portable_state()
}

#[tauri::command]
fn save_ui(ui: UiState) -> Result<(), String> {
    save_ui_state(&ui)
}

#[tauri::command]
fn export_backup(
    path: String,
    password: String,
    selection: BackupSelection,
    ui: UiState,
) -> Result<(), String> {
    let state = load_portable_state()?;
    let layout = app_layout()?;
    let config = load_config()?;
    let interface_text = ui_state_to_text(&ui)?;
    export_spl_backup(
        Path::new(&path),
        &layout,
        &state.config_text,
        &interface_text,
        &config,
        selection,
        &password,
    )
}

#[tauri::command]
fn restore_backup(
    path: String,
    password: String,
    selection: BackupSelection,
) -> Result<PortableState, String> {
    let layout = app_layout()?;
    let current = load_portable_state()?;
    let payload = read_backup(Path::new(&path), &password)?;
    restore_spl_backup(&layout, &payload, selection, &current.config_text)?;
    load_portable_state()
}

#[tauri::command]
fn load_library(refresh: bool) -> Result<MediaSnapshot, String> {
    let config = load_config()?;
    media_snapshot(&config, refresh)
}

#[tauri::command]
fn start_scan(
    app: AppHandle,
    state: State<'_, DesktopState>,
    request: ScanRequest,
) -> Result<(), String> {
    if request.album_paths.is_empty() {
        return Err("Select at least one Album before launching a scan.".to_string());
    }
    if state
        .scan_running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("A SPLINED scan is already active.".to_string());
    }

    let mut config = load_config().inspect_err(|_| {
        state.scan_running.store(false, Ordering::SeqCst);
    })?;
    config.mode = if request.mode.eq_ignore_ascii_case("write") {
        Mode::Write
    } else {
        Mode::Read
    };
    let sources = resolve_sources(&config.sources, &config.source_policies, None, None, &[])
        .inspect_err(|_| {
            state.scan_running.store(false, Ordering::SeqCst);
        })?;

    let (event_sender, event_receiver) = std::sync::mpsc::channel::<Value>();
    gui_events::begin_in_process(event_sender, request.review_required, request.auto_ideal)
        .inspect_err(|_| {
            state.scan_running.store(false, Ordering::SeqCst);
        })?;
    gui_events::set_bypass_overrides(request.bypass_overrides.iter().map(PathBuf::from));
    gui_events::set_scan_context(ScanBridgeContext {
        compilation_track_path: request.compilation_track_path.map(PathBuf::from),
        fallback_album: request.fallback_album,
        fallback_artist: request.fallback_artist,
        indexed_album_path: request.indexed_album_path.map(PathBuf::from),
        indexed_album_key: request.indexed_album_key,
    });

    let event_app = app.clone();
    std::thread::spawn(move || {
        while let Ok(event) = event_receiver.recv() {
            let _ = event_app.emit("splined-event", event);
        }
    });

    let scan_app = app.clone();
    let scan_running = state.scan_running.clone();
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                let _ = scan_app.emit(
                    "splined-event",
                    json!({"event":"scan_error","message":format!("Unable to start the in-process runtime: {error}")}),
                );
                gui_events::end_in_process();
                scan_running.store(false, Ordering::SeqCst);
                return;
            }
        };

        let mut error = None;
        for album_path in request.album_paths {
            if gui_events::cancelled() {
                break;
            }
            let mut album_config = config.clone();
            album_config.scan.scan_library_dir = album_path;
            if let Err(run_error) =
                runtime.block_on(run_scan_library_read_report(&album_config, &sources))
            {
                error = Some(run_error);
                break;
            }
        }
        gui_events::end_in_process();
        scan_running.store(false, Ordering::SeqCst);
        let event = match error {
            Some(message) => json!({"event":"scan_error","message":message}),
            None => json!({"event":"scan_idle"}),
        };
        let _ = scan_app.emit("splined-event", event);
    });
    Ok(())
}

#[tauri::command]
fn submit_scan_decision(decision: Value) -> Result<(), String> {
    gui_events::submit_decision(decision)
}

#[tauri::command]
fn stop_scan() {
    gui_events::request_cancel();
}

#[tauri::command]
fn scan_active(state: State<'_, DesktopState>) -> bool {
    state.scan_running.load(Ordering::SeqCst)
}

#[tauri::command]
fn read_image_data_url(path: String) -> Result<String, String> {
    image_path_data_url(&PathBuf::from(path))
}

#[tauri::command]
fn read_track_embedded_artwork(path: String) -> Result<Option<String>, String> {
    let config = load_config()?;
    let cache = embedded_preview_cache_dir(Path::new(&config.scan.temporary_cache_dir));
    embedded_candidate(Path::new(&path), &cache)?
        .map(|candidate| image_path_data_url(candidate.path()))
        .transpose()
}

fn image_path_data_url(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Unable to inspect artwork {}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.len() > MAX_PREVIEW_BYTES {
        return Err("Artwork preview is unavailable or exceeds the preview limit.".to_string());
    }
    let bytes = fs::read(path)
        .map_err(|error| format!("Unable to read artwork {}: {error}", path.display()))?;
    let mime = match image::guess_format(&bytes)
        .map_err(|error| format!("Artwork preview format is unsupported: {error}"))?
    {
        image::ImageFormat::Jpeg => "image/jpeg",
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::WebP => "image/webp",
        _ => return Err("Artwork preview must be JPEG, PNG, or WebP.".to_string()),
    };
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

#[tauri::command]
fn list_credentials() -> Result<Value, String> {
    let config = load_config()?;
    let root = PathBuf::from(config.credentials.credential_dir);
    let entries = CREDENTIAL_FILES
        .iter()
        .map(|name| {
            let path = root.join(name);
            json!({"name":name,"exists":path.is_file()})
        })
        .collect::<Vec<_>>();
    Ok(json!(entries))
}

#[tauri::command]
fn read_credential(name: String) -> Result<String, String> {
    let path = credential_path(&name)?;
    if !path.is_file() {
        return Ok("{}\n".to_string());
    }
    fs::read_to_string(&path)
        .map_err(|error| format!("Unable to read credential file {}: {error}", path.display()))
}

#[tauri::command]
fn save_credential(name: String, contents: String) -> Result<(), String> {
    let _: serde_json::Map<String, Value> = serde_json::from_str(&contents)
        .map_err(|error| format!("Credential JSON is invalid: {error}"))?;
    let path = credential_path(&name)?;
    replace_text_file(&path, &contents, "provider credential", |candidate| {
        serde_json::from_str::<serde_json::Map<String, Value>>(candidate)
            .map(|_| ())
            .map_err(|error| format!("Credential JSON is invalid: {error}"))
    })
}

fn credential_path(name: &str) -> Result<PathBuf, String> {
    if !CREDENTIAL_FILES.contains(&name) {
        return Err(
            "Credential filename is not part of the SPLINED provider contract.".to_string(),
        );
    }
    let config = load_config()?;
    Ok(Path::new(&config.credentials.credential_dir).join(name))
}

#[tauri::command]
fn begin_musicbrainz_authorization(state: State<'_, DesktopState>) -> Result<String, String> {
    let config = load_config()?;
    let client = MusicBrainzClient::new(&config.musicbrainz)?;
    let session = client.begin_authorization()?;
    let url = session.authorization_url.clone();
    *state
        .musicbrainz_authorization
        .lock()
        .map_err(|_| "MusicBrainz authorization state is unavailable.".to_string())? =
        Some(session);
    Ok(url)
}

#[tauri::command]
async fn complete_musicbrainz_authorization(
    state: State<'_, DesktopState>,
    code: String,
) -> Result<(), String> {
    let session = state
        .musicbrainz_authorization
        .lock()
        .map_err(|_| "MusicBrainz authorization state is unavailable.".to_string())?
        .take()
        .ok_or_else(|| "Start MusicBrainz authorization first.".to_string())?;
    let config = load_config()?;
    MusicBrainzClient::new(&config.musicbrainz)?
        .exchange_authorization_code(&session, code.trim())
        .await
}

#[tauri::command]
async fn begin_lastfm_authorization(state: State<'_, DesktopState>) -> Result<String, String> {
    let config = load_config()?;
    let client = LastFm::new(&config.lastfm.credential_file)?;
    let authorization = client.begin_authorization().await?;
    let url = authorization.authorization_url.clone();
    *state
        .lastfm_authorization
        .lock()
        .map_err(|_| "Last.fm authorization state is unavailable.".to_string())? =
        Some(authorization);
    Ok(url)
}

#[tauri::command]
async fn complete_lastfm_authorization(state: State<'_, DesktopState>) -> Result<Value, String> {
    let authorization = state
        .lastfm_authorization
        .lock()
        .map_err(|_| "Last.fm authorization state is unavailable.".to_string())?
        .clone()
        .ok_or_else(|| "Start Last.fm authorization first.".to_string())?;
    let config = load_config()?;
    let identity = LastFm::new(&config.lastfm.credential_file)?
        .complete_authorization(&authorization)
        .await?;
    *state
        .lastfm_authorization
        .lock()
        .map_err(|_| "Last.fm authorization state is unavailable.".to_string())? = None;
    Ok(json!({"username":identity.username,"subscriber":identity.subscriber}))
}

#[tauri::command]
async fn check_for_update(app: AppHandle) -> Result<UpdateStatus, String> {
    if updater_public_key().is_none() {
        return Ok(UpdateStatus {
            available: false,
            version: None,
            current_version: env!("CARGO_PKG_VERSION").into(),
            notes: Some("Signed updates are disabled in this unsigned development build.".into()),
            published_at: None,
            signed_install_enabled: false,
        });
    }
    let update = app
        .updater()
        .map_err(|error| format!("Unable to initialize signed updates: {error}"))?
        .check()
        .await
        .map_err(|error| format!("Unable to check for a signed update: {error}"))?;
    Ok(match update {
        Some(update) => UpdateStatus {
            available: true,
            version: Some(update.version.clone()),
            current_version: update.current_version.clone(),
            notes: update.body.clone(),
            published_at: update.date.map(|date| date.to_string()),
            signed_install_enabled: true,
        },
        None => UpdateStatus {
            available: false,
            version: None,
            current_version: env!("CARGO_PKG_VERSION").into(),
            notes: None,
            published_at: None,
            signed_install_enabled: true,
        },
    })
}

#[tauri::command]
async fn install_update(app: AppHandle, state: State<'_, DesktopState>) -> Result<(), String> {
    if updater_public_key().is_none() {
        return Err("This unsigned development build cannot install public updates.".to_string());
    }
    if state.scan_running.load(Ordering::SeqCst) {
        gui_events::request_cancel();
        for _ in 0..300 {
            if !state.scan_running.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        if state.scan_running.load(Ordering::SeqCst) {
            return Err(
                "The active scan did not close normally. Try the update again after it reaches Ready."
                    .to_string(),
            );
        }
    }
    let update = app
        .updater()
        .map_err(|error| format!("Unable to initialize signed updates: {error}"))?
        .check()
        .await
        .map_err(|error| format!("Unable to check for a signed update: {error}"))?
        .ok_or_else(|| "SPLINED is current.".to_string())?;
    let progress_app = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                let _ = progress_app.emit(
                    "splined-event",
                    json!({"event":"update_progress","chunk":chunk,"total":total}),
                );
            },
            {
                let app = app.clone();
                move || {
                    let _ = app.emit("splined-event", json!({"event":"update_downloaded"}));
                }
            },
        )
        .await
        .map_err(|error| format!("The signed update was not installed: {error}"))?;
    Ok(())
}

pub fn run() -> Result<(), String> {
    let layout = app_layout()?;
    // The official updater's installer inherits this value and the project
    // hook uses it to replace the current portable binary in place.
    unsafe { std::env::set_var("SPLINED_PORTABLE_ROOT", &layout.root) };

    let mut builder = tauri::Builder::default()
        .manage(DesktopState::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            save_config,
            save_ui,
            export_backup,
            restore_backup,
            load_library,
            start_scan,
            submit_scan_decision,
            stop_scan,
            scan_active,
            read_image_data_url,
            read_track_embedded_artwork,
            list_credentials,
            read_credential,
            save_credential,
            begin_musicbrainz_authorization,
            complete_musicbrainz_authorization,
            begin_lastfm_authorization,
            complete_lastfm_authorization,
            check_for_update,
            install_update,
        ]);
    if let Some(public_key) = updater_public_key() {
        builder = builder.plugin(
            tauri_plugin_updater::Builder::new()
                .pubkey(public_key)
                .build(),
        );
    }
    builder
        .run(tauri::generate_context!())
        .map_err(|error| format!("Unable to run the SPLINED desktop application: {error}"))
}
