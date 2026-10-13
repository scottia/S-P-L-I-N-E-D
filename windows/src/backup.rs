//! Cross-platform selective `.spl` backup/restore compatible with the former
//! Windows backup format.

use aes::Aes256;
use base64::Engine;
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha1_legacy::Sha1;
use sha2_legacy::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::safe_write::{replace_binary_file, replace_text_file};
use crate::settings::{config_text, save_config_text, ui_preferences_path};

const FORMAT: &str = "SPLINED-BACKUP";
const FORMAT_VERSION: u32 = 1;
const PASSWORD_ITERATIONS: u32 = 150_000;

type Aes256CbcEnc = cbc::Encryptor<Aes256>;
type Aes256CbcDec = cbc::Decryptor<Aes256>;
type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupPayload {
    version: u32,
    created_utc: String,
    splined_version: String,
    settings: Option<String>,
    interface_settings: Option<String>,
    #[serde(default)]
    credentials: BTreeMap<String, String>,
    database: Option<String>,
    diagnostics: Option<String>,
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
    requested_path: &Path,
    config: &Config,
    selection: BackupSelection,
    password: &str,
) -> Result<PathBuf, String> {
    let path = with_spl_extension(requested_path);
    let mut payload = BackupPayload {
        version: FORMAT_VERSION,
        created_utc: unix_timestamp().to_string(),
        splined_version: env!("CARGO_PKG_VERSION").to_string(),
        settings: selection.settings.then(config_text).transpose()?,
        interface_settings: None,
        credentials: BTreeMap::new(),
        database: None,
        diagnostics: selection.diagnostics.then(|| {
            format!(
                "verbosity={:?};log_retention_days={}",
                config.verbosity, config.logging.retention_days
            )
        }),
    };
    if selection.interface {
        let ui_path = ui_preferences_path()?;
        if ui_path.is_file() {
            payload.interface_settings =
                Some(std::fs::read_to_string(&ui_path).map_err(|error| {
                    format!(
                        "Unable to read interface settings {}: {error}",
                        ui_path.display()
                    )
                })?);
        }
    }
    if selection.credentials {
        let directory = Path::new(&config.credentials.credential_dir);
        if directory.is_dir() {
            for entry in std::fs::read_dir(directory).map_err(|error| {
                format!(
                    "Unable to read credential directory {}: {error}",
                    directory.display()
                )
            })? {
                let entry =
                    entry.map_err(|error| format!("Unable to read credential entry: {error}"))?;
                let path = entry.path();
                if path
                    .extension()
                    .is_some_and(|value| value.eq_ignore_ascii_case("json"))
                {
                    let name = path
                        .file_name()
                        .and_then(|value| value.to_str())
                        .ok_or_else(|| "Credential filename is not valid Unicode.".to_string())?;
                    payload.credentials.insert(
                        name.to_string(),
                        encode(&std::fs::read(&path).map_err(|error| {
                            format!("Unable to read credential {}: {error}", path.display())
                        })?),
                    );
                }
            }
        }
    }
    if selection.database {
        let database = Path::new(&config.scan.cache_dir).join("splined.db");
        if database.is_file() {
            payload.database = Some(encode(&std::fs::read(&database).map_err(|error| {
                format!("Unable to read database {}: {error}", database.display())
            })?));
        }
    }
    let plain = serde_json::to_vec(&payload)
        .map_err(|error| format!("Unable to serialize SPLINED backup: {error}"))?;
    let envelope = protect(&plain, password)?;
    let body = serde_json::to_string(&envelope)
        .map_err(|error| format!("Unable to serialize SPLINED backup envelope: {error}"))?;
    replace_text_file(&path, &body, "SPLINED backup", |candidate| {
        serde_json::from_str::<BackupEnvelope>(candidate)
            .map(|_| ())
            .map_err(|error| format!("Invalid staged SPLINED backup: {error}"))
    })?;
    Ok(path)
}

pub fn restore_backup(
    path: &Path,
    current: &Config,
    selection: BackupSelection,
    password: &str,
) -> Result<Config, String> {
    let body = std::fs::read_to_string(path)
        .map_err(|error| format!("Unable to read SPLINED backup {}: {error}", path.display()))?;
    let envelope: BackupEnvelope = serde_json::from_str(&body)
        .map_err(|error| format!("Invalid SPLINED backup envelope: {error}"))?;
    if envelope.format != FORMAT || envelope.version != FORMAT_VERSION {
        return Err("This is not a supported SPLINED .spl backup.".to_string());
    }
    let plain = unprotect(&envelope, password)?;
    let payload: BackupPayload = serde_json::from_slice(&plain)
        .map_err(|error| format!("Invalid SPLINED backup payload: {error}"))?;
    if payload.version != FORMAT_VERSION {
        return Err("The SPLINED backup payload version is unsupported.".to_string());
    }

    let mut destination = current.clone();
    if selection.settings
        && let Some(settings) = payload.settings.as_deref()
    {
        destination = save_config_text(settings)?;
    }
    if selection.interface
        && let Some(interface) = payload.interface_settings.as_deref()
    {
        let ui_path = ui_preferences_path()?;
        replace_text_file(
            &ui_path,
            interface,
            "SPLINED interface settings",
            |candidate| {
                toml::from_str::<crate::settings::UiPreferences>(candidate)
                    .map(|_| ())
                    .map_err(|error| format!("Invalid interface settings in backup: {error}"))
            },
        )?;
    }
    if selection.credentials {
        let directory = Path::new(&destination.credentials.credential_dir);
        for (name, encoded) in payload.credentials {
            validate_credential_filename(&name)?;
            let bytes = decode(&encoded, "credential")?;
            replace_binary_file(
                &directory.join(name),
                &bytes,
                "restored credential",
                |staged| {
                    let body = std::fs::read_to_string(staged)
                        .map_err(|error| format!("Restored credential is not UTF-8: {error}"))?;
                    serde_json::from_str::<Value>(&body)
                        .map(|_| ())
                        .map_err(|error| format!("Restored credential is invalid JSON: {error}"))
                },
            )?;
        }
    }
    if selection.database
        && let Some(encoded) = payload.database
    {
        let bytes = decode(&encoded, "database")?;
        let database = Path::new(&destination.scan.cache_dir).join("splined.db");
        replace_binary_file(&database, &bytes, "restored SPLINED database", |staged| {
            let bytes = std::fs::read(staged)
                .map_err(|error| format!("Unable to verify restored database: {error}"))?;
            if bytes.len() < 16 || &bytes[..16] != b"SQLite format 3\0" {
                return Err("The restored database is not a SQLite database.".to_string());
            }
            Ok(())
        })?;
    }
    Ok(destination)
}

fn protect(plain: &[u8], password: &str) -> Result<BackupEnvelope, String> {
    if password.is_empty() {
        return Ok(BackupEnvelope {
            format: FORMAT.to_string(),
            version: FORMAT_VERSION,
            password_protected: false,
            iterations: 0,
            salt: None,
            iv: None,
            payload: encode(plain),
            authentication: encode(&Sha256::digest(plain)),
        });
    }
    let mut salt = [0_u8; 16];
    let mut iv = [0_u8; 16];
    rand::rng().fill_bytes(&mut salt);
    rand::rng().fill_bytes(&mut iv);
    let mut key_material = [0_u8; 64];
    pbkdf2_hmac::<Sha1>(
        password.as_bytes(),
        &salt,
        PASSWORD_ITERATIONS,
        &mut key_material,
    );
    let cipher = Aes256CbcEnc::new_from_slices(&key_material[..32], &iv)
        .map_err(|_| "Unable to initialize SPLINED backup encryption.".to_string())?
        .encrypt_padded_vec_mut::<Pkcs7>(plain);
    let mut authenticated = Vec::with_capacity(iv.len() + cipher.len());
    authenticated.extend_from_slice(&iv);
    authenticated.extend_from_slice(&cipher);
    let authentication = hmac(&key_material[32..], &authenticated)?;
    key_material.fill(0);
    Ok(BackupEnvelope {
        format: FORMAT.to_string(),
        version: FORMAT_VERSION,
        password_protected: true,
        iterations: PASSWORD_ITERATIONS,
        salt: Some(encode(&salt)),
        iv: Some(encode(&iv)),
        payload: encode(&cipher),
        authentication: encode(&authentication),
    })
}

fn unprotect(envelope: &BackupEnvelope, password: &str) -> Result<Vec<u8>, String> {
    let payload = decode(&envelope.payload, "payload")?;
    let expected = decode(&envelope.authentication, "authentication")?;
    if !envelope.password_protected {
        let actual = Sha256::digest(&payload);
        if actual.as_slice() != expected.as_slice() {
            return Err("The SPLINED backup failed its integrity check.".to_string());
        }
        return Ok(payload);
    }
    if password.is_empty() {
        return Err("This SPLINED backup requires a password.".to_string());
    }
    let salt = decode(envelope.salt.as_deref().unwrap_or_default(), "salt")?;
    let iv = decode(envelope.iv.as_deref().unwrap_or_default(), "IV")?;
    if salt.len() != 16 || iv.len() != 16 || envelope.iterations == 0 {
        return Err("The SPLINED backup encryption metadata is invalid.".to_string());
    }
    let mut key_material = [0_u8; 64];
    pbkdf2_hmac::<Sha1>(
        password.as_bytes(),
        &salt,
        envelope.iterations,
        &mut key_material,
    );
    let mut authenticated = Vec::with_capacity(iv.len() + payload.len());
    authenticated.extend_from_slice(&iv);
    authenticated.extend_from_slice(&payload);
    let actual = hmac(&key_material[32..], &authenticated)?;
    if actual.as_slice() != expected.as_slice() {
        key_material.fill(0);
        return Err("The backup password is incorrect or the backup is damaged.".to_string());
    }
    let result = Aes256CbcDec::new_from_slices(&key_material[..32], &iv)
        .map_err(|_| "Unable to initialize SPLINED backup decryption.".to_string())?
        .decrypt_padded_vec_mut::<Pkcs7>(&payload)
        .map_err(|_| "The backup password is incorrect or the backup is damaged.".to_string());
    key_material.fill(0);
    result
}

fn hmac(key: &[u8], body: &[u8]) -> Result<Vec<u8>, String> {
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|_| "Unable to initialize SPLINED backup authentication.".to_string())?;
    mac.update(body);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn decode(value: &str, label: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|error| format!("Invalid SPLINED backup {label}: {error}"))
}

fn with_spl_extension(path: &Path) -> PathBuf {
    if path
        .extension()
        .is_some_and(|value| value.eq_ignore_ascii_case("spl"))
    {
        path.to_path_buf()
    } else {
        PathBuf::from(format!("{}.spl", path.display()))
    }
}

fn validate_credential_filename(name: &str) -> Result<(), String> {
    let path = Path::new(name);
    if path.file_name().and_then(|value| value.to_str()) != Some(name)
        || !path
            .extension()
            .is_some_and(|value| value.eq_ignore_ascii_case("json"))
    {
        return Err("Backup contains an unsafe credential filename.".to_string());
    }
    Ok(())
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

use serde_json::Value;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_and_plain_envelopes_round_trip() {
        for password in ["", "correct horse battery staple"] {
            let envelope = protect(b"SPLINED fixture", password).unwrap();
            assert_eq!(unprotect(&envelope, password).unwrap(), b"SPLINED fixture");
        }
    }

    #[test]
    fn wrong_password_fails_authentication_before_decryption() {
        let envelope = protect(b"private", "right").unwrap();
        assert!(
            unprotect(&envelope, "wrong")
                .unwrap_err()
                .contains("incorrect")
        );
    }

    #[test]
    fn credential_paths_must_be_plain_json_filenames() {
        assert!(validate_credential_filename("navidrome.json").is_ok());
        assert!(validate_credential_filename("../navidrome.json").is_err());
        assert!(validate_credential_filename("config.toml").is_err());
    }
}
