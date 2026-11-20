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

    def test_album_fixture_remains_cross_runtime(self):
        fixture = ROOT / "fixtures/cross-runtime-album-policy.json"
        self.assertTrue(fixture.is_file())
        windows = (ROOT / "build/Windows/src/compilation.rs").read_text(encoding="utf-8")
        webgui = (ROOT / "build/WEBGui/webgui/tests/test_cross_runtime_album_policy.py").read_text(encoding="utf-8")
        self.assertIn("../../../fixtures/cross-runtime-album-policy.json", windows)
        self.assertIn('"fixtures" / "cross-runtime-album-policy.json"', webgui)

    def test_shared_schema_stays_single_source(self):
        for name in ("media.sql", "settings-ui.json", "media-state-classification.json", "comp-report-policy.json"):
            self.assertTrue((ROOT / "schema" / name).is_file())
            self.assertFalse((ROOT / "build/Windows/schema" / name).exists())
            self.assertFalse((ROOT / "build/WEBGui/schema" / name).exists())


if __name__ == "__main__":
    unittest.main()
