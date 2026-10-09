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
    let store_workflow = source(".github/workflows/windows-store-update.yml");
    let package = source("windows/package/Package.appxmanifest");
    let appinstaller = source("windows/package/SPLINED.appinstaller.template");
    let store_docs = source("docs/windows-store.md");

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
    assert!(store_workflow.contains("WINDOWS_TARGET: \"x86_64-pc-windows-msvc\""));
    assert!(!store_workflow.contains("i686-pc-windows"));
    assert!(!store_workflow.contains("aarch64-pc-windows"));

    let targets = workflow
        .split(|character: char| character.is_whitespace() || matches!(character, '"' | '\''))
        .filter(|word| word.contains("-pc-windows-"))
        .collect::<BTreeSet<_>>();
    assert_eq!(targets, BTreeSet::from(["x86_64-pc-windows-msvc"]));
    assert!(package.contains("Name=\"Psycotix.SPLINED\""));
    assert!(package.contains("Publisher=\"CN=FE370EF6-D95D-4A6F-9AAB-2654E6DE00FE\""));
    assert!(package.contains("<PublisherDisplayName>Psycotix</PublisherDisplayName>"));
    assert!(package.contains("<DisplayName>SPLINED</DisplayName>"));
    assert!(package.contains("DisplayName=\"SPLINED\""));
    assert!(package.contains("Id=\"SPLINED\""));
    assert!(package.contains("ProcessorArchitecture=\"x64\""));
    assert!(package.contains("Name=\"Windows.Desktop\""));
    assert!(package.contains("<uap:FileType>.spl</uap:FileType>"));
    assert!(package.contains("<rescap:Capability Name=\"runFullTrust\" />"));
    assert!(appinstaller.contains("ProcessorArchitecture=\"x64\""));
    assert!(appinstaller.contains("__APPINSTALLER_HTTPS_URI__"));
    assert!(appinstaller.contains("Name=\"Psycotix.SPLINED\""));
    assert!(appinstaller.contains("Publisher=\"CN=FE370EF6-D95D-4A6F-9AAB-2654E6DE00FE\""));
    assert!(!package.contains("CN=SPLINED Development"));
    for expected in [
        "Psycotix.SPLINED",
        "CN=FE370EF6-D95D-4A6F-9AAB-2654E6DE00FE",
        "Psycotix.SPLINED_8pvn5te36e43t",
        "9P8G4GMBBVBS",
    ] {
        assert!(store_docs.contains(expected));
    }
}

#[test]
fn winappcli_isolates_production_store_and_development_msix_layouts() {
    let workflow = source(".github/workflows/release-next-patch.yml");
    let store_workflow = source(".github/workflows/windows-store-update.yml");
    let store_packager = source("windows/package/Build-StorePackage.ps1");
    let development_packager = source("windows/package/Test-DevelopmentPackage.ps1");
    assert!(
        workflow.contains("rustup component add rustfmt clippy --toolchain \"$RUST_TOOLCHAIN\"")
    );
    assert!(!store_workflow.contains("microsoft/setup-WinAppCli"));
    assert!(store_workflow.contains(
        "npm install --prefix $installRoot --no-save --no-audit --no-fund \"@microsoft/winappcli@0.7.1\""
    ));
    assert!(store_workflow.contains("$commandDirectory | Out-File -FilePath $env:GITHUB_PATH"));
    assert!(store_workflow.contains(
        "$reportedVersion = (& (Join-Path $commandDirectory \"winapp.cmd\") --version) -join \"`n\""
    ));
    assert!(store_workflow.contains("$reportedVersion -notmatch '0\\.7\\.1'"));
    for candidate in [&workflow, &store_workflow] {
        for line in candidate.lines().map(str::trim) {
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
    }
    assert!(store_packager.contains("winapp manifest update-assets"));
    let store = store_workflow
        .split("      - name: Build and strictly validate unsigned production Store MSIX")
        .nth(1)
        .expect("production Store MSIX step")
        .split("      - name: Development-sign, install, activate, native-smoke, and uninstall isolated QA package")
        .next()
        .expect("production Store MSIX body");
    assert!(store.contains("splined-msix-store-layout"));
    assert!(store.contains("SPLINED-x64-store-unsigned.msix"));
    assert!(!store.contains("winapp create-debug-identity"));
    assert!(!store.contains("--cert $pfx"));
    assert!(store.contains("windows/package/Build-StorePackage.ps1"));

    let development = store_workflow
        .split("      - name: Development-sign, install, activate, native-smoke, and uninstall isolated QA package")
        .nth(1)
        .expect("development MSIX step")
        .split("      - name: Upload Partner Center Store submission artifact")
        .next()
        .expect("development MSIX body");
    assert!(development.contains("splined-msix-development-layout"));
    assert!(development.contains("windows/package/Test-DevelopmentPackage.ps1"));
    assert!(!store_packager.contains("winapp create-debug-identity"));
    assert!(!store_packager.contains("--cert "));
    assert!(store_packager.contains("--no-sign"));
    assert!(store_packager.contains("Psycotix.SPLINED"));
    assert!(store_packager.contains("CN=FE370EF6-D95D-4A6F-9AAB-2654E6DE00FE"));
    assert!(store_packager.contains("PublisherDisplayName is not Psycotix"));
    assert!(store_packager.contains("DisplayName is not SPLINED"));
    assert!(store_packager.contains("TargetDeviceFamily is not Windows.Desktop"));
    assert!(store_packager.contains("MinVersion must remain 10.0.17763.0"));
    assert!(store_packager.contains("MaxVersionTested must remain 10.0.26100.0"));
    assert!(store_packager.contains("Application Id changed unexpectedly"));
    assert!(store_packager.contains("does not declare the .spl file association"));
    assert!(store_packager.contains("does not declare runFullTrust"));
    assert!(store_packager.contains("AppxSignature.p7x"));
    assert!(store_packager.contains("Partner Center Store MSIX must remain unsigned"));
    assert!(store_packager.contains("Store MSIX is missing splined.exe"));
    assert!(store_packager.contains("Store MSIX is missing runtime/splined-core.dll"));
    assert!(store_packager.contains("Assert-Amd64Pe $storeExe"));
    assert!(store_packager.contains("Assert-Amd64Pe $storeDll"));
    assert!(!store_packager.contains("SPLINED-x64-store-unsigned.msix"));
    assert!(development_packager.contains("winapp create-debug-identity"));
    assert!(development_packager.contains("--no-install"));
    assert!(development_packager.contains("Add-AppxPackage"));
    assert!(store_workflow.contains("windows/package/Build-StorePackage.ps1"));
    assert!(store_workflow.contains("windows/package/Test-DevelopmentPackage.ps1"));
    assert!(store_workflow.contains("name: splined-windows-store-submission"));
    assert!(development_packager.contains("winapp cert generate"));
    assert!(development_packager.contains("winapp cert install $cer"));
    assert!(development_packager.contains("--cert $pfx"));
    assert!(development_packager.contains("--cert-password $password"));
    assert!(development_packager.contains("Add-AppxPackage -Path $developmentPackage"));
    assert!(development_packager.contains("Get-AppxPackage -Name $developmentPackageName"));
    assert!(development_packager.contains("--package-identity-smoke"));
    assert!(development_packager.contains("[SplinedPackageActivator]::Activate"));
    assert!(development_packager.contains("package-identity-smoke.ok"));
    assert!(development_packager.contains("Remove-AppxPackage"));
    assert!(development_packager.contains("certutil -delstore TrustedPeople $thumbprint"));
    assert!(store_workflow.contains("path: ${{ steps.store-package.outputs.path }}"));
    assert!(store_workflow.contains("ref: refs/tags/${{ inputs.version }}"));
    assert!(store_workflow.contains("-ExpectedVersion \"$env:RELEASE_VERSION.0\""));
    assert!(!store_workflow.contains("contents: write"));
    assert!(!store_workflow.contains("git tag -a"));
    assert!(!store_workflow.contains("git push origin"));
    assert!(!workflow.contains("Build-StorePackage.ps1"));
    assert!(!workflow.contains("Test-DevelopmentPackage.ps1"));
    assert!(!workflow.contains("splined-windows-store-submission"));
    assert!(!workflow.contains("SPLINED-x64-store-unsigned.msix"));
    assert!(!workflow.contains("Get-Process -Name splined"));
    assert!(!workflow.contains("TAURI_UPDATER_PUBLIC_KEY"));
    assert!(!workflow.contains("TAURI_SIGNING_PRIVATE_KEY"));
    assert!(!workflow.contains("latest.json"));
    assert!(!workflow.contains("NSIS"));
}

#[test]
fn repository_exposes_exactly_the_three_supported_workflows() {
    let workflow_dir = Path::new(".github/workflows");
    let files = fs::read_dir(workflow_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        files,
        BTreeSet::from([
            "publish-existing-ghcr.yml".to_string(),
            "release-next-patch.yml".to_string(),
            "windows-store-update.yml".to_string(),
        ])
    );
    assert!(
        source(".github/workflows/release-next-patch.yml")
            .starts_with("name: SPLINED (All OS) and GHCR")
    );
    assert!(
        source(".github/workflows/windows-store-update.yml")
            .starts_with("name: SPLINED MS Store Windows Update")
    );
    assert!(
        source(".github/workflows/publish-existing-ghcr.yml")
            .starts_with("name: SPLINED Published > GHCR")
    );
}

#[test]
fn numeric_release_tag_is_pushed_only_after_selected_builds_pass() {
    let workflow = source(".github/workflows/release-next-patch.yml");
    let prepare = workflow
        .split("  prepare-release:")
        .nth(1)
        .unwrap()
        .split("  build-windows:")
        .next()
        .unwrap();
    assert!(prepare.contains("Prepared untagged release commit"));
    assert!(prepare.contains("git bundle create release-source.bundle HEAD \"^$BASE_MAIN_SHA\""));
    assert!(!prepare.contains("git tag -a"));
    assert!(!prepare.contains("refs/tags/${RELEASE_VERSION}:refs/tags/${RELEASE_VERSION}"));

    let finalize = workflow
        .split("  finalize-tag:")
        .nth(1)
        .unwrap()
        .split("  publish-release:")
        .next()
        .unwrap();
    for dependency in ["build-windows", "build-ubuntu", "build-macos"] {
        assert!(finalize.contains(dependency));
    }
    assert!(finalize.contains("Push numeric tag after successful builds"));
    assert!(finalize.contains("git tag -a \"$RELEASE_VERSION\" \"$RELEASE_COMMIT\""));
    assert!(finalize.contains("refs/tags/${RELEASE_VERSION}:refs/tags/${RELEASE_VERSION}"));
    assert!(finalize.contains("no tag was created"));
}

#[test]
fn windows_release_cache_excludes_discarded_target_artifacts() {
    let workflow = source(".github/workflows/release-next-patch.yml");
    assert!(workflow.contains("Calculate Windows dependency cache key"));
    assert!(workflow.contains("version = \"<release>\""));
    assert!(workflow.contains("steps.windows-dependency-cache-key.outputs.hash"));
    let cache = workflow
        .split("      - name: Restore Cargo dependency cache")
        .nth(1)
        .expect("Windows dependency cache step")
        .split("      - name: Run extended Windows regression suite")
        .next()
        .expect("Windows dependency cache body");
    assert!(cache.contains("~/.cargo/registry"));
    assert!(cache.contains("~/.cargo/git"));
    assert!(!cache.contains("windows/target"));
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
fn windows_updates_are_channel_aware_and_portable_updates_remain_notification_only() {
    let update = source("windows/gui/UpdateService.cs");
    let main = source("windows/gui/MainForm.cs");
    assert!(update.contains("ReleaseUrl"));
    assert!(update.contains("DownloadDataTaskAsync"));
    assert!(update.contains("MaximumReleaseListBytes"));
    assert!(main.contains("if (ConfigStore.IsPackaged)"));
    assert!(main.contains("CheckMicrosoftStoreForUpdateAsync"));
    assert!(main.contains("CheckPortableForUpdateAsync"));
    assert!(main.contains("WindowsUpdateService.OpenReleasePage"));
    assert!(main.contains("SPLINED Portable"));
    assert!(main.contains("SPLINED Microsoft Store"));
    assert!(main.contains("UPDATE NOW"));
    assert!(main.contains("LATER"));
    assert!(update.contains("Windows.Services.Store.StoreContext"));
    assert!(update.contains("GetAppAndOptionalStorePackageUpdatesAsync"));
    assert!(update.contains("RequestDownloadAndInstallStorePackageUpdatesAsync"));
    assert!(update.contains("https://apps.microsoft.com/detail/9p8g4gmbbvbs?hl=en-US&gl=US"));
    let store_path = main
        .split("private async Task CheckMicrosoftStoreForUpdateAsync")
        .nth(1)
        .unwrap()
        .split("private DialogResult ShowMicrosoftStoreUpdatePrompt")
        .next()
        .unwrap();
    assert!(!store_path.contains("GitHub"));
    assert!(!store_path.contains("Portable"));
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
