"""Resolve a release from the version already committed in both source trees.

Release workflows must build repository source exactly.  They may not invent a
new version in a temporary runner commit because that leaves ``dev`` carrying
metadata for an older, materially different build.
"""
from __future__ import annotations

import re
from collections.abc import Iterable


VERSION_RE = re.compile(r"^\d+\.\d+\.\d+$")


def _numeric(value: str) -> tuple[int, int, int]:
    if not VERSION_RE.fullmatch(value):
        raise ValueError(f"Version must be numeric X.Y.Z: {value!r}")
    return tuple(map(int, value.split(".")))


def choose_release_version(
    source_version: str,
    active_versions: Iterable[tuple[int, int, int]],
    all_tags: Iterable[str],
    requested: str = "",
) -> str:
    """Return the committed source version or reject an inconsistent release."""
    source = _numeric(source_version)
    requested = requested.strip()
    if requested and requested != source_version:
        raise ValueError(
            "Requested release version must equal the committed Windows and "
            "WebGUI source version"
        )

    active = list(active_versions)
    if active and source <= max(active):
        raise ValueError(
            "Committed source version must exceed every numeric release tag on "
            "the current main history"
        )

    occupied = set(all_tags)
    if source_version in occupied or f"v{source_version}" in occupied:
        raise ValueError(f"Release tag already exists: {source_version}")
    return source_version

