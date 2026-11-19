"""Structured, secret-safe WebGUI audit event helpers."""

from __future__ import annotations

import json
import os
import re
import subprocess
from collections.abc import Mapping, Sequence
from datetime import datetime, timezone
from hashlib import sha256
from pathlib import Path
from typing import Any
from urllib.parse import unquote_plus, urlsplit, urlunsplit

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
SUPPORTED_URL = re.compile(
    r"(?i)\b(?:https?|postgres(?:ql)?)://[^\s<>\"']*?"
    r"(?=(?:\b(?:https?|postgres(?:ql)?)://)|[\s<>\"']|$)"
)
URL_TRAILING_PUNCTUATION = ".,;!?)]}"
ALLOWED_SEVERITIES = {"verbose", "debug", "info", "warning", "error", "critical"}


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def _redact_url(value: str) -> str:
    try:
        parsed = urlsplit(value)
        if parsed.scheme.casefold() not in {"http", "https", "postgres", "postgresql"}:
            return value
        hostname = parsed.hostname or ""
        if ":" in hostname and not hostname.startswith("["):
            hostname = f"[{hostname}]"
        if parsed.port is not None:
            hostname += f":{parsed.port}"
        query_parts = []
        for pair in parsed.query.split("&"):
            if not pair:
                continue
            name, separator, _item = pair.partition("=")
            decoded_name = name
            # Classification must see encoded separators/letters without
            # rewriting safe query fields in the diagnostic text.
            for _ in range(3):
                expanded = unquote_plus(decoded_name)
                if expanded == decoded_name:
                    break
                decoded_name = expanded
            if separator and SECRET_KEYS.search(decoded_name):
                query_parts.append(f"{name}=[REDACTED]")
            else:
                query_parts.append(pair)
        return urlunsplit((parsed.scheme, hostname, parsed.path, "&".join(query_parts), ""))
    except (TypeError, UnicodeError, ValueError):
        # A malformed credential URL must not break Audit or be persisted
        # verbatim.  Retain the scheme so the diagnostic remains meaningful.
        scheme = value.split("://", 1)[0]
        return f"{scheme}://[REDACTED-MALFORMED-URL]"


def _redact_embedded_urls(text: str) -> str:
    def replacement(match: re.Match[str]) -> str:
        candidate = match.group(0)
        suffix = ""
        while candidate and candidate[-1] in URL_TRAILING_PUNCTUATION:
            character = candidate[-1]
            if character == ")" and candidate.count("(") >= candidate.count(")"):
                break
            if character == "]" and candidate.count("[") >= candidate.count("]"):
                break
            if character == "}" and candidate.count("{") >= candidate.count("}"):
                break
            candidate = candidate[:-1]
            suffix = character + suffix
        return _redact_url(candidate) + suffix

    return SUPPORTED_URL.sub(replacement, text)


def sanitize_text(value: Any, *, limit: int = 2000) -> str:
    text = str(value or "")
    text = _redact_embedded_urls(text)
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
