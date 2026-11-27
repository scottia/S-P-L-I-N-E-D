"""Independent source-policy checks for production artwork-write safeguards."""
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]


def source(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


class AudioIntegritySourceContract(unittest.TestCase):
    def test_owner_approved_policy_ids_and_protected_comp_policy(self):
        register = source("docs/POLICY-REGISTER.md")
        index = source("docs/POLICY-INDEX.md")
        agents = source("AGENTS.md")
        for number in range(1, 7):
            policy_id = f"AUDIO-INTEGRITY-{number:03}"
            self.assertIn(policy_id, register)
        self.assertIn("AUDIO-INTEGRITY-001..006", index)
        self.assertIn("AUDIO-INTEGRITY-001..006", agents)
        self.assertIn("MusicBrainz", register)
        self.assertIn("audio files", agents)

    def test_mutagen_mp3_writer_never_downgrades_v24(self):
        writer = source("build/WEBGui/webgui/backend/splined_compilation_authority_policy.py")
        self.assertNotIn("tags.save(path, v2_version=3)", writer)
        self.assertIn("path.write_bytes(header + preserved + audio_bytes)", writer)
        raw_writer = writer.split("def _replace_id3_front(", 1)[1].split("def _replace_flac_front(", 1)[0]
        self.assertNotIn("tags.save(", raw_writer)
        self.assertIn("if version not in (3, 4):", raw_writer)
        self.assertIn("assert_artwork_only(path, staged)", writer)

    def test_comp_has_no_musicbrainz_media_writers(self):
        service = source("build/WEBGui/webgui/services.py")
        start = service.index("    def _persist_comp_track_authority(")
        end = service.index("    def _comp_target(", start)
        implementation = service[start:end]
        self.assertNotIn("audio.save(", implementation)
        self.assertNotIn("os.replace(", implementation)
        self.assertNotIn("TXXX(", implementation)
        self.assertNotIn("UFID(", implementation)
        rust_writer = source("build/Windows/src/embedded_artwork.rs")
        rust_scan = source("build/Windows/src/scan_runtime.rs")
        self.assertNotIn("pub fn replace_musicbrainz_identity(", rust_writer)
        self.assertNotIn("replace_musicbrainz_identity(", rust_scan)
        self.assertIn("persist_track_musicbrainz_identity(", rust_scan)

    def test_staged_writer_protection_cannot_be_removed(self):
        rust = source("build/Windows/src/embedded_artwork.rs")
        self.assertIn("verify_only_front_artwork(track_path, staged)?", rust)
        self.assertIn("write_mp3_front(staged, artwork, mime)", rust)
        guard = source("build/WEBGui/webgui/backend/splined_audio_integrity.py")
        self.assertIn("Non-artwork metadata, ID3 version, or audio stream changed", guard)

    def test_retired_writer_routines_are_not_pr_jobs(self):
        pr = source(".github/workflows/validate.yml")
        self.assertNotIn("  audio-integrity-webgui:", pr)
        self.assertNotIn("  audio-integrity-windows:", pr)
        self.assertNotIn("real artwork writer", pr)
        self.assertIn("Validate WebGUI without retired writer routines", pr)
        for test in (
            "test_splined_compilation_authority_policy.py",
            "test_audio_integrity.py",
            "test_comp_job_scope.py",
            "test_cross_format_cover_overwrite.py",
        ):
            self.assertIn(test, pr)


if __name__ == "__main__":
    unittest.main()
