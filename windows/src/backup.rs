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
use crate::media_database::claim_shared_database;
use crate::safe_write::{replace_binary_file, replace_file_from, replace_text_file};
use crate::settings::{config_text, save_config_text, ui_preferences_path};
use rusqlite::{Connection, MAIN_DB, OpenFlags};
use serde_json::json;

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

#[derive(Debug)]
pub struct RestoredBackup {
    pub config: Config,
    pub interface_restored: bool,
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
        let interface = std::fs::read_to_string(&ui_path).map_err(|error| {
            format!(
                "Interface Settings was selected, but {} could not be read: {error}",
                ui_path.display()
            )
        })?;
        toml::from_str::<crate::settings::UiPreferences>(&interface)
            .map_err(|error| format!("Interface Settings is invalid: {error}"))?;
        payload.interface_settings = Some(interface);
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
        let _shared_lock = (config.scan.sqlite_shared && config.scan.windows_client_lock)
            .then(|| claim_shared_database(&database))
            .transpose()?;
        if database.is_file() {
            let (_temporary, snapshot_path) = create_sqlite_snapshot(&database)?;
            payload.database = Some(encode(&std::fs::read(&snapshot_path).map_err(|error| {
                format!(
                    "Unable to read SQLite snapshot {}: {error}",
                    snapshot_path.display()
                )
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

/// Create a consistent, standalone SQLite snapshot. SQLite's online backup API
/// is used instead of copying the live database bytes while another runtime may
/// be updating them.
pub fn export_database_backup(requested_path: &Path, config: &Config) -> Result<PathBuf, String> {
    let path = with_extension(requested_path, "sqlite");
    let source_path = Path::new(&config.scan.cache_dir).join("splined.db");
    let _shared_lock = (config.scan.sqlite_shared && config.scan.windows_client_lock)
        .then(|| claim_shared_database(&source_path))
        .transpose()?;
    if !source_path.is_file() {
        return Err(format!(
            "The configured SPLINED database does not exist: {}",
            source_path.display()
        ));
    }
    let (_temporary, snapshot_path) = create_sqlite_snapshot(&source_path)?;
    replace_file_from(
        &path,
        &snapshot_path,
        "SQLite backup",
        validate_sqlite_backup,
    )?;
    Ok(path)
}

fn create_sqlite_snapshot(source_path: &Path) -> Result<(tempfile::TempDir, PathBuf), String> {
    let temporary = tempfile::TempDir::new()
        .map_err(|error| format!("Unable to create SQLite backup staging directory: {error}"))?;
    let snapshot_path = temporary.path().join("splined.sqlite");
    let source = Connection::open_with_flags(source_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| {
            format!(
                "Unable to open SPLINED database {} for backup: {error}",
                source_path.display()
            )
        })?;
    source
        .backup(MAIN_DB, &snapshot_path, None)
        .map_err(|error| format!("Unable to create the SQLite snapshot: {error}"))?;
    validate_sqlite_backup(&snapshot_path)?;
    Ok((temporary, snapshot_path))
}

/// Restore a standalone SQLite snapshot to the configured database location.
pub fn restore_database_backup(path: &Path, config: &Config) -> Result<PathBuf, String> {
    validate_sqlite_backup(path)?;
    let destination = Path::new(&config.scan.cache_dir).join("splined.db");
    let _shared_lock = (config.scan.sqlite_shared && config.scan.windows_client_lock)
        .then(|| claim_shared_database(&destination))
        .transpose()?;
    restore_sqlite_snapshot(path, &destination)?;
    Ok(destination)
}

fn restore_sqlite_snapshot(source_path: &Path, destination: &Path) -> Result<(), String> {
    validate_sqlite_backup(source_path)?;
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Unable to prepare database directory {}: {error}",
                parent.display()
            )
        })?;
    }
    let source = Connection::open_with_flags(source_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| format!("Unable to open SQLite backup for restore: {error}"))?;
    source
        .backup(MAIN_DB, destination, None)
        .map_err(|error| format!("Unable to restore the SQLite snapshot: {error}"))?;
    validate_sqlite_backup(destination)
}

/// Write a human-readable, secret-free diagnostic summary as its own JSON
/// artifact. It is reference material, not a restorable settings archive.
pub fn export_diagnostic_summary(
    requested_path: &Path,
    config: &Config,
) -> Result<PathBuf, String> {
    let path = with_extension(requested_path, "json");
    let database = Path::new(&config.scan.cache_dir).join("splined.db");
    let database_metadata = std::fs::metadata(&database).ok();
    let database_check = if database.is_file() {
        validate_sqlite_backup(&database)
            .map(|_| "ok".to_string())
            .unwrap_or_else(|error| error)
    } else {
        "missing".to_string()
    };
    let summary = json!({
        "format": "SPLINED-DIAGNOSTIC-SUMMARY",
        "version": 1,
        "created_utc": unix_timestamp().to_string(),
        "splined_version": env!("CARGO_PKG_VERSION"),
        "config_version": config.config_version,
        "runtime": {
            "mode": format!("{:?}", config.mode),
            "verbosity": format!("{:?}", config.verbosity),
            "scan_mode": config.scan.scan_mode,
            "library_scan": config.scan.library_scan,
            "sqlite_shared": config.scan.sqlite_shared,
            "windows_client_lock": config.scan.windows_client_lock,
        },
        "paths": {
            "music_library": config.library.music_library,
            "scan_library": config.scan.scan_library_dir,
            "database": database,
            "temporary_cache": config.scan.temporary_cache_dir,
            "log_directory": config.scan.log_dir,
        },
        "database": {
            "exists": database.is_file(),
            "size_bytes": database_metadata.as_ref().map(std::fs::Metadata::len),
            "quick_check": database_check,
        },
        "retention": {
            "log_days": config.logging.retention_days,
            "history_enabled": config.history.enabled,
            "history_days": config.history.retention_days,
        },
        "sources": {
            "priority": config.sources.cover_sources,
            "excluded": config.sources.exclude_cover_sources,
        },
    });
    let body = serde_json::to_string_pretty(&summary)
        .map_err(|error| format!("Unable to serialize diagnostic summary: {error}"))?;
    replace_text_file(&path, &body, "diagnostic summary", |candidate| {
        let parsed: serde_json::Value = serde_json::from_str(candidate)
            .map_err(|error| format!("Invalid diagnostic summary JSON: {error}"))?;
        if parsed.get("format").and_then(serde_json::Value::as_str)
            != Some("SPLINED-DIAGNOSTIC-SUMMARY")
        {
            return Err("Diagnostic summary format marker is missing.".to_string());
        }
        Ok(())
    })?;
    Ok(path)
}

pub fn restore_backup(
    path: &Path,
    current: &Config,
    selection: BackupSelection,
    password: &str,
) -> Result<RestoredBackup, String> {
    let payload = read_backup_payload(path, password)?;

    if selection.interface && payload.interface_settings.is_none() {
        return Err("This archive does not contain Interface Settings. Deselect that restore option or choose a newer archive.".to_string());
    }
    if selection.settings && payload.settings.is_none() {
        return Err("This archive does not contain Config V5 Settings. Deselect that restore option or choose another archive.".to_string());
    }
    if selection.interface
        && let Some(interface) = payload.interface_settings.as_deref()
    {
        toml::from_str::<crate::settings::UiPreferences>(interface)
            .map_err(|error| format!("Invalid interface settings in backup: {error}"))?;
    }
    let mut destination = current.clone();
    if selection.settings
        && let Some(settings) = payload.settings.as_deref()
    {
        destination = save_config_text(settings)?;
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
        let _shared_lock = (destination.scan.sqlite_shared && destination.scan.windows_client_lock)
            .then(|| claim_shared_database(&database))
            .transpose()?;
        let temporary = tempfile::TempDir::new().map_err(|error| {
            format!("Unable to create database restore staging directory: {error}")
        })?;
        let staged = temporary.path().join("splined.sqlite");
        std::fs::write(&staged, bytes)
            .map_err(|error| format!("Unable to stage the database restore: {error}"))?;
        restore_sqlite_snapshot(&staged, &database)?;
    }
    // Install interface preferences last so a failed restore cannot leave the
    // running UI racing to overwrite newly restored preferences.
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
    Ok(RestoredBackup {
        config: destination,
        interface_restored: selection.interface && payload.interface_settings.is_some(),
    })
}

/// Inspect a Settings Archive without writing Config v5 or touching the database.
/// Setup uses this so a moved portable installation can correct its paths first.
pub fn read_backup_settings(path: &Path, password: &str) -> Result<Config, String> {
    let payload = read_backup_payload(path, password)?;
    let settings = payload.settings.as_deref().ok_or_else(|| {
        "This archive does not contain Config V5 Settings. Choose a Settings Archive.".to_string()
    })?;
    crate::config::parse_config(settings)
}

fn read_backup_payload(path: &Path, password: &str) -> Result<BackupPayload, String> {
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
    Ok(payload)
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

fn with_extension(path: &Path, extension: &str) -> PathBuf {
    if path
        .extension()
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
    {
        path.to_path_buf()
    } else {
        path.with_extension(extension)
    }
}

fn validate_sqlite_backup(path: &Path) -> Result<(), String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| format!("Unable to open SQLite backup {}: {error}", path.display()))?;
    let result = connection
        .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
        .map_err(|error| format!("Unable to verify SQLite backup {}: {error}", path.display()))?;
    if result.eq_ignore_ascii_case("ok") {
        Ok(())
    } else {
        Err(format!(
            "SQLite backup {} failed quick_check: {result}",
            path.display()
        ))
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
    use tempfile::TempDir;

    #[test]
    fn protected_and_plain_envelopes_round_trip() {
        for password in ["", "correct horse battery staple"] {
            let envelope = protect(b"SPLINED fixture", password).unwrap();
            assert_eq!(unprotect(&envelope, password).unwrap(), b"SPLINED fixture");
        }
    }

    #[test]
    fn setup_reads_settings_archive_without_restoring_files() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("settings.spl");
        let mut config = Config::default();
        config.scan.sqlite_shared = true;
        config.scan.windows_client_lock = false;
        let payload = BackupPayload {
            version: FORMAT_VERSION,
            created_utc: "fixture".to_string(),
            splined_version: "fixture".to_string(),
            settings: Some(toml::to_string(&config).unwrap()),
            interface_settings: None,
            credentials: BTreeMap::new(),
            database: None,
            diagnostics: None,
        };
        let envelope = protect(&serde_json::to_vec(&payload).unwrap(), "test password").unwrap();
        std::fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();
        assert!(
            read_backup_settings(&path, "")
                .unwrap_err()
                .contains("requires a password")
        );
        let restored = read_backup_settings(&path, "test password").unwrap();
        assert!(restored.scan.sqlite_shared);
        assert!(!restored.scan.windows_client_lock);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn settings_archive_payload_preserves_custom_interface_values() {
        let mut preferences = crate::settings::UiPreferences {
            appearance: "Chalk".to_string(),
            media_filter_locked: true,
            candidate_filter_locked: true,
            candidate_excluded_types: vec!["BackCover".to_string()],
            media_pane_width: 392,
            artwork_pane_width: 528,
            ..crate::settings::UiPreferences::default()
        };
        preferences.adjustment_presets[1].name = "Warm".to_string();
        preferences.adjustment_presets[1].values = [3, 2, 1, -1, 4, 5, 0, 8];
        preferences.adjustment_presets[1].saved = true;
        preferences.banner_theme_baseline_styles = preferences.banner_theme_styles.clone();
        preferences.album_frame_theme_baseline_styles =
            preferences.album_frame_theme_styles.clone();
        preferences.banner_theme_styles[0].background = "#123ABC".to_string();
        preferences.banner_sync_frame_letters = true;
        preferences.banner_random_harmony = true;
        preferences.wheel_hue_shift_degrees = 72;
        preferences.wheel_brightness_percent = 125;
        preferences.album_frame_theme_styles[0].color = "#20BFA0".to_string();
        let payload = BackupPayload {
            version: FORMAT_VERSION,
            created_utc: "fixture".to_string(),
            splined_version: "fixture".to_string(),
            settings: Some("config_version = 5".to_string()),
            interface_settings: Some(toml::to_string(&preferences).unwrap()),
            credentials: BTreeMap::new(),
            database: None,
            diagnostics: None,
        };
        let archive = protect(&serde_json::to_vec(&payload).unwrap(), "").unwrap();
        let decoded: BackupPayload =
            serde_json::from_slice(&unprotect(&archive, "").unwrap()).unwrap();
        let restored: crate::settings::UiPreferences =
            toml::from_str(decoded.interface_settings.as_deref().unwrap()).unwrap();
        assert_eq!(restored, preferences);
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

    #[test]
    fn separate_backup_extensions_are_stable() {
        assert_eq!(
            with_extension(Path::new("splined-database"), "sqlite"),
            PathBuf::from("splined-database.sqlite")
        );
        assert_eq!(
            with_extension(Path::new("splined-diagnostics.json"), "json"),
            PathBuf::from("splined-diagnostics.json")
        );
    }

    #[test]
    fn sqlite_snapshot_and_diagnostic_summary_are_independent_files() {
        let root = TempDir::new().unwrap();
        let cache = root.path().join("cache");
        std::fs::create_dir(&cache).unwrap();
        let database = cache.join("splined.db");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE fixture(value TEXT); INSERT INTO fixture VALUES ('before');",
            )
            .unwrap();
        drop(connection);

        let mut config = Config::default();
        config.scan.cache_dir = cache.to_string_lossy().into_owned();
        let backup = export_database_backup(&root.path().join("library"), &config).unwrap();
        assert_eq!(
            backup.extension().and_then(|value| value.to_str()),
            Some("sqlite")
        );

        let connection = Connection::open(&database).unwrap();
        connection
            .execute("UPDATE fixture SET value = 'after'", [])
            .unwrap();
        drop(connection);
        restore_database_backup(&backup, &config).unwrap();
        let restored = Connection::open(&database)
            .unwrap()
            .query_row("SELECT value FROM fixture", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap();
        assert_eq!(restored, "before");

        let diagnostics =
            export_diagnostic_summary(&root.path().join("diagnostics"), &config).unwrap();
        assert_eq!(
            diagnostics.extension().and_then(|value| value.to_str()),
            Some("json")
        );
        let summary: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(diagnostics).unwrap()).unwrap();
        assert_eq!(summary["format"], "SPLINED-DIAGNOSTIC-SUMMARY");
        assert_eq!(summary["database"]["quick_check"], "ok");
    }

    #[test]
    fn spl_archive_includes_committed_wal_rows() {
        let root = TempDir::new().unwrap();
        let cache = root.path().join("cache");
        std::fs::create_dir(&cache).unwrap();
        let database = cache.join("splined.db");
        let writer = Connection::open(&database).unwrap();
        writer
            .execute_batch(
                "PRAGMA journal_mode=WAL; CREATE TABLE fixture(value TEXT); INSERT INTO fixture VALUES ('from-wal');",
            )
            .unwrap();
        assert!(database.with_extension("db-wal").is_file());

        let mut config = Config::default();
        config.scan.cache_dir = cache.to_string_lossy().into_owned();
        let selection = BackupSelection {
            settings: false,
            credentials: false,
            database: true,
            interface: false,
            diagnostics: false,
        };
        let archive = export_backup(&root.path().join("library"), &config, selection, "").unwrap();
        let envelope: BackupEnvelope =
            serde_json::from_slice(&std::fs::read(&archive).unwrap()).unwrap();
        let payload: BackupPayload =
            serde_json::from_slice(&unprotect(&envelope, "").unwrap()).unwrap();
        let snapshot = root.path().join("extracted.sqlite");
        std::fs::write(
            &snapshot,
            decode(payload.database.as_deref().unwrap(), "database").unwrap(),
        )
        .unwrap();
        let value: String = Connection::open(&snapshot)
            .unwrap()
            .query_row("SELECT value FROM fixture", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, "from-wal");

        writer
            .execute("UPDATE fixture SET value = 'changed-after-backup'", [])
            .unwrap();
        restore_backup(&archive, &config, selection, "").unwrap();
        let restored: String = writer
            .query_row("SELECT value FROM fixture", [], |row| row.get(0))
            .unwrap();
        assert_eq!(restored, "from-wal");
    }
}
