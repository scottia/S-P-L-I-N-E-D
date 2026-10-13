//! In-process Rust scan orchestration for the Windows application.

use crate::application::{ApplicationState, RunPhase, ScanTarget};
use crate::config::resolve_sources;
use crate::gui_events::{self, ScanContext};
use crate::media_database::media_snapshot;
use crate::scan_runtime::run_scan_library_read_report;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControllerNotice {
    StateChanged,
    RunFinished,
}

pub struct ScanController {
    state: Arc<Mutex<ApplicationState>>,
    running: Arc<AtomicBool>,
    notices: mpsc::Sender<ControllerNotice>,
}

impl ScanController {
    pub fn new(state: Arc<Mutex<ApplicationState>>) -> (Self, mpsc::Receiver<ControllerNotice>) {
        let (notices, receiver) = mpsc::channel();
        (
            Self {
                state,
                running: Arc::new(AtomicBool::new(false)),
                notices,
            },
            receiver,
        )
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn start(&self) -> Result<(), String> {
        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("A SPLINED run is already active.".to_string());
        }

        let (base_config, queue, launch_mode) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
            let queue = state.library.queue(state.scan_scope);
            if queue.is_empty() {
                self.running.store(false, Ordering::SeqCst);
                return Err("Select at least one eligible album before launching.".to_string());
            }
            state.phase = RunPhase::Running;
            state.last_error = None;
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
            (state.config.clone(), queue, state.launch_mode)
        };
        let _ = self.notices.send(ControllerNotice::StateChanged);

        let state = Arc::clone(&self.state);
        let running = Arc::clone(&self.running);
        let notices = self.notices.clone();
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
                        format!("Unable to start the SPLINED runtime: {error}"),
                    );
                    return;
                }
            };

            for target in queue {
                if gui_events::cancelled() {
                    break;
                }
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
                        finish_with_error(&state, &running, &notices, error);
                        return;
                    }
                };

                if let Err(error) =
                    run_target(&runtime, &state, &notices, &config, &sources, &target)
                {
                    if !gui_events::cancelled()
                        && let Ok(mut app) = state.lock()
                    {
                        app.last_error = Some(error.clone());
                        app.push_log(format!("ERROR: {error}"));
                    }
                    break;
                }

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
            }
            running.store(false, Ordering::SeqCst);
            let _ = notices.send(ControllerNotice::RunFinished);
        });

        Ok(())
    }

    pub fn stop(&self) {
        if self.is_running() {
            gui_events::request_cancel();
            if let Ok(mut state) = self.state.lock() {
                state.phase = RunPhase::Stopping;
                state.push_log("Stopping after the active operation releases…".to_string());
            }
            let _ = self.notices.send(ControllerNotice::StateChanged);
        }
    }

    pub fn submit_decision(&self, decision: Value) -> Result<(), String> {
        gui_events::submit_decision(decision)?;
        if let Ok(mut state) = self.state.lock() {
            state.phase = RunPhase::Running;
            state.push_log("Operator decision submitted.".to_string());
        }
        let _ = self.notices.send(ControllerNotice::StateChanged);
        Ok(())
    }

    pub fn refresh_library(&self, refresh: bool) -> Result<(), String> {
        if self.is_running() {
            return Err("The media library cannot be rebuilt during an active run.".to_string());
        }
        let config = self
            .state
            .lock()
            .map_err(|_| "The SPLINED application state is unavailable.".to_string())?
            .config
            .clone();
        let snapshot = media_snapshot(&config, refresh)?;
        self.state
            .lock()
            .map_err(|_| "The SPLINED application state is unavailable.".to_string())?
            .library
            .replace_snapshot(snapshot)?;
        let _ = self.notices.send(ControllerNotice::StateChanged);
        Ok(())
    }
}

fn run_target(
    runtime: &tokio::runtime::Runtime,
    state: &Arc<Mutex<ApplicationState>>,
    notices: &mpsc::Sender<ControllerNotice>,
    config: &crate::config::Config,
    sources: &[String],
    target: &ScanTarget,
) -> Result<(), String> {
    let (event_sender, event_receiver) = mpsc::channel::<Value>();
    gui_events::begin_in_process(event_sender, true, false)?;
    gui_events::set_bypass_overrides(target.bypass_override.then(|| PathBuf::from(&target.path)));
    gui_events::set_scan_context(ScanContext {
        compilation_track_path: target.compilation_track_path.as_ref().map(PathBuf::from),
        fallback_album: None,
        fallback_artist: None,
        indexed_album_path: Some(PathBuf::from(&target.path)),
        indexed_album_key: Some(target.album_key.clone()),
    });

    let relay_state = Arc::clone(state);
    let relay_notices = notices.clone();
    let relay = std::thread::spawn(move || {
        while let Ok(event) = event_receiver.recv() {
            if let Ok(mut app) = relay_state.lock() {
                app.apply_runtime_event(&event);
            }
            let _ = relay_notices.send(ControllerNotice::StateChanged);
        }
    });

    let outcome = runtime.block_on(run_scan_library_read_report(config, sources));
    gui_events::end_in_process();
    let _ = relay.join();
    match outcome {
        Ok(summary) if summary.failed == 0 => Ok(()),
        Ok(summary) => Err(format!(
            "Processing failed for {} album operation{}.",
            summary.failed,
            if summary.failed == 1 { "" } else { "s" }
        )),
        Err(error) => Err(error),
    }
}

fn finish_with_error(
    state: &Arc<Mutex<ApplicationState>>,
    running: &Arc<AtomicBool>,
    notices: &mpsc::Sender<ControllerNotice>,
    error: String,
) {
    gui_events::end_in_process();
    gui_events::reset_cancel();
    if let Ok(mut state) = state.lock() {
        state.phase = RunPhase::Idle;
        state.last_error = Some(error.clone());
        state.push_log(format!("ERROR: {error}"));
    }
    running.store(false, Ordering::SeqCst);
    let _ = notices.send(ControllerNotice::RunFinished);
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
    use crate::media_database::{MediaAlbumSnapshot, MediaSnapshot};

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
    fn stopping_an_idle_controller_is_safe() {
        let state = Arc::new(Mutex::new(state_without_selection()));
        let (controller, _notices) = ScanController::new(state);
        controller.stop();
        assert!(!controller.is_running());
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
        assert!(app.library.albums[0].selected);
    }
}
