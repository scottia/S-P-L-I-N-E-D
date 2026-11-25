"""Non-mutating documentation layout/link checks for current main build roots."""

from __future__ import annotations

import re
import unittest
from pathlib import Path
from urllib.parse import unquote


ROOT = Path(__file__).resolve().parents[2]
DOCS = ROOT / "docs"
MARKDOWN_LINKS = re.compile(r"\]\(([^)]+)\)")
OBSOLETE = {
    "musicbrainz-fallback-workflow.md",
    "media-filter-status-colors.md",
    "cross-client-album-override-visual-contract.md",
    "catalog-index-coordinator-acceptance.md",
    "windows-rust-slint-migration-checklist.md",
}


class MainDocumentationAlignmentTests(unittest.TestCase):
    def test_active_docs_match_supported_build_and_schema(self) -> None:
        for required in (
            "build/Windows/Cargo.toml",
            "build/Windows/src/catalog_index_coordinator.rs",
            "build/WEBGui/Dockerfile",
            "build/WEBGui/webgui/index_coordinator.py",
            "build/WEBGui/webgui/catalog_authority.py",
            "schema/media-state-classification.json",
            "schema/comp-report-policy.json",
            "schema/lifecycle-policy.json",
            "docs/POLICY-REGISTER.md",
            "docs/POLICY-INDEX.md",
            "docs/SCHEMA-VALIDATION.md",
            "docs/lifecycle-policy.md",
        ):
            with self.subTest(path=required):
                self.assertTrue((ROOT / required).is_file(), required)

    def test_historical_or_superseded_docs_not_in_active_tree(self) -> None:
        for name in OBSOLETE:
            self.assertFalse((DOCS / name).exists(), name)
        self.assertFalse((DOCS / "evidence").exists())
        self.assertFalse(list(DOCS.glob("pr-*-acceptance.md")))
        self.assertFalse(list(DOCS.glob("windows-*-2026-*.md")))

    def test_current_markdown_links_resolve(self) -> None:
        broken = []
        for path in DOCS.rglob("*.md"):
            text = path.read_text(encoding="utf-8")
            self.assertNotIn("dev-archive", text, str(path))
            self.assertNotIn("dev-archived", text, str(path))
            for raw in MARKDOWN_LINKS.findall(text):
                url = raw.strip().split("#", 1)[0].split("?", 1)[0]
                if not url or url.startswith(("/", "#")):
                    continue
                if re.match(r"^[a-z][a-z0-9+.-]*:", url, flags=re.IGNORECASE):
                    continue
                target = path.parent / unquote(url)
                if not target.exists():
                    broken.append(f"{path.relative_to(ROOT)} -> {raw}")
        self.assertFalse(broken, "\n".join(broken))

    def test_shared_artist_identity_policy_is_registered_without_json_rewrite(self) -> None:
        for path in (
            ROOT / "AGENTS.md",
            DOCS / "POLICY-REGISTER.md",
            DOCS / "POLICY-INDEX.md",
        ):
            with self.subTest(path=path.relative_to(ROOT)):
                self.assertIn("ART-004", path.read_text(encoding="utf-8"))
        self.assertNotIn(
            "ART-004",
            (ROOT / "schema" / "lifecycle-policy.json").read_text(encoding="utf-8"),
        )


if __name__ == "__main__":
    unittest.main()
