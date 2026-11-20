//! Portable Windows `.spl` file association.
//!
//! This current, user-scoped shell integration is deliberately separate from
//! configuration discovery. It does not participate in the retired
//! configuration-registry migration.

#[cfg(windows)]
pub fn register_portable_backup_association() -> Result<(), String> {
    // Validation runs may redirect writable state while exercising a staged
    // executable. They must never replace the operator's real shell handler.
    if std::env::var_os("SPLINED_INTERNAL_STATE_ROOT").is_some()
        || crate::portable::is_packaged_process()?
    {
        return Ok(());
    }

    let executable = std::env::current_exe()
        .map_err(|error| format!("Unable to locate splined.exe for .spl association: {error}"))?;
    if !executable.is_file() {
        return Err(format!(
            "Unable to register .spl association because splined.exe is missing: {}",
            executable.display()
        ));
    }

    const FILE_CLASS: &str = "SPLINED.Backup.v1";
    let extension = windows_registry::CURRENT_USER
        .create(r"Software\Classes\.spl")
        .map_err(|error| format!("Unable to open the portable .spl association: {error}"))?;
    extension
        .set_string("", FILE_CLASS)
        .map_err(|error| format!("Unable to register the portable .spl extension: {error}"))?;

    let file_class = windows_registry::CURRENT_USER
        .create(format!(r"Software\Classes\{FILE_CLASS}"))
        .map_err(|error| format!("Unable to open the portable SPLINED file class: {error}"))?;
    file_class
        .set_string("", "SPLINED backup")
        .map_err(|error| format!("Unable to name the portable SPLINED file class: {error}"))?;

    let icon = windows_registry::CURRENT_USER
        .create(format!(r"Software\Classes\{FILE_CLASS}\DefaultIcon"))
        .map_err(|error| format!("Unable to open the portable SPLINED icon key: {error}"))?;
    icon.set_string("", format!("\"{}\",0", executable.display()))
        .map_err(|error| format!("Unable to register the portable SPLINED icon: {error}"))?;

    let command = windows_registry::CURRENT_USER
        .create(format!(r"Software\Classes\{FILE_CLASS}\shell\open\command"))
        .map_err(|error| format!("Unable to open the portable SPLINED command key: {error}"))?;
    command
        .set_string("", association_command(&executable))
        .map_err(|error| format!("Unable to register portable .spl opening: {error}"))?;
    Ok(())
}

#[cfg(not(windows))]
pub fn register_portable_backup_association() -> Result<(), String> {
    Ok(())
}

fn association_command(executable: &std::path::Path) -> String {
    format!("\"{}\" \"%1\"", executable.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn association_command_quotes_executable_and_backup_path() {
        assert_eq!(
            association_command(std::path::Path::new(r"E:\Apps\SPLINED\splined.exe")),
            r#""E:\Apps\SPLINED\splined.exe" "%1""#
        );
    }
}
