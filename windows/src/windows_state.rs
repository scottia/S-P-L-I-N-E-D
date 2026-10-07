use crate::config::{Config, default_toml, load_config_text};
use crate::portable::{AppLayout, app_layout};
use crate::safe_write::replace_text_file;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const LEGACY_CONFIG_VALUE: &str = "ConfigV5";
const LEGACY_UI_VALUE: &str = "UiV4";
const LEGACY_MIGRATED_VALUE: &str = "PortableFilesMigrated";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct UiState {
    pub theme: String,
    pub show_status_on_launch: bool,
    pub show_confirmations: bool,
    pub hover_enabled: bool,
    pub show_artwork: bool,
    pub show_media_selector: bool,
    pub media_filter_expanded: bool,
    pub candidate_filter_expanded: bool,
    pub candidate_excluded_sources: Vec<String>,
    pub candidate_excluded_types: Vec<String>,
    pub candidate_excluded_policies: Vec<String>,
    pub candidate_excluded_ranges: Vec<String>,
    pub media_artist_filter: String,
    pub media_album_filter: String,
    pub media_show_white: bool,
    pub media_show_orange: bool,
    pub media_show_red: bool,
    pub media_show_purple: bool,
    pub media_show_green: bool,
    pub media_show_blue: bool,
    pub media_show_incomplete: bool,
    pub show_tracks: bool,
    pub selected_compilation_track_path: String,
    pub filtered_scan_mode: String,
    pub auto_scan_enabled: bool,
    pub auto_scan_scope: String,
    pub selected_album_paths: Vec<String>,
    pub main_width: u32,
    pub main_height: u32,
    pub main_x: i32,
    pub main_y: i32,
    pub main_maximized: bool,
    pub main_splitter_distance: u32,
    pub right_splitter_distance: u32,
    pub layout_preset: String,
    pub layout_stacked: bool,
    pub setup_width: u32,
    pub setup_height: u32,
    pub setup_x: i32,
    pub setup_y: i32,
    pub setup_maximized: bool,
    pub setup_primary_tab: u32,
    pub setup_advanced_tab: u32,
    pub compare_width: u32,
    pub compare_height: u32,
    pub preview_width: u32,
    pub preview_height: u32,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            theme: "System".into(),
            show_status_on_launch: true,
            show_confirmations: true,
            hover_enabled: false,
            show_artwork: true,
            show_media_selector: true,
            media_filter_expanded: true,
            candidate_filter_expanded: false,
            candidate_excluded_sources: Vec::new(),
            candidate_excluded_types: Vec::new(),
            candidate_excluded_policies: Vec::new(),
            candidate_excluded_ranges: Vec::new(),
            media_artist_filter: String::new(),
            media_album_filter: String::new(),
            media_show_white: true,
            media_show_orange: true,
            media_show_red: true,
            media_show_purple: true,
            media_show_green: true,
            media_show_blue: true,
            media_show_incomplete: true,
            show_tracks: false,
            selected_compilation_track_path: String::new(),
            filtered_scan_mode: String::new(),
            auto_scan_enabled: false,
            auto_scan_scope: "selected".into(),
            selected_album_paths: Vec::new(),
            main_width: 1400,
            main_height: 900,
            main_x: -1,
            main_y: -1,
            main_maximized: false,
            main_splitter_distance: 430,
            right_splitter_distance: 285,
            layout_preset: "Balanced".into(),
            layout_stacked: false,
            setup_width: 980,
            setup_height: 790,
            setup_x: -1,
            setup_y: -1,
            setup_maximized: false,
            setup_primary_tab: 0,
            setup_advanced_tab: 0,
            compare_width: 980,
            compare_height: 620,
            preview_width: 520,
            preview_height: 560,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct UiDocument {
    #[serde(default)]
    ui: UiState,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortableState {
    pub app_root: String,
    pub config_path: String,
    pub ui_path: String,
    pub first_run: bool,
    pub config_text: String,
    pub ui: UiState,
}

pub fn ui_path(layout: &AppLayout) -> PathBuf {
    layout.config_dir.join("ui.toml")
}

pub fn load_portable_state() -> Result<PortableState, String> {
    let layout = app_layout()?;
    migrate_legacy_settings(&layout)?;
    load_portable_state_from_layout(&layout)
}

fn load_portable_state_from_layout(layout: &AppLayout) -> Result<PortableState, String> {
    let ui_file = ui_path(layout);
    let first_run = !layout.config_file.is_file();
    let config_text = if first_run {
        default_toml().map_err(|error| format!("Unable to generate Config v5 defaults: {error}"))?
    } else {
        fs::read_to_string(&layout.config_file).map_err(|error| {
            format!(
                "Unable to read portable Config v5 {}: {error}",
                layout.config_file.display()
            )
        })?
    };
    load_config_text(&config_text)?;
    let ui = if ui_file.is_file() {
        let text = fs::read_to_string(&ui_file).map_err(|error| {
            format!(
                "Unable to read portable UI state {}: {error}",
                ui_file.display()
            )
        })?;
        toml::from_str::<UiDocument>(&text)
            .map_err(|error| format!("Unable to parse portable UI state: {error}"))?
            .ui
    } else {
        UiState::default()
    };
    Ok(PortableState {
        app_root: layout.root.to_string_lossy().into_owned(),
        config_path: layout.config_file.to_string_lossy().into_owned(),
        ui_path: ui_file.to_string_lossy().into_owned(),
        first_run,
        config_text,
        ui,
    })
}

pub fn save_config_text(text: &str) -> Result<Config, String> {
    let config = load_config_text(text)?;
    let layout = app_layout()?;
    replace_text_file(
        &layout.config_file,
        text,
        "portable Config v5",
        |candidate| load_config_text(candidate).map(|_| ()),
    )?;
    for directory in [
        &config.scan.cache_dir,
        &config.scan.temporary_cache_dir,
        &config.scan.log_dir,
        &config.credentials.credential_dir,
    ] {
        fs::create_dir_all(directory).map_err(|error| {
            format!("Unable to create configured directory {directory}: {error}")
        })?;
    }
    Ok(config)
}

pub fn save_ui_state(state: &UiState) -> Result<(), String> {
    validate_ui_state(state)?;
    let layout = app_layout()?;
    let text = ui_state_to_text(state)?;
    replace_text_file(&ui_path(&layout), &text, "portable UI state", |candidate| {
        parse_ui_state(candidate).map(|_| ())
    })
}

pub fn ui_state_to_text(state: &UiState) -> Result<String, String> {
    validate_ui_state(state)?;
    toml::to_string_pretty(&UiDocument { ui: state.clone() })
        .map_err(|error| format!("Unable to serialize portable UI state: {error}"))
}

pub fn parse_ui_state(text: &str) -> Result<UiState, String> {
    let document = toml::from_str::<UiDocument>(text)
        .map_err(|error| format!("Unable to parse portable UI state: {error}"))?;
    validate_ui_state(&document.ui)?;
    Ok(document.ui)
}

fn validate_ui_state(state: &UiState) -> Result<(), String> {
    if !matches!(state.theme.as_str(), "System" | "Light" | "Dark") {
        return Err("Appearance must be System, Light, or Dark.".to_string());
    }
    if state.main_width < 940 || state.main_height < 640 {
        return Err("Saved main-window dimensions are below the supported minimum.".to_string());
    }
    Ok(())
}

fn legacy_registry_path(root: &Path) -> String {
    let normalized = root
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_uppercase();
    let digest = Sha256::digest(normalized.as_bytes());
    let id = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    format!(r"Software\SPLINED\WindowsV4\{id}")
}

fn should_attempt_legacy_migration(
    portable_config_exists: bool,
    portable_ui_exists: bool,
    migration_recorded: bool,
) -> bool {
    !portable_config_exists && !portable_ui_exists && !migration_recorded
}

#[cfg(windows)]
fn migrate_legacy_settings(layout: &AppLayout) -> Result<(), String> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

    let ui_file = ui_path(layout);
    if !should_attempt_legacy_migration(layout.config_file.exists(), ui_file.exists(), false) {
        return Ok(());
    }
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = legacy_registry_path(&layout.root);
    let Ok(key) = hkcu.open_subkey_with_flags(&path, KEY_READ | KEY_WRITE) else {
        return Ok(());
    };
    if !should_attempt_legacy_migration(
        false,
        false,
        key.get_value::<u32, _>(LEGACY_MIGRATED_VALUE).unwrap_or(0) == 1,
    ) {
        return Ok(());
    }
    let config = key.get_value::<String, _>(LEGACY_CONFIG_VALUE).ok();
    let ui = key.get_value::<String, _>(LEGACY_UI_VALUE).ok();
    if config
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
        && ui.as_deref().is_none_or(|value| value.trim().is_empty())
    {
        return Ok(());
    }

    if let Some(config) = config.filter(|value| !value.trim().is_empty()) {
        load_config_text(&config)?;
        replace_text_file(
            &layout.config_file,
            &config,
            "migrated Config v5",
            |candidate| load_config_text(candidate).map(|_| ()),
        )?;
    }
    if let Some(ui) = ui.filter(|value| !value.trim().is_empty()) {
        toml::from_str::<UiDocument>(&ui)
            .map_err(|error| format!("Unable to validate legacy UI state: {error}"))?;
        replace_text_file(&ui_file, &ui, "migrated UI state", |candidate| {
            toml::from_str::<UiDocument>(candidate)
                .map(|_| ())
                .map_err(|error| format!("Unable to validate migrated UI state: {error}"))
        })?;
    }
    key.set_value(LEGACY_MIGRATED_VALUE, &1u32)
        .map_err(|error| format!("Unable to record portable settings migration: {error}"))
}

#[cfg(not(windows))]
fn migrate_legacy_settings(_layout: &AppLayout) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn clean_portable_folder_does_not_create_state_while_loading_defaults() {
        let dir = TempDir::new().unwrap();
        let layout = AppLayout::from_root(dir.path().join("portable"));
        let state = load_portable_state_from_layout(&layout).unwrap();
        assert!(state.first_run);
        assert!(!layout.config_dir.exists());
        assert_eq!(state.ui, UiState::default());
    }

    #[test]
    fn ui_document_is_path_independent_and_round_trips() {
        let state = UiState {
            theme: "Dark".into(),
            selected_album_paths: vec![r"\\server\music\Artist\Album".into()],
            ..UiState::default()
        };
        let text = toml::to_string_pretty(&UiDocument { ui: state.clone() }).unwrap();
        let restored: UiDocument = toml::from_str(&text).unwrap();
        assert_eq!(restored.ui, state);
        assert!(!text.contains("portable"));
    }

    #[test]
    fn legacy_registry_path_is_stable_per_portable_root() {
        assert_eq!(
            legacy_registry_path(Path::new(r"C:\Portable\SPLINED")),
            legacy_registry_path(Path::new(r"c:\portable\splined\"))
        );
        assert_ne!(
            legacy_registry_path(Path::new(r"C:\Portable\SPLINED")),
            legacy_registry_path(Path::new(r"D:\Portable\SPLINED"))
        );
    }

    #[test]
    fn portable_files_or_completed_migration_prevent_legacy_resurrection() {
        assert!(!should_attempt_legacy_migration(true, false, false));
        assert!(!should_attempt_legacy_migration(false, true, false));
        assert!(!should_attempt_legacy_migration(false, false, true));
        assert!(should_attempt_legacy_migration(false, false, false));
    }
}
