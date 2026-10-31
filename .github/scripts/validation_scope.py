"""Select runtime validation from the files changed by a pull request."""

import re
import subprocess
import sys


def select_checks(paths):
    checks = {"webgui": False, "linux": False, "windows_advisory": False}
    for path in paths:
        if path == "__all__":
            return dict.fromkeys(checks, True)
        if path.endswith(".md"):
            continue
        if (
            path in {".gitattributes", "rust-toolchain", "rust-toolchain.toml"}
            or path.startswith(
                (
                    ".github/workflows/",
                    ".github/scripts/",
                    ".cargo/",
                    "schema/",
                    "fixtures/",
                )
            )
        ):
            return dict.fromkeys(checks, True)
        # Branding, instructions, package scripts, and local helper messages do
        # not benefit from compiling the native applications on every PR.
        if path.startswith("assets/") or path.startswith(
            ("windows/scripts/", "windows/package/")
        ):
            continue
        if path == "config.example.toml":
            checks["webgui"] = True
            checks["linux"] = True
            continue
        if path.startswith(("webgui/", "docker/")) or path == ".dockerignore":
            checks["webgui"] = True
        if (
            path.startswith(("windows/src/", "windows/ui/"))
            or path
            in {
                "windows/build.rs",
                "windows/Cargo.toml",
                "windows/Cargo.lock",
                "windows/gui/app.ico",
                "windows/gui/app.manifest",
            }
        ):
            checks["windows_advisory"] = True
        if path.startswith(("src/", "tests/")) or path in {"Cargo.toml", "Cargo.lock", "build.rs", "deny.toml"}:
            checks["linux"] = True
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
        print("::warning::Changed files unavailable; running all runtime checks", file=sys.stderr)
        checks = select_checks(["__all__"])
    print(f"webgui={str(checks['webgui']).lower()}")
    print(f"linux={str(checks['linux']).lower()}")
    print(f"windows_advisory={str(checks['windows_advisory']).lower()}")


if __name__ == "__main__":
    main()
