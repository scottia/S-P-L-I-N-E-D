use std::fs;

fn source(path: &str) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("unable to read {path}: {error}"))
        .replace("\r\n", "\n")
}

#[test]
fn git_cliff_2_14_2_is_the_single_release_note_engine() {
    let installer = source("release/Install-GitCliff.ps1");
    let generator = source("release/Generate-ReleaseNotes.ps1");
    let config = source("cliff.toml");
    let github_template = source("release/templates/github-release.tera");
    let store_template = source("release/templates/store-highlights.tera");

    assert!(installer.contains("$version = \"2.14.2\""));
    assert!(installer.contains("git-cliff $version"));
    assert!(!installer.to_ascii_lowercase().contains("latest"));
    assert!(installer.contains("24f397c733add5390fdceee3a2088588ab0d5f944ce00d34cb7029b888cf2db4"));
    assert!(installer.contains("6ece2112b3f4af462190ecbea4aeb1315fff6af415629b597f84319073c31131"));
    assert!(
        installer.contains("$env:PATH = \"$InstallDirectory$([IO.Path]::PathSeparator)$env:PATH\"")
    );

    assert_eq!(generator.matches("--context").count(), 1);
    assert_eq!(generator.matches("--from-context").count(), 2);
    assert!(generator.contains("release-context.json"));
    assert!(generator.contains("GITHUB-RELEASE.md"));
    assert!(generator.contains("STORE-HIGHLIGHTS.txt"));
    assert!(!generator.contains("python"));
    assert!(!generator.contains("node"));

    for group in [
        "✨ Highlights",
        "🪟 Windows",
        "🛍️ Microsoft Store",
        "🐧 Linux / Ubuntu",
        "🍎 macOS",
        "🐳 Docker / GHCR",
        "🛠️ Fixes & Reliability",
        "📚 Documentation",
        "🔧 Maintenance",
        "📦 Other Changes",
    ] {
        assert!(config.contains(group), "missing git-cliff group: {group}");
    }
    assert!(config.contains("{ message = \".*\", group = \"📦 Other Changes\" }"));
    assert!(config.contains("filter_unconventional = false"));
    assert!(config.contains("filter_commits = false"));
    assert!(github_template.contains("Full changelog:"));
    assert!(github_template.contains("/compare/"));
    assert!(store_template.contains("🛍️ Microsoft Store"));
    assert!(!store_template.contains("📚 Documentation"));
    assert!(!store_template.contains("🔧 Maintenance"));
}

#[test]
fn every_supported_workflow_uses_the_shared_git_cliff_engine() {
    for path in [
        ".github/workflows/release-next-patch.yml",
        ".github/workflows/windows-store-package-resolution.yml",
        ".github/workflows/windows-store-publish-update.yml",
        ".github/workflows/docker-ghcr-resolution.yml",
    ] {
        let workflow = source(path);
        assert!(
            workflow.contains("Install-GitCliff.ps1"),
            "{path} does not install git-cliff"
        );
        assert!(
            workflow.contains("git-cliff --version"),
            "{path} does not verify git-cliff"
        );
        assert!(
            workflow.contains("git-cliff 2.14.2"),
            "{path} does not pin git-cliff 2.14.2"
        );
        assert!(
            workflow.contains("Generate-ReleaseNotes.ps1"),
            "{path} does not use the shared context renderer"
        );
        assert!(
            workflow.contains("cliff.toml"),
            "{path} does not use the authoritative classification"
        );
    }
}

#[test]
fn every_workflow_action_is_pinned_to_an_immutable_commit() {
    for path in [
        ".github/workflows/release-next-patch.yml",
        ".github/workflows/windows-store-package-resolution.yml",
        ".github/workflows/windows-store-publish-update.yml",
        ".github/workflows/docker-ghcr-resolution.yml",
    ] {
        for line in source(path).lines().map(str::trim) {
            let Some(action) = line.strip_prefix("uses: ") else {
                continue;
            };
            let (repository, revision) = action
                .split_once('@')
                .unwrap_or_else(|| panic!("{path} action lacks a revision: {action}"));
            assert!(
                repository.starts_with("actions/")
                    || repository == "microsoft/microsoft-store-apppublisher",
                "{path} uses an unapproved action repository: {repository}"
            );
            let revision = revision.split_whitespace().next().unwrap_or_default();
            assert!(
                revision.len() == 40 && revision.chars().all(|value| value.is_ascii_hexdigit()),
                "{path} action is not pinned to a full commit SHA: {action}"
            );
        }
    }
}

#[test]
fn normal_release_notes_and_store_option_follow_the_release_contract() {
    let workflow = source(".github/workflows/release-next-patch.yml");
    assert!(workflow.contains("microsoft_store:"));
    assert!(workflow.contains("default: true"));
    assert!(!workflow.contains("--generate-notes"));
    assert!(workflow.contains("--notes-file ../release-notes/GITHUB-RELEASE.md"));
    assert!(workflow.contains("release-notes/release-context.json"));
    assert!(workflow.contains("release-notes/STORE-HIGHLIGHTS.txt"));

    let windows_job = workflow
        .split("  build-windows:")
        .nth(1)
        .unwrap()
        .split("  build-ubuntu:")
        .next()
        .unwrap();
    assert_eq!(
        windows_job
            .matches("cargo build --locked --release --manifest-path windows/Cargo.toml --target $env:WINDOWS_TARGET")
            .count(),
        1,
        "normal release must compile the Windows runtime pair once"
    );
    assert_eq!(
        windows_job
            .matches("inputs.microsoft_store && (inputs.platform == 'Windows' || inputs.platform == 'All')")
            .count(),
        4,
        "all Store build/QA/upload steps must be gated by the Store option"
    );
    assert!(windows_job.contains("$releaseRoot/splined.exe"));
    assert!(windows_job.contains("$releaseRoot/splined_core.dll"));
    assert!(windows_job.contains("name: splined-windows-store-package"));

    let public_release = workflow
        .split("  publish-release:")
        .nth(1)
        .unwrap()
        .split("  publish-ghcr:")
        .next()
        .unwrap();
    assert!(!public_release.contains("SPLINED-x64-store-unsigned.msix"));
    assert!(!public_release.contains("splined-windows-store-package"));
    assert!(!public_release.contains("splined-windows-store-submission"));
    assert!(public_release.contains("name: splined-windows-x86_64"));

    let store_job = workflow
        .split("  publish-store:")
        .nth(1)
        .unwrap()
        .split("  release-summary:")
        .next()
        .unwrap();
    assert!(store_job.contains("inputs.microsoft_store"));
    assert!(store_job.contains("inputs.platform == 'Windows' || inputs.platform == 'All'"));
    assert!(store_job.contains("name: splined-windows-store-submission"));
    assert!(store_job.contains("SPLINED-x64-store-unsigned.msix"));
    assert!(store_job.contains("STORE-HIGHLIGHTS.txt"));
    assert!(store_job.contains("windows/package/Publish-StoreUpdate.ps1"));
}

#[test]
fn normal_release_dependency_gates_tolerate_only_intentional_skips() {
    let workflow = source(".github/workflows/release-next-patch.yml");

    let finalize = workflow
        .split("  finalize-tag:")
        .nth(1)
        .unwrap()
        .split("  generate-release-notes:")
        .next()
        .unwrap();
    assert!(finalize.contains("always() &&"));
    for expected in [
        "inputs.platform == 'Windows' &&\n            needs.build-windows.result == 'success' &&\n            needs.build-ubuntu.result == 'skipped' &&\n            needs.build-macos.result == 'skipped'",
        "inputs.platform == 'Ubuntu' &&\n            needs.build-windows.result == 'skipped' &&\n            needs.build-ubuntu.result == 'success' &&\n            needs.build-macos.result == 'skipped'",
        "inputs.platform == 'macOS' &&\n            needs.build-windows.result == 'skipped' &&\n            needs.build-ubuntu.result == 'skipped' &&\n            needs.build-macos.result == 'success'",
        "inputs.platform == 'All' &&\n            needs.build-windows.result == 'success' &&\n            needs.build-ubuntu.result == 'success' &&\n            needs.build-macos.result == 'success'",
    ] {
        assert!(
            finalize.contains(expected),
            "missing selected-platform gate: {expected}"
        );
    }

    let notes = workflow
        .split("  generate-release-notes:")
        .nth(1)
        .unwrap()
        .split("  publish-release:")
        .next()
        .unwrap();
    assert!(notes.contains("always() &&"));
    assert!(notes.contains("needs.prepare-release.result == 'success'"));
    assert!(notes.contains("needs.finalize-tag.result == 'success'"));

    let publication = workflow
        .split("  publish-release:")
        .nth(1)
        .unwrap()
        .split("  publish-ghcr:")
        .next()
        .unwrap();
    assert!(publication.contains("always() &&"));
    assert!(publication.contains(
        "(needs.build-ubuntu.result == 'success' || needs.build-ubuntu.result == 'skipped')"
    ));
    assert!(publication.contains(
        "(needs.build-macos.result == 'success' || needs.build-macos.result == 'skipped')"
    ));
    assert!(publication.contains("needs.generate-release-notes.result == 'success'"));

    let summary = workflow.split("  release-summary:").nth(1).unwrap();
    for dependency in [
        "prepare-release",
        "build-windows",
        "build-ubuntu",
        "build-macos",
        "finalize-tag",
        "generate-release-notes",
        "publish-release",
        "publish-ghcr",
        "publish-store",
    ] {
        assert!(summary.contains(&format!("      - {dependency}")));
    }
    assert!(summary.contains("if: ${{ always() }}"));
    assert!(summary.contains("expect_result \"Windows build\" \"$WINDOWS_RESULT\" success"));
    assert!(summary.contains("expect_result \"Ubuntu build\" \"$UBUNTU_RESULT\" skipped"));
    assert!(summary.contains("expect_result \"macOS build\" \"$MACOS_RESULT\" skipped"));
    assert!(summary.contains("expect_result \"Numeric tag\" \"$TAG_RESULT\" success"));
    assert!(
        summary.contains("expect_result \"git-cliff release notes\" \"$NOTES_RESULT\" success")
    );
    assert!(
        summary.contains("expect_result \"GitHub Release\" \"$GITHUB_RELEASE_RESULT\" success")
    );
    assert!(summary.contains("expect_result \"GHCR publication\" \"$GHCR_RESULT\" success"));
    assert!(summary.contains("[ \"$MICROSOFT_STORE\" = true ]"));
    assert!(
        summary.contains("expect_result \"Microsoft Store submission\" \"$STORE_RESULT\" success")
    );
    assert!(
        summary.contains("expect_result \"Microsoft Store submission\" \"$STORE_RESULT\" skipped")
    );
    assert!(summary.contains("exit 1"));
    assert!(!summary.contains(
        "needs.publish-store.result == 'success' || needs.publish-store.result == 'skipped'",
    ));

    fn windows_path_is_complete(store_enabled: bool, notes: &str, store: &str) -> bool {
        let expected_store = if store_enabled { "success" } else { "skipped" };
        notes == "success" && store == expected_store
    }
    assert!(windows_path_is_complete(true, "success", "success"));
    assert!(!windows_path_is_complete(true, "skipped", "success"));
    assert!(!windows_path_is_complete(true, "success", "skipped"));
    assert!(windows_path_is_complete(false, "success", "skipped"));
    assert!(!windows_path_is_complete(false, "skipped", "skipped"));
    assert!(!windows_path_is_complete(false, "success", "success"));
}

#[test]
fn normal_release_atomically_keeps_the_tagged_commit_on_main() {
    let workflow = source(".github/workflows/release-next-patch.yml");
    let finalize = workflow
        .split("  finalize-tag:")
        .nth(1)
        .unwrap()
        .split("  generate-release-notes:")
        .next()
        .unwrap();

    assert!(finalize.contains(
        "name: Publish numeric tag and release commit after successful builds"
    ));
    assert!(finalize.contains("git push --atomic origin"));
    assert!(finalize.contains(
        "\"refs/tags/${RELEASE_VERSION}:refs/tags/${RELEASE_VERSION}\""
    ));
    assert!(finalize.contains("\"$RELEASE_COMMIT:refs/heads/main\""));
    assert!(finalize.contains(
        "git ls-remote origin \"refs/tags/$RELEASE_VERSION^{}\""
    ));
    assert!(finalize.contains("git ls-remote origin \"refs/heads/main\""));
}

#[test]
fn microsoft_store_publication_is_pinned_preserving_and_fail_closed() {
    let normal = source(".github/workflows/release-next-patch.yml");
    let publish = source(".github/workflows/windows-store-publish-update.yml");
    let helper = source("windows/package/Publish-StoreUpdate.ps1");
    let pinned_action =
        "microsoft/microsoft-store-apppublisher@cc9910a8d59f2eb55cbb83df0a3800cf3b5300e0";

    for workflow in [&normal, &publish] {
        assert!(workflow.contains(pinned_action));
        assert!(workflow.contains("version: v0.4.3"));
        for secret in [
            "AZURE_AD_APPLICATION_CLIENT_ID",
            "AZURE_AD_APPLICATION_SECRET",
            "AZURE_AD_TENANT_ID",
            "SELLER_ID",
        ] {
            assert!(workflow.contains(&format!("secrets.{secret}")));
        }
        assert!(!workflow.contains("Write-Host $env:AZURE"));
        assert!(!workflow.contains("Write-Output $env:AZURE"));
        assert!(!workflow.contains("Write-Host $env:SELLER_ID"));
        assert!(!workflow.contains("Write-Output $env:SELLER_ID"));
    }

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
    assert!(helper.contains("Listings"));
    assert!(helper.contains("en-US"));
    assert!(helper.contains("BaseListing"));
    assert!(helper.contains("ReleaseNotes"));
    assert!(helper.contains("every other listing and package field is"));
    assert!(helper.contains("submission status"));
}

#[test]
fn recovery_workflows_never_create_tags_or_duplicate_releases() {
    let package = source(".github/workflows/windows-store-package-resolution.yml");
    let publish = source(".github/workflows/windows-store-publish-update.yml");
    let docker = source(".github/workflows/docker-ghcr-resolution.yml");

    assert!(package.contains("ref: refs/tags/${{ inputs.version }}"));
    assert!(package.contains("name: splined-windows-store-submission"));
    assert!(!package.contains("microsoft-store-apppublisher"));
    assert!(!package.contains("Publish-StoreUpdate.ps1"));
    assert!(!package.contains("gh release create"));
    assert!(!package.contains("git tag -a"));
    assert!(!package.contains("packages: write"));

    assert!(publish.contains("ref: refs/tags/${{ inputs.version }}"));
    assert!(publish.contains("Publish-StoreUpdate.ps1"));
    assert!(!publish.contains("gh release create"));
    assert!(!publish.contains("git tag -a"));
    assert!(!publish.contains("packages: write"));

    assert!(docker.contains("ref: refs/tags/${{ inputs.version }}"));
    assert!(docker.contains("gh release edit \"$RELEASE_VERSION\""));
    assert!(!docker.contains("gh release create"));
    assert!(!docker.contains("git tag -a"));
    assert!(!docker.to_ascii_lowercase().contains("msstore"));
    assert!(!docker.contains("microsoft-store-apppublisher"));
}
