//! Durable, one-way Windows SQLite -> WebGUI PostgreSQL catalog transport.
//!
//! SQLite triggers capture entity identity and a monotonic local revision in
//! the same transaction as every Artist, Album, or track mutation.  The
//! publisher later materializes a complete row projection and retains the
//! outbox until PostgreSQL has durably accepted it.  PostgreSQL remains unable
//! to write this stream back into SQLite.

use base64::Engine;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params, types::ValueRef};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

pub const PROTOCOL_VERSION: i64 = 1;
const TRIGGER_SCHEMA_VERSION: i64 = 2;
pub const PUBLISH_BATCH_LIMIT: usize = 100;

const SQLITE_SOURCE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS catalog_sync_source (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    source_instance_id TEXT NOT NULL UNIQUE,
    protocol_version INTEGER NOT NULL DEFAULT 1,
    next_revision INTEGER NOT NULL DEFAULT 0,
    capture_enabled INTEGER NOT NULL DEFAULT 0,
    effective_mode TEXT NOT NULL DEFAULT 'manual-snap',
    baseline_generation INTEGER NOT NULL DEFAULT 1,
    change_revision INTEGER NOT NULL DEFAULT 0,
    trigger_schema_version INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
"#;

const SQLITE_CORE_SCHEMA: &str = r#"
INSERT OR IGNORE INTO catalog_sync_source(
    singleton,source_instance_id,protocol_version,next_revision,created_at,updated_at
) VALUES(
    1,'windows-' || lower(hex(randomblob(16))),1,0,
    strftime('%Y-%m-%dT%H:%M:%SZ','now'),strftime('%Y-%m-%dT%H:%M:%SZ','now')
);

CREATE TABLE IF NOT EXISTS catalog_sync_outbox (
    local_revision INTEGER PRIMARY KEY,
    event_id TEXT NOT NULL UNIQUE,
    entity_table TEXT NOT NULL CHECK(entity_table IN ('artists','albums','tracks')),
    entity_key TEXT NOT NULL,
    operation TEXT NOT NULL CHECK(operation IN ('upsert','delete')),
    observed_artwork_revision INTEGER NOT NULL DEFAULT 0,
    payload_json TEXT,
    payload_digest TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_catalog_sync_outbox_entity
    ON catalog_sync_outbox(entity_table,entity_key,local_revision);
"#;

const SQLITE_TRIGGER_SCHEMA: &str = r#"
DROP TRIGGER IF EXISTS catalog_sync_artists_insert;
DROP TRIGGER IF EXISTS catalog_sync_artists_update;
DROP TRIGGER IF EXISTS catalog_sync_artists_delete;
DROP TRIGGER IF EXISTS catalog_sync_albums_insert;
DROP TRIGGER IF EXISTS catalog_sync_albums_update;
DROP TRIGGER IF EXISTS catalog_sync_albums_delete;
DROP TRIGGER IF EXISTS catalog_sync_tracks_insert;
DROP TRIGGER IF EXISTS catalog_sync_tracks_update;
DROP TRIGGER IF EXISTS catalog_sync_tracks_delete;

CREATE TRIGGER IF NOT EXISTS catalog_sync_artists_insert
AFTER INSERT ON artists
BEGIN
    UPDATE catalog_sync_source SET change_revision=change_revision+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1;
    UPDATE catalog_sync_source SET next_revision=next_revision+1 WHERE singleton=1 AND capture_enabled=1;
    INSERT INTO catalog_sync_outbox(local_revision,event_id,entity_table,entity_key,operation,observed_artwork_revision,created_at)
        SELECT next_revision,lower(hex(randomblob(16))),'artists',new.artist_key,'upsert',0,strftime('%Y-%m-%dT%H:%M:%SZ','now') FROM catalog_sync_source WHERE singleton=1 AND capture_enabled=1;
END;
CREATE TRIGGER IF NOT EXISTS catalog_sync_artists_update
AFTER UPDATE ON artists
BEGIN
    UPDATE catalog_sync_source SET change_revision=change_revision+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1;
    UPDATE catalog_sync_source SET next_revision=next_revision+1 WHERE singleton=1 AND capture_enabled=1;
    INSERT INTO catalog_sync_outbox(local_revision,event_id,entity_table,entity_key,operation,observed_artwork_revision,created_at)
        SELECT next_revision,lower(hex(randomblob(16))),'artists',new.artist_key,'upsert',0,strftime('%Y-%m-%dT%H:%M:%SZ','now') FROM catalog_sync_source WHERE singleton=1 AND capture_enabled=1;
END;
CREATE TRIGGER IF NOT EXISTS catalog_sync_artists_delete
AFTER DELETE ON artists
BEGIN
    UPDATE catalog_sync_source SET change_revision=change_revision+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1;
    UPDATE catalog_sync_source SET next_revision=next_revision+1 WHERE singleton=1 AND capture_enabled=1;
    INSERT INTO catalog_sync_outbox(local_revision,event_id,entity_table,entity_key,operation,observed_artwork_revision,created_at)
        SELECT next_revision,lower(hex(randomblob(16))),'artists',old.artist_key,'delete',0,strftime('%Y-%m-%dT%H:%M:%SZ','now') FROM catalog_sync_source WHERE singleton=1 AND capture_enabled=1;
END;

CREATE TRIGGER IF NOT EXISTS catalog_sync_albums_insert
AFTER INSERT ON albums
BEGIN
    UPDATE catalog_sync_source SET change_revision=change_revision+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1;
    UPDATE catalog_sync_source SET next_revision=next_revision+1 WHERE singleton=1 AND capture_enabled=1;
    INSERT INTO catalog_sync_outbox(local_revision,event_id,entity_table,entity_key,operation,observed_artwork_revision,created_at)
        SELECT next_revision,lower(hex(randomblob(16))),'albums',new.album_key,'upsert',coalesce((SELECT CASE WHEN physically_verified=1 THEN verified_revision ELSE 0 END FROM album_artwork_write_state WHERE album_key=new.album_key),0),strftime('%Y-%m-%dT%H:%M:%SZ','now') FROM catalog_sync_source WHERE singleton=1 AND capture_enabled=1;
END;
CREATE TRIGGER IF NOT EXISTS catalog_sync_albums_update
AFTER UPDATE ON albums
BEGIN
    UPDATE catalog_sync_source SET change_revision=change_revision+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1;
    UPDATE catalog_sync_source SET next_revision=next_revision+1 WHERE singleton=1 AND capture_enabled=1;
    INSERT INTO catalog_sync_outbox(local_revision,event_id,entity_table,entity_key,operation,observed_artwork_revision,created_at)
        SELECT next_revision,lower(hex(randomblob(16))),'albums',new.album_key,'upsert',coalesce((SELECT CASE WHEN physically_verified=1 THEN verified_revision ELSE 0 END FROM album_artwork_write_state WHERE album_key=new.album_key),0),strftime('%Y-%m-%dT%H:%M:%SZ','now') FROM catalog_sync_source WHERE singleton=1 AND capture_enabled=1;
END;
CREATE TRIGGER IF NOT EXISTS catalog_sync_albums_delete
AFTER DELETE ON albums
BEGIN
    UPDATE catalog_sync_source SET change_revision=change_revision+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1;
    UPDATE catalog_sync_source SET next_revision=next_revision+1 WHERE singleton=1 AND capture_enabled=1;
    INSERT INTO catalog_sync_outbox(local_revision,event_id,entity_table,entity_key,operation,observed_artwork_revision,created_at)
        SELECT next_revision,lower(hex(randomblob(16))),'albums',old.album_key,'delete',0,strftime('%Y-%m-%dT%H:%M:%SZ','now') FROM catalog_sync_source WHERE singleton=1 AND capture_enabled=1;
END;

CREATE TRIGGER IF NOT EXISTS catalog_sync_tracks_insert
AFTER INSERT ON tracks
BEGIN
    UPDATE catalog_sync_source SET change_revision=change_revision+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1;
    UPDATE catalog_sync_source SET next_revision=next_revision+1 WHERE singleton=1 AND capture_enabled=1;
    INSERT INTO catalog_sync_outbox(local_revision,event_id,entity_table,entity_key,operation,observed_artwork_revision,created_at)
        SELECT next_revision,lower(hex(randomblob(16))),'tracks',new.track_key,'upsert',0,strftime('%Y-%m-%dT%H:%M:%SZ','now') FROM catalog_sync_source WHERE singleton=1 AND capture_enabled=1;
END;
CREATE TRIGGER IF NOT EXISTS catalog_sync_tracks_update
AFTER UPDATE ON tracks
BEGIN
    UPDATE catalog_sync_source SET change_revision=change_revision+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1;
    UPDATE catalog_sync_source SET next_revision=next_revision+1 WHERE singleton=1 AND capture_enabled=1;
    INSERT INTO catalog_sync_outbox(local_revision,event_id,entity_table,entity_key,operation,observed_artwork_revision,created_at)
        SELECT next_revision,lower(hex(randomblob(16))),'tracks',new.track_key,'upsert',0,strftime('%Y-%m-%dT%H:%M:%SZ','now') FROM catalog_sync_source WHERE singleton=1 AND capture_enabled=1;
END;
CREATE TRIGGER IF NOT EXISTS catalog_sync_tracks_delete
AFTER DELETE ON tracks
BEGIN
    UPDATE catalog_sync_source SET change_revision=change_revision+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1;
    UPDATE catalog_sync_source SET next_revision=next_revision+1 WHERE singleton=1 AND capture_enabled=1;
    INSERT INTO catalog_sync_outbox(local_revision,event_id,entity_table,entity_key,operation,observed_artwork_revision,created_at)
        SELECT next_revision,lower(hex(randomblob(16))),'tracks',old.track_key,'delete',0,strftime('%Y-%m-%dT%H:%M:%SZ','now') FROM catalog_sync_source WHERE singleton=1 AND capture_enabled=1;
END;
"#;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSource {
    pub source_instance_id: String,
    pub next_revision: i64,
    pub capture_enabled: bool,
    pub effective_mode: String,
    pub baseline_generation: i64,
    pub change_revision: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CatalogEvent {
    local_revision: i64,
    event_id: String,
    entity_table: String,
    entity_key: String,
    operation: String,
    observed_artwork_revision: i64,
    created_at: String,
    payload_json: String,
    payload_digest: String,
}

pub fn ensure_sqlite_schema(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch("BEGIN IMMEDIATE")
        .map_err(|error| format!("Unable to lock live catalog schema: {error}"))?;
    let result = (|| {
        connection
            .execute_batch(SQLITE_SOURCE_SCHEMA)
            .map_err(|error| format!("Unable to prepare live catalog source: {error}"))?;
        let columns = table_columns(connection, "catalog_sync_source")?;
        for (name, definition) in [
            ("capture_enabled", "INTEGER NOT NULL DEFAULT 0"),
            ("effective_mode", "TEXT NOT NULL DEFAULT 'manual-snap'"),
            ("baseline_generation", "INTEGER NOT NULL DEFAULT 1"),
            ("change_revision", "INTEGER NOT NULL DEFAULT 0"),
            ("trigger_schema_version", "INTEGER NOT NULL DEFAULT 0"),
        ] {
            if !columns.iter().any(|column| column == name) {
                connection
                    .execute_batch(&format!(
                        "ALTER TABLE catalog_sync_source ADD COLUMN {name} {definition}"
                    ))
                    .map_err(|error| format!("Unable to add live catalog {name}: {error}"))?;
            }
        }
        connection
            .execute_batch(SQLITE_CORE_SCHEMA)
            .map_err(|error| format!("Unable to prepare live catalog outbox: {error}"))?;
        let outbox_columns = table_columns(connection, "catalog_sync_outbox")?;
        for (name, definition) in [("payload_json", "TEXT"), ("payload_digest", "TEXT")] {
            if !outbox_columns.iter().any(|column| column == name) {
                connection
                    .execute_batch(&format!(
                        "ALTER TABLE catalog_sync_outbox ADD COLUMN {name} {definition}"
                    ))
                    .map_err(|error| format!("Unable to add immutable {name}: {error}"))?;
            }
        }
        let installed: i64 = connection
            .query_row(
                "SELECT trigger_schema_version FROM catalog_sync_source WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .map_err(|error| format!("Unable to inspect live catalog triggers: {error}"))?;
        if installed < TRIGGER_SCHEMA_VERSION {
            connection
                .execute_batch(SQLITE_TRIGGER_SCHEMA)
                .map_err(|error| format!("Unable to install live catalog triggers: {error}"))?;
            connection
                .execute(
                    "UPDATE catalog_sync_source SET trigger_schema_version=? WHERE singleton=1",
                    [TRIGGER_SCHEMA_VERSION],
                )
                .map_err(|error| {
                    format!("Unable to record live catalog trigger version: {error}")
                })?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => connection
            .execute_batch("COMMIT")
            .map_err(|error| format!("Unable to commit live catalog schema: {error}")),
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

fn table_columns(connection: &Connection, table: &str) -> Result<Vec<String>, String> {
    connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|error| format!("Unable to inspect live catalog {table}: {error}"))
}

pub fn set_runtime_mode(database_path: &Path, mode: &str) -> Result<CatalogSource, String> {
    if !matches!(mode, "live" | "manual-snap" | "off") {
        return Err(format!("Unsupported live catalog mode: {mode}"));
    }
    let mut connection = Connection::open(database_path)
        .map_err(|error| format!("Unable to open live catalog capture state: {error}"))?;
    connection
        .busy_timeout(Duration::from_secs(10))
        .map_err(|error| format!("Unable to configure live catalog capture state: {error}"))?;
    ensure_sqlite_schema(&connection)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| format!("Unable to begin live catalog mode transition: {error}"))?;
    let previous: String = transaction
        .query_row(
            "SELECT effective_mode FROM catalog_sync_source WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("Unable to read live catalog effective mode: {error}"))?;
    if previous == "live" && mode != "live" {
        transaction
            .execute(
                "UPDATE catalog_sync_source SET capture_enabled=0,effective_mode=?,baseline_generation=baseline_generation+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1",
                [mode],
            )
            .map_err(|error| format!("Unable to leave Live Synchronization: {error}"))?;
    } else if mode != "live" {
        transaction
            .execute(
                "UPDATE catalog_sync_source SET capture_enabled=0,effective_mode=?,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1",
                [mode],
            )
            .map_err(|error| format!("Unable to set catalog synchronization mode: {error}"))?;
    } else if previous != "live" {
        transaction
            .execute(
                "UPDATE catalog_sync_source SET capture_enabled=0,effective_mode='live',updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1",
                [],
            )
            .map_err(|error| format!("Unable to request Live Synchronization: {error}"))?;
    }
    let state = read_source(&transaction)?;
    transaction
        .commit()
        .map_err(|error| format!("Unable to commit live catalog mode transition: {error}"))?;
    Ok(state)
}

fn read_source(connection: &Connection) -> Result<CatalogSource, String> {
    connection
        .query_row(
            "SELECT source_instance_id,next_revision,capture_enabled,effective_mode,baseline_generation,change_revision FROM catalog_sync_source WHERE singleton=1",
            [],
            |row| {
                Ok(CatalogSource {
                    source_instance_id: row.get(0)?,
                    next_revision: row.get(1)?,
                    capture_enabled: row.get::<_, i64>(2)? != 0,
                    effective_mode: row.get(3)?,
                    baseline_generation: row.get(4)?,
                    change_revision: row.get(5)?,
                })
            },
        )
        .map_err(|error| format!("Unable to read live catalog source identity: {error}"))
}

pub fn source(connection: &Connection) -> Result<CatalogSource, String> {
    ensure_sqlite_schema(connection)?;
    read_source(connection)
}

fn json_value(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(value) => Value::from(value),
        ValueRef::Real(value) => Value::from(value),
        ValueRef::Text(value) => Value::String(String::from_utf8_lossy(value).into_owned()),
        ValueRef::Blob(value) => {
            Value::String(base64::engine::general_purpose::STANDARD.encode(value))
        }
    }
}

fn materialize_row(
    connection: &Connection,
    table: &str,
    key: &str,
) -> Result<Option<String>, String> {
    let key_name = match table {
        "artists" => "artist_key",
        "albums" => "album_key",
        "tracks" => "track_key",
        _ => return Err(format!("Unsupported live catalog table: {table}")),
    };
    let mut statement = connection
        .prepare(&format!("SELECT * FROM \"{table}\" WHERE \"{key_name}\"=?"))
        .map_err(|error| format!("Unable to prepare live catalog projection: {error}"))?;
    let names = statement
        .column_names()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    statement
        .query_row([key], |row| {
            let mut ordered = BTreeMap::new();
            for (index, name) in names.iter().enumerate() {
                ordered.insert(name.clone(), json_value(row.get_ref(index)?));
            }
            let map = ordered.into_iter().collect::<Map<String, Value>>();
            Ok(serde_json::to_string(&Value::Object(map)).unwrap_or_else(|_| "{}".to_string()))
        })
        .optional()
        .map_err(|error| format!("Unable to materialize live catalog projection: {error}"))
}

fn read_pending_events(connection: &Connection, limit: usize) -> Result<Vec<CatalogEvent>, String> {
    let mut statement = connection
        .prepare(concat!(
            "SELECT local_revision,event_id,entity_table,entity_key,operation,",
            "observed_artwork_revision,created_at,payload_json,payload_digest ",
            "FROM catalog_sync_outbox ORDER BY local_revision LIMIT ?"
        ))
        .map_err(|error| format!("Unable to read live catalog outbox: {error}"))?;
    let envelopes = statement
        .query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
            ))
        })
        .map_err(|error| format!("Unable to query live catalog outbox: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to collect live catalog outbox: {error}"))?;
    drop(statement);
    envelopes
        .into_iter()
        .map(
            |(
                revision,
                event_id,
                table,
                key,
                operation,
                observed,
                created_at,
                stored_payload,
                stored_digest,
            )| {
                let (effective_operation, payload_json, payload_digest) = if let (
                    Some(payload),
                    Some(digest),
                ) = (stored_payload, stored_digest)
                {
                    (operation, payload, digest)
                } else {
                    // Materialize exactly once. If PostgreSQL commits but the
                    // SQLite acknowledgement is lost, a later catalog write
                    // cannot change the accepted event's payload or digest.
                    let materialized = if operation == "delete" {
                        None
                    } else {
                        materialize_row(connection, &table, &key)?
                    };
                    let effective = if materialized.is_some() {
                        operation
                    } else {
                        "delete".to_string()
                    };
                    let payload = materialized.unwrap_or_else(|| "{}".to_string());
                    let digest = Sha256::digest(payload.as_bytes())
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>();
                    connection
                        .execute(
                            "UPDATE catalog_sync_outbox SET operation=?,payload_json=?,payload_digest=? WHERE local_revision=? AND event_id=? AND payload_json IS NULL",
                            params![effective, payload, digest, revision, event_id],
                        )
                        .map_err(|error| {
                            format!("Unable to freeze live catalog revision {revision}: {error}")
                        })?;
                    (effective, payload, digest)
                };
                Ok(CatalogEvent {
                    local_revision: revision,
                    event_id,
                    entity_table: table,
                    entity_key: key,
                    operation: effective_operation,
                    observed_artwork_revision: observed,
                    created_at,
                    payload_json,
                    payload_digest,
                })
            },
        )
        .collect()
}

#[cfg(test)]
fn pending_events(connection: &Connection) -> Result<Vec<CatalogEvent>, String> {
    ensure_sqlite_schema(connection)?;
    read_pending_events(connection, PUBLISH_BATCH_LIMIT)
}

fn enable_capture_at_baseline(
    database_path: &Path,
    source_id: &str,
    generation: i64,
    change_revision: i64,
) -> Result<(), String> {
    let mut connection = Connection::open(database_path)
        .map_err(|error| format!("Unable to open live catalog activation state: {error}"))?;
    connection
        .busy_timeout(Duration::from_secs(10))
        .map_err(|error| format!("Unable to configure live catalog activation: {error}"))?;
    ensure_sqlite_schema(&connection)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| format!("Unable to begin live catalog activation: {error}"))?;
    let current = read_source(&transaction)?;
    if current.source_instance_id != source_id
        || current.effective_mode != "live"
        || current.baseline_generation != generation
        || current.change_revision != change_revision
    {
        return Err(
            "Windows catalog changed after the authorized SNAP; create and authorize a fresh baseline"
                .to_string(),
        );
    }
    transaction
        .execute(
            "UPDATE catalog_sync_source SET capture_enabled=1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1",
            [],
        )
        .map_err(|error| format!("Unable to activate live catalog capture: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("Unable to commit live catalog activation: {error}"))
}

pub async fn synchronize(pool: &PgPool, database_path: &Path) -> Result<usize, String> {
    let connection = Connection::open(database_path)
        .map_err(|error| format!("Unable to open live catalog outbox: {error}"))?;
    connection
        .busy_timeout(Duration::from_secs(10))
        .map_err(|error| format!("Unable to configure live catalog outbox: {error}"))?;
    ensure_sqlite_schema(&connection)?;
    let source = read_source(&connection)?;
    drop(connection);

    let mut pg = pool
        .begin()
        .await
        .map_err(|error| format!("Unable to begin live catalog publication: {error}"))?;
    let registered = sqlx::query(concat!(
        "SELECT source.baseline_state,source.baseline_generation,source.baseline_change_revision,",
        "settings.mode,settings.active_source_instance_id,source.source_root,source.target_root,",
        "settings.source_root,settings.target_root FROM catalog_sync_sources source ",
        "CROSS JOIN catalog_sync_settings settings WHERE source.source_instance_id=$1 FOR UPDATE"
    ))
    .bind(&source.source_instance_id)
    .fetch_optional(&mut *pg)
    .await
    .map_err(|error| format!("Unable to read live catalog baseline: {error}"))?;
    let Some(registered) = registered else {
        return Err(
            "Live catalog source has no authorized SNAP baseline; queued changes remain in SQLite"
                .to_string(),
        );
    };
    let baseline_state: String = registered
        .try_get(0)
        .map_err(|error| format!("Unable to read live catalog baseline state: {error}"))?;
    let baseline_generation: i64 = registered
        .try_get(1)
        .map_err(|error| format!("Unable to read live catalog baseline generation: {error}"))?;
    let baseline_change_revision: i64 = registered
        .try_get(2)
        .map_err(|error| format!("Unable to read live catalog change revision: {error}"))?;
    let server_mode: String = registered
        .try_get(3)
        .map_err(|error| format!("Unable to read WebGUI catalog mode: {error}"))?;
    let active_source: String = registered
        .try_get(4)
        .map_err(|error| format!("Unable to read active Windows source: {error}"))?;
    let source_root: String = registered
        .try_get(5)
        .map_err(|error| format!("Unable to read baseline source root: {error}"))?;
    let target_root: String = registered
        .try_get(6)
        .map_err(|error| format!("Unable to read baseline target root: {error}"))?;
    let configured_source_root: String = registered
        .try_get(7)
        .map_err(|error| format!("Unable to read configured source root: {error}"))?;
    let configured_target_root: String = registered
        .try_get(8)
        .map_err(|error| format!("Unable to read configured target root: {error}"))?;
    if server_mode != "live" {
        return Err(format!(
            "WebGUI Catalog Synchronization Mode is {server_mode}; live publication remains queued"
        ));
    }
    if active_source != source.source_instance_id {
        return Err(
            "This Windows database is not the active authorized Live source; queued changes were not published"
                .to_string(),
        );
    }
    if source_root != configured_source_root || target_root != configured_target_root {
        return Err(
            "The active source library-root mapping no longer matches its authorized baseline"
                .to_string(),
        );
    }
    if baseline_state != "ready" {
        return Err(format!(
            "Live catalog baseline is {baseline_state}; queued changes remain in SQLite"
        ));
    }
    if source.baseline_generation != baseline_generation {
        return Err(
            "Live catalog baseline generation is stale; create and authorize a fresh Windows SNAP"
                .to_string(),
        );
    }
    if !source.capture_enabled {
        enable_capture_at_baseline(
            database_path,
            &source.source_instance_id,
            baseline_generation,
            baseline_change_revision,
        )?;
    }

    let mut connection = Connection::open(database_path)
        .map_err(|error| format!("Unable to reopen live catalog projection: {error}"))?;
    connection
        .busy_timeout(Duration::from_secs(10))
        .map_err(|error| format!("Unable to configure live catalog projection: {error}"))?;
    ensure_sqlite_schema(&connection)?;
    let read = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| format!("Unable to begin live catalog projection: {error}"))?;
    let pending = read_pending_events(&read, PUBLISH_BATCH_LIMIT)?;
    read.commit()
        .map_err(|error| format!("Unable to freeze live catalog projection: {error}"))?;
    drop(connection);
    if pending.is_empty() {
        pg.commit().await.map_err(|error| {
            format!("Unable to finish live catalog authorization check: {error}")
        })?;
        return Ok(0);
    }
    for event in &pending {
        let accepted = sqlx::query(concat!(
            "INSERT INTO catalog_sync_events(source_instance_id,local_revision,event_id,",
            "entity_table,entity_key,operation,payload,payload_digest,observed_artwork_revision,",
            "protocol_version,source_created_at) VALUES($1,$2,$3,$4,$5,$6,$7::jsonb,$8,$9,$10,$11) ",
            "ON CONFLICT(source_instance_id,local_revision) DO NOTHING RETURNING event_id"
        ))
        .bind(&source.source_instance_id)
        .bind(event.local_revision)
        .bind(&event.event_id)
        .bind(&event.entity_table)
        .bind(&event.entity_key)
        .bind(&event.operation)
        .bind(&event.payload_json)
        .bind(&event.payload_digest)
        .bind(event.observed_artwork_revision)
        .bind(PROTOCOL_VERSION)
        .bind(&event.created_at)
        .fetch_optional(&mut *pg)
        .await
        .map_err(|error| format!("Unable to publish live catalog revision: {error}"))?;
        if accepted.is_none() {
            let existing = sqlx::query(
                "SELECT event_id,entity_table,entity_key,operation,payload_digest,observed_artwork_revision \
                 FROM catalog_sync_events WHERE source_instance_id=$1 AND local_revision=$2",
            )
            .bind(&source.source_instance_id)
            .bind(event.local_revision)
            .fetch_optional(&mut *pg)
            .await
            .map_err(|error| format!("Unable to verify live catalog acknowledgement: {error}"))?;
            let Some(existing) = existing else {
                return Err(format!(
                    "PostgreSQL did not retain catalog revision {}",
                    event.local_revision
                ));
            };
            let matches = existing.try_get::<String, _>(0).ok().as_deref()
                == Some(event.event_id.as_str())
                && existing.try_get::<String, _>(1).ok().as_deref()
                    == Some(event.entity_table.as_str())
                && existing.try_get::<String, _>(2).ok().as_deref()
                    == Some(event.entity_key.as_str())
                && existing.try_get::<String, _>(3).ok().as_deref()
                    == Some(event.operation.as_str())
                && existing.try_get::<String, _>(4).ok().as_deref()
                    == Some(event.payload_digest.as_str())
                && existing.try_get::<i64, _>(5).ok() == Some(event.observed_artwork_revision);
            if !matches {
                return Err(format!(
                    "Catalog revision {} conflicts with a different durable PostgreSQL event; SQLite acknowledgement was refused",
                    event.local_revision
                ));
            }
        }
    }
    let last_revision = pending.last().map_or(0, |event| event.local_revision);
    sqlx::query(concat!(
        "UPDATE catalog_sync_sources SET last_received_revision=GREATEST(last_received_revision,$1),",
        "updated_at=now() WHERE source_instance_id=$2"
    ))
    .bind(last_revision)
    .bind(&source.source_instance_id)
    .execute(&mut *pg)
    .await
    .map_err(|error| format!("Unable to acknowledge live catalog revisions in PostgreSQL: {error}"))?;
    pg.commit()
        .await
        .map_err(|error| format!("Unable to commit live catalog publication: {error}"))?;

    let mut connection = Connection::open(database_path)
        .map_err(|error| format!("Unable to reopen live catalog outbox: {error}"))?;
    connection
        .busy_timeout(Duration::from_secs(10))
        .map_err(|error| format!("Unable to configure live catalog acknowledgement: {error}"))?;
    ensure_sqlite_schema(&connection)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Unable to begin live catalog acknowledgement: {error}"))?;
    for event in &pending {
        transaction
            .execute(
                "DELETE FROM catalog_sync_outbox WHERE local_revision=? AND event_id=?",
                params![event.local_revision, event.event_id],
            )
            .map_err(|error| format!("Unable to acknowledge live catalog outbox: {error}"))?;
    }
    transaction
        .commit()
        .map_err(|error| format!("Unable to commit live catalog acknowledgement: {error}"))?;
    Ok(pending.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(connection: &Connection) {
        connection
            .execute_batch(
                "CREATE TABLE artists(artist_key TEXT PRIMARY KEY,artist_name TEXT,primary_path TEXT);\
                 CREATE TABLE albums(album_key TEXT PRIMARY KEY,artist_key TEXT,album_name TEXT,path TEXT);\
                 CREATE TABLE tracks(track_key TEXT PRIMARY KEY,album_key TEXT,path TEXT,title TEXT);\
                 CREATE TABLE album_artwork_write_state(album_key TEXT PRIMARY KEY,physically_verified INTEGER NOT NULL DEFAULT 0,verified_revision INTEGER NOT NULL DEFAULT 0);",
            )
            .unwrap();
        ensure_sqlite_schema(connection).unwrap();
        connection
            .execute(
                "UPDATE catalog_sync_source SET capture_enabled=1 WHERE singleton=1",
                [],
            )
            .unwrap();
    }

    #[test]
    fn changes_are_captured_transactionally_and_revisions_are_monotonic() {
        let connection = Connection::open_in_memory().unwrap();
        catalog(&connection);
        connection
            .execute(
                "INSERT INTO artists VALUES('tag:artist','Artist','E:/music/Artist')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE artists SET artist_name='Artist Updated' WHERE artist_key='tag:artist'",
                [],
            )
            .unwrap();
        connection
            .execute("DELETE FROM artists WHERE artist_key='tag:artist'", [])
            .unwrap();
        let events = pending_events(&connection).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].local_revision, 1);
        assert_eq!(events[1].local_revision, 2);
        assert_eq!(events[2].local_revision, 3);
        assert_eq!(events[2].operation, "delete");
    }

    #[test]
    fn rollback_does_not_leave_an_outbox_event() {
        let mut connection = Connection::open_in_memory().unwrap();
        catalog(&connection);
        let transaction = connection.transaction().unwrap();
        transaction
            .execute(
                "INSERT INTO artists VALUES('tag:artist','Artist','E:/music/Artist')",
                [],
            )
            .unwrap();
        transaction.rollback().unwrap();
        let count: i64 = connection
            .query_row("SELECT count(*) FROM catalog_sync_outbox", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
        assert_eq!(source(&connection).unwrap().next_revision, 0);
    }

    #[test]
    fn complete_row_payload_survives_unrelated_later_session_activity() {
        let connection = Connection::open_in_memory().unwrap();
        catalog(&connection);
        connection
            .execute(
                "INSERT INTO artists VALUES('tag:old','Previously Processed','E:/music/Old')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO artists VALUES('tag:new','Current Session','E:/music/New')",
                [],
            )
            .unwrap();
        let events = pending_events(&connection).unwrap();
        assert_eq!(events.len(), 2);
        assert!(events[0].payload_json.contains("Previously Processed"));
        assert!(events[1].payload_json.contains("Current Session"));
    }

    #[test]
    fn frozen_event_payload_survives_lost_ack_and_later_write() {
        let connection = Connection::open_in_memory().unwrap();
        catalog(&connection);
        connection
            .execute(
                "INSERT INTO artists VALUES('tag:artist','Before Ack','E:/music/Artist')",
                [],
            )
            .unwrap();
        let accepted = pending_events(&connection).unwrap();
        assert_eq!(accepted.len(), 1);
        connection
            .execute(
                "UPDATE artists SET artist_name='After Ack Was Lost' WHERE artist_key='tag:artist'",
                [],
            )
            .unwrap();
        let retried = pending_events(&connection).unwrap();
        assert_eq!(retried.len(), 2);
        assert_eq!(retried[0].event_id, accepted[0].event_id);
        assert_eq!(retried[0].payload_digest, accepted[0].payload_digest);
        assert_eq!(retried[0].payload_json, accepted[0].payload_json);
        assert!(retried[0].payload_json.contains("Before Ack"));
        assert!(retried[1].payload_json.contains("After Ack Was Lost"));
    }

    #[test]
    fn ordinary_schema_checks_do_not_replace_capture_triggers() {
        let connection = Connection::open_in_memory().unwrap();
        catalog(&connection);
        let before: String = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='trigger' AND name='catalog_sync_artists_update'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        ensure_sqlite_schema(&connection).unwrap();
        let after: String = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='trigger' AND name='catalog_sync_artists_update'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let version: i64 = connection
            .query_row(
                "SELECT trigger_schema_version FROM catalog_sync_source WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(before, after);
        assert_eq!(version, TRIGGER_SCHEMA_VERSION);
    }

    #[test]
    fn leaving_live_invalidates_the_baseline_and_returning_live_waits() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("catalog.sqlite");
        let connection = Connection::open(&path).unwrap();
        catalog(&connection);
        connection
            .execute(
                "UPDATE catalog_sync_source SET effective_mode='live',capture_enabled=1 WHERE singleton=1",
                [],
            )
            .unwrap();
        drop(connection);

        let manual = set_runtime_mode(&path, "manual-snap").unwrap();
        assert!(!manual.capture_enabled);
        assert_eq!(manual.effective_mode, "manual-snap");
        assert_eq!(manual.baseline_generation, 2);
        let waiting = set_runtime_mode(&path, "live").unwrap();
        assert_eq!(waiting.effective_mode, "live");
        assert!(!waiting.capture_enabled);
        assert_eq!(waiting.baseline_generation, 2);
    }

    #[test]
    fn publisher_materializes_only_one_bounded_batch() {
        let connection = Connection::open_in_memory().unwrap();
        catalog(&connection);
        for index in 0..(PUBLISH_BATCH_LIMIT + 5) {
            connection
                .execute(
                    "INSERT INTO artists VALUES(?,?,?)",
                    params![
                        format!("tag:{index}"),
                        format!("Artist {index}"),
                        format!("E:/music/{index}")
                    ],
                )
                .unwrap();
        }
        let batch = pending_events(&connection).unwrap();
        assert_eq!(batch.len(), PUBLISH_BATCH_LIMIT);
        assert_eq!(batch.first().unwrap().local_revision, 1);
        assert_eq!(
            batch.last().unwrap().local_revision,
            i64::try_from(PUBLISH_BATCH_LIMIT).unwrap()
        );
        let frozen: i64 = connection
            .query_row(
                "SELECT count(*) FROM catalog_sync_outbox WHERE payload_json IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(frozen, i64::try_from(PUBLISH_BATCH_LIMIT).unwrap());
    }

    #[test]
    fn manual_mode_does_not_accumulate_catalog_events() {
        let connection = Connection::open_in_memory().unwrap();
        catalog(&connection);
        connection
            .execute(
                "UPDATE catalog_sync_source SET capture_enabled=0 WHERE singleton=1",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO artists VALUES('tag:manual','Manual SNAP','E:/music/Manual')",
                [],
            )
            .unwrap();
        assert!(pending_events(&connection).unwrap().is_empty());
        assert_eq!(source(&connection).unwrap().next_revision, 0);
        assert_eq!(source(&connection).unwrap().change_revision, 1);
    }
}
