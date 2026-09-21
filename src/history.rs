use crate::config::{Config, Mode};
use crate::safe_write::replace_text_file;
use crate::scan::AlbumDirectory;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const COMPLETION_HISTORY_VERSION: u32 = 1;
pub const COMPLETION_HISTORY_FILE: &str = "scan-completed-history.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompletionEntry {
    pub completed_at_unix: f64,
    pub album_fingerprint: String,
    pub policy_fingerprint: String,
    pub outcome: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompletionHistory {
    pub version: u32,
    pub albums: BTreeMap<String, CompletionEntry>,
}

impl Default for CompletionHistory {
    fn default() -> Self {
        Self {
            version: COMPLETION_HISTORY_VERSION,
            albums: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlbumHistoryState {
    New,
    Processed,
    TimeoutActive,
    Bypassed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlbumHistoryStatus {
    pub state: AlbumHistoryState,
    pub completed_at_unix: Option<f64>,
    pub eligible_at_unix: Option<f64>,
    pub remaining: Option<Duration>,
    pub outcome: Option<String>,
}

impl AlbumHistoryStatus {
    pub fn new() -> Self {
        Self {
            state: AlbumHistoryState::New,
            completed_at_unix: None,
            eligible_at_unix: None,
            remaining: None,
            outcome: None,
        }
    }

    pub fn has_history(&self) -> bool {
        self.state != AlbumHistoryState::New
    }

    pub fn eligible_by_default(&self) -> bool {
        self.state == AlbumHistoryState::New
    }
}

impl Default for AlbumHistoryStatus {
    fn default() -> Self {
        Self::new()
    }
}

pub fn completion_history_path(config: &Config) -> PathBuf {
    Path::new(&config.scan.history_dir).join(COMPLETION_HISTORY_FILE)
}

pub fn load_completion_history(config: &Config) -> CompletionHistory {
    if !config.history.enabled {
        return CompletionHistory::default();
    }

    let path = completion_history_path(config);
    let Ok(text) = fs::read_to_string(path) else {
        return CompletionHistory::default();
    };
    let Ok(mut history) = serde_json::from_str::<CompletionHistory>(&text) else {
        return CompletionHistory::default();
    };
    history.version = COMPLETION_HISTORY_VERSION;
    prune_expired_entries(&mut history, config, unix_now());
    history
}

pub fn save_completion_history(config: &Config, history: &CompletionHistory) -> Result<(), String> {
    if !config.history.enabled {
        return Ok(());
    }
    let path = completion_history_path(config);
    let mut persisted = history.clone();
    persisted.version = COMPLETION_HISTORY_VERSION;
    prune_expired_entries(&mut persisted, config, unix_now());
    let body = serde_json::to_string_pretty(&persisted)
        .map_err(|error| format!("Unable to serialize SPLINED completion history: {error}"))?
        + "\n";
    replace_text_file(&path, &body, "scan completion history", |staged| {
        serde_json::from_str::<CompletionHistory>(staged)
            .map(|_| ())
            .map_err(|error| format!("Invalid staged SPLINED completion history: {error}"))
    })
}

pub fn record_scan_completion(
    config: &Config,
    history: &mut CompletionHistory,
    album: &AlbumDirectory,
    sources: &[String],
    outcome: &str,
) -> Result<(), String> {
    // Read mode is a review/test pass. It must not promote an album to
    // processed/bypassed history merely because the user made a candidate
    // decision while testing.
    if !config.history.enabled || config.mode == Mode::Read {
        return Ok(());
    }
    history.albums.insert(
        album_key(&album.path),
        CompletionEntry {
            completed_at_unix: unix_now(),
            album_fingerprint: album_scan_fingerprint(album),
            policy_fingerprint: scan_policy_fingerprint(config, sources),
            outcome: outcome.to_string(),
        },
    );
    save_completion_history(config, history)
}

pub fn album_history_status(
    history: &CompletionHistory,
    album: &AlbumDirectory,
    config: &Config,
    sources: &[String],
    now: f64,
) -> AlbumHistoryStatus {
    if !config.history.enabled {
        return AlbumHistoryStatus::new();
    }

    let Some(entry) = history.albums.get(&album_key(&album.path)) else {
        return AlbumHistoryStatus::new();
    };
    if retention_expired(entry, config, now) {
        return AlbumHistoryStatus::new();
    }

    let age_seconds = (now - entry.completed_at_unix).max(0.0);
    let mut status = AlbumHistoryStatus {
        state: AlbumHistoryState::Processed,
        completed_at_unix: Some(entry.completed_at_unix),
        eligible_at_unix: None,
        remaining: None,
        outcome: Some(entry.outcome.clone()),
    };

    if entry.outcome.to_ascii_lowercase().contains("bypass") {
        status.state = AlbumHistoryState::Bypassed;
        return status;
    }

    let timeout_seconds = config.scan.scan_mode_timeout.0 * 3600.0;
    if timeout_seconds > 0.0
        && age_seconds < timeout_seconds
        && entry.album_fingerprint == album_scan_fingerprint(album)
        && entry.policy_fingerprint == scan_policy_fingerprint(config, sources)
    {
        let remaining_seconds = (timeout_seconds - age_seconds).max(0.0);
        status.state = AlbumHistoryState::TimeoutActive;
        status.eligible_at_unix = Some(entry.completed_at_unix + timeout_seconds);
        status.remaining = Some(Duration::from_secs_f64(remaining_seconds));
    }

    status
}

pub fn prune_expired_entries(history: &mut CompletionHistory, config: &Config, now: f64) {
    if !config.history.enabled {
        history.albums.clear();
        return;
    }
    history
        .albums
        .retain(|_, entry| !retention_expired(entry, config, now));
}

fn retention_expired(entry: &CompletionEntry, config: &Config, now: f64) -> bool {
    let days = config.history.retention_days;
    days > 0 && now - entry.completed_at_unix >= f64::from(days) * 86_400.0
}

pub fn album_scan_fingerprint(album: &AlbumDirectory) -> String {
    let mut files = album.audio_files.clone();
    files.sort_by_key(|path| path.to_string_lossy().to_ascii_lowercase());
    let mut digest = Sha256::new();
    for path in files {
        let payload = match fs::metadata(&path) {
            Ok(metadata) => {
                let modified_ns = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map(|duration| duration.as_nanos())
                    .unwrap_or_default();
                format!(
                    "{}\0{}\0{}\0",
                    path.file_name()
                        .map(|name| name.to_string_lossy())
                        .unwrap_or_default(),
                    metadata.len(),
                    modified_ns
                )
            }
            Err(_) => format!(
                "{}\0unstatable\0",
                path.file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_default()
            ),
        };
        digest.update(payload.as_bytes());
    }
    hex_digest(digest.finalize().as_slice())
}

pub fn scan_policy_fingerprint(config: &Config, sources: &[String]) -> String {
    let payload = serde_json::json!({
        "mode": config.mode,
        "sources": sources,
        "range": config.range,
        "source_policies": config.source_policies,
        "output": config.output,
        "samples": config.samples,
    });
    let encoded = serde_json::to_vec(&payload).unwrap_or_default();
    let mut digest = Sha256::new();
    digest.update(encoded);
    hex_digest(digest.finalize().as_slice())
}

fn album_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

pub fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    fn fixture_album(root: &Path) -> AlbumDirectory {
        let path = root.join("Artist").join("Album");
        fs::create_dir_all(&path).unwrap();
        let track = path.join("01.mp3");
        File::create(&track).unwrap();
        AlbumDirectory {
            path,
            audio_files: vec![track],
        }
    }

    #[test]
    fn timeout_is_absent_when_disabled() {
        let temp = tempfile::tempdir().unwrap();
        let album = fixture_album(temp.path());
        let mut config = Config::default();
        config.scan.scan_mode_timeout.0 = 0.0;
        let sources = vec!["itunes".to_string()];
        let now = 10_000.0;
        let mut history = CompletionHistory::default();
        history.albums.insert(
            album_key(&album.path),
            CompletionEntry {
                completed_at_unix: now - 60.0,
                album_fingerprint: album_scan_fingerprint(&album),
                policy_fingerprint: scan_policy_fingerprint(&config, &sources),
                outcome: "installed".to_string(),
            },
        );
        assert_eq!(
            album_history_status(&history, &album, &config, &sources, now).state,
            AlbumHistoryState::Processed
        );
    }

    #[test]
    fn bypass_precedes_active_timeout() {
        let temp = tempfile::tempdir().unwrap();
        let album = fixture_album(temp.path());
        let config = Config::default();
        let sources = vec!["itunes".to_string()];
        let now = 10_000.0;
        let mut history = CompletionHistory::default();
        history.albums.insert(
            album_key(&album.path),
            CompletionEntry {
                completed_at_unix: now - 60.0,
                album_fingerprint: album_scan_fingerprint(&album),
                policy_fingerprint: scan_policy_fingerprint(&config, &sources),
                outcome: "fallback-bypassed".to_string(),
            },
        );
        assert_eq!(
            album_history_status(&history, &album, &config, &sources, now).state,
            AlbumHistoryState::Bypassed
        );
    }

    #[test]
    fn read_mode_never_changes_completion_history() {
        let temp = tempfile::tempdir().unwrap();
        let album = fixture_album(temp.path());
        let mut config = Config::default();
        config.mode = Mode::Read;
        config.scan.history_dir = temp.path().join("history").to_string_lossy().into_owned();
        let mut history = CompletionHistory::default();

        record_scan_completion(
            &config,
            &mut history,
            &album,
            &["itunes".to_string()],
            "readonly",
        )
        .unwrap();

        assert!(history.albums.is_empty());
        assert!(!completion_history_path(&config).exists());
    }
}
