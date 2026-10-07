use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{
    Mutex, OnceLock,
    mpsc::{Receiver, Sender},
};

static EVENT_SENDER: OnceLock<Mutex<Option<Sender<Value>>>> = OnceLock::new();
static DECISION_SENDER: OnceLock<Mutex<Option<Sender<Value>>>> = OnceLock::new();
static DECISION_RECEIVER: OnceLock<Mutex<Option<Receiver<Value>>>> = OnceLock::new();
static BYPASS_OVERRIDES: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
static CANCELLED: AtomicBool = AtomicBool::new(false);
static IN_PROCESS: AtomicBool = AtomicBool::new(false);
static REVIEW_REQUIRED: AtomicBool = AtomicBool::new(false);
static AUTO_IDEAL: AtomicBool = AtomicBool::new(false);
static SCAN_CONTEXT: OnceLock<Mutex<ScanBridgeContext>> = OnceLock::new();

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanBridgeContext {
    pub compilation_track_path: Option<PathBuf>,
    pub fallback_album: Option<String>,
    pub fallback_artist: Option<String>,
    pub indexed_album_path: Option<PathBuf>,
    pub indexed_album_key: Option<String>,
}

pub fn set_sender(sender: Option<Sender<Value>>) {
    let slot = EVENT_SENDER.get_or_init(|| Mutex::new(None));
    if let Ok(mut current) = slot.lock() {
        *current = sender;
    }
}

pub fn begin_in_process(
    event_sender: Sender<Value>,
    review_required: bool,
    auto_ideal: bool,
) -> Result<(), String> {
    let (decision_sender, decision_receiver) = std::sync::mpsc::channel();
    set_sender(Some(event_sender));
    *DECISION_SENDER
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "SPLINED decision sender is unavailable.".to_string())? =
        Some(decision_sender);
    *DECISION_RECEIVER
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "SPLINED decision receiver is unavailable.".to_string())? =
        Some(decision_receiver);
    REVIEW_REQUIRED.store(review_required, Ordering::SeqCst);
    AUTO_IDEAL.store(review_required && auto_ideal, Ordering::SeqCst);
    IN_PROCESS.store(true, Ordering::SeqCst);
    reset_cancel();
    Ok(())
}

pub fn end_in_process() {
    IN_PROCESS.store(false, Ordering::SeqCst);
    REVIEW_REQUIRED.store(false, Ordering::SeqCst);
    AUTO_IDEAL.store(false, Ordering::SeqCst);
    set_sender(None);
    if let Some(slot) = DECISION_SENDER.get()
        && let Ok(mut current) = slot.lock()
    {
        *current = None;
    }
    if let Some(slot) = DECISION_RECEIVER.get()
        && let Ok(mut current) = slot.lock()
    {
        *current = None;
    }
    set_bypass_overrides(std::iter::empty());
    set_scan_context(ScanBridgeContext::default());
}

pub fn set_scan_context(context: ScanBridgeContext) {
    if let Ok(mut current) = SCAN_CONTEXT
        .get_or_init(|| Mutex::new(ScanBridgeContext::default()))
        .lock()
    {
        *current = context;
    }
}

pub fn scan_context() -> ScanBridgeContext {
    SCAN_CONTEXT
        .get_or_init(|| Mutex::new(ScanBridgeContext::default()))
        .lock()
        .map(|current| current.clone())
        .unwrap_or_default()
}

pub fn submit_decision(value: Value) -> Result<(), String> {
    let slot = DECISION_SENDER
        .get()
        .ok_or_else(|| "No SPLINED scan is waiting for a decision.".to_string())?;
    let sender = slot
        .lock()
        .map_err(|_| "SPLINED decision sender is unavailable.".to_string())?
        .clone()
        .ok_or_else(|| "No SPLINED scan is waiting for a decision.".to_string())?;
    sender
        .send(value)
        .map_err(|_| "The active SPLINED scan no longer accepts decisions.".to_string())
}

fn receive_in_process_decision() -> Result<Value, String> {
    let slot = DECISION_RECEIVER
        .get()
        .ok_or_else(|| "SPLINED in-process decision channel is unavailable.".to_string())?;
    let receiver = slot
        .lock()
        .map_err(|_| "SPLINED decision receiver is unavailable.".to_string())?;
    receiver
        .as_ref()
        .ok_or_else(|| "SPLINED in-process decision channel is closed.".to_string())?
        .recv()
        .map_err(|_| "SPLINED decision channel closed before a choice was made.".to_string())
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
    BYPASS_OVERRIDES
        .get()
        .and_then(|slot| slot.lock().ok())
        .is_some_and(|paths| paths.contains(path))
}

pub fn enabled() -> bool {
    IN_PROCESS.load(Ordering::SeqCst)
}

pub fn review_required() -> bool {
    enabled() && REVIEW_REQUIRED.load(Ordering::SeqCst)
}

pub fn auto_ideal_enabled() -> bool {
    review_required() && AUTO_IDEAL.load(Ordering::SeqCst)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpscaleOverrides {
    pub adaptive_defaults: bool,
    pub picture_percent: i32,
    pub sharpen_percent: i32,
    pub softness_percent: i32,
    pub contrast_percent: i32,
    pub exposure_percent: i32,
    pub brightness_percent: i32,
    pub gamma_percent: i32,
    pub color_temperature: i32,
    pub apply_edit_profile: bool,
    pub edit_existing_cover: bool,
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
    UseAuthority {
        recording_mbid: String,
        artist_mbids: String,
        release_mbid: String,
    },
    LeaveUnchanged,
}

fn parse_musicbrainz_match_decision(
    value: &Value,
) -> Result<Option<MusicBrainzMatchDecision>, String> {
    match value.get("action").and_then(Value::as_str).unwrap_or("") {
        "use_musicbrainz_match" => {
            let index = value.get("index").and_then(Value::as_u64).ok_or_else(|| {
                "MusicBrainz match decision did not include an index.".to_string()
            })?;
            if index == 0 {
                return Err("MusicBrainz match indexes start at 1.".to_string());
            }
            Ok(Some(MusicBrainzMatchDecision::Use(index as usize - 1)))
        }
        "use_musicbrainz_authority" => {
            let release_mbid = value
                .get("release_mbid")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if release_mbid.is_empty() {
                return Err("Edited MusicBrainz authority requires a Release ID.".to_string());
            }
            Ok(Some(MusicBrainzMatchDecision::UseAuthority {
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
                release_mbid,
            }))
        }
        "leave_unchanged" | "bypass" | "skip" => Ok(Some(MusicBrainzMatchDecision::LeaveUnchanged)),
        _ => Ok(None),
    }
}

pub fn wait_for_musicbrainz_match_decision() -> Result<MusicBrainzMatchDecision, String> {
    loop {
        let value = receive_in_process_decision()?;
        if let Some(decision) = parse_musicbrainz_match_decision(&value)? {
            return Ok(decision);
        }
    }
}

pub fn decisions_available() -> bool {
    enabled()
}

pub fn wait_for_candidate_decision() -> Result<CandidateDecision, String> {
    loop {
        let value = receive_in_process_decision()?;
        if let Some(decision) = parse_candidate_decision(&value)? {
            return Ok(decision);
        }
    }
}

fn parse_candidate_decision(value: &Value) -> Result<Option<CandidateDecision>, String> {
    match value.get("action").and_then(Value::as_str).unwrap_or("") {
        "use" => {
            let index = value
                .get("index")
                .and_then(Value::as_u64)
                .ok_or_else(|| "Artwork decision did not include a candidate index.".to_string())?;
            if index == 0 {
                return Err("Artwork candidate indexes start at 1.".to_string());
            }
            let upscale = value
                .get("upscale_adaptive_defaults")
                .map(|_| UpscaleOverrides {
                    adaptive_defaults: value
                        .get("upscale_adaptive_defaults")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                    picture_percent: decision_i32(value, "upscale_picture_percent"),
                    sharpen_percent: decision_i32(value, "upscale_sharpen_percent"),
                    softness_percent: decision_i32(value, "upscale_softness_percent"),
                    contrast_percent: decision_i32(value, "upscale_contrast_percent"),
                    exposure_percent: decision_i32(value, "upscale_exposure_percent"),
                    brightness_percent: decision_i32(value, "upscale_brightness_percent"),
                    gamma_percent: decision_i32(value, "upscale_gamma_percent"),
                    color_temperature: decision_i32(value, "upscale_color_temperature"),
                    apply_edit_profile: value
                        .get("apply_edit_profile")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    edit_existing_cover: value
                        .get("edit_existing_cover")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                });
            Ok(Some(CandidateDecision::Use {
                index: index as usize - 1,
                upscale,
            }))
        }
        "bypass" | "skip" => Ok(Some(CandidateDecision::Bypass)),
        "retry_musicbrainz" => Ok(Some(CandidateDecision::RetryMusicBrainz)),
        "back_musicbrainz" => Ok(Some(CandidateDecision::BackToMusicBrainz)),
        "retry" => {
            let artist = decision_text(value, "artist");
            let album = decision_text(value, "album");
            if artist.is_empty() || album.is_empty() {
                return Err("Fallback retry requires both Artist and Album.".to_string());
            }
            Ok(Some(CandidateDecision::Retry { artist, album }))
        }
        _ => Ok(None),
    }
}

fn decision_i32(value: &Value, key: &str) -> i32 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0) as i32
}

fn decision_text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

pub fn emit(value: Value) {
    if let Some(slot) = EVENT_SENDER.get()
        && let Ok(current) = slot.lock()
        && let Some(sender) = current.as_ref()
    {
        let _ = sender.send(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_edited_musicbrainz_authority_without_turning_it_into_a_result_index() {
        let decision = parse_musicbrainz_match_decision(&json!({
            "action": "use_musicbrainz_authority",
            "recording_mbid": "7b0e3436-afe7-4da7-8d41-b793b8d84b51",
            "artist_mbids": "5f9ee42f-84b1-42bb-a318-09a05b3fcde1",
            "release_mbid": "4f725973-aaf1-4d0d-a775-0a90ed2a0757"
        }))
        .unwrap()
        .unwrap();
        assert_eq!(
            decision,
            MusicBrainzMatchDecision::UseAuthority {
                recording_mbid: "7b0e3436-afe7-4da7-8d41-b793b8d84b51".to_string(),
                artist_mbids: "5f9ee42f-84b1-42bb-a318-09a05b3fcde1".to_string(),
                release_mbid: "4f725973-aaf1-4d0d-a775-0a90ed2a0757".to_string(),
            }
        );
    }
}
