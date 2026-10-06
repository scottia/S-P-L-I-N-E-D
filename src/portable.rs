use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

pub const APP_ROOT_ENV: &str = "SPLINED_HOME";

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
        let config_dir = root.join("config");
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
        .map(PathBuf::from)
        .ok_or_else(|| "Unable to determine SPLINED application directory.".to_string())
}

pub fn app_layout() -> Result<AppLayout, String> {
    app_root().map(AppLayout::from_root)
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
    use tempfile::TempDir;

    const DEFAULT_CONFIG: &str = "config_version = 5\nmode = \"read\"\n";

    #[test]
    fn layout_is_root_relative() {
        let root = PathBuf::from("portable-root");
        let layout = AppLayout::from_root(root.clone());
        assert_eq!(layout.config_file, root.join("config").join("config.toml"));
        assert_eq!(layout.cache_dir, root.join("_cache"));
        assert_eq!(layout.samples_dir, root.join("_cache").join("samples"));
        assert_eq!(layout.logs_dir, root.join("_logs"));
        assert_eq!(layout.credentials_dir, root.join("credentials"));
        assert_eq!(layout.docker_builds_dir, root.join("docker_builds"));
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
}
