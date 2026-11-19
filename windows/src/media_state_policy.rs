//! Shared Artist and Album file-list state classification.
//!
//! A standalone SQLite installation and a coordinated PostgreSQL installation
//! both use the same JSON policy. `SPLINED_MEDIA_STATE_POLICY` may point both
//! runtimes at one external file. Windows carries the repository policy only
//! as an unsaved first-run seed; Setup writes the editable runtime authority
//! in the selected Provider Credential Directory beside
//! `album-coordination.json`.

use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{LazyLock, RwLock};

const DEFAULT_POLICY: &str = include_str!("../../schema/media-state-classification.json");
pub const POLICY_ENV: &str = "SPLINED_MEDIA_STATE_POLICY";
pub const POLICY_FILENAME: &str = "media-state-classification.json";
const CURRENT_POLICY_ID: &str = "splined-media-state-v2";
const LEGACY_POLICY_ID: &str = "splined-media-state-v1";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MediaStatePolicy {
    pub schema_version: u32,
    pub policy_id: String,
    pub colors: BTreeMap<String, String>,
    pub light_colors: BTreeMap<String, String>,
    pub ui_status_colors: BTreeMap<String, String>,
    pub database_monitor: DatabaseMonitorPolicy,
    pub artist: RankedPolicy,
    pub album: AlbumPolicy,
    pub override_presentation: OverridePresentation,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DatabaseMonitorPolicy {
    pub poll_seconds: u64,
    pub backends: BTreeMap<String, String>,
    pub states: BTreeMap<String, DatabaseMonitorState>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DatabaseMonitorState {
    pub label: String,
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseMonitorPresentation {
    pub backend: String,
    pub health: String,
    pub text: String,
    pub description: String,
    pub color_name: String,
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresPolicySnapshot {
    pub policy_json: String,
    pub revision: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RankedPolicy {
    pub ranked_states: Vec<StateRule>,
    #[serde(default)]
    pub forbidden_name_colors: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AlbumPolicy {
    pub ranked_name_states: Vec<StateRule>,
    pub status_dots: Vec<StatusDotRule>,
    #[serde(default)]
    pub forbidden_name_colors: Vec<String>,
    pub red_definition: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StatusDotRule {
    pub condition: String,
    pub label: String,
    pub color: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OverridePresentation {
    pub condition: String,
    pub banner_kind: String,
    pub grayscale_thumbnail: bool,
    pub frame_color: String,
    pub status_dot_color: String,
    pub preserve_album_classification: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StateRule {
    pub condition: String,
    pub key: String,
    pub ui_status_key: String,
    pub label: String,
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaStateClassification {
    pub key: String,
    pub ui_status_key: String,
    pub label: String,
    pub color_name: String,
    pub color: String,
}

static POLICY: LazyLock<RwLock<MediaStatePolicy>> = LazyLock::new(|| RwLock::new(load_policy()));
static ACTIVE_POSTGRES_REVISION: AtomicI64 = AtomicI64::new(0);

pub fn policy() -> MediaStatePolicy {
    POLICY.read().expect("media-state policy lock").clone()
}

pub fn classify_artist(conditions: &[&str]) -> MediaStateClassification {
    let policy = policy();
    classify(
        &policy,
        &policy.artist.ranked_states,
        &policy.artist.forbidden_name_colors,
        conditions,
    )
}

pub fn classify_album(conditions: &[&str]) -> MediaStateClassification {
    let policy = policy();
    classify(
        &policy,
        &policy.album.ranked_name_states,
        &policy.album.forbidden_name_colors,
        conditions,
    )
}

pub fn status_color(status_key: &str) -> String {
    let policy = policy();
    let color_name = policy
        .ui_status_colors
        .get(status_key)
        .map(String::as_str)
        .unwrap_or("red");
    policy
        .colors
        .get(color_name)
        .cloned()
        .unwrap_or_else(|| policy.colors["red"].clone())
}

pub fn named_color(color_name: &str) -> String {
    let policy = policy();
    policy
        .colors
        .get(color_name)
        .cloned()
        .unwrap_or_else(|| policy.colors["red"].clone())
}

pub fn database_monitor_presentation(backend: &str, health: &str) -> DatabaseMonitorPresentation {
    let policy = policy();
    let backend = if policy.database_monitor.backends.contains_key(backend) {
        backend
    } else {
        "current"
    };
    let health = if policy.database_monitor.states.contains_key(health) {
        health
    } else {
        "available"
    };
    let state = &policy.database_monitor.states[health];
    let color = policy
        .colors
        .get(&state.color)
        .cloned()
        .expect("database monitor color must be defined");
    DatabaseMonitorPresentation {
        backend: backend.to_string(),
        health: health.to_string(),
        text: policy.database_monitor.backends[backend].clone(),
        description: state.label.clone(),
        color_name: state.color.clone(),
        color,
    }
}

pub fn database_monitor_poll_seconds() -> u64 {
    policy().database_monitor.poll_seconds
}

pub fn album_status_dot(condition: &str) -> Option<StatusDotRule> {
    policy()
        .album
        .status_dots
        .into_iter()
        .find(|rule| rule.condition == condition)
}

pub fn policy_json() -> String {
    serde_json::to_string_pretty(&policy()).expect("media-state policy serializes") + "\n"
}

pub fn validate_policy(text: &str) -> Result<(), String> {
    parse_policy(text).map(|_| ())
}

pub fn save_policy(path: &Path, text: &str) -> Result<(), String> {
    let parsed = parse_policy(text)?;
    let canonical = serde_json::to_vec_pretty(&parsed)
        .map_err(|error| format!("unable to serialize media-state policy: {error}"))?;
    crate::safe_write::replace_binary_file(path, &canonical, "media-state policy", |staged| {
        let value = std::fs::read_to_string(staged)
            .map_err(|error| format!("unable to verify media-state policy: {error}"))?;
        parse_policy(&value).map(|_| ())
    })?;
    *POLICY
        .write()
        .map_err(|_| "media-state policy lock is unavailable".to_string())? = parsed;
    Ok(())
}

pub fn activate_policy(text: &str) -> Result<(), String> {
    let parsed = parse_policy(text)?;
    *POLICY
        .write()
        .map_err(|_| "media-state policy lock is unavailable".to_string())? = parsed;
    Ok(())
}

pub fn activate_policy_revision(text: &str, revision: i64) -> Result<(), String> {
    activate_policy(text)?;
    ACTIVE_POSTGRES_REVISION.store(revision.max(0), Ordering::SeqCst);
    Ok(())
}

pub fn active_revision() -> i64 {
    ACTIVE_POSTGRES_REVISION.load(Ordering::SeqCst)
}

pub async fn fetch_postgres_policy(pool: &PgPool) -> Result<PostgresPolicySnapshot, String> {
    let row = sqlx::query(
        "SELECT policy::text AS policy_json,revision FROM media_state_classification_policy WHERE singleton=TRUE",
    )
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("PostgreSQL media-state policy is unavailable: {error}"))?
    .ok_or_else(|| "PostgreSQL media-state policy has not been initialized by WebGUI.".to_string())?;
    let raw = row
        .try_get::<String, _>("policy_json")
        .map_err(|error| format!("PostgreSQL media-state policy is unreadable: {error}"))?;
    let parsed = parse_policy(&raw)?;
    let canonical = serde_json::to_string_pretty(&parsed)
        .map_err(|error| format!("PostgreSQL media-state policy cannot be serialized: {error}"))?
        + "\n";
    parse_policy(&canonical)?;
    Ok(PostgresPolicySnapshot {
        policy_json: canonical,
        revision: row.try_get::<i64, _>("revision").map_err(|error| {
            format!("PostgreSQL media-state policy revision is unreadable: {error}")
        })?,
    })
}

pub async fn save_postgres_policy(
    pool: &PgPool,
    text: &str,
) -> Result<PostgresPolicySnapshot, String> {
    let parsed = parse_policy(text)?;
    let canonical_input = serde_json::to_string(&parsed)
        .map_err(|error| format!("Unable to serialize media-state policy: {error}"))?;
    let row = sqlx::query(
        "INSERT INTO media_state_classification_policy(singleton,policy,revision,updated_by) VALUES(TRUE,$1::jsonb,1,'windows-settings') ON CONFLICT(singleton) DO UPDATE SET policy=excluded.policy,revision=media_state_classification_policy.revision+1,updated_at=now(),updated_by=excluded.updated_by RETURNING policy::text AS policy_json,revision",
    )
    .bind(canonical_input)
    .fetch_one(pool)
    .await
    .map_err(|error| format!("Unable to save PostgreSQL media-state policy: {error}"))?;
    let raw = row
        .try_get::<String, _>("policy_json")
        .map_err(|error| format!("Saved PostgreSQL media-state policy is unreadable: {error}"))?;
    let value = serde_json::from_str::<serde_json::Value>(&raw)
        .map_err(|error| format!("Saved PostgreSQL media-state policy JSON is invalid: {error}"))?;
    let canonical = serde_json::to_string_pretty(&value)
        .map_err(|error| format!("Saved PostgreSQL policy cannot be serialized: {error}"))?
        + "\n";
    parse_policy(&canonical)?;
    Ok(PostgresPolicySnapshot {
        policy_json: canonical,
        revision: row
            .try_get::<i64, _>("revision")
            .map_err(|error| format!("Saved PostgreSQL policy revision is unreadable: {error}"))?,
    })
}

pub fn policy_path(config: &crate::config::Config) -> Result<PathBuf, String> {
    let directory = config.credentials.credential_dir.trim();
    if directory.is_empty() {
        return Err(
            "Provider Credential Directory is required for the media-state policy.".to_string(),
        );
    }
    Ok(PathBuf::from(directory).join(POLICY_FILENAME))
}

/// Adopt an existing policy without rewriting it, or create the first policy
/// when Setup saves into an otherwise empty Provider Credential Directory.
pub fn ensure_policy(config: &crate::config::Config) -> Result<PathBuf, String> {
    let path = policy_path(config)?;
    if path.is_file() {
        let text = std::fs::read_to_string(&path).map_err(|error| {
            format!(
                "Unable to read existing media-state policy {}: {error}",
                path.display()
            )
        })?;
        let parsed = parse_policy(&text)?;
        let stored_id = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|value| {
                value
                    .get("policy_id")
                    .and_then(|id| id.as_str())
                    .map(str::to_string)
            });
        if stored_id.as_deref() == Some(LEGACY_POLICY_ID) {
            save_policy(
                &path,
                &serde_json::to_string_pretty(&parsed)
                    .map_err(|error| format!("Unable to migrate media-state policy: {error}"))?,
            )?;
            return Ok(path);
        }
        *POLICY
            .write()
            .map_err(|_| "media-state policy lock is unavailable".to_string())? = parsed;
        return Ok(path);
    }
    save_policy(&path, &policy_json())?;
    Ok(path)
}

fn classify(
    policy: &MediaStatePolicy,
    rules: &[StateRule],
    forbidden: &[String],
    conditions: &[&str],
) -> MediaStateClassification {
    let evidence = conditions.iter().copied().collect::<HashSet<_>>();
    let rule = rules
        .iter()
        .find(|rule| rule.condition == "default" || evidence.contains(rule.condition.as_str()))
        .expect("media-state policy requires a default rule");
    assert!(
        !forbidden.iter().any(|value| value == &rule.color),
        "media-state policy selected forbidden name color {}",
        rule.color
    );
    MediaStateClassification {
        key: rule.key.clone(),
        ui_status_key: rule.ui_status_key.clone(),
        label: rule.label.clone(),
        color_name: rule.color.clone(),
        color: policy
            .colors
            .get(&rule.color)
            .cloned()
            .expect("media-state color must be defined"),
    }
}

fn load_policy() -> MediaStatePolicy {
    for path in policy_candidates() {
        match std::fs::read_to_string(&path) {
            Ok(text) => match parse_policy(&text) {
                Ok(value) => return value,
                Err(error) => panic!(
                    "invalid external media-state-classification.json {}: {error}",
                    path.display()
                ),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!(
                "unable to read external media-state-classification.json {}: {error}",
                path.display()
            ),
        }
    }
    parse_policy(DEFAULT_POLICY).expect("repository media-state policy seed must be valid")
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

fn parse_policy(text: &str) -> Result<MediaStatePolicy, String> {
    let mut value = serde_json::from_str::<MediaStatePolicy>(text)
        .map_err(|error| format!("invalid media-state policy JSON: {error}"))?;
    if value.policy_id == LEGACY_POLICY_ID {
        let current = serde_json::from_str::<MediaStatePolicy>(DEFAULT_POLICY)
            .map_err(|error| format!("invalid repository media-state policy: {error}"))?;
        value.policy_id = CURRENT_POLICY_ID.to_string();
        value.artist = current.artist;
        value.album = current.album;
        if let Some(unprocessed) = current.ui_status_colors.get("unprocessed") {
            value
                .ui_status_colors
                .insert("unprocessed".to_string(), unprocessed.clone());
        }
    }
    if value.schema_version != 1 {
        return Err(format!(
            "unsupported media-state policy schema {}",
            value.schema_version
        ));
    }
    if value.artist.ranked_states.is_empty() || value.album.ranked_name_states.is_empty() {
        return Err("media-state policy requires Artist and Album rules".to_string());
    }
    validate_colors(&value.colors)?;
    if !value
        .artist
        .ranked_states
        .iter()
        .any(|rule| rule.condition == "default")
        || !value
            .album
            .ranked_name_states
            .iter()
            .any(|rule| rule.condition == "default")
    {
        return Err("media-state policy requires default Artist and Album rules".to_string());
    }
    for required in ["white", "dark_grey", "orange", "gold", "blue"] {
        if !value
            .artist
            .forbidden_name_colors
            .iter()
            .any(|color| color == required)
        {
            return Err(format!(
                "media-state policy Artist forbidden_name_colors must include {required}"
            ));
        }
    }
    for required in ["white", "blue"] {
        if !value
            .album
            .forbidden_name_colors
            .iter()
            .any(|color| color == required)
        {
            return Err(format!(
                "media-state policy Album forbidden_name_colors must include {required}"
            ));
        }
    }
    for rule in value
        .artist
        .ranked_states
        .iter()
        .chain(value.album.ranked_name_states.iter())
    {
        if rule.condition.trim().is_empty()
            || rule.key.trim().is_empty()
            || rule.ui_status_key.trim().is_empty()
            || rule.label.trim().is_empty()
        {
            return Err("media-state policy contains an incomplete state rule".to_string());
        }
        if !value.colors.contains_key(&rule.color) {
            return Err(format!(
                "media-state policy references unknown color {}",
                rule.color
            ));
        }
    }
    for rule in &value.artist.ranked_states {
        if value
            .artist
            .forbidden_name_colors
            .iter()
            .any(|color| color == &rule.color)
        {
            return Err(format!("Artist names may not use {}", rule.color));
        }
    }
    for rule in &value.album.ranked_name_states {
        if value
            .album
            .forbidden_name_colors
            .iter()
            .any(|color| color == &rule.color)
        {
            return Err(format!("Album names may not use {}", rule.color));
        }
    }
    for (status, color) in &value.ui_status_colors {
        if !value.colors.contains_key(color) {
            return Err(format!(
                "media-state UI status {status} references unknown color {color}"
            ));
        }
    }
    for backend in ["current", "sqlite", "postgres"] {
        if value
            .database_monitor
            .backends
            .get(backend)
            .is_none_or(|label| label.trim().is_empty())
        {
            return Err(format!(
                "media-state policy requires database monitor backend {backend}"
            ));
        }
    }
    if !(30..=3600).contains(&value.database_monitor.poll_seconds) {
        return Err(
            "media-state policy database monitor poll_seconds must be from 30 through 3600"
                .to_string(),
        );
    }
    for health in ["current", "good", "available", "broken"] {
        let Some(state) = value.database_monitor.states.get(health) else {
            return Err(format!(
                "media-state policy requires database monitor state {health}"
            ));
        };
        if state.label.trim().is_empty() || !value.colors.contains_key(&state.color) {
            return Err(format!(
                "media-state policy has an invalid database monitor state {health}"
            ));
        }
    }
    for dot in &value.album.status_dots {
        if dot.condition.trim().is_empty()
            || dot.label.trim().is_empty()
            || !value.colors.contains_key(&dot.color)
        {
            return Err("media-state policy contains an invalid Album status dot".to_string());
        }
    }
    if value.override_presentation.condition != "opposing_override"
        || !value
            .colors
            .contains_key(&value.override_presentation.frame_color)
        || !value
            .colors
            .contains_key(&value.override_presentation.status_dot_color)
    {
        return Err("media-state policy has an invalid override presentation".to_string());
    }
    Ok(value)
}

fn validate_colors(colors: &BTreeMap<String, String>) -> Result<(), String> {
    if colors.is_empty() {
        return Err("media-state policy requires named colors".to_string());
    }
    for (name, color) in colors {
        let valid = color.len() == 7
            && color.starts_with('#')
            && color[1..].bytes().all(|value| value.is_ascii_hexdigit());
        if !valid {
            return Err(format!("media-state color {name} must be a #RRGGBB value"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn artist_policy_never_selects_album_only_colors() {
        for conditions in [
            vec!["missing_artist_art"],
            vec!["embedding_issues"],
            vec!["incomplete_or_error"],
            vec!["opposing_override"],
            vec!["all_albums_complete_or_overridden"],
            vec![],
        ] {
            let state = classify_artist(&conditions);
            assert!(!matches!(
                state.color_name.as_str(),
                "white" | "dark_grey" | "orange" | "gold" | "blue"
            ));
        }
    }

    #[test]
    fn artist_and_album_rankings_are_independent() {
        let artist = classify_artist(&["missing_artist_art", "embedding_issues"]);
        assert_eq!(artist.color_name, "yellow");
        let album = classify_album(&["embedding_issues", "incomplete_tracks", "processed"]);
        assert_eq!(album.color_name, "red");
        assert_eq!(classify_artist(&[]).color_name, "red");
        assert_eq!(classify_album(&[]).color_name, "red");
    }

    #[test]
    fn red_album_definition_is_track_only() {
        assert_eq!(policy().album.red_definition, "Incomplete album tracks");
        assert_eq!(classify_album(&["incomplete_tracks"]).color_name, "red");
        assert_eq!(classify_album(&["missing_art"]).color_name, "yellow");
    }

    #[test]
    fn database_monitor_uses_policy_labels_and_semantic_colors() {
        let local = database_monitor_presentation("current", "current");
        assert_eq!(local.text, "Current");
        assert_eq!(local.color_name, "green");
        let broken = database_monitor_presentation("postgres", "broken");
        assert_eq!(broken.text, "Postgres");
        assert_eq!(broken.color_name, "red");
        assert_eq!(database_monitor_poll_seconds(), 180);
    }

    #[test]
    fn editor_validation_rejects_an_unknown_color() {
        let invalid = DEFAULT_POLICY.replace("\"color\": \"yellow\"", "\"color\": \"not-defined\"");
        assert!(
            validate_policy(&invalid)
                .expect_err("unknown color must fail")
                .contains("unknown color")
        );
    }

    #[test]
    fn setup_adopts_existing_credential_directory_policy_without_rewriting_it() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join(POLICY_FILENAME);
        let existing = format!("\n{DEFAULT_POLICY}\n");
        std::fs::write(&path, &existing).unwrap();
        let mut config = crate::config::Config::default();
        config.credentials.credential_dir = temp.path().to_string_lossy().into_owned();

        assert_eq!(ensure_policy(&config).unwrap(), path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), existing);
    }

    #[test]
    fn setup_migrates_the_shipped_v1_classification_without_white_names() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join(POLICY_FILENAME);
        let legacy = DEFAULT_POLICY.replace(CURRENT_POLICY_ID, LEGACY_POLICY_ID);
        std::fs::write(&path, legacy).unwrap();
        let mut config = crate::config::Config::default();
        config.credentials.credential_dir = temp.path().to_string_lossy().into_owned();

        assert_eq!(ensure_policy(&config).unwrap(), path);
        let migrated = std::fs::read_to_string(path).unwrap();
        assert!(migrated.contains(CURRENT_POLICY_ID));
        assert!(!migrated.contains("\"color\": \"white\""));
    }
}
