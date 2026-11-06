//! Durable, optional Windows/WebGUI Album artwork write coordination.
//!
//! SQLite remains the Windows catalog authority. PostgreSQL stores only the
//! last successful artwork writer so each client can render the opposing-
//! writer indicator. No operation here locks, approves, or changes an Album.

use crate::config::Config;
use crate::credential_store::load_album_coordination_database_url;
use crate::embedded_artwork::embedded_front_metadata;
use crate::inspect::inspect_image;
use crate::media_database::{PathMapper, record_album_outcome, record_compilation_progress};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::Value;
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;

const SQLITE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS album_artwork_write_state (
    album_identity TEXT PRIMARY KEY,
    album_key TEXT NOT NULL COLLATE NOCASE,
    musicbrainz_albumid TEXT NOT NULL DEFAULT '',
    relative_path TEXT NOT NULL DEFAULT '',
    writer TEXT NOT NULL CHECK(writer IN ('windows', 'webgui')),
    operation TEXT NOT NULL,
    event_id TEXT NOT NULL UNIQUE,
    source_completed_ms INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0,
    physically_verified INTEGER NOT NULL DEFAULT 0,
    verified_revision INTEGER NOT NULL DEFAULT 0,
    completed_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS album_artwork_write_outbox (
    event_id TEXT PRIMARY KEY,
    album_identity TEXT NOT NULL,
    album_key TEXT NOT NULL COLLATE NOCASE,
    musicbrainz_albumid TEXT NOT NULL DEFAULT '',
    relative_path TEXT NOT NULL DEFAULT '',
    writer TEXT NOT NULL CHECK(writer = 'windows'),
    operation TEXT NOT NULL,
    source_completed_ms INTEGER NOT NULL,
    completed_at TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_album_artwork_write_state_key
    ON album_artwork_write_state(album_key);
CREATE INDEX IF NOT EXISTS idx_album_artwork_write_outbox_order
    ON album_artwork_write_outbox(source_completed_ms, event_id);
"#;

static EVENT_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
struct CoordinationEvent {
    album_identity: String,
    album_key: String,
    musicbrainz_albumid: String,
    relative_path: String,
    writer: String,
    operation: String,
    event_id: String,
    source_completed_ms: i64,
    revision: i64,
    completed_at: String,
}

pub fn ensure_sqlite_schema(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(SQLITE_SCHEMA)
        .map_err(|error| format!("Unable to prepare Album coordination state: {error}"))?;
    let columns = connection
        .prepare("PRAGMA table_info(album_artwork_write_state)")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|error| format!("Unable to inspect Album coordination state: {error}"))?;
    if !columns.iter().any(|name| name == "physically_verified") {
        connection
            .execute(
                "ALTER TABLE album_artwork_write_state ADD COLUMN physically_verified INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .map_err(|error| format!("Unable to add Album verification state: {error}"))?;
    }
    if !columns.iter().any(|name| name == "verified_revision") {
        connection
            .execute(
                "ALTER TABLE album_artwork_write_state ADD COLUMN verified_revision INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .map_err(|error| format!("Unable to add Album verification revision: {error}"))?;
    }
    Ok(())
}

pub fn canonical_album_identity(album_key: &str) -> Result<String, String> {
    let normalized = album_key
        .nfkc()
        .case_fold()
        .filter(|character| {
            !matches!(
                *character,
                '\u{feff}' | '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}'
            )
        })
        .collect::<String>();
    let normalized = normalized.trim();
    if normalized.is_empty() {
        return Err("Album coordination requires a canonical Album key".to_string());
    }
    Ok(format!("key:{normalized}"))
}

fn source_completed_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or_default()
}

fn event_id(completed_ms: i64) -> String {
    let counter = EVENT_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("windows-{}-{completed_ms}-{counter}", std::process::id())
}

fn relative_path(canonical_root: &str, canonical_album: &str) -> String {
    let root = canonical_root
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string();
    let album = canonical_album.replace('\\', "/");
    album
        .strip_prefix(&root)
        .unwrap_or("")
        .trim_start_matches('/')
        .to_ascii_lowercase()
}

/// Queue a verified Windows write in the same SQLite transaction as its
/// catalog outcome, and immediately make Windows the effective local writer.
pub fn queue_windows_album_write(
    transaction: &Transaction<'_>,
    album_key: &str,
    musicbrainz_albumid: &str,
    canonical_root: &str,
    canonical_album: &str,
    operation: &str,
    now: &str,
) -> Result<String, String> {
    let album_identity = canonical_album_identity(album_key)?;
    let completed_ms = source_completed_ms();
    let event_id = event_id(completed_ms);
    let relative_path = relative_path(canonical_root, canonical_album);
    transaction
        .execute(
            "INSERT INTO album_artwork_write_outbox(event_id,album_identity,album_key,musicbrainz_albumid,relative_path,writer,operation,source_completed_ms,completed_at,created_at) \
             VALUES(?,?,?,?,?,'windows',?,?,?,?)",
            params![event_id, album_identity, album_key, musicbrainz_albumid, relative_path,
                operation, completed_ms, now, now],
        )
        .map_err(|error| format!("Unable to queue Album write notification: {error}"))?;
    transaction
        .execute(
            "INSERT INTO album_artwork_write_state(album_identity,album_key,musicbrainz_albumid,relative_path,writer,operation,event_id,source_completed_ms,revision,physically_verified,verified_revision,completed_at,updated_at) \
             VALUES(?,?,?,?,'windows',?,?,?,0,1,0,?,?) \
             ON CONFLICT(album_identity) DO UPDATE SET album_key=excluded.album_key,musicbrainz_albumid=excluded.musicbrainz_albumid,relative_path=excluded.relative_path,writer=excluded.writer,operation=excluded.operation,event_id=excluded.event_id,source_completed_ms=excluded.source_completed_ms,revision=0,physically_verified=1,verified_revision=0,completed_at=excluded.completed_at,updated_at=excluded.updated_at \
             WHERE excluded.source_completed_ms > album_artwork_write_state.source_completed_ms \
                OR (excluded.source_completed_ms = album_artwork_write_state.source_completed_ms AND excluded.event_id > album_artwork_write_state.event_id)",
            params![album_identity, album_key, musicbrainz_albumid, relative_path,
                operation, event_id, completed_ms, now, now],
        )
        .map_err(|error| format!("Unable to persist local Album writer: {error}"))?;
    Ok(event_id)
}

fn read_events(connection: &Connection, table: &str) -> Result<Vec<CoordinationEvent>, String> {
    let sql = format!(
        "SELECT album_identity,album_key,musicbrainz_albumid,relative_path,writer,operation,event_id,source_completed_ms,{} AS revision,completed_at FROM {table} ORDER BY source_completed_ms,event_id",
        if table == "album_artwork_write_state" {
            "revision"
        } else {
            "0"
        }
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| format!("Unable to read Album coordination state: {error}"))?;
    statement
        .query_map([], |row| {
            Ok(CoordinationEvent {
                album_identity: row.get(0)?,
                album_key: row.get(1)?,
                musicbrainz_albumid: row.get(2)?,
                relative_path: row.get(3)?,
                writer: row.get(4)?,
                operation: row.get(5)?,
                event_id: row.get(6)?,
                source_completed_ms: row.get(7)?,
                revision: row.get(8)?,
                completed_at: row.get(9)?,
            })
        })
        .map_err(|error| format!("Unable to query Album coordination state: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to collect Album coordination state: {error}"))
}

fn newer(left: &CoordinationEvent, right: &CoordinationEvent) -> bool {
    (left.source_completed_ms, left.event_id.as_str())
        > (right.source_completed_ms, right.event_id.as_str())
}

pub fn effective_writers(database_path: &Path) -> Result<HashMap<String, String>, String> {
    let connection = Connection::open(database_path)
        .map_err(|error| format!("Unable to open Album coordination state: {error}"))?;
    connection
        .busy_timeout(Duration::from_secs(10))
        .map_err(|error| format!("Unable to configure Album coordination state: {error}"))?;
    ensure_sqlite_schema(&connection)?;
    let mut current = HashMap::<String, CoordinationEvent>::new();
    for event in read_events(&connection, "album_artwork_write_state")?
        .into_iter()
        .chain(read_events(&connection, "album_artwork_write_outbox")?)
    {
        let replace = current
            .get(&event.album_identity)
            .is_none_or(|prior| newer(&event, prior));
        if replace {
            current.insert(event.album_identity.clone(), event);
        }
    }
    Ok(current
        .into_values()
        .map(|event| (event.album_key.to_ascii_lowercase(), event.writer))
        .collect())
}

pub fn coordination_database_url(config: &Config) -> Result<Option<String>, String> {
    if let Ok(value) = std::env::var("SPLINED_COORDINATION_DATABASE_URL") {
        let value = value.trim().to_string();
        if value.is_empty() {
            return load_album_coordination_database_url(config);
        }
        if !value.starts_with("postgres://") && !value.starts_with("postgresql://") {
            return Err(
                "SPLINED_COORDINATION_DATABASE_URL must use postgres:// or postgresql://."
                    .to_string(),
            );
        }
        return Ok(Some(value));
    }
    load_album_coordination_database_url(config)
}

pub async fn connect_pool(url: &str) -> Result<PgPool, String> {
    PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(10))
        .connect(url)
        .await
        .map_err(|error| format!("Album coordination PostgreSQL is unavailable: {error}"))
}

pub async fn synchronize(
    pool: &PgPool,
    database_path: &Path,
) -> Result<HashMap<String, String>, String> {
    let connection = Connection::open(database_path)
        .map_err(|error| format!("Unable to open Album coordination outbox: {error}"))?;
    connection
        .busy_timeout(Duration::from_secs(10))
        .map_err(|error| format!("Unable to configure Album coordination outbox: {error}"))?;
    ensure_sqlite_schema(&connection)?;
    let pending = read_events(&connection, "album_artwork_write_outbox")?;
    drop(connection);

    let mut pg = pool
        .begin()
        .await
        .map_err(|error| format!("Unable to begin Album coordination: {error}"))?;
    for event in &pending {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(&event.album_identity)
            .execute(&mut *pg)
            .await
            .map_err(|error| format!("Unable to lock Album coordination row: {error}"))?;
        sqlx::query(
            "INSERT INTO album_artwork_write_coordination(album_identity,album_key,musicbrainz_albumid,relative_path,writer,operation,event_id,source_completed_ms,completed_at) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,to_timestamp($8::double precision / 1000.0)) \
             ON CONFLICT(album_identity) DO UPDATE SET album_key=excluded.album_key,musicbrainz_albumid=excluded.musicbrainz_albumid,relative_path=excluded.relative_path,writer=excluded.writer,operation=excluded.operation,event_id=excluded.event_id,source_completed_ms=excluded.source_completed_ms,revision=nextval('album_artwork_write_revision_seq'),completed_at=excluded.completed_at,updated_at=now() \
             WHERE excluded.source_completed_ms > album_artwork_write_coordination.source_completed_ms \
                OR (excluded.source_completed_ms = album_artwork_write_coordination.source_completed_ms AND excluded.event_id > album_artwork_write_coordination.event_id)",
        )
        .bind(&event.album_identity)
        .bind(&event.album_key)
        .bind(&event.musicbrainz_albumid)
        .bind(&event.relative_path)
        .bind(&event.writer)
        .bind(&event.operation)
        .bind(&event.event_id)
        .bind(event.source_completed_ms)
        .execute(&mut *pg)
        .await
        .map_err(|error| format!("Unable to publish Album coordination event: {error}"))?;
    }
    let rows = sqlx::query(
        "SELECT album_identity,album_key,musicbrainz_albumid,relative_path,writer,operation,event_id,source_completed_ms,revision,completed_at::text FROM album_artwork_write_coordination ORDER BY revision",
    )
    .fetch_all(&mut *pg)
    .await
    .map_err(|error| format!("Unable to read Album coordination state: {error}"))?;
    pg.commit()
        .await
        .map_err(|error| format!("Unable to commit Album coordination: {error}"))?;

    let mut connection = Connection::open(database_path)
        .map_err(|error| format!("Unable to persist Album coordination state: {error}"))?;
    connection
        .busy_timeout(Duration::from_secs(10))
        .map_err(|error| format!("Unable to configure Album coordination persistence: {error}"))?;
    ensure_sqlite_schema(&connection)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Unable to begin Album coordination persistence: {error}"))?;
    for row in rows {
        let event = CoordinationEvent {
            album_identity: row.try_get(0).map_err(|error| error.to_string())?,
            album_key: row.try_get(1).map_err(|error| error.to_string())?,
            musicbrainz_albumid: row.try_get(2).map_err(|error| error.to_string())?,
            relative_path: row.try_get(3).map_err(|error| error.to_string())?,
            writer: row.try_get(4).map_err(|error| error.to_string())?,
            operation: row.try_get(5).map_err(|error| error.to_string())?,
            event_id: row.try_get(6).map_err(|error| error.to_string())?,
            source_completed_ms: row.try_get(7).map_err(|error| error.to_string())?,
            revision: row.try_get(8).map_err(|error| error.to_string())?,
            completed_at: row.try_get(9).map_err(|error| error.to_string())?,
        };
        transaction.execute(
            "INSERT INTO album_artwork_write_state(album_identity,album_key,musicbrainz_albumid,relative_path,writer,operation,event_id,source_completed_ms,revision,physically_verified,verified_revision,completed_at,updated_at) \
             VALUES(?,?,?,?,?,?,?,?,?,CASE WHEN ?='windows' THEN 1 ELSE 0 END,CASE WHEN ?='windows' THEN ? ELSE 0 END,?,strftime('%Y-%m-%dT%H:%M:%SZ','now')) \
             ON CONFLICT(album_identity) DO UPDATE SET album_key=excluded.album_key,musicbrainz_albumid=excluded.musicbrainz_albumid,relative_path=excluded.relative_path,writer=excluded.writer,operation=excluded.operation,event_id=excluded.event_id,source_completed_ms=excluded.source_completed_ms,revision=excluded.revision,physically_verified=CASE WHEN excluded.writer='windows' THEN 1 WHEN excluded.event_id=album_artwork_write_state.event_id THEN album_artwork_write_state.physically_verified ELSE 0 END,verified_revision=CASE WHEN excluded.writer='windows' THEN excluded.revision WHEN excluded.event_id=album_artwork_write_state.event_id THEN album_artwork_write_state.verified_revision ELSE 0 END,completed_at=excluded.completed_at,updated_at=excluded.updated_at \
             WHERE excluded.source_completed_ms > album_artwork_write_state.source_completed_ms \
                OR (excluded.source_completed_ms = album_artwork_write_state.source_completed_ms AND excluded.event_id >= album_artwork_write_state.event_id)",
            params![event.album_identity,event.album_key,event.musicbrainz_albumid,event.relative_path,
                event.writer,event.operation,event.event_id,event.source_completed_ms,event.revision,event.writer,event.writer,event.revision,event.completed_at],
        ).map_err(|error| format!("Unable to acknowledge Album coordination row: {error}"))?;
    }
    for event in &pending {
        transaction
            .execute(
                "DELETE FROM album_artwork_write_outbox WHERE event_id=?",
                [&event.event_id],
            )
            .map_err(|error| format!("Unable to acknowledge Album coordination outbox: {error}"))?;
    }
    transaction
        .commit()
        .map_err(|error| format!("Unable to commit Album coordination persistence: {error}"))?;
    effective_writers(database_path)
}

#[derive(Clone, Debug)]
struct PendingWebguiVerification {
    album_key: String,
    operation: String,
    revision: i64,
    album_path: String,
    cover_path: String,
    track_paths: Vec<String>,
}

fn mapped_path(mapper: &PathMapper, value: &str) -> Option<PathBuf> {
    if value.trim().is_empty() {
        None
    } else {
        mapper.to_local(value).ok().map(PathBuf::from)
    }
}

fn readable_album_cover(
    mapper: &PathMapper,
    album_path: &str,
    indexed_cover: &str,
    configured_stem: &str,
) -> Option<PathBuf> {
    if let Some(path) = mapped_path(mapper, indexed_cover)
        && path.is_file()
        && inspect_image(&path).is_ok()
    {
        return Some(path);
    }
    let directory = mapped_path(mapper, album_path)?;
    fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case(configured_stem))
                && path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| {
                        matches!(
                            value.to_ascii_lowercase().as_str(),
                            "jpg" | "jpeg" | "png" | "webp"
                        )
                    })
        })
        .find(|path| inspect_image(path).is_ok())
}

fn compliant_tracks(mapper: &PathMapper, tracks: &[String], preferred_side: u32) -> (usize, usize) {
    let mut readable = 0usize;
    let mut compliant = 0usize;
    for track in tracks {
        let Some(path) = mapped_path(mapper, track) else {
            continue;
        };
        if let Ok(metadata) = embedded_front_metadata(&path) {
            readable += 1;
            if metadata.is_some_and(|(width, height, _)| {
                width == preferred_side && height == preferred_side
            }) {
                compliant += 1;
            }
        }
    }
    (readable, compliant)
}

/// Independently verify WebGUI's physical-media result before allowing a
/// later Windows catalog revision to cross the artwork causal barrier.
///
/// PostgreSQL contributes only event provenance. Cover and embedded artwork
/// evidence is read from the configured Windows library, then the existing
/// Windows SQLite outcome paths persist the bounded authoritative result.
pub fn verify_webgui_writes(config: &Config, database_path: &Path) -> Result<usize, String> {
    if config.mode != crate::config::Mode::Write {
        return Ok(0);
    }
    let connection = Connection::open(database_path)
        .map_err(|error| format!("Unable to open Album verification state: {error}"))?;
    connection
        .busy_timeout(Duration::from_secs(10))
        .map_err(|error| format!("Unable to configure Album verification state: {error}"))?;
    ensure_sqlite_schema(&connection)?;
    let signature: Option<String> = connection
        .query_row(
            "SELECT signature_json FROM picker_inventory WHERE inventory_key='splined-media-library'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("Unable to read Album verification path mapping: {error}"))?;
    let Some(canonical_root) = signature
        .as_deref()
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .and_then(|value| {
            value
                .get("library_root")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
    else {
        return Ok(0);
    };
    let mapper = PathMapper::new(canonical_root, &config.library.music_library)?;
    let mut statement = connection
        .prepare(
            "SELECT state.album_key,state.operation,state.revision,albums.path,COALESCE(albums.cover_path,'') \
             FROM album_artwork_write_state state JOIN albums ON albums.album_key=state.album_key COLLATE NOCASE \
             WHERE state.writer='webgui' AND state.physically_verified=0 ORDER BY state.revision",
        )
        .map_err(|error| format!("Unable to prepare pending Album verification: {error}"))?;
    let base_rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|error| format!("Unable to query pending Album verification: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to collect pending Album verification: {error}"))?;
    drop(statement);
    let mut rows = Vec::with_capacity(base_rows.len());
    for (album_key, operation, revision, album_path, cover_path) in base_rows {
        let mut tracks = connection
            .prepare(
                "SELECT path FROM tracks WHERE album_key=? COLLATE NOCASE ORDER BY path COLLATE NOCASE",
            )
            .map_err(|error| format!("Unable to prepare Album verification tracks: {error}"))?;
        let track_paths = tracks
            .query_map([&album_key], |row| row.get::<_, String>(0))
            .map_err(|error| format!("Unable to query Album verification tracks: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to collect Album verification tracks: {error}"))?;
        rows.push(PendingWebguiVerification {
            album_key,
            operation,
            revision,
            album_path,
            cover_path,
            track_paths,
        });
    }
    drop(connection);

    let mut verified = 0usize;
    for pending in rows {
        let local_album = mapped_path(&mapper, &pending.album_path);
        let cover = readable_album_cover(
            &mapper,
            &pending.album_path,
            &pending.cover_path,
            &config.output.file_name,
        );
        let (readable_tracks, compliant) = compliant_tracks(
            &mapper,
            &pending.track_paths,
            config.output.embedded_thumbnail_size,
        );
        let physically_complete = match pending.operation.as_str() {
            "write-cover" => cover.is_some(),
            "embed-album" => {
                cover.is_some()
                    && !pending.track_paths.is_empty()
                    && readable_tracks == pending.track_paths.len()
                    && compliant == pending.track_paths.len()
            }
            "embed-comp-track" => readable_tracks > 0 && compliant > 0,
            _ => false,
        };
        if !physically_complete {
            continue;
        }
        let Some(local_album) = local_album else {
            continue;
        };
        let authoritative_update = if pending.operation == "embed-comp-track" {
            record_compilation_progress(config, &local_album, pending.track_paths.len(), compliant)
        } else {
            record_album_outcome(
                config,
                &local_album,
                if pending.operation == "embed-album" {
                    "installed"
                } else {
                    "processed"
                },
                None,
            )
        };
        if authoritative_update.is_err() {
            continue;
        }

        let mut connection = Connection::open(database_path)
            .map_err(|error| format!("Unable to persist Album verification: {error}"))?;
        connection
            .busy_timeout(Duration::from_secs(10))
            .map_err(|error| {
                format!("Unable to configure Album verification persistence: {error}")
            })?;
        ensure_sqlite_schema(&connection)?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("Unable to begin Album verification persistence: {error}"))?;
        let changed = transaction
            .execute(
                "UPDATE album_artwork_write_state SET physically_verified=1,verified_revision=revision,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') \
                 WHERE album_key=? COLLATE NOCASE AND writer='webgui' AND revision=? AND physically_verified=0",
                params![pending.album_key, pending.revision],
            )
            .map_err(|error| format!("Unable to acknowledge verified WebGUI artwork: {error}"))?;
        if changed > 0 {
            // The outcome refresh was captured behind the causal barrier.
            // This otherwise no-op update is captured only after Windows has
            // acknowledged the exact coordination revision.
            transaction
                .execute(
                    "UPDATE albums SET updated_at=updated_at WHERE album_key=? COLLATE NOCASE",
                    [&pending.album_key],
                )
                .map_err(|error| {
                    format!("Unable to release verified Album catalog state: {error}")
                })?;
            verified += 1;
        }
        transaction
            .commit()
            .map_err(|error| format!("Unable to commit Album verification persistence: {error}"))?;
    }
    Ok(verified)
}

pub fn local_writer(database_path: &Path, album_key: &str) -> Result<Option<String>, String> {
    let connection = Connection::open(database_path)
        .map_err(|error| format!("Unable to read Album coordination state: {error}"))?;
    ensure_sqlite_schema(&connection)?;
    let identity = canonical_album_identity(album_key)?;
    connection
        .query_row(
            "SELECT writer FROM album_artwork_write_state WHERE album_identity=?",
            [identity],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("Unable to read Album coordination writer: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_cover_verifier_accepts_jpeg_png_and_webp() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_string_lossy().into_owned();
        let mapper = PathMapper::new(&root, &root).unwrap();
        for extension in ["jpg", "png", "webp"] {
            let path = directory.path().join(format!("cover.{extension}"));
            image::RgbImage::from_pixel(8, 8, image::Rgb([12, 34, 56]))
                .save(&path)
                .unwrap();
            assert_eq!(
                readable_album_cover(&mapper, &root, "", "cover"),
                Some(path.clone())
            );
            fs::remove_file(path).unwrap();
        }
        assert_eq!(readable_album_cover(&mapper, &root, "", "cover"), None);
    }

    #[test]
    fn identity_is_exact_except_for_bom_and_case() {
        assert_eq!(
            canonical_album_identity("\u{feff}MBID:ABC-123").unwrap(),
            "key:mbid:abc-123"
        );
        assert_ne!(
            canonical_album_identity("tag:a-b").unwrap(),
            canonical_album_identity("tag:a b").unwrap()
        );
    }

    #[test]
    fn local_outbox_is_durable_and_immediately_owns_the_card() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("splined.db");
        let mut connection = Connection::open(&path).unwrap();
        ensure_sqlite_schema(&connection).unwrap();
        let transaction = connection.transaction().unwrap();
        queue_windows_album_write(
            &transaction,
            "mbid:album",
            "album",
            "/music",
            "/music/Artist/Album",
            "write-cover",
            "2026-11-04T00:00:00Z",
        )
        .unwrap();
        transaction.commit().unwrap();
        assert_eq!(
            effective_writers(&path).unwrap().get("mbid:album"),
            Some(&"windows".to_string())
        );
        let outbox: i64 = connection
            .query_row(
                "SELECT count(*) FROM album_artwork_write_outbox",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(outbox, 1);
    }

    #[tokio::test]
    async fn postgres_round_trip_transfers_last_writer_when_test_database_is_configured() {
        let Some(url) = coordination_database_url(&Config::default()).unwrap() else {
            return;
        };
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("splined.db");
        let mut connection = Connection::open(&path).unwrap();
        ensure_sqlite_schema(&connection).unwrap();
        let unique = source_completed_ms();
        let album_key = format!("mbid:coordination-test-{unique}");
        let album_identity = canonical_album_identity(&album_key).unwrap();
        let transaction = connection.transaction().unwrap();
        queue_windows_album_write(
            &transaction,
            &album_key,
            &format!("coordination-test-{unique}"),
            "E:/isolated/music",
            "E:/isolated/music/Test Artist/Test Album",
            "embed-album",
            "2026-11-04T00:00:00Z",
        )
        .unwrap();
        transaction.commit().unwrap();

        let pool = connect_pool(&url).await.unwrap();
        let first = synchronize(&pool, &path).await.unwrap();
        assert_eq!(first.get(&album_key), Some(&"windows".to_string()));
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM album_artwork_write_outbox",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );

        let web_ms = source_completed_ms() + 10;
        let web_event = format!("webgui-{unique}");
        sqlx::query(
            "INSERT INTO album_artwork_write_coordination(\
                album_identity,album_key,musicbrainz_albumid,relative_path,\
                writer,operation,event_id,source_completed_ms,completed_at) \
             VALUES($1,$2,$3,$4,'webgui','write-cover',$5,$6,\
                    to_timestamp($6::double precision / 1000.0)) \
             ON CONFLICT(album_identity) DO UPDATE SET \
                album_key=excluded.album_key,\
                musicbrainz_albumid=excluded.musicbrainz_albumid,\
                relative_path=excluded.relative_path,writer=excluded.writer,\
                operation=excluded.operation,event_id=excluded.event_id,\
                source_completed_ms=excluded.source_completed_ms,\
                revision=nextval('album_artwork_write_revision_seq'),\
                completed_at=excluded.completed_at,updated_at=now()",
        )
        .bind(&album_identity)
        .bind(&album_key)
        .bind(format!("coordination-test-{unique}"))
        .bind("test artist/test album")
        .bind(&web_event)
        .bind(web_ms)
        .execute(&pool)
        .await
        .unwrap();

        let second = synchronize(&pool, &path).await.unwrap();
        assert_eq!(second.get(&album_key), Some(&"webgui".to_string()));
        assert_eq!(
            local_writer(&path, &album_key).unwrap(),
            Some("webgui".to_string())
        );

        sqlx::query("DELETE FROM album_artwork_write_coordination WHERE album_identity=$1")
            .bind(&album_identity)
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }
}
