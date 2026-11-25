"""Resolve reviewed source to a safe numeric release version."""
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


def choose_workflow10_release_version(
    source_version: str,
    active_versions: Iterable[tuple[int, int, int]],
    all_tags: Iterable[str],
    latest_product_matches: bool,
    *,
    complete_existing_release: bool = False,
) -> tuple[str, bool]:
    """Return Workflow 10's version and whether a temporary stamp is required.

    Normal builds and new releases retain the approved source lineage, even
    when the latest tagged product digest matches. Only an explicit recovery
    request may select an existing release; it must match the latest product.
    Otherwise use an untagged committed version or allocate the next patch.
    """
    source = _numeric(source_version)
    active = list(active_versions)
    occupied = set(all_tags)
    if complete_existing_release:
        if not active or not latest_product_matches:
            raise ValueError("Existing-release recovery requires matching latest tagged product source")
        return ".".join(map(str, max(active))), False
    if not active:
        if source_version in occupied or f"v{source_version}" in occupied:
            raise ValueError(f"Release tag already exists: {source_version}")
        return source_version, False

    latest = max(active)

    if source > latest and source_version not in occupied and f"v{source_version}" not in occupied:
        return source_version, False

    major, minor, patch = latest
    next_text = f"{major}.{minor}.{patch + 1}"
    if next_text in occupied or f"v{next_text}" in occupied:
        raise ValueError(f"Next release tag already exists: {next_text}")
    return next_text, next_text != source_version

