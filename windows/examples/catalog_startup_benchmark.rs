//! Measure published-catalog startup without invoking refresh or calibration.

use serde_json::json;
use splined_core::config::Config;
use splined_core::media_database::media_snapshot;
use std::env;
use std::path::PathBuf;
use std::time::Instant;

fn main() -> Result<(), String> {
    let arguments = env::args().collect::<Vec<_>>();
    if arguments.len() != 3 {
        return Err(
            "usage: catalog_startup_benchmark <music-root> <existing-cache>".to_string(),
        );
    }
    let music = PathBuf::from(&arguments[1]);
    let cache = PathBuf::from(&arguments[2]);
    let mut config = Config::default();
    config.library.music_library = music.to_string_lossy().into_owned();
    config.scan.scan_library_dir = config.library.music_library.clone();
    config.scan.cache_dir = cache.to_string_lossy().into_owned();
    config.scan.temporary_cache_dir = cache.join("temporary").to_string_lossy().into_owned();
    let started = Instant::now();
    let snapshot = media_snapshot(&config, false)?;
    println!(
        "{}",
        json!({
            "client": "windows-sqlite",
            "operation": "published-catalog-startup",
            "startup_seconds": started.elapsed().as_secs_f64(),
            "albums": snapshot.albums.len(),
            "tracks": snapshot.albums.iter().map(|album| album.track_count).sum::<i64>(),
        })
    );
    Ok(())
}
