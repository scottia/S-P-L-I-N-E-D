use crate::config::{Config, load_config_text_at_root};
use crate::media_database::database_path;
use crate::portable::AppLayout;
use crate::safe_write::{replace_binary_file, replace_text_file};
use crate::windows_state::{parse_ui_state, ui_path};
use aes::Aes256;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2_legacy::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const BACKUP_FORMAT: &str = "SPLINED-BACKUP";
const BACKUP_VERSION: u32 = 1;
const PASSWORD_ITERATIONS: u32 = 150_000;
type Aes256CbcEncryptor = cbc::Encryptor<Aes256>;
type Aes256CbcDecryptor = cbc::Decryptor<Aes256>;
type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct BackupSelection {
    pub settings: bool,
    pub credentials: bool,
    pub database: bool,
    pub interface: bool,
    pub diagnostics: bool,
}

impl Default for BackupSelection {
    fn default() -> Self {
        Self {
            settings: true,
            credentials: true,
            database: false,
            interface: true,
            diagnostics: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackupPayload {
    pub version: u32,
    pub created_utc: String,
    pub splined_version: String,
    pub settings: Option<String>,
    pub interface_settings: Option<String>,
    #[serde(default)]
    pub credentials: BTreeMap<String, String>,
    pub database: Option<String>,
    pub diagnostics: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupEnvelope {
    format: String,
    version: u32,
    password_protected: bool,
    iterations: u32,
    salt: Option<String>,
    iv: Option<String>,
    payload: String,
    authentication: String,
}

pub fn export_backup(
    destination: &Path,
    layout: &AppLayout,
    config_text: &str,
    interface_text: &str,
    config: &Config,
    selection: BackupSelection,
    password: &str,
) -> Result<(), String> {
    let mut credentials = BTreeMap::new();
    if selection.credentials {
        let credential_dir = Path::new(&config.credentials.credential_dir);
        if credential_dir.is_dir() {
            for entry in fs::read_dir(credential_dir).map_err(|error| {
                format!(
                    "Unable to read credential directory {}: {error}",
                    credential_dir.display()
                )
            })? {
                let entry =
                    entry.map_err(|error| format!("Unable to inspect credential: {error}"))?;
                let path = entry.path();
                if path.is_file()
                    && path
                        .extension()
                        .and_then(|value| value.to_str())
                        .is_some_and(|value| value.eq_ignore_ascii_case("json"))
                {
                    let name = path
                        .file_name()
                        .and_then(|value| value.to_str())
                        .ok_or_else(|| "Credential filename is not valid UTF-8.".to_string())?;
                    credentials.insert(
                        name.to_string(),
                        STANDARD.encode(fs::read(&path).map_err(|error| {
                            format!("Unable to read credential {}: {error}", path.display())
                        })?),
                    );
                }
            }
        }
    }

    let database = if selection.database {
        let path = database_path(&config.scan.cache_dir);
        path.is_file()
            .then(|| fs::read(&path))
            .transpose()
            .map_err(|error| format!("Unable to read database {}: {error}", path.display()))?
            .map(|bytes| STANDARD.encode(bytes))
    } else {
        None
    };
    let payload = BackupPayload {
        version: BACKUP_VERSION,
        created_utc: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string(),
        splined_version: env!("CARGO_PKG_VERSION").to_string(),
        settings: selection.settings.then(|| config_text.to_string()),
        interface_settings: selection.interface.then(|| interface_text.to_string()),
        credentials,
        database,
        diagnostics: selection.diagnostics.then(|| {
            format!(
                "verbosity={:?};log_retention_days={}",
                config.verbosity, config.logging.retention_days
            )
        }),
    };
    let plain = serde_json::to_vec(&payload)
        .map_err(|error| format!("Unable to serialize backup payload: {error}"))?;
    let envelope = protect(&plain, password)?;
    let text = serde_json::to_string(&envelope)
        .map_err(|error| format!("Unable to serialize backup envelope: {error}"))?;
    let destination = ensure_spl_extension(destination);
    let _ = layout;
    replace_text_file(&destination, &text, "SPLINED backup", |candidate| {
        let value: BackupEnvelope = serde_json::from_str(candidate)
            .map_err(|error| format!("Unable to validate backup envelope: {error}"))?;
        if value.format != BACKUP_FORMAT || value.version != BACKUP_VERSION {
            return Err("Unable to validate backup envelope format.".to_string());
        }
        Ok(())
    })
}

pub fn read_backup(path: &Path, password: &str) -> Result<BackupPayload, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("Unable to read backup {}: {error}", path.display()))?;
    let envelope: BackupEnvelope = serde_json::from_str(&text)
        .map_err(|error| format!("This is not a supported SPLINED .spl backup: {error}"))?;
    if envelope.format != BACKUP_FORMAT || envelope.version != BACKUP_VERSION {
        return Err("This is not a supported SPLINED .spl backup.".to_string());
    }
    let plain = unprotect(&envelope, password)?;
    let payload: BackupPayload = serde_json::from_slice(&plain)
        .map_err(|error| format!("The SPLINED backup payload is invalid: {error}"))?;
    if payload.version != BACKUP_VERSION {
        return Err("The SPLINED backup payload is invalid.".to_string());
    }
    Ok(payload)
}

pub fn restore_backup(
    layout: &AppLayout,
    payload: &BackupPayload,
    selection: BackupSelection,
    current_config_text: &str,
) -> Result<(), String> {
    let destination_text = if selection.settings {
        payload.settings.as_deref().unwrap_or(current_config_text)
    } else {
        current_config_text
    };
    let destination_config = load_config_text_at_root(destination_text, &layout.root)?;

    if selection.settings
        && let Some(settings) = payload.settings.as_deref()
    {
        replace_text_file(
            &layout.config_file,
            settings,
            "restored Config v5",
            |candidate| load_config_text_at_root(candidate, &layout.root).map(|_| ()),
        )?;
    }
    if selection.interface
        && let Some(interface) = payload.interface_settings.as_deref()
    {
        parse_ui_state(interface)?;
        replace_text_file(
            &ui_path(layout),
            interface,
            "restored UI state",
            |candidate| parse_ui_state(candidate).map(|_| ()),
        )?;
    }
    if selection.credentials {
        restore_credentials(&destination_config, &payload.credentials)?;
    }
    if selection.database
        && let Some(database) = payload.database.as_deref()
    {
        let bytes = STANDARD
            .decode(database)
            .map_err(|error| format!("Backup database data is invalid: {error}"))?;
        let destination = database_path(&destination_config.scan.cache_dir);
        replace_binary_file(&destination, &bytes, "restored database", |staged| {
            let connection = rusqlite::Connection::open(staged)
                .map_err(|error| format!("Restored database is invalid: {error}"))?;
            let integrity: String = connection
                .query_row("PRAGMA quick_check", [], |row| row.get(0))
                .map_err(|error| format!("Restored database validation failed: {error}"))?;
            if integrity.eq_ignore_ascii_case("ok") {
                Ok(())
            } else {
                Err(format!("Restored database validation failed: {integrity}"))
            }
        })?;
    }
    Ok(())
}

fn restore_credentials(
    config: &Config,
    credentials: &BTreeMap<String, String>,
) -> Result<(), String> {
    let root = Path::new(&config.credentials.credential_dir);
    for (name, encoded) in credentials {
        if !safe_credential_name(name) {
            return Err("Backup contains an unsafe credential filename.".to_string());
        }
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|error| format!("Backup credential {name} is invalid: {error}"))?;
        let _: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Backup credential {name} is not valid JSON: {error}"))?;
        replace_binary_file(&root.join(name), &bytes, "restored credential", |staged| {
            let bytes = fs::read(staged)
                .map_err(|error| format!("Unable to validate restored credential: {error}"))?;
            serde_json::from_slice::<serde_json::Value>(&bytes)
                .map(|_| ())
                .map_err(|error| format!("Restored credential is invalid: {error}"))
        })?;
    }
    Ok(())
}

fn safe_credential_name(name: &str) -> bool {
    Path::new(name).file_name().and_then(|value| value.to_str()) == Some(name)
        && name.to_ascii_lowercase().ends_with(".json")
        && !name.is_empty()
}

fn ensure_spl_extension(path: &Path) -> PathBuf {
    if path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("spl"))
    {
        path.to_path_buf()
    } else {
        PathBuf::from(format!("{}.spl", path.display()))
    }
}

fn protect(plain: &[u8], password: &str) -> Result<BackupEnvelope, String> {
    if password.is_empty() {
        return Ok(BackupEnvelope {
            format: BACKUP_FORMAT.to_string(),
            version: BACKUP_VERSION,
            password_protected: false,
            iterations: 0,
            salt: None,
            iv: None,
            payload: STANDARD.encode(plain),
            authentication: STANDARD.encode(Sha256::digest(plain)),
        });
    }
    let mut salt = [0u8; 16];
    let mut iv = [0u8; 16];
    rand::rng().fill(&mut salt);
    rand::rng().fill(&mut iv);
    let mut key_material = [0u8; 64];
    pbkdf2_hmac::<Sha1>(
        password.as_bytes(),
        &salt,
        PASSWORD_ITERATIONS,
        &mut key_material,
    );
    let cipher = Aes256CbcEncryptor::new_from_slices(&key_material[..32], &iv)
        .map_err(|_| "Unable to initialize backup encryption.".to_string())?
        .encrypt_padded_vec_mut::<Pkcs7>(plain);
    let mut authenticated = iv.to_vec();
    authenticated.extend_from_slice(&cipher);
    let mut hmac = HmacSha256::new_from_slice(&key_material[32..])
        .map_err(|_| "Unable to initialize backup authentication.".to_string())?;
    hmac.update(&authenticated);
    let authentication = hmac.finalize().into_bytes();
    key_material.fill(0);
    Ok(BackupEnvelope {
        format: BACKUP_FORMAT.to_string(),
        version: BACKUP_VERSION,
        password_protected: true,
        iterations: PASSWORD_ITERATIONS,
        salt: Some(STANDARD.encode(salt)),
        iv: Some(STANDARD.encode(iv)),
        payload: STANDARD.encode(cipher),
        authentication: STANDARD.encode(authentication),
    })
}

fn unprotect(envelope: &BackupEnvelope, password: &str) -> Result<Vec<u8>, String> {
    let payload = STANDARD
        .decode(&envelope.payload)
        .map_err(|error| format!("Backup payload encoding is invalid: {error}"))?;
    let expected = STANDARD
        .decode(&envelope.authentication)
        .map_err(|error| format!("Backup authentication encoding is invalid: {error}"))?;
    if !envelope.password_protected {
        let actual = Sha256::digest(&payload);
        return (actual.as_slice() == expected.as_slice())
            .then_some(payload)
            .ok_or_else(|| "The SPLINED backup failed its integrity check.".to_string());
    }
    if password.is_empty() {
        return Err("This SPLINED backup requires a password.".to_string());
    }
    let salt = decode_required(&envelope.salt, "salt")?;
    let iv = decode_required(&envelope.iv, "initialization vector")?;
    if salt.len() != 16 || iv.len() != 16 || envelope.iterations == 0 {
        return Err("The SPLINED backup encryption metadata is invalid.".to_string());
    }
    let mut key_material = [0u8; 64];
    pbkdf2_hmac::<Sha1>(
        password.as_bytes(),
        &salt,
        envelope.iterations,
        &mut key_material,
    );
    let mut authenticated = iv.clone();
    authenticated.extend_from_slice(&payload);
    let mut hmac = HmacSha256::new_from_slice(&key_material[32..])
        .map_err(|_| "Unable to initialize backup authentication.".to_string())?;
    hmac.update(&authenticated);
    hmac.verify_slice(&expected)
        .map_err(|_| "The backup password is incorrect or the backup is damaged.".to_string())?;
    let result = Aes256CbcDecryptor::new_from_slices(&key_material[..32], &iv)
        .map_err(|_| "The SPLINED backup encryption metadata is invalid.".to_string())?
        .decrypt_padded_vec_mut::<Pkcs7>(&payload)
        .map_err(|_| "The backup password is incorrect or the backup is damaged.".to_string());
    key_material.fill(0);
    result
}

fn decode_required(value: &Option<String>, label: &str) -> Result<Vec<u8>, String> {
    STANDARD
        .decode(value.as_deref().unwrap_or(""))
        .map_err(|error| format!("Backup {label} encoding is invalid: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_toml;
    use crate::windows_state::{UiState, ui_state_to_text};
    use tempfile::TempDir;

    #[test]
    fn unprotected_and_password_protected_envelopes_round_trip() {
        let plain = br#"{"version":1}"#;
        for password in ["", "fixture-password"] {
            let envelope = protect(plain, password).unwrap();
            assert_eq!(unprotect(&envelope, password).unwrap(), plain);
        }
        let envelope = protect(plain, "right").unwrap();
        assert!(unprotect(&envelope, "wrong").is_err());
    }

    #[test]
    fn deterministic_selective_restore_is_independent_of_portable_path() {
        let first = TempDir::new().unwrap();
        let second = TempDir::new().unwrap();
        let source = TempDir::new().unwrap();
        let config_text = default_toml().unwrap().replace(
            "music_library = \"\"",
            r#"music_library = "\\\\server\\music""#,
        );
        let ui = UiState {
            theme: "Dark".into(),
            media_artist_filter: "Backup Artist".into(),
            ..UiState::default()
        };
        let payload = BackupPayload {
            version: 1,
            created_utc: "fixture".into(),
            splined_version: "fixture".into(),
            settings: Some(config_text.clone()),
            interface_settings: Some(ui_state_to_text(&ui).unwrap()),
            credentials: BTreeMap::new(),
            database: None,
            diagnostics: None,
        };
        let before = default_toml().unwrap();
        for root in [first.path(), second.path()] {
            let layout = AppLayout::from_root(root.to_path_buf());
            restore_backup(
                &layout,
                &payload,
                BackupSelection {
                    settings: true,
                    interface: true,
                    ..BackupSelection::default()
                },
                &before,
            )
            .unwrap();
            assert_eq!(
                fs::read_to_string(&layout.config_file).unwrap(),
                config_text
            );
            assert_eq!(
                parse_ui_state(&fs::read_to_string(ui_path(&layout)).unwrap()).unwrap(),
                ui
            );
            let resolved = load_config_text_at_root(
                &fs::read_to_string(&layout.config_file).unwrap(),
                &layout.root,
            )
            .unwrap();
            assert_eq!(resolved.library.music_library, r"\\server\music");
        }
        assert!(!source.path().join("unused").exists());
    }

    #[test]
    fn unselected_restore_categories_remain_unchanged() {
        let root = TempDir::new().unwrap();
        let layout = AppLayout::from_root(root.path().to_path_buf());
        let current = default_toml().unwrap();
        fs::create_dir_all(&layout.config_dir).unwrap();
        fs::write(&layout.config_file, &current).unwrap();
        let before_ui = ui_state_to_text(&UiState::default()).unwrap();
        fs::write(ui_path(&layout), &before_ui).unwrap();
        let payload = BackupPayload {
            version: 1,
            created_utc: "fixture".into(),
            splined_version: "fixture".into(),
            settings: Some(current.replace("mode = \"read\"", "mode = \"write\"")),
            interface_settings: Some(
                ui_state_to_text(&UiState {
                    theme: "Dark".into(),
                    ..UiState::default()
                })
                .unwrap(),
            ),
            credentials: BTreeMap::new(),
            database: None,
            diagnostics: None,
        };
        restore_backup(
            &layout,
            &payload,
            BackupSelection {
                settings: false,
                credentials: false,
                database: false,
                interface: false,
                diagnostics: false,
            },
            &current,
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&layout.config_file).unwrap(), current);
        assert_eq!(fs::read_to_string(ui_path(&layout)).unwrap(), before_ui);
    }
}
