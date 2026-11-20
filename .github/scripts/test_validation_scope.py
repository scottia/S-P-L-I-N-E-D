"""Select active WebGUI validation from a pull request source diff.

Windows builds remain governed by Workflow 01 and its optional extended
Windows validation. Root-native CLI has been archived.
"""

import re
import subprocess
import sys


def select_checks(paths):
    checks = {"webgui": False}
    for path in paths:
        if path == "__all__":
            return {"webgui": True}
        if path.endswith(".md"):
            continue
        if path in {".gitattributes", ".dockerignore", ".github/CODEOWNERS", ".github/dependabot.yml"}:
            checks["webgui"] = True
        elif path.startswith((".github/workflows/", ".github/scripts/", "schema/", "docker/", "build/WEBGui/")):
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
        checks = select_checks(changed)
    except (ValueError, subprocess.CalledProcessError, UnicodeError):
        print("::warning::Changed files unavailable; running WebGUI checks", file=sys.stderr)
        checks = select_checks(["__all__"])
    print(f"webgui={str(checks['webgui']).lower()}")


if __name__ == "__main__":
    main()
