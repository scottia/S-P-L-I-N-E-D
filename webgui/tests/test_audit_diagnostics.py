from __future__ import annotations

import unittest

from webgui.audit import digest, normalize_severity, sanitize, sanitize_text


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
            "authorization=Bearer-value postgresql://owner:db-secret@db.invalid/catalog"
        )
        self.assertNotIn("Bearer-value", value)
        self.assertNotIn("db-secret", value)

    def test_digest_is_stable_after_sanitizing(self) -> None:
        first = digest({"album": "one", "password": "first"})
        second = digest({"password": "second", "album": "one"})
        self.assertEqual(first, second)

    def test_unknown_severity_falls_back_to_info(self) -> None:
        self.assertEqual(normalize_severity("DEBUG"), "debug")
        self.assertEqual(normalize_severity("everything"), "info")


if __name__ == "__main__":
    unittest.main()
