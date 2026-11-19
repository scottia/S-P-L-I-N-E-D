//! Shared COMP report matching, batching, and coordination policy.
//!
//! The repository JSON is the standalone default. Coordinated Windows uses a
//! validated local mirror immediately, checks PostgreSQL after the UI is ready,
//! and swaps revisions only while processing is idle. A `PolicySnapshot` is
//! captured by each `CompilationMatchCache`, pinning one revision for a batch.

use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, RwLock};
use unicode_normalization::UnicodeNormalization;

const DEFAULT_POLICY: &str = include_str!("../../schema/comp-report-policy.json");
pub const POLICY_ENV: &str = "SPLINED_COMP_REPORT_POLICY";
pub const POLICY_FILENAME: &str = "comp-report-policy.json";
const CURRENT_POLICY_ID: &str = "splined-comp-report-v1";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CompReportPolicy {
    pub schema_version: u32,
    pub policy_id: String,
    pub artist_identity: ArtistIdentityPolicy,
    pub recording_search: RecordingSearchPolicy,
    pub result_grouping: ResultGroupingPolicy,
    pub existing_artwork: ExistingArtworkPolicy,
    pub batch: BatchPolicy,
    pub coordination: CoordinationPolicy,
    pub synchronization: SynchronizationPolicy,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ArtistIdentityPolicy {
    pub selected_artist_precedence: bool,
    pub credit_separators: Vec<String>,
    pub normalization: ArtistNormalizationPolicy,
    pub local_artist_alias: MatchThresholdPolicy,
    pub musicbrainz_artist_search: MusicBrainzArtistSearchPolicy,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ArtistNormalizationPolicy {
    pub case_fold: bool,
    pub unicode_nfkd: bool,
    pub collapse_whitespace: bool,
    pub strip_leading_symbols: Vec<String>,
    pub remove_leading_articles: Vec<String>,
    pub word_equivalents: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MatchThresholdPolicy {
    pub minimum_score: u16,
    pub exact_score: u16,
    pub limit: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MusicBrainzArtistSearchPolicy {
    pub enabled: bool,
    pub minimum_score: u16,
    pub limit: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RecordingSearchPolicy {
    pub minimum_score: u16,
    pub exact_recording_score: u16,
    pub related_work_score: u16,
    pub maximum_non_exact_score: u16,
    pub limit: u8,
    pub detail_lookup_limit: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResultGroupingPolicy {
    pub percentage_bands: Vec<u16>,
    pub release_family_order: Vec<String>,
    pub official_releases_only: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExistingArtworkPolicy {
    pub show_before_remote_search: bool,
    pub reuse_if_identity_pending: bool,
    pub completion_requires_authoritative_ids: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BatchPolicy {
    pub completion_states: Vec<String>,
    pub pin_policy_revision: bool,
    pub refresh_report_after_each_track: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CoordinationPolicy {
    pub publish_verified_artwork_write: bool,
    pub publish_unresolved_identity_write: bool,
    pub completion_requires_authoritative_ids: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SynchronizationPolicy {
    pub startup_source: String,
    pub async_revision_check: bool,
    pub activate_only_when_idle: bool,
}

#[derive(Debug, Clone)]
pub struct PolicySnapshot {
    pub policy: CompReportPolicy,
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresPolicySnapshot {
    pub policy_json: String,
    pub revision: i64,
}

static POLICY: LazyLock<RwLock<PolicySnapshot>> = LazyLock::new(|| {
    RwLock::new(PolicySnapshot {
        policy: load_policy(),
        revision: 0,
    })
});

pub fn policy() -> CompReportPolicy {
    POLICY
        .read()
        .expect("COMP report policy lock")
        .policy
        .clone()
}

pub fn policy_snapshot() -> PolicySnapshot {
    POLICY.read().expect("COMP report policy lock").clone()
}

pub fn active_revision() -> i64 {
    POLICY.read().expect("COMP report policy lock").revision
}

pub fn policy_json() -> String {
    serde_json::to_string_pretty(&policy()).expect("COMP report policy serializes") + "\n"
}

pub fn validate_policy(text: &str) -> Result<(), String> {
    parse_policy(text).map(|_| ())
}

pub fn activate_policy(text: &str, revision: i64) -> Result<(), String> {
    let parsed = parse_policy(text)?;
    *POLICY
        .write()
        .map_err(|_| "COMP report policy lock is unavailable".to_string())? = PolicySnapshot {
        policy: parsed,
        revision: revision.max(0),
    };
    Ok(())
}

pub fn save_policy(path: &Path, text: &str, revision: i64) -> Result<(), String> {
    let parsed = parse_policy(text)?;
    let canonical = serde_json::to_vec_pretty(&parsed)
        .map_err(|error| format!("unable to serialize COMP report policy: {error}"))?;
    crate::safe_write::replace_binary_file(path, &canonical, "COMP report policy", |staged| {
        let value = std::fs::read_to_string(staged)
            .map_err(|error| format!("unable to verify COMP report policy: {error}"))?;
        parse_policy(&value).map(|_| ())
    })?;
    *POLICY
        .write()
        .map_err(|_| "COMP report policy lock is unavailable".to_string())? = PolicySnapshot {
        policy: parsed,
        revision: revision.max(0),
    };
    Ok(())
}

pub async fn fetch_postgres_policy(pool: &PgPool) -> Result<PostgresPolicySnapshot, String> {
    let row = sqlx::query(
        "SELECT policy::text AS policy_json,revision FROM comp_report_policy WHERE singleton=TRUE",
    )
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("PostgreSQL COMP report policy is unavailable: {error}"))?
    .ok_or_else(|| {
        "PostgreSQL COMP report policy has not been initialized by WebGUI.".to_string()
    })?;
    let raw = row
        .try_get::<String, _>("policy_json")
        .map_err(|error| format!("PostgreSQL COMP report policy is unreadable: {error}"))?;
    let parsed = parse_policy(&raw)?;
    let canonical = serde_json::to_string_pretty(&parsed)
        .map_err(|error| format!("PostgreSQL COMP report policy cannot be serialized: {error}"))?
        + "\n";
    Ok(PostgresPolicySnapshot {
        policy_json: canonical,
        revision: row.try_get::<i64, _>("revision").map_err(|error| {
            format!("PostgreSQL COMP report policy revision is unreadable: {error}")
        })?,
    })
}

pub async fn save_postgres_policy(
    pool: &PgPool,
    text: &str,
) -> Result<PostgresPolicySnapshot, String> {
    let parsed = parse_policy(text)?;
    let canonical_input = serde_json::to_string(&parsed)
        .map_err(|error| format!("Unable to serialize COMP report policy: {error}"))?;
    let row = sqlx::query(
        "INSERT INTO comp_report_policy(singleton,policy,revision,updated_by) VALUES(TRUE,$1::jsonb,1,'windows-settings') ON CONFLICT(singleton) DO UPDATE SET policy=excluded.policy,revision=comp_report_policy.revision+1,updated_at=now(),updated_by=excluded.updated_by RETURNING policy::text AS policy_json,revision",
    )
    .bind(canonical_input)
    .fetch_one(pool)
    .await
    .map_err(|error| format!("Unable to save PostgreSQL COMP report policy: {error}"))?;
    let raw = row
        .try_get::<String, _>("policy_json")
        .map_err(|error| format!("Saved PostgreSQL COMP report policy is unreadable: {error}"))?;
    let parsed = parse_policy(&raw)?;
    Ok(PostgresPolicySnapshot {
        policy_json: serde_json::to_string_pretty(&parsed).map_err(|error| {
            format!("Saved PostgreSQL COMP report policy cannot be serialized: {error}")
        })? + "\n",
        revision: row.try_get::<i64, _>("revision").map_err(|error| {
            format!("Saved PostgreSQL COMP report policy revision is unreadable: {error}")
        })?,
    })
}

pub fn policy_path(config: &crate::config::Config) -> Result<PathBuf, String> {
    let directory = config.credentials.credential_dir.trim();
    if directory.is_empty() {
        return Err(
            "Provider Credential Directory is required for the COMP report policy.".to_string(),
        );
    }
    Ok(PathBuf::from(directory).join(POLICY_FILENAME))
}

pub fn ensure_policy(config: &crate::config::Config) -> Result<PathBuf, String> {
    let path = policy_path(config)?;
    if path.is_file() {
        let text = std::fs::read_to_string(&path).map_err(|error| {
            format!(
                "Unable to read existing COMP report policy {}: {error}",
                path.display()
            )
        })?;
        activate_policy(&text, 0)?;
        return Ok(path);
    }
    save_policy(&path, &policy_json(), 0)?;
    Ok(path)
}

pub fn normalize_artist_name(value: &str, policy: &CompReportPolicy) -> String {
    let rules = &policy.artist_identity.normalization;
    let mut text = if rules.unicode_nfkd {
        value.nfkd().collect::<String>()
    } else {
        value.to_string()
    };
    for (source, replacement) in &rules.word_equivalents {
        text = text.replace(source, &format!(" {replacement} "));
    }
    loop {
        let trimmed = text.trim_start();
        let Some(symbol) = rules
            .strip_leading_symbols
            .iter()
            .find(|symbol| trimmed.starts_with(symbol.as_str()))
        else {
            text = trimmed.to_string();
            break;
        };
        text = trimmed[symbol.len()..].to_string();
    }
    if rules.case_fold {
        text = text.to_lowercase();
    }
    if rules.collapse_whitespace {
        text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    for article in &rules.remove_leading_articles {
        let prefix = format!("{} ", article.to_lowercase());
        if text.to_lowercase().starts_with(&prefix) {
            text = text[prefix.len()..].to_string();
            break;
        }
    }
    text.chars()
        .filter(|character| character.is_alphanumeric())
        .collect()
}

pub fn split_artist_credit(value: &str, policy: &CompReportPolicy) -> Vec<String> {
    let mut parts = vec![value.trim().to_string()];
    let mut separators = policy
        .artist_identity
        .credit_separators
        .iter()
        .map(|value| {
            if value == "\\u0000" {
                "\0".to_string()
            } else {
                value.clone()
            }
        })
        .collect::<Vec<_>>();
    separators.sort_by_key(|value| std::cmp::Reverse(value.len()));
    for separator in separators {
        let mut next = Vec::new();
        for part in parts {
            let lower = part.to_lowercase();
            let needle = separator.to_lowercase();
            let mut start = 0usize;
            while let Some(relative) = lower[start..].find(&needle) {
                let end = start + relative;
                if let Some(value) = part
                    .get(start..end)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    next.push(value.to_string());
                }
                start = end + needle.len();
            }
            if let Some(value) = part
                .get(start..)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                next.push(value.to_string());
            }
        }
        parts = next;
    }
    parts
}

pub fn focused_artist_name(
    track_credit: &str,
    selected_artist: Option<&str>,
    policy: &CompReportPolicy,
) -> String {
    if policy.artist_identity.selected_artist_precedence
        && let Some(selected) = selected_artist
            .map(str::trim)
            .filter(|value| !value.is_empty())
    {
        let selected_key = normalize_artist_name(selected, policy);
        if normalize_artist_name(track_credit, policy) == selected_key
            || split_artist_credit(track_credit, policy)
                .iter()
                .any(|part| normalize_artist_name(part, policy) == selected_key)
        {
            return selected.to_string();
        }
    }
    split_artist_credit(track_credit, policy)
        .into_iter()
        .find(|value| !value.trim().is_empty())
        .unwrap_or_else(|| track_credit.trim().to_string())
}

pub fn artist_names_compatible(local: &str, musicbrainz: &str, policy: &CompReportPolicy) -> bool {
    let right = normalize_artist_name(musicbrainz, policy);
    !right.is_empty()
        && (normalize_artist_name(local, policy) == right
            || split_artist_credit(local, policy)
                .iter()
                .any(|part| normalize_artist_name(part, policy) == right))
}

pub fn artist_credit_position(
    track_credit: &str,
    selected_artist: &str,
    policy: &CompReportPolicy,
) -> Option<usize> {
    let selected = normalize_artist_name(selected_artist, policy);
    if selected.is_empty() {
        return None;
    }
    if normalize_artist_name(track_credit, policy) == selected {
        return Some(0);
    }
    split_artist_credit(track_credit, policy)
        .iter()
        .position(|credit| normalize_artist_name(credit, policy) == selected)
}

pub fn artist_similarity_score(left: &str, right: &str, policy: &CompReportPolicy) -> u16 {
    let left = normalize_artist_name(left, policy)
        .chars()
        .collect::<Vec<_>>();
    let right = normalize_artist_name(right, policy)
        .chars()
        .collect::<Vec<_>>();
    if left.is_empty() || right.is_empty() {
        return 0;
    }
    if left == right {
        return policy.artist_identity.local_artist_alias.exact_score;
    }
    let mut prior = (0..=right.len()).collect::<Vec<_>>();
    for (row, left_char) in left.iter().enumerate() {
        let mut current = vec![row + 1];
        for (column, right_char) in right.iter().enumerate() {
            current.push(std::cmp::min(
                std::cmp::min(current[column] + 1, prior[column + 1] + 1),
                prior[column] + usize::from(left_char != right_char),
            ));
        }
        prior = current;
    }
    let maximum = left.len().max(right.len());
    (((maximum - prior[right.len()]) * 100) / maximum) as u16
}

fn load_policy() -> CompReportPolicy {
    for path in policy_candidates() {
        match std::fs::read_to_string(&path) {
            Ok(text) => match parse_policy(&text) {
                Ok(value) => return value,
                Err(error) => panic!(
                    "invalid external comp-report-policy.json {}: {error}",
                    path.display()
                ),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!(
                "unable to read external comp-report-policy.json {}: {error}",
                path.display()
            ),
        }
    }
    parse_policy(DEFAULT_POLICY).expect("repository COMP report policy seed must be valid")
}

fn policy_candidates() -> Vec<PathBuf> {
    let mut result = Vec::new();
    if let Some(path) = std::env::var_os(POLICY_ENV).filter(|value| !value.is_empty()) {
        result.push(PathBuf::from(path));
    }
    if let Ok(config) = crate::config::load_config()
        && let Ok(path) = policy_path(&config)
    {
        result.push(path);
    }
    result
}

fn parse_policy(text: &str) -> Result<CompReportPolicy, String> {
    let value = serde_json::from_str::<CompReportPolicy>(text)
        .map_err(|error| format!("invalid COMP report policy JSON: {error}"))?;
    if value.schema_version != 1 || value.policy_id != CURRENT_POLICY_ID {
        return Err("unsupported COMP report policy schema or policy_id".to_string());
    }
    let artist = &value.artist_identity;
    if !artist.selected_artist_precedence
        || artist.credit_separators.is_empty()
        || artist.credit_separators.iter().any(|item| item.is_empty())
        || !artist.normalization.case_fold
        || !artist.normalization.unicode_nfkd
        || !artist.normalization.collapse_whitespace
        || !artist.musicbrainz_artist_search.enabled
    {
        return Err("COMP report Artist identity recovery rules are incomplete".to_string());
    }
    for (name, score) in [
        ("local minimum", artist.local_artist_alias.minimum_score),
        ("local exact", artist.local_artist_alias.exact_score),
        (
            "MusicBrainz Artist minimum",
            artist.musicbrainz_artist_search.minimum_score,
        ),
        ("recording minimum", value.recording_search.minimum_score),
        (
            "recording exact",
            value.recording_search.exact_recording_score,
        ),
        ("related Work", value.recording_search.related_work_score),
        (
            "non-exact maximum",
            value.recording_search.maximum_non_exact_score,
        ),
    ] {
        if !(1..=100).contains(&score) {
            return Err(format!(
                "COMP report {name} score must be from 1 through 100"
            ));
        }
    }
    if artist.local_artist_alias.limit == 0
        || !(1..=25).contains(&artist.musicbrainz_artist_search.limit)
        || value.recording_search.limit == 0
        || value.recording_search.detail_lookup_limit > 25
    {
        return Err("COMP report search limits are invalid".to_string());
    }
    let bands = &value.result_grouping.percentage_bands;
    if bands.is_empty()
        || bands.iter().any(|score| !(1..=100).contains(score))
        || bands.windows(2).any(|pair| pair[0] <= pair[1])
    {
        return Err("COMP report percentage bands must be unique and descending".to_string());
    }
    let mut families = value.result_grouping.release_family_order.clone();
    families.sort();
    let required_families = [
        "album",
        "broadcast",
        "compilation",
        "ep",
        "other",
        "single",
        "soundtrack",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<Vec<_>>();
    let required_states = ["completed", "unresolved", "failed"]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if families != required_families || !value.result_grouping.official_releases_only {
        return Err("COMP report release-family grouping is incomplete".to_string());
    }
    if !value.existing_artwork.show_before_remote_search
        || !value.existing_artwork.reuse_if_identity_pending
        || !value.existing_artwork.completion_requires_authoritative_ids
        || value.batch.completion_states != required_states
        || !value.batch.pin_policy_revision
        || !value.batch.refresh_report_after_each_track
        || !value.coordination.publish_verified_artwork_write
        || !value.coordination.publish_unresolved_identity_write
        || !value.coordination.completion_requires_authoritative_ids
        || value.synchronization.startup_source != "local_mirror"
        || !value.synchronization.async_revision_check
        || !value.synchronization.activate_only_when_idle
    {
        return Err(
            "COMP report batch, coordination, or synchronization rules are incomplete".to_string(),
        );
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_policy_is_valid_and_focuses_selected_slash_credit() {
        let policy = parse_policy(DEFAULT_POLICY).unwrap();
        assert_eq!(
            focused_artist_name("*NSYNC/Gloria Estefan", Some("*NSYNC"), &policy),
            "*NSYNC"
        );
        assert!(artist_names_compatible(
            "*NSYNC/Gloria Estefan",
            "NSYNC",
            &policy
        ));
    }
}
