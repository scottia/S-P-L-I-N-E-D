"""Pre-release WebGUI artifact compatibility and release isolation."""
import json
from pathlib import Path
import tempfile
import unittest
import zipfile

from release_artifact_contract import verify_build
from webgui_evaluation_artifact import (
    CHECKSUMS, IMAGE, MANIFEST, SOURCE, VALIDATIONS, create_manifest, digest
)


ENV = {
    "GITHUB_REPOSITORY": "owner/repository",
    "GITHUB_RUN_ID": "100",
    "GITHUB_RUN_ATTEMPT": "1",
    "GITHUB_SHA": "a" * 40,
    "SOURCE_COMMIT": "b" * 40,
    "CANDIDATE_COMMIT": "c" * 40,
    "SOURCE_VERSION": "3.0.32",
    "CANDIDATE_ID": "3.0.31-rc.8",
    "OFFICIAL_RELEASE_VERSION": "3.0.31",
    "SOURCE_MODE": "pr-evaluation",
    "EVALUATION_PR": "356",
}


class WebguiEvaluationArtifactTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for name in (SOURCE, IMAGE):
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(name.encode())

    def test_compatible_development_download_not_release_candidate(self):
        manifest = create_manifest(self.root, ENV)
        self.assertEqual(manifest["schema"], "splined-release-build-v1")
        self.assertEqual(manifest["kind"], "webgui")
        self.assertEqual(manifest["release_version"], "3.0.32")
        self.assertEqual(manifest["candidate_run_id"], manifest["workflow_run_id"])
        self.assertEqual(manifest["candidate_id"], "3.0.31-rc.8")
        self.assertEqual(manifest["official_release_version_reference"], "3.0.31")
        self.assertEqual(manifest["source_commit"], ENV["SOURCE_COMMIT"])
        self.assertEqual(manifest["candidate_commit"], ENV["CANDIDATE_COMMIT"])
        self.assertEqual(manifest["validations"], VALIDATIONS)
        self.assertFalse(manifest["release_eligible"])
        self.assertTrue(manifest["evaluation_only"])
        self.assertFalse((self.root / "candidate").exists())
        for name in (SOURCE, IMAGE):
            self.assertEqual(manifest["files"][name], digest(self.root / name))
            self.assertIn(f"{digest(self.root / name)}  {name}", (self.root / CHECKSUMS).read_text())
        with zipfile.ZipFile(self.root / "test.zip", "w") as z:
            for name in (SOURCE, IMAGE, MANIFEST, CHECKSUMS):
                z.write(self.root / name, name)
        with zipfile.ZipFile(self.root / "test.zip") as z:
            self.assertEqual(json.loads(z.read(MANIFEST)), manifest)
            self.assertEqual(z.namelist().count(SOURCE), 1)
        with self.assertRaises((ValueError, FileNotFoundError)):
            verify_build(self.root, "webgui", 100)

    def test_retained_committed_source_validation_provenance(self):
        env = dict(ENV, SOURCE_VALIDATION_RUN_ID="99",
                   SOURCE_VALIDATION_RUN_ATTEMPT="2", SOURCE_VALIDATION_BASE_SHA="d" * 40)
        manifest = create_manifest(self.root, env)
        self.assertEqual(manifest["source_validation"], {
            "kind": "reused-committed-source-pr-validation",
            "run_id": 99, "run_attempt": 2, "source_commit": ENV["SOURCE_COMMIT"],
            "base_commit": "d" * 40, "runner_stamp_verified": True,
        })
        self.assertFalse(manifest["release_eligible"])

    def test_missing_asset_rejected(self):
        (self.root / IMAGE).unlink()
        with self.assertRaises(ValueError):
            create_manifest(self.root, ENV)

    def test_reject_release_candidate_bundle(self):
        (self.root / "candidate").mkdir()
        with self.assertRaises(ValueError):
            create_manifest(self.root, ENV)

    def test_dev_identity_is_unstamped(self):
        env = dict(ENV, SOURCE_MODE="dev", EVALUATION_PR="", CANDIDATE_COMMIT=ENV["SOURCE_COMMIT"])
        manifest = create_manifest(self.root, env)
        self.assertEqual(manifest["source_commit"], manifest["candidate_commit"])
        self.assertIsNone(manifest["evaluation_pr"])
        self.assertFalse(manifest["release_eligible"])

    def test_reject_bad_evaluation_identity(self):
        for updates in (
            {"CANDIDATE_COMMIT": ENV["SOURCE_COMMIT"]},
            {"EVALUATION_PR": ""},
            {"SOURCE_VERSION": "3.0.32-dev"},
            {"CANDIDATE_ID": "3.0.31"},
            {"OFFICIAL_RELEASE_VERSION": "3.0.31-rc.8"},
            {"SOURCE_MODE": "release"},
        ):
            with self.subTest(updates=updates), self.assertRaises(ValueError):
                create_manifest(self.root, dict(ENV, **updates))


if __name__ == "__main__":
    unittest.main()
