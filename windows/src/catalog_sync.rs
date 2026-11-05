//! Durable, one-way Windows SQLite -> WebGUI PostgreSQL catalog transport.
//!
//! SQLite triggers capture entity identity and a monotonic local revision in
//! the same transaction as every Artist, Album, or track mutation.  The
//! publisher later materializes a complete row projection and retains the
//! outbox until PostgreSQL has durably accepted it.  PostgreSQL remains unable
//! to write this stream back into SQLite.

use base64::Engine;
use rusqlite::{Connection, OptionalExtension, params, types::ValueRef};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

pub const PROTOCOL_VERSION: i64 = 1;

const SQLITE_SOURCE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS catalog_sync_source (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    source_instance_id TEXT NOT NULL UNIQUE,
    protocol_version INTEGER NOT NULL DEFAULT 1,
    next_revision INTEGER NOT NULL DEFAULT 0,
    capture_enabled INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
"#;

const SQLITE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS catalog_sync_source (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    source_instance_id TEXT NOT NULL UNIQUE,
    protocol_version INTEGER NOT NULL DEFAULT 1,
    next_revision INTEGER NOT NULL DEFAULT 0,
    capture_enabled INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

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
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_catalog_sync_outbox_entity
    ON catalog_sync_outbox(entity_table,entity_key,local_revision);

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
WHEN (SELECT capture_enabled FROM catalog_sync_source WHERE singleton=1)=1 BEGIN
    UPDATE catalog_sync_source SET next_revision=next_revision+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1;
    INSERT INTO catalog_sync_outbox VALUES((SELECT next_revision FROM catalog_sync_source WHERE singleton=1),lower(hex(randomblob(16))),'artists',new.artist_key,'upsert',0,strftime('%Y-%m-%dT%H:%M:%SZ','now'));
END;
CREATE TRIGGER IF NOT EXISTS catalog_sync_artists_update
AFTER UPDATE ON artists
WHEN (SELECT capture_enabled FROM catalog_sync_source WHERE singleton=1)=1 BEGIN
    UPDATE catalog_sync_source SET next_revision=next_revision+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1;
    INSERT INTO catalog_sync_outbox VALUES((SELECT next_revision FROM catalog_sync_source WHERE singleton=1),lower(hex(randomblob(16))),'artists',new.artist_key,'upsert',0,strftime('%Y-%m-%dT%H:%M:%SZ','now'));
END;
CREATE TRIGGER IF NOT EXISTS catalog_sync_artists_delete
AFTER DELETE ON artists
WHEN (SELECT capture_enabled FROM catalog_sync_source WHERE singleton=1)=1 BEGIN
    UPDATE catalog_sync_source SET next_revision=next_revision+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1;
    INSERT INTO catalog_sync_outbox VALUES((SELECT next_revision FROM catalog_sync_source WHERE singleton=1),lower(hex(randomblob(16))),'artists',old.artist_key,'delete',0,strftime('%Y-%m-%dT%H:%M:%SZ','now'));
END;

CREATE TRIGGER IF NOT EXISTS catalog_sync_albums_insert
AFTER INSERT ON albums
WHEN (SELECT capture_enabled FROM catalog_sync_source WHERE singleton=1)=1 BEGIN
    UPDATE catalog_sync_source SET next_revision=next_revision+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1;
    INSERT INTO catalog_sync_outbox VALUES((SELECT next_revision FROM catalog_sync_source WHERE singleton=1),lower(hex(randomblob(16))),'albums',new.album_key,'upsert',coalesce((SELECT CASE WHEN physically_verified=1 THEN verified_revision ELSE 0 END FROM album_artwork_write_state WHERE album_key=new.album_key),0),strftime('%Y-%m-%dT%H:%M:%SZ','now'));
END;
CREATE TRIGGER IF NOT EXISTS catalog_sync_albums_update
AFTER UPDATE ON albums
WHEN (SELECT capture_enabled FROM catalog_sync_source WHERE singleton=1)=1 BEGIN
    UPDATE catalog_sync_source SET next_revision=next_revision+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1;
    INSERT INTO catalog_sync_outbox VALUES((SELECT next_revision FROM catalog_sync_source WHERE singleton=1),lower(hex(randomblob(16))),'albums',new.album_key,'upsert',coalesce((SELECT CASE WHEN physically_verified=1 THEN verified_revision ELSE 0 END FROM album_artwork_write_state WHERE album_key=new.album_key),0),strftime('%Y-%m-%dT%H:%M:%SZ','now'));
END;
CREATE TRIGGER IF NOT EXISTS catalog_sync_albums_delete
AFTER DELETE ON albums
WHEN (SELECT capture_enabled FROM catalog_sync_source WHERE singleton=1)=1 BEGIN
    UPDATE catalog_sync_source SET next_revision=next_revision+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1;
    INSERT INTO catalog_sync_outbox VALUES((SELECT next_revision FROM catalog_sync_source WHERE singleton=1),lower(hex(randomblob(16))),'albums',old.album_key,'delete',0,strftime('%Y-%m-%dT%H:%M:%SZ','now'));
END;

CREATE TRIGGER IF NOT EXISTS catalog_sync_tracks_insert
AFTER INSERT ON tracks
WHEN (SELECT capture_enabled FROM catalog_sync_source WHERE singleton=1)=1 BEGIN
    UPDATE catalog_sync_source SET next_revision=next_revision+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1;
    INSERT INTO catalog_sync_outbox VALUES((SELECT next_revision FROM catalog_sync_source WHERE singleton=1),lower(hex(randomblob(16))),'tracks',new.track_key,'upsert',0,strftime('%Y-%m-%dT%H:%M:%SZ','now'));
END;
CREATE TRIGGER IF NOT EXISTS catalog_sync_tracks_update
AFTER UPDATE ON tracks
WHEN (SELECT capture_enabled FROM catalog_sync_source WHERE singleton=1)=1 BEGIN
    UPDATE catalog_sync_source SET next_revision=next_revision+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1;
    INSERT INTO catalog_sync_outbox VALUES((SELECT next_revision FROM catalog_sync_source WHERE singleton=1),lower(hex(randomblob(16))),'tracks',new.track_key,'upsert',0,strftime('%Y-%m-%dT%H:%M:%SZ','now'));
END;
CREATE TRIGGER IF NOT EXISTS catalog_sync_tracks_delete
AFTER DELETE ON tracks
WHEN (SELECT capture_enabled FROM catalog_sync_source WHERE singleton=1)=1 BEGIN
    UPDATE catalog_sync_source SET next_revision=next_revision+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1;
    INSERT INTO catalog_sync_outbox VALUES((SELECT next_revision FROM catalog_sync_source WHERE singleton=1),lower(hex(randomblob(16))),'tracks',old.track_key,'delete',0,strftime('%Y-%m-%dT%H:%M:%SZ','now'));
END;
"#;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSource {
    pub source_instance_id: String,
    pub next_revision: i64,
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
        .execute_batch(SQLITE_SOURCE_SCHEMA)
        .map_err(|error| format!("Unable to prepare live catalog source: {error}"))?;
    let columns = connection
        .prepare("PRAGMA table_info(catalog_sync_source)")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|error| format!("Unable to inspect live catalog source: {error}"))?;
    if !columns.iter().any(|name| name == "capture_enabled") {
        connection
            .execute(
                "ALTER TABLE catalog_sync_source ADD COLUMN capture_enabled INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .map_err(|error| format!("Unable to add live catalog capture mode: {error}"))?;
    }
    connection
        .execute_batch(SQLITE_SCHEMA)
        .map_err(|error| format!("Unable to prepare live catalog outbox: {error}"))?;
    Ok(())
}

pub fn set_capture_enabled(database_path: &Path, enabled: bool) -> Result<(), String> {
    let connection = Connection::open(database_path)
        .map_err(|error| format!("Unable to open live catalog capture state: {error}"))?;
    connection
        .busy_timeout(Duration::from_secs(10))
        .map_err(|error| format!("Unable to configure live catalog capture state: {error}"))?;
    ensure_sqlite_schema(&connection)?;
    connection
        .execute(
            "UPDATE catalog_sync_source SET capture_enabled=?,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE singleton=1",
            [i64::from(enabled)],
        )
        .map_err(|error| format!("Unable to update live catalog capture mode: {error}"))?;
    Ok(())
}

fn read_source(connection: &Connection) -> Result<CatalogSource, String> {
    connection
        .query_row(
            "SELECT source_instance_id,next_revision FROM catalog_sync_source WHERE singleton=1",
            [],
            |row| {
                Ok(CatalogSource {
                    source_instance_id: row.get(0)?,
                    next_revision: row.get(1)?,
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

fn read_pending_events(connection: &Connection) -> Result<Vec<CatalogEvent>, String> {
    let mut statement = connection
        .prepare(concat!(
            "SELECT local_revision,event_id,entity_table,entity_key,operation,",
            "observed_artwork_revision,created_at FROM catalog_sync_outbox ORDER BY local_revision"
        ))
        .map_err(|error| format!("Unable to read live catalog outbox: {error}"))?;
    let envelopes = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(|error| format!("Unable to query live catalog outbox: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to collect live catalog outbox: {error}"))?;
    drop(statement);
    envelopes
        .into_iter()
        .map(
            |(revision, event_id, table, key, operation, observed, created_at)| {
                // This is a state-convergence stream, not an edit-history
                // log. If a row was changed and then deleted while offline,
                // retain every revision but publish the unavailable older
                // projection as a tombstone.
                let materialized = if operation == "delete" {
                    None
                } else {
                    materialize_row(connection, &table, &key)?
                };
                let effective_operation = if materialized.is_some() {
                    operation
                } else {
                    "delete".to_string()
                };
                let payload_json = materialized.unwrap_or_else(|| "{}".to_string());
                let payload_digest = Sha256::digest(payload_json.as_bytes())
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
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
    read_pending_events(connection)
}

pub async fn synchronize(pool: &PgPool, database_path: &Path) -> Result<usize, String> {
    let mut connection = Connection::open(database_path)
        .map_err(|error| format!("Unable to open live catalog outbox: {error}"))?;
    connection
        .busy_timeout(Duration::from_secs(10))
        .map_err(|error| format!("Unable to configure live catalog outbox: {error}"))?;
    ensure_sqlite_schema(&connection)?;
    let read = connection
        .transaction()
        .map_err(|error| format!("Unable to begin live catalog projection: {error}"))?;
    let source = read_source(&read)?;
    let pending = read_pending_events(&read)?;
    read.commit()
        .map_err(|error| format!("Unable to finish live catalog projection: {error}"))?;
    if pending.is_empty() {
        return Ok(0);
    }
    drop(connection);

    let mut pg = pool
        .begin()
        .await
        .map_err(|error| format!("Unable to begin live catalog publication: {error}"))?;
    let registered = sqlx::query(
        "SELECT baseline_state FROM catalog_sync_sources WHERE source_instance_id=$1 FOR UPDATE",
    )
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
    if baseline_state != "ready" {
        return Err(format!(
            "Live catalog baseline is {baseline_state}; queued changes remain in SQLite"
        ));
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
    }
}
