use std::path::Path;

const BACKUP_EXTENSION_KEY: &str = r"Software\Classes\.spl";
const BACKUP_FILE_CLASS: &str = "SPLINED.Backup.v1";

fn backup_open_command(executable: &Path) -> String {
    format!(r#""{}" "%1""#, executable.display())
}

#[cfg(windows)]
pub fn register_backup_file_association(executable: &Path) -> Result<(), String> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_WRITE};

    if !executable.is_file() {
        return Ok(());
    }
    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    let (extension, _) = current_user
        .create_subkey_with_flags(BACKUP_EXTENSION_KEY, KEY_WRITE)
        .map_err(|error| format!("Unable to register the .spl backup extension: {error}"))?;
    extension
        .set_value("", &BACKUP_FILE_CLASS)
        .map_err(|error| format!("Unable to register the .spl backup file class: {error}"))?;

    let class_key = format!(r"Software\Classes\{BACKUP_FILE_CLASS}");
    let (file_class, _) = current_user
        .create_subkey_with_flags(&class_key, KEY_WRITE)
        .map_err(|error| format!("Unable to register the SPLINED backup shell class: {error}"))?;
    file_class
        .set_value("", &"SPLINED backup")
        .map_err(|error| format!("Unable to name the SPLINED backup shell class: {error}"))?;

    let (icon, _) = file_class
        .create_subkey_with_flags("DefaultIcon", KEY_WRITE)
        .map_err(|error| format!("Unable to register the SPLINED backup icon: {error}"))?;
    icon.set_value("", &format!(r#""{}",0"#, executable.display()))
        .map_err(|error| format!("Unable to save the SPLINED backup icon: {error}"))?;

    let (command, _) = file_class
        .create_subkey_with_flags(r"shell\open\command", KEY_WRITE)
        .map_err(|error| format!("Unable to register the SPLINED backup open command: {error}"))?;
    command
        .set_value("", &backup_open_command(executable))
        .map_err(|error| format!("Unable to save the SPLINED backup open command: {error}"))
}

#[cfg(not(windows))]
pub fn register_backup_file_association(_executable: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_open_command_quotes_portable_paths_and_the_selected_backup() {
        assert_eq!(
            backup_open_command(Path::new(r"D:\Portable Apps\SPLINED\splined.exe")),
            r#""D:\Portable Apps\SPLINED\splined.exe" "%1""#
        );
    }
}
