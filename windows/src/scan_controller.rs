//! In-process Rust scan orchestration for the Windows application.

use crate::application::{ApplicationState, RunPhase, ScanTarget};
use crate::config::resolve_sources;
use crate::gui_events::{self, ScanContext};
use crate::media_database::{media_snapshot, media_snapshot_incremental};
use crate::scan_runtime::run_scan_library_read_report;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};

pub type ProcessingDebugSink = Arc<dyn Fn(String) + Send + Sync + 'static>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControllerNotice {
    StateChanged,
    RunFinished,
}

pub struct ScanController {
    state: Arc<Mutex<ApplicationState>>,
    running: Arc<AtomicBool>,
    refreshing: Arc<AtomicBool>,
    musicbrainz_requested: Arc<AtomicBool>,
    notices: mpsc::Sender<ControllerNotice>,
    debug: Option<ProcessingDebugSink>,
}

struct TargetRunRequest<'a> {
    config: &'a crate::config::Config,
    sources: &'a [String],
    target: &'a ScanTarget,
    auto_ideal: bool,
}

struct CompilationQueueGuard {
    selection_epoch: u64,
    artist_report: Option<(String, u64)>,
}

impl CompilationQueueGuard {
    fn capture(app: &ApplicationState) -> Self {
        let artist_report =
            (app.library.view == crate::application::LibraryView::Artists
                && !app.library.show_tracks
                && !app.compilation_report_artist.is_empty()
                && app.library.focused_artist.as_deref().is_some_and(|artist| {
                    artist.eq_ignore_ascii_case(&app.compilation_report_artist)
                }))
            .then(|| {
                (
                    app.compilation_report_artist.clone(),
                    app.compilation_report_generation,
                )
            });
        Self {
            selection_epoch: app.library.selection_epoch,
            artist_report,
        }
    }

    fn allows(&self, app: &ApplicationState, target: &ScanTarget) -> bool {
        let Some(track) = target.compilation_track_path.as_deref() else {
            return true;
        };
        if app.library.selection_epoch != self.selection_epoch
            || !app
                .library
                .selected_compilation_track_paths
                .contains(&track.to_ascii_lowercase())
        {
            return false;
        }
        if let Some((artist, generation)) = self.artist_report.as_ref() {
            // A report queues several albums without focusing one album card.
            // Keep its Artist and generation as the authority for the whole run.
            app.library.view == crate::application::LibraryView::Artists
                && !app.library.show_tracks
                && app
                    .library
                    .focused_artist
                    .as_deref()
                    .is_some_and(|focused| focused.eq_ignore_ascii_case(artist))
                && app.compilation_report_artist.eq_ignore_ascii_case(artist)
                && app.compilation_report_generation == *generation
        } else {
            app.library
                .focused_album_key
                .as_deref()
                .is_some_and(|key| key.eq_ignore_ascii_case(&target.album_key))
        }
    }
}

fn musicbrainz_review_target(state: &ApplicationState) -> Result<ScanTarget, String> {
    let album_key = state
        .library
        .focused_album_key
        .as_deref()
        .ok_or_else(|| "Select an Artist or Album before opening MusicBrainz.".to_string())?;
    let album = state
        .library
        .albums
        .iter()
        .find(|album| album.snapshot.album_key.eq_ignore_ascii_case(album_key))
        .ok_or_else(|| "The selected album is no longer available.".to_string())?;
    let compilation_track_path = if album.snapshot.album_mbid.trim().is_empty() {
        // An explicitly selected fallback track is the user's authority. Do not
        // silently replace it with the first incomplete track in the compilation;
        // that can search and preview artwork for an entirely different artist.
        album
            .snapshot
            .compilation_tracks
            .iter()
            .find(|track| {
                state
                    .library
                    .selected_compilation_track_paths
                    .contains(&track.path.to_ascii_lowercase())
            })
            .or_else(|| {
                album
                    .snapshot
                    .compilation_tracks
                    .iter()
                    .find(|track| !track.embedded_artwork_recorded)
            })
            .or_else(|| album.snapshot.compilation_tracks.first())
            .map(|track| track.path.clone())
            .or_else(|| {
                (!album.snapshot.representative_file.is_empty())
                    .then(|| album.snapshot.representative_file.clone())
            })
    } else {
        None
    };
    Ok(ScanTarget {
        album_key: album.snapshot.album_key.clone(),
        artist: album.snapshot.artist.clone(),
        album: album.snapshot.title.clone(),
        path: album.snapshot.path.clone(),
        compilation_track_path,
        bypass_override: album.bypass_override,
    })
}

impl ScanController {
    pub fn new(state: Arc<Mutex<ApplicationState>>) -> (Self, mpsc::Receiver<ControllerNotice>) {
        Self::new_with_debug(state, None)
    }

    pub fn new_with_debug(
        state: Arc<Mutex<ApplicationState>>,
        debug: Option<ProcessingDebugSink>,
    ) -> (Self, mpsc::Receiver<ControllerNotice>) {
        let (notices, receiver) = mpsc::channel();
        (
            Self {
                state,
                running: Arc::new(AtomicBool::new(false)),
                refreshing: Arc::new(AtomicBool::new(false)),
                musicbrainz_requested: Arc::new(AtomicBool::new(false)),
                notices,
                debug,
            },
            receiver,
        )
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn start(&self) -> Result<(), String> {
        self.start_internal(false)
    }

    pub fn open_musicbrainz(&self) -> Result<(), String> {
        if self.is_running() {
            let (phase, back_available) = {
                let state = self
                    .state
                    .lock()
                    .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
                (state.phase, state.musicbrainz_back_available)
            };
            return match phase {
                RunPhase::AwaitingCandidate => self.submit_decision(serde_json::json!({
                    "action": if back_available { "back_musicbrainz" } else { "retry_musicbrainz" }
                })),
                RunPhase::AwaitingMusicBrainz => Ok(()),
                RunPhase::Running => {
                    self.musicbrainz_requested.store(true, Ordering::SeqCst);
                    gui_events::set_musicbrainz_requested(true);
                    if let Ok(mut state) = self.state.lock() {
                        state.operation_status =
                            "MusicBrainz will open after the current search…".to_string();
                        state.push_log(
                            "MusicBrainz results requested for the current album.".to_string(),
                        );
                    }
                    let _ = self.notices.send(ControllerNotice::StateChanged);
                    Ok(())
                }
                _ => {
                    Err("The active scan is stopping; wait before opening MusicBrainz.".to_string())
                }
            };
        }
        self.start_internal(true)
    }

    fn start_internal(&self, open_musicbrainz: bool) -> Result<(), String> {
        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("A SPLINED run is already active.".to_string());
        }
        if self.refreshing.load(Ordering::SeqCst) {
            self.running.store(false, Ordering::SeqCst);
            return Err("Wait for Refresh DB to finish before starting a run.".to_string());
        }

        let (base_config, queue, launch_mode, auto_ideal, compilation_guard) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
            let queue = if open_musicbrainz {
                match musicbrainz_review_target(&state) {
                    Ok(target) => vec![target],
                    Err(error) => {
                        self.running.store(false, Ordering::SeqCst);
                        return Err(error);
                    }
                }
            } else {
                state.launch_targets()
            };
            if queue.is_empty() {
                self.running.store(false, Ordering::SeqCst);
                return Err("Select at least one eligible album before launching.".to_string());
            }
            state.phase = RunPhase::Running;
            state.last_error = None;
            state.run_total = queue.len();
            state.run_completed = 0;
            state.run_failed = 0;
            state.operation_busy = true;
            state.operation_status = if open_musicbrainz {
                "Preparing MusicBrainz results for the selected album…".to_string()
            } else {
                "Preparing selected library targets…".to_string()
            };
            state.operation_tone = "cyan".to_string();
            let launch_label = match state.launch_mode {
                crate::application::LaunchMode::Read => "READ",
                crate::application::LaunchMode::LiveWrite => "LIVE WRITE",
            };
            state.push_log(format!(
                "Starting {} album{} in {} mode.",
                queue.len(),
                if queue.len() == 1 { "" } else { "s" },
                launch_label
            ));
            let mut config = state.config.clone();
            if open_musicbrainz {
                // An explicit MusicBrainz click is a one-run override, not a
                // persisted change to the user's automatic source policy.
                config.musicbrainz.enabled = true;
            }
            (
                config,
                queue,
                state.launch_mode,
                !open_musicbrainz && state.auto_scan_enabled,
                CompilationQueueGuard::capture(&state),
            )
        };
        self.musicbrainz_requested
            .store(open_musicbrainz, Ordering::SeqCst);
        debug_record(
            &self.debug,
            format!(
                "processing-start mode={} albums={}",
                match launch_mode {
                    crate::application::LaunchMode::Read => "read",
                    crate::application::LaunchMode::LiveWrite => "live-write",
                },
                queue.len()
            ),
        );
        let _ = self.notices.send(ControllerNotice::StateChanged);

        let state = Arc::clone(&self.state);
        let running = Arc::clone(&self.running);
        let musicbrainz_requested = Arc::clone(&self.musicbrainz_requested);
        let notices = self.notices.clone();
        let debug = self.debug.clone();
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    finish_with_error(
                        &state,
                        &running,
                        &notices,
                        &debug,
                        format!("Unable to start the SPLINED runtime: {error}"),
                    );
                    return;
                }
            };

            for target in queue {
                if gui_events::cancelled() {
                    break;
                }
                if let Some(track_path) = target.compilation_track_path.as_deref() {
                    let still_selected = state
                        .lock()
                        .is_ok_and(|app| compilation_guard.allows(&app, &target));
                    if !still_selected {
                        debug_record(
                            &debug,
                            format!(
                                "processing-comp-queue-aborted selection-changed album_key={} next_track={}",
                                quoted(&target.album_key),
                                quoted(track_path)
                            ),
                        );
                        if let Ok(mut app) = state.lock() {
                            app.push_log("COMP track list stopped because the selection changed; completed tracks remain green.".to_string());
                        }
                        break;
                    }
                }
                debug_record(
                    &debug,
                    format!(
                        "processing-target-start album_key={} path={} compilation_track={}",
                        quoted(&target.album_key),
                        quoted(&target.path),
                        quoted(target.compilation_track_path.as_deref().unwrap_or(""))
                    ),
                );
                let mut config = base_config.clone();
                config.mode = launch_mode.config_mode();
                config.scan.scan_library_dir = target.path.clone();
                let sources = match resolve_sources(
                    &config.sources,
                    &config.source_policies,
                    None,
                    None,
                    &[],
                ) {
                    Ok(sources) => sources,
                    Err(error) => {
                        finish_with_error(&state, &running, &notices, &debug, error);
                        return;
                    }
                };

                if let Err(error) = run_target(
                    &runtime,
                    &state,
                    &notices,
                    TargetRunRequest {
                        config: &config,
                        sources: &sources,
                        target: &target,
                        auto_ideal,
                    },
                    debug.clone(),
                    Arc::clone(&musicbrainz_requested),
                ) {
                    debug_record(
                        &debug,
                        format!(
                            "processing-target-error album_key={} error={}",
                            quoted(&target.album_key),
                            quoted(&error)
                        ),
                    );
                    if !gui_events::cancelled()
                        && let Ok(mut app) = state.lock()
                    {
                        app.run_failed = app.run_failed.saturating_add(1);
                        app.last_error = Some(error.clone());
                        app.push_log(format!("ERROR: {error}"));
                    }
                    break;
                }
                debug_record(
                    &debug,
                    format!(
                        "processing-target-finished album_key={} path={}",
                        quoted(&target.album_key),
                        quoted(&target.path)
                    ),
                );

                if let Ok(mut app) = state.lock() {
                    app.library.mark_target_completed(&target);
                    if let Ok(snapshot) = media_snapshot(&base_config, false) {
                        let _ = app.library.replace_snapshot(snapshot);
                    }
                }
                let _ = notices.send(ControllerNotice::StateChanged);
            }

            gui_events::end_in_process();
            let cancelled = gui_events::cancelled();
            gui_events::reset_cancel();
            if let Ok(mut app) = state.lock() {
                app.phase = RunPhase::Idle;
                app.candidates.clear();
                app.selected_candidate = None;
                if cancelled {
                    app.push_log("SPLINED run stopped by the operator.".to_string());
                } else if app.last_error.is_none() {
                    app.push_log("SPLINED run completed.".to_string());
                }
                app.service_status = if cancelled {
                    format!(
                        "Stopped · {} of {} completed",
                        app.run_completed, app.run_total
                    )
                } else if app.run_failed > 0 || app.last_error.is_some() {
                    format!(
                        "Run stopped · {} completed · {} failed",
                        app.run_completed,
                        app.run_failed.max(1)
                    )
                } else {
                    format!("Run complete · {} completed", app.run_completed)
                };
                app.operation_busy = false;
                app.operation_status = app.service_status.clone();
                app.operation_tone = if cancelled || app.last_error.is_some() {
                    "red".to_string()
                } else {
                    "green".to_string()
                };
                // A failed search is recoverable operator context, not a
                // completed run. Keep the focused Artist/Album and current
                // cover visible so the user can retry or choose another
                // action instead of landing on an empty Artist Overview.
                if app.last_error.is_none() && app.run_failed == 0 {
                    app.clear_completed_workspace(cancelled);
                } else {
                    app.completion_message.clear();
                    app.completion_dialog_visible = false;
                }
            }
            let (completed, failed) = state
                .lock()
                .map(|app| (app.run_completed, app.run_failed))
                .unwrap_or_default();
            debug_record(
                &debug,
                format!(
                    "processing-finished cancelled={cancelled} completed={completed} failed={failed}"
                ),
            );
            running.store(false, Ordering::SeqCst);
            musicbrainz_requested.store(false, Ordering::SeqCst);
            let _ = notices.send(ControllerNotice::RunFinished);
        });

        Ok(())
    }

    pub fn stop(&self) {
        if self.is_running() {
            debug_record(&self.debug, "processing-stop-requested".to_string());
            gui_events::request_cancel();
            if let Ok(mut state) = self.state.lock() {
                state.phase = RunPhase::Stopping;
                state.operation_busy = true;
                state.operation_status =
                    "Stopping after the active operation releases…".to_string();
                state.operation_tone = "orange".to_string();
                state.push_log("Stopping after the active operation releases…".to_string());
            }
            let _ = self.notices.send(ControllerNotice::StateChanged);
        }
    }

    pub fn submit_decision(&self, decision: Value) -> Result<(), String> {
        let action = decision
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        debug_record(
            &self.debug,
            format!("processing-decision action={}", quoted(&action)),
        );
        gui_events::submit_decision(decision)?;
        if let Ok(mut state) = self.state.lock() {
            state.phase = RunPhase::Running;
            state.operation_busy = true;
            state.operation_status = match action.as_str() {
                "use" => "Saving selected artwork…".to_string(),
                "bypass" => "Saving album bypass…".to_string(),
                "skip_album" => "Skipping album and preparing the next target…".to_string(),
                "retry" | "retry_musicbrainz" => "Searching MusicBrainz again…".to_string(),
                _ => "Applying the selected artwork decision…".to_string(),
            };
            state.operation_tone = if action == "bypass" || action == "skip_album" {
                "orange".to_string()
            } else {
                "magenta".to_string()
            };
            state.push_log("Operator decision submitted.".to_string());
        }
        let _ = self.notices.send(ControllerNotice::StateChanged);
        Ok(())
    }

    pub fn write_selected_musicbrainz(&self, decision: Value) -> Result<(), String> {
        let ready = self
            .state
            .lock()
            .map_err(|_| "The SPLINED application state is unavailable.".to_string())?
            .phase
            == RunPhase::AwaitingMusicBrainz;
        if !ready {
            return Err("A MusicBrainz cover is not ready to write.".to_string());
        }
        debug_record(
            &self.debug,
            "processing-musicbrainz-write-requested".to_string(),
        );
        self.submit_decision(decision)
    }

    pub fn refresh_library(&self, full_refresh: bool) -> Result<(), String> {
        if self
            .refreshing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("Refresh DB is already in progress.".to_string());
        }
        let result = (|| {
            if self.is_running() {
                return Err("The media library cannot be rebuilt during an active run.".to_string());
            }
            let config = self
                .state
                .lock()
                .map_err(|_| "The SPLINED application state is unavailable.".to_string())?
                .config
                .clone();
            let snapshot = if full_refresh {
                media_snapshot(&config, true)?
            } else {
                media_snapshot_incremental(&config)?
            };
            self.state
                .lock()
                .map_err(|_| "The SPLINED application state is unavailable.".to_string())?
                .library
                .replace_snapshot(snapshot)?;
            let _ = self.notices.send(ControllerNotice::StateChanged);
            Ok(())
        })();
        self.refreshing.store(false, Ordering::SeqCst);
        result
    }
}

fn run_target(
    runtime: &tokio::runtime::Runtime,
    state: &Arc<Mutex<ApplicationState>>,
    notices: &mpsc::Sender<ControllerNotice>,
    request: TargetRunRequest<'_>,
    debug: Option<ProcessingDebugSink>,
    musicbrainz_requested: Arc<AtomicBool>,
) -> Result<(), String> {
    let (event_sender, event_receiver) = mpsc::channel::<Value>();
    gui_events::begin_in_process(event_sender, true, request.auto_ideal)?;
    gui_events::set_musicbrainz_requested(musicbrainz_requested.load(Ordering::SeqCst));
    gui_events::set_bypass_overrides(
        request
            .target
            .bypass_override
            .then(|| PathBuf::from(&request.target.path)),
    );
    let focused_artist = state
        .lock()
        .ok()
        .and_then(|app| app.library.focused_artist.clone());
    gui_events::set_scan_context(ScanContext {
        compilation_track_path: request
            .target
            .compilation_track_path
            .as_ref()
            .map(PathBuf::from),
        fallback_album: None,
        fallback_artist: focused_artist,
        indexed_album_path: Some(PathBuf::from(&request.target.path)),
        indexed_album_key: Some(request.target.album_key.clone()),
    });

    let relay_state = Arc::clone(state);
    let relay_notices = notices.clone();
    let relay_debug = debug.clone();
    let relay = std::thread::spawn(move || {
        while let Ok(event) = event_receiver.recv() {
            debug_record(&relay_debug, runtime_event_summary(&event));
            if let Ok(mut app) = relay_state.lock() {
                app.apply_runtime_event(&event);
            }
            match event.get("event").and_then(Value::as_str) {
                Some("musicbrainz_matches") => {
                    musicbrainz_requested.store(false, Ordering::SeqCst);
                    gui_events::set_musicbrainz_requested(false);
                }
                Some("album_completed" | "album_skipped" | "album_postponed" | "album_error") => {
                    musicbrainz_requested.store(false, Ordering::SeqCst);
                    gui_events::set_musicbrainz_requested(false);
                }
                Some("decision_required")
                    if musicbrainz_requested.swap(false, Ordering::SeqCst) =>
                {
                    gui_events::set_musicbrainz_requested(false);
                    let action = if event
                        .get("musicbrainz_back_available")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        "back_musicbrainz"
                    } else {
                        "retry_musicbrainz"
                    };
                    let result = gui_events::submit_decision(serde_json::json!({"action": action}));
                    debug_record(
                        &relay_debug,
                        format!(
                            "processing-musicbrainz-request action={} result={}",
                            quoted(action),
                            quoted(if result.is_ok() { "queued" } else { "failed" })
                        ),
                    );
                }
                _ => {}
            }
            let _ = relay_notices.send(ControllerNotice::StateChanged);
        }
    });

    let outcome = runtime.block_on(run_scan_library_read_report(
        request.config,
        request.sources,
    ));
    gui_events::end_in_process();
    let _ = relay.join();
    match outcome {
        Ok(summary) if summary.failed == 0 => {
            debug_record(
                &debug,
                format!(
                    "processing-runtime-finished albums={} resolved={} unresolved={} failed={} installed={} unchanged={} read_only={}",
                    summary.albums,
                    summary.resolved,
                    summary.unresolved,
                    summary.failed,
                    summary.installed,
                    summary.unchanged,
                    summary.read_only
                ),
            );
            Ok(())
        }
        Ok(summary) => {
            debug_record(
                &debug,
                format!(
                    "processing-runtime-finished albums={} resolved={} unresolved={} failed={} installed={} unchanged={} read_only={}",
                    summary.albums,
                    summary.resolved,
                    summary.unresolved,
                    summary.failed,
                    summary.installed,
                    summary.unchanged,
                    summary.read_only
                ),
            );
            Err(format!(
                "Processing failed for {} album operation{}.",
                summary.failed,
                if summary.failed == 1 { "" } else { "s" }
            ))
        }
        Err(error) => {
            debug_record(
                &debug,
                format!("processing-runtime-error error={}", quoted(&error)),
            );
            Err(error)
        }
    }
}

fn finish_with_error(
    state: &Arc<Mutex<ApplicationState>>,
    running: &Arc<AtomicBool>,
    notices: &mpsc::Sender<ControllerNotice>,
    debug: &Option<ProcessingDebugSink>,
    error: String,
) {
    debug_record(
        debug,
        format!("processing-controller-error error={}", quoted(&error)),
    );
    gui_events::end_in_process();
    gui_events::reset_cancel();
    if let Ok(mut state) = state.lock() {
        state.phase = RunPhase::Idle;
        state.run_failed = state.run_failed.saturating_add(1);
        state.last_error = Some(error.clone());
        state.operation_busy = false;
        state.operation_status = format!("Processing error · {error}");
        state.operation_tone = "red".to_string();
        state.push_log(format!("ERROR: {error}"));
    }
    running.store(false, Ordering::SeqCst);
    let _ = notices.send(ControllerNotice::RunFinished);
}

fn debug_record(debug: &Option<ProcessingDebugSink>, event: String) {
    if let Some(debug) = debug {
        debug(event);
    }
}

fn quoted(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"<unavailable>\"".to_string())
}

fn runtime_event_summary(event: &Value) -> String {
    let name = event
        .get("event")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let mut fields = vec![format!("name={}", quoted(name))];
    for key in [
        "album_path",
        "track_path",
        "artist",
        "title",
        "album_artist",
        "album",
        "source",
        "status",
        "action",
        "reason",
        "message",
    ] {
        if let Some(value) = event.get(key).and_then(Value::as_str)
            && !value.trim().is_empty()
        {
            fields.push(format!("{key}={}", quoted(value)));
        }
    }
    if let Some(items) = event.get("items").and_then(Value::as_array) {
        fields.push(format!("items={}", items.len()));
    }
    if name == "compilation_track_completed" {
        for key in ["width", "height"] {
            if let Some(value) = event.get(key).and_then(Value::as_u64) {
                fields.push(format!("{key}={value}"));
            }
        }
        if let Some(value) = event.get("embed_thumbnails").and_then(Value::as_bool) {
            fields.push(format!("embed_thumbnails={value}"));
        }
        if let Some(value) = event.get("upscale_backend").and_then(Value::as_str) {
            fields.push(format!("upscale_backend={}", quoted(value)));
        }
    }
    format!("processing-event {}", fields.join(" "))
}

impl Drop for ScanController {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{LaunchMode, ScanScope};
    use crate::config::Config;
    use crate::media_database::{MediaAlbumSnapshot, MediaSnapshot, MediaTrackSnapshot};

    fn state_without_selection() -> ApplicationState {
        let snapshot = MediaSnapshot {
            schema_version: crate::media_database::SCHEMA_VERSION,
            database_path: String::new(),
            canonical_root: String::new(),
            local_root: String::new(),
            albums: vec![MediaAlbumSnapshot {
                album_key: "album".to_string(),
                artist: "Artist".to_string(),
                tagged_artist: "Artist".to_string(),
                title: "Album".to_string(),
                path: r"C:\missing\album".to_string(),
                representative_file: String::new(),
                status: "processed".to_string(),
                compilation: false,
                compilation_track_art_eligible: false,
                compilation_tracks: Vec::new(),
                track_count: 0,
                compilation_completed_tracks: 0,
                release_year: String::new(),
                artist_mbid: String::new(),
                album_mbid: "release-album".to_string(),
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
            }],
        };
        let mut state = ApplicationState::new(Config::default(), snapshot).unwrap();
        state.scan_scope = ScanScope::Selected;
        state.launch_mode = LaunchMode::Read;
        state
    }

    #[test]
    fn launch_is_rejected_without_a_valid_queue() {
        let state = Arc::new(Mutex::new(state_without_selection()));
        let (controller, _notices) = ScanController::new(state);
        let error = controller.start().unwrap_err();
        assert!(error.contains("Select at least one"));
        assert!(!controller.is_running());
    }

    #[test]
    fn artist_report_queue_can_cross_albums_and_still_cancels_on_navigation() {
        let mut app = state_without_selection();
        app.library.view = crate::application::LibraryView::Artists;
        app.library.focused_artist = Some("Alan Jackson".into());
        app.library.focused_album_key = None;
        app.compilation_report_artist = "Alan Jackson".into();
        app.compilation_report_generation = 7;
        let target = |key: &str, track: &str| ScanTarget {
            album_key: key.into(),
            artist: "Various Artists".into(),
            album: key.into(),
            path: format!("C:/missing/{key}"),
            compilation_track_path: Some(track.into()),
            bypass_override: false,
        };
        let first = target("first", "C:/missing/first/one.mp3");
        let second = target("second", "C:/missing/second/two.mp3");
        for target in [&first, &second] {
            app.library.selected_compilation_track_paths.insert(
                target
                    .compilation_track_path
                    .as_ref()
                    .unwrap()
                    .to_ascii_lowercase(),
            );
        }
        let guard = CompilationQueueGuard::capture(&app);
        assert!(guard.allows(&app, &first));
        app.library.mark_target_completed(&first);
        assert!(guard.allows(&app, &second));
        app.library.selection_epoch += 1;
        assert!(!guard.allows(&app, &second));
        app.library.selection_epoch -= 1;
        app.compilation_report_generation += 1;
        assert!(!guard.allows(&app, &second));
        app.compilation_report_generation -= 1;
        app.library.focused_artist = Some("Aerosmith".into());
        assert!(!guard.allows(&app, &second));
        app.compilation_report_artist.clear();
        app.library.focused_album_key = Some("first".into());
        let single_album = CompilationQueueGuard::capture(&app);
        assert!(!single_album.allows(&app, &second));
        app.library.focused_album_key = Some("second".into());
        assert!(single_album.allows(&app, &second));
    }

    #[test]
    fn artist_report_launch_reaches_processing_without_a_focused_album() {
        let mut app = state_without_selection();
        let track = "C:/missing/album/one.mp3";
        app.library.albums[0].snapshot.album_mbid.clear();
        app.library.albums[0].snapshot.compilation_tracks = vec![MediaTrackSnapshot {
            path: track.into(),
            title: "One".into(),
            artist: "Alan Jackson".into(),
            embedded_artwork_recorded: false,
        }];
        app.library.albums[0].selected = true;
        app.library.focused_artist = Some("Alan Jackson".into());
        app.library.focused_album_key = None;
        app.compilation_report_artist = "Alan Jackson".into();
        app.library
            .selected_compilation_track_paths
            .insert(track.to_ascii_lowercase());
        let state = Arc::new(Mutex::new(app));
        let events = Arc::new(Mutex::new(Vec::<String>::new()));
        let captured = Arc::clone(&events);
        let (controller, notices) = ScanController::new_with_debug(
            Arc::clone(&state),
            Some(Arc::new(move |event| captured.lock().unwrap().push(event))),
        );
        controller.start().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while controller.is_running() && std::time::Instant::now() < deadline {
            let _ = notices.recv_timeout(std::time::Duration::from_millis(100));
        }
        assert!(!controller.is_running());
        let events = events.lock().unwrap();
        assert!(
            events
                .iter()
                .any(|event| event.contains("processing-target-start"))
        );
        assert!(
            !events
                .iter()
                .any(|event| event.contains("processing-comp-queue-aborted"))
        );
    }

    #[test]
    fn musicbrainz_requires_album_context_without_claiming_a_run() {
        let state = Arc::new(Mutex::new(state_without_selection()));
        let (controller, _notices) = ScanController::new(state);
        let error = controller.open_musicbrainz().unwrap_err();
        assert!(error.contains("Select an Artist or Album"));
        assert!(!controller.is_running());
    }

    #[test]
    fn musicbrainz_uses_the_explicitly_selected_compilation_track() {
        let mut app = state_without_selection();
        let ace_of_base = r"C:\Music\Various Artists\Billboard 1994\01 - Ace of Base.mp3";
        let ten_thousand_maniacs =
            r"C:\Music\Various Artists\Billboard 1994\40 - 10,000 Maniacs.mp3";
        app.library.albums[0].snapshot.album_mbid.clear();
        app.library.albums[0].snapshot.compilation = true;
        app.library.albums[0].snapshot.compilation_tracks = vec![
            MediaTrackSnapshot {
                path: ace_of_base.to_string(),
                title: "The Sign".to_string(),
                artist: "Ace of Base".to_string(),
                embedded_artwork_recorded: false,
            },
            MediaTrackSnapshot {
                path: ten_thousand_maniacs.to_string(),
                title: "Because the Night".to_string(),
                artist: "10,000 Maniacs".to_string(),
                embedded_artwork_recorded: false,
            },
        ];
        assert_eq!(
            app.library
                .toggle_compilation_track("album", ten_thousand_maniacs, false,),
            crate::application::SelectionResult::Changed
        );

        let target = musicbrainz_review_target(&app).unwrap();

        assert_eq!(
            target.compilation_track_path.as_deref(),
            Some(ten_thousand_maniacs)
        );
    }

    #[test]
    fn musicbrainz_can_review_a_focused_processed_album_without_selection() {
        let state = Arc::new(Mutex::new(state_without_selection()));
        {
            let mut app = state.lock().unwrap();
            assert!(app.library.focus_album("album"));
            assert!(app.library.queue(ScanScope::Selected).is_empty());
        }
        let (controller, notices) = ScanController::new(Arc::clone(&state));
        controller.open_musicbrainz().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while controller.is_running() && std::time::Instant::now() < deadline {
            let _ = notices.recv_timeout(std::time::Duration::from_millis(100));
        }
        assert!(!controller.is_running());
        let app = state.lock().unwrap();
        assert_eq!(app.run_total, 1);
        assert_eq!(app.library.focused_album_key.as_deref(), Some("album"));
    }

    #[test]
    fn musicbrainz_request_during_search_is_queued() {
        let state = Arc::new(Mutex::new(state_without_selection()));
        let (controller, _notices) = ScanController::new(Arc::clone(&state));
        controller.running.store(true, Ordering::SeqCst);
        state.lock().unwrap().phase = RunPhase::Running;
        controller.open_musicbrainz().unwrap();
        assert!(controller.musicbrainz_requested.load(Ordering::SeqCst));
        assert!(
            state
                .lock()
                .unwrap()
                .operation_status
                .contains("MusicBrainz")
        );
        controller.running.store(false, Ordering::SeqCst);
        gui_events::set_musicbrainz_requested(false);
    }

    #[test]
    fn stopping_an_idle_controller_is_safe() {
        let state = Arc::new(Mutex::new(state_without_selection()));
        let (controller, _notices) = ScanController::new(state);
        controller.stop();
        assert!(!controller.is_running());
    }

    #[test]
    fn processing_debug_summary_keeps_actionable_runtime_fields() {
        let summary = runtime_event_summary(&serde_json::json!({
            "event": "album_error",
            "album_path": r"N:\\media\\music\\Artist\\Album",
            "source": "Discogs",
            "status": "Failed",
            "message": "provider timeout"
        }));
        assert!(summary.contains("name=\"album_error\""));
        assert!(summary.contains("source=\"Discogs\""));
        assert!(summary.contains("status=\"Failed\""));
        assert!(summary.contains("message=\"provider timeout\""));
        let track = runtime_event_summary(&serde_json::json!({
            "event": "compilation_track_completed",
            "track_path": "N:/music/comp/one.mp3",
            "width": 1200,
            "height": 1200,
            "embed_thumbnails": true,
            "upscale_backend": "lanczos"
        }));
        assert!(track.contains("width=1200 height=1200 embed_thumbnails=true"));
        assert!(track.contains("upscale_backend=\"lanczos\""));
    }

    #[test]
    fn musicbrainz_direct_write_uses_the_previewed_cover_and_is_explicit() {
        let directory = tempfile::tempdir().unwrap();
        let cover = directory.path().join("caa.jpg");
        std::fs::write(&cover, b"preview").unwrap();
        let mut state = state_without_selection();
        state.apply_runtime_event(&serde_json::json!({
            "event": "musicbrainz_matches",
            "items": [{"index": 2, "release_mbid": "release", "release_title": "Album"}]
        }));
        state.selected_musicbrainz_result = Some(2);
        state.musicbrainz_cover_preview_path = cover.to_string_lossy().into_owned();
        state.phase = RunPhase::AwaitingMusicBrainz;
        let decision = state.write_musicbrainz_preview_decision().unwrap();
        assert_eq!(decision["action"], "write_musicbrainz_preview");
        assert_eq!(decision["index"], 2);
        assert_eq!(decision["cover_path"], cover.to_string_lossy().as_ref());
    }

    #[test]
    fn stop_requests_native_cancellation_and_enters_stopping_phase() {
        let state = Arc::new(Mutex::new(state_without_selection()));
        let (controller, _notices) = ScanController::new(Arc::clone(&state));
        controller.running.store(true, Ordering::SeqCst);
        gui_events::reset_cancel();
        controller.stop();
        assert!(gui_events::cancelled());
        assert_eq!(state.lock().unwrap().phase, RunPhase::Stopping);
        controller.running.store(false, Ordering::SeqCst);
        gui_events::reset_cancel();
    }

    #[test]
    fn failed_album_remains_selected_and_the_controller_recovers() {
        let state = Arc::new(Mutex::new(state_without_selection()));
        {
            let mut app = state.lock().unwrap();
            assert_eq!(
                app.library.toggle_album("album", false),
                crate::application::SelectionResult::Changed
            );
            app.library.focused_artist = Some("Artist".to_string());
            app.library.focused_album_key = Some("album".to_string());
            app.current_artist = "Artist".to_string();
            app.current_album = "Album".to_string();
            app.current_album_path = r"C:\missing\album".to_string();
        }
        let (controller, notices) = ScanController::new(Arc::clone(&state));
        controller.start().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while controller.is_running() && std::time::Instant::now() < deadline {
            let _ = notices.recv_timeout(std::time::Duration::from_millis(100));
        }
        assert!(!controller.is_running(), "missing-path run did not finish");
        let app = state.lock().unwrap();
        assert_eq!(app.phase, RunPhase::Idle);
        assert!(app.last_error.is_some());
        assert_eq!(app.run_failed, 1);
        assert!(app.library.albums[0].selected);
        assert_eq!(app.library.focused_artist.as_deref(), Some("Artist"));
        assert_eq!(app.library.focused_album_key.as_deref(), Some("album"));
        assert_eq!(app.current_artist, "Artist");
        assert_eq!(app.current_album, "Album");
        assert_eq!(app.current_album_path, r"C:\missing\album");
    }
}
