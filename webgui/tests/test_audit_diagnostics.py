from __future__ import annotations

from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import json
import unittest
from unittest.mock import patch

from webgui.audit import (
    application_revision,
    digest,
    normalize_severity,
    sanitize,
    sanitize_text,
)
from webgui.services import Services


class AuditDiagnosticsTests(unittest.TestCase):
    def test_nested_secrets_and_credential_urls_are_redacted(self) -> None:
        payload = sanitize(
            {
                "authorization": "Bearer top-secret",
                "nested": {
                    "api_key": "key-secret",
                    "url": "https://listener:password@example.invalid/rest?token=abc&id=42",
                },
                "safe": "album-42",
            }
        )
        self.assertEqual(payload["authorization"], "[REDACTED]")
        self.assertEqual(payload["nested"]["api_key"], "[REDACTED]")
        self.assertNotIn("password", repr(payload))
        self.assertNotIn("abc", repr(payload))
        self.assertIn("album-42", repr(payload))

    def test_exception_text_and_postgres_credentials_are_redacted(self) -> None:
        value = sanitize_text(
            "authorization=Bearer-value credential=provider-secret "
            "cookie=session-secret csrf=request-secret "
            "postgresql://owner:db-secret@db.invalid/catalog"
        )
        self.assertNotIn("Bearer-value", value)
        self.assertNotIn("provider-secret", value)
        self.assertNotIn("session-secret", value)
        self.assertNotIn("request-secret", value)
        self.assertNotIn("db-secret", value)

    def test_digest_is_stable_after_sanitizing(self) -> None:
        first = digest({"album": "one", "password": "first"})
        second = digest({"password": "second", "album": "one"})
        self.assertEqual(first, second)

    def test_unknown_severity_falls_back_to_info(self) -> None:
        self.assertEqual(normalize_severity("DEBUG"), "debug")
        self.assertEqual(normalize_severity("everything"), "info")

    def test_frontend_action_correlation_is_carried_into_background_jobs(self) -> None:
        root = Path(__file__).parents[1]
        server = (root / "server.py").read_text(encoding="utf-8")
        jobs = (root / "jobs.py").read_text(encoding="utf-8")
        self.assertIn('payload["_audit"] = audit_context', server)
        self.assertIn('action_id=str(correlation.get("action_id") or "")', jobs)
        self.assertIn('trace_id=str(correlation.get("trace_id") or "")', jobs)

    def test_configured_revision_is_normalized(self) -> None:
        with patch.dict("os.environ", {"SPLINED_REVISION": "A" * 40}):
            self.assertEqual(application_revision(), "a" * 40)

    def test_failed_postgres_audit_is_redacted_and_retained_in_bounded_jsonl(self) -> None:
        class BrokenDatabase:
            def connect(self):
                raise OSError("postgres unavailable password=database-secret")

        with TemporaryDirectory() as directory:
            root = Path(directory)
            services = Services.__new__(Services)
            services.runtime = SimpleNamespace(
                database=BrokenDatabase(),
                cfg={"logging": {"retention_days": 14}},
                log_path=root / "runtime.log",
                cache=root,
            )
            services._audit_events_written = 0
            services.audit_session_id = "session-test"
            services.application_revision = "b" * 40
            services._audit_lock = __import__("threading").RLock()
            services._audit_fallback_counter = 0
            services._audit_fallback_path = root / "web-audit-fallback.jsonl"

            with patch("webgui.services.core.runtime_log"):
                sequence = services.audit(
                    "failed-action",
                    "test",
                    {"password": "payload-secret", "safe": "album-1"},
                    severity="error",
                )

            self.assertEqual(sequence, -1)
            event = json.loads(services._audit_fallback_path.read_text(encoding="utf-8"))
            self.assertEqual(event["details"]["password"], "[REDACTED]")
            self.assertNotIn("payload-secret", repr(event))
            self.assertNotIn("database-secret", repr(event))
            self.assertEqual(event["application_revision"], "b" * 40)
            self.assertEqual(event["session_id"], "session-test")

    def test_audit_recovery_migration_and_truthful_export_contract_are_present(self) -> None:
        root = Path(__file__).parents[1]
        migration = (root / "migrations" / "011_audit_sessions_recovery.sql").read_text(encoding="utf-8")
        server = (root / "server.py").read_text(encoding="utf-8")
        for field in ("application_version", "application_revision", "session_id", "recovered_from_fallback", "fallback_id"):
            self.assertIn(field, migration)
        self.assertIn('"selected_records_truncated": truncated', server)
        self.assertIn('"complete_for_retained_window"', server)
        self.assertIn('"fallback_records_pending_recovery"', (root / "services.py").read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
