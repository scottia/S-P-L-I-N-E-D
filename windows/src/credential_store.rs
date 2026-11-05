//! Rust-owned provider credential editing and validation.
//!
//! Credentials stay in the configured credential directory, never in Config
//! v5, SQLite, or diagnostic logs. Writes use the same atomic replacement and
//! user-only ACL policy as the processing engine.

use crate::config::Config;
use crate::credentials::{
    DiscogsCredential, FANARTTV_API_VERSION, FanartTvCredential, LastFmCredential,
};
use crate::override_coordination::connect_pool;
use crate::safe_write::replace_text_file;
use crate::source::navidrome::{self, NavidromeCredential};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use url::Url;

pub const ALBUM_COORDINATION_PROVIDER: &str = "Album Coordination";

pub const PROVIDERS: [&str; 6] = [
    "Navidrome",
    "Fanart.tv",
    "Last.fm",
    "Discogs",
    "MusicBrainz OAuth",
    ALBUM_COORDINATION_PROVIDER,
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CredentialEditor {
    pub provider: String,
    pub server_name: String,
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
            value.server_name = "SPLINED".to_string();
        } else if value.provider == ALBUM_COORDINATION_PROVIDER {
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
            editor.server_name = if credential.client_name.trim().is_empty() {
                "SPLINED".to_string()
            } else {
                credential.client_name
            };
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
        ALBUM_COORDINATION_PROVIDER => {
            editor.server_url = text(&object, "server_url");
            editor.username = text(&object, "username");
            editor.secret = text(&object, "password");
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
            object.insert("client_name".into(), json!(editor.server_name.trim()));
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
        ALBUM_COORDINATION_PROVIDER => {
            album_coordination_database_url(editor)?;
            object.insert("server_url".into(), json!(editor.server_url.trim()));
            object.insert("username".into(), json!(editor.username.trim()));
            object.insert("password".into(), json!(editor.secret));
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
                client_name: editor.server_name.clone(),
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
        ALBUM_COORDINATION_PROVIDER => {
            let database_url = album_coordination_database_url(editor)?;
            let pool = connect_pool(&database_url).await?;
            sqlx::query_scalar::<_, i32>("SELECT 1")
                .fetch_one(&pool)
                .await
                .map_err(|error| format!("Album coordination PostgreSQL test failed: {error}"))?;
            let coordination_table = sqlx::query_scalar::<_, Option<String>>(
                "SELECT to_regclass('album_artwork_write_coordination')::text",
            )
            .fetch_one(&pool)
            .await
            .map_err(|error| format!("Album coordination schema test failed: {error}"))?;
            pool.close().await;
            if coordination_table.is_none() {
                return Err(
                    "Album Coordination migration is not present in this PostgreSQL database."
                        .to_string(),
                );
            }
            Ok("Connected to Album Coordination PostgreSQL.".to_string())
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
                client_name: editor.server_name.clone(),
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
        ALBUM_COORDINATION_PROVIDER => album_coordination_database_url(editor).map(|_| ()),
        _ => Ok(()),
    }
}

pub fn load_album_coordination_database_url(config: &Config) -> Result<Option<String>, String> {
    let path = credential_path(config, ALBUM_COORDINATION_PROVIDER)?;
    if !path.is_file() {
        return Ok(None);
    }
    load_editor(config, ALBUM_COORDINATION_PROVIDER)
        .and_then(|editor| album_coordination_database_url(&editor))
        .map(Some)
}

fn album_coordination_database_url(editor: &CredentialEditor) -> Result<String, String> {
    let mut url = Url::parse(editor.server_url.trim())
        .map_err(|_| "Album Coordination requires a valid postgresql:// server URL.".to_string())?;
    if !matches!(url.scheme(), "postgres" | "postgresql") {
        return Err(
            "Album Coordination requires a postgres:// or postgresql:// server URL.".to_string(),
        );
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(
            "Put the PostgreSQL username and password in their separate fields, not in the server URL."
                .to_string(),
        );
    }
    let username = editor.username.trim();
    if username.is_empty() {
        return Err("Album Coordination requires a PostgreSQL username.".to_string());
    }
    if editor.secret.is_empty() {
        return Err("Album Coordination requires a PostgreSQL password.".to_string());
    }
    if url.path().trim_matches('/').is_empty() {
        return Err("Album Coordination server URL must include the database name.".to_string());
    }
    url.set_username(username)
        .map_err(|_| "Album Coordination PostgreSQL username is invalid.".to_string())?;
    url.set_password(Some(&editor.secret))
        .map_err(|_| "Album Coordination PostgreSQL password is invalid.".to_string())?;
    Ok(url.as_str().to_string())
}

fn canonical_provider(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "navidrome" => "Navidrome",
        "fanart.tv" | "fanarttv" => "Fanart.tv",
        "last.fm" | "lastfm" => "Last.fm",
        "discogs" => "Discogs",
        "musicbrainz" | "musicbrainz oauth" => "MusicBrainz OAuth",
        "album coordination" | "webgui coordination" => ALBUM_COORDINATION_PROVIDER,
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
        ALBUM_COORDINATION_PROVIDER => Some("album-coordination.json"),
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
            server_name: "SPLINED Dev PC".into(),
            server_url: "https://music.example.test/".into(),
            auth_method: "password".into(),
            username: "listener".into(),
            secret: "secret".into(),
            ..CredentialEditor::default()
        };
        save_editor(&config, &editor).unwrap();
        let loaded = load_editor(&config, "navidrome").unwrap();
        assert_eq!(loaded.server_url, "https://music.example.test");
        assert_eq!(loaded.server_name, "SPLINED Dev PC");
        assert_eq!(loaded.username, "listener");
        assert_eq!(loaded.secret, "secret");
        assert!(!serde_json::to_string(&config).unwrap().contains("secret"));
    }

    #[test]
    fn invalid_provider_names_cannot_escape_the_credential_directory() {
        let dir = TempDir::new().unwrap();
        assert!(credential_path(&config(dir.path()), "../outside").is_err());
    }

    #[test]
    fn album_coordination_round_trip_is_persistent_and_keeps_config_secret_free() {
        let dir = TempDir::new().unwrap();
        let config = config(dir.path());
        let editor = CredentialEditor {
            provider: ALBUM_COORDINATION_PROVIDER.into(),
            server_url: "postgresql://127.0.0.1:55432/splined?sslmode=disable".into(),
            username: "splined test".into(),
            secret: "synthetic p@ssword".into(),
            ..CredentialEditor::default()
        };
        save_editor(&config, &editor).unwrap();
        let loaded = load_editor(&config, ALBUM_COORDINATION_PROVIDER).unwrap();
        assert_eq!(loaded.server_url, editor.server_url);
        assert_eq!(loaded.username, editor.username);
        assert_eq!(loaded.secret, editor.secret);
        let database_url = load_album_coordination_database_url(&config)
            .unwrap()
            .unwrap();
        assert!(database_url.contains("splined%20test"));
        assert!(database_url.contains("synthetic%20p%40ssword"));
        assert!(
            !serde_json::to_string(&config)
                .unwrap()
                .contains("synthetic")
        );
    }

    #[test]
    fn album_coordination_rejects_embedded_credentials_and_missing_database() {
        let mut editor = CredentialEditor {
            provider: ALBUM_COORDINATION_PROVIDER.into(),
            server_url: "postgresql://user:secret@example.test/splined".into(),
            username: "user".into(),
            secret: "secret".into(),
            ..CredentialEditor::default()
        };
        assert!(album_coordination_database_url(&editor).is_err());
        editor.server_url = "postgresql://example.test".into();
        assert!(album_coordination_database_url(&editor).is_err());
    }
}
