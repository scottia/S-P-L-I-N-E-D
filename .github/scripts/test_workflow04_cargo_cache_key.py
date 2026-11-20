"""Regression tests for the exact Windows cache-key failure in Workflow 04."""

from __future__ import annotations

from pathlib import Path
import unittest

from normalized_cargo_cache_key import (
    MANIFEST,
    LOCK,
    cache_key,
    normalized_contents,
)

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "04-optimized-build-test.yml"


class Workflow04CargoCacheKeyTests(unittest.TestCase):
    def test_real_cargo_manifest_and_lock_are_supported(self) -> None:
        manifest = MANIFEST.read_bytes().decode("utf-8-sig")
        lock = LOCK.read_bytes().decode("utf-8-sig")
        digest = cache_key(manifest, lock)
        self.assertEqual(len(digest), 64)
        int(digest, 16)
        self.assertIn('version = "<release>"', normalized_contents(manifest, lock))
        self.assertEqual(normalized_contents(manifest, lock).count('version = "<release>"'), 2)

    def test_crlf_and_lf_normalize_equally(self) -> None:
        manifest = '[package]\nname = "splined-windows"\nversion = "3.0.19"\n\n[dependencies]\nserde = "1"\n'
        lock = '[[package]]\nname = "splined-windows"\nversion = "3.0.19"\n\n[[package]]\nname = "serde"\nversion = "1.0.0"\n'
        baseline = cache_key(manifest, lock)
        self.assertEqual(
            baseline,
            cache_key(manifest.replace("\n", "\r\n"), lock.replace("\n", "\r\n")),
        )
        self.assertEqual(
            baseline,
            cache_key(
                manifest.replace("3.0.19", "3.0.20"),
                lock.replace("3.0.19", "3.0.20"),
            ),
        )
        self.assertNotEqual(
            baseline,
            cache_key(manifest.replace('serde = "1"', 'serde = "2"'), lock),
        )

    def test_invalid_package_versions_fail_closed(self) -> None:
        valid_lock = '[[package]]\nname = "splined-windows"\nversion = "3.0.19"\n'
        for bad in (
            '[package]\nname = "splined-windows"\n',
            '[package]\nversion = "1"\nversion = "2"\n',
            '[other]\nversion = "3"\n',
        ):
            with self.subTest(manifest=bad):
                with self.assertRaises(ValueError):
                    cache_key(bad, valid_lock)

    def test_workflow_calls_normalizer_instead_of_broken_inline_replacements(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("python .github/scripts/normalized_cargo_cache_key.py", text)
        self.assertIn("actions/setup-python@", text)
        self.assertIn('id: cargo-key', text)
        self.assertIn('steps.cargo-key.outputs.hash', text)
        self.assertIn('cargo clean --manifest-path build/Windows/Cargo.toml -p splined-windows', text)
        self.assertNotIn('1version = "<release>"', text)
        self.assertNotIn('1<release>2', text)
        self.assertNotIn("$manifestPattern", text)
        self.assertNotIn("$normalManifest", text)


if __name__ == "__main__":
    unittest.main()
