//! Config v5 and Rust-owned UI preference persistence.

use crate::config::{Config, load_config_at_root, load_config_text_at_root};
use crate::portable::{app_layout, state_root};
use crate::safe_write::replace_text_file;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPreferences {
    #[serde(default = "legacy_workspace_layout_revision")]
    pub workspace_layout_revision: u32,
    pub appearance: String,
    pub workspace_mode: u8,
    pub library_view: String,
    pub status_filter: String,
    pub status_filters: Vec<String>,
    pub include_bypassed: bool,
    pub auto_scan_enabled: bool,
    pub auto_scan_scope: String,
    pub launch_mode: String,
    pub show_tracks: bool,
    pub media_selector_expanded: bool,
    pub candidate_filter_expanded: bool,
    pub artwork_editing_expanded: bool,
    pub show_artwork: bool,
    pub hover_enabled: bool,
    pub upscale_preview_only: bool,
    pub artist_filter: String,
    pub album_filter: String,
    pub candidate_included_sources: Vec<String>,
    pub candidate_included_types: Vec<String>,
    pub candidate_included_policies: Vec<String>,
    pub candidate_included_ranges: Vec<String>,
    pub candidate_excluded_sources: Vec<String>,
    pub candidate_excluded_types: Vec<String>,
    pub candidate_excluded_policies: Vec<String>,
    pub candidate_excluded_ranges: Vec<String>,
    pub primary_adjustments: Vec<String>,
    pub media_pane_width: u32,
    pub media_pane_expanded_width: u32,
    pub artwork_pane_width: u32,
    pub artwork_pane_expanded_width: u32,
    pub candidate_filter_height: u32,
    pub candidate_filter_expanded_height: u32,
    pub activity_pane_height: u32,
    pub window_width: u32,
    pub window_height: u32,
    pub window_maximized: bool,
}

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            workspace_layout_revision: CURRENT_WORKSPACE_LAYOUT_REVISION,
            appearance: "System".to_string(),
            workspace_mode: 0,
            library_view: "artists".to_string(),
            status_filter: "all".to_string(),
            status_filters: Vec::new(),
            include_bypassed: false,
            auto_scan_enabled: false,
            auto_scan_scope: "selected".to_string(),
            launch_mode: "read".to_string(),
            show_tracks: false,
            media_selector_expanded: true,
            candidate_filter_expanded: true,
            artwork_editing_expanded: true,
            show_artwork: true,
            hover_enabled: false,
            upscale_preview_only: true,
            artist_filter: String::new(),
            album_filter: String::new(),
            candidate_included_sources: Vec::new(),
            candidate_included_types: Vec::new(),
            candidate_included_policies: Vec::new(),
            candidate_included_ranges: Vec::new(),
            candidate_excluded_sources: Vec::new(),
            candidate_excluded_types: Vec::new(),
            candidate_excluded_policies: Vec::new(),
            candidate_excluded_ranges: Vec::new(),
            primary_adjustments: vec![
                "Picture".to_string(),
                "Sharpen".to_string(),
                "Contrast".to_string(),
                "Brightness".to_string(),
                "Color".to_string(),
            ],
            media_pane_width: 270,
            media_pane_expanded_width: 270,
            artwork_pane_width: 450,
            artwork_pane_expanded_width: 450,
            candidate_filter_height: 370,
            candidate_filter_expanded_height: 370,
            activity_pane_height: 210,
            window_width: 1680,
            window_height: 945,
            window_maximized: false,
        }
    }
}

pub const CURRENT_WORKSPACE_LAYOUT_REVISION: u32 = 2;

const fn legacy_workspace_layout_revision() -> u32 {
    0
}

impl UiPreferences {
    /// Migrates only presentation geometry after the numbered-screen workspace
    /// replaced the validation shell. Workflow, filters, appearance, and all
    /// Config v5-backed values remain untouched.
    pub fn migrate_workspace_layout(&mut self) -> bool {
        if self.workspace_layout_revision >= CURRENT_WORKSPACE_LAYOUT_REVISION {
            return false;
        }
        let defaults = Self::default();
        self.media_pane_width = defaults.media_pane_width;
        self.media_pane_expanded_width = defaults.media_pane_expanded_width;
        self.artwork_pane_width = defaults.artwork_pane_width;
        self.artwork_pane_expanded_width = defaults.artwork_pane_expanded_width;
        self.candidate_filter_height = defaults.candidate_filter_height;
        self.candidate_filter_expanded_height = defaults.candidate_filter_expanded_height;
        self.activity_pane_height = defaults.activity_pane_height;
        self.workspace_layout_revision = CURRENT_WORKSPACE_LAYOUT_REVISION;
        true
    }
}

pub const MIN_WINDOW_WIDTH: u32 = 960;
pub const MIN_WINDOW_HEIGHT: u32 = 640;
pub const MEDIA_PANE_MIN: u32 = 230;
pub const MEDIA_PANE_MAX: u32 = 620;
pub const MEDIA_PANE_COLLAPSED: u32 = 52;
pub const ARTWORK_PANE_MIN: u32 = 280;
pub const ARTWORK_PANE_MAX: u32 = 720;
pub const CENTER_PANE_MIN: u32 = 340;
pub const CANDIDATE_FILTER_MIN: u32 = 230;
pub const CANDIDATE_FILTER_COLLAPSED: u32 = 185;
pub const CANDIDATE_RESULTS_MIN: u32 = 160;
pub const ACTIVITY_PANE_MIN: u32 = 110;
pub const SPLITTER_HIT_AREA: u32 = 9;
pub const MAIN_WORKSPACE_MIN_HEIGHT: u32 =
    CANDIDATE_FILTER_MIN + SPLITTER_HIT_AREA + CANDIDATE_RESULTS_MIN;

/// Rust-owned workspace geometry. Slint renders and drags the dividers, but
/// every value stored in `ui.toml` passes through these bounds first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceLayout {
    pub media_pane_width: u32,
    pub media_pane_expanded_width: u32,
    pub artwork_pane_width: u32,
    pub artwork_pane_expanded_width: u32,
    pub candidate_filter_height: u32,
    pub candidate_filter_expanded_height: u32,
    pub activity_pane_height: u32,
    pub normal_window_width: u32,
    pub normal_window_height: u32,
    pub maximized: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceBounds {
    pub window_width: u32,
    pub window_height: u32,
    pub media_expanded: bool,
    pub artwork_visible: bool,
    pub candidate_filter_expanded: bool,
}

impl WorkspaceLayout {
    pub fn from_preferences(preferences: &UiPreferences) -> Self {
        Self {
            media_pane_width: preferences.media_pane_width,
            media_pane_expanded_width: preferences.media_pane_expanded_width,
            artwork_pane_width: preferences.artwork_pane_width,
            artwork_pane_expanded_width: preferences.artwork_pane_expanded_width,
            candidate_filter_height: preferences.candidate_filter_height,
            candidate_filter_expanded_height: preferences.candidate_filter_expanded_height,
            activity_pane_height: preferences.activity_pane_height,
            normal_window_width: preferences.window_width,
            normal_window_height: preferences.window_height,
            maximized: preferences.window_maximized,
        }
    }

    pub fn clamp_for_window(
        &mut self,
        window_width: u32,
        window_height: u32,
        media_expanded: bool,
        artwork_visible: bool,
        candidate_filter_expanded: bool,
    ) {
        let window_width = window_width.max(MIN_WINDOW_WIDTH);
        let window_height = window_height.max(MIN_WINDOW_HEIGHT);
        self.media_pane_expanded_width = self
            .media_pane_expanded_width
            .clamp(MEDIA_PANE_MIN, MEDIA_PANE_MAX);
        self.artwork_pane_expanded_width = self
            .artwork_pane_expanded_width
            .clamp(ARTWORK_PANE_MIN, ARTWORK_PANE_MAX);
        self.candidate_filter_expanded_height = self
            .candidate_filter_expanded_height
            .max(CANDIDATE_FILTER_MIN);

        let horizontal_chrome = 38;
        let available_width = window_width.saturating_sub(horizontal_chrome);
        let media_min = if media_expanded {
            MEDIA_PANE_MIN
        } else {
            MEDIA_PANE_COLLAPSED
        };
        let artwork_min = if artwork_visible { ARTWORK_PANE_MIN } else { 0 };
        let maximum_side_total = available_width
            .saturating_sub(CENTER_PANE_MIN)
            .max(media_min.saturating_add(artwork_min));

        let mut media = if media_expanded {
            self.media_pane_expanded_width
        } else {
            MEDIA_PANE_COLLAPSED
        };
        let mut artwork = if artwork_visible {
            self.artwork_pane_expanded_width
        } else {
            0
        };
        shrink_pair_to_total(
            &mut media,
            media_min,
            &mut artwork,
            artwork_min,
            maximum_side_total,
        );
        self.media_pane_width = media;
        self.artwork_pane_width = artwork;

        // App padding, the 54px header, and the layout gaps surrounding the
        // two vertical workspace regions consume 91px.
        let vertical_chrome = 91;
        let available_height = window_height.saturating_sub(vertical_chrome);
        let activity_maximum = available_height
            .saturating_sub(MAIN_WORKSPACE_MIN_HEIGHT + SPLITTER_HIT_AREA)
            .max(ACTIVITY_PANE_MIN);
        self.activity_pane_height = self
            .activity_pane_height
            .clamp(ACTIVITY_PANE_MIN, activity_maximum);
        let main_height =
            available_height.saturating_sub(self.activity_pane_height + SPLITTER_HIT_AREA);
        let candidate_maximum = main_height
            .saturating_sub(CANDIDATE_RESULTS_MIN + SPLITTER_HIT_AREA)
            .max(CANDIDATE_FILTER_MIN);
        self.candidate_filter_height = if candidate_filter_expanded {
            self.candidate_filter_expanded_height
                .clamp(CANDIDATE_FILTER_MIN, candidate_maximum)
        } else {
            CANDIDATE_FILTER_COLLAPSED.min(candidate_maximum)
        };
    }

    pub fn resize_region(&mut self, region: &str, requested: u32, bounds: WorkspaceBounds) {
        match region {
            "media" if bounds.media_expanded => self.media_pane_expanded_width = requested,
            "artwork" if bounds.artwork_visible => self.artwork_pane_expanded_width = requested,
            "candidate-filter" if bounds.candidate_filter_expanded => {
                self.candidate_filter_expanded_height = requested;
            }
            "activity" => self.activity_pane_height = requested,
            _ => {}
        }
        self.clamp_for_window(
            bounds.window_width,
            bounds.window_height,
            bounds.media_expanded,
            bounds.artwork_visible,
            bounds.candidate_filter_expanded,
        );
    }

    /// Clamp the shared Media Library pane for a secondary workspace that does
    /// not have the main window's separate artwork column. The remembered
    /// expanded width is retained when a smaller window temporarily needs to
    /// render the pane at its minimum.
    pub fn clamp_media_for_content(
        &mut self,
        window_width: u32,
        media_expanded: bool,
        content_minimum: u32,
    ) {
        self.media_pane_expanded_width = self
            .media_pane_expanded_width
            .clamp(MEDIA_PANE_MIN, MEDIA_PANE_MAX);
        if !media_expanded {
            self.media_pane_width = MEDIA_PANE_COLLAPSED;
            return;
        }
        let available = window_width
            .max(MIN_WINDOW_WIDTH)
            .saturating_sub(38)
            .saturating_sub(content_minimum);
        let maximum = available.clamp(MEDIA_PANE_MIN, MEDIA_PANE_MAX);
        self.media_pane_width = self.media_pane_expanded_width.min(maximum);
    }

    pub fn resize_media_for_content(
        &mut self,
        requested: u32,
        window_width: u32,
        media_expanded: bool,
        content_minimum: u32,
    ) {
        if media_expanded {
            self.media_pane_expanded_width = requested.clamp(MEDIA_PANE_MIN, MEDIA_PANE_MAX);
        }
        self.clamp_media_for_content(window_width, media_expanded, content_minimum);
    }

    pub fn record_normal_window(&mut self, width: u32, height: u32, maximized: bool) {
        self.maximized = maximized;
        if !maximized && width >= MIN_WINDOW_WIDTH && height >= MIN_WINDOW_HEIGHT {
            self.normal_window_width = width;
            self.normal_window_height = height;
        }
    }
}

fn shrink_pair_to_total(
    first: &mut u32,
    first_minimum: u32,
    second: &mut u32,
    second_minimum: u32,
    maximum_total: u32,
) {
    let total = first.saturating_add(*second);
    if total <= maximum_total {
        return;
    }
    let excess = total - maximum_total;
    let first_slack = first.saturating_sub(first_minimum);
    let second_slack = second.saturating_sub(second_minimum);
    let slack_total = first_slack.saturating_add(second_slack);
    if slack_total == 0 {
        return;
    }
    let first_reduction = excess
        .saturating_mul(first_slack)
        .checked_div(slack_total)
        .unwrap_or(0)
        .min(first_slack);
    *first = first.saturating_sub(first_reduction);
    let remaining = excess.saturating_sub(first_reduction);
    let second_reduction = remaining.min(second_slack);
    *second = second.saturating_sub(second_reduction);
    let still_excess = first.saturating_add(*second).saturating_sub(maximum_total);
    *first = first.saturating_sub(still_excess.min(first.saturating_sub(first_minimum)));
}

pub fn normalize_appearance(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "light" => "Light",
        "dark" => "Dark",
        _ => "System",
    }
    .to_string()
}

pub fn config_text() -> Result<String, String> {
    let path = app_layout()?.config_file;
    std::fs::read_to_string(&path)
        .map_err(|error| format!("Unable to read Config v5 {}: {error}", path.display()))
}

pub fn save_config_text(text: &str) -> Result<Config, String> {
    let root = state_root()?;
    save_config_text_at_root(text, &root)
}

pub fn save_config_text_at_root(text: &str, root: &Path) -> Result<Config, String> {
    let runtime = load_config_text_at_root(text, root)?;
    validate_operational_config(&runtime)?;
    create_configured_runtime_directories(&runtime)?;
    let path = root.join("data").join("config.toml");
    replace_text_file(&path, text, "SPLINED configuration", |candidate| {
        load_config_text_at_root(candidate, root)
            .and_then(|candidate| validate_operational_config(&candidate))
    })?;
    load_config_at_root(root)
}

pub fn save_config(config: &Config) -> Result<Config, String> {
    let root = state_root()?;
    save_config_at_root(config, &root)
}

pub fn save_config_at_root(config: &Config, root: &Path) -> Result<Config, String> {
    let mut portable = config.clone();
    portable.library.music_library = portable_path(root, &portable.library.music_library);
    portable.scan.scan_library_dir = portable_path(root, &portable.scan.scan_library_dir);
    portable.scan.cache_dir = portable_path(root, &portable.scan.cache_dir);
    portable.scan.temporary_cache_dir = portable_path(root, &portable.scan.temporary_cache_dir);
    portable.scan.log_dir = portable_path(root, &portable.scan.log_dir);
    portable.credentials.credential_dir = portable_path(root, &portable.credentials.credential_dir);
    let text = toml::to_string_pretty(&portable)
        .map_err(|error| format!("Unable to serialize Config v5: {error}"))?;
    save_config_text_at_root(&text, root)
}

/// Windows setup/settings validation. Parsing protects the complete Config v5
/// schema; this adds the operational path contract required before the GUI may
/// enter its normal workspace.
pub fn validate_operational_config(config: &Config) -> Result<(), String> {
    for (label, value) in [
        ("Music Library", config.library.music_library.as_str()),
        ("SQLite/database directory", config.scan.cache_dir.as_str()),
        (
            "Temporary cache directory",
            config.scan.temporary_cache_dir.as_str(),
        ),
        ("Log directory", config.scan.log_dir.as_str()),
        (
            "Credential directory",
            config.credentials.credential_dir.as_str(),
        ),
    ] {
        if value.trim().is_empty() {
            return Err(format!("{label} is required."));
        }
    }

    let library = Path::new(&config.library.music_library);
    if !library.is_dir() {
        return Err(format!(
            "Music Library does not exist or is unavailable: {}",
            library.display()
        ));
    }
    if !config.scan.scan_library_dir.trim().is_empty()
        && !Path::new(&config.scan.scan_library_dir).is_dir()
    {
        return Err(format!(
            "Music Library scan folder does not exist or is unavailable: {}",
            config.scan.scan_library_dir
        ));
    }
    Ok(())
}

fn create_configured_runtime_directories(config: &Config) -> Result<(), String> {
    let mut created = HashSet::new();
    for (label, value) in [
        ("SQLite/database", config.scan.cache_dir.as_str()),
        ("temporary cache", config.scan.temporary_cache_dir.as_str()),
        ("log", config.scan.log_dir.as_str()),
        ("credential", config.credentials.credential_dir.as_str()),
    ] {
        let path = Path::new(value);
        let key = path.to_string_lossy().to_ascii_lowercase();
        if !created.insert(key) {
            continue;
        }
        if path.exists() && !path.is_dir() {
            return Err(format!(
                "Configured {label} path is not a directory: {}",
                path.display()
            ));
        }
        std::fs::create_dir_all(path).map_err(|error| {
            format!(
                "Unable to create configured {label} directory {}: {error}",
                path.display()
            )
        })?;
    }
    Ok(())
}

pub fn ui_preferences_path() -> Result<PathBuf, String> {
    Ok(app_layout()?.ui_file)
}

pub fn load_ui_preferences() -> Result<UiPreferences, String> {
    let path = ui_preferences_path()?;
    if !path.is_file() {
        return Ok(UiPreferences::default());
    }
    let body = std::fs::read_to_string(&path).map_err(|error| {
        format!(
            "Unable to read SPLINED interface settings {}: {error}",
            path.display()
        )
    })?;
    toml::from_str(&body).map_err(|error| {
        format!(
            "Invalid SPLINED interface settings {}: {error}",
            path.display()
        )
    })
}

pub fn save_ui_preferences(preferences: &UiPreferences) -> Result<(), String> {
    let path = ui_preferences_path()?;
    let body = toml::to_string_pretty(preferences)
        .map_err(|error| format!("Unable to serialize SPLINED interface settings: {error}"))?;
    replace_text_file(&path, &body, "SPLINED interface settings", |candidate| {
        toml::from_str::<UiPreferences>(candidate)
            .map(|_| ())
            .map_err(|error| format!("Invalid staged interface settings: {error}"))
    })
}

fn portable_path(root: &Path, value: &str) -> String {
    let path = Path::new(value.trim());
    if value.trim().is_empty() || !path.is_absolute() {
        return value.trim().to_string();
    }
    path.strip_prefix(root)
        .ok()
        .and_then(Path::to_str)
        .map(|relative| relative.replace('\\', "/"))
        .unwrap_or_else(|| value.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn external_paths_remain_absolute_and_state_paths_become_portable() {
        let root = Path::new(r"C:\Portable\SPLINED");
        assert_eq!(
            portable_path(root, r"C:\Portable\SPLINED\credentials"),
            "credentials"
        );
        assert_eq!(portable_path(root, r"\\server\music"), r"\\server\music");
    }

    #[test]
    fn config_text_validation_rejects_wrong_schema_before_write() {
        assert!(crate::config::parse_config("config_version = 999").is_err());
    }

    #[test]
    fn clean_root_stays_empty_until_explicit_save() {
        let parent = TempDir::new().unwrap();
        let root = parent.path().join("clean-portable");
        let error = load_config_at_root(&root).unwrap_err();
        assert!(error.contains("Config v5 was not found"));
        assert!(!root.exists());
        assert!(!root.join("data").exists());
        assert!(!root.join("_cache").exists());
        assert!(!root.join("docker_builds").exists());
    }

    #[test]
    fn explicit_save_creates_only_configured_runtime_directories() {
        let root = TempDir::new().unwrap();
        let library = root.path().join("music");
        std::fs::create_dir(&library).unwrap();
        let mut config = Config::default();
        config.library.music_library = library.to_string_lossy().into_owned();
        config.scan.scan_library_dir = config.library.music_library.clone();
        config.scan.cache_dir = root.path().join("database").to_string_lossy().into_owned();
        config.scan.temporary_cache_dir =
            root.path().join("temporary").to_string_lossy().into_owned();
        config.scan.log_dir = root.path().join("logs").to_string_lossy().into_owned();
        config.credentials.credential_dir = root
            .path()
            .join("private-credentials")
            .to_string_lossy()
            .into_owned();

        let saved = save_config_at_root(&config, root.path()).unwrap();

        assert!(root.path().join("data/config.toml").is_file());
        assert!(Path::new(&saved.scan.cache_dir).is_dir());
        assert!(Path::new(&saved.scan.temporary_cache_dir).is_dir());
        assert!(Path::new(&saved.scan.log_dir).is_dir());
        assert!(Path::new(&saved.credentials.credential_dir).is_dir());
        assert!(!root.path().join("docker_builds").exists());
        assert!(!root.path().join("_cache").exists());
    }

    #[test]
    fn external_absolute_and_unc_paths_are_not_rewritten() {
        let root = Path::new(r"C:\Portable\SPLINED");
        assert_eq!(
            portable_path(root, r"E:\SPLINED Database"),
            r"E:\SPLINED Database"
        );
        assert_eq!(
            portable_path(root, r"\\server\share\credentials"),
            r"\\server\share\credentials"
        );
    }

    #[test]
    fn mature_interface_preferences_round_trip_without_a_second_authority() {
        let preferences = UiPreferences {
            appearance: "Light".to_string(),
            workspace_mode: 2,
            library_view: "folders".to_string(),
            status_filters: vec!["Unprocessed".to_string(), "Incomplete".to_string()],
            show_tracks: true,
            media_selector_expanded: false,
            candidate_filter_expanded: false,
            artwork_editing_expanded: true,
            show_artwork: true,
            hover_enabled: true,
            upscale_preview_only: false,
            artist_filter: "Beyoncé".to_string(),
            album_filter: "Renaissance".to_string(),
            candidate_included_types: vec!["FrontCover".to_string()],
            candidate_excluded_types: vec!["BackCover".to_string()],
            primary_adjustments: vec![
                "Gamma".to_string(),
                "Color".to_string(),
                "Picture".to_string(),
                "Sharpen".to_string(),
                "Contrast".to_string(),
            ],
            media_pane_width: 388,
            media_pane_expanded_width: 388,
            artwork_pane_width: 412,
            artwork_pane_expanded_width: 412,
            candidate_filter_height: 310,
            candidate_filter_expanded_height: 310,
            activity_pane_height: 166,
            window_maximized: true,
            ..UiPreferences::default()
        };
        let text = toml::to_string(&preferences).unwrap();
        let restored: UiPreferences = toml::from_str(&text).unwrap();
        assert_eq!(restored, preferences);
    }

    #[test]
    fn older_ui_toml_receives_resizable_workspace_defaults() {
        let mut restored: UiPreferences = toml::from_str(
            r#"
library_view = "albums"
window_width = 1200
window_height = 700
"#,
        )
        .unwrap();
        let defaults = UiPreferences::default();
        assert_eq!(restored.workspace_layout_revision, 0);
        assert_eq!(restored.media_pane_width, defaults.media_pane_width);
        assert_eq!(
            restored.artwork_pane_expanded_width,
            defaults.artwork_pane_expanded_width
        );
        assert_eq!(
            restored.candidate_filter_height,
            defaults.candidate_filter_height
        );
        assert_eq!(restored.appearance, "System");
        assert_eq!(restored.workspace_mode, 0);
        assert!(!restored.window_maximized);
        assert!(restored.migrate_workspace_layout());
        assert_eq!(
            restored.workspace_layout_revision,
            CURRENT_WORKSPACE_LAYOUT_REVISION
        );
        assert!(!restored.migrate_workspace_layout());
    }

    #[test]
    fn numbered_workspace_migration_replaces_only_obsolete_geometry() {
        let mut restored: UiPreferences = toml::from_str(
            r#"
appearance = "Dark"
library_view = "folders"
artist_filter = "Blue Merle"
media_pane_width = 320
media_pane_expanded_width = 320
artwork_pane_width = 720
artwork_pane_expanded_width = 720
candidate_filter_height = 983
candidate_filter_expanded_height = 983
activity_pane_height = 150
window_width = 2732
window_height = 1977
"#,
        )
        .unwrap();
        assert!(restored.migrate_workspace_layout());
        let defaults = UiPreferences::default();
        assert_eq!(restored.media_pane_width, defaults.media_pane_width);
        assert_eq!(restored.artwork_pane_width, defaults.artwork_pane_width);
        assert_eq!(restored.activity_pane_height, defaults.activity_pane_height);
        assert_eq!(restored.window_width, 2732);
        assert_eq!(restored.window_height, 1977);
        assert_eq!(restored.appearance, "Dark");
        assert_eq!(restored.library_view, "folders");
        assert_eq!(restored.artist_filter, "Blue Merle");
    }

    #[test]
    fn invalid_saved_geometry_is_clamped_to_usable_panes() {
        let preferences = UiPreferences {
            media_pane_width: 0,
            media_pane_expanded_width: 0,
            artwork_pane_width: u32::MAX,
            artwork_pane_expanded_width: u32::MAX,
            candidate_filter_height: u32::MAX,
            candidate_filter_expanded_height: u32::MAX,
            activity_pane_height: u32::MAX,
            ..UiPreferences::default()
        };
        let mut layout = WorkspaceLayout::from_preferences(&preferences);
        layout.clamp_for_window(960, 640, true, true, true);
        assert!(layout.media_pane_width >= MEDIA_PANE_MIN);
        assert!(layout.artwork_pane_width >= ARTWORK_PANE_MIN);
        assert!(layout.media_pane_width + layout.artwork_pane_width <= 960 - 38 - CENTER_PANE_MIN);
        assert!(layout.activity_pane_height >= ACTIVITY_PANE_MIN);
        assert!(layout.candidate_filter_height >= CANDIDATE_FILTER_MIN);
        let usable_height = 640 - 91;
        let main_height = usable_height - layout.activity_pane_height - SPLITTER_HIT_AREA;
        assert!(
            main_height - layout.candidate_filter_height - SPLITTER_HIT_AREA
                >= CANDIDATE_RESULTS_MIN
        );
    }

    #[test]
    fn collapsed_panes_reclaim_space_and_restore_expanded_widths() {
        let preferences = UiPreferences {
            media_pane_width: 410,
            media_pane_expanded_width: 410,
            artwork_pane_width: 430,
            artwork_pane_expanded_width: 430,
            candidate_filter_height: 320,
            candidate_filter_expanded_height: 320,
            ..UiPreferences::default()
        };
        let mut layout = WorkspaceLayout::from_preferences(&preferences);
        layout.clamp_for_window(1600, 1000, false, false, false);
        assert_eq!(layout.media_pane_width, MEDIA_PANE_COLLAPSED);
        assert_eq!(layout.artwork_pane_width, 0);
        assert_eq!(layout.candidate_filter_height, CANDIDATE_FILTER_COLLAPSED);
        assert_eq!(layout.media_pane_expanded_width, 410);
        assert_eq!(layout.artwork_pane_expanded_width, 430);
        assert_eq!(layout.candidate_filter_expanded_height, 320);

        layout.clamp_for_window(1600, 1000, true, true, true);
        assert_eq!(layout.media_pane_width, 410);
        assert_eq!(layout.artwork_pane_width, 430);
        assert_eq!(layout.candidate_filter_height, 320);
    }

    #[test]
    fn media_expansion_recovers_from_invalid_collapsed_geometry() {
        let preferences = UiPreferences {
            media_pane_width: 0,
            media_pane_expanded_width: 0,
            ..UiPreferences::default()
        };
        let mut layout = WorkspaceLayout::from_preferences(&preferences);

        layout.clamp_for_window(1280, 720, false, true, true);
        assert_eq!(layout.media_pane_width, MEDIA_PANE_COLLAPSED);

        layout.clamp_for_window(1280, 720, true, true, true);
        assert_eq!(layout.media_pane_width, MEDIA_PANE_MIN);
        assert_eq!(layout.media_pane_expanded_width, MEDIA_PANE_MIN);
    }

    #[test]
    fn secondary_workspace_uses_the_shared_media_width_without_clipping_content() {
        let preferences = UiPreferences {
            media_pane_width: 520,
            media_pane_expanded_width: 520,
            ..UiPreferences::default()
        };
        let mut layout = WorkspaceLayout::from_preferences(&preferences);

        layout.clamp_media_for_content(960, true, 650);
        assert_eq!(layout.media_pane_width, 272);
        assert_eq!(layout.media_pane_expanded_width, 520);

        layout.resize_media_for_content(300, 1680, true, 650);
        assert_eq!(layout.media_pane_width, 300);
        assert_eq!(layout.media_pane_expanded_width, 300);

        layout.clamp_media_for_content(1680, false, 650);
        assert_eq!(layout.media_pane_width, MEDIA_PANE_COLLAPSED);
        assert_eq!(layout.media_pane_expanded_width, 300);
    }

    #[test]
    fn resizing_updates_only_the_requested_expanded_dimension() {
        let mut layout = WorkspaceLayout::from_preferences(&UiPreferences::default());
        let artwork_before = layout.artwork_pane_expanded_width;
        let bounds = WorkspaceBounds {
            window_width: 1600,
            window_height: 900,
            media_expanded: true,
            artwork_visible: true,
            candidate_filter_expanded: true,
        };
        layout.resize_region("media", 444, bounds);
        assert_eq!(layout.media_pane_width, 444);
        assert_eq!(layout.media_pane_expanded_width, 444);
        assert_eq!(layout.artwork_pane_expanded_width, artwork_before);

        layout.resize_region("artwork", 470, bounds);
        layout.resize_region("candidate-filter", 305, bounds);
        layout.resize_region("activity", 125, bounds);
        assert_eq!(layout.artwork_pane_width, 470);
        assert_eq!(layout.artwork_pane_expanded_width, 470);
        assert_eq!(layout.candidate_filter_height, 305);
        assert_eq!(layout.candidate_filter_expanded_height, 305);
        assert_eq!(layout.activity_pane_height, 125);

        let media_before_collapse = layout.media_pane_expanded_width;
        layout.resize_region(
            "media",
            555,
            WorkspaceBounds {
                media_expanded: false,
                ..bounds
            },
        );
        assert_eq!(layout.media_pane_width, MEDIA_PANE_COLLAPSED);
        assert_eq!(layout.media_pane_expanded_width, media_before_collapse);
    }

    #[test]
    fn maximized_state_preserves_the_last_normal_window_size() {
        let mut layout = WorkspaceLayout::from_preferences(&UiPreferences::default());
        layout.record_normal_window(1280, 720, false);
        layout.record_normal_window(2560, 1440, true);
        assert_eq!(layout.normal_window_width, 1280);
        assert_eq!(layout.normal_window_height, 720);
        assert!(layout.maximized);
        assert_eq!(normalize_appearance("light"), "Light");
        assert_eq!(normalize_appearance("anything else"), "System");
    }
}
