"""Notification-only semantic release checking for Python/portable clients."""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Any

import requests


RELEASES_API = "https://api.github.com/repos/scottia/S-P-L-I-N-E-D/releases"
NUMERIC_VERSION = re.compile(r"^v?(\d+)\.(\d+)\.(\d+)$", re.IGNORECASE)


def semantic_version(value: str) -> tuple[int, int, int]:
    match = NUMERIC_VERSION.fullmatch(str(value).strip())
    if match is None:
        raise ValueError(f"Not a numeric SPLINED release version: {value!r}")
    return tuple(int(part) for part in match.groups())  # type: ignore[return-value]


def newer_version(installed: str, available: str) -> bool:
    """Commits are intentionally irrelevant; numeric SemVer is authority."""

    return semantic_version(available) > semantic_version(installed)


@dataclass(frozen=True)
class UpdateResult:
    installed: str
    available: str
    update_available: bool
    release_url: str


def check_latest_release(
    installed: str,
    *,
    session: Any = requests,
    timeout: float = 10.0,
) -> UpdateResult:
    response = session.get(
        RELEASES_API,
        headers={"Accept": "application/vnd.github+json"},
        timeout=timeout,
    )
    response.raise_for_status()
    body = response.json()
    if not isinstance(body, list):
        raise RuntimeError("GitHub returned an invalid SPLINED release list.")
    releases: list[tuple[tuple[int, int, int], str, str]] = []
    for item in body:
        if not isinstance(item, dict) or item.get("draft") or item.get("prerelease"):
            continue
        tag = str(item.get("tag_name") or "").strip()
        try:
            parsed = semantic_version(tag)
        except ValueError:
            continue
        url = str(item.get("html_url") or "").strip()
        if not url.startswith("https://github.com/scottia/S-P-L-I-N-E-D/releases/"):
            url = f"https://github.com/scottia/S-P-L-I-N-E-D/releases/tag/{tag.lstrip('v')}"
        releases.append((parsed, tag.lstrip("v"), url))
    if not releases:
        raise RuntimeError("No published numeric SPLINED release is available.")
    _parsed, available, url = max(releases, key=lambda item: item[0])
    return UpdateResult(
        installed=str(installed).lstrip("v"),
        available=available,
        update_available=newer_version(installed, available),
        release_url=url,
    )
