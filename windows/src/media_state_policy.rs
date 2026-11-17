//! Shared Artist and Album file-list state classification.
//!
//! A standalone SQLite installation and a coordinated PostgreSQL installation
//! both use the same JSON policy. `SPLINED_MEDIA_STATE_POLICY` may point both
//! runtimes at one external file. Release and Store packages ship that file
//! beside the executable; classification behavior is never compiled into the
//! application.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, RwLock};

#[cfg(test)]
const TEST_POLICY: &str = include_str!("../../schema/media-state-classification.json");
pub const POLICY_ENV: &str = "SPLINED_MEDIA_STATE_POLICY";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MediaStatePolicy {
    pub schema_version: u32,
    pub policy_id: String,
    pub colors: BTreeMap<String, String>,
    pub light_colors: BTreeMap<String, String>,
    pub ui_status_colors: BTreeMap<String, String>,
    pub artist: RankedPolicy,
    pub album: AlbumPolicy,
    pub override_presentation: OverridePresentation,
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
        .unwrap_or("white");
    policy
        .colors
        .get(color_name)
        .cloned()
        .unwrap_or_else(|| "#eef8ff".to_string())
}

pub fn named_color(color_name: &str) -> String {
    policy()
        .colors
        .get(color_name)
        .cloned()
        .unwrap_or_else(|| "#eef8ff".to_string())
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
    let canonical = serde_json::to_vec_pretty(
        &serde_json::from_str::<serde_json::Value>(text)
            .map_err(|error| format!("invalid media-state policy JSON: {error}"))?,
    )
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
    let mut errors = Vec::new();
    for path in policy_candidates() {
        match std::fs::read_to_string(&path) {
            Ok(text) => match parse_policy(&text) {
                Ok(value) => return value,
                Err(error) => errors.push(format!("{}: {error}", path.display())),
            },
            Err(error) => errors.push(format!("{}: {error}", path.display())),
        }
    }
    panic!(
        "external media-state-classification.json is required: {}",
        errors.join("; ")
    )
}

fn policy_candidates() -> Vec<PathBuf> {
    let mut result = Vec::new();
    if let Some(path) = std::env::var_os(POLICY_ENV).filter(|value| !value.is_empty()) {
        result.push(PathBuf::from(path));
    }
    if let Some(path) = std::env::var_os("SPLINED_CONFIG")
        .filter(|value| !value.is_empty())
        .and_then(|value| Path::new(&value).parent().map(Path::to_path_buf))
    {
        result.push(path.join("media-state-classification.json"));
    }
    if let Ok(layout) = crate::portable::app_layout()
        && let Some(directory) = layout.config_file.parent()
    {
        result.push(directory.join("media-state-classification.json"));
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(root) = executable.parent()
    {
        result.push(root.join("media-state-classification.json"));
        result.push(root.join("data").join("media-state-classification.json"));
    }
    #[cfg(debug_assertions)]
    result.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("schema")
            .join("media-state-classification.json"),
    );
    result
}

fn parse_policy(text: &str) -> Result<MediaStatePolicy, String> {
    let value = serde_json::from_str::<MediaStatePolicy>(text)
        .map_err(|error| format!("invalid media-state policy JSON: {error}"))?;
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
    for (status, color) in &value.ui_status_colors {
        if !value.colors.contains_key(color) {
            return Err(format!(
                "media-state UI status {status} references unknown color {color}"
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
                "orange" | "gold" | "blue"
            ));
        }
    }

    #[test]
    fn artist_and_album_rankings_are_independent() {
        let artist = classify_artist(&["missing_artist_art", "embedding_issues"]);
        assert_eq!(artist.color_name, "yellow");
        let album = classify_album(&["embedding_issues", "incomplete_tracks", "processed"]);
        assert_eq!(album.color_name, "purple");
    }

    #[test]
    fn red_album_definition_is_track_only() {
        assert_eq!(policy().album.red_definition, "Incomplete album tracks");
        assert_eq!(classify_album(&["incomplete_tracks"]).color_name, "red");
        assert_eq!(classify_album(&["missing_art"]).color_name, "yellow");
    }

    #[test]
    fn editor_validation_rejects_an_unknown_color() {
        let invalid = TEST_POLICY.replace("\"color\": \"yellow\"", "\"color\": \"not-defined\"");
        assert!(
            validate_policy(&invalid)
                .expect_err("unknown color must fail")
                .contains("unknown color")
        );
    }
}
