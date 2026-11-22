"""Read and advance SPLINED's repository-authoritative source version.

Workflow 05 uses this module to create the next source-version Draft PR.
Release workflows consume the already-committed version and must not call the
stamping command.
"""
from __future__ import annotations

import argparse
import codecs
import re
from collections.abc import Iterable
from pathlib import Path


VERSION_RE = re.compile(r"^\d+\.\d+\.\d+$")


def numeric_version(value: str) -> tuple[int, int, int]:
    if not VERSION_RE.fullmatch(value):
        raise ValueError(f"Version must be numeric X.Y.Z: {value!r}")
    return tuple(map(int, value.split(".")))


def _read_utf8(path: Path) -> tuple[str, bool]:
    data = path.read_bytes()
    has_bom = data.startswith(codecs.BOM_UTF8)
    return data.decode("utf-8-sig" if has_bom else "utf-8"), has_bom


def _write_utf8(path: Path, text: str, has_bom: bool) -> None:
    encoding = "utf-8-sig" if has_bom else "utf-8"
    path.write_bytes(text.encode(encoding))


def _one(pattern: str, text: str, label: str, flags: int = 0) -> str:
    matches = re.findall(pattern, text, flags)
    if len(matches) != 1:
        raise ValueError(f"Expected one {label}; found {len(matches)}")
    match = matches[0]
    return match if isinstance(match, str) else match[0]


def source_versions(root: Path) -> dict[str, str]:
    root = root.resolve()
    cargo, _ = _read_utf8(root / "build/Windows/Cargo.toml")
    lock, _ = _read_utf8(root / "build/Windows/Cargo.lock")
    executable_manifest, _ = _read_utf8(root / "build/Windows/gui/app.manifest")
    package_manifest, _ = _read_utf8(
        root / "build/Windows/package/Package.appxmanifest"
    )
    appinstaller, _ = _read_utf8(
        root / "build/Windows/package/SPLINED.appinstaller.template"
    )
    webgui, _ = _read_utf8(root / "build/WEBGui/webgui/backend/splined.py")
    versions = {
        "windows_cargo": _one(
            r'(?ms)^\[package\].*?^version\s*=\s*"(\d+\.\d+\.\d+)"',
            cargo,
            "Windows Cargo package version",
        ),
        "windows_lock": _one(
            r'(?m)^name = "splined-windows"\r?\nversion = "(\d+\.\d+\.\d+)"\r?$',
            lock,
            "Windows Cargo.lock package version",
        ),
        "windows_executable_manifest": _one(
            r'<assemblyIdentity version="(\d+\.\d+\.\d+)\.0" name="SPLINED\.Windows"/?>',
            executable_manifest,
            "Windows executable manifest version",
        ),
        "windows_package_manifest": _one(
            r'(?s)<Identity\b.*?\bVersion="(\d+\.\d+\.\d+)\.0"',
            package_manifest,
            "Windows package manifest version",
        ),
        "webgui_version": _one(
            r'(?m)^VERSION = "(\d+\.\d+\.\d+)"\r?$',
            webgui,
            "WebGUI version",
        ),
        "webgui_user_agent": _one(
            r'(?m)^USER_AGENT = "SPLINED/(\d+\.\d+\.\d+) ',
            webgui,
            "WebGUI user-agent version",
        ),
    }
    appinstaller_versions = re.findall(
        r'Version="(\d+\.\d+\.\d+)\.0"', appinstaller
    )
    if len(appinstaller_versions) != 2:
        raise ValueError(
            "Expected two Windows App Installer version fields; found "
            f"{len(appinstaller_versions)}"
        )
    if len(set(appinstaller_versions)) != 1:
        raise ValueError("Windows App Installer version fields do not match")
    versions["windows_appinstaller"] = appinstaller_versions[0]
    return versions


def current_source_version(root: Path) -> str:
    versions = source_versions(root)
    unique = set(versions.values())
    if len(unique) != 1:
        details = ", ".join(f"{name}={value}" for name, value in versions.items())
        raise ValueError(f"SPLINED source versions are inconsistent: {details}")
    return unique.pop()


def next_patch_version(current: str, tags: Iterable[str]) -> str:
    current_numeric = numeric_version(current)
    numeric_tags = []
    for tag in tags:
        candidate = tag.strip()
        if candidate.startswith("v"):
            candidate = candidate[1:]
        if VERSION_RE.fullmatch(candidate):
            numeric_tags.append(numeric_version(candidate))
    if not numeric_tags:
        raise ValueError("No numeric release tag exists on main history")
    latest = max(numeric_tags)
    if current_numeric != latest:
        raise ValueError(
            "Source preparation requires the committed source version to equal "
            f"the latest main-history tag; source={current}, latest={'.'.join(map(str, latest))}"
        )
    major, minor, patch = current_numeric
    return f"{major}.{minor}.{patch + 1}"


def _replace_one(
    path: Path,
    pattern: str,
    replacement: str,
    label: str,
    flags: int = 0,
) -> None:
    text, has_bom = _read_utf8(path)
    updated, count = re.subn(pattern, replacement, text, flags=flags)
    if count != 1:
        raise ValueError(f"Expected to stamp one {label}; found {count}")
    _write_utf8(path, updated, has_bom)


def stamp_source_version(root: Path, version: str) -> None:
    root = root.resolve()
    numeric_version(version)
    current = current_source_version(root)
    current_numeric = numeric_version(current)
    expected = f"{current_numeric[0]}.{current_numeric[1]}.{current_numeric[2] + 1}"
    if version != expected:
        raise ValueError(
            f"Source preparation may advance exactly one patch: expected {expected}, got {version}"
        )

    _replace_one(
        root / "build/Windows/Cargo.toml",
        r'(?ms)(^\[package\].*?^version\s*=\s*")\d+\.\d+\.\d+("\s*$)',
        rf"\g<1>{version}\g<2>",
        "Windows Cargo package version",
    )
    _replace_one(
        root / "build/Windows/Cargo.lock",
        r'(?m)(^name = "splined-windows"\r?\nversion = ")\d+\.\d+\.\d+("\r?$)',
        rf"\g<1>{version}\g<2>",
        "Windows Cargo.lock package version",
    )
    _replace_one(
        root / "build/Windows/gui/app.manifest",
        r'(<assemblyIdentity version=")\d+\.\d+\.\d+\.0(" name="SPLINED\.Windows"/?>)',
        rf"\g<1>{version}.0\g<2>",
        "Windows executable manifest version",
    )
    _replace_one(
        root / "build/Windows/package/Package.appxmanifest",
        r'(?s)(<Identity\b.*?\bVersion=")\d+\.\d+\.\d+\.0(")',
        rf"\g<1>{version}.0\g<2>",
        "Windows package manifest version",
    )

    appinstaller_path = root / "build/Windows/package/SPLINED.appinstaller.template"
    appinstaller, has_bom = _read_utf8(appinstaller_path)
    appinstaller, count = re.subn(
        r'(Version=")\d+\.\d+\.\d+\.0(")',
        rf"\g<1>{version}.0\g<2>",
        appinstaller,
    )
    if count != 2:
        raise ValueError(
            f"Expected to stamp two Windows App Installer versions; found {count}"
        )
    _write_utf8(appinstaller_path, appinstaller, has_bom)

    _replace_one(
        root / "build/WEBGui/webgui/backend/splined.py",
        r'(?m)^VERSION = "\d+\.\d+\.\d+"\r?$',
        f'VERSION = "{version}"',
        "WebGUI version",
    )
    _replace_one(
        root / "build/WEBGui/webgui/backend/splined.py",
        r'(?m)^USER_AGENT = "SPLINED/\d+\.\d+\.\d+ ',
        f'USER_AGENT = "SPLINED/{version} ',
        "WebGUI user-agent version",
    )
    stamped = current_source_version(root)
    if stamped != version:
        raise ValueError(f"Stamped source version did not validate: {stamped}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path.cwd())
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("current")
    next_parser = subparsers.add_parser("next")
    next_parser.add_argument("--tag", action="append", default=[])
    stamp_parser = subparsers.add_parser("stamp")
    stamp_parser.add_argument("version")
    args = parser.parse_args()

    if args.command == "current":
        print(current_source_version(args.root))
    elif args.command == "next":
        print(next_patch_version(current_source_version(args.root), args.tag))
    else:
        stamp_source_version(args.root, args.version)
        print(current_source_version(args.root))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
