//! Durable, optional Windows/WebGUI Album artwork write coordination.
//!
//! SQLite remains the Windows catalog authority. PostgreSQL stores only the
//! last successful artwork writer so each client can render the opposing-
//! writer indicator. No operation here locks, approves, or changes an Album.

use crate::config::Config;
use crate::credential_store::load_album_coordination_database_url;
use rusqlite::{Connection, Transaction, params};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use std::collections::HashMap;
use std::path::Path;
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
CREATE TABLE IF NOT EXISTS comp_track_write_state (
    track_identity TEXT PRIMARY KEY,
    album_identity TEXT NOT NULL,
    album_key TEXT NOT NULL COLLATE NOCASE,
    relative_track_path TEXT NOT NULL COLLATE NOCASE,
    writer TEXT NOT NULL CHECK(writer IN ('windows', 'webgui')),
    operation TEXT NOT NULL,
    event_id TEXT NOT NULL UNIQUE,
    artwork_sha256 TEXT NOT NULL,
    source_completed_ms INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0,
    completed_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS comp_track_write_outbox (
    event_id TEXT PRIMARY KEY,
    track_identity TEXT NOT NULL,
    album_identity TEXT NOT NULL,
    album_key TEXT NOT NULL COLLATE NOCASE,
    relative_track_path TEXT NOT NULL COLLATE NOCASE,
    writer TEXT NOT NULL CHECK(writer = 'windows'),
    operation TEXT NOT NULL,
    artwork_sha256 TEXT NOT NULL,
    source_completed_ms INTEGER NOT NULL,
    completed_at TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_comp_track_write_state_path
    ON comp_track_write_state(relative_track_path);
CREATE INDEX IF NOT EXISTS idx_comp_track_write_outbox_order
    ON comp_track_write_outbox(source_completed_ms, event_id);
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlbumWriteState {
    pub writer: String,
    pub event_id: String,
    pub revision: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TrackCoordinationEvent {
    track_identity: String,
    album_identity: String,
    album_key: String,
    relative_track_path: String,
    writer: String,
    operation: String,
    event_id: String,
    artwork_sha256: String,
    source_completed_ms: i64,
    revision: i64,
    completed_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackWriteState {
    pub album_key: String,
    pub relative_track_path: String,
    pub writer: String,
    pub event_id: String,
    pub artwork_sha256: String,
    pub revision: i64,
}

pub fn ensure_sqlite_schema(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(SQLITE_SCHEMA)
        .map_err(|error| format!("Unable to prepare Album coordination state: {error}"))?;
    normalize_legacy_sqlite_coordination(connection)
}

pub fn canonical_album_identity(album_key: &str) -> Result<String, String> {
    let normalized = normalize_coordination_value(album_key);
    if normalized.is_empty() {
        return Err("Album coordination requires a canonical Album key".to_string());
    }
    Ok(format!("key:{normalized}"))
}

fn normalize_coordination_value(value: &str) -> String {
    value
        .nfkc()
        .case_fold()
        .filter(|character| {
            !matches!(
                *character,
                '\u{feff}' | '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}'
            )
        })
        .collect::<String>()
        .trim()
        .to_string()
}

fn normalize_relative_path(value: &str) -> String {
    normalize_coordination_value(&value.replace('\\', "/"))
        .trim_matches('/')
        .to_string()
}

pub fn canonical_track_identity(
    album_key: &str,
    relative_track_path: &str,
) -> Result<String, String> {
    let album = canonical_album_identity(album_key)?;
    let track = normalize_relative_path(relative_track_path);
    if track.is_empty() {
        return Err("Track coordination requires a path inside the music library".to_string());
    }
    Ok(format!("{album}|track:{track}"))
}

fn normalize_legacy_sqlite_coordination(connection: &Connection) -> Result<(), String> {
    let album_state = read_raw_events(connection, "album_artwork_write_state")?;
    let album_outbox = read_raw_events(connection, "album_artwork_write_outbox")?;
    let track_state = read_track_events(connection, "comp_track_write_state")?;
    let track_outbox = read_track_events(connection, "comp_track_write_outbox")?;

    let mut album_identity_counts = HashMap::<String, usize>::new();
    let mut needs_repair = false;
    for event in &album_state {
        let identity = canonical_album_identity(&event.album_key)?;
        *album_identity_counts.entry(identity.clone()).or_default() += 1;
        needs_repair |= event.album_identity != identity
            || event.album_key != normalize_coordination_value(&event.album_key)
            || event.musicbrainz_albumid
                != normalize_coordination_value(&event.musicbrainz_albumid)
            || event.relative_path != normalize_relative_path(&event.relative_path);
    }
    needs_repair |= album_identity_counts.values().any(|count| *count > 1);
    needs_repair |= album_outbox.iter().any(|event| {
        let album_key = normalize_coordination_value(&event.album_key);
        canonical_album_identity(&album_key)
            .map(|identity| {
                event.album_identity != identity
                    || event.album_key != album_key
                    || event.musicbrainz_albumid
                        != normalize_coordination_value(&event.musicbrainz_albumid)
                    || event.relative_path != normalize_relative_path(&event.relative_path)
            })
            .unwrap_or(true)
    });

    let mut track_identity_counts = HashMap::<String, usize>::new();
    for event in &track_state {
        let album_key = normalize_coordination_value(&event.album_key);
        let relative = normalize_relative_path(&event.relative_track_path);
        let identity = canonical_track_identity(&album_key, &relative)?;
        *track_identity_counts.entry(identity.clone()).or_default() += 1;
        needs_repair |= event.track_identity != identity
            || event.album_identity != canonical_album_identity(&album_key)?
            || event.album_key != album_key
            || event.relative_track_path != relative;
    }
    needs_repair |= track_identity_counts.values().any(|count| *count > 1);
    needs_repair |= track_outbox.iter().any(|event| {
        let album_key = normalize_coordination_value(&event.album_key);
        let relative = normalize_relative_path(&event.relative_track_path);
        canonical_track_identity(&album_key, &relative)
            .map(|identity| {
                event.track_identity != identity
                    || event.album_identity
                        != canonical_album_identity(&album_key).unwrap_or_default()
                    || event.album_key != album_key
                    || event.relative_track_path != relative
            })
            .unwrap_or(true)
    });
    if !needs_repair {
        return Ok(());
    }

    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| format!("Unable to begin coordination identity repair: {error}"))?;

    let mut album_groups = HashMap::<String, Vec<CoordinationEvent>>::new();
    for event in album_state {
        let identity = canonical_album_identity(&event.album_key)?;
        album_groups.entry(identity).or_default().push(event);
    }
    for (identity, group) in album_groups {
        let normalized_mbids = group
            .iter()
            .map(|event| normalize_coordination_value(&event.musicbrainz_albumid))
            .filter(|value| !value.is_empty())
            .collect::<std::collections::HashSet<_>>();
        if normalized_mbids.len() > 1 {
            return Err(format!(
                "Ambiguous Album coordination collision for {identity}; MusicBrainz identities differ"
            ));
        }
        let changed = group.len() > 1
            || group.iter().any(|event| {
                event.album_identity != identity
                    || event.album_key != normalize_coordination_value(&event.album_key)
                    || event.musicbrainz_albumid
                        != normalize_coordination_value(&event.musicbrainz_albumid)
            });
        if !changed {
            continue;
        }
        let mut selected = group
            .iter()
            .max_by_key(|event| (event.source_completed_ms, event.event_id.as_str()))
            .cloned()
            .expect("coordination group is not empty");
        selected.album_identity = identity;
        selected.album_key = normalize_coordination_value(&selected.album_key);
        selected.musicbrainz_albumid = normalize_coordination_value(&selected.musicbrainz_albumid);
        selected.relative_path = normalize_relative_path(&selected.relative_path);
        for event in &group {
            transaction
                .execute(
                    "DELETE FROM album_artwork_write_state WHERE album_identity=?",
                    [&event.album_identity],
                )
                .map_err(|error| format!("Unable to remove legacy Album identity: {error}"))?;
        }
        transaction.execute(
            "INSERT INTO album_artwork_write_state(album_identity,album_key,musicbrainz_albumid,relative_path,writer,operation,event_id,source_completed_ms,revision,completed_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
            params![selected.album_identity,selected.album_key,selected.musicbrainz_albumid,selected.relative_path,selected.writer,selected.operation,selected.event_id,selected.source_completed_ms,selected.revision,selected.completed_at],
        ).map_err(|error| format!("Unable to normalize legacy Album identity: {error}"))?;
    }
    for mut event in album_outbox {
        event.album_key = normalize_coordination_value(&event.album_key);
        event.musicbrainz_albumid = normalize_coordination_value(&event.musicbrainz_albumid);
        event.album_identity = canonical_album_identity(&event.album_key)?;
        event.relative_path = normalize_relative_path(&event.relative_path);
        transaction.execute(
            "UPDATE album_artwork_write_outbox SET album_identity=?,album_key=?,musicbrainz_albumid=?,relative_path=? WHERE event_id=?",
            params![event.album_identity,event.album_key,event.musicbrainz_albumid,event.relative_path,event.event_id],
        ).map_err(|error| format!("Unable to normalize pending Album identity: {error}"))?;
    }

    let mut track_groups = HashMap::<String, Vec<TrackCoordinationEvent>>::new();
    for event in track_state {
        let relative = normalize_relative_path(&event.relative_track_path);
        let identity = canonical_track_identity(&event.album_key, &relative)?;
        track_groups.entry(identity).or_default().push(event);
    }
    for (identity, group) in track_groups {
        let changed = group.len() > 1
            || group.iter().any(|event| {
                event.track_identity != identity
                    || event.album_key != normalize_coordination_value(&event.album_key)
                    || event.relative_track_path
                        != normalize_relative_path(&event.relative_track_path)
            });
        if !changed {
            continue;
        }
        let mut selected = group
            .iter()
            .max_by_key(|event| (event.source_completed_ms, event.event_id.as_str()))
            .cloned()
            .expect("coordination group is not empty");
        selected.album_key = normalize_coordination_value(&selected.album_key);
        selected.album_identity = canonical_album_identity(&selected.album_key)?;
        selected.relative_track_path = normalize_relative_path(&selected.relative_track_path);
        selected.track_identity = identity;
        for event in &group {
            transaction
                .execute(
                    "DELETE FROM comp_track_write_state WHERE track_identity=?",
                    [&event.track_identity],
                )
                .map_err(|error| format!("Unable to remove legacy COMP track identity: {error}"))?;
        }
        transaction.execute(
            "INSERT INTO comp_track_write_state(track_identity,album_identity,album_key,relative_track_path,writer,operation,event_id,artwork_sha256,source_completed_ms,revision,completed_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
            params![selected.track_identity,selected.album_identity,selected.album_key,selected.relative_track_path,selected.writer,selected.operation,selected.event_id,selected.artwork_sha256,selected.source_completed_ms,selected.revision,selected.completed_at],
        ).map_err(|error| format!("Unable to normalize legacy COMP track identity: {error}"))?;
    }
    for mut event in track_outbox {
        event.album_key = normalize_coordination_value(&event.album_key);
        event.album_identity = canonical_album_identity(&event.album_key)?;
        event.relative_track_path = normalize_relative_path(&event.relative_track_path);
        event.track_identity =
            canonical_track_identity(&event.album_key, &event.relative_track_path)?;
        transaction.execute(
            "UPDATE comp_track_write_outbox SET track_identity=?,album_identity=?,album_key=?,relative_track_path=? WHERE event_id=?",
            params![event.track_identity,event.album_identity,event.album_key,event.relative_track_path,event.event_id],
        ).map_err(|error| format!("Unable to normalize pending COMP track identity: {error}"))?;
    }
    transaction
        .commit()
        .map_err(|error| format!("Unable to commit coordination identity repair: {error}"))
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
    normalize_relative_path(
        album
            .strip_prefix(&root)
            .unwrap_or("")
            .trim_start_matches('/'),
    )
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
    let album_key = normalize_coordination_value(album_key);
    let musicbrainz_albumid = normalize_coordination_value(musicbrainz_albumid);
    let album_identity = canonical_album_identity(&album_key)?;
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
            "INSERT INTO album_artwork_write_state(album_identity,album_key,musicbrainz_albumid,relative_path,writer,operation,event_id,source_completed_ms,revision,completed_at,updated_at) \
             VALUES(?,?,?,?,'windows',?,?,?,0,?,?) \
             ON CONFLICT(album_identity) DO UPDATE SET album_key=excluded.album_key,musicbrainz_albumid=excluded.musicbrainz_albumid,relative_path=excluded.relative_path,writer=excluded.writer,operation=excluded.operation,event_id=excluded.event_id,source_completed_ms=excluded.source_completed_ms,revision=0,completed_at=excluded.completed_at,updated_at=excluded.updated_at \
             WHERE excluded.source_completed_ms > album_artwork_write_state.source_completed_ms \
                OR (excluded.source_completed_ms = album_artwork_write_state.source_completed_ms AND excluded.event_id > album_artwork_write_state.event_id)",
            params![album_identity, album_key, musicbrainz_albumid, relative_path,
                operation, event_id, completed_ms, now, now],
        )
        .map_err(|error| format!("Unable to persist local Album writer: {error}"))?;
    Ok(event_id)
}

/// Queue a verified Windows COMP-track write without changing Album authority.
pub fn queue_windows_comp_track_write(
    transaction: &Transaction<'_>,
    album_key: &str,
    canonical_root: &str,
    canonical_track: &str,
    artwork_sha256: &str,
    operation: &str,
    now: &str,
) -> Result<String, String> {
    let album_key = normalize_coordination_value(album_key);
    let album_identity = canonical_album_identity(&album_key)?;
    let relative_track_path = relative_path(canonical_root, canonical_track);
    let track_identity = canonical_track_identity(&album_key, &relative_track_path)?;
    let completed_ms = source_completed_ms();
    let event_id = event_id(completed_ms);
    transaction
        .execute(
            "INSERT INTO comp_track_write_outbox(event_id,track_identity,album_identity,album_key,relative_track_path,writer,operation,artwork_sha256,source_completed_ms,completed_at,created_at) \
             VALUES(?,?,?,?,?,'windows',?,?,?,?,?)",
            params![event_id, track_identity, album_identity, album_key, relative_track_path,
                operation, artwork_sha256.to_ascii_lowercase(), completed_ms, now, now],
        )
        .map_err(|error| format!("Unable to queue COMP track write notification: {error}"))?;
    transaction
        .execute(
            "INSERT INTO comp_track_write_state(track_identity,album_identity,album_key,relative_track_path,writer,operation,event_id,artwork_sha256,source_completed_ms,revision,completed_at,updated_at) \
             VALUES(?,?,?,?,'windows',?,?,?, ?,0,?,?) \
             ON CONFLICT(track_identity) DO UPDATE SET album_identity=excluded.album_identity,album_key=excluded.album_key,relative_track_path=excluded.relative_track_path,writer=excluded.writer,operation=excluded.operation,event_id=excluded.event_id,artwork_sha256=excluded.artwork_sha256,source_completed_ms=excluded.source_completed_ms,revision=0,completed_at=excluded.completed_at,updated_at=excluded.updated_at \
             WHERE excluded.source_completed_ms > comp_track_write_state.source_completed_ms \
                OR (excluded.source_completed_ms = comp_track_write_state.source_completed_ms AND excluded.event_id > comp_track_write_state.event_id)",
            params![track_identity, album_identity, album_key, relative_track_path,
                operation, event_id, artwork_sha256.to_ascii_lowercase(), completed_ms, now, now],
        )
        .map_err(|error| format!("Unable to persist local COMP track writer: {error}"))?;
    Ok(event_id)
}

fn read_raw_events(connection: &Connection, table: &str) -> Result<Vec<CoordinationEvent>, String> {
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

fn read_events(connection: &Connection, table: &str) -> Result<Vec<CoordinationEvent>, String> {
    let mut events = read_raw_events(connection, table)?;
    for event in &mut events {
        event.album_key = normalize_coordination_value(&event.album_key);
        event.musicbrainz_albumid = normalize_coordination_value(&event.musicbrainz_albumid);
        event.album_identity = canonical_album_identity(&event.album_key)?;
        event.relative_path = normalize_relative_path(&event.relative_path);
    }
    Ok(events)
}

fn read_track_events(
    connection: &Connection,
    table: &str,
) -> Result<Vec<TrackCoordinationEvent>, String> {
    let sql = format!(
        "SELECT track_identity,album_identity,album_key,relative_track_path,writer,operation,event_id,artwork_sha256,source_completed_ms,{} AS revision,completed_at FROM {table} ORDER BY source_completed_ms,event_id",
        if table == "comp_track_write_state" {
            "revision"
        } else {
            "0"
        }
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| format!("Unable to read COMP track coordination state: {error}"))?;
    statement
        .query_map([], |row| {
            Ok(TrackCoordinationEvent {
                track_identity: row.get(0)?,
                album_identity: row.get(1)?,
                album_key: row.get(2)?,
                relative_track_path: row.get(3)?,
                writer: row.get(4)?,
                operation: row.get(5)?,
                event_id: row.get(6)?,
                artwork_sha256: row.get(7)?,
                source_completed_ms: row.get(8)?,
                revision: row.get(9)?,
                completed_at: row.get(10)?,
            })
        })
        .map_err(|error| format!("Unable to query COMP track coordination state: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to collect COMP track coordination state: {error}"))
}

fn newer(left: &CoordinationEvent, right: &CoordinationEvent) -> bool {
    (left.source_completed_ms, left.event_id.as_str())
        > (right.source_completed_ms, right.event_id.as_str())
}

fn newer_track(left: &TrackCoordinationEvent, right: &TrackCoordinationEvent) -> bool {
    (left.source_completed_ms, left.event_id.as_str())
        > (right.source_completed_ms, right.event_id.as_str())
}

pub fn effective_write_states(
    database_path: &Path,
) -> Result<HashMap<String, AlbumWriteState>, String> {
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
        .map(|event| {
            (
                normalize_coordination_value(&event.album_key),
                AlbumWriteState {
                    writer: event.writer,
                    event_id: event.event_id,
                    revision: event.revision,
                },
            )
        })
        .collect())
}

pub fn effective_writers(database_path: &Path) -> Result<HashMap<String, String>, String> {
    Ok(effective_write_states(database_path)?
        .into_iter()
        .map(|(album_key, state)| (album_key, state.writer))
        .collect())
}

pub fn effective_track_write_states(
    database_path: &Path,
) -> Result<HashMap<String, TrackWriteState>, String> {
    let connection = Connection::open(database_path)
        .map_err(|error| format!("Unable to open COMP track coordination state: {error}"))?;
    connection
        .busy_timeout(Duration::from_secs(10))
        .map_err(|error| format!("Unable to configure COMP track coordination state: {error}"))?;
    ensure_sqlite_schema(&connection)?;
    let mut current = HashMap::<String, TrackCoordinationEvent>::new();
    for mut event in read_track_events(&connection, "comp_track_write_state")?
        .into_iter()
        .chain(read_track_events(&connection, "comp_track_write_outbox")?)
    {
        event.album_key = normalize_coordination_value(&event.album_key);
        event.album_identity = canonical_album_identity(&event.album_key)?;
        event.relative_track_path = normalize_relative_path(&event.relative_track_path);
        event.track_identity =
            canonical_track_identity(&event.album_key, &event.relative_track_path)?;
        let replace = current
            .get(&event.track_identity)
            .is_none_or(|prior| newer_track(&event, prior));
        if replace {
            current.insert(event.track_identity.clone(), event);
        }
    }
    Ok(current
        .into_values()
        .map(|event| {
            (
                event.relative_track_path.clone(),
                TrackWriteState {
                    album_key: event.album_key,
                    relative_track_path: event.relative_track_path,
                    writer: event.writer,
                    event_id: event.event_id,
                    artwork_sha256: event.artwork_sha256,
                    revision: event.revision,
                },
            )
        })
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
            "INSERT INTO album_artwork_write_state(album_identity,album_key,musicbrainz_albumid,relative_path,writer,operation,event_id,source_completed_ms,revision,completed_at,updated_at) \
             VALUES(?,?,?,?,?,?,?,?,?,?,strftime('%Y-%m-%dT%H:%M:%SZ','now')) \
             ON CONFLICT(album_identity) DO UPDATE SET album_key=excluded.album_key,musicbrainz_albumid=excluded.musicbrainz_albumid,relative_path=excluded.relative_path,writer=excluded.writer,operation=excluded.operation,event_id=excluded.event_id,source_completed_ms=excluded.source_completed_ms,revision=excluded.revision,completed_at=excluded.completed_at,updated_at=excluded.updated_at \
             WHERE excluded.source_completed_ms > album_artwork_write_state.source_completed_ms \
                OR (excluded.source_completed_ms = album_artwork_write_state.source_completed_ms AND excluded.event_id >= album_artwork_write_state.event_id)",
            params![event.album_identity,event.album_key,event.musicbrainz_albumid,event.relative_path,
                event.writer,event.operation,event.event_id,event.source_completed_ms,event.revision,event.completed_at],
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

/// Publish Windows COMP-track outbox rows and pull the durable cross-client
/// last-writer projection. Album coordination remains completely separate.
pub async fn synchronize_tracks(
    pool: &PgPool,
    database_path: &Path,
) -> Result<HashMap<String, TrackWriteState>, String> {
    let connection = Connection::open(database_path)
        .map_err(|error| format!("Unable to open COMP track coordination outbox: {error}"))?;
    connection
        .busy_timeout(Duration::from_secs(10))
        .map_err(|error| format!("Unable to configure COMP track coordination outbox: {error}"))?;
    ensure_sqlite_schema(&connection)?;
    let pending = read_track_events(&connection, "comp_track_write_outbox")?;
    drop(connection);

    let mut pg = pool
        .begin()
        .await
        .map_err(|error| format!("Unable to begin COMP track coordination: {error}"))?;
    for event in &pending {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(&event.track_identity)
            .execute(&mut *pg)
            .await
            .map_err(|error| format!("Unable to lock COMP track coordination row: {error}"))?;
        sqlx::query(
            "INSERT INTO comp_track_write_coordination(track_identity,album_identity,album_key,track_key,relative_track_path,writer,operation,event_id,artwork_sha256,source_completed_ms,completed_at) \
             VALUES($1,$2,$3,'',$4,$5,$6,$7,$8,$9,to_timestamp($9::double precision / 1000.0)) \
             ON CONFLICT(track_identity) DO UPDATE SET album_identity=excluded.album_identity,album_key=excluded.album_key,track_key=excluded.track_key,relative_track_path=excluded.relative_track_path,writer=excluded.writer,operation=excluded.operation,event_id=excluded.event_id,artwork_sha256=excluded.artwork_sha256,source_completed_ms=excluded.source_completed_ms,revision=nextval('comp_track_write_revision_seq'),completed_at=excluded.completed_at,updated_at=now() \
             WHERE excluded.source_completed_ms > comp_track_write_coordination.source_completed_ms \
                OR (excluded.source_completed_ms = comp_track_write_coordination.source_completed_ms AND excluded.event_id > comp_track_write_coordination.event_id)",
        )
        .bind(&event.track_identity)
        .bind(&event.album_identity)
        .bind(&event.album_key)
        .bind(&event.relative_track_path)
        .bind(&event.writer)
        .bind(&event.operation)
        .bind(&event.event_id)
        .bind(&event.artwork_sha256)
        .bind(event.source_completed_ms)
        .execute(&mut *pg)
        .await
        .map_err(|error| format!("Unable to publish COMP track coordination event: {error}"))?;
    }
    let rows = sqlx::query(
        "SELECT track_identity,album_identity,album_key,relative_track_path,writer,operation,event_id,artwork_sha256,source_completed_ms,revision,completed_at::text FROM comp_track_write_coordination ORDER BY revision",
    )
    .fetch_all(&mut *pg)
    .await
    .map_err(|error| format!("Unable to read COMP track coordination state: {error}"))?;
    pg.commit()
        .await
        .map_err(|error| format!("Unable to commit COMP track coordination: {error}"))?;

    let mut connection = Connection::open(database_path)
        .map_err(|error| format!("Unable to persist COMP track coordination state: {error}"))?;
    connection
        .busy_timeout(Duration::from_secs(10))
        .map_err(|error| {
            format!("Unable to configure COMP track coordination persistence: {error}")
        })?;
    ensure_sqlite_schema(&connection)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Unable to begin COMP track coordination persistence: {error}"))?;
    for row in rows {
        let event = TrackCoordinationEvent {
            track_identity: row.try_get(0).map_err(|error| error.to_string())?,
            album_identity: row.try_get(1).map_err(|error| error.to_string())?,
            album_key: row.try_get(2).map_err(|error| error.to_string())?,
            relative_track_path: row.try_get(3).map_err(|error| error.to_string())?,
            writer: row.try_get(4).map_err(|error| error.to_string())?,
            operation: row.try_get(5).map_err(|error| error.to_string())?,
            event_id: row.try_get(6).map_err(|error| error.to_string())?,
            artwork_sha256: row.try_get(7).map_err(|error| error.to_string())?,
            source_completed_ms: row.try_get(8).map_err(|error| error.to_string())?,
            revision: row.try_get(9).map_err(|error| error.to_string())?,
            completed_at: row.try_get(10).map_err(|error| error.to_string())?,
        };
        transaction.execute(
            "INSERT INTO comp_track_write_state(track_identity,album_identity,album_key,relative_track_path,writer,operation,event_id,artwork_sha256,source_completed_ms,revision,completed_at,updated_at) \
             VALUES(?,?,?,?,?,?,?,?,?,?,?,strftime('%Y-%m-%dT%H:%M:%SZ','now')) \
             ON CONFLICT(track_identity) DO UPDATE SET album_identity=excluded.album_identity,album_key=excluded.album_key,relative_track_path=excluded.relative_track_path,writer=excluded.writer,operation=excluded.operation,event_id=excluded.event_id,artwork_sha256=excluded.artwork_sha256,source_completed_ms=excluded.source_completed_ms,revision=excluded.revision,completed_at=excluded.completed_at,updated_at=excluded.updated_at \
             WHERE excluded.source_completed_ms > comp_track_write_state.source_completed_ms \
                OR (excluded.source_completed_ms = comp_track_write_state.source_completed_ms AND excluded.event_id >= comp_track_write_state.event_id)",
            params![event.track_identity,event.album_identity,event.album_key,event.relative_track_path,
                event.writer,event.operation,event.event_id,event.artwork_sha256,event.source_completed_ms,event.revision,event.completed_at],
        ).map_err(|error| format!("Unable to acknowledge COMP track coordination row: {error}"))?;
    }
    for event in &pending {
        transaction
            .execute(
                "DELETE FROM comp_track_write_outbox WHERE event_id=?",
                [&event.event_id],
            )
            .map_err(|error| {
                format!("Unable to acknowledge COMP track coordination outbox: {error}")
            })?;
    }
    transaction.commit().map_err(|error| {
        format!("Unable to commit COMP track coordination persistence: {error}")
    })?;
    effective_track_write_states(database_path)
}

pub fn local_writer(database_path: &Path, album_key: &str) -> Result<Option<String>, String> {
    Ok(effective_write_states(database_path)?
        .get(&normalize_coordination_value(album_key))
        .map(|state| state.writer.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn queued_album_write_persists_normalized_key_and_mbid() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("splined.db");
        let mut connection = Connection::open(&path).unwrap();
        ensure_sqlite_schema(&connection).unwrap();
        let transaction = connection.transaction().unwrap();
        queue_windows_album_write(
            &transaction,
            "mbid:\u{feff}ABC-123",
            "\u{200b}ABC-123",
            "/music",
            "/music/Artist/Album",
            "write-cover",
            "2026-11-06T00:00:00Z",
        )
        .unwrap();
        transaction.commit().unwrap();
        let values = connection
            .query_row(
                "SELECT album_identity,album_key,musicbrainz_albumid FROM album_artwork_write_outbox",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
            )
            .unwrap();
        assert_eq!(
            values,
            (
                "key:mbid:abc-123".into(),
                "mbid:abc-123".into(),
                "abc-123".into()
            )
        );
    }

    #[test]
    fn legacy_bom_state_and_pending_outbox_are_repaired_without_loss() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("splined.db");
        let connection = Connection::open(&path).unwrap();
        ensure_sqlite_schema(&connection).unwrap();
        connection
            .execute(
                "INSERT INTO album_artwork_write_state VALUES(?,?,?,?,?,?,?,?,?,?,?)",
                params![
                    "key:mbid:\u{feff}album",
                    "mbid:\u{feff}album",
                    "\u{feff}album",
                    "artist/album",
                    "windows",
                    "write-cover",
                    "legacy-state",
                    10_i64,
                    4_i64,
                    "2026-11-06T00:00:00Z",
                    "2026-11-06T00:00:00Z"
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO album_artwork_write_outbox VALUES(?,?,?,?,?,?,?,?,?,?)",
                params![
                    "legacy-outbox",
                    "key:mbid:\u{feff}album",
                    "mbid:\u{feff}album",
                    "\u{feff}album",
                    "artist/album",
                    "windows",
                    "write-cover",
                    11_i64,
                    "2026-11-06T00:00:01Z",
                    "2026-11-06T00:00:01Z"
                ],
            )
            .unwrap();
        let states = effective_write_states(&path).unwrap();
        assert_eq!(states.get("mbid:album").unwrap().event_id, "legacy-outbox");
        assert_eq!(
            connection
                .query_row(
                    "SELECT album_identity FROM album_artwork_write_state",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "key:mbid:album"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT album_key FROM album_artwork_write_outbox",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "mbid:album"
        );
    }

    #[test]
    fn comp_track_write_is_track_scoped_and_durable() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("splined.db");
        let mut connection = Connection::open(&path).unwrap();
        ensure_sqlite_schema(&connection).unwrap();
        let transaction = connection.transaction().unwrap();
        queue_windows_comp_track_write(
            &transaction,
            "tag:Various Artists|COMP",
            "E:/music",
            "E:/music/Various Artists/COMP/one.flac",
            "abc123",
            "embed-comp-track",
            "2026-11-06T00:00:00Z",
        )
        .unwrap();
        transaction.commit().unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM album_artwork_write_outbox",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM comp_track_write_outbox", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            1
        );
        let states = effective_track_write_states(&path).unwrap();
        let state = states.get("various artists/comp/one.flac").unwrap();
        assert_eq!(state.writer, "windows");
        assert_eq!(state.artwork_sha256, "abc123");
    }

    #[test]
    fn ambiguous_legacy_album_identity_collision_is_reported() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("splined.db");
        let connection = Connection::open(&path).unwrap();
        ensure_sqlite_schema(&connection).unwrap();
        for (identity, mbid, event, completed) in [
            ("key:mbid:album", "one", "one", 1_i64),
            ("key:mbid:\u{feff}album", "two", "two", 2_i64),
        ] {
            connection
                .execute(
                    "INSERT INTO album_artwork_write_state VALUES(?,?,?,?,?,?,?,?,?,?,?)",
                    params![
                        identity,
                        "mbid:album",
                        mbid,
                        "artist/album",
                        "windows",
                        "write-cover",
                        event,
                        completed,
                        1_i64,
                        "2026-11-06T00:00:00Z",
                        "2026-11-06T00:00:00Z"
                    ],
                )
                .unwrap();
        }
        assert!(
            effective_write_states(&path)
                .unwrap_err()
                .contains("Ambiguous Album coordination collision")
        );
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
