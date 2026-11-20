use crate::safe_write::{recover_backup_if_needed, replace_text_file};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LastFmCredential {
    pub api_key: String,
    pub shared_secret: String,
    pub username: String,
    pub session_key: String,
    pub subscriber: bool,
}

pub const FANARTTV_API_VERSION: &str = "v3.2";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FanartTvCredential {
    pub api_key: String,
    pub client_key: String,
    pub api_version: String,
}

impl Default for FanartTvCredential {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            client_key: String::new(),
            api_version: FANARTTV_API_VERSION.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DiscogsCredential {
    pub token: String,
}

// Portable policy: credential paths must come from the portable configuration.
// There is intentionally no OS/AppData fallback.
pub fn resolve_credential_path(configured_path: &str, provider: &str) -> Result<PathBuf, String> {
    let configured_path = configured_path.trim();

    if configured_path.is_empty() {
        Err(format!(
            "SPLINED {provider} credential path is not configured."
        ))
    } else {
        Ok(PathBuf::from(configured_path))
    }
}

pub fn load_lastfm_credential(path: &Path) -> Result<LastFmCredential, String> {
    recover_backup_if_needed(path, "Last.fm credential")?;

    let body = std::fs::read_to_string(path).map_err(|error| {
        format!(
            "Unable to read Last.fm credential file {}: {error}",
            path.display()
        )
    })?;

    let credential: LastFmCredential = serde_json::from_str(&body).map_err(|error| {
        format!(
            "Invalid Last.fm credential file {}: {error}",
            path.display()
        )
    })?;

    validate_lastfm_credential(&credential, path)?;

    Ok(credential)
}

pub fn load_fanarttv_credential(path: &Path) -> Result<FanartTvCredential, String> {
    recover_backup_if_needed(path, "Fanart.tv credential")?;

    let body = std::fs::read_to_string(path).map_err(|error| {
        format!(
            "Unable to read Fanart.tv credential file {}: {error}",
            path.display()
        )
    })?;

    let credential: FanartTvCredential = serde_json::from_str(&body).map_err(|error| {
        format!(
            "Invalid Fanart.tv credential file {}: {error}",
            path.display()
        )
    })?;

    validate_fanarttv_credential(&credential, path)?;

    Ok(credential)
}

pub fn load_discogs_credential(path: &Path) -> Result<DiscogsCredential, String> {
    recover_backup_if_needed(path, "Discogs credential")?;

    let body = std::fs::read_to_string(path).map_err(|error| {
        format!(
            "Unable to read Discogs credential file {}: {error}",
            path.display()
        )
    })?;

    let credential: DiscogsCredential = serde_json::from_str(&body).map_err(|error| {
        format!(
            "Invalid Discogs credential file {}: {error}",
            path.display()
        )
    })?;

    if credential.token.trim().is_empty() {
        return Err(format!(
            "Discogs credential file contains no token: {}",
            path.display()
        ));
    }

    Ok(credential)
}

pub fn save_lastfm_credential(path: &Path, credential: &LastFmCredential) -> Result<(), String> {
    validate_lastfm_credential(credential, path)?;

    let json = serde_json::to_string_pretty(credential)
        .map_err(|error| format!("Unable to serialize Last.fm credential: {error}"))?;
    let body = format!("{json}\n");

    replace_text_file(path, &body, "Last.fm credential", |candidate| {
        let parsed: LastFmCredential = serde_json::from_str(candidate)
            .map_err(|error| format!("Invalid staged Last.fm credential: {error}"))?;
        validate_lastfm_credential(&parsed, path)
    })
}

pub fn save_fanarttv_credential(
    path: &Path,
    credential: &FanartTvCredential,
) -> Result<(), String> {
    validate_fanarttv_credential(credential, path)?;

    let json = serde_json::to_string_pretty(credential)
        .map_err(|error| format!("Unable to serialize Fanart.tv credential: {error}"))?;
    let body = format!("{json}\n");

    replace_text_file(path, &body, "Fanart.tv credential", |candidate| {
        let parsed: FanartTvCredential = serde_json::from_str(candidate)
            .map_err(|error| format!("Invalid staged Fanart.tv credential: {error}"))?;
        validate_fanarttv_credential(&parsed, path)
    })
}

fn validate_lastfm_credential(credential: &LastFmCredential, path: &Path) -> Result<(), String> {
    if credential.api_key.trim().is_empty() {
        return Err(format!(
            "Last.fm credential file contains no api_key: {}",
            path.display()
        ));
    }

    Ok(())
}

fn validate_fanarttv_credential(
    credential: &FanartTvCredential,
    path: &Path,
) -> Result<(), String> {
    if credential.api_key.trim().is_empty() {
        return Err(format!(
            "Fanart.tv credential file contains no api_key: {}",
            path.display()
        ));
    }

    if credential.api_version.trim() != FANARTTV_API_VERSION {
        return Err(format!(
            "Fanart.tv credential file must use api_version \"{FANARTTV_API_VERSION}\": {}",
            path.display()
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn blank_credential_path_has_no_os_fallback() {
        let error = resolve_credential_path("", "Last.fm").unwrap_err();
        assert!(error.contains("not configured"));
    }

    #[test]
    fn explicit_credential_path_wins() {
        let configured = PathBuf::from("portable-credentials").join("lastfm.json");
        let configured_text = configured.to_string_lossy();

        let resolved = resolve_credential_path(&configured_text, "Last.fm")
            .expect("configured path should resolve");

        assert_eq!(resolved, configured);
    }

    #[test]
    fn lastfm_authenticated_credential_round_trip() {
        let dir = TempDir::new().expect("temp directory should create");
        let path = dir.path().join("lastfm.json");

        let credential = LastFmCredential {
            api_key: "fixture-lastfm-key".to_string(),
            shared_secret: "fixture-lastfm-secret".to_string(),
            username: "FixtureUser".to_string(),
            session_key: "fixture-session-key".to_string(),
            subscriber: true,
        };

        save_lastfm_credential(&path, &credential).expect("credential should save");
        let loaded = load_lastfm_credential(&path).expect("credential should load");
        assert_eq!(loaded, credential);
    }

    #[test]
    fn legacy_lastfm_api_key_only_file_loads_with_auth_defaults() {
        let dir = TempDir::new().expect("temp directory should create");
        let path = dir.path().join("lastfm.json");
        std::fs::write(&path, r#"{"api_key":"fixture-lastfm-key"}"#).expect("fixture should write");

        let loaded = load_lastfm_credential(&path).expect("credential should load");

        assert_eq!(loaded.api_key, "fixture-lastfm-key");
        assert!(loaded.shared_secret.is_empty());
        assert!(loaded.username.is_empty());
        assert!(loaded.session_key.is_empty());
        assert!(!loaded.subscriber);
    }

    #[test]
    fn empty_lastfm_api_key_fails() {
        let dir = TempDir::new().expect("temp directory should create");
        let path = dir.path().join("lastfm.json");
        let credential = LastFmCredential::default();
        assert!(save_lastfm_credential(&path, &credential).is_err());
    }

    #[test]
    fn fanarttv_project_and_client_keys_round_trip() {
        let dir = TempDir::new().expect("temp directory should create");
        let path = dir.path().join("fanarttv.json");

        let credential = FanartTvCredential {
            api_key: "fixture-project-key".to_string(),
            client_key: "fixture-client-key".to_string(),
            ..FanartTvCredential::default()
        };

        save_fanarttv_credential(&path, &credential).expect("credential should save");
        let loaded = load_fanarttv_credential(&path).expect("credential should load");
        assert_eq!(loaded, credential);
    }

    #[test]
    fn fanarttv_client_key_is_optional() {
        let dir = TempDir::new().expect("temp directory should create");
        let path = dir.path().join("fanarttv.json");
        let credential = FanartTvCredential {
            api_key: "fixture-project-key".to_string(),
            client_key: String::new(),
            ..FanartTvCredential::default()
        };
        assert!(save_fanarttv_credential(&path, &credential).is_ok());
    }

    #[test]
    fn fanarttv_requires_project_api_key() {
        let dir = TempDir::new().expect("temp directory should create");
        let path = dir.path().join("fanarttv.json");
        let credential = FanartTvCredential {
            api_key: String::new(),
            client_key: "fixture-client-key".to_string(),
            ..FanartTvCredential::default()
        };
        assert!(save_fanarttv_credential(&path, &credential).is_err());
    }

    #[test]
    fn legacy_fanarttv_file_defaults_to_v32() {
        let dir = TempDir::new().expect("temp directory should create");
        let path = dir.path().join("fanarttv.json");
        std::fs::write(
            &path,
            r#"{"api_key":"fixture-project-key","client_key":"fixture-client-key"}"#,
        )
        .expect("fixture should write");

        let loaded = load_fanarttv_credential(&path).expect("legacy credential should load");
        assert_eq!(loaded.api_version, FANARTTV_API_VERSION);
    }

    #[test]
    fn fanarttv_rejects_non_v32_version() {
        let dir = TempDir::new().expect("temp directory should create");
        let path = dir.path().join("fanarttv.json");
        let credential = FanartTvCredential {
            api_key: "fixture-project-key".to_string(),
            api_version: "v3".to_string(),
            ..FanartTvCredential::default()
        };

        assert!(save_fanarttv_credential(&path, &credential).is_err());
    }

    #[test]
    fn discogs_token_file_loads() {
        let dir = TempDir::new().expect("temp directory should create");
        let path = dir.path().join("discogs.json");
        std::fs::write(&path, r#"{"token":"fixture-discogs-token"}"#)
            .expect("fixture should write");

        let loaded = load_discogs_credential(&path).expect("credential should load");
        assert_eq!(loaded.token, "fixture-discogs-token");
    }

    #[test]
    fn existing_credential_can_be_replaced_after_confirmation() {
        let dir = TempDir::new().expect("temp directory should create");
        let path = dir.path().join("lastfm.json");

        save_lastfm_credential(
            &path,
            &LastFmCredential {
                api_key: "first".to_string(),
                ..LastFmCredential::default()
            },
        )
        .expect("first credential should save");

        save_lastfm_credential(
            &path,
            &LastFmCredential {
                api_key: "second".to_string(),
                ..LastFmCredential::default()
            },
        )
        .expect("replacement credential should save");

        let loaded = load_lastfm_credential(&path).expect("credential should load");
        assert_eq!(loaded.api_key, "second");
    }
}
