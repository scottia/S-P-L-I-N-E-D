use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const APP_ROOT_ENV: &str = "SPLINED_HOME";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppLayout {
    pub root: PathBuf,
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub cache_dir: PathBuf,
    pub samples_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub history_dir: PathBuf,
    pub credentials_dir: PathBuf,
    pub docker_builds_dir: PathBuf,
}

impl AppLayout {
    pub fn from_root(root: PathBuf) -> Self {
        let config_dir = root.join("config");
        let cache_dir = root.join("_cache");
        let logs_dir = root.join("_logs");
        let credentials_dir = root.join("credentials");
        let docker_builds_dir = root.join("docker_builds");

        Self {
            config_file: config_dir.join("config.toml"),
            samples_dir: cache_dir.join("samples"),
            history_dir: logs_dir.join("_history"),
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
    if let Some(configured) = std::env::var_os(APP_ROOT_ENV) {
        let configured = PathBuf::from(configured);
        if configured.as_os_str().is_empty() {
            return Err(format!("{APP_ROOT_ENV} cannot be empty."));
        }

        return if configured.is_absolute() {
            Ok(configured)
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(configured))
                .map_err(|error| format!("Unable to resolve {APP_ROOT_ENV}: {error}"))
        };
    }

    let executable = std::env::current_exe()
        .map_err(|error| format!("Unable to determine SPLINED executable path: {error}"))?;

    executable
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "Unable to determine SPLINED application directory.".to_string())
}

pub fn app_layout() -> Result<AppLayout, String> {
    app_root().map(AppLayout::from_root)
}

#[cfg(windows)]
const SETUP_EXECUTABLE_NAME: &str = "setup-splined.exe";

#[cfg(not(windows))]
const SETUP_EXECUTABLE_NAME: &str = "setup-splined";

#[cfg(windows)]
const FINAL_EXECUTABLE_NAME: &str = "splined.exe";

#[cfg(not(windows))]
const FINAL_EXECUTABLE_NAME: &str = "splined";

#[cfg(windows)]
const RELEASE_README_NAME: &str = "README-WINDOWS.txt";

#[cfg(target_os = "macos")]
const RELEASE_README_NAME: &str = "README-MACOS.txt";

#[cfg(all(not(windows), not(target_os = "macos")))]
const RELEASE_README_NAME: &str = "README-LINUX.txt";

fn setup_executable_name_matches(file_name: &str) -> bool {
    #[cfg(windows)]
    {
        file_name.eq_ignore_ascii_case(SETUP_EXECUTABLE_NAME)
    }

    #[cfg(not(windows))]
    {
        file_name == SETUP_EXECUTABLE_NAME
    }
}

fn executable_path_is_setup(current_exe: &Path) -> Result<bool, String> {
    let file_name = current_exe
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Unable to determine SPLINED executable filename.".to_string())?;

    Ok(setup_executable_name_matches(file_name))
}

pub fn running_as_setup_executable() -> Result<bool, String> {
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("Unable to determine SPLINED executable path: {error}"))?;

    executable_path_is_setup(&current_exe)
}

fn install_final_executable_from(current_exe: &Path) -> Result<Option<PathBuf>, String> {
    if !executable_path_is_setup(current_exe)? {
        return Ok(None);
    }

    let parent = current_exe
        .parent()
        .ok_or_else(|| "Unable to determine SPLINED application directory.".to_string())?;

    let final_exe = parent.join(FINAL_EXECUTABLE_NAME);
    let stamp = unique_stamp();
    let staging = parent.join(format!(".splined-install-{}-{stamp}", std::process::id()));

    fs::copy(current_exe, &staging).map_err(|error| {
        format!(
            "Unable to prepare permanent SPLINED executable {} -> {}: {error}",
            current_exe.display(),
            staging.display()
        )
    })?;

    if final_exe.exists() {
        let backup = parent.join(format!(".splined-backup-{}-{stamp}", std::process::id()));

        if let Err(error) = fs::rename(&final_exe, &backup) {
            let _ = fs::remove_file(&staging);
            return Err(format!(
                "Unable to prepare existing SPLINED executable {} for upgrade: {error}",
                final_exe.display()
            ));
        }

        if let Err(error) = fs::rename(&staging, &final_exe) {
            let restore_result = fs::rename(&backup, &final_exe);
            let _ = fs::remove_file(&staging);

            return match restore_result {
                Ok(()) => Err(format!(
                    "Unable to install upgraded SPLINED executable {}: {error}. Previous executable was restored.",
                    final_exe.display()
                )),
                Err(restore_error) => Err(format!(
                    "Unable to install upgraded SPLINED executable {}: {error}. Automatic rollback also failed: {restore_error}. Previous executable remains at {}.",
                    final_exe.display(),
                    backup.display()
                )),
            };
        }

        if let Err(error) = fs::remove_file(&backup) {
            eprintln!(
                "Warning: SPLINED was upgraded, but the previous executable backup could not be removed: {}: {error}",
                backup.display()
            );
        }
    } else if let Err(error) = fs::rename(&staging, &final_exe) {
        let _ = fs::remove_file(&staging);
        return Err(format!(
            "Unable to install permanent SPLINED executable {}: {error}",
            final_exe.display()
        ));
    }

    Ok(Some(final_exe))
}

fn remove_release_readme(parent: &Path) {
    let readme = parent.join(RELEASE_README_NAME);

    if readme.exists()
        && let Err(error) = fs::remove_file(&readme)
    {
        eprintln!(
            "Warning: SPLINED setup completed, but release instructions could not be removed: {}: {error}",
            readme.display()
        );
    }
}

#[cfg(windows)]
fn remove_setup_executable_after_exit(current_exe: &Path) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let mut command = std::process::Command::new("cmd.exe");
    command
        .arg("/C")
        .arg("ping 127.0.0.1 -n 3 >nul & del /F /Q \"%SPLINED_SETUP_EXE%\"")
        .env("SPLINED_SETUP_EXE", current_exe)
        .creation_flags(CREATE_NO_WINDOW);

    if let Err(error) = command.spawn() {
        eprintln!(
            "Warning: SPLINED setup completed, but automatic setup launcher cleanup could not be scheduled for {}: {error}",
            current_exe.display()
        );
    }
}

#[cfg(not(windows))]
fn remove_setup_executable_after_exit(current_exe: &Path) {
    if let Err(error) = fs::remove_file(current_exe) {
        eprintln!(
            "Warning: SPLINED setup completed, but the setup launcher could not be removed: {}: {error}",
            current_exe.display()
        );
    }
}

fn finish_setup_executable() -> Result<(), String> {
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("Unable to determine SPLINED executable path: {error}"))?;

    if let Some(final_exe) = install_final_executable_from(&current_exe)? {
        let parent = current_exe
            .parent()
            .ok_or_else(|| "Unable to determine SPLINED application directory.".to_string())?;

        remove_release_readme(parent);
        remove_setup_executable_after_exit(&current_exe);

        println!("SPLINED setup complete.");
        println!();
        println!("Permanent executable:");
        println!("  {}", final_exe.display());
        println!();
        println!("Existing config, credentials, logs, and history were preserved.");
        println!("Disposable cache is recreated as needed by scan operations.");
        println!("Temporary setup files are removed automatically.");
        println!();
        println!("Use this executable for future SPLINED launches.");
        println!();
    }

    Ok(())
}

pub fn bootstrap_portable_install(default_config: &str) -> Result<AppLayout, String> {
    let layout = app_layout()?;

    if layout.config_file.exists() {
        create_owned_directories(&layout)?;
    } else {
        create_new_install(&layout, default_config)?;
    }

    finish_setup_executable()?;
    Ok(layout)
}

fn create_owned_directories(layout: &AppLayout) -> Result<(), String> {
    for path in [
        &layout.config_dir,
        &layout.cache_dir,
        &layout.samples_dir,
        &layout.logs_dir,
        &layout.history_dir,
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

fn create_new_install(layout: &AppLayout, default_config: &str) -> Result<(), String> {
    create_owned_directories(layout)?;

    if layout.config_file.exists() {
        return Ok(());
    }

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&layout.config_file)
        .map_err(|error| {
            format!(
                "Unable to create SPLINED portable config {}: {error}",
                layout.config_file.display()
            )
        })?;

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
    })?;

    println!("SPLINED portable setup created:");
    println!("  {}", layout.root.display());
    println!("Config:");
    println!("  {}", layout.config_file.display());
    println!();

    Ok(())
}

fn unique_stamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const DEFAULT_CONFIG: &str = "config_version = 4\nmode = \"read\"\n";

    #[test]
    fn layout_is_root_relative() {
        let root = PathBuf::from("portable-root");
        let layout = AppLayout::from_root(root.clone());

        assert_eq!(layout.config_file, root.join("config").join("config.toml"));
        assert_eq!(layout.cache_dir, root.join("_cache"));
        assert_eq!(layout.samples_dir, root.join("_cache").join("samples"));
        assert_eq!(layout.logs_dir, root.join("_logs"));
        assert_eq!(layout.history_dir, root.join("_logs").join("_history"));
        assert_eq!(layout.credentials_dir, root.join("credentials"));
        assert_eq!(layout.docker_builds_dir, root.join("docker_builds"));
    }

    #[test]
    fn setup_name_is_detected_without_reading_os_configuration() {
        let path = PathBuf::from("portable-root").join(SETUP_EXECUTABLE_NAME);
        assert!(executable_path_is_setup(&path).unwrap());

        let path = PathBuf::from("portable-root").join(FINAL_EXECUTABLE_NAME);
        assert!(!executable_path_is_setup(&path).unwrap());
    }

    #[test]
    fn new_install_creates_expected_structure_without_overwriting_config() {
        let dir = TempDir::new().unwrap();
        let layout = AppLayout::from_root(dir.path().join("splined"));
        fs::create_dir_all(&layout.root).unwrap();

        create_new_install(&layout, DEFAULT_CONFIG).unwrap();

        assert_eq!(
            fs::read_to_string(&layout.config_file).unwrap(),
            DEFAULT_CONFIG
        );
        assert!(layout.samples_dir.is_dir());
        assert!(layout.logs_dir.is_dir());
        assert!(layout.history_dir.is_dir());
        assert!(layout.credentials_dir.is_dir());
        assert!(layout.docker_builds_dir.is_dir());
        assert_eq!(fs::read_dir(&layout.credentials_dir).unwrap().count(), 0);

        fs::write(&layout.config_file, "custom").unwrap();
        create_new_install(&layout, DEFAULT_CONFIG).unwrap();
        assert_eq!(fs::read_to_string(&layout.config_file).unwrap(), "custom");
    }

    #[test]
    fn release_readme_cleanup_removes_only_release_instruction_file() {
        let dir = TempDir::new().unwrap();
        let readme = dir.path().join(RELEASE_README_NAME);
        let protected = dir.path().join("config.toml");

        fs::write(&readme, b"temporary release instructions").unwrap();
        fs::write(&protected, b"preserve me").unwrap();

        remove_release_readme(dir.path());

        assert!(!readme.exists());
        assert_eq!(fs::read(&protected).unwrap(), b"preserve me");
    }

    #[test]
    fn setup_executable_creates_permanent_executable_without_removing_setup() {
        let dir = TempDir::new().unwrap();
        let setup = dir.path().join(SETUP_EXECUTABLE_NAME);

        fs::write(&setup, b"splined-test-executable").unwrap();

        let installed = install_final_executable_from(&setup)
            .unwrap()
            .expect("setup executable should install permanent executable");

        assert_eq!(installed, dir.path().join(FINAL_EXECUTABLE_NAME));
        assert!(setup.is_file());
        assert!(installed.is_file());
        assert_eq!(fs::read(&installed).unwrap(), b"splined-test-executable");
    }

    #[test]
    fn setup_executable_upgrades_program_without_touching_persistent_data() {
        let dir = TempDir::new().unwrap();
        let layout = AppLayout::from_root(dir.path().to_path_buf());
        create_owned_directories(&layout).unwrap();

        let setup = layout.root.join(SETUP_EXECUTABLE_NAME);
        let installed = layout.root.join(FINAL_EXECUTABLE_NAME);

        fs::write(&setup, b"new executable").unwrap();
        fs::write(&installed, b"old executable").unwrap();
        fs::write(&layout.config_file, b"custom portable config").unwrap();
        fs::write(
            layout.credentials_dir.join("lastfm.json"),
            b"existing credential",
        )
        .unwrap();
        fs::write(
            layout.history_dir.join("scan-completed-history.json"),
            b"existing history",
        )
        .unwrap();

        let result = install_final_executable_from(&setup)
            .unwrap()
            .expect("setup executable should install upgrade");

        assert_eq!(result, installed);
        assert_eq!(fs::read(&installed).unwrap(), b"new executable");
        assert_eq!(
            fs::read(&layout.config_file).unwrap(),
            b"custom portable config"
        );
        assert_eq!(
            fs::read(layout.credentials_dir.join("lastfm.json")).unwrap(),
            b"existing credential"
        );
        assert_eq!(
            fs::read(layout.history_dir.join("scan-completed-history.json")).unwrap(),
            b"existing history"
        );
        assert!(setup.is_file());
    }
}
