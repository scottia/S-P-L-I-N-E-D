use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

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
    use std::process::Command;
    let whoami = match Command::new("whoami")
        .args(["/user", "/fo", "csv", "/nh"])
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return false,
    };
    let stdout = match String::from_utf8(whoami.stdout) {
        Ok(stdout) => stdout,
        Err(_) => return false,
    };
    let sid = match stdout
        .trim()
        .rsplit_once(',')
        .map(|(_, sid)| sid.trim().trim_matches('"'))
        .filter(|sid| sid.starts_with("S-1-"))
    {
        Some(sid) => sid,
        None => return false,
    };
    let user_grant = format!("*{sid}:F");
    match Command::new("icacls")
        .arg(path)
        .arg("/grant:r")
        .arg(user_grant)
        .arg("*S-1-5-18:F")
        .arg("/inheritance:r")
        .output()
    {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
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
    println!("This storage location does not support SPLINED's normal");
    println!("user-only file protection. Consider storing credentials");
    println!("in a private or encrypted location.");
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
