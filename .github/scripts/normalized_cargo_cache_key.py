"""Stable, read-only Windows Cargo dependency cache key, independent of release version.

Unlike an inline PowerShell regex, this handles both CRLF and LF Windows runner
checkouts. The package is rebuilt for each exact commit; only unchanged
dependency compilation artifacts may be cached across versions.
"""

from __future__ import annotations

import hashlib
from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "build" / "Windows" / "Cargo.toml"
LOCK = ROOT / "build" / "Windows" / "Cargo.lock"

_LOCK_ENTRY = re.compile(
    r'(?m)(^\[\[package\]\]\r?\nname = "splined-windows"\r?\n)'
    r'version = "[^"\r\n]+"'
)
_PACKAGE = re.compile(
    r'(?ms)^\[package\]\r?\n(?P<body>.*?)(?=^\[|\Z)'
)
_VERSION = re.compile(r'(?m)^[ \t]*version[ \t]*=[ \t]*"[^"\r\n]+"[ \t]*\r?$')


def normalized_contents(manifest: str, lock: str) -> str:
    lock_matches = list(_LOCK_ENTRY.finditer(lock))
    if len(lock_matches) != 1:
        raise ValueError("Expected exactly one splined-windows Cargo.lock package entry")
    normalized_lock = _LOCK_ENTRY.sub(
        lambda match: match.group(1) + 'version = "<release>"', lock
    )

    packages = list(_PACKAGE.finditer(manifest))
    if len(packages) != 1:
        raise ValueError("Expected exactly one Cargo.toml [package] section")
    section = packages[0]
    versions = list(_VERSION.finditer(section.group("body")))
    if len(versions) != 1:
        raise ValueError("Expected exactly one [package] version in Cargo.toml")
    value = versions[0]
    offset = section.start("body") + value.start()
    end = section.start("body") + value.end()
    normalized_manifest = manifest[:offset] + 'version = "<release>"' + manifest[end:]

    # Git auto-CRLF must not give distinct dependency keys on otherwise
    # identical checked-out source.
    return (normalized_lock + "\n" + normalized_manifest).replace("\r\n", "\n")


def cache_key(manifest: str, lock: str) -> str:
    return hashlib.sha256(normalized_contents(manifest, lock).encode("utf-8")).hexdigest()


def main() -> None:
    print("hash=" + cache_key(
        MANIFEST.read_bytes().decode("utf-8-sig"),
        LOCK.read_bytes().decode("utf-8-sig"),
    ))


if __name__ == "__main__":
    main()
