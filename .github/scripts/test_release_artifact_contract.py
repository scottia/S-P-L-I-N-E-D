"""Unit tests for cross-workflow candidate and build provenance."""

from argparse import Namespace
import hashlib
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / ".github" / "scripts"))

from release_artifact_contract import create_build, verify_build, verify_candidate


class ReleaseArtifactContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.candidate = self.root / "candidate"
        self.candidate.mkdir()
        bundle = self.candidate / "release-source.bundle"
        bundle.write_bytes(b"synthetic bundle")
        self.manifest = {
            "schema": "splined-release-candidate-v1",
            "repository": "scottia/S-P-L-I-N-E-D",
            "workflow_run_id": 21,
            "workflow_run_attempt": 1,
            "main_commit": "1" * 40,
            "source_commit": "2" * 40,
            "candidate_commit": "3" * 40,
            "source_version": "3.0.27",
            "release_version": "3.0.28",
            "validation_commit": "4" * 40,
            "validation_run_id": 19,
            "source_relation": "same tree",
            "target": "ALL",
            "recommended_compile_mode": "PARTIAL UPDATE",
            "temporary_version_stamp": True,
            "existing_release": False,
            "bundle_sha256": hashlib.sha256(bundle.read_bytes()).hexdigest(),
            "created_at": "2026-11-24T00:00:00+00:00",
        }
        (self.candidate / "release-candidate.json").write_text(
            json.dumps(self.manifest), encoding="utf-8"
        )
        (self.candidate / "source-validation.json").write_text(
            json.dumps(
                {
                    "jobs": [
                        {
                            "name": "required",
                            "run_id": 19,
                            "head_sha": "4" * 40,
                            "status": "completed",
                            "conclusion": "success",
                        }
                    ]
                }
            ),
            encoding="utf-8",
        )
        names = (
            "release-source.bundle",
            "release-candidate.json",
            "source-validation.json",
        )
        (self.candidate / "SHA256SUMS-CANDIDATE.txt").write_text(
            "".join(
                f"{hashlib.sha256((self.candidate / name).read_bytes()).hexdigest()}  {name}\n"
                for name in names
            ),
            encoding="ascii",
        )

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_candidate_verification_binds_requested_run_and_bundle(self) -> None:
        result = verify_candidate(self.candidate, 21)
        self.assertEqual(result["candidate_commit"], "3" * 40)
        with self.assertRaisesRegex(ValueError, "Candidate run mismatch"):
            verify_candidate(self.candidate, 22)
        (self.candidate / "release-source.bundle").write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "bundle checksum mismatch"):
            verify_candidate(self.candidate, 21)

    def test_build_manifest_detects_changed_or_missing_artifacts(self) -> None:
        build = self.root / "build"
        (build / "release-assets").mkdir(parents=True)
        shutil.copytree(self.candidate, build / "candidate")
        artifact = build / "release-assets" / "artifact.zip"
        artifact.write_bytes(b"validated artifact")
        create_build(
            Namespace(
                kind="windows",
                root=build,
                candidate_root=self.candidate,
                candidate_run_id=21,
                run_id=22,
                compile_mode="PARTIAL UPDATE",
                validation=[
                    "portable-identity-x64-native-smoke",
                    "unsigned-msix-package-contract",
                    "isolated-development-package",
                    "offline-local-store-review",
                ],
                metric=["compile_seconds=10"],
            )
        )
        manifest = verify_build(build, "windows", 22)
        self.assertEqual(manifest["candidate_commit"], "3" * 40)
        self.assertIn("portable-identity-x64-native-smoke", manifest["validations"])
        artifact.write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            verify_build(build, "windows", 22)

    def test_candidate_rejects_validation_from_another_run_or_commit(self) -> None:
        validation_path = self.candidate / "source-validation.json"
        validation = json.loads(validation_path.read_text(encoding="utf-8"))
        validation["jobs"][0]["run_id"] = 999
        validation_path.write_text(json.dumps(validation), encoding="utf-8")
        self._refresh_candidate_checksums()
        with self.assertRaisesRegex(ValueError, "recorded run"):
            verify_candidate(self.candidate, 21)

        validation["jobs"][0]["run_id"] = 19
        validation["jobs"][0]["head_sha"] = "5" * 40
        validation_path.write_text(json.dumps(validation), encoding="utf-8")
        self._refresh_candidate_checksums()
        with self.assertRaisesRegex(ValueError, "recorded commit"):
            verify_candidate(self.candidate, 21)

    def test_build_rejects_path_traversal_and_candidate_mismatch(self) -> None:
        build = self.root / "build"
        build.mkdir()
        shutil.copytree(self.candidate, build / "candidate")
        (build / "artifact.zip").write_bytes(b"validated artifact")
        create_build(
            Namespace(
                kind="windows",
                root=build,
                candidate_root=self.candidate,
                candidate_run_id=21,
                run_id=22,
                compile_mode="PARTIAL UPDATE",
                validation=[
                    "portable-identity-x64-native-smoke",
                    "unsigned-msix-package-contract",
                    "isolated-development-package",
                    "offline-local-store-review",
                ],
                metric=[],
            )
        )
        manifest_path = build / "windows-build-manifest.json"
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        manifest["files"]["../outside.zip"] = "0" * 64
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "Unsafe artifact path"):
            verify_build(build, "windows", 22)

        manifest["files"].pop("../outside.zip")
        manifest["candidate_commit"] = "5" * 40
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "embedded candidate disagree"):
            verify_build(build, "windows", 22)

    def _refresh_candidate_checksums(self) -> None:
        names = (
            "release-source.bundle",
            "release-candidate.json",
            "source-validation.json",
        )
        (self.candidate / "SHA256SUMS-CANDIDATE.txt").write_text(
            "".join(
                f"{hashlib.sha256((self.candidate / name).read_bytes()).hexdigest()}  {name}\n"
                for name in names
            ),
            encoding="ascii",
        )


if __name__ == "__main__":
    unittest.main()
