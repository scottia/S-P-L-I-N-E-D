//! Portable SQLite storage for runtime state that was formerly written as JSON.

use crate::config::Config;
use rusqlite::{Connection, OptionalExtension, params};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const DB_NAME: &str = "splined.db";
const SCHEMA_VERSION: i64 = 2;
const SCHEMA: &str = include_str!("../python/splined_schema.sql");

pub fn database_path(config: &Config) -> PathBuf {
    Path::new(&config.scan.cache_dir).join(DB_NAME)
}

pub fn read_payload(config: &Config, cache_key: &str) -> Result<Option<String>, String> {
    let path = database_path(config);
    if !path.exists() {
        return Ok(None);
    }
    let connection = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| {
            format!(
                "Unable to open SPLINED runtime database {}: {error}",
                path.display()
            )
        })?;
    configure_timeout(&connection)?;
    let current: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(db_error("read SQLite schema version"))?;
    if current != SCHEMA_VERSION {
        return Err(incompatible_schema(current));
    }
    connection
        .query_row(
            "SELECT payload_json FROM cache_entries WHERE cache_key=?",
            [cache_key],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_error("read SQLite runtime state"))
}

pub fn write_payload(
    config: &Config,
    cache_key: &str,
    cache_type: &str,
    payload_json: &str,
) -> Result<(), String> {
    let path = database_path(config);
    let connection = open_writable(&path, config.scan.sqlite_shared)?;
    let now: String = connection
        .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ', 'now')", [], |row| {
            row.get(0)
        })
        .map_err(db_error("read SQLite time"))?;
    connection
        .execute(
            "INSERT INTO cache_entries(cache_key, cache_type, album_key, payload_json, splined_version, created_at, updated_at) \
             VALUES(?, ?, NULL, ?, ?, ?, ?) \
             ON CONFLICT(cache_key) DO UPDATE SET cache_type=excluded.cache_type, payload_json=excluded.payload_json, \
             splined_version=excluded.splined_version, updated_at=excluded.updated_at",
            params![cache_key, cache_type, payload_json, env!("CARGO_PKG_VERSION"), now, now],
        )
        .map_err(db_error("write SQLite runtime state"))?;
    Ok(())
}

fn open_writable(path: &Path, shared: bool) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Unable to create SPLINED cache directory {}: {error}",
                parent.display()
            )
        })?;
    }
    let connection = Connection::open(path).map_err(|error| {
        format!(
            "Unable to open SPLINED database {}: {error}",
            path.display()
        )
    })?;
    configure_timeout(&connection)?;
    let current: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(db_error("read SQLite schema version"))?;
    if current != 0 && current != SCHEMA_VERSION {
        return Err(incompatible_schema(current));
    }
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(db_error("enable SQLite foreign keys"))?;
    connection
        .pragma_update(None, "journal_mode", if shared { "delete" } else { "wal" })
        .map_err(db_error("configure SQLite journal mode"))?;
    connection
        .pragma_update(None, "synchronous", if shared { "FULL" } else { "NORMAL" })
        .map_err(db_error("configure SQLite synchronization"))?;
    if current == 0 {
        connection
            .execute_batch(SCHEMA)
            .map_err(db_error("apply shared schema"))?;
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(db_error("set SQLite schema version"))?;
    }
    Ok(connection)
}

fn configure_timeout(connection: &Connection) -> Result<(), String> {
    connection
        .busy_timeout(Duration::from_secs(30))
        .map_err(db_error("configure SQLite busy timeout"))
}

fn incompatible_schema(current: i64) -> String {
    format!(
        "SPLINED database schema v{current} is incompatible with required v{SCHEMA_VERSION}; the database was not changed."
    )
}

fn db_error(context: &'static str) -> impl FnOnce(rusqlite::Error) -> String {
    move |error| format!("Unable to {context}: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_state_round_trips_without_history_directory() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.scan.cache_dir = temp.path().join("cache").to_string_lossy().into_owned();

        write_payload(
            &config,
            "album-completion-state",
            "album_history",
            r#"{"version":1}"#,
        )
        .unwrap();

        assert_eq!(
            read_payload(&config, "album-completion-state").unwrap(),
            Some(r#"{"version":1}"#.to_string())
        );
        assert!(!temp.path().join("_history").exists());
    }
}
