//! Python-compatible SQLite Select Media index for the native Windows core.
//!
//! Keep normal indexing Album-oriented: one deterministic representative track
//! supplies tag identity. Per-track reads belong only to processing workflows.

use crate::config::{Config, resolve_sources};
use crate::history::{AlbumHistoryState, album_history_status, load_completion_history, unix_now};
use crate::scan::{AlbumDirectory, inventory_album_directories};
use crate::scan_tags::{AlbumIndexTags, read_album_index_tags};
use image::ImageFormat;
use rusqlite::{
    Connection, ErrorCode, OpenFlags, OptionalExtension, Transaction, named_params, params,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;

#[cfg(windows)]
use windows_sys::Win32::Foundation::{
    ERROR_ALREADY_ASSIGNED, ERROR_DEVICE_ALREADY_REMEMBERED, ERROR_SUCCESS,
};
#[cfg(windows)]
use windows_sys::Win32::NetworkManagement::WNet::{
    CONNECT_TEMPORARY, NETRESOURCEW, RESOURCETYPE_DISK, WNetAddConnection2W,
    WNetCancelConnection2W, WNetGetConnectionW,
};

pub const DB_NAME: &str = "splined.db";
pub const SCHEMA_VERSION: i64 = 2;
const INVENTORY_KEY: &str = "splined-media-library";
const SCHEMA: &str = include_str!("../../python/splined_schema.sql");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MediaSnapshot {
    pub schema_version: i64,
    pub database_path: String,
    pub canonical_root: String,
    pub local_root: String,
    pub albums: Vec<MediaAlbumSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MediaAlbumSnapshot {
    pub album_key: String,
    pub artist: String,
    pub tagged_artist: String,
    pub title: String,
    pub path: String,
    pub representative_file: String,
    pub status: String,
    pub compilation: bool,
    pub track_count: i64,
    pub has_local_artwork: bool,
    pub local_artwork_files: Vec<String>,
    pub processed_at: Option<String>,
    pub timeout_until: Option<String>,
    pub selected_source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMapper {
    canonical_root: String,
    local_root: String,
}

impl PathMapper {
    pub fn new(
        canonical_root: impl Into<String>,
        local_root: impl Into<String>,
    ) -> Result<Self, String> {
        let canonical_root = trim_root(&canonical_root.into());
        let local_root = trim_root(&local_root.into());
        if canonical_root.is_empty() || local_root.is_empty() {
            return Err(
                "SPLINED database path mapping requires canonical and local library roots."
                    .to_string(),
            );
        }
        Ok(Self {
            canonical_root,
            local_root,
        })
    }

    pub fn canonical_root(&self) -> &str {
        &self.canonical_root
    }

    pub fn local_root(&self) -> &str {
        &self.local_root
    }

    pub fn to_local(&self, canonical: &str) -> Result<String, String> {
        map_root(canonical, &self.canonical_root, &self.local_root, true)
    }

    pub fn to_canonical(&self, local: &str) -> Result<String, String> {
        map_root(local, &self.local_root, &self.canonical_root, cfg!(windows))
    }

    fn json_paths_to_local(&self, value: &str) -> Vec<String> {
        serde_json::from_str::<Vec<String>>(value)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|path| self.to_local(&path).ok())
            .collect()
    }
}

#[derive(Debug, Clone)]
struct ExistingAlbum {
    album_key: String,
    artist_name: String,
    artist_sort: String,
    album_name: String,
    album_sort: String,
    album_mbid: String,
    release_group_mbid: String,
    artist_mbid: String,
    release_year: String,
    compilation: bool,
    path: String,
    representative_file: String,
    representative_size: i64,
    representative_mtime_ns: i64,
    tag_signature: String,
    status: String,
    processed_at: Option<String>,
    bypassed: bool,
    timeout_until: Option<String>,
    selected_source: Option<String>,
    created_at: String,
}

#[derive(Debug)]
struct AlbumRow {
    artist_key: String,
    artist_name: String,
    artist_sort: String,
    artist_mbid: String,
    artist_primary_path: String,
    album_key: String,
    album_name: String,
    album_sort: String,
    album_mbid: String,
    release_group_mbid: String,
    release_year: String,
    compilation: bool,
    path: String,
    representative_file: String,
    representative_size: i64,
    representative_mtime_ns: i64,
    tag_signature: String,
    track_count: i64,
    inventory_fingerprint: String,
    status: String,
    cover_found: bool,
    cover_path: String,
    cover_name: String,
    cover_format: String,
    cover_width: Option<u32>,
    cover_height: Option<u32>,
    artwork_jpeg: i64,
    artwork_png: i64,
    artwork_webp: i64,
    artwork_other: i64,
    root_files: i64,
    cover_files: i64,
    cover_names_json: String,
    local_art_json: String,
    other_filenames_json: String,
    webp_found: bool,
    webp_size_mb: f64,
    webp_resolution: String,
    webp_conversion: bool,
    processed_at: Option<String>,
    bypassed: bool,
    timeout_until: Option<String>,
    selected_source: Option<String>,
    created_at: String,
}

#[derive(Debug, Default)]
struct CoverFacts {
    found: bool,
    path: String,
    name: String,
    format: String,
    width: Option<u32>,
    height: Option<u32>,
    jpeg: i64,
    png: i64,
    webp: i64,
    other: i64,
    root_files: i64,
    cover_files: i64,
    cover_names: Vec<String>,
    local_art: Vec<String>,
    other_filenames: Vec<String>,
    webp_size_mb: f64,
    webp_resolution: String,
    webp_conversion: bool,
}

#[derive(Debug, Clone)]
pub struct RuntimeArtworkMaterial {
    pub path: PathBuf,
    pub format: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AlbumOutcomeTiming {
    pub database_ms: u64,
    pub filesystem_ms: u64,
    pub update_ms: u64,
    pub aggregate_ms: u64,
    pub audit_ms: u64,
    pub commit_ms: u64,
    pub total_ms: u64,
}

enum OutcomeMaterial<'a> {
    Inspect,
    Runtime(Option<&'a RuntimeArtworkMaterial>),
}

pub fn database_path(cache_dir: impl AsRef<Path>) -> PathBuf {
    cache_dir.as_ref().join(DB_NAME)
}

struct DatabasePathAccess {
    sqlite_path: PathBuf,
    #[cfg(windows)]
    _temporary_mapping: Option<TemporaryDriveMapping>,
}

impl DatabasePathAccess {
    fn new(path: &Path, shared: bool) -> Result<Self, String> {
        #[cfg(windows)]
        if shared {
            // Preserve the active network provider (including NFS) by
            // discovering any current drive connection for this UNC prefix.
            // The configured path remains independent of its drive letter.
            if let Some(sqlite_path) = existing_mapped_path(path) {
                return Ok(Self {
                    sqlite_path,
                    _temporary_mapping: None,
                });
            }
            if let Some((remote_root, relative_path)) = split_unc_path(path) {
                let mapping = TemporaryDriveMapping::connect(&remote_root)?;
                return Ok(Self {
                    sqlite_path: PathBuf::from(format!(
                        "{}\\{}",
                        mapping.local_name, relative_path
                    )),
                    _temporary_mapping: Some(mapping),
                });
            }
        }

        Ok(Self {
            sqlite_path: path.to_path_buf(),
            #[cfg(windows)]
            _temporary_mapping: None,
        })
    }

    fn path(&self) -> &Path {
        &self.sqlite_path
    }
}

#[cfg(windows)]
struct TemporaryDriveMapping {
    local_name: String,
}

#[cfg(windows)]
impl TemporaryDriveMapping {
    fn connect(remote_root: &str) -> Result<Self, String> {
        let mut remote = wide_null(remote_root);
        for letter in (b'D'..=b'Z').rev() {
            let local_name = format!("{}:", char::from(letter));
            if Path::new(&format!("{local_name}\\")).exists() {
                continue;
            }
            let mut local = wide_null(&local_name);
            let resource = NETRESOURCEW {
                dwType: RESOURCETYPE_DISK,
                lpLocalName: local.as_mut_ptr(),
                lpRemoteName: remote.as_mut_ptr(),
                ..Default::default()
            };
            // SAFETY: NETRESOURCEW points to live, NUL-terminated UTF-16
            // buffers for the duration of this call. Null credentials reuse
            // the caller's existing Windows SMB/Tailscale session.
            let result = unsafe {
                WNetAddConnection2W(
                    &resource,
                    std::ptr::null(),
                    std::ptr::null(),
                    CONNECT_TEMPORARY,
                )
            };
            if result == ERROR_SUCCESS {
                return Ok(Self { local_name });
            }
            if result != ERROR_ALREADY_ASSIGNED && result != ERROR_DEVICE_ALREADY_REMEMBERED {
                return Err(format!(
                    "Unable to create temporary SQLite drive adapter for {remote_root}: {} (Windows error {result}).",
                    std::io::Error::from_raw_os_error(result as i32)
                ));
            }
        }
        Err(format!(
            "Unable to create temporary SQLite drive adapter for {remote_root}: no drive letter is available."
        ))
    }
}

#[cfg(windows)]
impl Drop for TemporaryDriveMapping {
    fn drop(&mut self) {
        let local = wide_null(&self.local_name);
        // SAFETY: `local` is a live, NUL-terminated UTF-16 buffer. SQLite
        // connections are declared after the mapping guard and have already
        // been dropped before this guard is released.
        let _ = unsafe { WNetCancelConnection2W(local.as_ptr(), 0, 0) };
    }
}

#[cfg(windows)]
fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn existing_mapped_path(path: &Path) -> Option<PathBuf> {
    let unc_path = path.to_string_lossy();
    if !unc_path.replace('/', "\\").starts_with("\\\\") {
        return None;
    }
    for letter in (b'D'..=b'Z').rev() {
        let local_name = format!("{}:", char::from(letter));
        let local = wide_null(&local_name);
        let mut remote = vec![0_u16; 32_768];
        let mut remote_len = u32::try_from(remote.len()).ok()?;
        // SAFETY: all pointers reference live UTF-16 buffers and
        // `remote_len` describes the writable destination capacity.
        let result =
            unsafe { WNetGetConnectionW(local.as_ptr(), remote.as_mut_ptr(), &mut remote_len) };
        if result != ERROR_SUCCESS {
            continue;
        }
        let end = remote
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(remote.len());
        let remote_name = String::from_utf16_lossy(&remote[..end]);
        if let Some(mapped) = translate_unc_to_drive(&unc_path, &local_name, &remote_name) {
            return Some(PathBuf::from(mapped));
        }
    }
    None
}

#[cfg(windows)]
fn translate_unc_to_drive(unc_path: &str, local_name: &str, remote_name: &str) -> Option<String> {
    let unc = unc_path.replace('/', "\\");
    let remote = remote_name.replace('/', "\\");
    let remote = remote.trim_end_matches('\\');
    let unc_folded = unc.to_lowercase();
    let remote_folded = remote.to_lowercase();
    let boundary = format!("{remote_folded}\\");
    let relative = if unc_folded == remote_folded {
        ""
    } else if unc_folded.starts_with(&boundary) {
        &unc[remote.len() + 1..]
    } else {
        return None;
    };
    if relative.is_empty() {
        Some(format!("{local_name}\\"))
    } else {
        Some(format!("{local_name}\\{relative}"))
    }
}

#[cfg(windows)]
fn split_unc_path(path: &Path) -> Option<(String, String)> {
    let normalized = path.to_string_lossy().replace('/', "\\");
    let remainder = normalized.strip_prefix("\\\\")?;
    let mut parts = remainder.split('\\').filter(|part| !part.is_empty());
    let server = parts.next()?;
    let share = parts.next()?;
    let relative = parts.collect::<Vec<_>>().join("\\");
    if relative.is_empty() {
        return None;
    }
    Some((format!("\\\\{server}\\{share}"), relative))
}

pub fn media_snapshot(config: &Config, refresh: bool) -> Result<MediaSnapshot, String> {
    let local_root = config.library.music_library.trim();
    if local_root.is_empty() {
        return Err(
            "[library].music_library must be configured before loading Select Media.".to_string(),
        );
    }
    let db_path = database_path(&config.scan.cache_dir);
    let database_access = DatabasePathAccess::new(&db_path, config.scan.sqlite_shared)?;

    // A normal Select Media startup is a projection of an already-published
    // index. Keep that path strictly read-only: in particular, do not request
    // a journal-mode transition or reapply the schema over an SMB share. This
    // also lets simultaneous GUI warm-load requests coexist as readers. Only
    // an explicit refresh (or a missing/uninitialized index) enters the
    // writable initialization path below.
    if (!refresh || config.scan.sqlite_shared) && database_access.path().exists() {
        let (connection, current) = open_database_read_only(database_access.path())?;
        if current == SCHEMA_VERSION {
            let saved_signature = read_signature(&connection)?;
            validate_signature(config, saved_signature.as_ref())?;
            let usable = saved_signature.is_some()
                && connection
                    .query_row("SELECT count(*) FROM albums", [], |row| {
                        row.get::<_, i64>(0)
                    })
                    .unwrap_or(0)
                    > 0;
            if usable {
                let canonical_root = saved_signature
                    .as_ref()
                    .and_then(|value| value.get("library_root"))
                    .and_then(Value::as_str)
                    .unwrap_or(local_root);
                let mapper = PathMapper::new(canonical_root, local_root)?;
                return load_snapshot(&connection, &db_path, &mapper);
            }
        } else if current != 0 {
            return Err(incompatible_schema(current));
        }
    }

    if config.scan.sqlite_shared {
        return Err(
            "The shared SPLINED database has no usable Python inventory. Refresh the library index from Python, then reload it in Windows."
                .to_string(),
        );
    }

    let mut connection = open_database(database_access.path(), config.scan.sqlite_shared)?;
    let saved_signature = read_signature(&connection)?;
    let canonical_root = saved_signature
        .as_ref()
        .and_then(|value| value.get("library_root"))
        .and_then(Value::as_str)
        .unwrap_or(local_root)
        .to_string();
    validate_signature(config, saved_signature.as_ref())?;
    let mapper = PathMapper::new(canonical_root, local_root)?;

    let usable = saved_signature.is_some()
        && connection
            .query_row("SELECT count(*) FROM albums", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap_or(0)
            > 0;
    if refresh || !usable {
        rebuild_index(
            &mut connection,
            config,
            &mapper,
            if refresh { "refresh" } else { "initial-build" },
        )?;
    }
    load_snapshot(&connection, &db_path, &mapper)
}

pub fn record_album_outcome(
    config: &Config,
    local_album_path: &Path,
    outcome: &str,
    selected_source: Option<&str>,
) -> Result<(), String> {
    record_album_outcome_inner(
        config,
        local_album_path,
        None,
        outcome,
        selected_source,
        OutcomeMaterial::Inspect,
    )
    .map(|_| ())
}

pub fn record_album_outcome_from_runtime(
    config: &Config,
    local_album_path: &Path,
    indexed_album_path: Option<&Path>,
    outcome: &str,
    selected_source: Option<&str>,
    material: Option<&RuntimeArtworkMaterial>,
) -> Result<AlbumOutcomeTiming, String> {
    record_album_outcome_inner(
        config,
        local_album_path,
        indexed_album_path,
        outcome,
        selected_source,
        OutcomeMaterial::Runtime(material),
    )
}

fn record_album_outcome_inner(
    config: &Config,
    local_album_path: &Path,
    indexed_album_path: Option<&Path>,
    outcome: &str,
    selected_source: Option<&str>,
    material_source: OutcomeMaterial<'_>,
) -> Result<AlbumOutcomeTiming, String> {
    let total_started = Instant::now();
    if config.mode == crate::config::Mode::Read {
        return Ok(AlbumOutcomeTiming::default());
    }
    let database_started = Instant::now();
    let db_path = database_path(&config.scan.cache_dir);
    let database_access = DatabasePathAccess::new(&db_path, config.scan.sqlite_shared)?;
    if !database_access.path().exists() {
        return Err(format!(
            "SPLINED database is missing: {}",
            db_path.display()
        ));
    }
    let mut connection = open_database(database_access.path(), config.scan.sqlite_shared)?;
    let Some(signature) = read_signature(&connection)? else {
        return Err("SPLINED database has no published media inventory.".to_string());
    };
    validate_signature(config, Some(&signature))?;
    let canonical_root = signature
        .get("library_root")
        .and_then(Value::as_str)
        .ok_or_else(|| "SPLINED database signature has no canonical library_root.".to_string())?;
    let mapper = PathMapper::new(canonical_root, &config.library.music_library)?;
    let canonical_album = mapper.to_canonical(
        &indexed_album_path
            .unwrap_or(local_album_path)
            .to_string_lossy(),
    )?;
    let bypassed = outcome.to_ascii_lowercase().contains("bypass");
    let database_ms = elapsed_ms(database_started);
    let filesystem_started = Instant::now();
    let inspected_material = match &material_source {
        OutcomeMaterial::Inspect => inventory_album_directories(local_album_path, &[])?
            .albums
            .into_iter()
            .find(|album| album.path == local_album_path)
            .map(|album| inspect_cover(&album, &config.output.file_name, &mapper))
            .transpose()?,
        OutcomeMaterial::Runtime(_) => None,
    };
    let filesystem_ms = elapsed_ms(filesystem_started);
    let transaction = connection
        .transaction()
        .map_err(db_error("begin Album outcome transaction"))?;
    let now = sqlite_now(&transaction)?;
    let timeout_until = if !bypassed && config.scan.scan_mode_timeout.0 > 0.0 {
        let eligible = unix_now() + config.scan.scan_mode_timeout.0 * 3600.0;
        Some(
            transaction
                .query_row(
                    "SELECT strftime('%Y-%m-%dT%H:%M:%SZ', ?, 'unixepoch')",
                    [eligible],
                    |row| row.get::<_, String>(0),
                )
                .map_err(db_error("calculate Album timeout"))?,
        )
    } else {
        None
    };
    let status = if bypassed {
        "bypassed"
    } else if timeout_until.is_some() {
        "timeout"
    } else {
        "processed"
    };
    let update_started = Instant::now();
    let changed = match (&material_source, inspected_material) {
        (_, Some(cover)) => transaction.execute(
            "UPDATE albums SET status=?, processed_at=?, bypassed=?, timeout_until=?, \
             selected_source=COALESCE(?, selected_source), cover_found=?, cover_path=?, \
             cover_name=?, cover_format=?, cover_width=?, cover_height=?, artwork_jpeg=?, \
             artwork_png=?, artwork_webp=?, artwork_other=?, root_files=?, cover_files=?, \
             cover_names_json=?, local_art_json=?, other_filenames_json=?, webp_found=?, \
             webp_size_mb=?, webp_resolution=?, webp_conversion=?, updated_at=?, last_seen_at=? \
             WHERE path=? COLLATE NOCASE",
            params![
                status,
                now,
                i64::from(bypassed),
                timeout_until,
                selected_source,
                i64::from(cover.found),
                cover.path,
                cover.name,
                cover.format,
                cover.width,
                cover.height,
                cover.jpeg,
                cover.png,
                cover.webp,
                cover.other,
                cover.root_files,
                cover.cover_files,
                serde_json::to_string(&cover.cover_names).unwrap_or_else(|_| "[]".to_string()),
                serde_json::to_string(&cover.local_art).unwrap_or_else(|_| "[]".to_string()),
                serde_json::to_string(&cover.other_filenames).unwrap_or_else(|_| "[]".to_string()),
                i64::from(cover.webp > 0),
                cover.webp_size_mb,
                cover.webp_resolution,
                i64::from(cover.webp_conversion),
                now,
                now,
                canonical_album
            ],
        ),
        (OutcomeMaterial::Runtime(Some(material)), None) => {
            let canonical_cover = canonical_runtime_path(
                &mapper,
                local_album_path,
                &canonical_album,
                &material.path,
            )?;
            let cover_name = material
                .path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            transaction.execute(
                "UPDATE albums SET status=?, processed_at=?, bypassed=?, timeout_until=?, \
                 selected_source=COALESCE(?, selected_source), cover_found=1, cover_path=?, \
                 cover_name=?, cover_format=?, cover_width=?, cover_height=?, updated_at=?, \
                 last_seen_at=? WHERE path=? COLLATE NOCASE",
                params![
                    status,
                    now,
                    i64::from(bypassed),
                    timeout_until,
                    selected_source,
                    canonical_cover,
                    cover_name,
                    material.format,
                    material.width,
                    material.height,
                    now,
                    now,
                    canonical_album
                ],
            )
        }
        _ => transaction.execute(
            "UPDATE albums SET status=?, processed_at=?, bypassed=?, timeout_until=?, \
             selected_source=COALESCE(?, selected_source), updated_at=?, last_seen_at=? \
             WHERE path=? COLLATE NOCASE",
            params![
                status,
                now,
                i64::from(bypassed),
                timeout_until,
                selected_source,
                now,
                now,
                canonical_album
            ],
        ),
    }
    .map_err(db_error("update Album outcome"))?;
    let update_ms = elapsed_ms(update_started);
    if changed == 0 {
        return Err(format!(
            "Album outcome has no matching shared inventory row: {canonical_album}"
        ));
    }
    let aggregate_started = Instant::now();
    refresh_album_artist_aggregate(&transaction, &canonical_album)?;
    let aggregate_ms = elapsed_ms(aggregate_started);
    let audit_started = Instant::now();
    transaction
        .execute(
            "INSERT INTO cache_history(cache_key, cache_type, album_key, action, payload_json, splined_version, event_at) \
             SELECT ?, 'windows-runtime', album_key, ?, ?, ?, ? FROM albums WHERE path=? COLLATE NOCASE",
            params![canonical_album, outcome, json!({"source": selected_source}).to_string(), env!("CARGO_PKG_VERSION"), now, canonical_album],
        )
        .map_err(db_error("write Album audit"))?;
    let audit_ms = elapsed_ms(audit_started);
    let commit_started = Instant::now();
    transaction
        .commit()
        .map_err(db_error("commit Album outcome transaction"))?;
    let commit_ms = elapsed_ms(commit_started);
    Ok(AlbumOutcomeTiming {
        database_ms,
        filesystem_ms,
        update_ms,
        aggregate_ms,
        audit_ms,
        commit_ms,
        total_ms: elapsed_ms(total_started),
    })
}

fn canonical_runtime_path(
    mapper: &PathMapper,
    local_album_path: &Path,
    canonical_album: &str,
    local_material_path: &Path,
) -> Result<String, String> {
    if let Ok(relative) = local_material_path.strip_prefix(local_album_path) {
        let separator = if canonical_album.contains('\\')
            || (canonical_album.len() >= 2 && canonical_album.as_bytes()[1] == b':')
        {
            '\\'
        } else {
            '/'
        };
        let relative = relative
            .to_string_lossy()
            .replace(['/', '\\'], &separator.to_string());
        let relative = relative.trim_start_matches(['/', '\\']);
        if relative.is_empty() {
            return Ok(canonical_album.to_string());
        }
        return Ok(format!(
            "{}{}{}",
            canonical_album.trim_end_matches(['/', '\\']),
            separator,
            relative
        ));
    }
    mapper.to_canonical(&local_material_path.to_string_lossy())
}

pub fn read_runtime_cache_payload(
    config: &Config,
    cache_key: &str,
) -> Result<Option<String>, String> {
    let db_path = database_path(&config.scan.cache_dir);
    let database_access = DatabasePathAccess::new(&db_path, config.scan.sqlite_shared)?;
    if !database_access.path().exists() {
        return Ok(None);
    }
    let (connection, current) = open_database_read_only(database_access.path())?;
    if current != SCHEMA_VERSION {
        return Err(incompatible_schema(current));
    }
    connection
        .query_row(
            "SELECT payload_json FROM cache_entries WHERE cache_key=?",
            [cache_key],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_error("read SQLite runtime state"))
}

pub fn write_runtime_cache_payload(
    config: &Config,
    cache_key: &str,
    cache_type: &str,
    payload_json: &str,
) -> Result<(), String> {
    if config.mode == crate::config::Mode::Read {
        return Ok(());
    }
    let db_path = database_path(&config.scan.cache_dir);
    let database_access = DatabasePathAccess::new(&db_path, config.scan.sqlite_shared)?;
    if !database_access.path().exists() {
        return Err(format!(
            "SPLINED database is missing: {}",
            db_path.display()
        ));
    }
    let connection = open_database(database_access.path(), config.scan.sqlite_shared)?;
    let now = sqlite_now(&connection)?;
    connection
        .execute(
            "INSERT INTO cache_entries(cache_key, cache_type, album_key, payload_json, splined_version, created_at, updated_at) \
             VALUES(?, ?, NULL, ?, ?, ?, ?) \
             ON CONFLICT(cache_key) DO UPDATE SET cache_type=excluded.cache_type, payload_json=excluded.payload_json, \
             splined_version=excluded.splined_version, updated_at=excluded.updated_at",
            params![cache_key, cache_type, payload_json, env!("CARGO_PKG_VERSION"), now, now],
        )
        .map_err(db_error("write SQLite runtime state"))?;
    Ok(())
}

fn open_database(path: &Path, shared: bool) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Unable to create SPLINED cache directory {}: {error}",
                parent.display()
            )
        })?;
    }
    let connection = Connection::open(path).map_err(|error| {
        format!(
            "Unable to open SPLINED database {}: {error}",
            path.display()
        )
    })?;
    connection
        .busy_timeout(Duration::from_secs(30))
        .map_err(db_error("configure SQLite busy timeout"))?;
    let current: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(db_error("read SQLite schema version"))?;
    if current != 0 && current != SCHEMA_VERSION {
        return Err(incompatible_schema(current));
    }
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(db_error("enable SQLite foreign keys"))?;
    let journal = if shared { "delete" } else { "wal" };
    let mut actual_journal: String = connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .map_err(db_error("read SQLite journal mode"))?;
    if !actual_journal.eq_ignore_ascii_case(journal) {
        connection
            .pragma_update(None, "journal_mode", journal)
            .map_err(db_error("configure SQLite journal mode"))?;
        actual_journal = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .map_err(db_error("verify SQLite journal mode"))?;
    }
    if !actual_journal.eq_ignore_ascii_case(journal) {
        return Err(format!(
            "SPLINED SQLite journal mode did not activate: expected {journal}, received {actual_journal}."
        ));
    }
    connection
        .pragma_update(None, "synchronous", if shared { "FULL" } else { "NORMAL" })
        .map_err(db_error("configure SQLite synchronization"))?;
    if current == 0 {
        connection
            .execute_batch(SCHEMA)
            .map_err(db_error("apply shared schema"))?;
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(db_error("set SQLite schema version"))?;
    }
    Ok(connection)
}

fn open_database_read_only(path: &Path) -> Result<(Connection, i64), String> {
    let connection = open_read_only_connection(path)?;
    connection
        .busy_timeout(Duration::from_secs(30))
        .map_err(db_error("configure SQLite read timeout"))?;
    let current = match connection.pragma_query_value(None, "user_version", |row| row.get(0)) {
        Ok(current) => current,
        Err(error) if sqlite_read_only_error(&error) => {
            // A process stopped during a rollback-journal transaction can
            // leave a hot journal. SQLite must briefly open read/write to
            // roll that transaction back before normal read-only hydration.
            drop(connection);
            recover_interrupted_transaction(path)?;
            let recovered = open_read_only_connection(path)?;
            recovered
                .busy_timeout(Duration::from_secs(30))
                .map_err(db_error("configure recovered SQLite read timeout"))?;
            let current = recovered
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .map_err(db_error("read recovered SQLite schema version"))?;
            return Ok((recovered, current));
        }
        Err(error) => return Err(db_error("read SQLite schema version")(error)),
    };
    Ok((connection, current))
}

fn open_read_only_connection(path: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| {
        format!(
            "Unable to open existing SPLINED database read-only {}: {error}",
            path.display()
        )
    })
}

fn sqlite_read_only_error(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(details, _) if details.code == ErrorCode::ReadOnly
    )
}

fn recover_interrupted_transaction(path: &Path) -> Result<(), String> {
    let connection =
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(|error| {
            format!(
                "Unable to recover interrupted SPLINED database transaction {}: {error}",
                path.display()
            )
        })?;
    connection
        .busy_timeout(Duration::from_secs(30))
        .map_err(db_error("configure SQLite recovery timeout"))?;
    let current: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(db_error("recover interrupted SQLite transaction"))?;
    if current != SCHEMA_VERSION {
        return Err(incompatible_schema(current));
    }
    Ok(())
}

fn incompatible_schema(current: i64) -> String {
    format!(
        "SPLINED database schema v{current} is incompatible with required v{SCHEMA_VERSION}; the database was not changed."
    )
}

fn read_signature(connection: &Connection) -> Result<Option<Value>, String> {
    let text = connection
        .query_row(
            "SELECT signature_json FROM picker_inventory WHERE inventory_key=?",
            [INVENTORY_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(db_error("read media-index signature"))?;
    text.map(|value| {
        serde_json::from_str(&value)
            .map_err(|error| format!("SPLINED media-index signature is invalid JSON: {error}"))
    })
    .transpose()
}

fn expected_signature(config: &Config, canonical_root: &str) -> Value {
    let mut ignored = config
        .library
        .ignored_subs
        .iter()
        .map(|value| casefold(value))
        .collect::<Vec<_>>();
    ignored.sort();
    json!({
        "schema_version": SCHEMA_VERSION,
        "library_root": canonical_root,
        "ignored_subs": ignored,
        "cover_name": casefold(&config.output.file_name),
    })
}

fn validate_signature(config: &Config, saved: Option<&Value>) -> Result<(), String> {
    let Some(saved) = saved else {
        return Ok(());
    };
    let canonical_root = saved
        .get("library_root")
        .and_then(Value::as_str)
        .ok_or_else(|| "SPLINED database signature is missing library_root.".to_string())?;
    let expected = expected_signature(config, canonical_root);
    // A shared database is a Python-published inventory. Windows maps its own
    // local library root onto that inventory and must not impose its local
    // scan exclusions or output filename on Python's indexing policy.
    let fields: &[&str] = if config.scan.sqlite_shared {
        &["schema_version"]
    } else {
        &["schema_version", "ignored_subs", "cover_name"]
    };
    for field in fields {
        if saved.get(field) != expected.get(field) {
            return Err(format!(
                "SPLINED shared database is incompatible for {field}; the database was not rebuilt or modified."
            ));
        }
    }
    Ok(())
}

fn rebuild_index(
    connection: &mut Connection,
    config: &Config,
    mapper: &PathMapper,
    reason: &str,
) -> Result<(), String> {
    let inventory =
        inventory_album_directories(Path::new(mapper.local_root()), &config.library.ignored_subs)?;
    let existing = load_existing(connection)?;
    let completion_history = load_completion_history(config);
    let sources = resolve_sources(&config.sources, &config.source_policies, None, None, &[])
        .unwrap_or_else(|_| config.sources.cover_sources.clone());
    let now = sqlite_now(connection)?;
    let mut used_keys = HashMap::<String, String>::new();
    let mut rows = Vec::with_capacity(inventory.albums.len());
    let mut tag_reads = 0_i64;
    let mut tag_reuses = 0_i64;

    for album in &inventory.albums {
        let representative = album.audio_files.first().ok_or_else(|| {
            format!(
                "Album has no representative track: {}",
                album.path.display()
            )
        })?;
        let canonical_path = mapper.to_canonical(&album.path.to_string_lossy())?;
        let canonical_representative = mapper.to_canonical(&representative.to_string_lossy())?;
        let metadata = fs::metadata(representative).ok();
        let size = metadata
            .as_ref()
            .map(|value| i64::try_from(value.len()).unwrap_or(i64::MAX))
            .unwrap_or(0);
        let mtime = metadata.as_ref().map(modified_ns).unwrap_or(0);
        let prior = existing.get(&canonical_path.to_lowercase());
        let reuse = prior.is_some_and(|value| {
            representative_unchanged(value, &canonical_representative, size, mtime)
        });
        let (tags, prior_key, prior_status) = if let Some(prior) = prior.filter(|_| reuse) {
            tag_reuses += 1;
            (
                AlbumIndexTags {
                    album: prior.album_name.clone(),
                    album_artist: prior.artist_name.clone(),
                    artist_sort: prior.artist_sort.clone(),
                    album_sort: prior.album_sort.clone(),
                    musicbrainz_album_id: prior.album_mbid.clone(),
                    musicbrainz_release_group_id: prior.release_group_mbid.clone(),
                    musicbrainz_album_artist_id: prior.artist_mbid.clone(),
                    year: prior.release_year.clone(),
                    compilation: prior.compilation,
                },
                Some(prior.album_key.clone()),
                Some(prior.clone()),
            )
        } else {
            tag_reads += 1;
            (
                read_album_index_tags(representative).unwrap_or_default(),
                None,
                prior.cloned(),
            )
        };
        let physical_artist = physical_artist_name(mapper.local_root(), &album.path);
        let artist_name = nonempty(&tags.album_artist, &physical_artist);
        let album_name = nonempty(
            &tags.album,
            album
                .path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("Unknown Album"),
        );
        let artist_key = artist_key(&artist_name, &tags.musicbrainz_album_artist_id);
        let base_key = album_key(&artist_key, &album_name, &tags);
        let mut logical_key = prior_key
            .filter(|value| value == &base_key || value.starts_with(&(base_key.clone() + ":copy:")))
            .unwrap_or(base_key.clone());
        if let Some(other_path) = used_keys.get(&logical_key)
            && !other_path.eq_ignore_ascii_case(&canonical_path)
        {
            logical_key = format!(
                "{base_key}:copy:{}",
                short_hash(relative_text(&canonical_path, mapper.canonical_root()))
            );
        }
        used_keys.insert(logical_key.clone(), canonical_path.clone());
        let previous = prior_status.clone().or_else(|| {
            existing
                .values()
                .find(|value| value.album_key.eq_ignore_ascii_case(&logical_key))
                .cloned()
        });
        let cover = inspect_cover(album, &config.output.file_name, mapper)?;
        let history =
            album_history_status(&completion_history, album, config, &sources, unix_now());
        let (status, processed_at, bypassed, timeout_until) = match history.state {
            AlbumHistoryState::Bypassed => (
                "bypassed".to_string(),
                history.completed_at_unix.map(|value| value.to_string()),
                true,
                None,
            ),
            AlbumHistoryState::TimeoutActive => (
                "timeout".to_string(),
                history.completed_at_unix.map(|value| value.to_string()),
                false,
                history.eligible_at_unix.map(|value| value.to_string()),
            ),
            AlbumHistoryState::Processed => (
                "processed".to_string(),
                history.completed_at_unix.map(|value| value.to_string()),
                false,
                None,
            ),
            AlbumHistoryState::New => previous
                .as_ref()
                .map(|value| {
                    (
                        value.status.clone(),
                        value.processed_at.clone(),
                        value.bypassed,
                        value.timeout_until.clone(),
                    )
                })
                .unwrap_or_else(|| {
                    (
                        if cover.found {
                            "processed"
                        } else {
                            "unprocessed"
                        }
                        .to_string(),
                        None,
                        false,
                        None,
                    )
                }),
        };
        let tag_signature = if reuse {
            prior_status
                .as_ref()
                .map(|value| value.tag_signature.clone())
                .unwrap_or_default()
        } else {
            tag_hash(&tags)
        };
        rows.push(AlbumRow {
            artist_key,
            artist_name: artist_name.clone(),
            artist_sort: nonempty(&tags.artist_sort, &artist_name),
            artist_mbid: tags.musicbrainz_album_artist_id.clone(),
            artist_primary_path: mapper
                .to_canonical(&top_artist_path(mapper.local_root(), &album.path))?,
            album_key: logical_key,
            album_name: album_name.clone(),
            album_sort: nonempty(&tags.album_sort, &album_name),
            album_mbid: tags.musicbrainz_album_id.clone(),
            release_group_mbid: tags.musicbrainz_release_group_id.clone(),
            release_year: tags.year.clone(),
            compilation: tags.compilation,
            path: canonical_path,
            representative_file: canonical_representative,
            representative_size: size,
            representative_mtime_ns: mtime,
            tag_signature,
            track_count: album.audio_files.len() as i64,
            inventory_fingerprint: inventory_fingerprint(album),
            status,
            cover_found: cover.found,
            cover_path: cover.path,
            cover_name: cover.name,
            cover_format: cover.format,
            cover_width: cover.width,
            cover_height: cover.height,
            artwork_jpeg: cover.jpeg,
            artwork_png: cover.png,
            artwork_webp: cover.webp,
            artwork_other: cover.other,
            root_files: cover.root_files,
            cover_files: cover.cover_files,
            cover_names_json: serde_json::to_string(&cover.cover_names)
                .unwrap_or_else(|_| "[]".to_string()),
            local_art_json: serde_json::to_string(&cover.local_art)
                .unwrap_or_else(|_| "[]".to_string()),
            other_filenames_json: serde_json::to_string(&cover.other_filenames)
                .unwrap_or_else(|_| "[]".to_string()),
            webp_found: cover.webp > 0,
            webp_size_mb: cover.webp_size_mb,
            webp_resolution: cover.webp_resolution,
            webp_conversion: cover.webp_conversion,
            processed_at,
            bypassed,
            timeout_until,
            selected_source: previous
                .as_ref()
                .and_then(|value| value.selected_source.clone()),
            created_at: previous
                .map(|value| value.created_at.clone())
                .unwrap_or_else(|| now.clone()),
        });
    }

    promote_rows(
        connection, config, mapper, &rows, &existing, reason, tag_reads, tag_reuses, &now,
    )
}

#[allow(clippy::too_many_arguments)]
fn promote_rows(
    connection: &mut Connection,
    config: &Config,
    mapper: &PathMapper,
    rows: &[AlbumRow],
    existing: &HashMap<String, ExistingAlbum>,
    reason: &str,
    tag_reads: i64,
    tag_reuses: i64,
    now: &str,
) -> Result<(), String> {
    let transaction = connection
        .transaction()
        .map_err(db_error("start media-index transaction"))?;
    for previous in existing.values() {
        let replacement = rows
            .iter()
            .find(|row| row.album_key.eq_ignore_ascii_case(&previous.album_key));
        if replacement.is_none_or(|row| !previous.path.eq_ignore_ascii_case(&row.path)) {
            let reason = if replacement.is_some() {
                "moved"
            } else {
                "removed"
            };
            transaction.execute(
                "INSERT INTO retired_album_paths(album_key, album_path, reason, retired_at, splined_version) VALUES(?, ?, ?, ?, ?) \
                 ON CONFLICT(album_key) DO UPDATE SET album_path=excluded.album_path, reason=excluded.reason, retired_at=excluded.retired_at, splined_version=excluded.splined_version",
                params![previous.album_key, previous.path, reason, now, env!("CARGO_PKG_VERSION")],
            ).map_err(db_error("record retired Album path"))?;
        }
    }
    transaction
        .execute("DELETE FROM albums", [])
        .map_err(db_error("replace Album index"))?;
    transaction
        .execute("DELETE FROM artists", [])
        .map_err(db_error("replace Artist index"))?;
    transaction
        .execute("DELETE FROM album_refresh_review_queue", [])
        .map_err(db_error("replace Album review queue"))?;
    let mut artists = BTreeMap::<String, (&AlbumRow, Vec<&AlbumRow>)>::new();
    for row in rows {
        artists
            .entry(row.artist_key.clone())
            .or_insert_with(|| (row, Vec::new()))
            .1
            .push(row);
    }
    for (key, (first, albums)) in &artists {
        let aggregate = aggregate_status(albums.iter().map(|row| row.status.as_str()));
        let counts = status_counts(albums.iter().map(|row| row.status.as_str()));
        transaction.execute(
            "INSERT INTO artists(artist_key, artist_name, artist_sort, musicbrainz_artistid, primary_path, status, album_count, unprocessed_count, processed_count, bypassed_count, timeout_count, created_at, updated_at, last_seen_at, splined_version) \
             VALUES(?, ?, ?, NULLIF(?, ''), ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![key, first.artist_name, first.artist_sort, first.artist_mbid, first.artist_primary_path, aggregate, albums.len() as i64, counts.0, counts.1, counts.2, counts.3, now, now, now, env!("CARGO_PKG_VERSION")],
        ).map_err(db_error("insert Artist index row"))?;
    }
    for row in rows {
        insert_album(&transaction, row, now)?;
        if let Some((base_key, _)) = row.album_key.split_once(":copy:") {
            let existing_path = rows
                .iter()
                .find(|candidate| candidate.album_key.eq_ignore_ascii_case(base_key))
                .map(|candidate| candidate.path.as_str())
                .unwrap_or("");
            transaction.execute(
                "INSERT INTO album_refresh_review_queue(album_key, requested_at, source, details_json, splined_version) VALUES(?, ?, 'duplicate-tag-identity', ?, ?)",
                params![row.album_key, now, json!({"base_key": base_key, "existing_path": existing_path, "duplicate_path": row.path}).to_string(), env!("CARGO_PKG_VERSION")],
            ).map_err(db_error("insert Album review row"))?;
        }
    }
    let signature = expected_signature(config, mapper.canonical_root());
    let review_count = rows
        .iter()
        .filter(|row| row.album_key.contains(":copy:"))
        .count();
    let folders =
        json!({"artists": artists.len(), "albums": rows.len(), "review_queue": review_count});
    transaction.execute(
        "INSERT INTO picker_inventory(inventory_key, signature_json, folders_json, generated_at, splined_version) VALUES(?, ?, ?, ?, ?) \
         ON CONFLICT(inventory_key) DO UPDATE SET signature_json=excluded.signature_json, folders_json=excluded.folders_json, generated_at=excluded.generated_at, splined_version=excluded.splined_version",
        params![INVENTORY_KEY, signature.to_string(), folders.to_string(), now, env!("CARGO_PKG_VERSION")],
    ).map_err(db_error("publish media-index inventory"))?;
    transaction.execute(
        "INSERT INTO cache_history(cache_key, cache_type, album_key, action, payload_json, splined_version, event_at) VALUES(?, 'picker_inventory', NULL, ?, ?, ?, ?)",
        params![INVENTORY_KEY, reason, json!({"artists": artists.len(), "albums": rows.len(), "tag_reads": tag_reads, "tag_reuses": tag_reuses}).to_string(), env!("CARGO_PKG_VERSION"), now],
    ).map_err(db_error("write media-index audit"))?;
    transaction.execute(
        "INSERT INTO db_maintenance_state(action_name, completed_at, details_json, splined_version) VALUES('media-index-refresh', ?, ?, ?) \
         ON CONFLICT(action_name) DO UPDATE SET completed_at=excluded.completed_at, details_json=excluded.details_json, splined_version=excluded.splined_version",
        params![now, json!({"reason": reason, "albums": rows.len(), "tag_reads": tag_reads, "tag_reuses": tag_reuses}).to_string(), env!("CARGO_PKG_VERSION")],
    ).map_err(db_error("write media-index maintenance"))?;
    transaction
        .commit()
        .map_err(db_error("commit media-index transaction"))?;
    connection
        .execute_batch("ANALYZE; PRAGMA optimize;")
        .map_err(db_error("optimize media index"))?;
    Ok(())
}

fn insert_album(transaction: &Transaction<'_>, row: &AlbumRow, now: &str) -> Result<(), String> {
    transaction.execute(
        "INSERT INTO albums(album_key, artist_key, album_name, album_sort, musicbrainz_albumid, musicbrainz_releasegroupid, release_year, compilation, path, representative_file, representative_size, representative_mtime_ns, tag_signature, track_count, inventory_fingerprint, status, cover_required, cover_found, cover_path, cover_name, cover_format, cover_width, cover_height, artwork_jpeg, artwork_png, artwork_webp, artwork_other, root_files, cover_files, cover_names_json, local_art_json, other_filenames_json, webp_found, webp_size_mb, webp_resolution, webp_conversion, processed_at, bypassed, timeout_until, selected_source, created_at, updated_at, last_seen_at, splined_version) \
         VALUES(:album_key, :artist_key, :album_name, :album_sort, NULLIF(:album_mbid, ''), NULLIF(:release_group_mbid, ''), NULLIF(:release_year, ''), :compilation, :path, :representative_file, :representative_size, :representative_mtime_ns, :tag_signature, :track_count, :inventory_fingerprint, :status, 1, :cover_found, NULLIF(:cover_path, ''), NULLIF(:cover_name, ''), NULLIF(:cover_format, ''), :cover_width, :cover_height, :artwork_jpeg, :artwork_png, :artwork_webp, :artwork_other, :root_files, :cover_files, :cover_names_json, :local_art_json, :other_filenames_json, :webp_found, :webp_size_mb, NULLIF(:webp_resolution, ''), :webp_conversion, :processed_at, :bypassed, :timeout_until, :selected_source, :created_at, :updated_at, :last_seen_at, :version)",
        named_params! {
            ":album_key": row.album_key, ":artist_key": row.artist_key, ":album_name": row.album_name,
            ":album_sort": row.album_sort, ":album_mbid": row.album_mbid, ":release_group_mbid": row.release_group_mbid,
            ":release_year": row.release_year, ":compilation": i64::from(row.compilation), ":path": row.path,
            ":representative_file": row.representative_file, ":representative_size": row.representative_size,
            ":representative_mtime_ns": row.representative_mtime_ns, ":tag_signature": row.tag_signature,
            ":track_count": row.track_count, ":inventory_fingerprint": row.inventory_fingerprint, ":status": row.status,
            ":cover_found": i64::from(row.cover_found), ":cover_path": row.cover_path, ":cover_name": row.cover_name,
            ":cover_format": row.cover_format, ":cover_width": row.cover_width, ":cover_height": row.cover_height,
            ":artwork_jpeg": row.artwork_jpeg, ":artwork_png": row.artwork_png, ":artwork_webp": row.artwork_webp,
            ":artwork_other": row.artwork_other, ":root_files": row.root_files, ":cover_files": row.cover_files,
            ":cover_names_json": row.cover_names_json, ":local_art_json": row.local_art_json,
            ":other_filenames_json": row.other_filenames_json, ":webp_found": i64::from(row.webp_found),
            ":webp_size_mb": row.webp_size_mb, ":webp_resolution": row.webp_resolution,
            ":webp_conversion": i64::from(row.webp_conversion), ":processed_at": row.processed_at,
            ":bypassed": i64::from(row.bypassed), ":timeout_until": row.timeout_until,
            ":selected_source": row.selected_source, ":created_at": row.created_at, ":updated_at": now,
            ":last_seen_at": now, ":version": env!("CARGO_PKG_VERSION"),
        },
    ).map_err(db_error("insert Album index row"))?;
    Ok(())
}

fn load_snapshot(
    connection: &Connection,
    db_path: &Path,
    mapper: &PathMapper,
) -> Result<MediaSnapshot, String> {
    let mut statement = connection.prepare(
        "SELECT albums.album_key, artists.artist_name, albums.album_name, albums.path, albums.representative_file, albums.status, albums.compilation, albums.track_count, albums.cover_found, albums.local_art_json, albums.processed_at, albums.timeout_until, albums.selected_source \
         FROM albums JOIN artists ON artists.artist_key=albums.artist_key ORDER BY albums.path COLLATE NOCASE",
    ).map_err(db_error("prepare Select Media snapshot"))?;
    let albums = statement
        .query_map([], |row| {
            let canonical_path: String = row.get(3)?;
            let local_path = mapper
                .to_local(&canonical_path)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))?;
            let representative: String = row.get(4)?;
            let physical_artist = physical_artist_name(mapper.local_root(), Path::new(&local_path));
            let local_art_json: String = row.get(9)?;
            Ok(MediaAlbumSnapshot {
                album_key: row.get(0)?,
                artist: physical_artist,
                tagged_artist: row.get(1)?,
                title: row.get(2)?,
                path: local_path,
                representative_file: mapper.to_local(&representative).unwrap_or_default(),
                status: row.get(5)?,
                compilation: row.get::<_, i64>(6)? != 0,
                track_count: row.get(7)?,
                has_local_artwork: row.get::<_, i64>(8)? != 0,
                local_artwork_files: mapper.json_paths_to_local(&local_art_json),
                processed_at: row.get(10)?,
                timeout_until: row.get(11)?,
                selected_source: row.get(12)?,
            })
        })
        .map_err(db_error("read Select Media snapshot"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error("decode Select Media snapshot"))?;
    Ok(MediaSnapshot {
        schema_version: SCHEMA_VERSION,
        database_path: db_path.to_string_lossy().into_owned(),
        canonical_root: mapper.canonical_root().to_string(),
        local_root: mapper.local_root().to_string(),
        albums,
    })
}

fn load_existing(connection: &Connection) -> Result<HashMap<String, ExistingAlbum>, String> {
    let mut statement = connection.prepare(
        "SELECT albums.album_key, artists.artist_name, COALESCE(artists.artist_sort,''), albums.album_name, COALESCE(albums.album_sort,''), COALESCE(albums.musicbrainz_albumid,''), COALESCE(albums.musicbrainz_releasegroupid,''), COALESCE(artists.musicbrainz_artistid,''), COALESCE(albums.release_year,''), albums.compilation, albums.path, COALESCE(albums.representative_file,''), COALESCE(albums.representative_size,0), COALESCE(albums.representative_mtime_ns,0), albums.tag_signature, albums.status, albums.processed_at, albums.bypassed, albums.timeout_until, albums.selected_source, albums.created_at \
         FROM albums JOIN artists ON artists.artist_key=albums.artist_key",
    ).map_err(db_error("prepare existing media index"))?;
    let values = statement
        .query_map([], |row| {
            Ok(ExistingAlbum {
                album_key: row.get(0)?,
                artist_name: row.get(1)?,
                artist_sort: row.get(2)?,
                album_name: row.get(3)?,
                album_sort: row.get(4)?,
                album_mbid: row.get(5)?,
                release_group_mbid: row.get(6)?,
                artist_mbid: row.get(7)?,
                release_year: row.get(8)?,
                compilation: row.get::<_, i64>(9)? != 0,
                path: row.get(10)?,
                representative_file: row.get(11)?,
                representative_size: row.get(12)?,
                representative_mtime_ns: row.get(13)?,
                tag_signature: row.get(14)?,
                status: row.get(15)?,
                processed_at: row.get(16)?,
                bypassed: row.get::<_, i64>(17)? != 0,
                timeout_until: row.get(18)?,
                selected_source: row.get(19)?,
                created_at: row.get(20)?,
            })
        })
        .map_err(db_error("read existing media index"))?;
    let mut output = HashMap::new();
    for value in values {
        let value = value.map_err(db_error("decode existing media index"))?;
        output.insert(value.path.to_lowercase(), value);
    }
    Ok(output)
}

fn inspect_cover(
    album: &AlbumDirectory,
    cover_name: &str,
    mapper: &PathMapper,
) -> Result<CoverFacts, String> {
    let mut facts = CoverFacts::default();
    let prefix = casefold(if cover_name.trim().is_empty() {
        "cover"
    } else {
        cover_name.trim()
    });
    let audio = album.audio_files.iter().collect::<HashSet<_>>();
    let mut files = fs::read_dir(&album.path)
        .map_err(|error| {
            format!(
                "Unable to inspect Album material {}: {error}",
                album.path.display()
            )
        })?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && !audio.contains(path))
        .collect::<Vec<_>>();
    files.sort_by_key(|path| casefold(&path.to_string_lossy()));
    let mut canonical: Option<PathBuf> = None;
    let mut largest_webp: Option<(PathBuf, u64)> = None;
    for path in files {
        facts.root_files += 1;
        let extension = casefold(
            path.extension()
                .and_then(|value| value.to_str())
                .unwrap_or(""),
        );
        let is_image = matches!(extension.as_str(), "jpg" | "jpeg" | "png" | "webp");
        match extension.as_str() {
            "jpg" | "jpeg" => facts.jpeg += 1,
            "png" => facts.png += 1,
            "webp" => facts.webp += 1,
            _ => facts.other += 1,
        }
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_string();
        let stem = casefold(
            path.file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or(""),
        );
        if is_image {
            facts
                .local_art
                .push(mapper.to_canonical(&path.to_string_lossy())?);
        }
        if is_image && stem.starts_with(&prefix) {
            facts.cover_files += 1;
            facts.cover_names.push(name);
            if canonical.as_ref().is_none_or(|current| {
                let current_stem = casefold(
                    current
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .unwrap_or(""),
                );
                (stem != prefix, casefold(&path.to_string_lossy()))
                    < (current_stem != prefix, casefold(&current.to_string_lossy()))
            }) {
                canonical = Some(path.clone());
            }
        } else {
            facts.other_filenames.push(name);
        }
        if extension == "webp" {
            let size = fs::metadata(&path).map(|value| value.len()).unwrap_or(0);
            if largest_webp
                .as_ref()
                .is_none_or(|(_, current)| size > *current)
            {
                largest_webp = Some((path, size));
            }
        }
    }
    if let Some(path) = canonical {
        facts.found = true;
        facts.path = mapper.to_canonical(&path.to_string_lossy())?;
        facts.name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_string();
        facts.format = match ImageFormat::from_path(&path) {
            Ok(ImageFormat::Jpeg) => "JPEG".to_string(),
            Ok(ImageFormat::Png) => "PNG".to_string(),
            Ok(ImageFormat::WebP) => "WEBP".to_string(),
            _ => path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_ascii_uppercase(),
        };
        if let Ok((width, height)) = image::image_dimensions(&path) {
            facts.width = Some(width);
            facts.height = Some(height);
        }
    }
    if let Some((path, size)) = largest_webp {
        facts.webp_size_mb = size as f64 / 1_000_000.0;
        if let Ok((width, height)) = image::image_dimensions(path) {
            facts.webp_resolution = format!("{width}x{height}");
            facts.webp_conversion = true;
        }
    }
    Ok(facts)
}

fn refresh_album_artist_aggregate(connection: &Connection, album_path: &str) -> Result<(), String> {
    let key = connection
        .query_row(
            "SELECT artist_key FROM albums WHERE path=? COLLATE NOCASE",
            [album_path],
            |row| row.get::<_, String>(0),
        )
        .map_err(db_error("read Album Artist aggregate key"))?;
    let mut statuses = connection
        .prepare("SELECT status FROM albums WHERE artist_key=?")
        .map_err(db_error("prepare Album statuses"))?;
    let values = statuses
        .query_map([&key], |row| row.get::<_, String>(0))
        .map_err(db_error("read Album statuses"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error("decode Album statuses"))?;
    let aggregate = aggregate_status(values.iter().map(String::as_str));
    let counts = status_counts(values.iter().map(String::as_str));
    connection.execute(
        "UPDATE artists SET status=?, album_count=?, unprocessed_count=?, processed_count=?, bypassed_count=?, timeout_count=?, updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE artist_key=?",
        params![aggregate, values.len() as i64, counts.0, counts.1, counts.2, counts.3, key],
    ).map_err(db_error("update Artist aggregate"))?;
    Ok(())
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}

fn aggregate_status<'a>(statuses: impl Iterator<Item = &'a str>) -> &'static str {
    let values = statuses.collect::<Vec<_>>();
    if values.contains(&"bypassed") {
        "contains-bypass"
    } else if values.contains(&"incomplete") {
        "partial"
    } else if !values.is_empty()
        && values
            .iter()
            .all(|value| *value == "processed" || *value == "timeout")
    {
        "complete"
    } else if values
        .iter()
        .any(|value| matches!(*value, "processed" | "timeout"))
    {
        "partial"
    } else {
        "unprocessed"
    }
}

fn status_counts<'a>(statuses: impl Iterator<Item = &'a str>) -> (i64, i64, i64, i64) {
    let mut values = (0, 0, 0, 0);
    for status in statuses {
        match status {
            "processed" => values.1 += 1,
            "bypassed" => values.2 += 1,
            "timeout" => values.3 += 1,
            _ => values.0 += 1,
        }
    }
    values
}

fn artist_key(name: &str, mbid: &str) -> String {
    if mbid.trim().is_empty() {
        let normalized = normalize(name);
        format!(
            "tag:{}",
            if normalized.is_empty() {
                "unknown artist"
            } else {
                &normalized
            }
        )
    } else {
        format!("mbid:{}", casefold(mbid.trim()))
    }
}

fn album_key(artist_key: &str, name: &str, tags: &AlbumIndexTags) -> String {
    if !tags.musicbrainz_album_id.trim().is_empty() {
        return format!("mbid:{}", casefold(tags.musicbrainz_album_id.trim()));
    }
    let normalized_name = normalize(name);
    let body = [
        artist_key,
        if normalized_name.is_empty() {
            "unknown album"
        } else {
            &normalized_name
        },
        &casefold(tags.musicbrainz_release_group_id.trim()),
        tags.year.trim(),
        if tags.compilation { "1" } else { "0" },
    ]
    .join("\x1f");
    format!("tag:{}", hex_hash(body.as_bytes()))
}

fn tag_hash(tags: &AlbumIndexTags) -> String {
    let value = json!({
        "album": tags.album, "album_artist": tags.album_artist, "artist_sort": tags.artist_sort,
        "album_sort": tags.album_sort, "album_mbid": tags.musicbrainz_album_id,
        "release_group_mbid": tags.musicbrainz_release_group_id, "artist_mbid": tags.musicbrainz_album_artist_id,
        "year": tags.year, "compilation": tags.compilation,
    });
    hex_hash(value.to_string().as_bytes())
}

fn normalize(value: &str) -> String {
    value
        .nfkc()
        .case_fold()
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn casefold(value: &str) -> String {
    value.case_fold().collect()
}

fn inventory_fingerprint(album: &AlbumDirectory) -> String {
    let mut digest = Sha256::new();
    for path in &album.audio_files {
        let metadata = fs::metadata(path).ok();
        digest.update(
            path.file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .as_bytes(),
        );
        digest.update([0]);
        digest.update(
            metadata
                .as_ref()
                .map(|value| value.len())
                .unwrap_or(0)
                .to_string()
                .as_bytes(),
        );
        digest.update([0]);
        digest.update(
            metadata
                .as_ref()
                .map(modified_ns)
                .unwrap_or(0)
                .to_string()
                .as_bytes(),
        );
        digest.update([0]);
    }
    hex_bytes(&digest.finalize())
}

fn modified_ns(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .and_then(|value| i64::try_from(value.as_nanos()).ok())
        .unwrap_or(0)
}

fn representative_unchanged(
    prior: &ExistingAlbum,
    representative_file: &str,
    representative_size: i64,
    representative_mtime_ns: i64,
) -> bool {
    prior
        .representative_file
        .eq_ignore_ascii_case(representative_file)
        && prior.representative_size == representative_size
        && prior.representative_mtime_ns == representative_mtime_ns
}

fn sqlite_now(connection: &Connection) -> Result<String, String> {
    connection
        .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ','now')", [], |row| {
            row.get(0)
        })
        .map_err(db_error("read SQLite clock"))
}

fn physical_artist_name(local_root: &str, album: &Path) -> String {
    album
        .strip_prefix(local_root)
        .ok()
        .and_then(|relative| relative.components().next())
        .map(|value| value.as_os_str().to_string_lossy().into_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            album
                .parent()
                .and_then(Path::file_name)
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Library".to_string())
        })
}

fn top_artist_path(local_root: &str, album: &Path) -> String {
    let root = Path::new(local_root);
    album
        .strip_prefix(root)
        .ok()
        .and_then(|relative| relative.components().next())
        .map(|component| {
            root.join(component.as_os_str())
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_else(|| {
            album
                .parent()
                .unwrap_or(album)
                .to_string_lossy()
                .into_owned()
        })
}

fn trim_root(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed == "/" {
        return "/".to_string();
    }
    if trimmed.len() == 3 && trimmed.as_bytes()[1] == b':' {
        return trimmed.to_string();
    }
    trimmed.trim_end_matches(['/', '\\']).to_string()
}

fn map_root(value: &str, from: &str, to: &str, insensitive: bool) -> Result<String, String> {
    let value_cmp = value.replace('\\', "/");
    let from_cmp = from.replace('\\', "/");
    let matches = if insensitive {
        value_cmp
            .to_lowercase()
            .starts_with(&from_cmp.to_lowercase())
    } else {
        value_cmp.starts_with(&from_cmp)
    };
    if !matches
        || (value_cmp.len() > from_cmp.len()
            && value_cmp.as_bytes().get(from_cmp.len()) != Some(&b'/'))
    {
        return Err(format!(
            "Path {value:?} is outside configured root {from:?}."
        ));
    }
    let relative = value_cmp[from_cmp.len()..].trim_start_matches('/');
    let separator = if to.contains('\\') || (to.len() >= 2 && to.as_bytes()[1] == b':') {
        '\\'
    } else {
        '/'
    };
    let mut output = trim_root(to);
    if !relative.is_empty() {
        output.push(separator);
        output.push_str(&relative.replace(['/', '\\'], &separator.to_string()));
    }
    Ok(output)
}

fn relative_text<'a>(value: &'a str, root: &str) -> &'a str {
    value
        .get(root.len()..)
        .unwrap_or(value)
        .trim_start_matches(['/', '\\'])
}

fn nonempty(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.trim().to_string()
    }
}

fn hex_hash(value: &[u8]) -> String {
    hex_bytes(&Sha256::digest(value))
}
fn hex_bytes(value: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(value.len() * 2);
    for byte in value {
        let _ = write!(output, "{byte:02x}");
    }
    output
}
fn short_hash(value: &str) -> String {
    hex_hash(value.as_bytes())[..12].to_string()
}

fn db_error(context: &'static str) -> impl FnOnce(rusqlite::Error) -> String {
    move |error| format!("Unable to {context}: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn database_path_is_derived_from_local_and_unc_cache_directories() {
        assert_eq!(
            database_path(r"X:\some\cache"),
            PathBuf::from(r"X:\some\cache").join(DB_NAME)
        );
        assert_eq!(
            database_path(r"\\server\share\splined\_cache"),
            PathBuf::from(r"\\server\share\splined\_cache").join(DB_NAME)
        );
    }

    #[cfg(windows)]
    #[test]
    fn unc_database_path_splits_into_share_root_and_relative_path() {
        let (remote_root, relative_path) = split_unc_path(Path::new(
            r"\\100.105.181.116\volume2\data\downloads\splined\_cache\splined.db",
        ))
        .unwrap();
        assert_eq!(remote_root, r"\\100.105.181.116\volume2");
        assert_eq!(relative_path, r"data\downloads\splined\_cache\splined.db");
    }

    #[cfg(windows)]
    #[test]
    fn unc_path_translates_through_any_matching_drive_connection() {
        assert_eq!(
            translate_unc_to_drive(
                r"\\100.105.181.116\volume2\data\downloads\splined.db",
                "Q:",
                r"\\100.105.181.116\volume2\data",
            ),
            Some(r"Q:\downloads\splined.db".to_string())
        );
        assert_eq!(
            translate_unc_to_drive(r"\\server\share-two\file", "Q:", r"\\server\share"),
            None
        );
    }

    #[test]
    fn shared_signature_accepts_independent_windows_scan_and_output_policy() {
        let mut config = Config::default();
        config.scan.sqlite_shared = true;
        config.library.ignored_subs = vec!["windows-only".to_string()];
        config.output.file_name = "folder".to_string();
        let python_signature = json!({
            "schema_version": SCHEMA_VERSION,
            "library_root": "/music",
            "ignored_subs": ["python-only"],
            "cover_name": "cover",
        });

        validate_signature(&config, Some(&python_signature)).unwrap();

        config.scan.sqlite_shared = false;
        assert!(validate_signature(&config, Some(&python_signature)).is_err());
    }

    #[test]
    fn canonical_posix_paths_round_trip_through_windows_namespace() {
        let mapper = PathMapper::new("/music", r"\\server\share\media\music").unwrap();
        let canonical = "/music/10,000 Maniacs/Love Among the Ruins/1-01 - Rainy Day.mp3";
        let local = mapper.to_local(canonical).unwrap();
        assert_eq!(
            local,
            r"\\server\share\media\music\10,000 Maniacs\Love Among the Ruins\1-01 - Rainy Day.mp3"
        );
        assert_eq!(mapper.to_canonical(&local).unwrap(), canonical);

        let unicode_canonical = "/music/Françoise Hardy/L’amitié — Big Bambú/01 - Voilà.mp3";
        let unicode_local = mapper.to_local(unicode_canonical).unwrap();
        assert_eq!(
            unicode_local,
            r"\\server\share\media\music\Françoise Hardy\L’amitié — Big Bambú\01 - Voilà.mp3"
        );
        assert_eq!(
            mapper.to_canonical(&unicode_local).unwrap(),
            unicode_canonical
        );
    }

    #[test]
    fn python_created_database_loads_without_rewriting_canonical_paths() {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join(DB_NAME);
        let connection = open_database(&db, false).unwrap();
        let now = sqlite_now(&connection).unwrap();
        connection.execute("INSERT INTO artists(artist_key, artist_name, primary_path, created_at, updated_at, last_seen_at, splined_version) VALUES('tag:artist','10,000 Maniacs','/music/10,000 Maniacs',?,?,?,'python')", params![now, now, now]).unwrap();
        connection.execute("INSERT INTO albums(album_key,artist_key,album_name,path,representative_file,tag_signature,created_at,updated_at,last_seen_at,splined_version) VALUES('tag:album','tag:artist','Love Among the Ruins','/music/10,000 Maniacs/Love Among the Ruins','/music/10,000 Maniacs/Love Among the Ruins/1-01 - 10,000 Maniacs - Rainy Day.mp3','tags',?,?,?,'python')", params![now, now, now]).unwrap();
        connection.execute("INSERT INTO picker_inventory(inventory_key,signature_json,folders_json,generated_at,splined_version) VALUES(?,?, '{}',?,'python')", params![INVENTORY_KEY, json!({"schema_version":2,"library_root":"/music","ignored_subs":[],"cover_name":"cover"}).to_string(), now]).unwrap();
        let mapper = PathMapper::new("/music", r"\\server\share\media\music").unwrap();
        let snapshot = load_snapshot(&connection, &db, &mapper).unwrap();
        assert!(
            snapshot.albums[0]
                .path
                .starts_with(r"\\server\share\media\music")
        );
        let stored: String = connection
            .query_row("SELECT path FROM albums", [], |row| row.get(0))
            .unwrap();
        assert_eq!(stored, "/music/10,000 Maniacs/Love Among the Ruins");
    }

    #[test]
    fn independent_database_uses_schema_v2() {
        let temp = tempfile::tempdir().unwrap();
        let connection = open_database(&database_path(temp.path()), false).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn established_database_warm_connection_is_read_only() {
        let temp = tempfile::tempdir().unwrap();
        let db = database_path(temp.path());
        drop(open_database(&db, true).unwrap());

        let (connection, version) = open_database_read_only(&db).unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        let error = connection
            .execute("DELETE FROM albums", [])
            .expect_err("warm connection unexpectedly allowed a database write");
        assert!(error.to_string().to_ascii_lowercase().contains("readonly"));
    }

    #[test]
    fn runtime_auxiliary_state_round_trips_through_sqlite() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.mode = crate::config::Mode::Write;
        config.scan.cache_dir = temp.path().to_string_lossy().into_owned();
        drop(open_database(&database_path(temp.path()), false).unwrap());

        write_runtime_cache_payload(
            &config,
            "source-selection-stats",
            "source_history",
            r#"{"sources":{"itunes":{"selected":2}}}"#,
        )
        .unwrap();

        assert_eq!(
            read_runtime_cache_payload(&config, "source-selection-stats").unwrap(),
            Some(r#"{"sources":{"itunes":{"selected":2}}}"#.to_string())
        );
        assert!(!temp.path().join("_history").exists());
    }

    #[test]
    fn independent_database_builds_once_then_warm_loads() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("music");
        let album = library.join("Artist").join("Album");
        fs::create_dir_all(&album).unwrap();
        File::create(album.join("01.mp3")).unwrap();
        let mut config = Config::default();
        config.library.music_library = library.to_string_lossy().into_owned();
        config.scan.cache_dir = temp.path().join("cache").to_string_lossy().into_owned();

        let initial = media_snapshot(&config, false).unwrap();
        let warm = media_snapshot(&config, false).unwrap();
        assert_eq!(initial.schema_version, SCHEMA_VERSION);
        assert_eq!(initial.albums.len(), 1);
        assert_eq!(warm.albums, initial.albums);
        let connection = Connection::open(database_path(&config.scan.cache_dir)).unwrap();
        let builds: i64 = connection
            .query_row(
                "SELECT count(*) FROM cache_history WHERE cache_type='picker_inventory'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(builds, 1);
    }

    #[test]
    fn incompatible_shared_database_is_not_rebuilt() {
        let temp = tempfile::tempdir().unwrap();
        let db = database_path(temp.path());
        let connection = Connection::open(&db).unwrap();
        connection.pragma_update(None, "user_version", 1).unwrap();
        drop(connection);

        assert!(
            open_database(&db, true)
                .unwrap_err()
                .contains("incompatible")
        );
        let connection = Connection::open(&db).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn unchanged_representative_identity_is_detectable_without_other_tag_reads() {
        let temp = tempfile::tempdir().unwrap();
        let album = temp.path().join("Artist").join("Album");
        fs::create_dir_all(&album).unwrap();
        let track = album.join("01.mp3");
        File::create(&track).unwrap();
        let metadata = fs::metadata(&track).unwrap();
        let prior = ExistingAlbum {
            album_key: "tag:a".into(),
            artist_name: "Artist".into(),
            artist_sort: "Artist".into(),
            album_name: "Album".into(),
            album_sort: "Album".into(),
            album_mbid: String::new(),
            release_group_mbid: String::new(),
            artist_mbid: String::new(),
            release_year: String::new(),
            compilation: false,
            path: album.to_string_lossy().into_owned(),
            representative_file: track.to_string_lossy().into_owned(),
            representative_size: metadata.len() as i64,
            representative_mtime_ns: modified_ns(&metadata),
            tag_signature: "saved".into(),
            status: "unprocessed".into(),
            processed_at: None,
            bypassed: false,
            timeout_until: None,
            selected_source: None,
            created_at: "now".into(),
        };
        assert!(representative_unchanged(
            &prior,
            &track.to_string_lossy(),
            metadata.len() as i64,
            modified_ns(&metadata),
        ));
    }

    #[test]
    fn logical_identity_uses_python_compatible_nfkc_casefolding() {
        assert_eq!(normalize("Straße  Records"), "strasse records");
        assert_eq!(normalize("ＡＲＴＩＳＴ"), "artist");
        assert_eq!(artist_key("", ""), "tag:unknown artist");
    }

    #[test]
    fn runtime_outcome_uses_authoritative_material_and_targeted_artist_update() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("music");
        let album = library.join("Artist").join("Album");
        fs::create_dir_all(&album).unwrap();
        File::create(album.join("01.mp3")).unwrap();
        let mut config = Config::default();
        config.mode = crate::config::Mode::Write;
        config.scan.scan_mode_timeout = crate::config::ScanTimeout(0.0);
        config.library.music_library = library.to_string_lossy().into_owned();
        config.scan.cache_dir = temp.path().join("cache").to_string_lossy().into_owned();
        let db = database_path(&config.scan.cache_dir);
        let connection = open_database(&db, false).unwrap();
        let now = sqlite_now(&connection).unwrap();
        connection.execute("INSERT INTO artists(artist_key, artist_name, primary_path, created_at, updated_at, last_seen_at, splined_version) VALUES('tag:artist','Artist',?,?,?,?,'test')", params![library.join("Artist").to_string_lossy(), now, now, now]).unwrap();
        connection.execute("INSERT INTO albums(album_key,artist_key,album_name,path,tag_signature,created_at,updated_at,last_seen_at,splined_version) VALUES('tag:album','tag:artist','Album',?,'tags',?,?,?,'test')", params![album.to_string_lossy(), now, now, now]).unwrap();
        connection.execute("INSERT INTO artists(artist_key, artist_name, primary_path, status, created_at, updated_at, last_seen_at, splined_version) VALUES('tag:other','Other',?,'complete',?,?,?,'test')", params![library.join("Other").to_string_lossy(), now, now, now]).unwrap();
        connection.execute("INSERT INTO albums(album_key,artist_key,album_name,path,tag_signature,status,created_at,updated_at,last_seen_at,splined_version) VALUES('tag:other-album','tag:other','Other Album',?,'tags','unprocessed',?,?,?,'test')", params![library.join("Other").join("Other Album").to_string_lossy(), now, now, now]).unwrap();
        connection.execute("INSERT INTO picker_inventory(inventory_key,signature_json,folders_json,generated_at,splined_version) VALUES(?,?, '{}',?,'test')", params![INVENTORY_KEY, expected_signature(&config, &config.library.music_library).to_string(), now]).unwrap();
        drop(connection);

        let cover = album.join("cover.jpg");
        assert!(!cover.exists());
        let timing = record_album_outcome_from_runtime(
            &config,
            &album,
            None,
            "installed",
            Some("itunes"),
            Some(&RuntimeArtworkMaterial {
                path: cover,
                format: "JPEG".to_string(),
                width: 1800,
                height: 1800,
            }),
        )
        .unwrap();
        let connection = Connection::open(db).unwrap();
        let values: (String, i64, String, i64, i64) = connection
            .query_row(
                "SELECT status, bypassed, selected_source, cover_found, cover_width FROM albums WHERE album_key='tag:album'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .unwrap();
        assert_eq!(
            values,
            ("processed".to_string(), 0, "itunes".to_string(), 1, 1800)
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT status FROM artists WHERE artist_key='tag:other'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "complete"
        );
        assert!(timing.total_ms >= timing.commit_ms);
    }

    #[test]
    fn runtime_outcome_separates_physical_and_indexed_unicode_paths() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("music");
        let physical = library.join("Cheech & Chong").join("Big BambÃº");
        let indexed = library.join("Cheech & Chong").join("Big Bambú");
        fs::create_dir_all(&physical).unwrap();
        let mut config = Config::default();
        config.mode = crate::config::Mode::Write;
        config.scan.scan_mode_timeout = crate::config::ScanTimeout(0.0);
        config.library.music_library = library.to_string_lossy().into_owned();
        config.scan.cache_dir = temp.path().join("cache").to_string_lossy().into_owned();
        let db = database_path(&config.scan.cache_dir);
        let connection = open_database(&db, false).unwrap();
        let now = sqlite_now(&connection).unwrap();
        connection.execute("INSERT INTO artists(artist_key, artist_name, primary_path, created_at, updated_at, last_seen_at, splined_version) VALUES('tag:cheech','Cheech & Chong',?,?,?,?,'test')", params![library.join("Cheech & Chong").to_string_lossy(), now, now, now]).unwrap();
        connection.execute("INSERT INTO albums(album_key,artist_key,album_name,path,tag_signature,created_at,updated_at,last_seen_at,splined_version) VALUES('tag:bambu','tag:cheech','Big Bambú',?,'tags',?,?,?,'test')", params![indexed.to_string_lossy(), now, now, now]).unwrap();
        connection.execute("INSERT INTO picker_inventory(inventory_key,signature_json,folders_json,generated_at,splined_version) VALUES(?,?, '{}',?,'test')", params![INVENTORY_KEY, expected_signature(&config, &config.library.music_library).to_string(), now]).unwrap();
        drop(connection);

        let physical_cover = physical.join("cover.jpg");
        record_album_outcome_from_runtime(
            &config,
            &physical,
            Some(&indexed),
            "installed",
            Some("amazon"),
            Some(&RuntimeArtworkMaterial {
                path: physical_cover,
                format: "JPEG".to_string(),
                width: 1600,
                height: 1600,
            }),
        )
        .unwrap();

        let connection = Connection::open(db).unwrap();
        let values: (String, String, String) = connection
            .query_row(
                "SELECT status, selected_source, cover_path FROM albums WHERE album_key='tag:bambu'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(values.0, "processed");
        assert_eq!(values.1, "amazon");
        assert_eq!(
            values.2,
            indexed.join("cover.jpg").to_string_lossy().into_owned()
        );
    }
}
