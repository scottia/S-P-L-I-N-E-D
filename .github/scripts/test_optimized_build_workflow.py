"""Nonmutating checks: Workflow 04 mirrors Workflow 01 with opt-in publication."""

from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/04-optimized-build-test.yml"
RELEASE = ROOT / ".github/workflows/01-full-release.yml"

PUBLISH_JOBS = ("finalize-tag", "publish-release", "publish-ghcr", "publish-store")
MIRRORED_JOBS = (
    "prepare-release", "build-windows", "build-webgui", "finalize-tag",
    "generate-release-notes", "publish-release", "publish-ghcr",
    "publish-store", "release-summary",
)
RELEASE_CHOICES = ("Stop - no release", "Create release now")


def job(source: str, name: str) -> str:
    matches = list(re.finditer(r"(?m)^  " + re.escape(name) + r":\s*$", source))
    if len(matches) != 1:
        raise AssertionError(f"Expected exactly one job {name}, got {len(matches)}")
    start = matches[0].start()
    following = re.search(r"(?m)^  [a-z][a-z0-9-]*:\s*$", source[matches[0].end():])
    end = matches[0].end() + following.start() if following else len(source)
    return source[start:end]


class OptimizedFullReleaseMirrorTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflow = WORKFLOW.read_text(encoding="utf-8")
        cls.release = RELEASE.read_text(encoding="utf-8")

    def test_same_dispatch_inputs_and_release_confirmation_last(self):
        def inputs(source):
            form = source.split("  workflow_dispatch:\n", 1)[1].split("\npermissions:", 1)[0]
            return re.findall(r"(?m)^      ([a-z][a-z0-9_]+):\s*$", form)

        original = inputs(self.release)
        mirrored = inputs(self.workflow)
        self.assertEqual(mirrored, original)
        self.assertEqual(mirrored[-1], "release_confirmation")
        self.assertEqual(mirrored, [
            "release_version", "platform", "extended_windows_validation",
            "store_submit_update", "store_submit_draft", "release_confirmation",
        ])
        for choice in RELEASE_CHOICES:
            self.assertIn(f'- "{choice}"', self.workflow)
            self.assertIn(f'- "{choice}"', self.release)
        for platform in ("All", "Windows", "WebGUI / Python"):
            self.assertIn(f'- "{platform}"', self.workflow)
        self.assertNotIn("benchmark_mode:", self.workflow)

    def test_all_workflow_one_jobs_and_full_publish_graph_remain(self):
        self.assertTrue(self.workflow.startswith("name: 04. Optimized Full Release"))
        for name in MIRRORED_JOBS:
            with self.subTest(job=name):
                job(self.workflow, name)
                job(self.release, name)
        self.assertIn("  webgui-tests:\n", self.workflow)
        self.assertIn("  qa-artifacts:\n", self.workflow)
        self.assertIn("git bundle create release-source.bundle", self.workflow)
        self.assertIn("git push --atomic origin", job(self.workflow, "finalize-tag"))
        self.assertIn("gh release create", job(self.workflow, "publish-release"))
        self.assertIn("docker push", job(self.workflow, "publish-ghcr"))
        self.assertIn("Publish-StoreUpdate.ps1", job(self.workflow, "publish-store"))
        self.assertIn("Generate-StoreHighlights.ps1", job(self.workflow, "publish-store"))

    def test_no_runs_real_versioned_build_without_remote_release(self):
        preflight = job(self.workflow, "prepare-release")
        self.assertIn('if [ "$RELEASE_CONFIRMATION" != "Create release now" ] &&', preflight)
        self.assertIn('if [ "$RELEASE_CONFIRMATION" = "Stop - no release" ]; then', preflight)
        self.assertIn("git commit -m", preflight)
        self.assertIn("release-source.bundle", preflight)
        self.assertIn("temporary local QA source commit", preflight)
        self.assertIn("Validate canonical policies and indexer contracts", preflight)
        # No can validate a Draft PR branch before merging; a real release
        # remains restricted to main and uses the exact production source.
        self.assertIn("inputs.release_confirmation == 'Create release now'", preflight)
        self.assertIn("Require main branch dispatch", preflight)
        self.assertIn("inputs.release_confirmation == 'Create release now' && 'main' || github.sha", preflight)
        for job_name in ("build-windows", "build-webgui", "webgui-tests", "qa-ghcr-build"):
            self.assertIn(
                "inputs.release_confirmation == 'Create release now' && 'main' || github.sha",
                job(self.workflow, job_name),
            )

        for publisher in PUBLISH_JOBS:
            with self.subTest(publisher=publisher):
                self.assertIn("inputs.release_confirmation == 'Create release now'",
                              job(self.workflow, publisher).split("    steps:\n", 1)[0])

        notes = job(self.workflow, "generate-release-notes")
        self.assertIn("needs.finalize-tag.result == 'skipped'", notes)
        self.assertIn("Create temporary local tag for release-note validation (never pushed)", notes)
        self.assertIn("release/Generate-ReleaseNotes.ps1", notes)
        # Local temporary Git tags for note simulation are not remote tags or releases.
        local_tag_step = notes.split("Create temporary local tag", 1)[1].split(
            "- name: Install and verify", 1
        )[0]
        self.assertIn('git tag "$RELEASE_VERSION" "$RELEASE_COMMIT"', local_tag_step)
        self.assertNotIn("git push", local_tag_step)
        self.assertNotIn("GH_TOKEN:", local_tag_step)

        qa = job(self.workflow, "qa-artifacts")
        self.assertIn("inputs.release_confirmation == 'Stop - no release'", qa)
        self.assertIn("sha256sum -c SHA256SUMS.txt", qa)
        self.assertIn("QA-NOT-A-RELEASE.txt", qa)
        self.assertNotIn("gh release create", qa)
        self.assertNotIn("git push", qa)

        ghcr_qa = job(self.workflow, "qa-ghcr-build")
        self.assertIn("inputs.release_confirmation == 'Stop - no release'", ghcr_qa)
        self.assertIn("docker build --pull", ghcr_qa)
        self.assertIn("org.opencontainers.image.revision", ghcr_qa)
        self.assertNotIn("docker login", ghcr_qa)
        self.assertNotIn("docker push", ghcr_qa)

    def test_yes_mode_preserves_the_publication_and_store_recovery_paths(self):
        self.assertIn("splined-release-next-patch", self.workflow)
        self.assertIn("inputs.release_confirmation == 'Create release now'",
                      job(self.workflow, "finalize-tag"))
        for name in ("publish-release", "publish-ghcr", "publish-store"):
            self.assertIn("inputs.release_confirmation == 'Create release now'",
                          job(self.workflow, name))
        for token in (
            "release/Install-GitCliff.ps1",
            "release/Test-StoreBaseline.ps1",
            "release/Resolve-StoreBaseline.ps1",
            "release/Generate-StoreHighlights.ps1",
            "build/Windows/package/Publish-StoreUpdate.ps1",
            "store-submission/STORE-SUBMISSION.json",
        ):
            self.assertIn(token, self.workflow)
        self.assertIn("Test-DevelopmentPackage.ps1", job(self.workflow, "build-windows"))
        self.assertIn("Build-StorePackage.ps1", job(self.workflow, "build-windows"))

    def test_compiled_cache_reuses_dependencies_not_final_binary(self):
        windows = job(self.workflow, "build-windows")
        self.assertIn("Calculate Windows dependency cache key", windows)
        self.assertIn("Restore cached compiled Windows release dependencies", windows)
        self.assertIn("build/Windows/target/x86_64-pc-windows-msvc/release/deps", windows)
        self.assertIn("build/Windows/target/release/.fingerprint", windows)
        self.assertIn("cargo clean --manifest-path build/Windows/Cargo.toml -p splined-windows --release --target", windows)
        self.assertIn("cargo build --locked --release --manifest-path build/Windows/Cargo.toml --target", windows)
        self.assertNotIn("Remove-Item -LiteralPath $releaseRoot -Recurse -Force", windows)
        self.assertNotIn("1<release>2", windows)
        self.assertIn("SPLINED_BUILD_COMMIT: ${{ needs.prepare-release.outputs.commit }}", windows)
        self.assertIn("--write-build-info", windows)
        self.assertIn("--native-smoke", windows)
        self.assertIn("Compress-Archive", windows)
        self.assertIn("Build-StorePackage.ps1", windows)
        self.assertIn("Development-sign, install, activate, native-smoke, and uninstall", windows)

    def test_validation_only_store_fixture_installs_pinned_git_cliff_first(self):
        windows = job(self.workflow, "build-windows")
        install = "Install and verify git-cliff 2.14.2 for Store fixture validation"
        fixture = "Validate Store published-baseline logic against synthetic regression fixtures"
        self.assertEqual(windows.count(install), 1)
        self.assertEqual(windows.count(fixture), 1)
        self.assertLess(windows.index(install), windows.index(fixture))
        installer_block = windows.split(install, 1)[1].split(fixture, 1)[0]
        self.assertIn("release/Install-GitCliff.ps1", installer_block)
        self.assertIn("git-cliff 2.14.2", installer_block)
        self.assertIn("inputs.release_confirmation == 'Stop - no release'", installer_block)

    def test_webgui_full_test_and_docker_checks_run_in_parallel(self):
        tests = job(self.workflow, "webgui-tests")
        docker = job(self.workflow, "build-webgui")
        for part in (tests, docker):
            self.assertIn("    needs: prepare-release", part)
            self.assertIn("postgres:17", part)
            self.assertIn("release-source.bundle", part)
        self.assertIn("python -m unittest discover -s build/WEBGui/webgui/tests -v", tests)
        self.assertIn("node --check build/WEBGui/webgui/static/app.js", tests)
        self.assertIn("docker build --file build/WEBGui/Dockerfile", docker)
        self.assertIn("curl --fail --silent", docker)
        self.assertIn("tar -czf release-assets/splined-webgui-python.tar.gz", docker)
        self.assertNotIn("python -m unittest discover", docker)
        self.assertIn("needs.webgui-tests.result == 'success'", job(self.workflow, "finalize-tag"))
        self.assertIn("needs.webgui-tests.result == 'success'", job(self.workflow, "finalize-tag"))

    def test_strict_summary_fails_closed_for_both_modes(self):
        summary = job(self.workflow, "release-summary")
        self.assertIn("inputs.release_confirmation == 'Create release now'", summary)
        self.assertIn("inputs.release_confirmation == 'Stop - no release'", summary)
        for step in ("QA_RESULT", "QA_GHCR_RESULT", "GHCR_RESULT", "STORE_RESULT", "WEBGUI_TEST_RESULT"):
            self.assertIn(step, summary)
        self.assertIn('check "GitHub Release" "$RELEASE_RESULT" skipped', summary)
        self.assertIn('check "QA-only checksum and artifacts" "$QA_RESULT" success', summary)
        self.assertIn('check "Local release-channel GHCR image build" "$QA_GHCR_RESULT" success', summary)
        self.assertIn('check "WebGUI tests" "$WEBGUI_TEST_RESULT" success', summary)

    def test_workflow_one_remains_the_unmodified_reference(self):
        self.assertIn("name: 01. Full Release - Build Selected Target and Publish", self.release)
        self.assertIn("if [ \"$RELEASE_CONFIRMATION\" != \"Create release now\" ]; then", self.release)
        self.assertIn("    name: Prepare next release commit", self.release)
        self.assertIn("git bundle create release-source.bundle", self.release)
        self.assertNotIn("splined-optimized-compiled-v1", self.release)


if __name__ == "__main__":
    unittest.main()
