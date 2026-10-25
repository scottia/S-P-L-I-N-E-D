use std::fs;

fn source(path: &str) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("unable to read {path}: {error}"))
        .replace("\r\n", "\n")
}

const FULL: &str = ".github/workflows/01-full-release.yml";
const STORE: &str = ".github/workflows/02-store-recovery.yml";
const GHCR: &str = ".github/workflows/03-ghcr-recovery.yml";

#[test]
fn all_three_workflows_are_manual_and_actions_are_pinned() {
    for path in [FULL, STORE, GHCR] {
        let workflow = source(path);
        assert!(workflow.contains("workflow_dispatch:"), "{path}");
        assert!(workflow.contains("description:"), "{path}");
        for line in workflow.lines().map(str::trim) {
            let Some(action) = line.strip_prefix("uses: ") else {
                continue;
            };
            let (repository, revision) = action.split_once('@').expect("pinned action revision");
            assert!(
                repository.starts_with("actions/")
                    || repository == "microsoft/microsoft-store-apppublisher",
                "unapproved action: {action}"
            );
            let revision = revision.split_whitespace().next().unwrap_or_default();
            assert!(
                revision.len() == 40 && revision.chars().all(|c| c.is_ascii_hexdigit()),
                "action is not pinned to a SHA: {action}"
            );
        }
    }
}

#[test]
fn git_cliff_is_the_shared_pinned_release_note_engine() {
    let installer = source("release/Install-GitCliff.ps1");
    let github = source("release/Generate-ReleaseNotes.ps1");
    let store = source("release/Generate-StoreHighlights.ps1");
    let config = source("cliff.toml");
    assert!(installer.contains("$version = \"2.14.2\""));
    assert!(installer.contains("24f397c733add5390fdceee3a2088588ab0d5f944ce00d34cb7029b888cf2db4"));
    assert!(installer.contains("6ece2112b3f4af462190ecbea4aeb1315fff6af415629b597f84319073c31131"));
    assert!(github.contains("release-context.json"));
    assert!(github.contains("GITHUB-RELEASE.md"));
    assert!(store.contains("STORE-HIGHLIGHTS.txt"));
    assert!(store.contains("STORE-SUBMISSION.json"));
    assert!(config.contains("🛍️ Microsoft Store"));
    for path in [FULL, STORE, GHCR] {
        let workflow = source(path);
        assert!(workflow.contains("Install-GitCliff.ps1"), "{path}");
        assert!(workflow.contains("git-cliff 2.14.2"), "{path}");
        assert!(workflow.contains("cliff.toml"), "{path}");
    }
    for path in [FULL, GHCR] {
        assert!(source(path).contains("Generate-ReleaseNotes.ps1"), "{path}");
    }
    for path in [FULL, STORE] {
        assert!(
            source(path).contains("Generate-StoreHighlights.ps1"),
            "{path}"
        );
    }
}

#[test]
fn full_release_has_selected_os_gates_and_optional_strict_windows_validation() {
    let workflow = source(FULL);
    for choice in ["- \"All\"", "- \"Windows\"", "- \"Ubuntu\"", "- \"macOS\""] {
        assert!(workflow.contains(choice), "missing OS choice {choice}");
    }
    assert!(workflow.contains("extended_windows_validation:"));
    assert!(workflow.contains("cargo fmt --manifest-path windows/Cargo.toml -- --check"));
    assert!(workflow.contains("cargo clippy --locked --manifest-path windows/Cargo.toml"));
    assert!(workflow.contains("cargo test --locked --manifest-path windows/Cargo.toml"));
    assert!(workflow.contains("git push --atomic origin"));
    assert!(workflow.contains("--notes-file ../release-notes/GITHUB-RELEASE.md"));
    assert!(workflow.contains("gh release create \"$RELEASE_VERSION\""));
    let finalize = workflow.split("  finalize-tag:").nth(1).unwrap();
    for build in ["build-windows", "build-ubuntu", "build-macos"] {
        assert!(finalize.contains(build));
    }
}

#[test]
fn both_store_workflows_require_one_explicit_mode_and_preserve_draft_choice() {
    for path in [FULL, STORE] {
        let workflow = source(path);
        assert!(workflow.contains("store_submit_update:"), "{path}");
        assert!(workflow.contains("store_submit_draft:"), "{path}");
        assert!(workflow.contains("default: false"), "{path}");
        assert!(workflow.contains("STORE_UPDATE"), "{path}");
        assert!(workflow.contains("STORE_DRAFT"), "{path}");
        assert!(workflow.contains("-DraftOnly:$draftOnly"), "{path}");
        assert!(workflow.contains("Resolve-StoreBaseline.ps1"), "{path}");
        assert!(workflow.contains("Generate-StoreHighlights.ps1"), "{path}");
        assert!(workflow.contains("Publish-StoreUpdate.ps1"), "{path}");
        assert!(
            workflow.contains("group: splined-microsoft-store-submission"),
            "{path}"
        );
    }
    let full = source(FULL);
    assert!(full.contains("Select only one Store mode"));
    assert!(full.contains("Store publication requires Windows or All"));
    let recovery = source(STORE);
    assert!(recovery.contains("Select exactly one Store mode"));
    assert!(recovery.contains("ref: refs/tags/${{ inputs.version }}"));
    assert!(recovery.contains("$env:RELEASE_VERSION.0"));
    assert!(!recovery.contains("git tag -a"));
    assert!(!recovery.contains("gh release create"));
}

#[test]
fn store_helper_stages_once_and_submits_only_when_not_draft() {
    let helper = source("windows/package/Publish-StoreUpdate.ps1");
    let stage = helper
        .find("& msstore publish $package --appId $ProductId --noCommit")
        .unwrap();
    let read = helper.find("& msstore submission get $ProductId").unwrap();
    let update = helper
        .find("& msstore submission update $ProductId $updatedSubmission")
        .unwrap();
    let submit = helper
        .find("& msstore submission publish $ProductId")
        .unwrap();
    assert!(stage < read && read < update && update < submit);
    assert_eq!(helper.matches("& msstore publish ").count(), 1);
    assert!(helper.contains("[switch]$DraftOnly"));
    assert!(helper.contains("if ($DraftOnly)"));
    assert!(helper.contains("else {\n    $summaryTitle = \"submitted\"\n    & msstore submission publish"));
    assert!(helper.contains("1500-character"));
    assert!(helper.contains("highlights_sha256"));
}

#[test]
fn ghcr_recovery_uses_existing_tag_and_adds_missing_ubuntu_asset_only() {
    let workflow = source(GHCR);
    assert!(workflow.contains("ref: refs/tags/${{ inputs.version }}"));
    assert!(workflow.contains("gh release view \"$RELEASE_VERSION\""));
    assert!(workflow.contains("docker push \"$IMAGE_NAME:v$RELEASE_VERSION\""));
    assert!(workflow.contains("splined-linux-x86_64.tar.gz"));
    assert!(workflow.contains("cargo build --locked --release"));
    assert!(workflow.contains("gh release upload \"$RELEASE_VERSION\""));
    assert!(!workflow.contains("--clobber"));
    assert!(!workflow.contains("gh release create"));
    assert!(!workflow.contains("git tag -a"));
    assert!(!workflow.to_ascii_lowercase().contains("msstore"));
}
