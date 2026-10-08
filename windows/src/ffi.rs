//! Stable UTF-8 C ABI used by the WinForms host.
//!
//! Every returned pointer owns one Rust allocation and must be released with
//! `splined_free_string`. Event bytes are borrowed only for the duration of the
//! callback. No panic is allowed to cross this boundary.

use crate::candidate::Candidate;
use crate::config::{Mode, load_config_text_at_root, resolve_sources};
use crate::final_artwork::{install_prepared_artwork, prepare_existing_cover_edit};
use crate::gui_events::{self, ScanBridgeContext};
use crate::local_artwork::{embedded_candidate, embedded_preview_cache_dir};
use crate::media_database::media_snapshot;
use crate::portable::{initialize_app_root, initialize_state_root, state_root};
use crate::range::Range;
use crate::scan_runtime::run_scan_library_read_report;
use serde::Deserialize;
use serde_json::{Value, json};
use std::ffi::{CString, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::slice;
use std::sync::atomic::{AtomicBool, Ordering};

static SCAN_RUNNING: AtomicBool = AtomicBool::new(false);

pub type EventCallback = unsafe extern "C" fn(*const u8, usize, *mut c_void);

#[derive(Debug, Deserialize)]
struct ScanRequest {
    config_text: String,
    path: String,
    indexed_album_path: String,
    indexed_album_key: Option<String>,
    compilation_track_path: Option<String>,
    #[serde(default)]
    bypass_override: bool,
    #[serde(default = "default_true")]
    review_required: bool,
    #[serde(default)]
    auto_ideal: bool,
    fallback_album: Option<String>,
    fallback_artist: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SnapshotRequest {
    config_text: String,
    #[serde(default)]
    refresh: bool,
}

#[derive(Debug, Deserialize)]
struct EmbeddedArtworkRequest {
    config_text: String,
    track_path: String,
}

#[derive(Debug, Deserialize)]
struct ExistingCoverRequest {
    config_text: String,
    path: String,
}

#[derive(Debug, Deserialize)]
struct InitializeRequest {
    application_root: String,
    state_root: String,
}

fn default_true() -> bool {
    true
}

fn request_text<'a>(pointer: *const u8, length: usize) -> Result<&'a str, String> {
    if pointer.is_null() {
        return Err("SPLINED received a null UTF-8 request pointer.".to_string());
    }
    let bytes = unsafe { slice::from_raw_parts(pointer, length) };
    std::str::from_utf8(bytes)
        .map_err(|error| format!("SPLINED received invalid UTF-8 through its native ABI: {error}"))
}

fn owned_json(value: Value) -> *mut c_char {
    let text = serde_json::to_string(&value).unwrap_or_else(|_| {
        "{\"ok\":false,\"error\":\"Unable to serialize the native SPLINED response.\"}".to_string()
    });
    CString::new(text)
        .expect("serialized JSON never contains a NUL byte")
        .into_raw()
}

fn ffi_result(operation: impl FnOnce() -> Result<Value, String>) -> *mut c_char {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(value)) => owned_json(json!({"ok": true, "value": value})),
        Ok(Err(error)) => owned_json(json!({"ok": false, "error": error})),
        Err(_) => owned_json(json!({
            "ok": false,
            "error": "The SPLINED Rust core contained an internal panic at the native boundary."
        })),
    }
}

fn parse_request<T: for<'de> Deserialize<'de>>(
    pointer: *const u8,
    length: usize,
) -> Result<T, String> {
    let text = request_text(pointer, length)?;
    serde_json::from_str(text).map_err(|error| format!("Invalid SPLINED native request: {error}"))
}

fn emit_callback(callback: EventCallback, context: usize, event: &Value) {
    let text = match serde_json::to_vec(event) {
        Ok(text) => text,
        Err(_) => {
            br#"{"event":"scan_error","message":"Unable to serialize a SPLINED event."}"#.to_vec()
        }
    };
    unsafe { callback(text.as_ptr(), text.len(), context as *mut c_void) };
}

#[unsafe(no_mangle)]
pub extern "C" fn splined_api_version() -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn splined_build_identity() -> *mut c_char {
    ffi_result(|| {
        Ok(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "commit": option_env!("SPLINED_BUILD_COMMIT").unwrap_or("unknown")
        }))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn splined_initialize(root_pointer: *const u8, root_length: usize) -> *mut c_char {
    ffi_result(|| {
        let request: InitializeRequest = parse_request(root_pointer, root_length)?;
        let application_root = initialize_app_root(Path::new(&request.application_root))?;
        let state_root = initialize_state_root(Path::new(&request.state_root))?;
        Ok(json!({
            "api_version": 1,
            "application_root": application_root,
            "state_root": state_root
        }))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn splined_media_snapshot(
    request_pointer: *const u8,
    request_length: usize,
) -> *mut c_char {
    ffi_result(|| {
        let request: SnapshotRequest = parse_request(request_pointer, request_length)?;
        let root = state_root()?;
        let config = load_config_text_at_root(&request.config_text, &root)?;
        serde_json::to_value(media_snapshot(&config, request.refresh)?)
            .map_err(|error| format!("Unable to serialize the SQLite media snapshot: {error}"))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn splined_embedded_artwork_preview(
    request_pointer: *const u8,
    request_length: usize,
) -> *mut c_char {
    ffi_result(|| {
        let request: EmbeddedArtworkRequest = parse_request(request_pointer, request_length)?;
        let root = state_root()?;
        let config = load_config_text_at_root(&request.config_text, &root)?;
        let cache = embedded_preview_cache_dir(Path::new(&config.scan.temporary_cache_dir));
        let path = embedded_candidate(Path::new(&request.track_path), &cache)?
            .map(|candidate| candidate.path().to_string_lossy().into_owned());
        Ok(json!(path))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn splined_edit_existing_cover(
    request_pointer: *const u8,
    request_length: usize,
) -> *mut c_char {
    ffi_result(|| {
        let request: ExistingCoverRequest = parse_request(request_pointer, request_length)?;
        let root = state_root()?;
        let config = load_config_text_at_root(&request.config_text, &root)?;
        if config.mode != Mode::Write {
            return Err("Existing-cover editing requires SPLINED Write mode.".to_string());
        }
        let path = Path::new(&request.path);
        let candidate = Candidate::from_file("local", path, 0)?;
        let range = Range {
            min: config.range.min,
            ideal: config.range.ideal,
            max: config.range.max,
            ladder: config.range.ladder,
        };
        let prepared = prepare_existing_cover_edit(
            &candidate,
            path,
            &range,
            candidate.format,
            &config.output,
            true,
        )?;
        install_prepared_artwork(path, &prepared)?;
        Ok(json!({
            "event": "existing_cover_edited",
            "path": path,
            "width": prepared.info.width,
            "height": prepared.info.height,
            "upscale_backend": prepared.info.upscale_backend.as_str(),
            "picture_percent": prepared.info.picture_percent,
            "brightness_percent": prepared.info.brightness_percent,
            "contrast_percent": prepared.info.contrast_percent,
            "exposure_percent": prepared.info.exposure_percent,
            "sharpen_percent": prepared.info.sharpen_percent,
            "softness_percent": prepared.info.softness_percent,
            "gamma_percent": prepared.info.gamma_percent,
            "color_temperature": prepared.info.color_temperature,
        }))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn splined_start_scan(
    request_pointer: *const u8,
    request_length: usize,
    callback: Option<EventCallback>,
    callback_context: *mut c_void,
) -> *mut c_char {
    ffi_result(|| {
        let callback = callback.ok_or_else(|| "SPLINED requires an event callback.".to_string())?;
        let request: ScanRequest = parse_request(request_pointer, request_length)?;
        if request.path.trim().is_empty() {
            return Err("SPLINED scan path cannot be empty.".to_string());
        }
        if SCAN_RUNNING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("A SPLINED scan is already active.".to_string());
        }

        let prepared = (|| {
            let root = state_root()?;
            let mut config = load_config_text_at_root(&request.config_text, &root)?;
            config.scan.scan_library_dir = request.path.clone();
            let sources =
                resolve_sources(&config.sources, &config.source_policies, None, None, &[])?;
            let (event_sender, event_receiver) = std::sync::mpsc::channel::<Value>();
            gui_events::begin_in_process(
                event_sender,
                request.review_required,
                request.auto_ideal,
            )?;
            gui_events::set_bypass_overrides(
                request
                    .bypass_override
                    .then(|| PathBuf::from(&request.path)),
            );
            gui_events::set_scan_context(ScanBridgeContext {
                compilation_track_path: request.compilation_track_path.map(PathBuf::from),
                fallback_album: request.fallback_album,
                fallback_artist: request.fallback_artist,
                indexed_album_path: Some(PathBuf::from(
                    if request.indexed_album_path.trim().is_empty() {
                        &request.path
                    } else {
                        &request.indexed_album_path
                    },
                )),
                indexed_album_key: request.indexed_album_key,
            });
            Ok((config, sources, event_receiver))
        })();

        let (config, sources, event_receiver) = match prepared {
            Ok(value) => value,
            Err(error) => {
                SCAN_RUNNING.store(false, Ordering::SeqCst);
                gui_events::end_in_process();
                return Err(error);
            }
        };

        let context = callback_context as usize;
        std::thread::spawn(move || {
            while let Ok(event) = event_receiver.recv() {
                emit_callback(callback, context, &event);
            }
        });

        std::thread::spawn(move || {
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| format!("Unable to start the in-process runtime: {error}"))?;
                runtime.block_on(run_scan_library_read_report(&config, &sources))
            }));
            let event = if gui_events::cancelled() {
                json!({"event": "scan_idle", "cancelled": true})
            } else {
                match outcome {
                    Ok(Ok(_)) => json!({"event": "scan_idle"}),
                    Ok(Err(message)) => json!({"event": "scan_error", "message": message}),
                    Err(_) => json!({
                        "event": "scan_error",
                        "message": "The SPLINED Rust core contained an internal panic during processing."
                    }),
                }
            };
            gui_events::emit(event);
            gui_events::end_in_process();
            SCAN_RUNNING.store(false, Ordering::SeqCst);
        });
        Ok(json!({"started": true}))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn splined_submit_decision(
    request_pointer: *const u8,
    request_length: usize,
) -> *mut c_char {
    ffi_result(|| {
        let value: Value = parse_request(request_pointer, request_length)?;
        gui_events::submit_decision(value)?;
        Ok(json!(null))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn splined_cancel_scan() -> *mut c_char {
    ffi_result(|| {
        gui_events::request_cancel();
        Ok(json!(null))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn splined_scan_active() -> bool {
    SCAN_RUNNING.load(Ordering::SeqCst)
}

#[unsafe(no_mangle)]
/// Releases a string returned by a SPLINED C ABI function.
///
/// # Safety
///
/// `pointer` must be null or the unmodified pointer returned by this library,
/// and each non-null pointer must be released exactly once.
pub unsafe extern "C" fn splined_free_string(pointer: *mut c_char) {
    if pointer.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        drop(CString::from_raw(pointer));
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffi_failures_are_structured_and_allocations_are_releasable() {
        let pointer = ffi_result(|| Err("fixture failure".to_string()));
        assert!(!pointer.is_null());
        let text = unsafe { std::ffi::CStr::from_ptr(pointer) }
            .to_str()
            .unwrap()
            .to_string();
        unsafe { splined_free_string(pointer) };
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"], "fixture failure");
    }

    #[test]
    fn ffi_allocations_can_be_repeatedly_allocated_and_freed() {
        for index in 0..256 {
            let pointer = ffi_result(|| Ok(json!({"index": index})));
            assert!(!pointer.is_null());
            let text = unsafe { std::ffi::CStr::from_ptr(pointer) }
                .to_str()
                .unwrap()
                .to_string();
            unsafe { splined_free_string(pointer) };
            let value: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(value["ok"], true);
            assert_eq!(value["value"]["index"], index);
        }
        unsafe { splined_free_string(std::ptr::null_mut()) };
    }

    #[test]
    fn ffi_panics_do_not_cross_the_native_boundary() {
        let pointer = ffi_result(|| -> Result<Value, String> { panic!("fixture panic") });
        let text = unsafe { std::ffi::CStr::from_ptr(pointer) }
            .to_str()
            .unwrap()
            .to_string();
        unsafe { splined_free_string(pointer) };
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["ok"], false);
        assert!(value["error"].as_str().unwrap().contains("panic"));
    }

    #[test]
    fn scan_request_preserves_per_album_sqlite_identity() {
        let request: ScanRequest = serde_json::from_value(json!({
            "config_text": "config_version = 5",
            "path": "D:\\\\Music\\\\Artist B\\\\Album B",
            "indexed_album_path": "Z:\\\\Library\\\\Artist B\\\\Album B",
            "indexed_album_key": "album-key-b"
        }))
        .unwrap();
        assert_eq!(request.indexed_album_key.as_deref(), Some("album-key-b"));
        assert_ne!(request.path, request.indexed_album_path);
    }
}
