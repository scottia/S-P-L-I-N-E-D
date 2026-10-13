use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use sha2::{Digest, Sha256};

static APP_ROOT_OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static STATE_ROOT_OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

const LEGACY_CONFIG_VALUE: &str = "ConfigV5";
const LEGACY_UI_VALUE: &str = "UiV4";
const LEGACY_MIGRATION_VALUE: &str = "PortableFilesMigrated";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppLayout {
    pub root: PathBuf,
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub cache_dir: PathBuf,
    pub samples_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub credentials_dir: PathBuf,
    pub docker_builds_dir: PathBuf,
}

impl AppLayout {
    pub fn from_root(root: PathBuf) -> Self {
        let config_dir = root.join("data");
        let cache_dir = root.join("_cache");
        let logs_dir = root.join("_logs");
        let credentials_dir = root.join("credentials");
        let docker_builds_dir = root.join("docker_builds");

        Self {
            config_file: config_dir.join("config.toml"),
            samples_dir: cache_dir.join("samples"),
            root,
            config_dir,
            cache_dir,
            logs_dir,
            credentials_dir,
            docker_builds_dir,
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

/// Establish the directory containing the Rust application and its portable
/// assets. This is distinct from packaged LocalState.
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

/// Establish the writable configuration/state root selected at startup. It is
/// the executable root in portable mode and the package LocalState directory
/// in installed mode.
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
    state_root().map(AppLayout::from_root)
}

/// Imports the former per-installation HKCU settings exactly once when the
/// established file authorities are both absent. The registry is migration
/// input only; Config v5 and `ui.toml` remain the sole ongoing authorities.
pub fn migrate_legacy_settings_if_needed() -> Result<bool, String> {
    let layout = app_layout()?;
    let ui_path = layout.config_dir.join("ui.toml");
    if layout.config_file.is_file() || ui_path.is_file() {
        return Ok(false);
    }

    let Some(legacy) = read_legacy_settings(&app_root()?)? else {
        return Ok(false);
    };
    if legacy.already_migrated
        || (legacy.config.as_deref().is_none_or(str::is_empty)
            && legacy.ui.as_deref().is_none_or(str::is_empty))
    {
        return Ok(false);
    }

    migrate_legacy_values(&layout, &legacy, mark_legacy_settings_migrated)
}

struct LegacySettings {
    config: Option<String>,
    ui: Option<String>,
    already_migrated: bool,
}

fn migrate_legacy_values(
    layout: &AppLayout,
    legacy: &LegacySettings,
    mark_migrated: impl FnOnce() -> Result<(), String>,
) -> Result<bool, String> {
    create_owned_directories(layout)?;
    let ui_path = layout.config_dir.join("ui.toml");
    let mut created = Vec::new();
    let result = (|| {
        if let Some(config) = legacy.config.as_deref().filter(|value| !value.is_empty()) {
            write_new_file(&layout.config_file, config.as_bytes())?;
            created.push(layout.config_file.clone());
        }
        if let Some(ui) = legacy.ui.as_deref().filter(|value| !value.is_empty()) {
            write_new_file(&ui_path, ui.as_bytes())?;
            created.push(ui_path.clone());
        }
        mark_migrated()?;
        Ok(true)
    })();
    if result.is_err() {
        for path in created {
            let _ = fs::remove_file(path);
        }
    }
    result
}

fn write_new_file(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| {
            format!(
                "Unable to create migrated settings {}: {error}",
                path.display()
            )
        })?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| {
            format!(
                "Unable to persist migrated settings {}: {error}",
                path.display()
            )
        })
}

fn legacy_registry_path(root: &std::path::Path) -> String {
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

#[cfg(windows)]
fn read_legacy_settings(root: &std::path::Path) -> Result<Option<LegacySettings>, String> {
    let path = legacy_registry_path(root);
    let Ok(key) = windows_registry::CURRENT_USER.open(&path) else {
        return Ok(None);
    };
    Ok(Some(LegacySettings {
        config: key.get_string(LEGACY_CONFIG_VALUE).ok(),
        ui: key.get_string(LEGACY_UI_VALUE).ok(),
        already_migrated: key.get_u32(LEGACY_MIGRATION_VALUE).unwrap_or(0) == 1,
    }))
}

#[cfg(not(windows))]
fn read_legacy_settings(_root: &std::path::Path) -> Result<Option<LegacySettings>, String> {
    Ok(None)
}

#[cfg(windows)]
fn mark_legacy_settings_migrated() -> Result<(), String> {
    let path = legacy_registry_path(&app_root()?);
    let key = windows_registry::CURRENT_USER
        .options()
        .read()
        .write()
        .open(&path)
        .map_err(|error| format!("Unable to open legacy SPLINED settings marker: {error}"))?;
    key.set_u32(LEGACY_MIGRATION_VALUE, 1)
        .map_err(|error| format!("Unable to record legacy SPLINED settings migration: {error}"))
}

#[cfg(not(windows))]
fn mark_legacy_settings_migrated() -> Result<(), String> {
    Ok(())
}

/// Initializes the executable root and selects the correct writable state root.
/// A packaged MSIX process uses its user-specific LocalState directory; a
/// portable process uses the directory containing `splined.exe`.
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

/// Returns true only when the current executable has an MSIX package identity.
/// This is the single deployment-mode authority used by state-path and update
/// selection code; callers must not infer packaging from installation paths.
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

/// Ensures the persistent portable data layout exists. This function manages
/// configuration and data directories only; executable artifacts are never
/// copied, renamed, replaced, launched, or removed at runtime.
pub fn bootstrap_portable_install(default_config: &str) -> Result<AppLayout, String> {
    let layout = app_layout()?;
    create_owned_directories(&layout)?;
    if !layout.config_file.exists() {
        create_config(&layout, default_config)?;
    }
    Ok(layout)
}

fn create_owned_directories(layout: &AppLayout) -> Result<(), String> {
    for path in [
        &layout.config_dir,
        &layout.cache_dir,
        &layout.samples_dir,
        &layout.logs_dir,
        &layout.credentials_dir,
        &layout.docker_builds_dir,
    ] {
        fs::create_dir_all(path).map_err(|error| {
            format!(
                "Unable to create SPLINED application directory {}: {error}",
                path.display()
            )
        })?;
    }
    Ok(())
}

fn create_config(layout: &AppLayout, default_config: &str) -> Result<(), String> {
    let mut file = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&layout.config_file)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(()),
        Err(error) => {
            return Err(format!(
                "Unable to create SPLINED portable config {}: {error}",
                layout.config_file.display()
            ));
        }
    };
    file.write_all(default_config.as_bytes()).map_err(|error| {
        format!(
            "Unable to write SPLINED portable config {}: {error}",
            layout.config_file.display()
        )
    })?;
    file.sync_all().map_err(|error| {
        format!(
            "Unable to sync SPLINED portable config {}: {error}",
            layout.config_file.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use tempfile::TempDir;

    const DEFAULT_CONFIG: &str = "config_version = 5\nmode = \"read\"\n";

    #[test]
    fn layout_is_root_relative() {
        let root = PathBuf::from("portable-root");
        let layout = AppLayout::from_root(root.clone());
        assert_eq!(layout.config_file, root.join("data").join("config.toml"));
        assert_eq!(layout.cache_dir, root.join("_cache"));
        assert_eq!(layout.samples_dir, root.join("_cache").join("samples"));
        assert_eq!(layout.logs_dir, root.join("_logs"));
        assert_eq!(layout.credentials_dir, root.join("credentials"));
        assert_eq!(layout.docker_builds_dir, root.join("docker_builds"));
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
    fn data_bootstrap_creates_layout_without_overwriting_config() {
        let dir = TempDir::new().unwrap();
        let layout = AppLayout::from_root(dir.path().join("splined"));
        create_owned_directories(&layout).unwrap();
        create_config(&layout, DEFAULT_CONFIG).unwrap();
        assert_eq!(
            fs::read_to_string(&layout.config_file).unwrap(),
            DEFAULT_CONFIG
        );
        assert!(layout.samples_dir.is_dir());
        assert!(layout.logs_dir.is_dir());
        assert!(layout.credentials_dir.is_dir());

        fs::write(&layout.config_file, "custom").unwrap();
        create_config(&layout, DEFAULT_CONFIG).unwrap();
        assert_eq!(fs::read_to_string(&layout.config_file).unwrap(), "custom");
    }

    #[test]
    fn legacy_values_are_copied_once_and_marker_failure_rolls_back() {
        let dir = TempDir::new().unwrap();
        let layout = AppLayout::from_root(dir.path().join("splined"));
        let legacy = LegacySettings {
            config: Some("config_version = 4\n".to_string()),
            ui: Some("theme = \"dark\"\n".to_string()),
            already_migrated: false,
        };
        let marked = Cell::new(false);
        assert!(
            migrate_legacy_values(&layout, &legacy, || {
                marked.set(true);
                Ok(())
            })
            .unwrap()
        );
        assert!(marked.get());
        assert_eq!(
            fs::read_to_string(&layout.config_file).unwrap(),
            "config_version = 4\n"
        );
        assert_eq!(
            fs::read_to_string(layout.config_dir.join("ui.toml")).unwrap(),
            "theme = \"dark\"\n"
        );

        let failed = AppLayout::from_root(dir.path().join("failed"));
        assert!(
            migrate_legacy_values(&failed, &legacy, || Err("marker denied".to_string())).is_err()
        );
        assert!(!failed.config_file.exists());
        assert!(!failed.config_dir.join("ui.toml").exists());
    }

    #[test]
    fn legacy_registry_identity_is_stable_and_path_specific() {
        let first = legacy_registry_path(PathBuf::from(r"C:\Portable\SPLINED").as_path());
        let same = legacy_registry_path(PathBuf::from(r"c:\portable\splined\\").as_path());
        let other = legacy_registry_path(PathBuf::from(r"D:\Portable\SPLINED").as_path());
        assert_eq!(first, same);
        assert_ne!(first, other);
        assert!(first.starts_with(r"Software\SPLINED\WindowsV4\"));
        assert_eq!(first.rsplit('\\').next().unwrap().len(), 16);
    }
}
