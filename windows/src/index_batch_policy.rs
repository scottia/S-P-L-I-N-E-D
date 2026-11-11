//! Client-specific adaptive batching for the native SQLite catalog indexer.
//!
//! Decisions are made only while an explicit generation is being built and
//! only after a durable checkpoint has committed.  Normal application startup
//! never constructs this controller.

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{Value, json};
use std::time::Duration;

pub const DEFAULT_BATCH_SIZE: usize = 16;
pub const MIN_BATCH_SIZE: usize = 1;
pub const MAX_BATCH_SIZE: usize = 64;
const TARGET_PAYLOAD_BYTES: u64 = 8 * 1024 * 1024;
const TARGET_CHECKPOINT: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CatalogWorkload {
    pub artists: u64,
    pub albums: u64,
    pub tracks: u64,
    pub average_tracks_per_album: f64,
    pub maximum_tracks_per_album: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BatchDecision {
    pub checkpoint: u64,
    pub previous_size: usize,
    pub next_size: usize,
    pub reason: String,
    pub albums: usize,
    pub tracks: u64,
    pub payload_bytes: u64,
    pub processing_seconds: f64,
    pub checkpoint_seconds: f64,
    pub interactive_pressure: bool,
}

#[derive(Debug, Clone)]
pub struct AdaptiveBatchController {
    enabled: bool,
    configured_size: usize,
    initial_size: usize,
    current_size: usize,
    workload: CatalogWorkload,
    checkpoints: u64,
    observed_albums: u64,
    stable_checkpoints: u8,
    peak_payload_bytes: u64,
    decisions: Vec<BatchDecision>,
}

impl AdaptiveBatchController {
    pub fn from_database(
        connection: &Connection,
        enabled: bool,
        configured_size: usize,
    ) -> Result<Self, String> {
        let configured_size = configured_size.clamp(MIN_BATCH_SIZE, MAX_BATCH_SIZE);
        let workload = read_workload(connection)?;
        let prior = prior_adaptive_size(connection, workload)?;
        let initial_size = if enabled {
            prior.unwrap_or(DEFAULT_BATCH_SIZE)
        } else {
            configured_size
        }
        .clamp(MIN_BATCH_SIZE, MAX_BATCH_SIZE);
        Ok(Self {
            enabled,
            configured_size,
            initial_size,
            current_size: initial_size,
            workload,
            checkpoints: 0,
            observed_albums: 0,
            stable_checkpoints: 0,
            peak_payload_bytes: 0,
            decisions: Vec::new(),
        })
    }

    #[cfg(test)]
    pub fn for_test(enabled: bool, configured_size: usize, workload: CatalogWorkload) -> Self {
        let configured_size = configured_size.clamp(MIN_BATCH_SIZE, MAX_BATCH_SIZE);
        let initial_size = if enabled {
            DEFAULT_BATCH_SIZE
        } else {
            configured_size
        };
        Self {
            enabled,
            configured_size,
            initial_size,
            current_size: initial_size,
            workload,
            checkpoints: 0,
            observed_albums: 0,
            stable_checkpoints: 0,
            peak_payload_bytes: 0,
            decisions: Vec::new(),
        }
    }

    pub fn current_size(&self) -> usize {
        self.current_size
    }

    pub fn should_checkpoint(&self, albums: usize, payload_bytes: u64) -> bool {
        albums >= self.current_size
            || (self.enabled && payload_bytes >= TARGET_PAYLOAD_BYTES)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn observe_checkpoint(
        &mut self,
        albums: usize,
        tracks: u64,
        payload_bytes: u64,
        processing: Duration,
        checkpoint: Duration,
        interactive_pressure: bool,
    ) {
        self.checkpoints += 1;
        self.observed_albums += albums as u64;
        self.peak_payload_bytes = self.peak_payload_bytes.max(payload_bytes);
        if !self.enabled || albums == 0 {
            return;
        }

        let previous = self.current_size;
        let average_tracks = tracks as f64 / albums as f64;
        let bytes_per_album = payload_bytes / albums as u64;
        let processing_per_album = processing.as_secs_f64() / albums as f64;
        let severe_memory_pressure = payload_bytes > TARGET_PAYLOAD_BYTES
            || bytes_per_album > 768 * 1024;
        let heavy_album = average_tracks >= 96.0 || processing_per_album >= 0.35;
        let slow_checkpoint = checkpoint > TARGET_CHECKPOINT * 4;

        let (candidate, reason, immediate) = if interactive_pressure {
            (lower_step(previous), "interactive activity", true)
        } else if severe_memory_pressure {
            (lower_step(previous), "batch memory pressure", true)
        } else if slow_checkpoint {
            (lower_step(previous), "SQLite checkpoint latency", true)
        } else if heavy_album {
            (lower_step(previous), "heavy per-Album workload", false)
        } else if (self.workload.albums >= 512 || self.observed_albums >= 512)
            && checkpoint < TARGET_CHECKPOINT / 2
            && payload_bytes < TARGET_PAYLOAD_BYTES / 2
            && average_tracks < 48.0
            && processing_per_album < 0.20
        {
            (higher_step(previous), "low-cost checkpoint", false)
        } else {
            (previous, "within target", false)
        };

        if candidate == previous {
            self.stable_checkpoints = 0;
            return;
        }
        self.stable_checkpoints = self.stable_checkpoints.saturating_add(1);
        if !immediate && self.stable_checkpoints < 2 {
            return;
        }
        self.stable_checkpoints = 0;
        self.current_size = candidate;
        self.decisions.push(BatchDecision {
            checkpoint: self.checkpoints,
            previous_size: previous,
            next_size: candidate,
            reason: reason.to_string(),
            albums,
            tracks,
            payload_bytes,
            processing_seconds: processing.as_secs_f64(),
            checkpoint_seconds: checkpoint.as_secs_f64(),
            interactive_pressure,
        });
    }

    pub fn summary(&self) -> Value {
        json!({
            "enabled": self.enabled,
            "configured_size": self.configured_size,
            "initial_size": self.initial_size,
            "final_size": self.current_size,
            "minimum_size": MIN_BATCH_SIZE,
            "maximum_size": MAX_BATCH_SIZE,
            "checkpoints_observed": self.checkpoints,
            "peak_batch_payload_bytes": self.peak_payload_bytes,
            "catalog_artists": self.workload.artists,
            "catalog_albums": self.workload.albums,
            "catalog_tracks": self.workload.tracks,
            "average_tracks_per_album": self.workload.average_tracks_per_album,
            "maximum_tracks_per_album": self.workload.maximum_tracks_per_album,
            "decisions": self.decisions,
        })
    }
}

pub fn checkpoint_payload_bytes(rows: &[crate::catalog_index_coordinator::StagedAlbumCheckpoint]) -> u64 {
    rows.iter()
        .map(|row| {
            [
                Some(row.album_path.as_str()),
                Some(row.album_key.as_str()),
                Some(row.artist_key.as_str()),
                Some(row.inventory_signature.as_str()),
                Some(row.artwork_signature.as_str()),
                Some(row.tag_signature.as_str()),
                row.artist_payload_json.as_deref(),
                row.album_payload_json.as_deref(),
                row.tracks_payload_json.as_deref(),
                row.review_payload_json.as_deref(),
            ]
            .into_iter()
            .flatten()
            .map(|value| value.len() as u64)
            .sum::<u64>()
        })
        .sum()
}

fn read_workload(connection: &Connection) -> Result<CatalogWorkload, String> {
    connection
        .query_row(
            "SELECT (SELECT count(*) FROM artists),
                    (SELECT count(*) FROM albums),
                    (SELECT count(*) FROM tracks),
                    COALESCE((SELECT avg(track_count) FROM albums),0.0),
                    COALESCE((SELECT max(track_count) FROM albums),0)",
            [],
            |row| {
                Ok(CatalogWorkload {
                    artists: row.get::<_, i64>(0)?.max(0) as u64,
                    albums: row.get::<_, i64>(1)?.max(0) as u64,
                    tracks: row.get::<_, i64>(2)?.max(0) as u64,
                    average_tracks_per_album: row.get::<_, f64>(3)?,
                    maximum_tracks_per_album: row.get::<_, i64>(4)?.max(0) as u64,
                })
            },
        )
        .map_err(|error| format!("Unable to read SQLite catalog batching statistics: {error}"))
}

fn prior_adaptive_size(
    connection: &Connection,
    workload: CatalogWorkload,
) -> Result<Option<usize>, String> {
    let details = connection
        .query_row(
            "SELECT details_json FROM db_maintenance_state
             WHERE action_name='media-index-refresh'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("Unable to read prior SQLite batch measurements: {error}"))?;
    let Some(details) = details.and_then(|value| serde_json::from_str::<Value>(&value).ok()) else {
        return Ok(None);
    };
    let Some(adaptive) = details.get("adaptive_batching") else {
        return Ok(None);
    };
    if !adaptive.get("enabled").and_then(Value::as_bool).unwrap_or(false) {
        return Ok(None);
    }
    let prior_albums = adaptive
        .get("catalog_albums")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .or_else(|| details.get("albums").and_then(Value::as_u64))
        .unwrap_or(0);
    if prior_albums == 0 || workload.albums == 0 {
        return Ok(None);
    }
    let ratio = workload.albums as f64 / prior_albums as f64;
    if !(0.5..=2.0).contains(&ratio) {
        return Ok(None);
    }
    Ok(adaptive
        .get("final_size")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .filter(|value| (MIN_BATCH_SIZE..=MAX_BATCH_SIZE).contains(value)))
}

fn lower_step(value: usize) -> usize {
    match value {
        0 | 1 => 1,
        2 => 1,
        3..=4 => 2,
        5..=8 => 4,
        9..=16 => 8,
        17..=32 => 16,
        _ => 32,
    }
}

fn higher_step(value: usize) -> usize {
    match value {
        0 | 1 => 2,
        2 => 4,
        3..=4 => 8,
        5..=8 => 16,
        9..=16 => 32,
        _ => 64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workload() -> CatalogWorkload {
        CatalogWorkload {
            artists: 100,
            albums: 1_000,
            tracks: 12_000,
            average_tracks_per_album: 12.0,
            maximum_tracks_per_album: 20,
        }
    }

    #[test]
    fn default_is_sixteen_and_manual_override_remains_fixed() {
        let adaptive = AdaptiveBatchController::for_test(true, 3, workload());
        assert_eq!(adaptive.current_size(), 16);
        let mut fixed = AdaptiveBatchController::for_test(false, 3, workload());
        for _ in 0..4 {
            fixed.observe_checkpoint(
                3,
                3,
                128,
                Duration::from_millis(2),
                Duration::from_millis(1),
                false,
            );
        }
        assert_eq!(fixed.current_size(), 3);
        assert!(fixed.decisions.is_empty());
    }

    #[test]
    fn grows_only_after_stable_checkpoint_boundaries() {
        let mut controller = AdaptiveBatchController::for_test(true, 16, workload());
        controller.observe_checkpoint(
            16,
            160,
            100_000,
            Duration::from_millis(200),
            Duration::from_millis(10),
            false,
        );
        assert_eq!(controller.current_size(), 16);
        controller.observe_checkpoint(
            16,
            160,
            100_000,
            Duration::from_millis(200),
            Duration::from_millis(10),
            false,
        );
        assert_eq!(controller.current_size(), 32);
    }

    #[test]
    fn memory_or_interactive_pressure_shrinks_at_checkpoint() {
        let mut controller = AdaptiveBatchController::for_test(true, 16, workload());
        controller.observe_checkpoint(
            16,
            160,
            TARGET_PAYLOAD_BYTES + 1,
            Duration::from_millis(200),
            Duration::from_millis(10),
            false,
        );
        assert_eq!(controller.current_size(), 8);
        controller.observe_checkpoint(
            8,
            80,
            100_000,
            Duration::from_millis(100),
            Duration::from_millis(10),
            true,
        );
        assert_eq!(controller.current_size(), 4);
    }

    #[test]
    fn prior_published_measurement_seeds_the_next_generation() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE artists(id INTEGER);
                 CREATE TABLE albums(id INTEGER,track_count INTEGER);
                 CREATE TABLE tracks(id INTEGER);
                 CREATE TABLE db_maintenance_state(action_name TEXT,details_json TEXT);
                 WITH RECURSIVE n(value) AS (
                   SELECT 1 UNION ALL SELECT value+1 FROM n WHERE value<1000
                 ) INSERT INTO albums SELECT value,100 FROM n;
                 WITH RECURSIVE n(value) AS (
                   SELECT 1 UNION ALL SELECT value+1 FROM n WHERE value<100
                 ) INSERT INTO artists SELECT value FROM n;
                 INSERT INTO db_maintenance_state VALUES(
                   'media-index-refresh',
                   '{\"albums\":1000,\"adaptive_batching\":{\"enabled\":true,\"catalog_albums\":0,\"final_size\":8}}'
                 );",
            )
            .unwrap();
        let controller = AdaptiveBatchController::from_database(&connection, true, 16).unwrap();
        assert_eq!(controller.current_size(), 8);
    }
}
