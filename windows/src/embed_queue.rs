//! Machine-local, durable work list for album-wide embedded thumbnails.
//! The library SQLite database is deliberately not used for long-running jobs.

use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct EmbedJob {
    pub id: i64,
    pub album_path: PathBuf,
    pub label: String,
    pub thumbnail: Vec<u8>,
    pub side: u32,
    pub cover_width: u32,
    pub cover_height: u32,
    pub total: usize,
    pub completed: usize,
    pub state: String,
    pub detail: String,
}

pub fn local_queue_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("SPLINED").join("embed-queue.sqlite")
}

fn open(path: &Path) -> Result<Connection, String> {
    let parent = path
        .parent()
        .ok_or("The embed queue path has no parent directory.")?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Unable to create local embed queue directory: {error}"))?;
    let connection = Connection::open(path)
        .map_err(|error| format!("Unable to open local embed queue: {error}"))?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|error| format!("Unable to configure embed queue timeout: {error}"))?;
    connection
        .execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS embed_jobs (
                 id INTEGER PRIMARY KEY,
                 album_path TEXT NOT NULL,
                 label TEXT NOT NULL,
                 thumbnail BLOB NOT NULL,
                 side INTEGER NOT NULL,
                 cover_width INTEGER NOT NULL DEFAULT 0,
                 cover_height INTEGER NOT NULL DEFAULT 0,
                 total INTEGER NOT NULL,
                 completed INTEGER NOT NULL DEFAULT 0,
                 state TEXT NOT NULL DEFAULT 'staged',
                 detail TEXT NOT NULL DEFAULT '',
                 created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE INDEX IF NOT EXISTS embed_jobs_state_id ON embed_jobs(state, id);
             CREATE UNIQUE INDEX IF NOT EXISTS embed_jobs_one_active_album_v2
             ON embed_jobs(album_path) WHERE state IN ('staged','queued','running','paused');",
        )
        .map_err(|error| format!("Unable to initialize local embed queue: {error}"))?;
    let columns = connection
        .prepare("PRAGMA table_info(embed_jobs)")
        .and_then(|mut query| {
            query
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(|error| format!("Unable to inspect local embed queue: {error}"))?;
    for column in ["cover_width", "cover_height"] {
        if !columns.iter().any(|existing| existing == column) {
            connection
                .execute(
                    &format!(
                        "ALTER TABLE embed_jobs ADD COLUMN {column} INTEGER NOT NULL DEFAULT 0"
                    ),
                    [],
                )
                .map_err(|error| format!("Unable to update local embed queue: {error}"))?;
        }
    }
    Ok(connection)
}

pub fn recover_interrupted(path: &Path) -> Result<(), String> {
    open(path)?
        .execute(
            "UPDATE embed_jobs SET state='paused', detail='Interrupted; resume to retry album.' WHERE state='running'",
            [],
        )
        .map_err(|error| format!("Unable to recover interrupted embed jobs: {error}"))?;
    Ok(())
}

pub fn enqueue(
    path: &Path,
    album_path: &Path,
    label: &str,
    thumbnail: &[u8],
    side: u32,
    cover_dimensions: (u32, u32),
    total: usize,
) -> Result<i64, String> {
    if total == 0 || thumbnail.is_empty() || !(300..=4000).contains(&side) {
        return Err(
            "A queue job needs audio tracks and a prepared thumbnail at the preferred size.".into(),
        );
    }
    let (cover_width, cover_height) = cover_dimensions;
    let album_text = album_path.to_string_lossy();
    let connection = open(path)?;
    let existing: Option<i64> = connection
        .query_row(
            "SELECT id FROM embed_jobs WHERE album_path=?1 AND state IN ('staged','queued','running','paused') ORDER BY id LIMIT 1",
            [album_text.as_ref()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("Unable to check queued album: {error}"))?;
    if let Some(id) = existing {
        return Err(format!(
            "This album is already in the embed queue (job {id}). Remove or finish that job before queueing another cover."
        ));
    }
    connection
        .execute(
            "INSERT INTO embed_jobs(album_path,label,thumbnail,side,cover_width,cover_height,total,state) VALUES (?1,?2,?3,?4,?5,?6,?7,'staged')",
            params![album_text.as_ref(), label, thumbnail, side, cover_width, cover_height, total as i64],
        )
        .map_err(|error| format!("Unable to queue album embedding: {error}"))?;
    Ok(connection.last_insert_rowid())
}

fn map_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<EmbedJob> {
    Ok(EmbedJob {
        id: row.get(0)?,
        album_path: PathBuf::from(row.get::<_, String>(1)?),
        label: row.get(2)?,
        thumbnail: row.get(3)?,
        side: row.get::<_, i64>(4)? as u32,
        cover_width: row.get::<_, i64>(5)? as u32,
        cover_height: row.get::<_, i64>(6)? as u32,
        total: row.get::<_, i64>(7)? as usize,
        completed: row.get::<_, i64>(8)? as usize,
        state: row.get(9)?,
        detail: row.get(10)?,
    })
}

pub fn list(path: &Path) -> Result<Vec<EmbedJob>, String> {
    let connection = open(path)?;
    let mut statement = connection
        .prepare("SELECT id,album_path,label,thumbnail,side,cover_width,cover_height,total,completed,state,detail FROM embed_jobs ORDER BY id DESC LIMIT 100")
        .map_err(|error| format!("Unable to list embed jobs: {error}"))?;
    statement
        .query_map([], map_job)
        .map_err(|error| format!("Unable to read embed jobs: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to decode embed jobs: {error}"))
}

pub fn active_album_paths(path: &Path) -> Result<HashSet<String>, String> {
    let connection = open(path)?;
    let mut statement = connection
        .prepare("SELECT album_path FROM embed_jobs WHERE state IN ('staged','queued','running','paused')")
        .map_err(|error| format!("Unable to inspect active embed albums: {error}"))?;
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| format!("Unable to read active embed albums: {error}"))?
        .map(|path| path.map(|path| path.to_lowercase()))
        .collect::<Result<HashSet<_>, _>>()
        .map_err(|error| format!("Unable to decode active embed albums: {error}"))
}

pub fn start_staged(path: &Path) -> Result<usize, String> {
    let count = open(path)?
        .execute(
            "UPDATE embed_jobs SET state='queued' WHERE state='staged'",
            [],
        )
        .map_err(|error| format!("Unable to start staged embed albums: {error}"))?;
    Ok(count)
}

pub fn claim_next(path: &Path) -> Result<Option<EmbedJob>, String> {
    let mut connection = open(path)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Unable to start embed job claim: {error}"))?;
    let id: Option<i64> = transaction
        .query_row(
            "SELECT id FROM embed_jobs WHERE state='queued' ORDER BY id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("Unable to find queued album: {error}"))?;
    let Some(id) = id else { return Ok(None) };
    transaction
        .execute(
            "UPDATE embed_jobs SET state='running', detail='' WHERE id=?1 AND state='queued'",
            [id],
        )
        .map_err(|error| format!("Unable to claim queued album: {error}"))?;
    let job = transaction
        .query_row(
            "SELECT id,album_path,label,thumbnail,side,cover_width,cover_height,total,completed,state,detail FROM embed_jobs WHERE id=?1",
            [id],
            map_job,
        )
        .map_err(|error| format!("Unable to read claimed album: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("Unable to commit embed job claim: {error}"))?;
    Ok(Some(job))
}

pub fn progress(path: &Path, id: i64, completed: usize, total: usize) -> Result<(), String> {
    open(path)?
        .execute(
            "UPDATE embed_jobs SET completed=?2,total=?3 WHERE id=?1 AND state='running'",
            params![id, completed as i64, total as i64],
        )
        .map_err(|error| format!("Unable to save embed progress: {error}"))?;
    Ok(())
}

pub fn finish(path: &Path, id: i64, error: Option<&str>) -> Result<(), String> {
    let (state, detail) = match error {
        Some(error) => ("failed", error),
        None => ("done", "All album track thumbnails replaced."),
    };
    open(path)?
        .execute(
            "UPDATE embed_jobs SET state=?2,detail=?3 WHERE id=?1 AND state='running'",
            params![id, state, detail],
        )
        .map_err(|error| format!("Unable to save embed result: {error}"))?;
    Ok(())
}

pub fn pause_running(path: &Path, id: i64, completed: usize, total: usize) -> Result<(), String> {
    open(path)?
        .execute(
            "UPDATE embed_jobs SET state='paused',completed=?2,total=?3,detail='Paused after the current track; resume to continue.' WHERE id=?1 AND state='running'",
            params![id, completed as i64, total as i64],
        )
        .map_err(|error| format!("Unable to pause embed album: {error}"))?;
    Ok(())
}

pub fn retry(path: &Path, id: i64) -> Result<(), String> {
    open(path)?
        .execute(
            "UPDATE embed_jobs SET state='queued',completed=CASE WHEN state='failed' THEN 0 ELSE completed END,detail='' WHERE id=?1 AND state IN ('failed','paused')",
            [id],
        )
        .map_err(|error| format!("Unable to resume embed job: {error}"))?;
    Ok(())
}

pub fn resume_paused(path: &Path) -> Result<usize, String> {
    open(path)?
        .execute(
            "UPDATE embed_jobs SET state='queued',detail='' WHERE state='paused'",
            [],
        )
        .map_err(|error| format!("Unable to resume paused embed albums: {error}"))
}

pub fn remove(path: &Path, id: i64) -> Result<(), String> {
    open(path)?
        .execute(
            "DELETE FROM embed_jobs WHERE id=?1 AND state!='running'",
            [id],
        )
        .map_err(|error| format!("Unable to remove embed job: {error}"))?;
    Ok(())
}

/// Clear only successfully embedded albums. Staged, active, paused, and
/// failed jobs remain available for the operator to review or retry.
pub fn remove_completed(path: &Path) -> Result<usize, String> {
    open(path)?
        .execute("DELETE FROM embed_jobs WHERE state='done'", [])
        .map_err(|error| format!("Unable to remove completed embed jobs: {error}"))
}

pub fn remove_all(path: &Path) -> Result<usize, String> {
    let connection = open(path)?;
    let running: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM embed_jobs WHERE state='running'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("Unable to inspect running embed albums: {error}"))?;
    if running > 0 {
        return Err(
            "Pause All Albums and wait for the current track before removing the queue.".into(),
        );
    }
    connection
        .execute("DELETE FROM embed_jobs", [])
        .map_err(|error| format!("Unable to remove all embed albums: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_is_durable_and_claims_one_album_at_a_time() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("queue.sqlite");
        let album = temp.path().join("Album");
        let first = enqueue(
            &database,
            &album,
            "Artist — Album",
            b"thumbnail",
            1200,
            (1600, 1600),
            2,
        )
        .unwrap();
        assert!(
            enqueue(
                &database,
                &album,
                "duplicate",
                b"other",
                1200,
                (1200, 1200),
                2
            )
            .unwrap_err()
            .contains("already in the embed queue")
        );
        assert_eq!(list(&database).unwrap().len(), 1);
        assert_eq!(list(&database).unwrap()[0].state, "staged");
        assert!(
            active_album_paths(&database)
                .unwrap()
                .contains(&album.to_string_lossy().to_lowercase())
        );
        assert!(claim_next(&database).unwrap().is_none());
        assert_eq!(start_staged(&database).unwrap(), 1);
        let claimed = claim_next(&database).unwrap().unwrap();
        assert_eq!(claimed.id, first);
        assert_eq!(claimed.thumbnail, b"thumbnail");
        assert_eq!((claimed.cover_width, claimed.cover_height), (1600, 1600));
        assert!(claim_next(&database).unwrap().is_none());
        progress(&database, first, 1, 2).unwrap();
        pause_running(&database, first, 1, 2).unwrap();
        assert_eq!(list(&database).unwrap()[0].state, "paused");
        assert_eq!(list(&database).unwrap()[0].completed, 1);
        assert_eq!(resume_paused(&database).unwrap(), 1);
        assert_eq!(claim_next(&database).unwrap().unwrap().completed, 1);
        recover_interrupted(&database).unwrap();
        assert_eq!(list(&database).unwrap()[0].state, "paused");
        retry(&database, first).unwrap();
        assert!(claim_next(&database).unwrap().is_some());
        finish(&database, first, None).unwrap();
        assert_eq!(list(&database).unwrap()[0].state, "done");
        assert!(active_album_paths(&database).unwrap().is_empty());
        remove(&database, first).unwrap();
        assert!(list(&database).unwrap().is_empty());
    }

    #[test]
    fn remove_completed_keeps_staged_and_failed_jobs() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("queue.sqlite");
        let done = enqueue(
            &database,
            &temp.path().join("Done"),
            "Done",
            b"a",
            1200,
            (1200, 1200),
            1,
        )
        .unwrap();
        let staged = enqueue(
            &database,
            &temp.path().join("Staged"),
            "Staged",
            b"b",
            1200,
            (1200, 1200),
            1,
        )
        .unwrap();
        assert_eq!(start_staged(&database).unwrap(), 2);
        let first = claim_next(&database).unwrap().unwrap();
        assert_eq!(first.id, done);
        finish(&database, done, None).unwrap();
        let second = claim_next(&database).unwrap().unwrap();
        assert_eq!(second.id, staged);
        finish(&database, staged, Some("test failure")).unwrap();
        let kept = enqueue(
            &database,
            &temp.path().join("Waiting"),
            "Waiting",
            b"c",
            1200,
            (1200, 1200),
            1,
        )
        .unwrap();
        assert_eq!(remove_completed(&database).unwrap(), 1);
        let jobs = list(&database).unwrap();
        assert_eq!(jobs.len(), 2);
        assert!(
            jobs.iter()
                .any(|job| job.id == staged && job.state == "failed")
        );
        assert!(
            jobs.iter()
                .any(|job| job.id == kept && job.state == "staged")
        );
        assert_eq!(remove_all(&database).unwrap(), 2);
        assert!(list(&database).unwrap().is_empty());
    }

    #[test]
    fn existing_queue_gains_cover_resolution_columns() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("queue.sqlite");
        let connection = Connection::open(&database).unwrap();
        connection.execute_batch("CREATE TABLE embed_jobs (id INTEGER PRIMARY KEY, album_path TEXT NOT NULL, label TEXT NOT NULL, thumbnail BLOB NOT NULL, side INTEGER NOT NULL, total INTEGER NOT NULL, completed INTEGER NOT NULL DEFAULT 0, state TEXT NOT NULL DEFAULT 'staged', detail TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);").unwrap();
        drop(connection);
        let album = temp.path().join("Album");
        enqueue(&database, &album, "Album", b"thumb", 1200, (1500, 900), 1).unwrap();
        let jobs = list(&database).unwrap();
        assert_eq!((jobs[0].cover_width, jobs[0].cover_height), (1500, 900));
    }
}
