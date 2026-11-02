"""Structured, secret-safe WebGUI audit event helpers."""

from __future__ import annotations

from collections.abc import Mapping, Sequence
from datetime import datetime, timezone
from hashlib import sha256
from pathlib import Path
import json
import os
import re
import subprocess
from typing import Any
from urllib.parse import urlsplit, urlunsplit


SECRET_KEYS = re.compile(
    r"(?:authorization|cookie|password|passwd|secret|token|api[_-]?key|"
    r"access[_-]?key|client[_-]?secret|credential|csrf)",
    re.IGNORECASE,
)
SECRET_TEXT = (
    (re.compile(r"(?i)(bearer\s+)[A-Za-z0-9._~+/=-]+"), r"\1[REDACTED]"),
    (
        re.compile(
            r"(?i)(password|passwd|secret|token|api[_-]?key|access[_-]?key|"
            r"client[_-]?secret|authorization|credential|cookie|csrf)"
            r"(\s*[:=]\s*)([^\s,;&]+)"
        ),
        r"\1\2[REDACTED]",
    ),
    (re.compile(r"(?i)(postgres(?:ql)?://)([^/@\s]+)@"), r"\1[REDACTED]@"),
)
ALLOWED_SEVERITIES = {"verbose", "debug", "info", "warning", "error", "critical"}


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def _redact_url(value: str) -> str:
    try:
        parsed = urlsplit(value)
    except ValueError:
        return value
    if parsed.scheme not in {"http", "https", "postgres", "postgresql"}:
        return value
    hostname = parsed.hostname or ""
    if parsed.port:
        hostname += f":{parsed.port}"
    query = "&".join(
        f"{name}={'[REDACTED]' if SECRET_KEYS.search(name) else item}"
        for pair in parsed.query.split("&")
        if pair
        for name, _, item in [pair.partition("=")]
    )
    return urlunsplit((parsed.scheme, hostname, parsed.path, query, ""))


def sanitize_text(value: Any, *, limit: int = 2000) -> str:
    text = str(value or "")
    if "://" in text and not any(character.isspace() for character in text):
        text = _redact_url(text)
    for pattern, replacement in SECRET_TEXT:
        text = pattern.sub(replacement, text)
    return text[:limit]


def sanitize(value: Any, *, depth: int = 0) -> Any:
    """Return a bounded JSON-compatible value with nested secrets removed."""
    if depth > 8:
        return "[TRUNCATED]"
    if value is None or isinstance(value, (bool, int, float)):
        return value
    if isinstance(value, bytes):
        return {"bytes": len(value), "sha256": sha256(value).hexdigest()}
    if isinstance(value, Mapping):
        output: dict[str, Any] = {}
        for index, (raw_key, item) in enumerate(value.items()):
            if index >= 100:
                output["_truncated"] = True
                break
            key = sanitize_text(raw_key, limit=100)
            output[key] = "[REDACTED]" if SECRET_KEYS.search(key) else sanitize(item, depth=depth + 1)
        return output
    if isinstance(value, Sequence) and not isinstance(value, (str, bytearray)):
        items = list(value)
        return [sanitize(item, depth=depth + 1) for item in items[:100]] + (
            ["[TRUNCATED]"] if len(items) > 100 else []
        )
    return sanitize_text(value)


def digest(value: Any) -> str:
    body = json.dumps(sanitize(value), sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return sha256(body.encode("utf-8")).hexdigest()


def normalize_severity(value: Any) -> str:
    severity = str(value or "info").strip().casefold()
    return severity if severity in ALLOWED_SEVERITIES else "info"


def application_revision(repository: Path | None = None) -> str:
    """Resolve an immutable build revision without making it a startup dependency."""
    configured = str(
        os.environ.get("SPLINED_REVISION")
        or os.environ.get("GITHUB_SHA")
        or ""
    ).strip()
    if re.fullmatch(r"[0-9a-fA-F]{7,64}", configured):
        return configured.casefold()
    root = (repository or Path(__file__).resolve().parents[1]).resolve()
    try:
        result = subprocess.run(
            ["git", "-C", str(root), "rev-parse", "HEAD"],
            check=True,
            capture_output=True,
            text=True,
            timeout=3,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
        revision = result.stdout.strip()
        if re.fullmatch(r"[0-9a-fA-F]{40,64}", revision):
            return revision.casefold()
    except (OSError, subprocess.SubprocessError):
        pass
    return "unknown"
