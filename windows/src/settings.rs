//! Config v5 and Rust-owned UI preference persistence.

use crate::config::{Config, load_config, load_config_text_at_root};
use crate::portable::{app_layout, state_root};
use crate::safe_write::replace_text_file;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPreferences {
    pub library_view: String,
    pub status_filter: String,
    pub include_bypassed: bool,
    pub window_width: u32,
    pub window_height: u32,
}

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            library_view: "artists".to_string(),
            status_filter: "all".to_string(),
            include_bypassed: false,
            window_width: 1380,
            window_height: 860,
        }
    }
}

pub fn config_text() -> Result<String, String> {
    let path = app_layout()?.config_file;
    std::fs::read_to_string(&path)
        .map_err(|error| format!("Unable to read Config v5 {}: {error}", path.display()))
}

pub fn save_config_text(text: &str) -> Result<Config, String> {
    let root = state_root()?;
    load_config_text_at_root(text, &root)?;
    let path = app_layout()?.config_file;
    replace_text_file(&path, text, "SPLINED configuration", |candidate| {
        load_config_text_at_root(candidate, &root).map(|_| ())
    })?;
    load_config()
}

pub fn save_config(config: &Config) -> Result<Config, String> {
    let root = state_root()?;
    let mut portable = config.clone();
    portable.library.music_library = portable_path(&root, &portable.library.music_library);
    portable.scan.scan_library_dir = portable_path(&root, &portable.scan.scan_library_dir);
    portable.scan.cache_dir = portable_path(&root, &portable.scan.cache_dir);
    portable.scan.temporary_cache_dir = portable_path(&root, &portable.scan.temporary_cache_dir);
    portable.scan.log_dir = portable_path(&root, &portable.scan.log_dir);
    portable.credentials.credential_dir =
        portable_path(&root, &portable.credentials.credential_dir);
    let text = toml::to_string_pretty(&portable)
        .map_err(|error| format!("Unable to serialize Config v5: {error}"))?;
    save_config_text(&text)
}

pub fn ui_preferences_path() -> Result<PathBuf, String> {
    Ok(app_layout()?.config_dir.join("ui.toml"))
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
}
