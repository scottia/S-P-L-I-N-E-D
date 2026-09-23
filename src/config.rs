use crate::musicbrainz::MusicBrainzConfig;
use crate::portable::{app_layout, app_root, bootstrap_portable_install};
use crate::source_policy::SourcePolicyConfig;
use serde::de::{self, Visitor};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

pub const CURRENT_CONFIG_VERSION: u32 = 5;
pub const SUPPORTED_COVER_SOURCES: [&str; 6] = [
    "deezer",
    "itunes",
    "fanarttv",
    "lastfm",
    "coverartarchive",
    "discogs",
];
pub const SUPPORTED_SOURCE_POLICIES: [&str; 7] = [
    "deezer",
    "itunes",
    "fanarttv",
    "lastfm",
    "coverartarchive",
    "discogs",
    "musicbrainz",
];

// Public portable defaults are deliberately neutral. User library locations,
// ignore rules, and provider exclusions belong in the user's config, not in
// the compiled application.
pub const DEFAULT_MUSIC_LIBRARY: &str = "";
pub const DEFAULT_SCAN_LIBRARY_DIR: &str = "";
pub const DEFAULT_CACHE_DIR: &str = "_cache";
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
    #[serde(default)]
    pub scan_mode_timeout: ScanTimeout,
    pub cache_dir: String,
    #[serde(default = "default_log_dir")]
    pub log_dir: String,
    #[serde(default = "default_history_dir")]
    pub history_dir: String,
    pub scan_library_dir: String,
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct ScanTimeout(pub f64);

impl Eq for ScanTimeout {}

impl Default for ScanTimeout {
    fn default() -> Self {
        Self(24.0)
    }
}

impl Serialize for ScanTimeout {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_f64(self.0)
    }
}

struct ScanTimeoutVisitor;

impl<'de> Visitor<'de> for ScanTimeoutVisitor {
    type Value = ScanTimeout;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a non-negative number of hours, false, or 'off'")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if value {
            Err(E::custom("scan_mode_timeout may be false, but not true"))
        } else {
            Ok(ScanTimeout(0.0))
        }
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_f64(value as f64)
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_f64(value as f64)
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if value.is_finite() && value >= 0.0 {
            Ok(ScanTimeout(value))
        } else {
            Err(E::custom(
                "scan_mode_timeout must be finite and non-negative",
            ))
        }
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if value.trim().eq_ignore_ascii_case("off") {
            return Ok(ScanTimeout(0.0));
        }
        value
            .trim()
            .parse::<f64>()
            .map_err(E::custom)
            .and_then(|value| self.visit_f64(value))
    }
}

impl<'de> Deserialize<'de> for ScanTimeout {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(ScanTimeoutVisitor)
    }
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
#[serde(default)]
pub struct OutputConfig {
    pub preserve_file: bool,
    pub file_formats: Vec<String>,
    pub file_name: String,
    pub square: bool,
    pub square_mode: String,
    pub square_round_to: u32,
    pub upscale_below_ideal: bool,
    pub evaluate_final_image: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    pub retention_days: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryConfig {
    pub enabled: bool,
    /// Zero means keep history forever.
    pub retention_days: u32,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSplinedConfig {
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
    // emit it into a v5 config.
    #[serde(default, skip_serializing)]
    pub read: ReadConfig,
    #[serde(default)]
    pub credentials: CredentialsConfig,
    #[serde(default, skip_serializing)]
    pub fanarttv: FanartTvConfig,
    #[serde(default, skip_serializing)]
    pub lastfm: LastFmConfig,
    #[serde(default, skip_serializing)]
    pub musicbrainz: MusicBrainzConfig,
    #[serde(default, alias = "splineai")]
    pub aisplined: AiSplinedConfig,
    #[serde(default)]
    pub output: OutputConfig,
    pub range: RangeConfig,
    #[serde(default)]
    pub sources: SourcesConfig,
    #[serde(default)]
    pub source_policies: BTreeMap<String, SourcePolicyConfig>,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub history: HistoryConfig,
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
            scan_mode_timeout: ScanTimeout::default(),
            cache_dir: DEFAULT_CACHE_DIR.to_string(),
            log_dir: default_log_dir(),
            history_dir: default_history_dir(),
            scan_library_dir: DEFAULT_SCAN_LIBRARY_DIR.to_string(),
        }
    }
}

fn default_log_dir() -> String {
    "_logs".to_string()
}

fn default_history_dir() -> String {
    "_logs/_history".to_string()
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
            square: true,
            square_mode: "crop".to_string(),
            square_round_to: 16,
            upscale_below_ideal: false,
            evaluate_final_image: true,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self { retention_days: 14 }
    }
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: 0,
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
            aisplined: AiSplinedConfig::default(),
            output: OutputConfig::default(),
            range: RangeConfig::default(),
            sources: SourcesConfig::default(),
            source_policies: BTreeMap::new(),
            logging: LoggingConfig::default(),
            history: HistoryConfig::default(),
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

    if config.config_version != CURRENT_CONFIG_VERSION {
        return Err(format!(
            "Unsupported SPLINED configuration version {}; expected version {}.",
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

    if config.output.square_mode != "off" && config.output.square_mode != "crop" {
        return Err("SPLINED output square_mode must be 'off' or 'crop'.".to_string());
    }

    if config.scan.scan_mode_timeout.0 < 0.0 || !config.scan.scan_mode_timeout.0.is_finite() {
        return Err("SPLINED scan_mode_timeout must be finite and non-negative.".to_string());
    }

    config.library.music_library = normalize_optional_directory(&config.library.music_library);
    config.scan.scan_library_dir = normalize_optional_directory(&config.scan.scan_library_dir);
    config.scan.cache_dir = normalize_required_directory(&config.scan.cache_dir, "scan.cache_dir")?;
    config.scan.log_dir = normalize_required_directory(&config.scan.log_dir, "scan.log_dir")?;
    config.scan.history_dir =
        normalize_required_directory(&config.scan.history_dir, "scan.history_dir")?;
    config.credentials.credential_dir = normalize_required_directory(
        &config.credentials.credential_dir,
        "credentials.credential_dir",
    )?;
    config.library.ignored_subs = normalize_ignored_subs(&config.library.ignored_subs);
    config.aisplined.endpoint = config.aisplined.endpoint.trim().to_string();

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

    let mut normalized_policies = BTreeMap::new();
    for (raw_source, policy) in std::mem::take(&mut config.source_policies) {
        let source = raw_source.trim().to_ascii_lowercase();
        if !SUPPORTED_SOURCE_POLICIES.contains(&source.as_str()) {
            return Err(format!(
                "Unsupported SPLINED source policy provider: {raw_source}"
            ));
        }
        policy
            .validate()
            .map_err(|error| format!("Invalid source policy for {source}: {error}"))?;
        normalized_policies.insert(source, policy);
    }
    config.source_policies = normalized_policies;

    if let Some(policy) = config.source_policies.get("musicbrainz") {
        config.musicbrainz.enabled = policy.enabled;
        config.musicbrainz.source_override = policy.source_override;
    }

    // Config v5 stores only the credential directory. Provider filenames are
    // fixed application contracts and are never written to config.toml.
    config.fanarttv.credential_file = PathBuf::from(&config.credentials.credential_dir)
        .join("fanarttv.json")
        .to_string_lossy()
        .into_owned();
    config.lastfm.credential_file = PathBuf::from(&config.credentials.credential_dir)
        .join("lastfm.json")
        .to_string_lossy()
        .into_owned();
    config.musicbrainz.token_file = PathBuf::from(&config.credentials.credential_dir)
        .join("musicbrainz.json")
        .to_string_lossy()
        .into_owned();

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
    source_policies: &BTreeMap<String, SourcePolicyConfig>,
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
        .filter(|source| {
            source_policies
                .get(source.as_str())
                .is_none_or(|policy| policy.enabled)
        })
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
    config.scan.log_dir = resolve_runtime_directory(root, &config.scan.log_dir);
    config.scan.history_dir = resolve_runtime_directory(root, &config.scan.history_dir);
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
    fn default_config_is_neutral_portable_and_v5() {
        let config = Config::default();

        assert_eq!(config.config_version, 5);
        assert_eq!(config.mode, Mode::Read);
        assert_eq!(config.verbosity, Verbosity::Info);
        assert!(config.scan.scan_mode);
        assert!(!config.scan.library_scan);
        assert_eq!(config.scan.cache_dir, "_cache");
        assert!(config.scan.scan_library_dir.is_empty());
        assert!(config.library.music_library.is_empty());
        assert!(config.library.ignored_subs.is_empty());
        assert!(config.samples.sample_write);
        assert_eq!(config.credentials.credential_dir, "credentials");
        assert!(!config.aisplined.enabled);
        assert!(config.aisplined.endpoint.is_empty());
        assert_eq!(
            config.output.file_formats,
            vec!["jpeg".to_string(), "png".to_string(), "webp".to_string()]
        );
        assert!(config.output.preserve_file);
        assert!(config.sources.exclude_cover_sources.is_empty());
        assert!(config.source_policies.is_empty());
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
        assert!(!text.contains("[fanarttv]"));
        assert!(!text.contains("[lastfm]"));
        assert!(!text.contains("[musicbrainz]"));
        assert!(!text.contains("credential_file"));
        assert!(!text.contains("token_file"));
        assert!(text.contains("[aisplined]"));
        assert_eq!(parsed.scan.cache_dir, "_cache");
        assert_eq!(parsed.credentials.credential_dir, "credentials");
        assert!(!parsed.aisplined.enabled);
        assert!(parsed.aisplined.endpoint.is_empty());
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
    fn provider_credential_files_are_fixed_under_configured_directory() {
        let mut config = Config::default();
        config.credentials.credential_dir = "portable-credentials".to_string();
        let mut text = toml::to_string_pretty(&config).unwrap();
        text.push_str(
            "\n[fanarttv]\ncredential_file = \"outside-fanart.json\"\n\
             [lastfm]\ncredential_file = \"outside-lastfm.json\"\n\
             [musicbrainz]\ntoken_file = \"outside-musicbrainz.json\"\n",
        );
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
    fn source_policy_round_trips_without_changing_legacy_defaults() {
        let mut config = Config::default();
        config.source_policies.insert(
            "Discogs".to_string(),
            SourcePolicyConfig {
                enabled: true,
                source_override: true,
                minimum_range_type: crate::source_policy::MinimumRangeType::LowerRange,
                allow_below_minimum_fallback: true,
                minimum_short_side: Some(1500),
                maximum_short_side: Some(4200),
                minimum_width: Some(1400),
                minimum_height: Some(1300),
                primary_image_only: false,
            },
        );

        let text = toml::to_string_pretty(&config).expect("source policy should serialize");
        let parsed = parse_config(&text).expect("source policy should parse");
        let policy = parsed
            .source_policies
            .get("discogs")
            .expect("provider key should be normalized");

        assert!(policy.source_override);
        assert_eq!(policy.minimum_short_side, Some(1500));
        assert_eq!(policy.maximum_short_side, Some(4200));
        assert_eq!(policy.minimum_width, Some(1400));
        assert_eq!(policy.minimum_height, Some(1300));
        assert!(policy.allow_below_minimum_fallback);
        assert!(!policy.primary_image_only);
    }

    #[test]
    fn musicbrainz_source_policy_controls_metadata_runtime_without_becoming_cover_source() {
        let mut config = Config::default();
        config.source_policies.insert(
            "musicbrainz".to_string(),
            SourcePolicyConfig {
                enabled: false,
                source_override: true,
                ..SourcePolicyConfig::default()
            },
        );
        let text = toml::to_string_pretty(&config).unwrap();
        let parsed = parse_config(&text).unwrap();
        assert!(!parsed.musicbrainz.enabled);
        assert!(parsed.musicbrainz.source_override);
        assert!(
            !parsed
                .sources
                .cover_sources
                .iter()
                .any(|source| source == "musicbrainz")
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

        assert_eq!(config.scan.cache_dir, root.join("_cache").to_string_lossy());
        assert_eq!(
            config.credentials.credential_dir,
            root.join("credentials").to_string_lossy()
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

/// Load a GUI-selected configuration without changing the process-wide
/// portable root. Relative runtime paths retain the same application-root
/// semantics used by the normal CLI.
pub fn load_config_from(path: &Path) -> Result<Config, String> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        format!(
            "Unable to read SPLINED configuration {}: {error}",
            path.display()
        )
    })?;
    let mut config = parse_config(&text)?;
    let root = app_root()?;
    resolve_runtime_paths(&mut config, &root);
    Ok(config)
}
