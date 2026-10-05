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

pub fn auto_ideal_enabled() -> bool {
    review_required()
        && std::env::var("SPLINED_GUI_AUTO_IDEAL")
            .ok()
            .is_some_and(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes"
                )
            })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpscaleOverrides {
    pub adaptive_defaults: bool,
    pub sharpen_percent: i32,
    pub contrast_percent: i32,
    pub exposure_percent: i32,
    pub brightness_percent: i32,
    pub color_temperature: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateDecision {
    Use {
        index: usize,
        upscale: Option<UpscaleOverrides>,
    },
    Bypass,
    Retry {
        artist: String,
        album: String,
    },
    RetryMusicBrainz,
    BackToMusicBrainz,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MusicBrainzMatchDecision {
    Use(usize),
    Search,
    EditAuthority {
        recording_mbid: String,
        artist_mbids: String,
        release_mbid: String,
    },
    LeaveUnchanged,
}

pub fn wait_for_musicbrainz_match_decision() -> Result<MusicBrainzMatchDecision, String> {
    loop {
        let mut answer = String::new();
        io::stdin()
            .read_line(&mut answer)
            .map_err(|error| format!("Unable to read MusicBrainz match decision: {error}"))?;
        if answer.is_empty() {
            return Err("MusicBrainz match input closed before a choice was made.".to_string());
        }
        let trimmed = answer.trim();
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            match value.get("action").and_then(Value::as_str).unwrap_or("") {
                "use_musicbrainz_match" => {
                    let index = value.get("index").and_then(Value::as_u64).ok_or_else(|| {
                        "MusicBrainz match decision did not include an index.".to_string()
                    })?;
                    if index == 0 {
                        return Err("MusicBrainz match indexes start at 1.".to_string());
                    }
                    return Ok(MusicBrainzMatchDecision::Use(index as usize - 1));
                }
                "search_musicbrainz" => return Ok(MusicBrainzMatchDecision::Search),
                "edit_musicbrainz_authority" => {
                    return Ok(MusicBrainzMatchDecision::EditAuthority {
                        recording_mbid: value
                            .get("recording_mbid")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .trim()
                            .to_string(),
                        artist_mbids: value
                            .get("artist_mbids")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .trim()
                            .to_string(),
                        release_mbid: value
                            .get("release_mbid")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .trim()
                            .to_string(),
                    });
                }
                "leave_unchanged" | "bypass" | "skip" => {
                    return Ok(MusicBrainzMatchDecision::LeaveUnchanged);
                }
                _ => {}
            }
        }
        match trimmed.to_ascii_lowercase().as_str() {
            "m" => return Ok(MusicBrainzMatchDecision::Search),
            "b" | "skip" => return Ok(MusicBrainzMatchDecision::LeaveUnchanged),
            value if value.parse::<usize>().is_ok_and(|index| index > 0) => {
                return Ok(MusicBrainzMatchDecision::Use(
                    value.parse::<usize>().unwrap() - 1,
                ));
            }
            _ => {}
        }
    }
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
                    let upscale =
                        value
                            .get("upscale_adaptive_defaults")
                            .map(|_| UpscaleOverrides {
                                adaptive_defaults: value
                                    .get("upscale_adaptive_defaults")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(true),
                                sharpen_percent: value
                                    .get("upscale_sharpen_percent")
                                    .and_then(Value::as_i64)
                                    .unwrap_or(0)
                                    as i32,
                                contrast_percent: value
                                    .get("upscale_contrast_percent")
                                    .and_then(Value::as_i64)
                                    .unwrap_or(0)
                                    as i32,
                                exposure_percent: value
                                    .get("upscale_exposure_percent")
                                    .and_then(Value::as_i64)
                                    .unwrap_or(0)
                                    as i32,
                                brightness_percent: value
                                    .get("upscale_brightness_percent")
                                    .and_then(Value::as_i64)
                                    .unwrap_or(0)
                                    as i32,
                                color_temperature: value
                                    .get("upscale_color_temperature")
                                    .and_then(Value::as_i64)
                                    .unwrap_or(0)
                                    as i32,
                            });
                    return Ok(CandidateDecision::Use {
                        index: index as usize - 1,
                        upscale,
                    });
                }
                "bypass" | "skip" => return Ok(CandidateDecision::Bypass),
                "retry_musicbrainz" => return Ok(CandidateDecision::RetryMusicBrainz),
                "back_musicbrainz" => return Ok(CandidateDecision::BackToMusicBrainz),
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
            "back" => return Ok(CandidateDecision::BackToMusicBrainz),
            value => {
                if let Ok(index) = value.parse::<usize>()
                    && index > 0
                {
                    return Ok(CandidateDecision::Use {
                        index: index - 1,
                        upscale: None,
                    });
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
