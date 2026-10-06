use std::fs;

fn source(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("unable to read {path}: {error}"))
}

#[test]
fn windows_release_has_one_root_gui_and_one_fixed_runtime_worker() {
    let manifest = source("windows/Cargo.toml");
    let build = source("windows/build.rs");
    let gui = source("windows/gui/MainForm.cs");
    let core = source("windows/src/main.rs");
    let workflow = source(".github/workflows/release-next-patch.yml");

    assert!(manifest.contains("name = \"splined-core\""));
    assert!(build.contains("profile_dir.join(\"splined.exe\")"));
    assert!(gui.contains(
        "Path.Combine(AppDomain.CurrentDomain.BaseDirectory, \"runtime\", \"splined-core.exe\")"
    ));
    assert!(workflow.contains("release-package/splined.exe"));
    assert!(workflow.contains("release-package/runtime/splined-core.exe"));

    for (path, body) in [
        ("windows/build.rs", build.as_str()),
        ("windows/gui/MainForm.cs", gui.as_str()),
        ("windows/src/main.rs", core.as_str()),
        ("release workflow", workflow.as_str()),
    ] {
        for retired in [
            "SPLINED_EMBEDDED_GUI",
            "SPLINED_CORE_PATH",
            "splined-gui-",
            "SPLINED_UPDATE_RELAUNCH",
        ] {
            assert!(!body.contains(retired), "{path} retained {retired}");
        }
    }
}

#[test]
fn portable_runtime_never_manages_executable_files() {
    for path in ["src/portable.rs", "windows/src/portable.rs"] {
        let body = source(path);
        for retired in [
            "cmd.exe",
            "powershell",
            "remove_setup_executable",
            "install_final_executable",
            "cleanup_stale_upgrade_files",
            "finish_setup_executable",
            "running_as_setup_executable",
            "std::process::Command",
        ] {
            assert!(!body.contains(retired), "{path} retained {retired}");
        }
    }
}

#[test]
fn windows_config_and_ui_authority_is_portable_and_outside_updates() {
    let config = source("windows/gui/ConfigState.cs");
    let updater = source("windows/updater/Program.cs");

    assert!(config.contains("Path.Combine(AppRoot, \"data\")"));
    assert!(config.contains("Path.Combine(DataRoot, \"config.toml\")"));
    assert!(config.contains("Path.Combine(DataRoot, \"ui.toml\")"));
    assert!(config.contains("WriteTextAtomic(DefaultConfigPath"));
    assert!(config.contains("WriteTextAtomic(UiPath"));
    assert!(config.contains("PortableMigrationRegistryValue"));
    assert!(config.contains("Registry.CurrentUser.OpenSubKey(RegistryPath, false)"));
    assert!(!config.contains("key.SetValue(ConfigRegistryValue"));
    assert!(!config.contains("key.SetValue(UiRegistryValue"));
    assert!(source("windows/src/portable.rs").contains("root.join(\"data\")"));
    assert!(updater.contains("GuiRelativePath = \"splined.exe\""));
    assert!(updater.contains("CoreRelativePath = \"runtime\\\\splined-core.exe\""));
    assert!(!updater.contains("data\\\\config.toml"));
    assert!(!updater.contains("data\\\\ui.toml"));
}

#[test]
fn automatic_update_is_explicit_verified_and_temporary() {
    let update = source("windows/gui/UpdateService.cs");
    let form = source("windows/gui/MainForm.cs");
    let updater = source("windows/updater/Program.cs");
    let workflow = source(".github/workflows/release-next-patch.yml");

    assert!(update.contains("DownloadAndStageAsync"));
    assert!(update.contains("LaunchUpdater"));
    assert!(update.contains("StableUpdaterName = \"splined-update.exe\""));
    assert!(update.contains("CreateNoWindow = false"));
    assert!(update.contains("ValidateNotificationManifest"));
    assert!(update.contains("ValidateAutomaticManifest"));
    assert!(update.contains("IsAutomaticInstallManifest"));
    assert!(form.contains("transactional replacement and rollback protection"));
    assert!(updater.contains("WaitForProcess(options.CoreProcessId"));
    assert!(updater.contains("WaitForProcess(options.GuiProcessId"));
    assert!(updater.contains("UpdaterFileName = \"splined-update.exe\""));
    assert!(updater.contains("HashSet<string> allowed"));
    assert!(updater.contains("The release archive contains an unexpected file"));
    assert!(updater.contains("_cache\", \"splined.db"));
    assert!(updater.contains("RollBack("));
    assert!(!updater.contains("cmd.exe"));
    assert!(!updater.to_ascii_lowercase().contains("powershell"));
    assert!(workflow.contains("release-assets/splined-update.exe"));
    assert!(!workflow.contains("release-package/splined-update.exe"));
    assert!(!std::path::Path::new(".github/workflows/windows-dev-update.yml").exists());
}
