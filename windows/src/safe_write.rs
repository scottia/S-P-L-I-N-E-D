use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::{Builder, NamedTempFile};

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
    println!();
    println!("Credential file created; user-specific filesystem ACL protection");
    println!("is unavailable on this storage location.");
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

/// Copy an existing file to a same-directory staging file, mutate and
/// validate the staged copy, then atomically replace the original. This keeps
/// audio-tag updates recoverable without reading an entire media file into
/// memory.
pub fn transform_existing_file<M, V>(
    path: &Path,
    label: &str,
    mutate: M,
    validate: V,
) -> Result<(), String>
where
    M: FnOnce(&Path) -> Result<(), String>,
    V: FnOnce(&Path) -> Result<(), String>,
{
    if !path.is_file() {
        return Err(format!(
            "Unable to update {label}; file does not exist: {}",
            path.display()
        ));
    }
    recover_backup_if_needed(path, label)?;
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
    let staged = Builder::new()
        .prefix(".splined-stage-")
        .suffix(&staged_suffix)
        .tempfile_in(parent)
        .map_err(|error| {
            format!(
                "Unable to create staged {label} file in {}: {error}",
                parent.display()
            )
        })?;
    fs::copy(path, staged.path()).map_err(|error| {
        format!(
            "Unable to copy staged {label} file {}: {error}",
            path.display()
        )
    })?;
    mutate(staged.path())?;
    staged
        .as_file()
        .sync_all()
        .map_err(|error| format!("Unable to sync staged {label} file: {error}"))?;
    validate(staged.path())?;
    install_staged_file(path, staged, label)
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
