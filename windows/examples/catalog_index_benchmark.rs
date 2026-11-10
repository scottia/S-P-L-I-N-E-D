//! Reproducible PR #170 SQLite batch/recovery benchmark.
//! Run only against an isolated read-only media fixture and disposable cache.

use rusqlite::Connection;
use serde_json::{Value, json};
use splined_core::config::Config;
use splined_core::media_database::{
    database_path, media_snapshot_incremental, media_snapshot_with_progress,
};
use std::env;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::time::Instant;

#[cfg(windows)]
#[repr(C)]
#[allow(non_snake_case)]
struct ProcessMemoryCounters {
    cb: u32,
    PageFaultCount: u32,
    PeakWorkingSetSize: usize,
    WorkingSetSize: usize,
    QuotaPeakPagedPoolUsage: usize,
    QuotaPagedPoolUsage: usize,
    QuotaPeakNonPagedPoolUsage: usize,
    QuotaNonPagedPoolUsage: usize,
    PagefileUsage: usize,
    PeakPagefileUsage: usize,
}

#[cfg(windows)]
#[link(name = "psapi")]
unsafe extern "system" {
    fn GetProcessMemoryInfo(process: isize, counters: *mut ProcessMemoryCounters, size: u32)
    -> i32;
}

#[cfg(windows)]
fn memory() -> (u64, u64) {
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut counters = ProcessMemoryCounters {
        cb: u32::try_from(std::mem::size_of::<ProcessMemoryCounters>()).unwrap(),
        PageFaultCount: 0,
        PeakWorkingSetSize: 0,
        WorkingSetSize: 0,
        QuotaPeakPagedPoolUsage: 0,
        QuotaPagedPoolUsage: 0,
        QuotaPeakNonPagedPoolUsage: 0,
        QuotaNonPagedPoolUsage: 0,
        PagefileUsage: 0,
        PeakPagefileUsage: 0,
    };
    let read = unsafe {
        GetProcessMemoryInfo(GetCurrentProcess() as isize, &raw mut counters, counters.cb)
    };
    if read == 0 {
        (0, 0)
    } else {
        (
            counters.WorkingSetSize as u64,
            counters.PeakWorkingSetSize as u64,
        )
    }
}

#[cfg(not(windows))]
fn memory() -> (u64, u64) {
    (0, 0)
}

fn details(connection: &Connection) -> Value {
    connection
        .query_row(
            "SELECT details_json FROM db_maintenance_state WHERE action_name='media-index-refresh'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_else(|| json!({}))
}

fn main() -> Result<(), String> {
    let arguments = env::args().collect::<Vec<_>>();
    if arguments.len() != 4 {
        return Err(
            "usage: catalog_index_benchmark <music-root> <disposable-cache> <batch-size>"
                .to_string(),
        );
    }
    let music = PathBuf::from(&arguments[1]);
    let cache = PathBuf::from(&arguments[2]);
    let batch = arguments[3]
        .parse::<usize>()
        .map_err(|error| format!("invalid batch size: {error}"))?
        .clamp(1, 64);
    if !music.is_dir() {
        return Err(format!("music fixture does not exist: {}", music.display()));
    }
    fs::create_dir_all(&cache)
        .map_err(|error| format!("unable to create disposable cache: {error}"))?;

    let mut config = Config::default();
    config.library.music_library = music.to_string_lossy().into_owned();
    config.scan.scan_library_dir = config.library.music_library.clone();
    config.scan.cache_dir = cache.to_string_lossy().into_owned();
    config.scan.temporary_cache_dir = cache.join("temporary").to_string_lossy().into_owned();
    config.scan.log_dir = cache.join("logs").to_string_lossy().into_owned();
    config.scan.index_batch_size = batch;
    let (baseline_rss, _) = memory();

    let interrupt_started = Instant::now();
    let interrupted = catch_unwind(AssertUnwindSafe(|| {
        let _ = media_snapshot_with_progress(&config, true, &mut |status| {
            assert!(
                status.completed_albums < batch as i64,
                "intentional benchmark interruption after durable batch"
            );
            true
        });
    }));
    if interrupted.is_ok() {
        return Err("benchmark interruption did not occur".to_string());
    }
    let interruption_seconds = interrupt_started.elapsed().as_secs_f64();

    let database = database_path(&config.scan.cache_dir);
    let connection = Connection::open(&database)
        .map_err(|error| format!("unable to open benchmark SQLite database: {error}"))?;
    let generation: String = connection
        .query_row(
            "SELECT generation_id FROM catalog_generations WHERE state='building' ORDER BY created_at DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("unable to read interrupted generation: {error}"))?;
    let staged_before_resume: i64 = connection
        .query_row(
            "SELECT count(*) FROM catalog_generation_albums WHERE generation_id=?1",
            [&generation],
            |row| row.get(0),
        )
        .map_err(|error| format!("unable to count durable checkpoints: {error}"))?;
    let live_before_resume: i64 = connection
        .query_row("SELECT count(*) FROM albums", [], |row| row.get(0))
        .map_err(|error| format!("unable to count published Albums: {error}"))?;
    connection
        .execute(
            "UPDATE catalog_generations SET lease_expires_at='2000-01-01T00:00:00.000Z' WHERE generation_id=?1",
            [&generation],
        )
        .map_err(|error| format!("unable to expire benchmark lease: {error}"))?;
    drop(connection);

    let resume_started = Instant::now();
    let mut final_completed = 0_i64;
    let snapshot = media_snapshot_with_progress(&config, true, &mut |status| {
        final_completed = final_completed.max(status.completed_albums);
        true
    })?;
    let resume_seconds = resume_started.elapsed().as_secs_f64();
    let connection = Connection::open(&database)
        .map_err(|error| format!("unable to reopen benchmark SQLite database: {error}"))?;
    let full_details = details(&connection);
    drop(connection);

    let incremental_started = Instant::now();
    let incremental = media_snapshot_incremental(&config)?;
    let incremental_seconds = incremental_started.elapsed().as_secs_f64();
    let connection = Connection::open(&database)
        .map_err(|error| format!("unable to inspect incremental benchmark: {error}"))?;
    let incremental_details = details(&connection);
    let (current_rss, peak_rss) = memory();

    println!(
        "{}",
        json!({
            "client": "windows-sqlite",
            "batch_size": batch,
            "albums": snapshot.albums.len(),
            "tracks": snapshot.albums.iter().map(|album| album.track_count).sum::<i64>(),
            "interruption_seconds": interruption_seconds,
            "staged_before_resume": staged_before_resume,
            "live_before_resume": live_before_resume,
            "resume_seconds": resume_seconds,
            "total_index_seconds": interruption_seconds + resume_seconds,
            "final_completed": final_completed,
            "full_timings": full_details,
            "incremental_seconds": incremental_seconds,
            "incremental_albums": incremental.albums.len(),
            "incremental_timings": incremental_details,
            "baseline_rss_bytes": baseline_rss,
            "current_rss_bytes": current_rss,
            "peak_rss_bytes": peak_rss,
            "peak_rss_delta_bytes": peak_rss.saturating_sub(baseline_rss),
        })
    );
    Ok(())
}
