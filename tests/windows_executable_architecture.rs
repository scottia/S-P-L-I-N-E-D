use std::fs;

fn source(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("unable to read {path}: {error}"))
}

#[test]
fn windows_release_has_fixed_gui_and_core_roles() {
    let manifest = source("windows/Cargo.toml");
    let build = source("windows/build.rs");
    let gui = source("windows/gui/MainForm.cs");
    let core = source("windows/src/main.rs");
    let workflow = source(".github/workflows/release-next-patch.yml");

    assert!(manifest.contains("name = \"splined-core\""));
    assert!(build.contains("profile_dir.join(\"splined.exe\")"));
    assert!(
        gui.contains("Path.Combine(AppDomain.CurrentDomain.BaseDirectory, \"splined-core.exe\")")
    );
    assert!(workflow.contains("release-package/splined.exe"));
    assert!(workflow.contains("release-package/splined-core.exe"));

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
            "setup-splined.exe",
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
fn update_service_is_notification_only() {
    let update = source("windows/gui/UpdateService.cs");
    let form = source("windows/gui/MainForm.cs");
    for retired in [
        "DownloadAndStageAsync",
        "LaunchUpdater",
        "File.WriteAllBytes",
        "File.Move",
        "setup-splined.exe",
    ] {
        assert!(
            !update.contains(retired),
            "UpdateService retained {retired}"
        );
        assert!(!form.contains(retired), "MainForm retained {retired}");
    }
    assert!(update.contains("OpenReleasePage"));
    assert!(form.contains("SPLINED will not download, install, or run an executable"));
    assert!(!std::path::Path::new(".github/workflows/windows-dev-update.yml").exists());
}
