use serde_json::Value;
use std::collections::HashSet;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, mpsc::Sender};

pub const PREFIX: &str = "@@SPLINED_GUI@@";
static EVENT_SENDER: OnceLock<Mutex<Option<Sender<Value>>>> = OnceLock::new();
static BYPASS_OVERRIDES: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
static CANCELLED: AtomicBool = AtomicBool::new(false);

pub fn set_sender(sender: Option<Sender<Value>>) {
    let slot = EVENT_SENDER.get_or_init(|| Mutex::new(None));
    if let Ok(mut current) = slot.lock() {
        *current = sender;
    }
}

pub fn request_cancel() {
    CANCELLED.store(true, Ordering::SeqCst);
}

pub fn reset_cancel() {
    CANCELLED.store(false, Ordering::SeqCst);
}

pub fn cancelled() -> bool {
    CANCELLED.load(Ordering::SeqCst)
}

pub fn set_bypass_overrides(paths: impl IntoIterator<Item = PathBuf>) {
    let slot = BYPASS_OVERRIDES.get_or_init(|| Mutex::new(HashSet::new()));
    if let Ok(mut current) = slot.lock() {
        current.clear();
        current.extend(paths);
    }
}

pub fn bypass_allowed(path: &Path) -> bool {
    if std::env::var("SPLINED_BYPASS_OVERRIDE")
        .ok()
        .is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            )
        })
    {
        return true;
    }
    BYPASS_OVERRIDES
        .get()
        .and_then(|slot| slot.lock().ok())
        .is_some_and(|paths| paths.contains(path))
}

pub fn enabled() -> bool {
    std::env::var_os("SPLINED_GUI_EVENTS").is_some()
}

pub fn review_required() -> bool {
    enabled()
        && std::env::var("SPLINED_GUI_REVIEW")
            .ok()
            .is_some_and(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes"
                )
            })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateDecision {
    Use(usize),
    Bypass,
    Retry { artist: String, album: String },
    RetryMusicBrainz,
}

pub fn decisions_available() -> bool {
    enabled() || io::stdin().is_terminal()
}

pub fn wait_for_candidate_decision() -> Result<CandidateDecision, String> {
    loop {
        let mut answer = String::new();
        io::stdin()
            .read_line(&mut answer)
            .map_err(|error| format!("Unable to read artwork decision: {error}"))?;
        if answer.is_empty() {
            return Err("Artwork decision input closed before a choice was made.".to_string());
        }
        let trimmed = answer.trim();
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            match value.get("action").and_then(Value::as_str).unwrap_or("") {
                "use" => {
                    let index = value.get("index").and_then(Value::as_u64).ok_or_else(|| {
                        "Artwork decision did not include a candidate index.".to_string()
                    })?;
                    if index == 0 {
                        return Err("Artwork candidate indexes start at 1.".to_string());
                    }
                    return Ok(CandidateDecision::Use(index as usize - 1));
                }
                "bypass" | "skip" => return Ok(CandidateDecision::Bypass),
                "retry_musicbrainz" => return Ok(CandidateDecision::RetryMusicBrainz),
                "retry" => {
                    let artist = value
                        .get("artist")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    let album = value
                        .get("album")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    if artist.is_empty() || album.is_empty() {
                        return Err("Fallback retry requires both Artist and Album.".to_string());
                    }
                    return Ok(CandidateDecision::Retry { artist, album });
                }
                _ => {}
            }
        }
        match trimmed.to_ascii_lowercase().as_str() {
            "b" | "bypass" | "skip" => return Ok(CandidateDecision::Bypass),
            "m" => return Ok(CandidateDecision::RetryMusicBrainz),
            value => {
                if let Ok(index) = value.parse::<usize>()
                    && index > 0
                {
                    return Ok(CandidateDecision::Use(index - 1));
                }
            }
        }
        if !enabled() {
            print!("Choose a candidate number or b to bypass: ");
            let _ = io::stdout().flush();
        }
    }
}

pub fn emit(value: Value) {
    if let Some(slot) = EVENT_SENDER.get()
        && let Ok(current) = slot.lock()
        && let Some(sender) = current.as_ref()
    {
        let _ = sender.send(value.clone());
    }
    if !enabled() {
        return;
    }
    println!("{PREFIX}{value}");
    let _ = io::stdout().flush();
}
