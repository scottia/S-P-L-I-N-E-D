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
}
