"""Select client validation and real-writer integrity from a PR source diff."""

import re
import subprocess
import sys


VERSION_METADATA_PATHS = {
    "build/WEBGui/webgui/backend/splined.py",
    "build/Windows/Cargo.lock",
    "build/Windows/Cargo.toml",
    "build/Windows/gui/app.manifest",
    "build/Windows/package/Package.appxmanifest",
    "build/Windows/package/SPLINED.appinstaller.template",
}

SHARED_POLICY_PATHS = {
    "AGENTS.md",
    "docs/POLICY-REGISTER.md",
}

VALIDATION_GATE_PATHS = {
    ".github/workflows/validate.yml",
    ".github/workflows/21-release-preparation.yml",
    ".github/workflows/22-windows-build-store.yml",
    ".github/workflows/23-webgui-build.yml",
    ".github/workflows/24-complete-release.yml",
    ".github/scripts/check_protected_policies.py",
    ".github/scripts/test_audio_integrity_contract.py",
    ".github/scripts/test_policy_release_gates.py",
    ".github/scripts/test_promotion_validation.py",
    ".github/scripts/test_validation_scope.py",
    ".github/scripts/validate_release_policy_contracts.py",
    ".github/scripts/validation_scope.py",
}

VERSION_MUTATION_PATTERNS = (
    (re.compile(
        r'USER_AGENT = "SPLINED/(?P<version>\d+\.\d+\.\d+) '
        r'\(https://github\.com/scottia/S-P-L-I-N-E-D\)"'
    ), 1),
    (re.compile(r'VERSION = "(?P<version>\d+\.\d+\.\d+)"'), 1),
    (re.compile(r'version = "(?P<version>\d+\.\d+\.\d+)"'), 2),
    (re.compile(
        r'<assemblyIdentity version="(?P<version>\d+\.\d+\.\d+)\.0" '
        r'name="SPLINED\.Windows"/>'
    ), 1),
    (re.compile(r'Version="(?P<version>\d+\.\d+\.\d+)\.0"'), 3),
)


def _release_versions(lines):
    """Return the one version represented by an exact metadata side."""
    counts = [0] * len(VERSION_MUTATION_PATTERNS)
    versions = set()
    for line in lines:
        matches = [
            (index, pattern.fullmatch(line))
            for index, (pattern, _expected) in enumerate(VERSION_MUTATION_PATTERNS)
        ]
        matches = [(index, match) for index, match in matches if match]
        if len(matches) != 1:
            return None
        index, match = matches[0]
        counts[index] += 1
        versions.add(match.group("version"))
    expected = [value for _pattern, value in VERSION_MUTATION_PATTERNS]
    return versions.pop() if counts == expected and len(versions) == 1 else None


def is_release_metadata_only(paths, diff_text):
    """Recognize the synchronized numeric-version update without a runtime edit."""
    changed = {path for path in paths if path}
    if changed != VERSION_METADATA_PATHS:
        return False
    removed = []
    added = []
    for line in diff_text.splitlines():
        if line.startswith(("+++", "---")):
            continue
        if line.startswith("-"):
            removed.append(line[1:].strip())
        elif line.startswith("+"):
            added.append(line[1:].strip())
    old_version = _release_versions(removed)
    new_version = _release_versions(added)
    return bool(old_version and new_version and old_version != new_version)


def select_checks(paths, *, release_metadata_only=False):
    checks = {"webgui": False, "audio_webgui": False, "audio_windows": False}
    if release_metadata_only:
        return {"webgui": True, "audio_webgui": True, "audio_windows": False}
    for path in paths:
        if path == "__all__":
            return {"webgui": True, "audio_webgui": True, "audio_windows": True}
        if path in SHARED_POLICY_PATHS or path in VALIDATION_GATE_PATHS:
            checks.update(webgui=True, audio_webgui=True, audio_windows=True)
            continue
        if path.endswith(".md"):
            continue
        if path in {".gitattributes", ".dockerignore", ".github/CODEOWNERS", ".github/dependabot.yml"}:
            checks["webgui"] = True
        elif path.startswith(("schema/", "shared/")):
            checks.update(webgui=True, audio_webgui=True, audio_windows=True)
        elif path.startswith("build/WEBGui/"):
            checks.update(webgui=True, audio_webgui=True)
        elif path.startswith("build/Windows/"):
            checks.update(webgui=True, audio_windows=True)
        elif path.startswith((".github/workflows/", ".github/scripts/")):
            checks["webgui"] = True
    return checks


def main():
    try:
        base, head = sys.argv[1:]
        if not all(re.fullmatch(r"[0-9a-fA-F]{40}", ref) for ref in (base, head)):
            raise ValueError("Expected commit SHAs")
        changed = subprocess.check_output(
            ["git", "diff", "--no-renames", "--name-only", "-z", f"{base}...{head}"],
            text=True,
            encoding="utf-8",
        ).split("\0")
        diff_text = subprocess.check_output(
            ["git", "diff", "--no-renames", "--unified=0", f"{base}...{head}", "--", *VERSION_METADATA_PATHS],
            text=True,
            encoding="utf-8",
        )
        checks = select_checks(
            changed,
            release_metadata_only=is_release_metadata_only(changed, diff_text),
        )
    except (ValueError, subprocess.CalledProcessError, UnicodeError):
        print("::warning::Changed files unavailable; running both client writer checks", file=sys.stderr)
        checks = select_checks(["__all__"])
    for name, selected in checks.items():
        print(f"{name}={str(selected).lower()}")


if __name__ == "__main__":
    main()
