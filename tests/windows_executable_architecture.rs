use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn source(path: &str) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("unable to read {path}: {error}"))
        .replace("\r\n", "\n")
}

#[test]
fn machine_readable_windows_slint_check_contract_matches_the_surface() {
    let contract: serde_json::Value =
        serde_json::from_str(&source("tests/windows_slint_check_contract.json"))
            .expect("Windows Slint check contract must remain valid JSON");
    assert_eq!(contract["schema"], 1);
    let ui = source("windows/ui/app.slint");
    let main = source("windows/src/main.rs");
    for label in contract["required_ui_labels"].as_array().unwrap() {
        let label = label.as_str().unwrap();
        assert!(ui.contains(label), "Slint surface is missing {label}");
    }
    for token in contract["forbidden_ui_tokens"].as_array().unwrap() {
        let token = token.as_str().unwrap();
        assert!(!ui.contains(token), "Slint surface still contains {token}");
    }
    for proof in contract["required_diagnostic_proofs"].as_array().unwrap() {
        let proof = proof.as_str().unwrap();
        assert!(main.contains(proof), "Slint diagnostic is missing {proof}");
    }
}

fn repository_files(root: &Path, files: &mut Vec<PathBuf>) {
    if !root.is_dir() {
        return;
    }
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            if !matches!(
                path.file_name().and_then(|value| value.to_str()),
                Some("target" | "rust-temp" | "debug" | "release")
            ) {
                repository_files(&path, files);
            }
        } else {
            files.push(path);
        }
    }
}

#[test]
fn windows_desktop_is_one_cargo_built_rust_slint_executable() {
    let manifest = source("windows/Cargo.toml");
    let build = source("windows/build.rs");
    let main = source("windows/src/main.rs");
    let library = source("windows/src/lib.rs");

    assert!(manifest.contains("[[bin]]\nname = \"splined\""));
    assert!(manifest.contains("slint ="));
    assert!(manifest.contains("slint-build ="));
    assert!(manifest.contains("crate-type = [\"rlib\"]"));
    assert!(!manifest.contains("cdylib"));
    assert!(!manifest.contains("autobins = false"));
    assert!(build.contains("slint_build::compile(\"ui/app.slint\")"));
    assert!(build.contains("1 ICON"));
    assert!(build.contains("rc.exe"));
    assert!(!build.contains("csc.exe"));
    assert!(!build.contains("System.Windows.Forms"));
    assert!(main.contains("slint::include_modules!()"));
    assert!(main.contains("ApplicationState"));
    assert!(main.contains("ScanController"));
    assert!(main.contains("config_migration::migrate_config_if_needed"));
    assert!(!main.contains("migrate_legacy_settings_if_needed"));
    assert!(main.contains("restore_request_from_args"));
    assert!(!library.contains("pub mod ffi"));
    assert!(!manifest.to_ascii_lowercase().contains("tauri"));
}

#[test]
fn no_active_csharp_or_winforms_implementation_remains() {
    let mut files = Vec::new();
    repository_files(Path::new("windows"), &mut files);
    let csharp = files
        .iter()
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("cs"))
        .collect::<Vec<_>>();
    assert!(csharp.is_empty(), "active C# files remain: {csharp:?}");

    for path in files.iter().filter(|path| {
        matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("rs" | "toml" | "ps1" | "slint")
        )
    }) {
        let lower = source(path.to_str().unwrap()).to_ascii_lowercase();
        for forbidden in [
            "system.windows.forms",
            "nativecore.cs",
            "mainform.cs",
            "splined_build_gui",
            "csc.exe",
        ] {
            assert!(
                !lower.contains(forbidden),
                "{} retained retired architecture token {forbidden}",
                path.display()
            );
        }
    }
    assert!(!Path::new("windows/src/ffi.rs").exists());
}

#[test]
fn slint_windows_restore_natively_and_share_live_media_models() {
    let ui = source("windows/ui/app.slint");
    let main = source("windows/src/main.rs");
    let manifest = source("windows/gui/app.manifest");
    let media_pane = ui
        .split("component MediaLibraryPane")
        .nth(1)
        .unwrap()
        .split("component SourceCard")
        .next()
        .unwrap();

    assert!(main.contains("let restore_maximized_timer = Timer::default()"));
    assert!(main.contains("let startup_reflow_timer = Timer::default()"));
    assert!(main.contains("Duration::from_millis(420)"));
    assert!(main.contains("app.media_selector_expanded = true"));
    assert!(main.contains("TimerMode::SingleShot"));
    assert!(!main.contains("ui.window().set_maximized(preferences.window_maximized)"));
    assert!(!main.contains("set_maximized(workspace_layout.borrow().maximized)"));
    assert!(!main.contains("window.window().set_maximized(layout.maximized)"));
    assert!(main.contains("repair_main_maximized_rect(&ui, &geometry_maximize_guard)"));
    assert!(!ui.contains("CustomUpscaleWindow"));
    assert!(!main.contains("repair_custom_maximized_rect"));
    assert!(!main.contains("sync_custom_upscale"));
    assert!(!main.contains("SLINT_SCALE_FACTOR"));
    assert!(manifest.contains("PerMonitorV2"));
    assert!(media_pane.contains(
        "if root.expanded: VerticalLayout {\n            spacing: 7px;\n            vertical-stretch: 1;"
    ));
    assert!(media_pane.contains("min-height: 180px;"));
    for proof in [
        "artist_click_kept_view",
        "artist_inline_album_rows",
        "fallback_album_enables_launch",
        "fallback_album_expands_inline",
        "fallback_album_queues_whole_album",
        "restored_media_width_at_least_min",
        "media_model_restored",
    ] {
        assert!(
            main.contains(proof),
            "missing Slint diagnostic proof {proof}"
        );
    }
}

#[test]
fn slint_surface_exposes_real_migrated_workflows() {
    let ui = source("windows/ui/app.slint");
    let main = source("windows/src/main.rs");
    let application = source("windows/src/application.rs");
    let controller = source("windows/src/scan_controller.rs");

    for label in [
        "Artists",
        "Albums",
        "Fallback",
        "Candidate Artwork",
        "Bypass",
        "MusicBrainz Results",
        "Upscale / Enhance",
        "Provider Credentials",
        "Backup / Restore",
        "SQLite Database Snapshot (.sqlite)",
        "Diagnostic Summary (.json)",
    ] {
        assert!(ui.contains(label), "Slint surface is missing {label}");
    }
    assert!(
        ui.contains("launch-mode-label") && main.contains("\"LIVE WRITE\""),
        "Slint launch mode is not projected from the Rust LIVE WRITE state"
    );
    assert!(
        ui.contains("Shared/network SQLite (Python/Docker owns the inventory)"),
        "first-run Setup does not expose the optional shared SQLite authority"
    );
    assert!(
        ui.contains("preferred-width: 1680px")
            && ui.contains("min-width: 960px")
            && !ui.contains("width: 1380px; height: 860px"),
        "the Slint application window is fixed-size instead of resizable"
    );
    assert!(
        ui.contains("clip: true")
            && ui.contains(
                "source: root.current-preview; width: parent.width; height: parent.height"
            ),
        "the artwork preview can overflow and intercept the workspace"
    );
    assert!(
        main.contains("import_settings_from_opened_backup")
            && main.contains("Config v5 settings imported from the opened .spl backup"),
        "associated .spl files do not import Config v5 settings before startup"
    );
    for action in [
        "toggle_row",
        "candidate_decision",
        "musicbrainz_result_decision",
        "persist_adjustments_to_config",
    ] {
        assert!(
            main.contains(action),
            "UI is not wired to Rust action {action}"
        );
    }
    assert!(application.contains("pub struct ApplicationState"));
    assert!(application.contains("pub struct LibraryState"));
    assert!(controller.contains("run_scan_library_read_report"));
    assert!(controller.contains("std::thread::spawn"));
    assert!(!ui.contains("CheckBox"));
    assert!(!ui.contains("Select up to five"));
    assert!(!ui.contains("Select up to 5"));
    assert!(ui.contains("Activate All 8"));
    assert_eq!(
        ui.matches("for candidate in root.candidate-rows: CandidateCard")
            .count(),
        1,
        "candidate cards are duplicated outside the Candidate Artwork panel"
    );
    assert!(!ui.contains("Upscale Settings"));
    assert!(!ui.contains("Select a candidate below to preview and apply."));
    assert!(!ui.contains("All eight controls update both previews live."));
    assert!(ui.contains("label: \"Upscale Preview\"; width: (parent.width - 12px) / 3"));
    assert!(
        ui.contains("label: root.selected-artwork-write-label; width: (parent.width - 12px) / 3")
    );
    assert!(ui.contains("label: \"EMBED Thumbs\"; width: (parent.width - 12px) / 3"));
    assert!(ui.contains("source: root.original-preview"));
    assert!(ui.contains("source: root.current-preview"));
    assert_eq!(main.matches("set_original_preview").count(), 1);
    let activate_all_handler = main
        .split("ui.on_activate_all_adjustments")
        .nth(1)
        .unwrap()
        .split("ui.on_edit_existing_cover")
        .next()
        .unwrap();
    assert!(activate_all_handler.contains("schedule_adjustment_preview"));
    assert!(!activate_all_handler.contains("set_original_preview"));
    assert!(!ui.contains("label: \"Up\""));
    assert!(!ui.contains("label: \"Down\""));
    assert!(!ui.contains("\"Expand\""));
    assert!(!ui.contains("\"Collapse\""));
    assert!(ui.contains("callback set-value(string, float)"));
    let expansion_glyph = ui
        .split("component ExpansionGlyph")
        .nth(1)
        .unwrap()
        .split("component RadioToggle")
        .next()
        .unwrap();
    assert!(expansion_glyph.contains("background: transparent"));
    assert!(expansion_glyph.contains("border-width: 0px"));
    assert!(expansion_glyph.contains("root.expanded ? AppTheme.magenta : AppTheme.cyan"));
    assert_eq!(ui.matches("Open MB Page").count(), 2);
    assert!(ui.contains("MusicBrainz Results · grouped by release family"));
    assert!(ui.contains("musicbrainz-page-scroll := ScrollView"));
    assert!(ui.contains("content-height: max(self.visible-height, musicbrainz-page-panel.height)"));
    assert!(ui.contains("VISITED"));
    assert!(main.contains("item.visited = true"));
    assert!(main.contains("coverartarchive.org/release/{release_mbid}/front-500"));
    assert!(main.contains("musicbrainz-cover-previews"));
    assert!(!ui.contains("Show Tracks / Embedded Art"));
    assert!(!ui.contains("MUSIC LOOKS BETTER HERE"));
    assert!(!ui.contains("MANAGE · ENHANCE · PRESERVE · MUSIC FOREVER"));
    assert_eq!(ui.matches("\"LAUNCH\"").count(), 1);
    assert!(ui.contains("Media Filter"));
    assert!(ui.contains("Fallback Albums"));
    assert!(ui.contains("for row in root.library-rows: VerticalLayout"));
    assert!(ui.contains("if row.expanded: VerticalLayout"));
    assert!(!ui.contains("Select an album to show its tracks."));
    assert!(ui.contains("horizontal-stretch: 1; NeonAction { label: \"All\""));
    assert!(ui.contains("height: 430px; clip: true"));
    assert!(ui.contains("callback library-row-expansion-activated(string)"));
    assert!(application.contains("select_album_exclusive"));
    assert!(ui.contains("private property <float> drag-origin: root.position"));
    assert!(!ui.contains("root.position + (root.reverse"));
    assert!(ui.contains("component WorkspaceButton inherits Rectangle"));
    assert_eq!(ui.matches("WorkspaceButton { label: \"").count(), 5);
    assert!(ui.contains("component WorkspaceButton inherits Rectangle"));
    assert!(ui.contains("border-color: AppTheme.border;"));
    assert!(
        ui.contains("width: 6px;\n    background: drag-area.pressed ? AppTheme.selected-surface")
    );
    assert!(
        ui.contains("height: 6px;\n    background: drag-area.pressed ? AppTheme.selected-surface")
    );
    assert!(ui.contains("x: (parent.width - self.width) / 2; source: @image-url(\"../gui/source-icons/musicbrainz.svg\")"));
    assert!(ui.contains("padding-left: 6px; padding-right: 6px; spacing: 7px"));
    assert!(ui.contains("in-out property <bool> artist-biography-expanded: false"));
    assert!(
        ui.contains("text: \"(…more)\"; color: AppTheme.yellow; font-size: 17px; font-weight: 800")
    );
    assert!(
        ui.contains("text: \"(less…)\"; color: AppTheme.yellow; font-size: 17px; font-weight: 800")
    );
    assert!(ui.contains("font-size: 18px; wrap: word-wrap; overflow: elide"));
    assert!(ui.contains("if root.artist-biography-expanded: VerticalLayout"));
    assert!(ui.contains("height: self.preferred-height;"));
    assert!(ui.contains("Rectangle { horizontal-stretch: 1; background: transparent; }"));
    assert!(ui.contains("callback artist-overview-navigate(int)"));
    assert!(!ui.contains("clicked => root.artist-overview-navigate(-1)"));
    assert!(!ui.contains("clicked => root.artist-overview-navigate(1)"));
    assert!(main.contains("adjacent_pending_artist"));
    assert!(!ui.contains("Select an artist for Artist Overview"));
    assert!(!ui.contains("root.current-artist + \"  >  \" + root.current-album; color:"));
    assert!(ui.contains("visible: root.current-artist != \"\" || root.current-album != \"\""));
    assert!(!ui.contains("Text { text: root.selected-artwork-dimensions; color: #ffffff; font-size: 16px; font-weight: 700"));
    assert!(ui.contains("Text { text: root.selected-artwork-quality; color: AppTheme.green"));
    assert!(ui.contains("default-font-size: 16px"));
    assert!(ui.contains("text: root.detail; color: AppTheme.foreground; font-size: 14px"));
    assert!(ui.contains("On keeps existing art and uses a numbered filename. Off replaces the matching cover or Artist Art.\"; color: AppTheme.secondary-foreground; font-size: 13px"));
    assert!(application.contains("fallback_album_rows"));
    assert!(application.contains("toggle_artist_scope"));
    assert!(ui.contains("SKIP ALBUM"));
    assert!(ui.contains("splined-watermark.png"));
    assert!(Path::new("windows/gui/set-your-imagination-tv.jpg").is_file());
    assert_eq!(ui.matches("set-your-imagination-tv.jpg").count(), 1);
    assert!(ui.contains(
        "private property <bool> artwork-placeholder-visible: root.intro-active || (root.current-album == \"\" && !root.selected-artwork-artist-overview && !root.artist-artwork-mode)"
    ));
    assert!(
        ui.contains("width: min(max(0px, parent.width - 40px), max(0px, parent.height - 40px))")
    );
    assert!(ui.contains("visible: !root.artwork-placeholder-visible"));
    assert!(ui.contains("component MissingArtistArt inherits Rectangle"));
    assert!(ui.contains("source: @image-url(\"../gui/source-icons/navidrome.svg\")"));
    assert!(ui.contains("Text { text: \"ARTIST\"; color: AppTheme.violet"));
    assert!(ui.contains("selected-artwork-artist-overview"));
    assert!(ui.contains("selected-artwork-write-ready"));
    assert!(main.contains("Saving artist.*…"));
    assert!(main.contains("Saved {file_name}"));
    assert!(main.contains("discover_remote_artist_artwork"));
    assert!(main.contains("synchronize_fallback_album"));
    assert!(main.contains("let current_cover_path = if suppress_fallback_cover"));
    assert!(application.contains("self.album_visible(*index, &artists)"));
    assert!(ui.contains("window-background: root.chalk ?"));
    assert!(ui.contains("panel-background: root.chalk ?"));
    assert!(!ui.contains("imagination-message.svg"));
    assert!(!ui.contains("Welcome to S.P.L.I.N.E.D."));
    assert!(ui.contains(
        "HorizontalLayout { height: 62px; alignment: center;\n                    BrandMark { width: 360px; height: 54px; }"
    ));
    let neon_panel = ui
        .split("component NeonPanel")
        .nth(1)
        .unwrap()
        .split("component NeonAction")
        .next()
        .unwrap();
    assert!(neon_panel.contains("border-width: 0px"));
    assert!(!neon_panel.contains("border-color:"));
    for colored_panel in [
        "NeonPanel { vertical-stretch: 1; border-color:",
        "NeonPanel { horizontal-stretch: 1; max-width: 10000px; border-color:",
        "visible: root.show-artwork; border-color:",
        "NeonPanel { height: root.activity-pane-height * 1px; border-color:",
        "NeonPanel { horizontal-stretch: 1; border-color:",
    ] {
        assert!(
            !ui.contains(colored_panel),
            "workspace panel still carries a colored border: {colored_panel}"
        );
    }
    let candidate_hover_handler = main
        .split("ui.on_candidate_hover")
        .nth(1)
        .unwrap()
        .split("ui.on_set_hover_enabled")
        .next()
        .unwrap();
    assert!(candidate_hover_handler.contains("state.try_lock()"));
    assert!(!candidate_hover_handler.contains("state.lock()"));
    assert!(!ui.contains("fixture"));
}

#[test]
fn portable_and_packaged_state_authorities_are_distinct() {
    let portable = source("windows/src/portable.rs");
    let main = source("windows/src/main.rs");
    assert!(portable.contains("GetCurrentPackageFamilyName"));
    assert!(portable.contains("LocalState"));
    assert!(portable.contains("SPLINED_INTERNAL_STATE_ROOT"));
    assert!(portable.contains("pub fn is_packaged_process"));
    assert!(main.contains("package_identity_smoke"));
    assert!(main.contains("package-identity-smoke.ok"));
    for retired in [
        "WindowsV4",
        "ConfigV5",
        "UiV4",
        "PortableFilesMigrated",
        "bootstrap_portable_install",
        "docker_builds",
    ] {
        assert!(
            !portable.contains(retired),
            "portable startup retained retired behavior: {retired}"
        );
    }
}

#[test]
fn windows_release_is_x64_single_executable_with_exact_portable_allowlist() {
    let workflow = source(".github/workflows/release-next-patch.yml");
    let store_workflow = source(".github/workflows/windows-store-package-resolution.yml");
    assert!(workflow.contains("WINDOWS_TARGET: \"x86_64-pc-windows-msvc\""));
    assert!(workflow.contains("Build clean x64 Rust/Slint executable"));
    assert!(workflow.contains("Assert-Amd64Pe \"$portableRoot/splined.exe\""));
    assert!(workflow.contains("\"README-WINDOWS.txt\", \"splined.exe\""));
    assert!(workflow.contains("--write-build-info"));
    assert!(workflow.contains("--native-smoke"));
    assert!(workflow.contains("splined-portable-smoke-state"));
    assert!(workflow.contains("$env:SPLINED_INTERNAL_STATE_ROOT = $smokeStateRoot"));
    assert!(!workflow.contains("splined_core.dll"));
    assert!(!workflow.contains("splined-core.dll"));
    assert!(!workflow.contains("WinForms"));
    assert!(!store_workflow.contains("splined_core.dll"));
    assert!(!store_workflow.contains("WinForms"));
    assert!(!workflow.contains("i686-pc-windows"));
    assert!(!workflow.contains("aarch64-pc-windows"));
}

#[test]
fn store_package_uses_the_same_single_rust_executable() {
    let workflow = source(".github/workflows/release-next-patch.yml");
    let store_workflow = source(".github/workflows/windows-store-package-resolution.yml");
    let packager = source("windows/package/Build-StorePackage.ps1");
    let qa = source("windows/package/Test-DevelopmentPackage.ps1");
    let package = source("windows/package/Package.appxmanifest");

    for candidate in [&workflow, &store_workflow] {
        assert!(candidate.contains("windows/package/Build-StorePackage.ps1"));
        assert!(candidate.contains("windows/package/Test-DevelopmentPackage.ps1"));
        assert!(!candidate.contains("CoreDllPath"));
    }
    assert!(!packager.contains("CoreDllPath"));
    assert!(!qa.contains("CoreDllPath"));
    assert!(!qa.contains("Add-Type"));
    assert!(qa.contains("shell:AppsFolder"));
    assert!(qa.contains("package-identity-smoke.request"));
    assert!(qa.contains("$developmentApplicationId"));
    assert!(qa.contains("$certificateInstalled = $true"));
    assert!(qa.contains("$certificateInstalled -and $thumbprint"));
    assert!(packager.contains("Store MSIX is missing splined.exe"));
    assert!(packager.contains("Assert-Amd64Pe $storeExe"));
    assert!(packager.contains("--no-sign"));
    assert!(package.contains("Name=\"Psycotix.SPLINED\""));
    assert!(package.contains("ProcessorArchitecture=\"x64\""));
    assert!(package.contains("<uap:FileType>.spl</uap:FileType>"));
    assert!(package.contains("<rescap:Capability Name=\"runFullTrust\" />"));
}

#[test]
fn updates_are_channel_aware_and_never_self_replace_binaries() {
    let update = source("windows/src/update.rs");
    let main = source("windows/src/main.rs");
    assert!(update.contains("is_newer_semantic_version"));
    assert!(update.contains("GetAppAndOptionalStorePackageUpdatesAsync"));
    assert!(update.contains("RequestDownloadAndInstallStorePackageUpdatesAsync"));
    assert!(update.contains("IInitializeWithWindow"));
    assert!(update.contains("check_portable_update"));
    assert!(update.contains("STABLE_RELEASES_API"));
    assert!(main.contains("is_packaged_process"));
    assert!(main.contains("check_store_updates"));
    assert!(main.contains("install_store_updates"));
    assert!(main.contains("check_portable_update"));
    for forbidden in [
        "splined-update.exe",
        "Process.Kill",
        "File.Replace",
        "latest.json",
    ] {
        assert!(!update.contains(forbidden));
    }
}

#[test]
fn repository_exposes_exactly_the_four_supported_workflows() {
    let workflow_dir = Path::new(".github/workflows");
    let files = fs::read_dir(workflow_dir)
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            let path = entry.path();
            let contents = fs::read_to_string(&path).ok()?;
            contents
                .starts_with("name: SPLINED")
                .then(|| entry.file_name().to_string_lossy().into_owned())
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        files,
        BTreeSet::from([
            "docker-ghcr-resolution.yml".to_string(),
            "release-next-patch.yml".to_string(),
            "windows-store-package-resolution.yml".to_string(),
            "windows-store-publish-update.yml".to_string(),
        ])
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
    assert!(!prepare.contains("git tag -a"));
    let finalize = workflow
        .split("  finalize-tag:")
        .nth(1)
        .unwrap()
        .split("  generate-release-notes:")
        .next()
        .unwrap();
    for dependency in ["build-windows", "build-ubuntu", "build-macos"] {
        assert!(finalize.contains(dependency));
    }
    assert!(finalize.contains("git tag -a \"$RELEASE_VERSION\" \"$RELEASE_COMMIT\""));
}

#[test]
fn windows_dependency_cache_does_not_restore_binary_targets() {
    let workflow = source(".github/workflows/release-next-patch.yml");
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
