use std::collections::BTreeSet;
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
    assert!(configuration.contains("\"fileAssociations\""));
    assert!(configuration.contains("\"ext\": [\"spl\"]"));
    assert!(source("windows/src/tauri_app.rs").contains("startup_backup_path"));
    let shell = source("windows/src/windows_shell.rs");
    assert!(shell.contains(r"Software\Classes\.spl"));
    assert!(shell.contains(r"shell\open\command"));
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
    assert!(workflow.contains("SPLINED_UPDATE_ARTIFACT"));
    assert!(workflow.contains("verifies_release_updater_signature"));
    assert!(workflow.contains("Generate updater metadata after signature validation"));
    assert!(workflow.contains("Expected AMD64 PE Machine 0x8664"));
    assert!(!configuration.contains("digestAlgorithm"));
    assert!(!configuration.contains("timestampUrl"));
    assert!(!workflow.contains("Copy-Item windows/data"));
    assert!(!workflow.contains("Copy-Item data"));
}

#[test]
fn windows_release_is_amd64_only_and_requires_only_tauri_updater_secrets() {
    let workflow = source(".github/workflows/release-next-patch.yml");
    assert!(workflow.contains("WINDOWS_TARGET: \"x86_64-pc-windows-msvc\""));
    assert!(workflow.contains("rustup target add \"$WINDOWS_TARGET\""));
    assert!(workflow.contains("cargo tauri build --target $env:WINDOWS_TARGET"));
    assert!(workflow.contains("windows/target/$env:WINDOWS_TARGET/release"));
    assert!(workflow.contains("$machine -ne 0x8664"));
    let windows_targets = workflow
        .split(|character: char| character.is_whitespace() || matches!(character, '"' | '\''))
        .filter(|word| word.contains("-pc-windows-"))
        .collect::<BTreeSet<_>>();
    assert_eq!(windows_targets, BTreeSet::from(["x86_64-pc-windows-msvc"]));
    let required_secrets = BTreeSet::from([
        "TAURI_UPDATER_PUBLIC_KEY",
        "TAURI_SIGNING_PRIVATE_KEY",
        "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
    ]);
    let preflight = workflow
        .split("  release-preflight:")
        .nth(1)
        .expect("release preflight job")
        .split("  prepare-release:")
        .next()
        .expect("release preflight body");
    let configured_secrets = preflight
        .lines()
        .filter_map(|line| line.split("${{ secrets.").nth(1))
        .filter_map(|value| value.split(" }}").next())
        .collect::<BTreeSet<_>>();
    assert_eq!(configured_secrets, required_secrets);
    let signature_validation = workflow
        .find("Validate x64 package and updater signature")
        .expect("signature validation step");
    let metadata = workflow
        .find("Generate updater metadata after signature validation")
        .expect("metadata generation step");
    assert!(signature_validation < metadata);
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
    assert!(backend.contains("This development build has no updater verification key"));
    assert!(manifest.contains("tauri-plugin-updater = \"=2.13.2\""));
    assert!(!backend.contains("sha256"));
    assert!(!backend.contains("rename("));
    assert!(!backend.contains("remove_file"));
}
