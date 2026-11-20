"""Workflow 01 is the only release root for the two supported application build targets."""

from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class BuildLayoutTests(unittest.TestCase):
    def test_only_two_build_target_directories(self):
        root = ROOT / "build"
        self.assertEqual(sorted(x.name for x in root.iterdir()), ["WEBGui", "Windows"])
        self.assertTrue((root / "Windows/Cargo.toml").is_file())
        self.assertTrue((root / "Windows/src/main.rs").is_file())
        self.assertTrue((root / "Windows/ui/app.slint").is_file())
        self.assertTrue((root / "WEBGui/Dockerfile").is_file())
        self.assertTrue((root / "WEBGui/docker/compose.yaml").is_file())
        self.assertTrue((root / "WEBGui/docker/.env.example").is_file())
        self.assertTrue((root / "WEBGui/docker/config.example.toml").is_file())
        self.assertFalse((ROOT / "docker").exists())
        self.assertTrue((root / "WEBGui/webgui/__main__.py").is_file())
        self.assertTrue((root / "WEBGui/webgui/static/index.html").is_file())

    def test_build_folders_have_no_markdown_docs(self):
        files = list((ROOT / "build").rglob("*.md"))
        self.assertEqual(files, [], str(files))
        self.assertFalse((ROOT / "src").exists())
        self.assertFalse((ROOT / "Cargo.toml").exists())
        self.assertFalse((ROOT / "webgui").exists())
        self.assertFalse((ROOT / "windows").exists())

    def test_ghcr_uses_exact_webgui_runtime_inputs(self):
        dockerfile = (ROOT / "build/WEBGui/Dockerfile").read_text(encoding="utf-8")
        dockerignore = (ROOT / ".dockerignore").read_text(encoding="utf-8")
        self.assertIn("COPY build/WEBGui/webgui /app/webgui", dockerfile)
        self.assertIn("COPY schema/media.sql schema/settings-ui.json", dockerfile)
        self.assertIn("CMD [\"python\", \"-m\", \"webgui\"]", dockerfile)
        self.assertIn("!build/WEBGui/webgui/**", dockerignore)
        self.assertIn("!build/WEBGui/docker/config.example.toml", dockerignore)
        self.assertIn("COPY build/WEBGui/docker/config.example.toml /app/build/WEBGui/docker/config.example.toml", dockerfile)
        compose = (ROOT / "build/WEBGui/docker/compose.yaml").read_text(encoding="utf-8")
        self.assertIn("context: ../../..", compose)
        self.assertIn("dockerfile: build/WEBGui/Dockerfile", compose)
        self.assertIn("build/WEBGui/docker/config.example.toml", (ROOT / "build/WEBGui/webgui/runtime.py").read_text(encoding="utf-8"))
        self.assertIn("build/WEBGui/webgui/tests/", dockerignore)

    def test_workflow_one_validates_and_stamps_relocated_sources(self):
        workflow = (ROOT / ".github/workflows/01-full-release.yml").read_text(encoding="utf-8")
        self.assertIn('Path("build/Windows/Cargo.toml")', workflow)
        self.assertIn('Path("build/WEBGui/webgui/backend/splined.py")', workflow)
        self.assertIn("cargo build --locked --release --manifest-path build/Windows/Cargo.toml", workflow)
        self.assertIn("--file build/WEBGui/Dockerfile", workflow)
        self.assertIn("discover -s build/WEBGui/webgui/tests", workflow)
        self.assertIn("build/WEBGui:build/WEBGui/webgui/backend", workflow)
        self.assertIn("cp -R schema docs release-package/", workflow)
        self.assertNotIn("cp -R schema docker docs release-package/", workflow)

    def test_configuration_examples_are_runtime_owned(self):
        self.assertFalse((ROOT / "config.example.toml").exists())
        windows_example = ROOT / "build/Windows/config.example.toml"
        webgui_example = ROOT / "build/WEBGui/docker/config.example.toml"
        self.assertTrue(windows_example.is_file())
        self.assertTrue(webgui_example.is_file())
        windows_source = (ROOT / "build/Windows/src/config.rs").read_text(encoding="utf-8")
        self.assertIn('include_str!("../config.example.toml")', windows_source)
        webgui_source = (ROOT / "build/WEBGui/webgui/runtime.py").read_text(encoding="utf-8")
        self.assertIn("build/WEBGui/docker/config.example.toml", webgui_source)
        workflow = (ROOT / ".github/workflows/01-full-release.yml").read_text(encoding="utf-8")
        self.assertIn("cp -R build/WEBGui release-package/build/", workflow)
        self.assertIn("cp README.md LICENSE PRIVACY.md SECURITY.md SUPPORT.md release-package/", workflow)
        self.assertNotIn("cp config.example.toml ", workflow)

    def test_cliff_config_and_release_consumers_are_colocated(self):
        self.assertFalse((ROOT / "cliff.toml").exists())
        classification = ROOT / "release/cliff.toml"
        self.assertTrue(classification.is_file())
        self.assertIn('tag_pattern = ', classification.read_text(encoding="utf-8"))
        workflow_one = (ROOT / ".github/workflows/01-full-release.yml").read_text(encoding="utf-8")
        self.assertEqual(workflow_one.count("-ConfigPath release/cliff.toml"), 2)
        workflow_two = (ROOT / ".github/workflows/02-store-recovery.yml").read_text(encoding="utf-8")
        self.assertIn("            release/cliff.toml", workflow_two)
        self.assertIn("-ConfigPath .splined-release-engine/release/cliff.toml", workflow_two)
        github_fixture = (ROOT / "release/Test-ReleaseNoteClassification.ps1").read_text(encoding="utf-8")
        self.assertIn('Join-Path $PSScriptRoot "cliff.toml"', github_fixture)
        store_fixture = (ROOT / "release/Test-StoreBaseline.ps1").read_text(encoding="utf-8")
        self.assertIn('Join-Path $PSScriptRoot "cliff.toml"', store_fixture)
        self.assertIn('$audit.classification -cne "release/cliff.toml"', store_fixture)
        store_renderer = (ROOT / "release/Generate-StoreHighlights.ps1").read_text(encoding="utf-8")
        self.assertIn('classification = "release/cliff.toml"', store_renderer)
        release_docs = (ROOT / "docs/release-automation.md").read_text(encoding="utf-8")
        self.assertIn("../release/cliff.toml", release_docs)

    def test_no_legacy_root_docker_config_references_in_python(self):
        # A missing relocated example can otherwise fail unrelated tests at
        # import time before the Docker image build is reached.
        legacy = 'REPOSITORY / "docker" / "config.example.toml"'
        stale = []
        for source in (ROOT / "build/WEBGui").rglob("*.py"):
            if legacy in source.read_text(encoding="utf-8"):
                stale.append(str(source.relative_to(ROOT)))
        self.assertEqual(stale, [])

    def test_shared_schema_stays_single_source(self):
        for name in ("media.sql", "settings-ui.json", "media-state-classification.json", "comp-report-policy.json"):
            self.assertTrue((ROOT / "schema" / name).is_file())
            self.assertFalse((ROOT / "build/Windows/schema" / name).exists())
            self.assertFalse((ROOT / "build/WEBGui/schema" / name).exists())


if __name__ == "__main__":
    unittest.main()
