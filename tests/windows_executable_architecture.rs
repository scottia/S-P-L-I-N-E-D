use std::fs;
use std::path::{Path, PathBuf};

fn source(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("unable to read {path}: {error}"))
}

fn repository_files(root: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            if !matches!(
                path.file_name().and_then(|value| value.to_str()),
                Some("target" | ".git" | "debug")
            ) {
                repository_files(&path, files);
            }
        } else {
            files.push(path);
        }
    }
}

#[test]
fn windows_desktop_is_one_permanent_executable_with_in_process_rust() {
    let manifest = source("windows/Cargo.toml");
    let main = source("windows/src/main.rs");
    let backend = source("windows/src/tauri_app.rs");
    let build = source("windows/build.rs");

    assert!(manifest.contains("name = \"splined\""));
    assert_eq!(manifest.matches("[[bin]]").count(), 1);
    assert!(manifest.contains("tauri ="));
    let normalized_build = build.replace("\r\n", "\n");
    assert_eq!(
        normalized_build.trim(),
        "fn main() {\n    tauri_build::build()\n}"
    );
    assert!(main.contains("tauri_app::run()"));
    assert!(backend.contains("run_scan_library_read_report"));
    assert!(backend.contains("begin_in_process"));
    assert!(!backend.contains("std::process::Command"));
    assert!(!backend.contains("stdin"));
    assert!(!backend.contains("stdout"));
}

#[test]
fn obsolete_windows_executable_and_process_paths_are_absent() {
    assert!(!Path::new("windows/updater").exists());
    let gui_sources = [
        "MainForm.cs",
        "Program.cs",
        "UpdateService.cs",
        "ConfigState.cs",
    ];
    for name in gui_sources {
        assert!(
            !Path::new("windows/gui").join(name).exists(),
            "legacy GUI source remained: {name}"
        );
    }
    let mut files = Vec::new();
    repository_files(Path::new("windows"), &mut files);
    for path in files {
        if path.extension().and_then(|value| value.to_str()) == Some("ico") {
            continue;
        }
        let Ok(body) = fs::read_to_string(&path) else {
            continue;
        };
        let lower = body.to_ascii_lowercase();
        for retired in [
            "splined-core.exe",
            "splined-update.exe",
            "splined-gui-",
            "splined_gui_events",
            "splined_compilation_track_path",
        ] {
            assert!(
                !lower.contains(retired),
                "{} retained {retired}",
                path.display()
            );
        }
        if path.starts_with("windows/src")
            || path.starts_with("windows/frontend")
            || path.starts_with("windows/nsis")
        {
            assert!(
                !lower.contains("cmd.exe"),
                "{} invokes cmd.exe",
                path.display()
            );
            assert!(
                !lower.contains("powershell"),
                "{} invokes PowerShell",
                path.display()
            );
        }
    }
}

#[test]
fn portable_release_and_official_update_contract_exclude_durable_state() {
    let configuration = source("windows/tauri.conf.json");
    let hooks = source("windows/nsis/portable-hooks.nsh");
    let workflow = source(".github/workflows/release-next-patch.yml");
    let state = source("windows/src/windows_state.rs");
    let portable = source("windows/src/portable.rs");

    assert!(configuration.contains("\"createUpdaterArtifacts\": true"));
    assert!(configuration.contains("\"updater\""));
    assert!(hooks.contains("SPLINED_PORTABLE_ROOT"));
    assert!(hooks.contains("Abort"));
    assert!(hooks.contains("SetOutPath $INSTDIR"));
    assert!(hooks.contains("DeleteRegKey SHCTX"));
    assert!(!hooks.contains("$INSTDIR\\data"));
    assert!(!hooks.contains("splined.db"));
    assert!(portable.contains("root.join(\"data\")"));
    assert!(portable.contains("config.toml"));
    assert!(state.contains("ui.toml"));
    assert!(workflow.contains("Portable archive contains forbidden state or executable"));
    assert!(workflow.contains("splined-windows-x86_64-installer.exe.sig"));
    assert!(workflow.contains("Get-AuthenticodeSignature"));
    assert!(workflow.contains("TimeStamperCertificate"));
    assert!(!workflow.contains("Copy-Item windows/data"));
    assert!(!workflow.contains("Copy-Item data"));
}

#[test]
fn updater_signature_validation_failure_has_no_installation_fallback() {
    let backend = source("windows/src/tauri_app.rs");
    let manifest = source("windows/Cargo.toml");
    assert!(backend.contains("SPLINED_UPDATER_PUBLIC_KEY"));
    assert!(backend.contains("download_and_install"));
    assert_eq!(backend.matches(".download_and_install(").count(), 1);
    assert!(!backend.contains(".download("));
    assert!(!backend.contains(".install("));
    assert!(backend.contains("The signed update was not installed"));
    assert!(backend.contains("The active scan did not close normally"));
    assert!(backend.contains("This unsigned development build cannot install public updates"));
    assert!(manifest.contains("tauri-plugin-updater = \"=2.13.2\""));
    assert!(!backend.contains("sha256"));
    assert!(!backend.contains("rename("));
    assert!(!backend.contains("remove_file"));
}
