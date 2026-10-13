#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};
use splined_core::application::{
    ApplicationState, LaunchMode, LibraryView, RunPhase, ScanScope, SelectionResult, StatusFilter,
};
use splined_core::backup::{BackupSelection, export_backup, restore_backup};
use splined_core::candidate::{Candidate, StaticFormat};
use splined_core::config::{Config, Mode, Verbosity};
use splined_core::credential_store::{CredentialEditor, load_editor, save_editor, test_editor};
use splined_core::final_artwork::{
    install_prepared_artwork, prepare_existing_cover_edit, prepare_selected_artwork_edit,
};
use splined_core::inspect::inspect_image;
use splined_core::local_artwork::embedded_candidate;
use splined_core::media_database::{MediaSnapshot, media_snapshot};
use splined_core::range::Range;
use splined_core::safe_write::{replace_binary_file, take_credential_protection_warning};
use splined_core::scan_controller::ScanController;
use splined_core::settings::UiPreferences;
use splined_core::source_policy::{MinimumRangeType, SourcePolicyConfig};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
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
    let (mut state, setup_required, startup_error) = load_application_state();
    let preferences = splined_core::settings::load_ui_preferences().unwrap_or_default();
    apply_ui_preferences(&mut state, &preferences);
    let state = Arc::new(Mutex::new(state));
    if let Some(ref error) = startup_error
        && let Ok(mut state) = state.lock()
    {
        state.last_error = Some(error.clone());
        state.push_log(format!("ERROR: {error}"));
    }

    let (controller, notices) = ScanController::new(Arc::clone(&state));
    let controller = Arc::new(controller);
    let ui = AppWindow::new()?;
    ui.set_setup_required(setup_required);
    if setup_required {
        ui.set_page_index(3);
        ui.set_setup_message(SharedString::from(startup_error.clone().unwrap_or_else(
            || "Config v5 was not found. Complete SPLINED Setup.".to_string(),
        )));
    }
    ui.set_include_bypassed(preferences.include_bypassed);
    if preferences.window_width >= 800 && preferences.window_height >= 600 {
        ui.window().set_size(slint::PhysicalSize::new(
            preferences.window_width,
            preferences.window_height,
        ));
    }
    initialize_service_ui(&ui, &state);
    if let Some(path) = restore_request_from_args(std::env::args_os().skip(1)) {
        ui.set_page_index(2);
        ui.set_backup_path(SharedString::from(path.to_string_lossy().into_owned()));
        if let Ok(mut app) = state.lock() {
            app.service_status = "Backup selected for restore. Review the included categories, then choose Restore selected.".to_string();
        }
    }
    sync_ui(&ui, &state);
    bind_callbacks(&ui, Arc::clone(&state), Arc::clone(&controller));

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

    let result = ui.run();
    let size = ui.window().size();
    let preferences = state
        .lock()
        .map(|state| UiPreferences {
            library_view: state.library.view.label().to_ascii_lowercase(),
            status_filter: state.library.status_filter.label().to_ascii_lowercase(),
            include_bypassed: ui.get_include_bypassed(),
            window_width: size.width,
            window_height: size.height,
        })
        .unwrap_or_default();
    if !ui.get_setup_required()
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
            "folders" => LibraryView::Folders,
            _ => LibraryView::Artists,
        });
    state
        .library
        .set_status_filter(match preferences.status_filter.as_str() {
            "unprocessed" => StatusFilter::Unprocessed,
            "incomplete" => StatusFilter::Incomplete,
            "processed" => StatusFilter::Processed,
            "bypassed" => StatusFilter::Bypassed,
            "partial / timeout" => StatusFilter::PartialTimeout,
            "artist complete" => StatusFilter::ArtistComplete,
            "artist contains bypass" => StatusFilter::ArtistContainsBypass,
            _ => StatusFilter::All,
        });
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
    bind_callbacks(&ui, Arc::clone(&state), Arc::new(controller));

    let visible_artists = ui.get_library_rows().row_count();
    ui.invoke_change_library_view(1);
    let visible_albums = ui.get_library_rows().row_count();
    ui.invoke_change_library_view(2);
    let visible_folders = ui.get_library_rows().row_count();
    ui.invoke_change_library_view(0);

    let artists = ui.get_library_rows();
    let artist = (0..artists.row_count())
        .filter_map(|index| artists.row_data(index))
        .find(|row| row.eligible)
        .ok_or_else(|| "The Slint Artist model has no selectable row.".to_string())?;
    ui.invoke_library_row_activated(artist.key, false);
    let artist_click_selected_albums = ui.get_selected_count();
    ui.invoke_change_library_view(1);
    let artist_navigation_rows = ui.get_library_rows().row_count();
    let album = ui
        .get_library_rows()
        .row_data(0)
        .ok_or_else(|| "The selected Artist produced no Slint Album row.".to_string())?;
    ui.invoke_library_row_activated(album.key, false);
    let current_album_populated = !ui.get_current_artist().trim().is_empty()
        && !ui.get_current_album().trim().is_empty()
        && !ui.get_current_path().trim().is_empty();
    ui.invoke_select_none();
    let select_none_count = ui.get_selected_count();

    Ok(format!(
        "visible_artists={visible_artists}\nvisible_albums={visible_albums}\nvisible_folders={visible_folders}\nartist_click_selected_albums={artist_click_selected_albums}\nartist_navigation_rows={artist_navigation_rows}\ncurrent_album_populated={current_album_populated}\nselect_none_count={select_none_count}\n"
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
            setup.scan.cache_dir.clear();
            setup.scan.temporary_cache_dir.clear();
            setup.scan.log_dir.clear();
            setup.credentials.credential_dir.clear();
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
            first_error.get_or_insert(error);
            empty_snapshot(&config)
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

fn bind_callbacks(
    ui: &AppWindow,
    state: Arc<Mutex<ApplicationState>>,
    controller: Arc<ScanController>,
) {
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_change_library_view(move |value| {
            if let Ok(mut app) = state.lock() {
                app.library.set_view(match value {
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
        ui.on_compilation_track_activated(move |album_key, track_path, include_bypassed| {
            let preview_request = if let Ok(mut app) = state.lock() {
                let result = app.library.toggle_compilation_track(
                    album_key.as_str(),
                    track_path.as_str(),
                    include_bypassed,
                );
                if result != SelectionResult::NotFound {
                    app.synchronize_focused_album();
                }
                match result {
                    SelectionResult::Changed => {
                        let normalized_track = track_path.to_ascii_lowercase();
                        if app
                            .library
                            .selected_compilation_track_paths
                            .contains(&normalized_track)
                        {
                            Some((
                                track_path.to_string(),
                                app.config.scan.temporary_cache_dir.clone(),
                            ))
                        } else {
                            app.edit_preview_path.clear();
                            app.push_log(format!(
                                "Compilation track deselected: {track_path}"
                            ));
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
                            app.edit_preview_path =
                                candidate.path().to_string_lossy().into_owned();
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
                    Err(error) => record_action_result(&state, Err(error)),
                }
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
        ui.on_set_upscale_preview(move |preview| {
            if let Ok(mut app) = state.lock() {
                app.adjustments.preserve_source_resolution = preview;
                app.adjustments.apply_edit_profile = !preview;
                app.adjustments.edit_existing_cover = false;
                if !preview {
                    app.edit_preview_path.clear();
                }
                app.push_log(if preview {
                    "Upscale Preview selected; source resolution will be preserved.".to_string()
                } else {
                    "Apply Upscale selected; the edit profile will be applied on use.".to_string()
                });
            }
            if preview {
                record_action_result(&state, preview_selected_candidate(&state));
            }
            refresh_weak(&weak, &state);
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
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
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
                "Partial / Timeout" => StatusFilter::PartialTimeout,
                "Artist Complete" => StatusFilter::ArtistComplete,
                "Artist Contains Bypass" => StatusFilter::ArtistContainsBypass,
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
        ui.on_library_row_activated(move |key, include_bypassed| {
            if let Ok(mut app) = state.lock() {
                let result = app.library.toggle_row(key.as_str(), include_bypassed);
                if result != SelectionResult::NotFound {
                    app.synchronize_focused_album();
                }
                match result {
                    SelectionResult::Changed => {}
                    SelectionResult::NeedsBypassOverride(album) => app.push_log(format!(
                        "{album} is bypassed. Enable the one-run bypass override to select it."
                    )),
                    SelectionResult::BlockedByTimeout(album) => app.push_log(format!(
                        "{album} is protected by its active processing timeout."
                    )),
                    SelectionResult::NotFound => {
                        app.push_log("The selected library row is no longer available.".to_string())
                    }
                }
            }
            refresh_weak(&weak, &state);
        });
    }
    bind_simple_library_action(ui, &state, |app| app.library.select_all(), "all");
    bind_simple_library_action(ui, &state, |app| app.library.select_filtered(), "filtered");
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
            if let Ok(mut app) = state.lock() {
                app.launch_mode = if currently_read {
                    LaunchMode::LiveWrite
                } else {
                    LaunchMode::Read
                };
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let controller = Arc::clone(&controller);
        ui.on_launch(move || {
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
            if let Ok(mut app) = state.lock() {
                app.select_candidate(index.max(0) as usize);
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let controller = Arc::clone(&controller);
        ui.on_candidate_action(move |action| {
            let decision = state
                .lock()
                .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                .and_then(|app| app.candidate_decision(action.as_str()));
            let result = decision.and_then(|decision| controller.submit_decision(decision));
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let controller = Arc::clone(&controller);
        ui.on_musicbrainz_selected(move |index| {
            let decision = state
                .lock()
                .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                .and_then(|app| app.musicbrainz_result_decision(index.max(0) as usize));
            let result = decision.and_then(|decision| controller.submit_decision(decision));
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        let controller = Arc::clone(&controller);
        ui.on_musicbrainz_apply_authority(move || {
            let decision = state
                .lock()
                .map_err(|_| "The SPLINED application state is unavailable.".to_string())
                .and_then(|app| app.musicbrainz_authority_decision());
            let result = decision.and_then(|decision| controller.submit_decision(decision));
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_adjustment_step(move |name, delta| {
            if let Ok(mut app) = state.lock() {
                step_adjustment(&mut app, name.as_str(), delta);
                app.edit_preview_path.clear();
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = Arc::clone(&state);
        let weak = ui.as_weak();
        ui.on_edit_existing_cover(move |apply| {
            let result = edit_existing_cover(&state, apply);
            record_action_result(&state, result);
            refresh_weak(&weak, &state);
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
            let result =
                (|| {
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
                    let mut replacement = ApplicationState::new(config.clone(), snapshot)?;
                    replacement.push_log("Validated and atomically saved Config v5.".to_string());
                    for diagnostic in diagnostics {
                        replacement.push_log(diagnostic);
                    }
                    *state.lock().map_err(|_| {
                        "The SPLINED application state is unavailable.".to_string()
                    })? = replacement;
                    *settings_draft.lock().map_err(|_| {
                        "The structured Settings draft is unavailable.".to_string()
                    })? = config.clone();
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
                    let mut config = Config::default();
                    apply_settings_ui(&ui, &mut config)?;
                    if config.scan.scan_library_dir.trim().is_empty() {
                        config.scan.scan_library_dir = config.library.music_library.clone();
                    }
                    let config = splined_core::settings::save_config(&config)?;
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
                    ui.set_page_index(0);
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
            move |provider, server, auth, username, identifier, secret| {
                let editor =
                    credential_from_fields(provider, server, auth, username, identifier, secret);
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
            move |provider, server, auth, username, identifier, secret| {
                let editor =
                    credential_from_fields(provider, server, auth, username, identifier, secret);
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
                let weak = weak.clone();
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    let result = match (action.as_str(), config) {
                        (_, None) => {
                            Err("The SPLINED application state is unavailable.".to_string())
                        }
                        ("export", Some(config)) => {
                            export_backup(&path, &config, selection, &password)
                                .map(|saved| format!("Backup created: {}", saved.display()))
                        }
                        ("restore", Some(config)) => {
                            restore_backup(&path, &config, selection, &password).map(|_| {
                                "Backup restored. Restart SPLINED before processing albums."
                                    .to_string()
                            })
                        }
                        _ => Err("Unsupported backup action.".to_string()),
                    };
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Ok(mut app) = state.lock() {
                            app.service_status =
                                result.clone().unwrap_or_else(|error| error.clone());
                        }
                        record_action_result(&state, result.map(|_| ()));
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
    value
        .split([',', '\n', '\r'])
        .map(str::trim)
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
    server: SharedString,
    auth: SharedString,
    username: SharedString,
    identifier: SharedString,
    secret: SharedString,
) -> CredentialEditor {
    CredentialEditor {
        provider: provider.to_string(),
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

fn edit_existing_cover(state: &Arc<Mutex<ApplicationState>>, apply: bool) -> Result<(), String> {
    let (path, config) = {
        let app = state
            .lock()
            .map_err(|_| "The SPLINED application state is unavailable.".to_string())?;
        if apply && app.launch_mode != LaunchMode::LiveWrite {
            return Err("Existing-cover editing requires LIVE WRITE mode.".to_string());
        }
        if app.current_cover_path.trim().is_empty() {
            return Err("The current album has no local cover to edit.".to_string());
        }
        let mut config = app.config.clone();
        config.mode = if apply { Mode::Write } else { Mode::Read };
        project_adjustments(&app, &mut config);
        (app.current_cover_path.clone(), config)
    };
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
    let preview_path = if apply {
        install_prepared_artwork(path, &prepared)?;
        path.to_path_buf()
    } else {
        let extension = match prepared.info.format {
            StaticFormat::Jpeg => "jpg",
            StaticFormat::Png => "png",
            StaticFormat::Webp => "webp",
        };
        let preview_path = Path::new(&config.scan.temporary_cache_dir)
            .join(format!("existing-cover-edit-preview.{extension}"));
        if let Some(parent) = preview_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "Unable to create existing-cover preview directory {}: {error}",
                    parent.display()
                )
            })?;
        }
        replace_binary_file(
            &preview_path,
            &prepared.bytes,
            "existing-cover edit preview",
            |staged| inspect_image(staged).map(|_| ()),
        )?;
        preview_path
    };
    if let Ok(mut app) = state.lock() {
        app.edit_preview_path = preview_path.to_string_lossy().into_owned();
        app.push_log(if apply {
            format!("Edited existing cover: {}", path.display())
        } else {
            format!(
                "Prepared existing-cover preview at {} without changing album files.",
                preview_path.display()
            )
        });
    }
    Ok(())
}

fn preview_selected_candidate(state: &Arc<Mutex<ApplicationState>>) -> Result<(), String> {
    let (index, source, source_path, config) = {
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
        )
    };
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
    let extension = match prepared.info.format {
        StaticFormat::Jpeg => "jpg",
        StaticFormat::Png => "png",
        StaticFormat::Webp => "webp",
    };
    let preview_path = Path::new(&config.scan.temporary_cache_dir)
        .join(format!("candidate-upscale-preview-{index}.{extension}"));
    if let Some(parent) = preview_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Unable to create candidate preview directory {}: {error}",
                parent.display()
            )
        })?;
    }
    replace_binary_file(
        &preview_path,
        &prepared.bytes,
        "candidate upscale preview",
        |staged| inspect_image(staged).map(|_| ()),
    )?;
    if let Ok(mut app) = state.lock() {
        app.edit_preview_path = preview_path.to_string_lossy().into_owned();
        app.push_log(format!(
            "Rendered candidate {index} preview at {} × {} without changing album files.",
            prepared.info.width, prepared.info.height
        ));
    }
    Ok(())
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

fn refresh_weak(weak: &slint::Weak<AppWindow>, state: &Arc<Mutex<ApplicationState>>) {
    if let Some(ui) = weak.upgrade() {
        sync_ui(&ui, state);
    }
}

fn sync_ui(ui: &AppWindow, shared: &Arc<Mutex<ApplicationState>>) {
    let Ok(state) = shared.lock() else {
        return;
    };

    let library_rows = state
        .library
        .rows()
        .into_iter()
        .map(|row| UiLibraryRow {
            key: SharedString::from(row.key),
            primary: SharedString::from(row.primary),
            secondary: SharedString::from(row.secondary),
            status: SharedString::from(row.status),
            selected: row.selected,
            eligible: row.eligible,
        })
        .collect::<Vec<_>>();
    ui.set_library_rows(model(library_rows));
    ui.set_compilation_track_rows(model(
        state
            .library
            .compilation_track_rows()
            .into_iter()
            .map(|track| UiCompilationTrackRow {
                album_key: SharedString::from(track.album_key),
                path: SharedString::from(track.path),
                file_name: SharedString::from(track.file_name),
                completed: track.embedded_artwork_recorded,
                selected: track.selected,
            })
            .collect(),
    ));

    let sources = state
        .sources
        .iter()
        .map(|source| UiSourceRow {
            name: SharedString::from(source.name.clone()),
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
            source: SharedString::from(candidate.source.clone()),
            dimensions: SharedString::from(candidate.dimensions.clone()),
            range_class: SharedString::from(candidate.range_class.clone()),
            policy: SharedString::from(candidate.policy_status.clone()),
            selected: candidate.selected,
            recommended: candidate.recommended,
            preview: load_image(&candidate.cache_path),
        })
        .collect::<Vec<_>>();
    ui.set_candidate_rows(model(candidates));

    let musicbrainz = state
        .musicbrainz_results
        .iter()
        .map(|item| UiMusicBrainzRow {
            index: item.index as i32,
            title: SharedString::from(item.release_title.clone()),
            artist: SharedString::from(item.release_artist.clone()),
            detail: SharedString::from(format!(
                "{} · {} · {}",
                item.release_date, item.release_class, item.release_mbid
            )),
            current: item.current,
            visited: item.visited,
        })
        .collect::<Vec<_>>();
    ui.set_musicbrainz_rows(model(musicbrainz));

    let adjustment = &state.adjustments;
    ui.set_adjustment_rows(model(vec![
        adjustment_row("Picture", adjustment.picture_percent, "%"),
        adjustment_row("Sharpen", adjustment.sharpen_percent, "%"),
        adjustment_row("Softness", adjustment.softness_percent, "%"),
        adjustment_row("Contrast", adjustment.contrast_percent, "%"),
        adjustment_row("Exposure", adjustment.exposure_percent, "%"),
        adjustment_row("Brightness", adjustment.brightness_percent, "%"),
        adjustment_row("Gamma", adjustment.gamma_percent, "%"),
        adjustment_row("Color", adjustment.color_temperature, ""),
    ]));

    let preview_path = (!state.edit_preview_path.is_empty())
        .then_some(state.edit_preview_path.as_str())
        .or_else(|| {
            state
                .selected_candidate()
                .map(|candidate| candidate.cache_path.as_str())
                .filter(|path| !path.is_empty())
        })
        .unwrap_or(&state.current_cover_path);
    ui.set_current_preview(load_image(preview_path));
    ui.set_selected_count(state.library.selected_count() as i32);
    ui.set_status_filter_label(SharedString::from(state.library.status_filter.label()));
    ui.set_database_label(SharedString::from(
        if state.library.database_path.is_empty() {
            "No media database loaded".to_string()
        } else {
            state.library.database_path.clone()
        },
    ));
    ui.set_current_artist(SharedString::from(state.current_artist.clone()));
    ui.set_current_album(SharedString::from(state.current_album.clone()));
    ui.set_current_path(SharedString::from(state.current_album_path.clone()));
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
    ui.set_can_launch(!ui.get_setup_required() && state.can_launch());
    ui.set_running(state.phase != RunPhase::Idle);
    ui.set_live_write(state.launch_mode == LaunchMode::LiveWrite);
    ui.set_awaiting_candidate(state.phase == RunPhase::AwaitingCandidate);
    ui.set_awaiting_musicbrainz(state.phase == RunPhase::AwaitingMusicBrainz);
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
}

fn model<T: Clone + 'static>(values: Vec<T>) -> ModelRc<T> {
    Rc::new(VecModel::from(values)).into()
}

fn adjustment_row(name: &str, value: i32, unit: &str) -> UiAdjustmentRow {
    UiAdjustmentRow {
        name: SharedString::from(name),
        value,
        unit: SharedString::from(unit),
    }
}

fn load_image(path: &str) -> slint::Image {
    if path.trim().is_empty() {
        return slint::Image::default();
    }
    let Ok(metadata) = std::fs::metadata(path) else {
        IMAGE_CACHE.with(|cache| cache.borrow_mut().remove(path));
        return slint::Image::default();
    };
    let identity = (metadata.len(), metadata.modified().ok());
    if let Some(image) = IMAGE_CACHE.with(|cache| {
        cache
            .borrow()
            .get(path)
            .filter(|cached| cached.identity == identity)
            .map(|cached| cached.image.clone())
    }) {
        return image;
    }
    let Ok(bytes) = std::fs::read(path) else {
        return slint::Image::default();
    };
    let Ok(decoded) = image::load_from_memory(&bytes) else {
        return slint::Image::default();
    };
    let rgba = decoded.into_rgba8();
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        rgba.as_raw(),
        rgba.width(),
        rgba.height(),
    );
    let image = slint::Image::from_rgba8(buffer);
    IMAGE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= 256 {
            cache.clear();
        }
        cache.insert(
            path.to_string(),
            CachedImage {
                identity,
                image: image.clone(),
            },
        );
    });
    image
}

#[cfg(test)]
mod tests {
    use super::restore_request_from_args;
    use std::ffi::OsString;
    use std::path::PathBuf;

    #[test]
    fn spl_file_association_and_explicit_restore_open_the_restore_surface() {
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
}

#[derive(Clone)]
struct CachedImage {
    identity: (u64, Option<std::time::SystemTime>),
    image: slint::Image,
}

thread_local! {
    static IMAGE_CACHE: RefCell<HashMap<String, CachedImage>> = RefCell::new(HashMap::new());
}
