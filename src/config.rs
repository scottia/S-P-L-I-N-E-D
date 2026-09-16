use crate::musicbrainz::MusicBrainzConfig;
use crate::portable::{app_layout, app_root, bootstrap_portable_install};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const CURRENT_CONFIG_VERSION: u32 = 4;
pub const SUPPORTED_COVER_SOURCES: [&str; 6] = [
    "deezer",
    "itunes",
    "fanarttv",
    "lastfm",
    "coverartarchive",
    "discogs",
];

// Public portable defaults are deliberately neutral. User library locations,
// ignore rules, and provider exclusions belong in the user's config, not in
// the compiled application.
pub const DEFAULT_MUSIC_LIBRARY: &str = "";
pub const DEFAULT_SCAN_LIBRARY_DIR: &str = "";
pub const DEFAULT_CACHE_DIR: &str = "cache";
pub const DEFAULT_SAMPLE_DIR: &str = DEFAULT_CACHE_DIR;
pub const DEFAULT_CREDENTIAL_DIR: &str = "credentials";
pub const DEFAULT_IGNORED_SUBS: [&str; 0] = [];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Write,
    Read,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verbosity {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RangeConfig {
    pub min: u32,
    pub ideal: u32,
    pub max: u32,
    pub ladder: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanConfig {
    pub scan_mode: bool,
    pub library_scan: bool,
    pub cache_dir: String,
    pub scan_library_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryConfig {
    pub music_library: String,
    pub ignored_subs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SamplesConfig {
    pub sample_write: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialsConfig {
    pub credential_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputConfig {
    pub preserve_file: bool,
    pub file_formats: Vec<String>,
    pub file_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReadConfig {
    pub sample_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SourcesConfig {
    pub cover_sources: Vec<String>,
    pub exclude_cover_sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LastFmConfig {
    pub credential_file: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FanartTvConfig {
    pub credential_file: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SplineAiConfig {
    pub enabled: bool,
    pub endpoint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_config_version")]
    pub config_version: u32,
    pub mode: Mode,
    pub verbosity: Verbosity,
    #[serde(default)]
    pub scan: ScanConfig,
    #[serde(default)]
    pub library: LibraryConfig,
    #[serde(default)]
    pub samples: SamplesConfig,
    // Legacy v1-v3 compatibility only. Accept old [read] when parsing but never
    // emit it into a v4 config.
    #[serde(default, skip_serializing)]
    pub read: ReadConfig,
    #[serde(default)]
    pub credentials: CredentialsConfig,
    #[serde(default)]
    pub fanarttv: FanartTvConfig,
    #[serde(default)]
    pub lastfm: LastFmConfig,
    #[serde(default)]
    pub musicbrainz: MusicBrainzConfig,
    #[serde(default)]
    pub splineai: SplineAiConfig,
    #[serde(default)]
    pub output: OutputConfig,
    pub range: RangeConfig,
    #[serde(default)]
    pub sources: SourcesConfig,
}

fn default_config_version() -> u32 {
    CURRENT_CONFIG_VERSION
}

impl Default for RangeConfig {
    fn default() -> Self {
        Self {
            min: 1200,
            ideal: 1800,
            max: 2400,
            ladder: 3600,
        }
    }
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            scan_mode: true,
            library_scan: false,
            cache_dir: DEFAULT_CACHE_DIR.to_string(),
            scan_library_dir: DEFAULT_SCAN_LIBRARY_DIR.to_string(),
        }
    }
}

impl Default for LibraryConfig {
    fn default() -> Self {
        Self {
            music_library: DEFAULT_MUSIC_LIBRARY.to_string(),
            ignored_subs: DEFAULT_IGNORED_SUBS
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
        }
    }
}

impl Default for SamplesConfig {
    fn default() -> Self {
        Self { sample_write: true }
    }
}

impl Default for CredentialsConfig {
    fn default() -> Self {
        Self {
            credential_dir: DEFAULT_CREDENTIAL_DIR.to_string(),
        }
    }
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            preserve_file: true,
            file_formats: vec!["jpeg".to_string(), "png".to_string(), "webp".to_string()],
            file_name: "cover".to_string(),
        }
    }
}

impl Default for SourcesConfig {
    fn default() -> Self {
        Self {
            cover_sources: SUPPORTED_COVER_SOURCES
                .iter()
                .map(|source| (*source).to_string())
                .collect(),
            exclude_cover_sources: Vec::new(),
        }
    }
}

impl Default for LastFmConfig {
    fn default() -> Self {
        Self {
            credential_file: "lastfm.json".to_string(),
        }
    }
}

impl Default for FanartTvConfig {
    fn default() -> Self {
        Self {
            credential_file: "fanarttv.json".to_string(),
        }
    }
}

impl Default for SplineAiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: String::new(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let musicbrainz = MusicBrainzConfig {
            token_file: "musicbrainz.json".to_string(),
            ..MusicBrainzConfig::default()
        };

        Self {
            config_version: CURRENT_CONFIG_VERSION,
            mode: Mode::Read,
            verbosity: Verbosity::Info,
            scan: ScanConfig::default(),
            library: LibraryConfig::default(),
            samples: SamplesConfig::default(),
            read: ReadConfig::default(),
            credentials: CredentialsConfig::default(),
            fanarttv: FanartTvConfig::default(),
            lastfm: LastFmConfig::default(),
            musicbrainz,
            splineai: SplineAiConfig::default(),
            output: OutputConfig::default(),
            range: RangeConfig::default(),
            sources: SourcesConfig::default(),
        }
    }
}

pub fn config_path() -> Option<PathBuf> {
    app_layout().ok().map(|layout| layout.config_file)
}

pub fn default_toml() -> Result<String, toml::ser::Error> {
    toml::to_string_pretty(&Config::default())
}

pub fn parse_config(text: &str) -> Result<Config, String> {
    let mut config: Config = toml::from_str(text)
        .map_err(|error| format!("Unable to parse SPLINED configuration: {error}"))?;

    if config.config_version > CURRENT_CONFIG_VERSION {
        return Err(format!(
            "SPLINED configuration version {} is newer than supported version {}.",
            config.config_version, CURRENT_CONFIG_VERSION
        ));
    }

    let range = crate::range::Range {
        min: config.range.min,
        ideal: config.range.ideal,
        max: config.range.max,
        ladder: config.range.ladder,
    };

    range
        .validate()
        .map_err(|error| format!("Invalid SPLINED range configuration: {error:?}"))?;

    if config.output.file_formats.is_empty() {
        return Err("SPLINED output file_formats cannot be empty.".to_string());
    }

    for format in &config.output.file_formats {
        match format.as_str() {
            "jpeg" | "png" | "webp" => {}
            _ => {
                return Err(format!(
                    "Unsupported SPLINED static output format: {format}"
                ));
            }
        }
    }

    if config.output.file_name.trim().is_empty() {
        return Err("SPLINED output file_name cannot be empty.".to_string());
    }

    config.library.music_library = normalize_optional_directory(&config.library.music_library);
    config.scan.scan_library_dir = normalize_optional_directory(&config.scan.scan_library_dir);
    config.scan.cache_dir = normalize_required_directory(&config.scan.cache_dir, "scan.cache_dir")?;
    config.credentials.credential_dir = normalize_required_directory(
        &config.credentials.credential_dir,
        "credentials.credential_dir",
    )?;
    config.library.ignored_subs = normalize_ignored_subs(&config.library.ignored_subs);
    config.splineai.endpoint = config.splineai.endpoint.trim().to_string();

    config.sources.cover_sources = normalize_source_list(
        &config.sources.cover_sources,
        "configured cover_sources",
        false,
    )?;
    config.sources.exclude_cover_sources = normalize_source_list(
        &config.sources.exclude_cover_sources,
        "configured exclude_cover_sources",
        true,
    )?;

    config.fanarttv.credential_file = resolve_configured_file(
        &config.credentials.credential_dir,
        &config.fanarttv.credential_file,
        "fanarttv.json",
    );
    config.lastfm.credential_file = resolve_configured_file(
        &config.credentials.credential_dir,
        &config.lastfm.credential_file,
        "lastfm.json",
    );
    config.musicbrainz.token_file = resolve_configured_file(
        &config.credentials.credential_dir,
        &config.musicbrainz.token_file,
        "musicbrainz.json",
    );

    Ok(config)
}

pub fn load_config() -> Result<Config, String> {
    let defaults =
        default_toml().map_err(|error| format!("Unable to generate SPLINED defaults: {error}"))?;
    let layout = bootstrap_portable_install(&defaults)?;
    let text = std::fs::read_to_string(&layout.config_file).map_err(|error| {
        format!(
            "Unable to read SPLINED configuration {}: {error}",
            layout.config_file.display()
        )
    })?;

    let mut config = parse_config(&text)?;
    let root = app_root()?;
    resolve_runtime_paths(&mut config, &root);

    // A completely bare native invocation is the operational scan command.
    // Keep explicit --scan-dir behavior config-driven, but make `splined`
    // itself scan the caller's current working directory recursively.
    if std::env::args_os().len() == 1 {
        let current_dir = std::env::current_dir()
            .map_err(|error| format!("Unable to determine current working directory: {error}"))?;
        config.scan.scan_library_dir = current_dir.to_string_lossy().into_owned();
    }

    Ok(config)
}

pub fn resolve_sources(
    config: &SourcesConfig,
    cover_sources_override: Option<&[String]>,
    only_cover_sources: Option<&[String]>,
    exclude_cover_sources: &[String],
) -> Result<Vec<String>, String> {
    let configured =
        normalize_source_list(&config.cover_sources, "configured cover_sources", false)?;
    let configured_exclusions = normalize_source_list(
        &config.exclude_cover_sources,
        "configured exclude_cover_sources",
        true,
    )?;
    let cli_exclusions =
        normalize_source_list(exclude_cover_sources, "--exclude-cover-sources", true)?;

    let base = if let Some(only) = only_cover_sources {
        normalize_source_list(only, "--only-cover-sources", false)?
    } else if let Some(override_sources) = cover_sources_override {
        normalize_source_list(override_sources, "--cover-sources", false)?
    } else {
        configured
    };

    Ok(base
        .into_iter()
        .filter(|source| !configured_exclusions.contains(source))
        .filter(|source| !cli_exclusions.contains(source))
        .collect())
}

pub fn scan_mode(write_enabled: bool) -> Mode {
    if write_enabled {
        Mode::Write
    } else {
        Mode::Read
    }
}

pub fn samples_dir(cache_dir: &str) -> PathBuf {
    PathBuf::from(cache_dir).join("samples")
}

fn resolve_runtime_paths(config: &mut Config, root: &Path) {
    config.library.music_library = resolve_runtime_directory(root, &config.library.music_library);
    config.scan.scan_library_dir = resolve_runtime_directory(root, &config.scan.scan_library_dir);
    config.scan.cache_dir = resolve_runtime_directory(root, &config.scan.cache_dir);
    config.credentials.credential_dir =
        resolve_runtime_directory(root, &config.credentials.credential_dir);
    config.fanarttv.credential_file =
        resolve_runtime_directory(root, &config.fanarttv.credential_file);
    config.lastfm.credential_file = resolve_runtime_directory(root, &config.lastfm.credential_file);
    config.musicbrainz.token_file = resolve_runtime_directory(root, &config.musicbrainz.token_file);
}

fn resolve_runtime_directory(root: &Path, value: &str) -> String {
    let value = value.trim();
    if value.is_empty() || path_is_absolute_or_unc(value) {
        return value.to_string();
    }

    root.join(value).to_string_lossy().into_owned()
}

fn normalize_optional_directory(value: &str) -> String {
    value.trim().to_string()
}

fn normalize_required_directory(value: &str, label: &str) -> Result<String, String> {
    let value = value.trim();

    if value.is_empty() {
        return Err(format!("SPLINED {label} cannot be empty."));
    }

    Ok(value.to_string())
}

fn normalize_ignored_subs(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    for raw in values {
        let value = raw.trim();
        if value.is_empty() {
            continue;
        }

        let key = value.to_ascii_lowercase();
        if seen.insert(key) {
            normalized.push(value.to_string());
        }
    }

    normalized
}

fn resolve_configured_file(
    credential_dir: &str,
    configured_file: &str,
    default_file: &str,
) -> String {
    let configured_file = configured_file.trim();
    let file = if configured_file.is_empty() {
        default_file
    } else {
        configured_file
    };

    if path_is_absolute_or_unc(file) {
        return file.to_string();
    }

    PathBuf::from(credential_dir)
        .join(file)
        .to_string_lossy()
        .into_owned()
}

fn path_is_absolute_or_unc(value: &str) -> bool {
    let path = Path::new(value);
    if path.is_absolute() || value.starts_with(r"\\") {
        return true;
    }

    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
        && bytes[0].is_ascii_alphabetic()
}

fn normalize_source_list(
    values: &[String],
    label: &str,
    allow_empty: bool,
) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    for raw in values {
        let source = raw.trim().to_ascii_lowercase();

        if source.is_empty() {
            continue;
        }

        if !SUPPORTED_COVER_SOURCES.contains(&source.as_str()) {
            return Err(format!(
                "Unsupported SPLINED cover source in {label}: {raw}"
            ));
        }

        if seen.insert(source.clone()) {
            normalized.push(source);
        }
    }

    if normalized.is_empty() && !allow_empty {
        return Err(format!("SPLINED {label} cannot be empty."));
    }

    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_neutral_portable_and_v4() {
        let config = Config::default();

        assert_eq!(config.config_version, 4);
        assert_eq!(config.mode, Mode::Read);
        assert_eq!(config.verbosity, Verbosity::Info);
        assert!(config.scan.scan_mode);
        assert!(!config.scan.library_scan);
        assert_eq!(config.scan.cache_dir, "cache");
        assert!(config.scan.scan_library_dir.is_empty());
        assert!(config.library.music_library.is_empty());
        assert!(config.library.ignored_subs.is_empty());
        assert!(config.samples.sample_write);
        assert_eq!(config.credentials.credential_dir, "credentials");
        assert!(!config.splineai.enabled);
        assert!(config.splineai.endpoint.is_empty());
        assert_eq!(
            config.output.file_formats,
            vec!["jpeg".to_string(), "png".to_string(), "webp".to_string()]
        );
        assert!(config.output.preserve_file);
        assert!(config.sources.exclude_cover_sources.is_empty());
        assert_eq!(
            config.sources.cover_sources.len(),
            SUPPORTED_COVER_SOURCES.len()
        );
    }

    #[test]
    fn default_toml_uses_only_relative_application_owned_paths() {
        let text = default_toml().expect("default config should serialize");
        let parsed: Config = toml::from_str(&text).expect("default config should parse");

        assert!(!text.contains("[read]"));
        assert!(text.contains("[splineai]"));
        assert_eq!(parsed.scan.cache_dir, "cache");
        assert_eq!(parsed.credentials.credential_dir, "credentials");
        assert!(!parsed.splineai.enabled);
        assert!(parsed.splineai.endpoint.is_empty());
        assert!(parsed.scan.scan_library_dir.is_empty());
        assert!(parsed.library.music_library.is_empty());
        assert!(parsed.library.ignored_subs.is_empty());
        assert!(parsed.sources.exclude_cover_sources.is_empty());
    }

    #[test]
    fn empty_library_and_scan_directories_are_allowed_until_scan_execution() {
        let text = default_toml().expect("default config should serialize");
        let parsed = parse_config(&text).expect("neutral default config should parse");

        assert!(parsed.library.music_library.is_empty());
        assert!(parsed.scan.scan_library_dir.is_empty());
    }

    #[test]
    fn relative_credential_files_resolve_under_configured_directory() {
        let mut config = Config::default();
        config.credentials.credential_dir = "portable-credentials".to_string();
        let text = toml::to_string_pretty(&config).unwrap();
        let parsed = parse_config(&text).unwrap();

        assert_eq!(
            parsed.fanarttv.credential_file,
            PathBuf::from("portable-credentials")
                .join("fanarttv.json")
                .to_string_lossy()
        );
        assert_eq!(
            parsed.lastfm.credential_file,
            PathBuf::from("portable-credentials")
                .join("lastfm.json")
                .to_string_lossy()
        );
        assert_eq!(
            parsed.musicbrainz.token_file,
            PathBuf::from("portable-credentials")
                .join("musicbrainz.json")
                .to_string_lossy()
        );
    }

    #[test]
    fn samples_directory_is_derived_from_cache_directory() {
        assert_eq!(
            samples_dir("portable-cache"),
            PathBuf::from("portable-cache").join("samples")
        );
    }

    #[test]
    fn relative_runtime_paths_resolve_from_portable_root() {
        let root = Path::new("portable-root");
        let mut config = parse_config(&default_toml().unwrap()).unwrap();

        resolve_runtime_paths(&mut config, root);

        assert_eq!(config.scan.cache_dir, root.join("cache").to_string_lossy());
        assert_eq!(
            config.credentials.credential_dir,
            root.join("credentials")
                .to_string_lossy()
        );
        assert_eq!(
            config.lastfm.credential_file,
            root.join("credentials")
                .join("lastfm.json")
                .to_string_lossy()
        );
        assert!(config.library.music_library.is_empty());
        assert!(config.scan.scan_library_dir.is_empty());
    }
}
