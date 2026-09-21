use crate::candidate::Candidate;
use crate::config::Config;
use crate::final_artwork::project_configured_artwork;
use crate::range::{Range, RangeClass};
use crate::safe_write::replace_text_file;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const SOURCE_HISTORY_VERSION: u32 = 1;
pub const SOURCE_HISTORY_FILE: &str = "chosen-source-history.json";
const SUPPORTED_SOURCES: [&str; 6] = [
    "deezer",
    "itunes",
    "fanarttv",
    "lastfm",
    "coverartarchive",
    "discogs",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SourceHistoryEntry {
    #[serde(default)]
    pub selected: u64,
    #[serde(default)]
    pub ideal: u64,
    #[serde(default)]
    pub acceptable: u64,
    #[serde(default)]
    pub last_selected_unix: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceHistory {
    pub version: u32,
    pub sources: BTreeMap<String, SourceHistoryEntry>,
}

impl Default for SourceHistory {
    fn default() -> Self {
        let sources = SUPPORTED_SOURCES
            .iter()
            .map(|source| ((*source).to_string(), SourceHistoryEntry::default()))
            .collect();
        Self {
            version: SOURCE_HISTORY_VERSION,
            sources,
        }
    }
}

pub fn source_history_path(config: &Config) -> PathBuf {
    Path::new(&config.scan.history_dir).join(SOURCE_HISTORY_FILE)
}

/// Chosen-source history is advisory. A missing, old, or damaged file must
/// never prevent discovery or selection.
pub fn load_source_history(config: &Config) -> SourceHistory {
    let mut defaults = SourceHistory::default();
    if !config.history.enabled {
        return defaults;
    }
    let Ok(text) = fs::read_to_string(source_history_path(config)) else {
        return defaults;
    };
    let Ok(parsed) = serde_json::from_str::<SourceHistory>(&text) else {
        return defaults;
    };
    for source in SUPPORTED_SOURCES {
        if let Some(entry) = parsed.sources.get(source) {
            defaults.sources.insert(source.to_string(), entry.clone());
        }
    }
    defaults
}

pub fn record_source_selection(
    config: &Config,
    history: &mut SourceHistory,
    candidate: &Candidate,
    range: &Range,
    selected_at_unix: f64,
    fallback_selection: bool,
) -> Result<(), String> {
    if fallback_selection
        || !config.history.enabled
        || !SUPPORTED_SOURCES.contains(&candidate.source.as_str())
    {
        return Ok(());
    }
    let projected = project_configured_artwork(candidate, range, &config.output);
    let range_class = range.classify(projected.width.min(projected.height));
    let entry = history.sources.entry(candidate.source.clone()).or_default();
    entry.selected = entry.selected.saturating_add(1);
    if range_class == RangeClass::Ideal {
        entry.ideal = entry.ideal.saturating_add(1);
    }
    if projected.acceptable {
        entry.acceptable = entry.acceptable.saturating_add(1);
    }
    entry.last_selected_unix = Some(selected_at_unix);
    history.version = SOURCE_HISTORY_VERSION;
    save_source_history(config, history)
}

fn save_source_history(config: &Config, history: &SourceHistory) -> Result<(), String> {
    let mut persisted = history.clone();
    persisted.version = SOURCE_HISTORY_VERSION;
    let body = serde_json::to_string_pretty(&persisted)
        .map_err(|error| format!("Unable to serialize SPLINED chosen-source history: {error}"))?
        + "\n";
    replace_text_file(
        &source_history_path(config),
        &body,
        "chosen source history",
        |staged| {
            serde_json::from_str::<SourceHistory>(staged)
                .map(|_| ())
                .map_err(|error| format!("Invalid staged chosen-source history: {error}"))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::candidate::StaticFormat;
    use tempfile::tempdir;

    fn test_config() -> (tempfile::TempDir, Config) {
        let temp = tempdir().unwrap();
        let mut config = Config::default();
        config.scan.history_dir = temp.path().to_string_lossy().into_owned();
        (temp, config)
    }

    #[test]
    fn corrupt_history_is_advisory_and_returns_defaults() {
        let (_temp, config) = test_config();
        fs::write(source_history_path(&config), "not json").unwrap();
        let history = load_source_history(&config);
        assert_eq!(history.version, SOURCE_HISTORY_VERSION);
        assert_eq!(history.sources["discogs"].selected, 0);
    }

    #[test]
    fn normal_provider_selection_is_recorded_atomically() {
        let (_temp, config) = test_config();
        let range = Range::default();
        let candidate = Candidate {
            source: "itunes".to_string(),
            width: 1800,
            height: 1800,
            format: StaticFormat::Jpeg,
            source_priority: 1,
        };
        let mut history = load_source_history(&config);
        record_source_selection(&config, &mut history, &candidate, &range, 123.5, false).unwrap();
        let loaded = load_source_history(&config);
        assert_eq!(loaded.sources["itunes"].selected, 1);
        assert_eq!(loaded.sources["itunes"].ideal, 1);
        assert_eq!(loaded.sources["itunes"].acceptable, 1);
        assert_eq!(loaded.sources["itunes"].last_selected_unix, Some(123.5));
    }

    #[test]
    fn local_and_fallback_only_sources_are_not_counted() {
        let (_temp, config) = test_config();
        let candidate = Candidate {
            source: "local".to_string(),
            width: 1800,
            height: 1800,
            format: StaticFormat::Jpeg,
            source_priority: 0,
        };
        let mut history = SourceHistory::default();
        record_source_selection(
            &config,
            &mut history,
            &candidate,
            &Range::default(),
            123.5,
            false,
        )
        .unwrap();
        assert!(history.sources.values().all(|entry| entry.selected == 0));
        assert!(!source_history_path(&config).exists());
    }

    #[test]
    fn disabled_history_never_writes() {
        let (_temp, mut config) = test_config();
        config.history.enabled = false;
        let candidate = Candidate {
            source: "discogs".to_string(),
            width: 1800,
            height: 1800,
            format: StaticFormat::Jpeg,
            source_priority: 0,
        };
        let mut history = SourceHistory::default();
        record_source_selection(
            &config,
            &mut history,
            &candidate,
            &Range::default(),
            1.0,
            false,
        )
        .unwrap();
        assert!(!source_history_path(&config).exists());
    }

    #[test]
    fn fallback_provider_selection_never_increments_history() {
        let (_temp, config) = test_config();
        let candidate = Candidate {
            source: "discogs".to_string(),
            width: 1800,
            height: 1800,
            format: StaticFormat::Jpeg,
            source_priority: 0,
        };
        let mut history = SourceHistory::default();
        record_source_selection(
            &config,
            &mut history,
            &candidate,
            &Range::default(),
            1.0,
            true,
        )
        .unwrap();
        assert_eq!(history.sources["discogs"].selected, 0);
        assert!(!source_history_path(&config).exists());
    }
}
