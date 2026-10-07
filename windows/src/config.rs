use crate::musicbrainz::MusicBrainzConfig;
use crate::portable::{app_layout, app_root, bootstrap_portable_install};
use crate::source_policy::SourcePolicyConfig;
use serde::de::{self, Visitor};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

pub const CURRENT_CONFIG_VERSION: u32 = 5;
pub const SUPPORTED_COVER_SOURCES: [&str; 8] = [
    "deezer",
    "itunes",
    "fanarttv",
    "lastfm",
    "musicbrainz",
    "coverartarchive",
    "discogs",
    "amazon",
];
pub const SUPPORTED_SOURCE_POLICIES: [&str; 8] = [
    "deezer",
    "itunes",
    "fanarttv",
    "lastfm",
    "musicbrainz",
    "coverartarchive",
    "discogs",
    "amazon",
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
    /// Disposable downloaded and derived artwork used only during an active run.
    /// Older Config v5 files omit this field and continue to use `cache_dir`.
    #[serde(default)]
    pub temporary_cache_dir: String,
    /// Use rollback journaling for an intentionally shared/network SQLite file.
    #[serde(default)]
    pub sqlite_shared: bool,
    #[serde(default = "default_log_dir")]
    pub log_dir: String,
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
    pub upscale_max_percent: u32,
    pub upscale_adaptive_defaults: bool,
    pub upscale_picture_percent: i32,
    pub upscale_sharpen_percent: i32,
    pub upscale_softness_percent: i32,
    pub upscale_contrast_percent: i32,
    pub upscale_exposure_percent: i32,
    pub upscale_brightness_percent: i32,
    pub upscale_gamma_percent: i32,
    pub upscale_color_temperature: i32,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSplinedConfig {
    pub enabled: bool,
    pub endpoint: String,
    pub minimum_short_side: u32,
    pub allow_below_minimum_override: bool,
}

impl Default for AiSplinedConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: String::new(),
            minimum_short_side: 600,
            allow_below_minimum_override: false,
        }
    }
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
            temporary_cache_dir: DEFAULT_CACHE_DIR.to_string(),
            sqlite_shared: false,
            log_dir: default_log_dir(),
            scan_library_dir: DEFAULT_SCAN_LIBRARY_DIR.to_string(),
        }
    }
}

fn default_log_dir() -> String {
    "_logs".to_string()
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
            upscale_max_percent: 200,
            upscale_adaptive_defaults: true,
            upscale_picture_percent: 0,
            upscale_sharpen_percent: 0,
            upscale_softness_percent: 0,
            upscale_contrast_percent: 0,
            upscale_exposure_percent: 0,
            upscale_brightness_percent: 0,
            upscale_gamma_percent: 0,
            upscale_color_temperature: 0,
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

        let mut source_policies = BTreeMap::new();
        source_policies.insert(
            "amazon".to_string(),
            SourcePolicyConfig {
                enabled: false,
                strict_override: true,
                ..SourcePolicyConfig::default()
            },
        );

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
            source_policies,
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
    let amazon_strict_explicit = toml::from_str::<toml::Value>(text)
        .ok()
        .and_then(|root| root.get("source_policies").cloned())
        .and_then(|policies| policies.as_table().cloned())
        .and_then(|policies| {
            policies
                .iter()
                .find(|(source, _)| source.eq_ignore_ascii_case("amazon"))
                .map(|(_, policy)| policy.clone())
        })
        .and_then(|policy| policy.as_table().cloned())
        .is_some_and(|policy| policy.contains_key("strict_override"));
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

    if !(100..=800).contains(&config.output.upscale_max_percent) {
        return Err("SPLINED output upscale_max_percent must be between 100 and 800.".to_string());
    }
    for (name, value) in [
        (
            "upscale_sharpen_percent",
            config.output.upscale_sharpen_percent,
        ),
        (
            "upscale_softness_percent",
            config.output.upscale_softness_percent,
        ),
    ] {
        if !(0..=20).contains(&value) {
            return Err(format!("SPLINED output {name} must be between 0 and 20."));
        }
    }
    for (name, value) in [
        (
            "upscale_picture_percent",
            config.output.upscale_picture_percent,
        ),
        (
            "upscale_contrast_percent",
            config.output.upscale_contrast_percent,
        ),
        (
            "upscale_exposure_percent",
            config.output.upscale_exposure_percent,
        ),
        (
            "upscale_brightness_percent",
            config.output.upscale_brightness_percent,
        ),
        ("upscale_gamma_percent", config.output.upscale_gamma_percent),
    ] {
        if !(-20..=20).contains(&value) {
            return Err(format!("SPLINED output {name} must be between -20 and 20."));
        }
    }
    if !(-100..=100).contains(&config.output.upscale_color_temperature) {
        return Err(
            "SPLINED output upscale_color_temperature must be between -100 and 100.".to_string(),
        );
    }

    if config.scan.scan_mode_timeout.0 < 0.0 || !config.scan.scan_mode_timeout.0.is_finite() {
        return Err("SPLINED scan_mode_timeout must be finite and non-negative.".to_string());
    }

    config.library.music_library = normalize_optional_directory(&config.library.music_library);
    config.scan.scan_library_dir = normalize_optional_directory(&config.scan.scan_library_dir);
    config.scan.cache_dir = normalize_required_directory(&config.scan.cache_dir, "scan.cache_dir")?;
    config.scan.temporary_cache_dir = if config.scan.temporary_cache_dir.trim().is_empty() {
        config.scan.cache_dir.clone()
    } else {
        normalize_required_directory(&config.scan.temporary_cache_dir, "scan.temporary_cache_dir")?
    };
    config.scan.log_dir = normalize_required_directory(&config.scan.log_dir, "scan.log_dir")?;
    config.credentials.credential_dir = normalize_required_directory(
        &config.credentials.credential_dir,
        "credentials.credential_dir",
    )?;
    config.library.ignored_subs = normalize_ignored_subs(&config.library.ignored_subs);
    config.aisplined.endpoint = config.aisplined.endpoint.trim().to_string();
    if config.aisplined.minimum_short_side == 0 {
        return Err("SPLINED aisplined.minimum_short_side must be greater than zero.".to_string());
    }

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
    if !amazon_strict_explicit && let Some(policy) = config.source_policies.get_mut("amazon") {
        policy.strict_override = true;
    }

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
    config.scan.temporary_cache_dir =
        resolve_runtime_directory(root, &config.scan.temporary_cache_dir);
    config.scan.log_dir = resolve_runtime_directory(root, &config.scan.log_dir);
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

/// Load an in-memory GUI configuration without creating a runtime file.
/// Relative paths retain the same application-root semantics as file-backed
/// configuration, and the source text must never be written to diagnostics.
pub fn load_config_text(text: &str) -> Result<Config, String> {
    let root = app_root()?;
    load_config_text_at_root(text, &root)
}

/// Load and resolve Config v5 against an explicit portable root. This keeps
/// restore validation deterministic when the same backup is applied in two
/// different portable directories.
pub fn load_config_text_at_root(text: &str, root: &Path) -> Result<Config, String> {
    if text.trim().is_empty() {
        return Err("SPLINED received empty Windows Config v5 settings.".to_string());
    }
    let mut config = parse_config(text)?;
    resolve_runtime_paths(&mut config, root);
    Ok(config)
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
        assert_eq!(config.scan.temporary_cache_dir, "_cache");
        assert!(config.scan.scan_library_dir.is_empty());
        assert!(config.library.music_library.is_empty());
        assert!(config.library.ignored_subs.is_empty());
        assert!(config.samples.sample_write);
        assert_eq!(config.credentials.credential_dir, "credentials");
        assert!(!config.aisplined.enabled);
        assert!(config.aisplined.endpoint.is_empty());
        assert_eq!(config.aisplined.minimum_short_side, 600);
        assert!(!config.aisplined.allow_below_minimum_override);
        assert_eq!(
            config.output.file_formats,
            vec!["jpeg".to_string(), "png".to_string(), "webp".to_string()]
        );
        assert!(config.output.preserve_file);
        assert!(config.output.upscale_adaptive_defaults);
        assert_eq!(config.output.upscale_sharpen_percent, 0);
        assert_eq!(config.output.upscale_contrast_percent, 0);
        assert_eq!(config.output.upscale_exposure_percent, 0);
        assert_eq!(config.output.upscale_brightness_percent, 0);
        assert_eq!(config.output.upscale_color_temperature, 0);
        assert!(config.sources.exclude_cover_sources.is_empty());
        assert_eq!(config.source_policies.len(), 1);
        assert!(!config.source_policies["amazon"].enabled);
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
        assert_eq!(parsed.scan.temporary_cache_dir, "_cache");
        assert_eq!(parsed.credentials.credential_dir, "credentials");
        assert!(!parsed.aisplined.enabled);
        assert!(parsed.aisplined.endpoint.is_empty());
        assert_eq!(parsed.aisplined.minimum_short_side, 600);
        assert!(!parsed.aisplined.allow_below_minimum_override);
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
                strict_override: false,
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
    fn missing_amazon_strict_key_migrates_to_safe_default() {
        let text = default_toml()
            .expect("default config should serialize")
            .replace("strict_override = true\n", "");
        let parsed = parse_config(&text).expect("legacy Config v5 should parse");
        assert!(parsed.source_policies["amazon"].strict_override);

        let explicit = default_toml()
            .expect("default config should serialize")
            .replace("strict_override = true", "strict_override = false");
        let parsed = parse_config(&explicit).expect("explicit strict setting should parse");
        assert!(!parsed.source_policies["amazon"].strict_override);
    }

    #[test]
    fn musicbrainz_source_policy_controls_metadata_and_artwork_runtime() {
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
            parsed
                .sources
                .cover_sources
                .iter()
                .any(|source| source == "musicbrainz")
        );
    }

    #[test]
    fn samples_directory_is_derived_from_temporary_cache_directory() {
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
            config.scan.temporary_cache_dir,
            root.join("_cache").to_string_lossy()
        );
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

    #[test]
    fn portable_config_moves_with_relative_state_and_preserves_absolute_authority() {
        let text = default_toml().unwrap();
        let first = load_config_text_at_root(&text, Path::new(r"C:\Portable\One")).unwrap();
        let second = load_config_text_at_root(&text, Path::new(r"D:\Portable\Two")).unwrap();
        assert_eq!(first.scan.cache_dir, r"C:\Portable\One\_cache");
        assert_eq!(second.scan.cache_dir, r"D:\Portable\Two\_cache");

        let mut explicit = parse_config(&text).unwrap();
        explicit.library.music_library = r"\\server\music".into();
        explicit.scan.cache_dir = r"E:\State\Database".into();
        explicit.credentials.credential_dir = r"\\server\private\credentials".into();
        let explicit_text = toml::to_string_pretty(&explicit).unwrap();
        let first =
            load_config_text_at_root(&explicit_text, Path::new(r"C:\Portable\One")).unwrap();
        let second =
            load_config_text_at_root(&explicit_text, Path::new(r"D:\Portable\Two")).unwrap();
        assert_eq!(first.library.music_library, r"\\server\music");
        assert_eq!(second.library.music_library, r"\\server\music");
        assert_eq!(first.scan.cache_dir, r"E:\State\Database");
        assert_eq!(second.scan.cache_dir, r"E:\State\Database");
        assert_eq!(
            first.credentials.credential_dir,
            r"\\server\private\credentials"
        );
        assert_eq!(
            first.credentials.credential_dir,
            second.credentials.credential_dir
        );
    }

    #[test]
    fn single_executable_preserves_sqlite_and_all_runtime_path_authority() {
        use crate::media_database::database_path;
        use crate::portable::{AppLayout, portable_root_from_executable};

        let portable_root = PathBuf::from(r"C:\Portable\SPLINED");
        let executable = portable_root.join("splined.exe");
        let resolved_root = portable_root_from_executable(&executable).unwrap();
        assert_eq!(resolved_root, portable_root);

        let before_layout = AppLayout::from_root(resolved_root.clone());
        let after_layout = AppLayout::from_root(resolved_root.clone());
        assert_eq!(before_layout, after_layout);
        assert_eq!(
            after_layout.config_file,
            portable_root.join("data").join("config.toml")
        );
        assert_eq!(after_layout.cache_dir, portable_root.join("_cache"));
        assert_eq!(after_layout.logs_dir, portable_root.join("_logs"));
        assert_eq!(
            after_layout.credentials_dir,
            portable_root.join("credentials")
        );
        let source = parse_config(&default_toml().unwrap()).unwrap();
        let mut before = source.clone();
        let mut after = source;
        resolve_runtime_paths(&mut before, &resolved_root);
        resolve_runtime_paths(&mut after, &resolved_root);
        assert_eq!(before, after);
        let expected_database = portable_root.join("_cache").join("splined.db");
        assert_eq!(database_path(&before.scan.cache_dir), expected_database);
        assert_eq!(database_path(&after.scan.cache_dir), expected_database);

        let mut explicit = parse_config(&default_toml().unwrap()).unwrap();
        explicit.scan.cache_dir = r"\\server\share\SPLINED database".to_string();
        explicit.scan.temporary_cache_dir = r"D:\SPLINED run cache".to_string();
        explicit.scan.log_dir = r"\\server\share\SPLINED logs".to_string();
        explicit.credentials.credential_dir = r"D:\SPLINED credentials".to_string();
        explicit.library.music_library = r"\\server\music".to_string();
        explicit.scan.scan_library_dir = r"\\server\music".to_string();
        let expected = explicit.clone();
        resolve_runtime_paths(&mut explicit, &resolved_root);
        assert_eq!(explicit.scan.cache_dir, expected.scan.cache_dir);
        assert_eq!(
            database_path(&explicit.scan.cache_dir),
            PathBuf::from(r"\\server\share\SPLINED database\splined.db")
        );
        assert_eq!(
            explicit.scan.temporary_cache_dir,
            expected.scan.temporary_cache_dir
        );
        assert_eq!(explicit.scan.log_dir, expected.scan.log_dir);
        assert_eq!(
            explicit.credentials.credential_dir,
            expected.credentials.credential_dir
        );
        assert_eq!(
            explicit.library.music_library,
            expected.library.music_library
        );
        assert_eq!(
            explicit.scan.scan_library_dir,
            expected.scan.scan_library_dir
        );
    }

    #[test]
    fn in_memory_windows_config_loads_without_a_runtime_file() {
        let text = default_toml().unwrap();
        let config = load_config_text(&text).expect("in-memory config should load");
        assert_eq!(config.config_version, Config::default().config_version);
        assert!(load_config_text("  \r\n ").is_err());
    }

    #[test]
    fn older_config_v5_uses_database_directory_for_missing_temporary_cache() {
        let text = default_toml()
            .unwrap()
            .lines()
            .filter(|line| !line.trim_start().starts_with("temporary_cache_dir ="))
            .collect::<Vec<_>>()
            .join("\n")
            .replace("cache_dir = \"_cache\"", "cache_dir = \"custom-database\"");
        let parsed = parse_config(&text).expect("legacy Config v5 should remain compatible");
        assert_eq!(parsed.scan.cache_dir, "custom-database");
        assert_eq!(parsed.scan.temporary_cache_dir, "custom-database");
    }
}
