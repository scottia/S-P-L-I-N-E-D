from __future__ import annotations

import json
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
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

    def test_embedded_urls_redact_userinfo_and_encoded_secret_names(self) -> None:
        value = sanitize_text(
            "Provider failed calling "
            "https://synthetic-user:synthetic-password@example.invalid:8443/api/cover"
            "?api%5Fkey=synthetic-api-key&to%6Ben=synthetic-token&id=42 "
            "after timeout"
        )
        for secret in (
            "synthetic-user",
            "synthetic-password",
            "synthetic-api-key",
            "synthetic-token",
        ):
            self.assertNotIn(secret, value)
        self.assertIn("https://example.invalid:8443/api/cover", value)
        self.assertIn("api%5Fkey=[REDACTED]", value)
        self.assertIn("to%6Ben=[REDACTED]", value)
        self.assertIn("id=42", value)
        self.assertTrue(value.startswith("Provider failed calling "))
        self.assertTrue(value.endswith(" after timeout"))

    def test_every_encoded_sensitive_query_name_is_classified(self) -> None:
        encoded_names = (
            "to%6Ben",
            "api%5Fkey",
            "sec%72et",
            "pass%77ord",
            "authoriz%61tion",
            "cred%65ntial",
            "coo%6Bie",
            "c%73rf",
        )
        query = "&".join(
            f"{name}=synthetic-secret-{index}"
            for index, name in enumerate(encoded_names)
        )
        value = sanitize_text(f"https://example.invalid/api?{query}&id=42")
        for index in range(len(encoded_names)):
            self.assertNotIn(f"synthetic-secret-{index}", value)
        for name in encoded_names:
            self.assertIn(f"{name}=[REDACTED]", value)
        self.assertIn("id=42", value)

    def test_whole_http_and_https_urls_preserve_safe_location_fields(self) -> None:
        for scheme in ("http", "https"):
            with self.subTest(scheme=scheme):
                value = sanitize_text(
                    f"{scheme}://whole-user:whole-pass@example.invalid:8443/path"
                    "?token=whole-secret&id=42"
                )
                self.assertEqual(
                    value,
                    f"{scheme}://example.invalid:8443/path?token=[REDACTED]&id=42",
                )

    def test_embedded_postgres_and_postgresql_dsns_are_redacted(self) -> None:
        for scheme in ("postgres", "postgresql"):
            with self.subTest(scheme=scheme):
                value = sanitize_text(
                    f"Database failed at {scheme}://db-user:db-pass@db.invalid:5432/catalog"
                    "?password=db-query-secret&id=42 after timeout"
                )
                for secret in ("db-user", "db-pass", "db-query-secret"):
                    self.assertNotIn(secret, value)
                self.assertIn(
                    f"{scheme}://db.invalid:5432/catalog?password=[REDACTED]&id=42",
                    value,
                )

    def test_multiple_mixed_urls_and_sentence_punctuation_are_redacted(self) -> None:
        value = sanitize_text(
            "First http://one-user:one-pass@one.invalid/a?token=one-secret, "
            "then postgresql://two-user:two-pass@two.invalid:5432/catalog"
            "?authoriz%61tion=two-secret&id=42."
        )
        for secret in (
            "one-user",
            "one-pass",
            "one-secret",
            "two-user",
            "two-pass",
            "two-secret",
        ):
            self.assertNotIn(secret, value)
        self.assertIn("http://one.invalid/a?token=[REDACTED],", value)
        self.assertIn(
            "postgresql://two.invalid:5432/catalog?authoriz%61tion=[REDACTED]&id=42.",
            value,
        )

    def test_encoded_userinfo_and_values_are_redacted_without_losing_safe_context(self) -> None:
        value = sanitize_text(
            "Request https://user%40example.invalid:p%40ss@example.invalid/path"
            "?password=secret%2Dvalue&credential=other%2Dvalue&id=42 failed"
        )
        for secret in (
            "user%40example.invalid",
            "p%40ss",
            "secret%2Dvalue",
            "other%2Dvalue",
        ):
            self.assertNotIn(secret, value)
        self.assertIn("https://example.invalid/path", value)
        self.assertIn("id=42", value)
        self.assertTrue(value.endswith(" failed"))

    def test_nested_embedded_urls_are_sanitized(self) -> None:
        payload = sanitize(
            {
                "errors": [
                    "https://list-user:list-pass@one.invalid/a?csrf=first-secret&id=42",
                    {
                        "message": (
                            "failed at https://two.invalid/b?pass%77ord=second-secret"
                        )
                    },
                ]
            }
        )
        rendered = repr(payload)
        for secret in ("list-user", "list-pass", "first-secret", "second-secret"):
            self.assertNotIn(secret, rendered)
        self.assertIn("id=42", rendered)

    def test_malformed_url_fails_closed_without_breaking_audit_text(self) -> None:
        value = sanitize_text(
            "Provider rejected https://synthetic-user:synthetic-pass@example.invalid:bad/path "
            "after timeout"
        )
        self.assertEqual(
            value,
            "Provider rejected https://[REDACTED-MALFORMED-URL] after timeout",
        )

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
                    {
                        "password": "payload-secret",
                        "safe": "album-1",
                        "provider_error": (
                            "failed at https://fallback-user:fallback-pass@example.invalid/api"
                            "?api%5Fkey=fallback-query-secret&id=42"
                        ),
                    },
                    severity="error",
                )

            self.assertEqual(sequence, -1)
            event = json.loads(services._audit_fallback_path.read_text(encoding="utf-8"))
            self.assertEqual(event["details"]["password"], "[REDACTED]")
            self.assertNotIn("payload-secret", repr(event))
            self.assertNotIn("database-secret", repr(event))
            self.assertNotIn("fallback-user", repr(event))
            self.assertNotIn("fallback-pass", repr(event))
            self.assertNotIn("fallback-query-secret", repr(event))
            self.assertIn("id=42", repr(event))
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

    def test_v3_upgrade_prunes_only_pre_v3_diagnostics(self) -> None:
        root = Path(__file__).parents[1]
        migration = (
            root / "migrations" / "023_prune_pre_v3_diagnostics.sql"
        ).read_text(encoding="utf-8")
        self.assertIn("DELETE FROM web_audit", migration)
        self.assertIn("DELETE FROM web_audit_sessions", migration)
        self.assertNotIn("DELETE FROM albums", migration)
        self.assertNotIn("UPDATE albums", migration)


if __name__ == "__main__":
    unittest.main()
