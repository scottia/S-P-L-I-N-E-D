//! Config v5 and Rust-owned UI preference persistence.

use crate::config::{Config, load_config_at_root, load_config_text_at_root};
use crate::portable::{app_layout, state_root};
use crate::safe_write::replace_text_file;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdjustmentPreset {
    pub name: String,
    pub values: [i32; 8],
    pub saved: bool,
}

pub fn default_adjustment_presets() -> Vec<AdjustmentPreset> {
    ["Default", "Custom A", "Custom B", "Custom C", "Custom D"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| AdjustmentPreset {
            name: name.to_string(),
            values: if index == 0 {
                [2, 4, 0, 10, -5, -1, -6, 0]
            } else {
                [0; 8]
            },
            saved: index == 0,
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BannerThemeStyle {
    pub key: String,
    pub name: String,
    pub background: String,
    pub frame: String,
    pub letters: String,
}

pub fn default_banner_theme_styles() -> Vec<BannerThemeStyle> {
    [
        ("compiled", "COMPILED", "#3478FF", "#D9AD43", "#D9AD43"),
        ("featuring", "FEATURING", "#B8875D", "#008B8B", "#008B8B"),
        ("soundtrack", "SOUNDTRACK", "#4B0082", "#B49B00", "#B49B00"),
        ("various", "VARIOUS", "#654321", "#32DDA0", "#32DDA0"),
        ("artist", "ARTIST", "#006B3C", "#D9AD43", "#D9AD43"),
    ]
    .into_iter()
    .map(|(key, name, background, frame, letters)| BannerThemeStyle {
        key: key.to_string(),
        name: name.to_string(),
        background: background.to_string(),
        frame: frame.to_string(),
        letters: letters.to_string(),
    })
    .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumFrameThemeStyle {
    pub key: String,
    pub name: String,
    pub color: String,
}

pub fn default_album_frame_theme_styles() -> Vec<AlbumFrameThemeStyle> {
    [
        ("complete", "Complete", "#32DDA0"),
        ("in_process", "In Process", "#EFA54E"),
        ("thumbnail", "Thumbnail Check", "#7C4DFF"),
    ]
    .into_iter()
    .map(|(key, name, color)| AlbumFrameThemeStyle {
        key: key.to_string(),
        name: name.to_string(),
        color: color.to_string(),
    })
    .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArtistLookupOverride {
    pub artist: String,
    pub lookup_name: String,
    pub musicbrainz_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPreferences {
    #[serde(default = "legacy_workspace_layout_revision")]
    pub workspace_layout_revision: u32,
    pub appearance: String,
    pub status_bar_intensity: String,
    pub status_bar_position: String,
    pub status_bar_size: String,
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
    pub media_filter_expanded: bool,
    pub candidate_filter_expanded: bool,
    pub media_filter_locked: bool,
    pub candidate_filter_locked: bool,
    /// Play an animated cover.webp in the selected-artwork preview only.
    pub animate_webp_preview: bool,
    pub artwork_editing_expanded: bool,
    pub show_artwork: bool,
    pub hover_enabled: bool,
    pub upscale_preview_only: bool,
    pub disable_intro_message: bool,
    pub disable_confirmation_messages: bool,
    pub backup_directory: String,
    pub restore_path: String,
    pub backup_selected_type: String,
    /// Zero keeps every matching file. Limits apply only to SPLINED's own
    /// timestamped backups in the selected backup directory.
    pub max_settings_backup_files: String,
    pub max_database_backup_files: String,
    pub max_diagnostic_backup_files: String,
    pub backup_prompt_dismissed: bool,
    /// Settings/credentials/interface `.spl` schedule. The original field name
    /// is retained so existing portable UI preferences migrate without loss.
    pub backup_schedule: String,
    pub database_backup_schedule: String,
    pub diagnostics_backup_schedule: String,
    pub backup_mode: String,
    pub backup_settings: bool,
    pub backup_credentials: bool,
    pub backup_database: bool,
    pub backup_interface: bool,
    pub backup_diagnostics: bool,
    pub artist_filter: String,
    pub album_filter: String,
    /// Local Navidrome lookup hints keyed by the album-tagged Artist name.
    pub artist_lookup_overrides: Vec<ArtistLookupOverride>,
    pub candidate_included_sources: Vec<String>,
    pub candidate_included_types: Vec<String>,
    pub candidate_included_policies: Vec<String>,
    pub candidate_included_ranges: Vec<String>,
    pub candidate_excluded_sources: Vec<String>,
    pub candidate_excluded_types: Vec<String>,
    pub candidate_excluded_policies: Vec<String>,
    pub candidate_excluded_ranges: Vec<String>,
    pub primary_adjustments: Vec<String>,
    pub adjustment_presets: Vec<AdjustmentPreset>,
    pub banner_theme_styles: Vec<BannerThemeStyle>,
    pub album_frame_theme_styles: Vec<AlbumFrameThemeStyle>,
    pub banner_sync_frame_letters: bool,
    pub banner_random_harmony: bool,
    pub wheel_hue_shift_degrees: u16,
    pub wheel_brightness_percent: u8,
    /// The first saved palette remains the operator's reset point when colors change.
    pub banner_theme_baseline_styles: Vec<BannerThemeStyle>,
    pub album_frame_theme_baseline_styles: Vec<AlbumFrameThemeStyle>,
    pub selected_source_name: String,
    pub musicbrainz_id_editor_expanded: bool,
    pub media_pane_width: u32,
    pub media_pane_expanded_width: u32,
    pub artwork_pane_width: u32,
    pub artwork_pane_expanded_width: u32,
    pub candidate_filter_height: u32,
    pub candidate_filter_expanded_height: u32,
    pub activity_pane_height: u32,
    pub enhancement_original_height: u32,
    pub window_width: u32,
    pub window_height: u32,
    pub window_maximized: bool,
    pub custom_media_selector_expanded: bool,
    pub custom_media_filter_expanded: bool,
    pub custom_media_pane_width: u32,
    pub custom_media_pane_expanded_width: u32,
    pub custom_window_width: u32,
    pub custom_window_height: u32,
    pub custom_window_maximized: bool,
}

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            workspace_layout_revision: CURRENT_WORKSPACE_LAYOUT_REVISION,
            appearance: "System".to_string(),
            status_bar_intensity: "Full".to_string(),
            status_bar_position: "Top".to_string(),
            status_bar_size: "Small".to_string(),
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
            media_filter_expanded: false,
            candidate_filter_expanded: true,
            media_filter_locked: false,
            candidate_filter_locked: false,
            animate_webp_preview: false,
            artwork_editing_expanded: true,
            show_artwork: true,
            hover_enabled: false,
            upscale_preview_only: true,
            disable_intro_message: false,
            disable_confirmation_messages: false,
            backup_directory: String::new(),
            restore_path: String::new(),
            backup_selected_type: "Settings Archive".to_string(),
            max_settings_backup_files: "0".to_string(),
            max_database_backup_files: "0".to_string(),
            max_diagnostic_backup_files: "0".to_string(),
            backup_prompt_dismissed: false,
            backup_schedule: "Off".to_string(),
            database_backup_schedule: "Off".to_string(),
            diagnostics_backup_schedule: "Off".to_string(),
            backup_mode: "Backup".to_string(),
            backup_settings: true,
            backup_credentials: true,
            backup_database: false,
            backup_interface: true,
            backup_diagnostics: false,
            artist_filter: String::new(),
            album_filter: String::new(),
            artist_lookup_overrides: Vec::new(),
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
                "Softness".to_string(),
                "Contrast".to_string(),
                "Exposure".to_string(),
                "Brightness".to_string(),
                "Gamma".to_string(),
                "Color".to_string(),
            ],
            adjustment_presets: default_adjustment_presets(),
            banner_theme_styles: default_banner_theme_styles(),
            album_frame_theme_styles: default_album_frame_theme_styles(),
            banner_sync_frame_letters: false,
            banner_random_harmony: false,
            wheel_hue_shift_degrees: 0,
            wheel_brightness_percent: 100,
            banner_theme_baseline_styles: Vec::new(),
            album_frame_theme_baseline_styles: Vec::new(),
            selected_source_name: "All".to_string(),
            musicbrainz_id_editor_expanded: false,
            media_pane_width: 350,
            media_pane_expanded_width: 350,
            artwork_pane_width: 450,
            artwork_pane_expanded_width: 450,
            candidate_filter_height: 370,
            candidate_filter_expanded_height: 370,
            activity_pane_height: 210,
            enhancement_original_height: 420,
            window_width: 1680,
            window_height: 945,
            window_maximized: false,
            custom_media_selector_expanded: true,
            custom_media_filter_expanded: false,
            custom_media_pane_width: 350,
            custom_media_pane_expanded_width: 350,
            custom_window_width: 1480,
            custom_window_height: 850,
            custom_window_maximized: false,
        }
    }
}

pub const CURRENT_WORKSPACE_LAYOUT_REVISION: u32 = 5;

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
        self.enhancement_original_height = defaults.enhancement_original_height;
        self.custom_media_pane_width = defaults.custom_media_pane_width;
        self.custom_media_pane_expanded_width = defaults.custom_media_pane_expanded_width;
        // A collapsed media rail made a maximized per-monitor-DPI restore look
        // like a half-rendered launch. Each new UI revision starts with the
        // library available, while the user can still collapse it afterward.
        self.media_selector_expanded = true;
        self.primary_adjustments = defaults.primary_adjustments;
        self.workspace_layout_revision = CURRENT_WORKSPACE_LAYOUT_REVISION;
        true
    }
}

pub const MIN_WINDOW_WIDTH: u32 = 960;
pub const MIN_WINDOW_HEIGHT: u32 = 640;
pub const MEDIA_PANE_MIN: u32 = 270;
pub const MEDIA_PANE_MAX: u32 = 900;
pub const MEDIA_PANE_COLLAPSED: u32 = 0;
pub const ARTWORK_PANE_MIN: u32 = 280;
pub const ARTWORK_PANE_MAX: u32 = 1600;
pub const CENTER_PANE_MIN: u32 = 300;
pub const CANDIDATE_FILTER_MIN: u32 = 230;
pub const CANDIDATE_FILTER_COLLAPSED: u32 = 185;
pub const CANDIDATE_RESULTS_MIN: u32 = 140;
pub const ACTIVITY_PANE_MIN: u32 = 150;
pub const SPLITTER_HIT_AREA: u32 = 12;
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
        let window_width = bounds.window_width.max(MIN_WINDOW_WIDTH);
        let window_height = bounds.window_height.max(MIN_WINDOW_HEIGHT);
        let available_width = window_width.saturating_sub(38);
        let media_min = if bounds.media_expanded {
            MEDIA_PANE_MIN
        } else {
            MEDIA_PANE_COLLAPSED
        };
        let artwork_min = if bounds.artwork_visible {
            ARTWORK_PANE_MIN
        } else {
            0
        };
        let maximum_side_total = available_width
            .saturating_sub(CENTER_PANE_MIN)
            .max(media_min.saturating_add(artwork_min));
        let available_height = window_height.saturating_sub(91);

        match region {
            "media" if bounds.media_expanded => {
                let maximum = maximum_side_total
                    .saturating_sub(self.artwork_pane_width)
                    .clamp(MEDIA_PANE_MIN, MEDIA_PANE_MAX);
                let value = requested.clamp(MEDIA_PANE_MIN, maximum);
                self.media_pane_width = value;
                self.media_pane_expanded_width = value;
            }
            "media" => self.media_pane_width = MEDIA_PANE_COLLAPSED,
            "artwork" if bounds.artwork_visible => {
                let maximum = maximum_side_total
                    .saturating_sub(self.media_pane_width)
                    .clamp(ARTWORK_PANE_MIN, ARTWORK_PANE_MAX);
                let value = requested.clamp(ARTWORK_PANE_MIN, maximum);
                self.artwork_pane_width = value;
                self.artwork_pane_expanded_width = value;
            }
            "artwork" => self.artwork_pane_width = 0,
            "candidate-filter" if bounds.candidate_filter_expanded => {
                let main_height =
                    available_height.saturating_sub(self.activity_pane_height + SPLITTER_HIT_AREA);
                let maximum = main_height
                    .saturating_sub(CANDIDATE_RESULTS_MIN + SPLITTER_HIT_AREA)
                    .max(CANDIDATE_FILTER_MIN);
                let value = requested.clamp(CANDIDATE_FILTER_MIN, maximum);
                self.candidate_filter_height = value;
                self.candidate_filter_expanded_height = value;
            }
            "candidate-filter" => {
                self.candidate_filter_height = CANDIDATE_FILTER_COLLAPSED;
            }
            "activity" => {
                let candidate_height = if bounds.candidate_filter_expanded {
                    self.candidate_filter_height
                } else {
                    CANDIDATE_FILTER_COLLAPSED
                };
                let required_main_height = candidate_height
                    .saturating_add(SPLITTER_HIT_AREA)
                    .saturating_add(CANDIDATE_RESULTS_MIN);
                let maximum = available_height
                    .saturating_sub(required_main_height + SPLITTER_HIT_AREA)
                    .max(ACTIVITY_PANE_MIN);
                self.activity_pane_height = requested.clamp(ACTIVITY_PANE_MIN, maximum);
            }
            _ => {}
        }
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
        "chalk" => "Chalk",
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
    let sample_directory = crate::config::configured_samples_dir(config)
        .to_string_lossy()
        .into_owned();
    let mut directories = vec![
        ("SQLite/database", config.scan.cache_dir.as_str()),
        ("temporary cache", config.scan.temporary_cache_dir.as_str()),
        ("log", config.scan.log_dir.as_str()),
        ("credential", config.credentials.credential_dir.as_str()),
    ];
    if config.samples.sample_write {
        directories.push(("sample artwork", sample_directory.as_str()));
    }
    for (label, value) in directories {
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
        let mut preferences = UiPreferences {
            appearance: "Light".to_string(),
            status_bar_intensity: "Mild".to_string(),
            status_bar_position: "Bottom".to_string(),
            status_bar_size: "Large".to_string(),
            workspace_mode: 2,
            library_view: "folders".to_string(),
            status_filters: vec!["Unprocessed".to_string(), "Incomplete".to_string()],
            show_tracks: true,
            media_selector_expanded: false,
            candidate_filter_expanded: false,
            media_filter_locked: true,
            candidate_filter_locked: true,
            animate_webp_preview: true,
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
        preferences.adjustment_presets[2].name = "Warm Artwork".to_string();
        preferences.adjustment_presets[2].values = [6, 2, 1, -3, 4, 5, -2, 16];
        preferences.adjustment_presets[2].saved = true;
        preferences.banner_theme_baseline_styles = preferences.banner_theme_styles.clone();
        preferences.album_frame_theme_baseline_styles =
            preferences.album_frame_theme_styles.clone();
        preferences.banner_theme_styles[0].background = "#123ABC".to_string();
        preferences.banner_theme_styles[0].name = "COMPILED".to_string();
        preferences.banner_sync_frame_letters = true;
        preferences.banner_random_harmony = true;
        preferences.wheel_hue_shift_degrees = 72;
        preferences.wheel_brightness_percent = 125;
        preferences.album_frame_theme_styles[0].color = "#24AABB".to_string();
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
        assert!(!restored.media_filter_locked);
        assert!(!restored.candidate_filter_locked);
        assert!(!restored.animate_webp_preview);
        assert_eq!(restored.workspace_mode, 0);
        assert!(!restored.window_maximized);
        assert!(restored.migrate_workspace_layout());
        assert_eq!(
            restored.workspace_layout_revision,
            CURRENT_WORKSPACE_LAYOUT_REVISION
        );
        assert!(restored.media_selector_expanded);
        assert_eq!(restored.primary_adjustments.len(), 8);
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
    fn reference_display_sizes_keep_main_and_upscale_workspaces_usable() {
        let reference_viewports = [
            (2732, 2048),
            (2208, 1242),
            (2048, 1536),
            (1920, 1080),
            (1440, 900),
            (1334, 750),
            (1280, 1024),
            (1280, 800),
            (1136, 640),
            (1024, 768),
        ];

        for (width, height) in reference_viewports {
            let preferences = UiPreferences {
                media_pane_width: 900,
                media_pane_expanded_width: 900,
                artwork_pane_width: 1600,
                artwork_pane_expanded_width: 1600,
                candidate_filter_height: 900,
                candidate_filter_expanded_height: 900,
                activity_pane_height: 900,
                ..UiPreferences::default()
            };
            let mut main = WorkspaceLayout::from_preferences(&preferences);
            main.clamp_for_window(width, height, true, true, true);

            let available_width = width.saturating_sub(38);
            assert!(main.media_pane_width >= MEDIA_PANE_MIN, "{width}x{height}");
            assert!(
                main.artwork_pane_width >= ARTWORK_PANE_MIN,
                "{width}x{height}"
            );
            assert!(
                main.media_pane_width + main.artwork_pane_width
                    <= available_width.saturating_sub(CENTER_PANE_MIN),
                "{width}x{height}"
            );

            let available_height = height.saturating_sub(91);
            let main_height =
                available_height.saturating_sub(main.activity_pane_height + SPLITTER_HIT_AREA);
            assert!(
                main_height.saturating_sub(main.candidate_filter_height + SPLITTER_HIT_AREA)
                    >= CANDIDATE_RESULTS_MIN,
                "{width}x{height}"
            );

            let mut upscale = WorkspaceLayout::from_preferences(&preferences);
            upscale.clamp_media_for_content(width, true, 650);
            assert!(
                upscale.media_pane_width >= MEDIA_PANE_MIN,
                "{width}x{height}"
            );
            assert!(
                width
                    .saturating_sub(38)
                    .saturating_sub(upscale.media_pane_width)
                    >= 650,
                "{width}x{height}"
            );
        }
    }

    #[test]
    fn collapsed_panes_reclaim_space_and_restore_expanded_widths() {
        assert_eq!(MEDIA_PANE_COLLAPSED, 0);
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
        layout.resize_region("activity", 165, bounds);
        assert_eq!(layout.artwork_pane_width, 470);
        assert_eq!(layout.artwork_pane_expanded_width, 470);
        assert_eq!(layout.candidate_filter_height, 305);
        assert_eq!(layout.candidate_filter_expanded_height, 305);
        assert_eq!(layout.activity_pane_height, 165);

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
    fn splitter_drag_does_not_resize_a_different_pane() {
        let mut layout = WorkspaceLayout::from_preferences(&UiPreferences::default());
        let bounds = WorkspaceBounds {
            window_width: 1920,
            window_height: 1009,
            media_expanded: true,
            artwork_visible: true,
            candidate_filter_expanded: true,
        };
        layout.clamp_for_window(
            bounds.window_width,
            bounds.window_height,
            bounds.media_expanded,
            bounds.artwork_visible,
            bounds.candidate_filter_expanded,
        );

        let candidate_before = layout.candidate_filter_height;
        layout.resize_region("activity", 420, bounds);
        assert_eq!(layout.candidate_filter_height, candidate_before);

        let activity_before = layout.activity_pane_height;
        layout.resize_region("candidate-filter", 280, bounds);
        assert_eq!(layout.activity_pane_height, activity_before);

        let artwork_before = layout.artwork_pane_width;
        layout.resize_region("media", 500, bounds);
        assert_eq!(layout.artwork_pane_width, artwork_before);

        let media_before = layout.media_pane_width;
        layout.resize_region("artwork", 600, bounds);
        assert_eq!(layout.media_pane_width, media_before);
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
        assert_eq!(normalize_appearance("chalk"), "Chalk");
        assert_eq!(normalize_appearance("anything else"), "System");
    }
}
