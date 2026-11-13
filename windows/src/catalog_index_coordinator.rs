//! Durable SQLite coordination for catalog index construction.
//!
//! The live `artists`/`albums`/`tracks` tables always represent the last
//! published generation.  Workers checkpoint one Album at a time into the
//! generation tables and may publish only while they still own the lease.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
thread_local! {
    static VALIDATION_TEST_INTERRUPTION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn interrupt_next_validation_for_test() {
    VALIDATION_TEST_INTERRUPTION.set(true);
}

#[cfg(test)]
fn validation_test_interrupted() -> bool {
    VALIDATION_TEST_INTERRUPTION.replace(false)
}

#[cfg(not(test))]
fn validation_test_interrupted() -> bool {
    false
}

const LEASE_SECONDS: i64 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogGenerationLease {
    pub generation_id: String,
    pub owner_token: String,
    pub scan_token: String,
    pub mode: String,
    pub resumed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedAlbumCheckpoint {
    pub album_path: String,
    pub album_key: String,
    pub artist_key: String,
    pub inventory_signature: String,
    pub artwork_signature: String,
    pub tag_signature: String,
    pub track_count: i64,
    pub total_size: i64,
    pub newest_mtime_ns: i64,
    pub disposition: String,
    pub artist_payload_json: Option<String>,
    pub album_payload_json: Option<String>,
    pub tracks_payload_json: Option<String>,
    pub review_payload_json: Option<String>,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogIndexStatus {
    pub active_generation_id: String,
    pub maintenance_generation_id: String,
    pub catalog_state: String,
    pub generation_state: String,
    pub phase: String,
    pub current_path: String,
    pub discovered_albums: i64,
    pub completed_albums: i64,
    pub changed_albums: i64,
    pub unchanged_albums: i64,
    pub removed_albums: i64,
    pub error_count: i64,
    pub change_summary: Value,
}

fn unique_token(prefix: &str, library_root: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "{prefix}-{}-{nanos}-{:016x}",
        process::id(),
        stable_hash(library_root.as_bytes())
    )
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut value = 0xcbf29ce484222325_u64;
    for byte in bytes {
        value ^= u64::from(*byte);
        value = value.wrapping_mul(0x100000001b3);
    }
    value
}

fn sqlite_error(action: &'static str) -> impl FnOnce(rusqlite::Error) -> String {
    move |error| format!("Unable to {action}: {error}")
}

pub fn claim_generation(
    connection: &mut Connection,
    mode: &str,
    library_root: &str,
) -> Result<CatalogGenerationLease, String> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error("start Catalog Index Coordinator claim"))?;
    let active = transaction
        .query_row(
            "SELECT generation_id,mode,library_root,lease_expires_at,
                    CASE WHEN julianday(lease_expires_at)>julianday('now') THEN 1 ELSE 0 END
             FROM catalog_generations
             WHERE backend='sqlite' AND state IN ('building','validating','publishing')
             ORDER BY created_at DESC LIMIT 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)? != 0,
                ))
            },
        )
        .optional()
        .map_err(sqlite_error("read active catalog generation"))?;
    if active.as_ref().is_some_and(|value| value.4) {
        let value = active.as_ref().expect("checked active generation");
        return Err(format!(
            "Catalog index generation {} is already active until {}.",
            value.0, value.3
        ));
    }

    let owner_token = unique_token("owner", library_root);
    let scan_token = unique_token("scan", library_root);
    let (generation_id, resumed) =
        if let Some((generation_id, saved_mode, saved_root, _, _)) = active {
            if saved_mode == mode && saved_root.eq_ignore_ascii_case(library_root) {
                let changed = transaction
                    .execute(
                        "UPDATE catalog_generations SET
                         state='building',owner_token=?1,
                         lease_expires_at=strftime('%Y-%m-%dT%H:%M:%fZ','now',?2),
                         discovered_albums=0,completed_albums=0,changed_albums=0,
                         unchanged_albums=0,removed_albums=0,error_count=0,
                         current_path='',phase='enumerating',error='',
                         updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')
                     WHERE generation_id=?3 AND backend='sqlite'",
                        params![
                            owner_token,
                            format!("+{LEASE_SECONDS} seconds"),
                            generation_id
                        ],
                    )
                    .map_err(sqlite_error("resume expired catalog generation"))?;
                if changed != 1 {
                    return Err("Expired catalog generation could not be resumed.".to_string());
                }
                (generation_id, true)
            } else {
                transaction
                    .execute(
                        "UPDATE catalog_generations SET state='abandoned',phase='abandoned',
                         error='Superseded after lease expiration',
                         updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')
                     WHERE generation_id=?1",
                        [&generation_id],
                    )
                    .map_err(sqlite_error("abandon mismatched expired generation"))?;
                (unique_token("sqlite-generation", library_root), false)
            }
        } else {
            (unique_token("sqlite-generation", library_root), false)
        };

    if !resumed {
        let base_generation_id = transaction
            .query_row(
                "SELECT active_generation_id FROM catalog_index_state WHERE singleton=1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sqlite_error("read published generation marker"))?
            .unwrap_or_default();
        transaction
            .execute(
                "INSERT INTO catalog_generations(
                     generation_id,backend,mode,state,base_generation_id,library_root,
                     owner_token,lease_expires_at,phase,created_at,updated_at
                 ) VALUES(?1,'sqlite',?2,'building',?3,?4,?5,
                     strftime('%Y-%m-%dT%H:%M:%fZ','now',?6),'enumerating',
                     strftime('%Y-%m-%dT%H:%M:%fZ','now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
                params![
                    generation_id,
                    mode,
                    base_generation_id,
                    library_root,
                    owner_token,
                    format!("+{LEASE_SECONDS} seconds")
                ],
            )
            .map_err(sqlite_error("create catalog generation"))?;
    }
    transaction
        .execute(
            "INSERT INTO catalog_index_state(singleton,maintenance_generation_id,catalog_state,updated_at)
             VALUES(1,?1,'maintenance',strftime('%Y-%m-%dT%H:%M:%fZ','now'))
             ON CONFLICT(singleton) DO UPDATE SET
                 maintenance_generation_id=excluded.maintenance_generation_id,
                 catalog_state='maintenance',updated_at=excluded.updated_at",
            [&generation_id],
        )
        .map_err(sqlite_error("mark catalog maintenance generation"))?;
    transaction
        .commit()
        .map_err(sqlite_error("commit Catalog Index Coordinator claim"))?;
    Ok(CatalogGenerationLease {
        generation_id,
        owner_token,
        scan_token,
        mode: mode.to_string(),
        resumed,
    })
}

pub fn reusable_checkpoint(
    connection: &Connection,
    lease: &CatalogGenerationLease,
    album_path: &str,
    inventory_signature: &str,
) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT 1 FROM catalog_generation_albums
             WHERE generation_id=?1 AND lower(album_path)=lower(?2)
               AND inventory_signature=?3 AND disposition<>'error'",
            params![lease.generation_id, album_path, inventory_signature],
            |_| Ok(true),
        )
        .optional()
        .map(|value| value.unwrap_or(false))
        .map_err(sqlite_error("read catalog Album checkpoint"))
}

pub fn reusable_checkpoints(
    connection: &Connection,
    lease: &CatalogGenerationLease,
) -> Result<Vec<(String, String)>, String> {
    let mut statement = connection
        .prepare(
            "SELECT album_path,inventory_signature FROM catalog_generation_albums
             WHERE generation_id=?1 AND disposition<>'error'",
        )
        .map_err(sqlite_error("prepare reusable Album checkpoints"))?;
    let rows = statement
        .query_map([&lease.generation_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(sqlite_error("read reusable Album checkpoints"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_error("decode reusable Album checkpoints"))
}

pub fn touch_reusable_checkpoint(
    connection: &mut Connection,
    lease: &CatalogGenerationLease,
    album_path: &str,
) -> Result<(), String> {
    touch_reusable_checkpoints(connection, lease, &[album_path.to_string()])
}

pub fn touch_reusable_checkpoints(
    connection: &mut Connection,
    lease: &CatalogGenerationLease,
    album_paths: &[String],
) -> Result<(), String> {
    if album_paths.is_empty() {
        return Ok(());
    }
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error("start catalog checkpoint reuse"))?;
    assert_lease(&transaction, lease)?;
    for album_path in album_paths {
        let disposition = transaction
            .query_row(
                "SELECT disposition FROM catalog_generation_albums
                 WHERE generation_id=?1 AND lower(album_path)=lower(?2)",
                params![lease.generation_id, album_path],
                |row| row.get::<_, String>(0),
            )
            .map_err(sqlite_error("read reusable Album disposition"))?;
        transaction
            .execute(
                "UPDATE catalog_generation_albums SET scan_token=?1,
                     completed_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')
                 WHERE generation_id=?2 AND lower(album_path)=lower(?3)",
                params![lease.scan_token, lease.generation_id, album_path],
            )
            .map_err(sqlite_error("reuse catalog Album checkpoint"))?;
        increment_progress(&transaction, lease, album_path, &disposition, false)?;
    }
    transaction
        .commit()
        .map_err(sqlite_error("commit catalog checkpoint reuse batch"))
}

pub fn stage_album(
    connection: &mut Connection,
    lease: &CatalogGenerationLease,
    checkpoint: &StagedAlbumCheckpoint,
) -> Result<(), String> {
    stage_albums(connection, lease, std::slice::from_ref(checkpoint))
}

pub fn heartbeat(
    connection: &Connection,
    lease: &CatalogGenerationLease,
    current_path: &str,
) -> Result<(), String> {
    let updated = connection
        .execute(
            "UPDATE catalog_generations SET phase='indexing',current_path=?1,
             lease_expires_at=strftime('%Y-%m-%dT%H:%M:%fZ','now',?2),
             updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE generation_id=?3 AND owner_token=?4 AND state='building'",
            params![
                current_path,
                format!("+{LEASE_SECONDS} seconds"),
                lease.generation_id,
                lease.owner_token,
            ],
        )
        .map_err(sqlite_error("heartbeat catalog generation"))?;
    if updated != 1 {
        return Err("Catalog generation heartbeat lost its lease.".to_string());
    }
    Ok(())
}

pub fn stage_albums(
    connection: &mut Connection,
    lease: &CatalogGenerationLease,
    checkpoints: &[StagedAlbumCheckpoint],
) -> Result<(), String> {
    if checkpoints.is_empty() {
        return Ok(());
    }
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error("start catalog Album checkpoint batch"))?;
    assert_lease(&transaction, lease)?;
    for checkpoint in checkpoints {
        transaction
            .execute(
                "INSERT INTO catalog_generation_albums(
                 generation_id,album_path,scan_token,album_key,artist_key,
                 inventory_signature,artwork_signature,tag_signature,track_count,
                 total_size,newest_mtime_ns,disposition,artist_payload_json,
                 album_payload_json,tracks_payload_json,review_payload_json,error,completed_at
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,
                 strftime('%Y-%m-%dT%H:%M:%fZ','now'))
             ON CONFLICT(generation_id,album_path) DO UPDATE SET
                 scan_token=excluded.scan_token,album_key=excluded.album_key,
                 artist_key=excluded.artist_key,inventory_signature=excluded.inventory_signature,
                 artwork_signature=excluded.artwork_signature,tag_signature=excluded.tag_signature,
                 track_count=excluded.track_count,total_size=excluded.total_size,
                 newest_mtime_ns=excluded.newest_mtime_ns,disposition=excluded.disposition,
                 artist_payload_json=excluded.artist_payload_json,
                 album_payload_json=excluded.album_payload_json,
                 tracks_payload_json=excluded.tracks_payload_json,
                 review_payload_json=excluded.review_payload_json,error=excluded.error,
                 completed_at=excluded.completed_at",
                params![
                    lease.generation_id,
                    checkpoint.album_path,
                    lease.scan_token,
                    checkpoint.album_key,
                    checkpoint.artist_key,
                    checkpoint.inventory_signature,
                    checkpoint.artwork_signature,
                    checkpoint.tag_signature,
                    checkpoint.track_count,
                    checkpoint.total_size,
                    checkpoint.newest_mtime_ns,
                    checkpoint.disposition,
                    checkpoint.artist_payload_json,
                    checkpoint.album_payload_json,
                    checkpoint.tracks_payload_json,
                    checkpoint.review_payload_json,
                    checkpoint.error,
                ],
            )
            .map_err(sqlite_error("write catalog Album checkpoint"))?;
        increment_progress(
            &transaction,
            lease,
            &checkpoint.album_path,
            &checkpoint.disposition,
            !checkpoint.error.is_empty(),
        )?;
    }
    transaction
        .commit()
        .map_err(sqlite_error("commit catalog Album checkpoint batch"))
}

fn increment_progress(
    connection: &Connection,
    lease: &CatalogGenerationLease,
    album_path: &str,
    disposition: &str,
    error: bool,
) -> Result<(), String> {
    let changed = i64::from(matches!(disposition, "changed" | "protected" | "deferred"));
    let unchanged = i64::from(disposition == "unchanged");
    let errors = i64::from(error || disposition == "error");
    let updated = connection
        .execute(
            "UPDATE catalog_generations SET
                 discovered_albums=discovered_albums+1,
                 completed_albums=completed_albums+1,
                 changed_albums=changed_albums+?1,
                 unchanged_albums=unchanged_albums+?2,
                 error_count=error_count+?3,current_path=?4,phase='staging',
                 lease_expires_at=strftime('%Y-%m-%dT%H:%M:%fZ','now',?5),
                 updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE generation_id=?6 AND owner_token=?7
               AND state='building' AND julianday(lease_expires_at)>julianday('now')",
            params![
                changed,
                unchanged,
                errors,
                album_path,
                format!("+{LEASE_SECONDS} seconds"),
                lease.generation_id,
                lease.owner_token,
            ],
        )
        .map_err(sqlite_error("advance catalog generation progress"))?;
    if updated != 1 {
        return Err("Catalog generation lease was lost while checkpointing.".to_string());
    }
    Ok(())
}

pub fn begin_publication(
    connection: &mut Connection,
    lease: &CatalogGenerationLease,
) -> Result<i64, String> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error("start catalog validation"))?;
    assert_lease(&transaction, lease)?;
    let (discovered, completed, errors) = transaction
        .query_row(
            "SELECT discovered_albums,completed_albums,error_count
             FROM catalog_generations WHERE generation_id=?1",
            [&lease.generation_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .map_err(sqlite_error("read catalog validation counters"))?;
    let staged: i64 = transaction
        .query_row(
            "SELECT count(*) FROM catalog_generation_albums
             WHERE generation_id=?1 AND scan_token=?2",
            params![lease.generation_id, lease.scan_token],
            |row| row.get(0),
        )
        .map_err(sqlite_error("count staged catalog Albums"))?;
    if errors != 0 || discovered != completed || completed != staged {
        return Err(format!(
            "Catalog generation validation failed: discovered={discovered}, completed={completed}, staged={staged}, errors={errors}."
        ));
    }
    if validation_test_interrupted() {
        return Err("Forced interruption during catalog generation validation.".to_string());
    }
    let removed: i64 = transaction
        .query_row(
            "SELECT count(*) FROM albums a WHERE NOT EXISTS(
                 SELECT 1 FROM catalog_generation_albums s
                 WHERE s.generation_id=?1 AND s.scan_token=?2
                   AND lower(s.album_path)=lower(a.path)
             ) AND NOT EXISTS(
                 SELECT 1 FROM catalog_generation_albums s
                 WHERE s.generation_id=?1 AND s.scan_token=?2
                   AND s.album_key<>'' AND lower(s.album_key)=lower(a.album_key)
             )",
            params![lease.generation_id, lease.scan_token],
            |row| row.get(0),
        )
        .map_err(sqlite_error("count removed catalog Albums"))?;
    let updated = transaction
        .execute(
            "UPDATE catalog_generations SET state='publishing',phase='publishing',
                 removed_albums=?1,
                 lease_expires_at=strftime('%Y-%m-%dT%H:%M:%fZ','now',?2),
                 updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE generation_id=?3 AND owner_token=?4 AND state='building'",
            params![
                removed,
                format!("+{LEASE_SECONDS} seconds"),
                lease.generation_id,
                lease.owner_token,
            ],
        )
        .map_err(sqlite_error("mark catalog generation for publication"))?;
    if updated != 1 {
        return Err("Catalog generation became stale before publication.".to_string());
    }
    transaction
        .commit()
        .map_err(sqlite_error("commit catalog validation"))?;
    Ok(removed)
}

pub fn assert_publication_lease(
    connection: &Connection,
    lease: &CatalogGenerationLease,
) -> Result<(), String> {
    let valid = connection
        .query_row(
            "SELECT 1 FROM catalog_generations
             WHERE generation_id=?1 AND owner_token=?2 AND state='publishing'
               AND julianday(lease_expires_at)>julianday('now')",
            params![lease.generation_id, lease.owner_token],
            |_| Ok(true),
        )
        .optional()
        .map_err(sqlite_error("verify catalog publication lease"))?
        .unwrap_or(false);
    if valid {
        Ok(())
    } else {
        Err("Catalog publication rejected a stale worker.".to_string())
    }
}

fn assert_lease(connection: &Connection, lease: &CatalogGenerationLease) -> Result<(), String> {
    let valid = connection
        .query_row(
            "SELECT 1 FROM catalog_generations
             WHERE generation_id=?1 AND owner_token=?2 AND state='building'
               AND julianday(lease_expires_at)>julianday('now')",
            params![lease.generation_id, lease.owner_token],
            |_| Ok(true),
        )
        .optional()
        .map_err(sqlite_error("verify catalog generation lease"))?
        .unwrap_or(false);
    if valid {
        Ok(())
    } else {
        Err("Catalog generation lease expired or belongs to another worker.".to_string())
    }
}

pub fn mark_terminal(
    connection: &mut Connection,
    lease: &CatalogGenerationLease,
    state: &str,
    error: &str,
) -> Result<(), String> {
    if !matches!(state, "failed" | "cancelled") {
        return Err(format!("Unsupported terminal catalog state: {state}"));
    }
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error("start terminal catalog generation update"))?;
    let updated = transaction
        .execute(
            "UPDATE catalog_generations SET state=?1,phase=?1,error=?2,
                 updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE generation_id=?3 AND owner_token=?4
               AND state IN ('building','validating','publishing')",
            params![state, error, lease.generation_id, lease.owner_token],
        )
        .map_err(sqlite_error("finish failed catalog generation"))?;
    if updated != 1 {
        return Err(
            "Catalog generation lease was lost before its terminal state could be recorded."
                .to_string(),
        );
    }
    let restored = transaction
        .execute(
            "UPDATE catalog_index_state SET maintenance_generation_id='',
                 catalog_state=CASE WHEN active_generation_id='' THEN 'failed' ELSE 'ready' END,
                 updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE singleton=1 AND maintenance_generation_id=?1",
            [&lease.generation_id],
        )
        .map_err(sqlite_error("restore published catalog state"))?;
    if restored != 1 {
        return Err(
            "Catalog generation is no longer the active maintenance generation.".to_string(),
        );
    }
    transaction
        .commit()
        .map_err(sqlite_error("commit terminal catalog generation state"))
}

pub fn status(connection: &Connection) -> Result<CatalogIndexStatus, String> {
    let (active, maintenance, catalog_state, summary) = connection
        .query_row(
            "SELECT active_generation_id,maintenance_generation_id,catalog_state,change_summary_json
             FROM catalog_index_state WHERE singleton=1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .map_err(sqlite_error("read catalog index status"))?
        .unwrap_or_else(|| (String::new(), String::new(), "initializing".to_string(), "{}".to_string()));
    let generation = if maintenance.is_empty() {
        None
    } else {
        connection
            .query_row(
                "SELECT state,phase,current_path,discovered_albums,completed_albums,
                        changed_albums,unchanged_albums,removed_albums,error_count
                 FROM catalog_generations WHERE generation_id=?1",
                [&maintenance],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, i64>(8)?,
                    ))
                },
            )
            .optional()
            .map_err(sqlite_error("read active catalog generation status"))?
    };
    let (state, phase, path, discovered, completed, changed, unchanged, removed, errors) =
        generation.unwrap_or_else(|| {
            (
                String::new(),
                String::new(),
                String::new(),
                0,
                0,
                0,
                0,
                0,
                0,
            )
        });
    Ok(CatalogIndexStatus {
        active_generation_id: active,
        maintenance_generation_id: maintenance,
        catalog_state,
        generation_state: state,
        phase,
        current_path: path,
        discovered_albums: discovered,
        completed_albums: completed,
        changed_albums: changed,
        unchanged_albums: unchanged,
        removed_albums: removed,
        error_count: errors,
        change_summary: serde_json::from_str(&summary).unwrap_or_else(|_| json!({})),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Connection {
        let connection = Connection::open_in_memory().expect("memory database");
        connection
            .execute_batch(include_str!("../../schema/media.sql"))
            .expect("catalog schema");
        connection
            .execute(
                "INSERT INTO catalog_index_state(singleton,catalog_state,updated_at)
                 VALUES(1,'ready',strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
                [],
            )
            .expect("coordinator state");
        connection
    }

    fn checkpoint(path: &str) -> StagedAlbumCheckpoint {
        StagedAlbumCheckpoint {
            album_path: path.to_string(),
            album_key: format!("album-{path}"),
            artist_key: "artist".to_string(),
            inventory_signature: "inventory".to_string(),
            artwork_signature: "artwork".to_string(),
            tag_signature: "tags".to_string(),
            track_count: 1,
            total_size: 10,
            newest_mtime_ns: 20,
            disposition: "changed".to_string(),
            artist_payload_json: Some("{}".to_string()),
            album_payload_json: Some("{}".to_string()),
            tracks_payload_json: Some("[]".to_string()),
            review_payload_json: None,
            error: String::new(),
        }
    }

    #[test]
    fn only_one_live_generation_can_be_claimed() {
        let mut connection = database();
        let first = claim_generation(&mut connection, "incremental", "C:/music").unwrap();
        let error = claim_generation(&mut connection, "incremental", "C:/music").unwrap_err();
        assert!(error.contains("already active"));
        assert_eq!(
            status(&connection).unwrap().maintenance_generation_id,
            first.generation_id
        );
    }

    #[test]
    fn expired_generation_resumes_and_rejects_stale_owner() {
        let mut connection = database();
        let stale = claim_generation(&mut connection, "incremental", "C:/music").unwrap();
        stage_album(&mut connection, &stale, &checkpoint("C:/music/one")).unwrap();
        connection
            .execute(
                "UPDATE catalog_generations SET lease_expires_at='2000-01-01T00:00:00.000Z'
                 WHERE generation_id=?1",
                [&stale.generation_id],
            )
            .unwrap();
        let resumed = claim_generation(&mut connection, "incremental", "C:/music").unwrap();
        assert!(resumed.resumed);
        assert_eq!(resumed.generation_id, stale.generation_id);
        assert_ne!(resumed.owner_token, stale.owner_token);
        assert!(stage_album(&mut connection, &stale, &checkpoint("C:/music/stale")).is_err());
        assert!(reusable_checkpoint(&connection, &resumed, "C:/music/one", "inventory").unwrap());
        touch_reusable_checkpoint(&mut connection, &resumed, "C:/music/one").unwrap();
    }

    #[test]
    fn stale_owner_cannot_cancel_a_resumed_generation() {
        let mut connection = database();
        let stale = claim_generation(&mut connection, "incremental", "C:/music").unwrap();
        stage_album(&mut connection, &stale, &checkpoint("C:/music/one")).unwrap();
        connection
            .execute(
                "UPDATE catalog_generations SET lease_expires_at='2000-01-01T00:00:00.000Z'
                 WHERE generation_id=?1",
                [&stale.generation_id],
            )
            .unwrap();
        let resumed = claim_generation(&mut connection, "incremental", "C:/music").unwrap();

        let error = mark_terminal(&mut connection, &stale, "cancelled", "stale cancellation")
            .expect_err("the previous owner must not cancel recovered work");

        assert!(error.contains("lease"));
        let state = status(&connection).unwrap();
        assert_eq!(state.maintenance_generation_id, resumed.generation_id);
        assert_eq!(state.generation_state, "building");
        assert!(reusable_checkpoint(&connection, &resumed, "C:/music/one", "inventory").unwrap());
    }

    #[test]
    fn failed_generation_preserves_published_catalog() {
        let mut connection = database();
        connection
            .execute(
                "INSERT INTO artists(
                     artist_key,artist_name,primary_path,created_at,updated_at,last_seen_at,splined_version
                 ) VALUES('artist','Artist','C:/music/Artist','now','now','now','test')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO albums(
                     album_key,artist_key,album_name,path,tag_signature,created_at,updated_at,last_seen_at,splined_version
                 ) VALUES('published','artist','Published','C:/music/Artist/Published','tag','now','now','now','test')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE catalog_index_state SET active_generation_id='published-generation'
                 WHERE singleton=1",
                [],
            )
            .unwrap();
        let lease = claim_generation(&mut connection, "full", "C:/music").unwrap();
        stage_album(&mut connection, &lease, &checkpoint("C:/music/Artist/New")).unwrap();
        mark_terminal(&mut connection, &lease, "failed", "forced test failure").unwrap();
        assert_eq!(
            connection
                .query_row("SELECT album_name FROM albums", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "Published"
        );
        let state = status(&connection).unwrap();
        assert_eq!(state.catalog_state, "ready");
        assert!(state.maintenance_generation_id.is_empty());
        let terminal: (String, String, String) = connection
            .query_row(
                "SELECT state,phase,error FROM catalog_generations WHERE generation_id=?1",
                [&lease.generation_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(terminal.0, "failed");
        assert_eq!(terminal.1, "failed");
        assert_eq!(terminal.2, "forced test failure");
    }

    #[test]
    fn cancelled_and_failed_states_survive_database_reopen() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("catalog.db");
        let mut recorded = Vec::new();
        {
            let mut connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(include_str!("../../schema/media.sql"))
                .unwrap();
            connection
                .execute(
                    "INSERT INTO catalog_index_state(
                         singleton,active_generation_id,catalog_state,updated_at
                     ) VALUES(1,'published-generation','ready',
                         strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
                    [],
                )
                .unwrap();
            for terminal in ["cancelled", "failed"] {
                let lease = claim_generation(&mut connection, "full", "C:/music").unwrap();
                stage_album(
                    &mut connection,
                    &lease,
                    &checkpoint(&format!("C:/music/{terminal}")),
                )
                .unwrap();
                mark_terminal(
                    &mut connection,
                    &lease,
                    terminal,
                    &format!("forced {terminal}"),
                )
                .unwrap();
                recorded.push((lease.generation_id, terminal.to_string()));
            }
        }

        let connection = Connection::open(&path).unwrap();
        for (generation_id, terminal) in recorded {
            let row: (String, String, String) = connection
                .query_row(
                    "SELECT state,phase,error FROM catalog_generations WHERE generation_id=?1",
                    [generation_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert_eq!(row.0, terminal);
            assert_eq!(row.1, terminal);
            assert_eq!(row.2, format!("forced {terminal}"));
        }
        let state = status(&connection).unwrap();
        assert_eq!(state.active_generation_id, "published-generation");
        assert!(state.maintenance_generation_id.is_empty());
        assert_eq!(state.catalog_state, "ready");
    }

    #[test]
    fn validation_rejects_error_checkpoints() {
        let mut connection = database();
        let lease = claim_generation(&mut connection, "full", "C:/music").unwrap();
        let mut failed = checkpoint("C:/music/Broken");
        failed.disposition = "error".to_string();
        failed.error = "forced".to_string();
        stage_album(&mut connection, &lease, &failed).unwrap();
        let error = begin_publication(&mut connection, &lease).unwrap_err();
        assert!(error.contains("errors=1"));
    }

    #[test]
    fn bounded_batch_stages_every_album_in_one_progress_checkpoint() {
        let mut connection = database();
        let lease = claim_generation(&mut connection, "full", "C:/music").unwrap();
        let checkpoints = [
            checkpoint("C:/music/One"),
            checkpoint("C:/music/Two"),
            checkpoint("C:/music/Three"),
        ];

        stage_albums(&mut connection, &lease, &checkpoints).unwrap();

        let staged: i64 = connection
            .query_row(
                "SELECT count(*) FROM catalog_generation_albums WHERE generation_id=?1",
                [&lease.generation_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(staged, 3);
        let status = status(&connection).unwrap();
        assert_eq!(status.discovered_albums, 3);
        assert_eq!(status.completed_albums, 3);
        assert_eq!(status.changed_albums, 3);
    }

    #[test]
    fn resumed_checkpoints_are_loaded_and_touched_as_one_batch() {
        let mut connection = database();
        let stale = claim_generation(&mut connection, "full", "C:/music").unwrap();
        stage_albums(
            &mut connection,
            &stale,
            &[
                checkpoint("C:/music/One"),
                checkpoint("C:/music/Two"),
                checkpoint("C:/music/Three"),
            ],
        )
        .unwrap();
        connection
            .execute(
                "UPDATE catalog_generations SET lease_expires_at='2000-01-01T00:00:00.000Z'
                 WHERE generation_id=?1",
                [&stale.generation_id],
            )
            .unwrap();
        let resumed = claim_generation(&mut connection, "full", "C:/music").unwrap();
        let checkpoints = reusable_checkpoints(&connection, &resumed).unwrap();
        assert_eq!(checkpoints.len(), 3);
        let paths = checkpoints
            .into_iter()
            .map(|(path, _)| path)
            .collect::<Vec<_>>();

        touch_reusable_checkpoints(&mut connection, &resumed, &paths).unwrap();

        let status = status(&connection).unwrap();
        assert_eq!(status.discovered_albums, 3);
        assert_eq!(status.completed_albums, 3);
        assert_eq!(status.changed_albums, 3);
    }
}
