use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};
use tempfile::{Builder, NamedTempFile};

static CREDENTIAL_PROTECTION_WARNING: AtomicBool = AtomicBool::new(false);

fn backup_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("splined-file");
    path.with_file_name(format!("{file_name}.splined-backup"))
}

fn is_credential_label(label: &str) -> bool {
    label.to_ascii_lowercase().contains("credential")
}

#[cfg(windows)]
fn apply_user_only_file_protection(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        EXPLICIT_ACCESS_W, NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT, SET_ACCESS, SetEntriesInAclW,
        SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_IS_WELL_KNOWN_GROUP,
        TRUSTEE_W,
    };
    use windows_sys::Win32::Security::{
        CreateWellKnownSid, DACL_SECURITY_INFORMATION, GetTokenInformation, NO_INHERITANCE,
        PROTECTED_DACL_SECURITY_INFORMATION, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser,
        WinLocalSystemSid,
    };
    use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    struct TokenHandle(HANDLE);
    impl Drop for TokenHandle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    struct LocalAcl(*mut windows_sys::Win32::Security::ACL);
    impl Drop for LocalAcl {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { LocalFree(self.0.cast()) };
            }
        }
    }

    unsafe fn protect(path: &Path) -> bool {
        let mut token = null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return false;
        }
        let _token = TokenHandle(token);

        let mut user_length = 0;
        unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &mut user_length) };
        if user_length < std::mem::size_of::<TOKEN_USER>() as u32 {
            return false;
        }
        let mut user_buffer = vec![0u8; user_length as usize];
        if unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                user_buffer.as_mut_ptr().cast(),
                user_length,
                &mut user_length,
            )
        } == 0
        {
            return false;
        }
        let user_sid = unsafe { (*(user_buffer.as_ptr() as *const TOKEN_USER)).User.Sid };

        let mut system_length = 0;
        unsafe {
            CreateWellKnownSid(
                WinLocalSystemSid,
                null_mut(),
                null_mut(),
                &mut system_length,
            )
        };
        if system_length == 0 {
            return false;
        }
        let mut system_buffer = vec![0u8; system_length as usize];
        let system_sid = system_buffer.as_mut_ptr().cast();
        if unsafe {
            CreateWellKnownSid(
                WinLocalSystemSid,
                null_mut(),
                system_sid,
                &mut system_length,
            )
        } == 0
        {
            return false;
        }

        fn trustee(sid: PSID, trustee_type: i32) -> TRUSTEE_W {
            TRUSTEE_W {
                pMultipleTrustee: null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: trustee_type,
                ptstrName: sid.cast(),
            }
        }

        let entries = [
            EXPLICIT_ACCESS_W {
                grfAccessPermissions: FILE_ALL_ACCESS,
                grfAccessMode: SET_ACCESS,
                grfInheritance: NO_INHERITANCE,
                Trustee: trustee(user_sid, TRUSTEE_IS_USER),
            },
            EXPLICIT_ACCESS_W {
                grfAccessPermissions: FILE_ALL_ACCESS,
                grfAccessMode: SET_ACCESS,
                grfInheritance: NO_INHERITANCE,
                Trustee: trustee(system_sid, TRUSTEE_IS_WELL_KNOWN_GROUP),
            },
        ];
        let mut acl = null_mut();
        if unsafe { SetEntriesInAclW(entries.len() as u32, entries.as_ptr(), null(), &mut acl) }
            != ERROR_SUCCESS
            || acl.is_null()
        {
            return false;
        }
        let acl = LocalAcl(acl);
        let mut wide_path = path.as_os_str().encode_wide().collect::<Vec<_>>();
        wide_path.push(0);
        (unsafe {
            SetNamedSecurityInfoW(
                wide_path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                acl.0,
                null(),
            )
        }) == ERROR_SUCCESS
    }

    unsafe { protect(path) }
}

#[cfg(unix)]
fn apply_user_only_file_protection(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).is_ok()
}

#[cfg(not(any(windows, unix)))]
fn apply_user_only_file_protection(_path: &Path) -> bool {
    false
}

fn warn_unavailable_credential_protection() {
    CREDENTIAL_PROTECTION_WARNING.store(true, Ordering::SeqCst);
    eprintln!(
        "Credential file created; user-specific filesystem ACL protection is unavailable on this storage location."
    );
}

/// Return and clear the warning raised when a newly created credential file
/// could not receive restrictive platform filesystem permissions.
pub fn take_credential_protection_warning() -> bool {
    CREDENTIAL_PROTECTION_WARNING.swap(false, Ordering::SeqCst)
}

pub fn recover_backup_if_needed(path: &Path, label: &str) -> Result<(), String> {
    let backup = backup_path(path);
    if path.exists() {
        if backup.exists() {
            fs::remove_file(&backup).map_err(|error| {
                format!(
                    "Unable to remove stale {label} backup {}: {error}",
                    backup.display()
                )
            })?;
        }
        return Ok(());
    }
    if backup.exists() {
        fs::rename(&backup, path).map_err(|error| {
            format!(
                "Unable to recover {label} backup {} -> {}: {error}",
                backup.display(),
                path.display()
            )
        })?;
    }
    Ok(())
}

pub fn replace_text_file<F>(
    path: &Path,
    contents: &str,
    label: &str,
    validate: F,
) -> Result<(), String>
where
    F: Fn(&str) -> Result<(), String>,
{
    validate(contents)?;
    let credential_file = is_credential_label(label);
    let credential_already_existed = path.exists() || backup_path(path).exists();
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Unable to create {label} directory {}: {error}",
                parent.display()
            )
        })?;
    }
    recover_backup_if_needed(path, label)?;
    let parent = path.parent().ok_or_else(|| {
        format!(
            "Unable to determine parent directory for {label}: {}",
            path.display()
        )
    })?;
    let mut staged = NamedTempFile::new_in(parent).map_err(|error| {
        format!(
            "Unable to create staged {label} file in {}: {error}",
            parent.display()
        )
    })?;
    staged
        .write_all(contents.as_bytes())
        .map_err(|error| format!("Unable to write staged {label} file: {error}"))?;
    staged
        .flush()
        .map_err(|error| format!("Unable to flush staged {label} file: {error}"))?;
    staged
        .as_file()
        .sync_all()
        .map_err(|error| format!("Unable to sync staged {label} file: {error}"))?;
    let staged_text = fs::read_to_string(staged.path())
        .map_err(|error| format!("Unable to verify staged {label} file: {error}"))?;
    validate(&staged_text)?;
    let credential_protected = !credential_file || apply_user_only_file_protection(staged.path());
    install_staged_file(path, staged, label)?;
    if credential_file && !credential_already_existed && !credential_protected {
        warn_unavailable_credential_protection();
    }
    Ok(())
}

pub fn replace_binary_file<F>(
    path: &Path,
    contents: &[u8],
    label: &str,
    validate: F,
) -> Result<(), String>
where
    F: Fn(&Path) -> Result<(), String>,
{
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Unable to create {label} directory {}: {error}",
                parent.display()
            )
        })?;
    }
    recover_backup_if_needed(path, label)?;
    let parent = path.parent().ok_or_else(|| {
        format!(
            "Unable to determine parent directory for {label}: {}",
            path.display()
        )
    })?;
    let mut staged = NamedTempFile::new_in(parent).map_err(|error| {
        format!(
            "Unable to create staged {label} file in {}: {error}",
            parent.display()
        )
    })?;
    staged
        .write_all(contents)
        .map_err(|error| format!("Unable to write staged {label} file: {error}"))?;
    staged
        .flush()
        .map_err(|error| format!("Unable to flush staged {label} file: {error}"))?;
    staged
        .as_file()
        .sync_all()
        .map_err(|error| format!("Unable to sync staged {label} file: {error}"))?;
    validate(staged.path())?;
    install_staged_file(path, staged, label)
}

/// Copy an existing file through a same-directory staging file, validate it,
/// then atomically replace the destination. This avoids loading large SQLite
/// backups into memory during restore.
pub fn replace_file_from<F>(
    path: &Path,
    source: &Path,
    label: &str,
    validate: F,
) -> Result<(), String>
where
    F: Fn(&Path) -> Result<(), String>,
{
    if !source.is_file() {
        return Err(format!(
            "Unable to restore {label}; source file does not exist: {}",
            source.display()
        ));
    }
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Unable to create {label} directory {}: {error}",
                parent.display()
            )
        })?;
    }
    recover_backup_if_needed(path, label)?;
    let parent = path.parent().ok_or_else(|| {
        format!(
            "Unable to determine parent directory for {label}: {}",
            path.display()
        )
    })?;
    let mut staged = NamedTempFile::new_in(parent).map_err(|error| {
        format!(
            "Unable to create staged {label} file in {}: {error}",
            parent.display()
        )
    })?;
    let mut input = fs::File::open(source).map_err(|error| {
        format!(
            "Unable to open {label} source {}: {error}",
            source.display()
        )
    })?;
    std::io::copy(&mut input, &mut staged)
        .map_err(|error| format!("Unable to copy staged {label} file: {error}"))?;
    staged
        .flush()
        .map_err(|error| format!("Unable to flush staged {label} file: {error}"))?;
    staged
        .as_file()
        .sync_all()
        .map_err(|error| format!("Unable to sync staged {label} file: {error}"))?;
    validate(staged.path())?;
    install_staged_file(path, staged, label)
}

/// Prepare remote audio files locally, then validate a same-directory staged
/// copy before replacing the original. Local audio files use the same-directory
/// stage directly. Neither route reads the entire file into memory.
pub fn transform_existing_file<M, V>(
    path: &Path,
    label: &str,
    mutate: M,
    validate: V,
) -> Result<(), String>
where
    M: FnOnce(&Path) -> Result<(), String>,
    V: Fn(&Path) -> Result<(), String>,
{
    transform_existing_file_with_trace(path, label, mutate, validate, &mut |_, _| {})
}

pub fn transform_existing_file_with_trace<M, V>(
    path: &Path,
    label: &str,
    mutate: M,
    validate: V,
    trace: &mut dyn FnMut(&'static str, Duration),
) -> Result<(), String>
where
    M: FnOnce(&Path) -> Result<(), String>,
    V: Fn(&Path) -> Result<(), String>,
{
    transform_existing_file_with_staging(path, label, mutate, validate, is_remote_path(path), trace)
}

fn transform_existing_file_with_staging<M, V>(
    path: &Path,
    label: &str,
    mutate: M,
    validate: V,
    stage_locally: bool,
    trace: &mut dyn FnMut(&'static str, Duration),
) -> Result<(), String>
where
    M: FnOnce(&Path) -> Result<(), String>,
    V: Fn(&Path) -> Result<(), String>,
{
    trace(
        if stage_locally {
            "route_local_stage"
        } else {
            "route_album_stage"
        },
        Duration::ZERO,
    );
    if !path.is_file() {
        return Err(format!(
            "Unable to update {label}; file does not exist: {}",
            path.display()
        ));
    }
    let started = Instant::now();
    let recovered = recover_backup_if_needed(path, label);
    trace("recover_backup", started.elapsed());
    recovered?;
    let parent = path.parent().ok_or_else(|| {
        format!(
            "Unable to determine parent directory for {label}: {}",
            path.display()
        )
    })?;
    let staged_suffix = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!(".{extension}"))
        .unwrap_or_default();
    if stage_locally {
        // Tag libraries perform several seeks and rewrites. On an SMB share,
        // do that work in the user's local temp directory, then make one
        // validated same-directory copy for recoverable replacement.
        let started = Instant::now();
        let local_stage = Builder::new()
            .prefix(".splined-local-stage-")
            .suffix(&staged_suffix)
            .tempfile_in(std::env::temp_dir())
            .map_err(|error| format!("Unable to create local {label} stage: {error}"));
        trace("create_local_stage", started.elapsed());
        let local_stage = local_stage?;
        let started = Instant::now();
        let copied = fs::copy(path, local_stage.path()).map_err(|error| {
            format!(
                "Unable to copy {label} file {} to local stage: {error}",
                path.display()
            )
        });
        trace("copy_to_local", started.elapsed());
        copied?;
        let started = Instant::now();
        let mutated = mutate(local_stage.path());
        trace("mutate_local", started.elapsed());
        mutated?;
        let started = Instant::now();
        let validated = validate(local_stage.path());
        trace("validate_local", started.elapsed());
        validated?;
        let started = Instant::now();
        let staged = Builder::new()
            .prefix(".splined-stage-")
            .suffix(&staged_suffix)
            .tempfile_in(parent)
            .map_err(|error| format!("Unable to create staged {label} file: {error}"));
        trace("create_share_stage", started.elapsed());
        let mut staged = staged?;
        let mut input = fs::File::open(local_stage.path())
            .map_err(|error| format!("Unable to open local {label} stage: {error}"))?;
        let started = Instant::now();
        let copied = std::io::copy(&mut input, &mut staged)
            .map_err(|error| format!("Unable to copy prepared {label} to the library: {error}"));
        trace("copy_to_share", started.elapsed());
        copied?;
        let started = Instant::now();
        let flushed = staged
            .flush()
            .map_err(|error| format!("Unable to flush staged {label} file: {error}"));
        trace("flush_share", started.elapsed());
        flushed?;
        let started = Instant::now();
        let synced = staged
            .as_file()
            .sync_all()
            .map_err(|error| format!("Unable to sync staged {label} file: {error}"));
        trace("sync_share", started.elapsed());
        synced?;
        let started = Instant::now();
        let validated = validate(staged.path());
        trace("validate_share", started.elapsed());
        validated?;
        let started = Instant::now();
        let installed = install_staged_file(path, staged, label);
        trace("replace_share", started.elapsed());
        return installed;
    }
    let started = Instant::now();
    let staged = Builder::new()
        .prefix(".splined-stage-")
        .suffix(&staged_suffix)
        .tempfile_in(parent)
        .map_err(|error| {
            format!(
                "Unable to create staged {label} file in {}: {error}",
                parent.display()
            )
        });
    trace("create_album_stage", started.elapsed());
    let staged = staged?;
    let started = Instant::now();
    let copied = fs::copy(path, staged.path()).map_err(|error| {
        format!(
            "Unable to copy staged {label} file {}: {error}",
            path.display()
        )
    });
    trace("copy_to_album_stage", started.elapsed());
    copied?;
    let started = Instant::now();
    let mutated = mutate(staged.path());
    trace("mutate_album_stage", started.elapsed());
    mutated?;
    let started = Instant::now();
    let synced = staged
        .as_file()
        .sync_all()
        .map_err(|error| format!("Unable to sync staged {label} file: {error}"));
    trace("sync_album_stage", started.elapsed());
    synced?;
    let started = Instant::now();
    let validated = validate(staged.path());
    trace("validate_album_stage", started.elapsed());
    validated?;
    let started = Instant::now();
    let installed = install_staged_file(path, staged, label);
    trace("replace_album_stage", started.elapsed());
    installed
}

#[cfg(windows)]
fn is_remote_path(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Component, Prefix};
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;

    const DRIVE_REMOTE: u32 = 4;

    match path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::UNC(..) | Prefix::VerbatimUNC(..) => true,
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                let root = [letter as u16, b':' as u16, b'\\' as u16, 0];
                unsafe { GetDriveTypeW(root.as_ptr()) == DRIVE_REMOTE }
            }
            _ => false,
        },
        _ => path
            .as_os_str()
            .encode_wide()
            .next()
            .is_some_and(|first| first == b'\\' as u16),
    }
}

#[cfg(not(windows))]
fn is_remote_path(_path: &Path) -> bool {
    false
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct StageCleanupReport {
    pub removed: usize,
    pub skipped_recent: usize,
    pub skipped_busy: usize,
    pub errors: usize,
}

/// Remove only old SPLINED audio stages from known album directories. This is
/// intended to run in the background at startup, never on the UI thread.
/// The age grace and exclusive-open check protect another client's active
/// write on a shared music library.
pub fn cleanup_stale_audio_stages(
    album_directories: &[PathBuf],
    minimum_age: Duration,
) -> StageCleanupReport {
    let mut report = StageCleanupReport::default();
    let now = SystemTime::now();
    for directory in album_directories {
        let Ok(entries) = fs::read_dir(directory) else {
            report.errors += 1;
            continue;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                report.errors += 1;
                continue;
            };
            if !entry
                .file_name()
                .to_str()
                .is_some_and(is_splined_audio_stage_name)
            {
                continue;
            }
            let Ok(file_type) = entry.file_type() else {
                report.errors += 1;
                continue;
            };
            if !file_type.is_file() || file_type.is_symlink() {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                report.errors += 1;
                continue;
            };
            if !metadata
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age >= minimum_age)
            {
                report.skipped_recent += 1;
                continue;
            }
            if !stage_is_exclusively_openable(&entry.path()) {
                report.skipped_busy += 1;
                continue;
            }
            match fs::remove_file(entry.path()) {
                Ok(()) => report.removed += 1,
                Err(_) => report.errors += 1,
            }
        }
    }
    report
}

fn is_splined_audio_stage_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    (lower.starts_with(".splined-stage-") || lower.starts_with(".splined-local-stage-"))
        && [
            ".mp3", ".flac", ".m4a", ".mp4", ".ogg", ".opus", ".wav", ".aiff", ".aif",
        ]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

#[cfg(windows)]
fn stage_is_exclusively_openable(path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path)
        .is_ok()
}

#[cfg(not(windows))]
fn stage_is_exclusively_openable(_path: &Path) -> bool {
    false
}

fn install_staged_file(path: &Path, staged: NamedTempFile, label: &str) -> Result<(), String> {
    let backup = backup_path(path);
    let had_existing = path.exists();
    if had_existing {
        if backup.exists() {
            fs::remove_file(&backup).map_err(|error| {
                format!(
                    "Unable to remove stale {label} backup {}: {error}",
                    backup.display()
                )
            })?;
        }
        fs::rename(path, &backup).map_err(|error| {
            format!(
                "Unable to stage existing {label} file {} for replacement: {error}",
                path.display()
            )
        })?;
    }
    match staged.persist(path) {
        Ok(_) => {
            if had_existing && backup.exists() {
                fs::remove_file(&backup).map_err(|error| {
                    format!(
                        "Replaced {label} file but could not remove backup {}: {error}",
                        backup.display()
                    )
                })?;
            }
            Ok(())
        }
        Err(error) => {
            if had_existing && backup.exists() {
                let _ = fs::rename(&backup, path);
            }
            Err(format!(
                "Unable to install staged {label} file {}: {}",
                path.display(),
                error.error
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn replace_text_file_round_trips_and_replaces() {
        let dir = TempDir::new().expect("temp directory should create");
        let path = dir.path().join("fixture.json");
        replace_text_file(&path, "{\"value\":1}\n", "fixture", |text| {
            serde_json::from_str::<serde_json::Value>(text)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .expect("first write should succeed");
        replace_text_file(&path, "{\"value\":2}\n", "fixture", |text| {
            serde_json::from_str::<serde_json::Value>(text)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .expect("replacement should succeed");
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"value\":2}\n");
        assert!(!backup_path(&path).exists());
    }

    #[test]
    fn failed_validation_preserves_existing_file() {
        let dir = TempDir::new().expect("temp directory should create");
        let path = dir.path().join("fixture.json");
        fs::write(&path, "good").unwrap();
        let result = replace_text_file(&path, "bad", "fixture", |text| {
            if text == "good" {
                Ok(())
            } else {
                Err("invalid".to_string())
            }
        });
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "good");
    }

    #[test]
    fn replace_binary_file_round_trips_and_replaces() {
        let dir = TempDir::new().expect("temp directory should create");
        let path = dir.path().join("fixture.bin");
        replace_binary_file(&path, b"first", "fixture", |staged| {
            if fs::read(staged).map_err(|error| error.to_string())? == b"first" {
                Ok(())
            } else {
                Err("unexpected staged bytes".to_string())
            }
        })
        .expect("first binary write should succeed");
        replace_binary_file(&path, b"second", "fixture", |staged| {
            if fs::read(staged).map_err(|error| error.to_string())? == b"second" {
                Ok(())
            } else {
                Err("unexpected staged bytes".to_string())
            }
        })
        .expect("binary replacement should succeed");
        assert_eq!(fs::read(&path).unwrap(), b"second");
        assert!(!backup_path(&path).exists());
    }

    #[test]
    fn replace_file_from_streams_and_validates_large_artifacts() {
        let dir = TempDir::new().expect("temp directory should create");
        let source = dir.path().join("source.sqlite");
        let destination = dir.path().join("destination.sqlite");
        fs::write(&source, b"SQLite format 3\0snapshot").unwrap();
        fs::write(&destination, b"old").unwrap();

        replace_file_from(&destination, &source, "fixture database", |path| {
            let bytes = fs::read(path).map_err(|error| error.to_string())?;
            if bytes.starts_with(b"SQLite format 3\0") {
                Ok(())
            } else {
                Err("not SQLite".to_string())
            }
        })
        .unwrap();

        assert_eq!(
            fs::read(&destination).unwrap(),
            b"SQLite format 3\0snapshot"
        );
        assert!(!backup_path(&destination).exists());
    }

    #[test]
    fn failed_binary_validation_preserves_existing_file() {
        let dir = TempDir::new().expect("temp directory should create");
        let path = dir.path().join("fixture.bin");
        fs::write(&path, b"good").unwrap();
        let result = replace_binary_file(&path, b"bad", "fixture", |_staged| {
            Err("invalid".to_string())
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"good");
        assert!(!backup_path(&path).exists());
    }

    #[test]
    fn transform_existing_file_preserves_source_extension_for_callbacks() {
        let dir = TempDir::new().expect("temp directory should create");
        let path = dir.path().join("track.mp3");
        fs::write(&path, b"original").unwrap();

        transform_existing_file(
            &path,
            "audio tags",
            |staged| {
                assert_eq!(
                    staged.extension().and_then(|value| value.to_str()),
                    Some("mp3")
                );
                fs::write(staged, b"updated").map_err(|error| error.to_string())
            },
            |staged| {
                assert_eq!(
                    staged.extension().and_then(|value| value.to_str()),
                    Some("mp3")
                );
                if fs::read(staged).map_err(|error| error.to_string())? == b"updated" {
                    Ok(())
                } else {
                    Err("unexpected staged bytes".to_string())
                }
            },
        )
        .expect("existing file transformation should succeed");

        assert_eq!(fs::read(&path).unwrap(), b"updated");
        assert!(!backup_path(&path).exists());
    }

    #[test]
    fn local_preparation_keeps_recoverable_album_replacement() {
        let dir = TempDir::new().expect("temporary directory");
        let track = dir.path().join("song.mp3");
        fs::write(&track, b"original").expect("original track");
        let mut phases = Vec::new();
        transform_existing_file_with_staging(
            &track,
            "audio tag",
            |stage| {
                assert_eq!(
                    stage.extension().and_then(|value| value.to_str()),
                    Some("mp3")
                );
                fs::write(stage, b"updated").map_err(|error| error.to_string())
            },
            |stage| {
                (fs::read(stage).map_err(|error| error.to_string())? == b"updated")
                    .then_some(())
                    .ok_or_else(|| "invalid stage".to_string())
            },
            true,
            &mut |phase, _| phases.push(phase),
        )
        .expect("local preparation should install");
        assert_eq!(
            phases,
            [
                "route_local_stage",
                "recover_backup",
                "create_local_stage",
                "copy_to_local",
                "mutate_local",
                "validate_local",
                "create_share_stage",
                "copy_to_share",
                "flush_share",
                "sync_share",
                "validate_share",
                "replace_share",
            ]
        );
        assert_eq!(fs::read(&track).unwrap(), b"updated");
        assert!(!backup_path(&track).exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn startup_cleanup_removes_only_old_splined_audio_stages() {
        let dir = TempDir::new().expect("temporary directory");
        let album = dir.path().join("Album");
        fs::create_dir(&album).expect("album directory");
        let stage = album.join(".splined-stage-orphan.mp3");
        let song = album.join("01 - Song.mp3");
        let image = album.join(".splined-stage-image.jpg");
        fs::write(&stage, b"stage").expect("stage");
        fs::write(&song, b"song").expect("song");
        fs::write(&image, b"image").expect("image");

        let protected = cleanup_stale_audio_stages(
            std::slice::from_ref(&album),
            Duration::from_secs(24 * 60 * 60),
        );
        assert_eq!(protected.removed, 0);
        assert_eq!(protected.skipped_recent, 1);
        let cleaned = cleanup_stale_audio_stages(std::slice::from_ref(&album), Duration::ZERO);
        #[cfg(windows)]
        assert_eq!(cleaned.removed, 1);
        #[cfg(not(windows))]
        assert_eq!(cleaned.removed, 0);
        assert_eq!(song.exists(), true);
        assert_eq!(image.exists(), true);
    }

    #[cfg(windows)]
    #[test]
    fn unc_audio_path_uses_local_preparation() {
        assert!(is_remote_path(Path::new(r"\\server\share\song.mp3")));
    }

    #[cfg(windows)]
    #[test]
    fn startup_cleanup_skips_a_stage_held_open_by_another_writer() {
        let dir = TempDir::new().expect("temporary directory");
        let stage = dir.path().join(".splined-stage-active.mp3");
        fs::write(&stage, b"active").expect("stage");
        let held = fs::File::open(&stage).expect("hold stage open");
        let report = cleanup_stale_audio_stages(&[dir.path().to_path_buf()], Duration::ZERO);
        assert_eq!(report.removed, 0);
        assert_eq!(report.skipped_busy, 1);
        assert!(stage.exists());
        drop(held);
    }

    #[test]
    fn missing_destination_recovers_backup() {
        let dir = TempDir::new().expect("temp directory should create");
        let path = dir.path().join("fixture.json");
        let backup = backup_path(&path);
        fs::write(&backup, "recover me").unwrap();
        recover_backup_if_needed(&path, "fixture").expect("recovery should succeed");
        assert_eq!(fs::read_to_string(&path).unwrap(), "recover me");
        assert!(!backup.exists());
    }
}
