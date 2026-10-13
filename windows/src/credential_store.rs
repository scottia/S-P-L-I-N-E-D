//! Rust-owned provider credential editing and validation.
//!
//! Credentials stay in the configured credential directory, never in Config
//! v5, SQLite, or diagnostic logs. Writes use the same atomic replacement and
//! user-only ACL policy as the processing engine.

use crate::config::Config;
use crate::credentials::{
    DiscogsCredential, FANARTTV_API_VERSION, FanartTvCredential, LastFmCredential,
};
use crate::safe_write::replace_text_file;
use crate::source::navidrome::{self, NavidromeCredential};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

pub const PROVIDERS: [&str; 5] = [
    "Navidrome",
    "Fanart.tv",
    "Last.fm",
    "Discogs",
    "MusicBrainz OAuth",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CredentialEditor {
    pub provider: String,
    pub server_url: String,
    pub auth_method: String,
    pub username: String,
    pub identifier: String,
    pub secret: String,
    pub status: String,
}

impl CredentialEditor {
    pub fn blank(provider: &str) -> Self {
        let mut value = Self {
            provider: canonical_provider(provider).to_string(),
            status: "Not configured".to_string(),
            ..Self::default()
        };
        if value.provider == "Navidrome" {
            value.auth_method = "password".to_string();
        }
        value
    }
}

pub fn credential_path(config: &Config, provider: &str) -> Result<PathBuf, String> {
    let file = provider_filename(provider)
        .ok_or_else(|| format!("Unsupported SPLINED credential provider: {provider}"))?;
    Ok(Path::new(&config.credentials.credential_dir).join(file))
}

pub fn load_editor(config: &Config, provider: &str) -> Result<CredentialEditor, String> {
    let provider = canonical_provider(provider);
    let path = credential_path(config, provider)?;
    if !path.is_file() {
        return Ok(CredentialEditor::blank(provider));
    }
    let body = std::fs::read_to_string(&path).map_err(|error| {
        format!(
            "Unable to read {provider} credential file {}: {error}",
            path.display()
        )
    })?;
    let object = parse_object(&body, provider)?;
    let mut editor = CredentialEditor::blank(provider);
    editor.status = "Saved".to_string();
    match provider {
        "Navidrome" => {
            let credential: NavidromeCredential = serde_json::from_value(Value::Object(object))
                .map_err(|error| format!("Invalid Navidrome credential: {error}"))?;
            let uses_api_key = credential.uses_api_key();
            editor.server_url = credential.server_url;
            editor.auth_method = if uses_api_key {
                "api_key".to_string()
            } else {
                "password".to_string()
            };
            editor.username = credential.username;
            if uses_api_key {
                editor.secret = credential.api_key;
            } else {
                editor.secret = credential.password;
            }
        }
        "Fanart.tv" => {
            editor.identifier = text(&object, "api_key");
            editor.secret = text(&object, "client_key");
        }
        "Last.fm" => {
            editor.identifier = text(&object, "api_key");
            editor.secret = text(&object, "shared_secret");
            editor.username = text(&object, "username");
        }
        "Discogs" => editor.secret = text(&object, "token"),
        "MusicBrainz OAuth" => {
            editor.identifier = text(&object, "client_id");
            editor.secret = text(&object, "client_secret");
            editor.username = text(&object, "username");
        }
        _ => unreachable!(),
    }
    Ok(editor)
}

pub fn save_editor(config: &Config, editor: &CredentialEditor) -> Result<(), String> {
    let provider = canonical_provider(&editor.provider);
    let path = credential_path(config, provider)?;
    let mut object = if path.is_file() {
        parse_object(
            &std::fs::read_to_string(&path)
                .map_err(|error| format!("Unable to read {}: {error}", path.display()))?,
            provider,
        )?
    } else {
        Map::new()
    };
    match provider {
        "Navidrome" => {
            object.insert("server_url".into(), json!(editor.server_url.trim()));
            object.insert(
                "auth_method".into(),
                json!(editor.auth_method.trim().to_ascii_lowercase()),
            );
            object.insert("username".into(), json!(editor.username.trim()));
            if editor.auth_method.trim().eq_ignore_ascii_case("api_key") {
                object.insert("api_key".into(), json!(editor.secret.trim()));
                object.insert("password".into(), json!(""));
            } else {
                object.insert("password".into(), json!(editor.secret));
                object.insert("api_key".into(), json!(""));
            }
            object.entry("bearer_token").or_insert_with(|| json!(""));
            let credential: NavidromeCredential =
                serde_json::from_value(Value::Object(object.clone()))
                    .map_err(|error| format!("Invalid Navidrome credential: {error}"))?;
            return navidrome::save_credential(&path, &credential);
        }
        "Fanart.tv" => {
            object.insert("api_key".into(), json!(editor.identifier.trim()));
            object.insert("client_key".into(), json!(editor.secret.trim()));
            object.insert("api_version".into(), json!(FANARTTV_API_VERSION));
            let credential: FanartTvCredential = from_object(&object, provider)?;
            if credential.api_key.trim().is_empty() {
                return Err("Fanart.tv requires a project API key.".to_string());
            }
        }
        "Last.fm" => {
            object.insert("api_key".into(), json!(editor.identifier.trim()));
            object.insert("shared_secret".into(), json!(editor.secret.trim()));
            object.insert("username".into(), json!(editor.username.trim()));
            object.entry("session_key").or_insert_with(|| json!(""));
            object.entry("subscriber").or_insert_with(|| json!(false));
            let credential: LastFmCredential = from_object(&object, provider)?;
            if credential.api_key.trim().is_empty() {
                return Err("Last.fm requires an API key.".to_string());
            }
        }
        "Discogs" => {
            object.insert("token".into(), json!(editor.secret.trim()));
            let credential: DiscogsCredential = from_object(&object, provider)?;
            if credential.token.trim().is_empty() {
                return Err("Discogs requires a personal access token.".to_string());
            }
        }
        "MusicBrainz OAuth" => {
            object.insert("client_id".into(), json!(editor.identifier.trim()));
            object.insert("client_secret".into(), json!(editor.secret.trim()));
            object.insert("username".into(), json!(editor.username.trim()));
            if editor.identifier.trim().is_empty() {
                return Err("MusicBrainz OAuth requires a client ID.".to_string());
            }
        }
        _ => unreachable!(),
    }
    let json = serde_json::to_string_pretty(&Value::Object(object))
        .map_err(|error| format!("Unable to serialize {provider} credential: {error}"))?;
    replace_text_file(
        &path,
        &format!("{json}\n"),
        &format!("{provider} credential"),
        |candidate| parse_object(candidate, provider).map(|_| ()),
    )
}

pub async fn test_editor(editor: &CredentialEditor) -> Result<String, String> {
    match canonical_provider(&editor.provider) {
        "Navidrome" => {
            let credential = NavidromeCredential {
                server_url: editor.server_url.clone(),
                auth_method: editor.auth_method.clone(),
                username: editor.username.clone(),
                password: if editor.auth_method.eq_ignore_ascii_case("api_key") {
                    String::new()
                } else {
                    editor.secret.clone()
                },
                api_key: if editor.auth_method.eq_ignore_ascii_case("api_key") {
                    editor.secret.clone()
                } else {
                    String::new()
                },
                bearer_token: String::new(),
            };
            navidrome::test_credential(&credential).await
        }
        provider => {
            validate_editor_locally(editor)?;
            Ok(format!(
                "{provider} credential fields are valid and ready to save."
            ))
        }
    }
}

pub fn validate_editor_locally(editor: &CredentialEditor) -> Result<(), String> {
    match canonical_provider(&editor.provider) {
        "Fanart.tv" | "Last.fm" if editor.identifier.trim().is_empty() => {
            Err(format!("{} requires an API key.", editor.provider))
        }
        "Discogs" if editor.secret.trim().is_empty() => {
            Err("Discogs requires a personal access token.".to_string())
        }
        "MusicBrainz OAuth" if editor.identifier.trim().is_empty() => {
            Err("MusicBrainz OAuth requires a client ID.".to_string())
        }
        "Navidrome" => {
            let credential = NavidromeCredential {
                server_url: editor.server_url.clone(),
                auth_method: editor.auth_method.clone(),
                username: editor.username.clone(),
                password: if editor.auth_method.eq_ignore_ascii_case("api_key") {
                    String::new()
                } else {
                    editor.secret.clone()
                },
                api_key: if editor.auth_method.eq_ignore_ascii_case("api_key") {
                    editor.secret.clone()
                } else {
                    String::new()
                },
                bearer_token: String::new(),
            };
            credential.validate()
        }
        _ => Ok(()),
    }
}

fn canonical_provider(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "navidrome" => "Navidrome",
        "fanart.tv" | "fanarttv" => "Fanart.tv",
        "last.fm" | "lastfm" => "Last.fm",
        "discogs" => "Discogs",
        "musicbrainz" | "musicbrainz oauth" => "MusicBrainz OAuth",
        _ => value.trim(),
    }
}

fn provider_filename(provider: &str) -> Option<&'static str> {
    match canonical_provider(provider) {
        "Navidrome" => Some("navidrome.json"),
        "Fanart.tv" => Some("fanarttv.json"),
        "Last.fm" => Some("lastfm.json"),
        "Discogs" => Some("discogs.json"),
        "MusicBrainz OAuth" => Some("musicbrainz.json"),
        _ => None,
    }
}

fn parse_object(body: &str, provider: &str) -> Result<Map<String, Value>, String> {
    serde_json::from_str::<Value>(body)
        .map_err(|error| format!("Invalid {provider} credential JSON: {error}"))?
        .as_object()
        .cloned()
        .ok_or_else(|| format!("{provider} credential must be a JSON object."))
}

fn from_object<T: serde::de::DeserializeOwned>(
    object: &Map<String, Value>,
    provider: &str,
) -> Result<T, String> {
    serde_json::from_value(Value::Object(object.clone()))
        .map_err(|error| format!("Invalid {provider} credential: {error}"))
}

fn text(object: &Map<String, Value>, name: &str) -> String {
    object
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn config(root: &Path) -> Config {
        let mut config = Config::default();
        config.credentials.credential_dir = root.to_string_lossy().into_owned();
        config
    }

    #[test]
    fn navidrome_editor_round_trip_uses_separate_atomic_credential_file() {
        let dir = TempDir::new().unwrap();
        let config = config(dir.path());
        let editor = CredentialEditor {
            provider: "Navidrome".into(),
            server_url: "https://music.example.test/".into(),
            auth_method: "password".into(),
            username: "listener".into(),
            secret: "secret".into(),
            ..CredentialEditor::default()
        };
        save_editor(&config, &editor).unwrap();
        let loaded = load_editor(&config, "navidrome").unwrap();
        assert_eq!(loaded.server_url, "https://music.example.test");
        assert_eq!(loaded.username, "listener");
        assert_eq!(loaded.secret, "secret");
        assert!(!serde_json::to_string(&config).unwrap().contains("secret"));
    }

    #[test]
    fn invalid_provider_names_cannot_escape_the_credential_directory() {
        let dir = TempDir::new().unwrap();
        assert!(credential_path(&config(dir.path()), "../outside").is_err());
    }
}
