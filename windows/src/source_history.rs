use crate::candidate::Candidate;
use crate::config::Config;
use crate::final_artwork::project_configured_artwork;
use crate::media_database::{read_runtime_cache_payload, write_runtime_cache_payload};
use crate::range::{Range, RangeClass};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SOURCE_HISTORY_VERSION: u32 = 1;
pub const SOURCE_HISTORY_CACHE_KEY: &str = "source-selection-stats";
const SUPPORTED_SOURCES: [&str; 8] = [
    "deezer",
    "itunes",
    "fanarttv",
    "lastfm",
    "musicbrainz",
    "coverartarchive",
    "discogs",
    "amazon",
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
        Self {
            version: SOURCE_HISTORY_VERSION,
            sources: SUPPORTED_SOURCES
                .iter()
                .map(|source| ((*source).to_string(), SourceHistoryEntry::default()))
                .collect(),
        }
    }
}

pub fn load_source_history(config: &Config) -> SourceHistory {
    let mut defaults = SourceHistory::default();
    if !config.history.enabled {
        return defaults;
    }
    let Ok(Some(text)) = read_runtime_cache_payload(config, SOURCE_HISTORY_CACHE_KEY) else {
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
    let body = serde_json::to_string(history)
        .map_err(|error| format!("Unable to serialize SPLINED chosen-source history: {error}"))?;
    write_runtime_cache_payload(config, SOURCE_HISTORY_CACHE_KEY, "source_history", &body)
}
