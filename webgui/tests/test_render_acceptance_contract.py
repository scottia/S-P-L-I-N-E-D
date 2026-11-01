"""Checks the disposable rendered-acceptance harness and audit example."""

from __future__ import annotations

from pathlib import Path
import json
import unittest

from fastapi.testclient import TestClient

from webgui.tests.render_acceptance_app import app


class RenderAcceptanceContractTests(unittest.TestCase):
    def test_harness_exposes_version_collapsed_ui_and_provider_similarity(self):
        with TestClient(app) as client:
            self.assertEqual(client.get("/api/status").json()["version"], "2.0.18")
            document = client.get("/").text
            self.assertIn('id="aboutButton"', document)
            self.assertIn('id="artistInfoRows" class="artist-info-rows" aria-label="Selected Artist information" hidden', document)
            self.assertIn('id="mediaFilter" class="filter-body" hidden', document)
            metadata = client.get(
                "/api/artist/metadata", params={"id": "artist-acceptance"}
            ).json()
            self.assertEqual(metadata["source"], "Navidrome")
            self.assertEqual(metadata["similar"][0]["name"], "Signal Coast")

    def test_harness_reconciliation_schedule_is_disabled_and_conflict_visible(self):
        with TestClient(app) as client:
            result = client.get("/api/reconciliation").json()
            self.assertFalse(result["settings"]["enabled"])
            self.assertEqual(result["pending_safe"], 0)
            self.assertEqual(result["differences"][0]["resolution"], "conflict")

    def test_audit_example_is_valid_chronological_json_without_raw_secret(self):
        sample = json.loads(
            (
                Path(__file__).resolve().parents[2]
                / "docs"
                / "pr-131-audit-sample.json"
            ).read_text(encoding="utf-8")
        )
        self.assertEqual(sample["format"], "splined-web-audit-v1")
        sequences = [event["sequence"] for event in sample["events"]]
        self.assertEqual(sequences, sorted(sequences))
        self.assertNotIn("provider-secret", json.dumps(sample))


if __name__ == "__main__":
    unittest.main()
