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
                Some("target" | "qa-app" | "qa-test" | "qa-temp" | "debug")
            ) {
                repository_files(&path, files);
            }
        } else {
            files.push(path);
        }
    }
}

#[test]
fn windows_desktop_is_winforms_with_one_in_process_rust_dll() {
    let manifest = source("windows/Cargo.toml");
    let build = source("windows/build.rs");
    let native = source("windows/gui/NativeCore.cs");
    let ffi = source("windows/src/ffi.rs");

    assert!(Path::new("windows/gui/MainForm.cs").is_file());
    assert!(Path::new("windows/gui/SetupForm.cs").is_file());
    assert!(Path::new("windows/gui/ThemeManager.cs").is_file());
    assert!(manifest.contains("autobins = false"));
    assert!(manifest.contains("crate-type = [\"rlib\", \"cdylib\"]"));
    assert!(!manifest.contains("[[bin]]"));
    assert!(!manifest.to_ascii_lowercase().contains("tauri"));
    assert!(build.contains("SPLINED_BUILD_GUI"));
    assert!(build.contains("/platform:x64"));
    assert!(native.contains("runtime\", \"splined-core.dll"));
    assert!(native.contains("LoadLibraryEx(path"));
    assert!(native.contains("LoadLibrarySearchDllLoadDir | LoadLibrarySearchSystem32"));
    assert!(native.contains("GetProcAddress(libraryHandle, name)"));
    assert!(!native.contains("DllImport(\"splined-core.dll\""));
    assert!(native.contains("splined_build_identity"));
    assert!(!native.contains("System.Diagnostics.Process"));
    for export in [
        "splined_initialize",
        "splined_media_snapshot",
        "splined_start_scan",
        "splined_submit_decision",
        "splined_cancel_scan",
        "splined_scan_active",
        "splined_build_identity",
        "splined_free_string",
    ] {
        assert!(
            ffi.contains(&format!("fn {export}")),
            "missing native export {export}"
        );
    }
    assert!(ffi.contains("catch_unwind"));
    assert!(ffi.contains("EventCallback"));
    assert!(ffi.contains("std::thread::spawn"));
    assert!(ffi.contains("initialize_app_root"));
    assert!(ffi.contains("initialize_state_root"));
}

#[test]
fn windows_gui_qa_uses_the_x64_excluded_output_directory() {
    let scripts = [
        "windows/gui/RUN-GUI-QA.cmd",
        "windows/gui/TEST-WINDOWS-GUI.cmd",
        "windows/gui/TEST-WINFORMS-GUI.cmd",
        "windows/gui/TEST-CREDENTIALS-WINDOWS-GUI.cmd",
        "windows/gui/TEST-PREVIEW-WINDOWS-GUI.cmd",
    ];
    for path in scripts {
        let script = source(path);
        assert!(
            script.contains(r#"set "QA_ROOT=%~dp0..\..\target-windows\winforms-qa""#)
                && script.contains(r#"set "QA_OUT=%QA_ROOT%\"#),
            "{path} does not use the excluded QA output directory"
        );
        assert!(script.contains("/platform:x64"), "{path} is not x64-only");
        assert!(
            !script.contains("qa-test"),
            "{path} still writes to the blocked legacy QA directory"
        );
    }
}

#[test]
fn rejected_desktop_and_executable_lifecycles_are_absent() {
    for path in ["windows/frontend", "windows/capabilities", "windows/nsis"] {
        let path = Path::new(path);
        if path.is_dir() {
            let mut files = Vec::new();
            repository_files(path, &mut files);
            assert!(
                files.is_empty(),
                "retired Windows component retained files: {}",
                path.display()
            );
        }
    }
    for path in [
        "windows/tauri.conf.json",
        "windows/src/main.rs",
        "windows/src/tauri_app.rs",
        "windows/updater",
    ] {
        assert!(
            !Path::new(path).exists(),
            "retired Windows component remained: {path}"
        );
    }

    let mut files = Vec::new();
    repository_files(Path::new("windows/src"), &mut files);
    repository_files(Path::new("windows/gui"), &mut files);
    for path in files {
        if !matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("rs" | "cs")
        ) {
            continue;
        }
        let lower = source(path.to_str().unwrap()).to_ascii_lowercase();
        for retired in [
            "splined-core.exe",
            "splined-update.exe",
            "--scan-dir",
            "splined-gui-",
            "extract executable",
            "cmd.exe",
            "powershell.exe",
            "std::process::command",
            "command::new(",
        ] {
            assert!(
                !lower.contains(retired),
                "{} retained {retired}",
                path.display()
            );
        }
    }
}

#[test]
fn portable_and_packaged_state_authorities_are_distinct() {
    let state = source("windows/gui/ConfigState.cs");
    let program = source("windows/gui/Program.cs");
    let backup = source("windows/gui/BackupWindows.cs");
    let portable = source("windows/src/portable.rs");

    assert!(state.contains("AppDomain.CurrentDomain.BaseDirectory"));
    assert!(state.contains("GetCurrentPackageFamilyName"));
    assert!(state.contains("PackageFamilyName, \"LocalState\", \"SPLINED\""));
    assert!(state.contains("RelativePathRoot = RuntimeStateRoot"));
    assert!(state.contains("Path.Combine(AppRoot, \"data\")"));
    assert!(state.contains("config.toml"));
    assert!(state.contains("ui.toml"));
    assert!(
        program
            .contains("NativeCore.Initialize(ConfigStore.AppRoot, ConfigStore.RelativePathRoot)")
    );
    assert!(backup.contains("Software\\Classes\\.spl"));
    assert!(backup.contains("if (ConfigStore.IsPackaged) return"));
    assert!(portable.contains("initialize_app_root"));
    assert!(portable.contains("initialize_state_root"));
}

#[test]
fn windows_release_is_x64_only_and_has_exact_portable_allowlist() {
    let workflow = source(".github/workflows/release-next-patch.yml");
    let package = source("windows/package/Package.appxmanifest");
    let appinstaller = source("windows/package/SPLINED.appinstaller.template");

    assert!(workflow.contains("WINDOWS_TARGET: \"x86_64-pc-windows-msvc\""));
    assert!(workflow.contains("rustup target add \"$WINDOWS_TARGET\""));
    assert!(workflow.contains("cargo build --locked --release --manifest-path windows/Cargo.toml --target $env:WINDOWS_TARGET"));
    assert!(workflow.contains("Expected AMD64 PE Machine 0x8664"));
    assert!(workflow.contains("Assert-Amd64Pe \"$portableRoot/splined.exe\""));
    assert!(workflow.contains("Assert-Amd64Pe \"$portableRoot/runtime/splined-core.dll\""));
    assert!(
        workflow.contains("\"README-WINDOWS.txt\", \"runtime/splined-core.dll\", \"splined.exe\"")
    );
    assert!(workflow.contains("SPLINED/README-WINDOWS.txt"));
    assert!(workflow.contains("SPLINED/runtime/splined-core.dll"));
    assert!(workflow.contains("SPLINED/splined.exe"));
    assert!(!workflow.contains("i686-pc-windows"));
    assert!(!workflow.contains("aarch64-pc-windows"));
    assert!(!workflow.contains("TAURI_"));

    let targets = workflow
        .split(|character: char| character.is_whitespace() || matches!(character, '"' | '\''))
        .filter(|word| word.contains("-pc-windows-"))
        .collect::<BTreeSet<_>>();
    assert_eq!(targets, BTreeSet::from(["x86_64-pc-windows-msvc"]));
    assert!(package.contains("ProcessorArchitecture=\"x64\""));
    assert!(package.contains("<uap:FileType>.spl</uap:FileType>"));
    assert!(appinstaller.contains("ProcessorArchitecture=\"x64\""));
    assert!(appinstaller.contains("__APPINSTALLER_HTTPS_URI__"));
    assert!(appinstaller.contains("__PUBLIC_PUBLISHER_DN__"));
}

#[test]
fn winappcli_builds_store_ready_and_development_signed_msix_without_release_secrets() {
    let workflow = source(".github/workflows/release-next-patch.yml");
    assert!(
        workflow.contains("rustup component add rustfmt clippy --toolchain \"$RUST_TOOLCHAIN\"")
    );
    assert!(!workflow.contains("microsoft/setup-WinAppCli"));
    assert!(workflow.contains(
        "npm install --prefix $installRoot --no-save --no-audit --no-fund \"@microsoft/winappcli@0.7.1\""
    ));
    assert!(workflow.contains("$commandDirectory | Out-File -FilePath $env:GITHUB_PATH"));
    assert!(workflow.contains(
        "$reportedVersion = (& (Join-Path $commandDirectory \"winapp.cmd\") --version) -join \"`n\""
    ));
    assert!(workflow.contains("$reportedVersion -notmatch '0\\.7\\.1'"));
    for line in workflow.lines().map(str::trim) {
        let Some(action) = line.strip_prefix("uses: ") else {
            continue;
        };
        let (repository, revision) = action
            .split_once('@')
            .expect("workflow action must include an explicit revision");
        assert!(
            repository.starts_with("actions/"),
            "workflow uses disallowed action repository: {repository}"
        );
        let revision = revision.split_whitespace().next().unwrap_or_default();
        assert!(
            revision.len() == 40 && revision.chars().all(|value| value.is_ascii_hexdigit()),
            "workflow action is not pinned to a full commit SHA: {action}"
        );
    }
    assert!(workflow.contains("winapp manifest update-assets"));
    assert!(workflow.contains("winapp create-debug-identity"));
    assert!(workflow.contains("--no-install"));
    assert!(workflow.contains("--output release-assets/SPLINED-x64.msix --no-sign"));
    assert!(workflow.contains("winapp cert generate --manifest"));
    assert!(workflow.contains("winapp cert install $cer"));
    assert!(workflow.contains("--cert $pfx --cert-password $password"));
    assert!(workflow.contains("Add-AppxPackage -Path $devPackage"));
    assert!(workflow.contains("Get-AppxPackage -Name SPLINED"));
    assert!(workflow.contains("--package-identity-smoke"));
    assert!(workflow.contains("[SplinedPackageActivator]::Activate"));
    assert!(workflow.contains("package-identity-smoke.ok"));
    assert!(workflow.contains("Remove-AppxPackage"));
    assert!(workflow.contains("certutil -delstore TrustedPeople $thumbprint"));
    assert!(!workflow.contains("Get-Process -Name splined"));
    assert!(!workflow.contains("TAURI_UPDATER_PUBLIC_KEY"));
    assert!(!workflow.contains("TAURI_SIGNING_PRIVATE_KEY"));
    assert!(!workflow.contains("latest.json"));
    assert!(!workflow.contains("NSIS"));
}

#[test]
fn full_windows_regression_suite_is_available_without_blocking_default_packaging() {
    let workflow = source(".github/workflows/release-next-patch.yml");
    assert!(workflow.contains("extended_windows_validation:"));
    assert!(workflow.contains("default: false"));
    assert!(workflow.contains("if: ${{ inputs.extended_windows_validation }}"));
    assert!(workflow.contains("cargo test --locked --manifest-path windows/Cargo.toml --lib"));
    assert!(workflow.contains("cmd /d /c RUN-GUI-QA.cmd"));
    assert!(workflow.contains("Build clean x64 WinForms and Rust DLL pair"));
}

#[test]
fn portable_updates_are_notification_only() {
    let update = source("windows/gui/UpdateService.cs");
    let main = source("windows/gui/MainForm.cs");
    assert!(update.contains("ReleaseUrl"));
    assert!(update.contains("DownloadDataTaskAsync"));
    assert!(update.contains("MaximumReleaseListBytes"));
    assert!(main.contains("UpdateService.OpenReleasePage"));
    for forbidden in [
        "DownloadFile",
        "splined-update.exe",
        "Process.Kill",
        "File.Replace",
        "latest.json",
    ] {
        assert!(!update.contains(forbidden));
    }
}
