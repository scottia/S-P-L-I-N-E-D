//! Windows application and configuration-state root discovery.
//!
//! This module deliberately performs no filesystem writes. Portable state is
//! rooted beside `splined.exe`; packaged state is rooted under the package's
//! LocalState directory. Configuration/setup code creates configuration and
//! runtime files only after an explicit save.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;

static APP_ROOT_OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static STATE_ROOT_OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppLayout {
    pub root: PathBuf,
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub ui_file: PathBuf,
}

impl AppLayout {
    pub fn from_root(root: PathBuf) -> Self {
        let config_dir = root.clone();
        Self::from_root_and_config_dir(root, config_dir)
    }

    pub fn from_root_and_config_dir(root: PathBuf, config_dir: PathBuf) -> Self {
        Self {
            config_file: config_dir.join("config.toml"),
            ui_file: config_dir.join("ui.toml"),
            root,
            config_dir,
        }
    }
}

pub fn app_root() -> Result<PathBuf, String> {
    if let Some(root) = APP_ROOT_OVERRIDE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "SPLINED application-root authority is unavailable.".to_string())?
        .clone()
    {
        return Ok(root);
    }
    let executable = std::env::current_exe()
        .map_err(|error| format!("Unable to determine SPLINED executable path: {error}"))?;
    portable_root_from_executable(&executable)
}

pub fn initialize_app_root(root: &std::path::Path) -> Result<PathBuf, String> {
    if !root.is_absolute() {
        return Err("The SPLINED application root must be an absolute path.".to_string());
    }
    let root = root.to_path_buf();
    let slot = APP_ROOT_OVERRIDE.get_or_init(|| Mutex::new(None));
    let mut current = slot
        .lock()
        .map_err(|_| "SPLINED application-root authority is unavailable.".to_string())?;
    if let Some(existing) = current.as_ref() {
        if existing != &root {
            return Err(
                "The SPLINED application root cannot change during a process lifetime.".to_string(),
            );
        }
    } else {
        *current = Some(root.clone());
    }
    Ok(root)
}

pub fn initialize_state_root(root: &std::path::Path) -> Result<PathBuf, String> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if !root.is_absolute() {
        return Err("The SPLINED state root must be absolute.".to_string());
    }
    let slot = STATE_ROOT_OVERRIDE.get_or_init(|| Mutex::new(None));
    let mut guard = slot
        .lock()
        .map_err(|_| "The SPLINED state-root lock is poisoned.".to_string())?;
    if let Some(existing) = guard.as_ref() {
        if existing != &root {
            return Err(
                "The SPLINED state root cannot change while the application is running."
                    .to_string(),
            );
        }
    } else {
        *guard = Some(root.clone());
    }
    Ok(root)
}

pub fn state_root() -> Result<PathBuf, String> {
    if let Some(slot) = STATE_ROOT_OVERRIDE.get()
        && let Ok(guard) = slot.lock()
        && let Some(root) = guard.as_ref()
    {
        return Ok(root.clone());
    }
    app_root()
}

pub(crate) fn portable_root_from_executable(
    executable: &std::path::Path,
) -> Result<PathBuf, String> {
    let parent = executable
        .parent()
        .ok_or_else(|| "Unable to determine SPLINED application directory.".to_string())?;
    Ok(parent.to_path_buf())
}

pub fn app_layout() -> Result<AppLayout, String> {
    let root = state_root()?;
    let config_dir = configuration_directory_for_root(&root)?;
    Ok(AppLayout::from_root_and_config_dir(root, config_dir))
}

#[derive(Debug, Deserialize)]
struct ConfigurationLocation {
    directory: PathBuf,
}

pub fn default_configuration_directory(root: &Path) -> PathBuf {
    root.to_path_buf()
}

/// The locator belongs to this installation. A portable copy cannot inherit a
/// path selected by an unrelated test or installation under the same account.
pub fn configuration_locator_path() -> Result<PathBuf, String> {
    Ok(state_root()?.join("configuration-location.toml"))
}

pub fn configuration_directory_for_root(root: &Path) -> Result<PathBuf, String> {
    let locator = root.join("configuration-location.toml");
    if locator.is_file() {
        return configuration_directory_from_locator(root, &locator);
    }
    // Existing portable releases stored both TOMLs under data/. Keep those
    // installations loadable without manufacturing a locator during startup.
    let legacy = root.join("data");
    if legacy.join("config.toml").is_file() {
        return Ok(legacy);
    }
    Ok(default_configuration_directory(root))
}

pub fn configuration_directory_from_locator(
    root: &Path,
    locator: &Path,
) -> Result<PathBuf, String> {
    if !locator.is_file() {
        return Ok(default_configuration_directory(root));
    }
    let body = std::fs::read_to_string(locator).map_err(|error| {
        format!(
            "Unable to read the SPLINED configuration locator {}: {error}",
            locator.display()
        )
    })?;
    let location: ConfigurationLocation = toml::from_str(&body).map_err(|error| {
        format!(
            "Invalid SPLINED configuration locator {}: {error}",
            locator.display()
        )
    })?;
    if !location.directory.is_absolute() {
        return Err(format!(
            "The SPLINED configuration directory must be absolute: {}",
            location.directory.display()
        ));
    }
    Ok(location.directory)
}

/// Format an internal Windows path for editable UI and operator-facing text.
/// `canonicalize` may add the Win32 extended-length prefix; it is useful for
/// filesystem calls but needlessly alarming in Setup and Settings.
pub fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(remainder) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{remainder}")
    } else if let Some(remainder) = value.strip_prefix(r"\\?\") {
        remainder.to_string()
    } else {
        value.into_owned()
    }
}

/// Selects the correct state root without creating it. A packaged MSIX process
/// uses its user-specific LocalState SPLINED root; a portable process uses the
/// directory containing `splined.exe`.
pub fn initialize_process_roots() -> Result<AppLayout, String> {
    let application_root = app_root()?;
    initialize_app_root(&application_root)?;
    let writable_root = match std::env::var_os("SPLINED_INTERNAL_STATE_ROOT") {
        Some(root) => {
            let root = PathBuf::from(root);
            if !root.is_absolute() {
                return Err("SPLINED_INTERNAL_STATE_ROOT must be an absolute path.".to_string());
            }
            root
        }
        None => packaged_local_state()?.unwrap_or(application_root),
    };
    initialize_state_root(&writable_root)?;
    app_layout()
}

pub fn is_packaged_process() -> Result<bool, String> {
    packaged_local_state().map(|path| path.is_some())
}

#[cfg(not(windows))]
fn packaged_local_state() -> Result<Option<PathBuf>, String> {
    Ok(None)
}

#[cfg(windows)]
fn packaged_local_state() -> Result<Option<PathBuf>, String> {
    const ERROR_INSUFFICIENT_BUFFER: i32 = 122;
    const APPMODEL_ERROR_NO_PACKAGE: i32 = 15_700;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentPackageFamilyName(length: *mut u32, family_name: *mut u16) -> i32;
    }

    let mut length = 0_u32;
    let status = unsafe { GetCurrentPackageFamilyName(&mut length, std::ptr::null_mut()) };
    if status == APPMODEL_ERROR_NO_PACKAGE {
        return Ok(None);
    }
    if status != ERROR_INSUFFICIENT_BUFFER || length == 0 {
        return Err(format!(
            "Unable to query the SPLINED package identity (Windows error {status})."
        ));
    }
    let mut buffer = vec![0_u16; length as usize];
    let status = unsafe { GetCurrentPackageFamilyName(&mut length, buffer.as_mut_ptr()) };
    if status != 0 {
        return Err(format!(
            "Unable to read the SPLINED package identity (Windows error {status})."
        ));
    }
    let nul = buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len());
    let family = String::from_utf16(&buffer[..nul])
        .map_err(|error| format!("Windows returned an invalid package family name: {error}"))?;
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| "LOCALAPPDATA is unavailable for packaged SPLINED state.".to_string())?;
    Ok(Some(
        PathBuf::from(local_app_data)
            .join("Packages")
            .join(family)
            .join("LocalState")
            .join("SPLINED"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_contains_only_file_authorities() {
        let root = PathBuf::from("portable-root");
        let layout = AppLayout::from_root(root.clone());
        assert_eq!(layout.config_dir, root);
        assert_eq!(layout.config_file, root.join("config.toml"));
        assert_eq!(layout.ui_file, root.join("ui.toml"));
        assert!(!root.join("_cache").exists());
    }

    #[test]
    fn root_discovery_does_not_create_directories() {
        let parent = tempfile::TempDir::new().unwrap();
        let root = parent.path().join("clean-portable");
        let layout = AppLayout::from_root(root.clone());
        assert!(!root.exists());
        assert!(!layout.config_dir.exists());
    }

    #[test]
    fn explicit_locator_selects_an_absolute_configuration_directory() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().join("portable");
        let selected = temp.path().join("settings");
        let locator = temp.path().join("locator.toml");
        std::fs::write(
            &locator,
            format!("directory = {:?}\n", selected.to_string_lossy()),
        )
        .unwrap();

        assert_eq!(
            configuration_directory_from_locator(&root, &locator).unwrap(),
            selected
        );
        assert_eq!(
            configuration_directory_from_locator(&root, &temp.path().join("missing.toml")).unwrap(),
            root
        );
    }

    #[test]
    fn legacy_data_configuration_is_discovered_without_writing_a_locator() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().join("portable");
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(root.join("data/config.toml"), "config_version = 5\n").unwrap();

        assert_eq!(
            configuration_directory_for_root(&root).unwrap(),
            root.join("data")
        );
        assert!(!root.join("configuration-location.toml").exists());
    }

    #[test]
    fn executable_parent_is_the_portable_root() {
        let direct = PathBuf::from("portable-root").join("splined.exe");
        assert_eq!(
            portable_root_from_executable(&direct).unwrap(),
            PathBuf::from("portable-root")
        );
    }

    #[test]
    fn display_paths_hide_internal_windows_extended_prefixes() {
        assert_eq!(
            display_path(Path::new(r"\\?\E:\apps\SPLINED\_db")),
            r"E:\apps\SPLINED\_db"
        );
        assert_eq!(
            display_path(Path::new(r"\\?\UNC\server\music\_db")),
            r"\\server\music\_db"
        );
        assert_eq!(display_path(Path::new(r"N:\music\_db")), r"N:\music\_db");
    }
}
