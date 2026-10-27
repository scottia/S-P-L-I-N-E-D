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
    for path in [FULL, STORE] {
        let workflow = source(path);
        assert!(workflow.contains("Install-GitCliff.ps1"), "{path}");
        assert!(workflow.contains("git-cliff 2.14.2"), "{path}");
        assert!(workflow.contains("cliff.toml"), "{path}");
    }
    assert!(source(FULL).contains("Generate-ReleaseNotes.ps1"));
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
    assert!(
        helper.contains(
            "else {\n    $summaryTitle = \"submitted\"\n    & msstore submission publish"
        )
    );
    assert!(helper.contains("1500-character"));
    assert!(helper.contains("highlights_sha256"));
}

#[test]
fn ghcr_recovery_is_release_immutable_and_existing_release_only() {
    let workflow = source(GHCR);
    assert!(workflow.contains("ref: refs/tags/${{ inputs.version }}"));
    assert!(workflow.contains("gh release view \"$RELEASE_VERSION\""));
    assert!(workflow.contains("test \"$(jq -r '.isDraft' <<< \"$release\")\" = false"));
    assert!(workflow.contains("test \"$(jq -r '.isPrerelease' <<< \"$release\")\" = false"));
    assert!(workflow.contains("GitHub Release, tag, and assets modified: \\`no\\`"));

    for forbidden in [
        "gh release upload",
        "gh release download",
        "gh release edit",
        "gh release create",
        "gh release delete",
        "splined-linux-x86_64",
        "git tag -a",
        "git push",
        "--clobber",
        "Generate-ReleaseNotes.ps1",
        "Install-GitCliff.ps1",
        "msstore",
    ] {
        assert!(
            !workflow
                .to_ascii_lowercase()
                .contains(&forbidden.to_ascii_lowercase()),
            "recovery contains forbidden release mutation: {forbidden}"
        );
    }
}

#[test]
fn ghcr_recovery_is_idempotent_and_fails_closed_on_conflicts() {
    let workflow = source(GHCR);
    for action in [
        "action=noop",
        "action=recover-version",
        "action=recover-latest",
    ] {
        assert!(
            workflow.contains(action),
            "missing recovery action {action}"
        );
    }
    for provenance in [
        "org.opencontainers.image.source",
        "org.opencontainers.image.version",
        "org.opencontainers.image.revision",
        "Provenance cannot be verified",
    ] {
        assert!(
            workflow.contains(provenance),
            "missing provenance check {provenance}"
        );
    }
    assert!(workflow.contains("Existing version and latest tags have different digests"));
    assert!(workflow.contains("neither tag will be overwritten"));
    assert!(workflow.contains("assert_remote_tag_missing"));
    assert!(workflow.contains("refusing a possible overwrite"));
    assert!(workflow.contains("if: ${{ steps.inspect.outputs.action != 'noop' }}"));
    assert!(workflow.contains("docker push \"$VERSION_REF\""));
    assert!(workflow.contains("docker push \"$LATEST_REF\""));
    assert!(workflow.contains("Both tags are missing"));
    assert!(workflow.contains("historical build inputs cannot be proven identical"));
    assert!(
        workflow.contains("recovery will not rebuild or publish a potentially different image")
    );
    assert!(!workflow.contains("action=build-both"));
    assert!(!workflow.contains("docker build"));

    let recover_version = workflow
        .split("            recover-version)")
        .nth(1)
        .expect("recover-version branch")
        .split("            recover-latest)")
        .next()
        .unwrap();
    assert!(recover_version.contains("assert_remote_tag_missing \"$VERSION_REF\""));
    assert!(recover_version.contains("docker push \"$VERSION_REF\""));
    assert!(!recover_version.contains("docker push \"$LATEST_REF\""));

    let recover_latest = workflow
        .split("            recover-latest)")
        .nth(1)
        .expect("recover-latest branch")
        .split("            *)")
        .next()
        .unwrap();
    assert!(recover_latest.contains("assert_remote_tag_missing \"$LATEST_REF\""));
    assert!(recover_latest.contains("docker push \"$LATEST_REF\""));
    assert!(!recover_latest.contains("docker push \"$VERSION_REF\""));
}

#[test]
fn workflow_one_remains_the_sole_normal_ghcr_publisher() {
    let full = source(FULL);
    let recovery = source(GHCR);
    assert!(full.contains("  publish-ghcr:"));
    assert!(full.contains("needs.publish-release.result == 'success'"));
    assert!(full.contains("docker push \"$IMAGE_NAME:v${RELEASE_VERSION}\""));
    assert!(full.contains("docker push \"$IMAGE_NAME:latest\""));
    assert!(recovery.contains("group: splined-release-next-patch"));
    assert!(recovery.contains("Older release refused"));
    assert!(recovery.contains("action=noop"));
    assert!(!recovery.contains("release_version:"));
    assert!(!recovery.contains("gh release create"));
    assert!(!recovery.contains("git tag -a"));
}
