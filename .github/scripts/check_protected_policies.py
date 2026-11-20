#!/usr/bin/env python3
"""Fail PRs that alter either canonical behavioral policy JSON file.

Never modifies source, policy state or GitHub settings. The owner approval
process is documented in AGENTS.md, not inferred from existing tests.
"""
from __future__ import annotations

import re
import subprocess
import sys

PROTECTED_POLICIES = frozenset({
    "schema/media-state-classification.json",
    "schema/comp-report-policy.json",
})


def protected_changes(name_status_z: bytes) -> list[str]:
    tokens = name_status_z.decode("utf-8").split("\0")
    if tokens[-1] == "":
        tokens.pop()
    touched: set[str] = set()
    index = 0
    while index < len(tokens):
        status = tokens[index]
        index += 1
        if not re.fullmatch(r"[A-Z][0-9]*", status):
            raise ValueError(f"Malformed git diff status: {status!r}")
        count = 2 if status.startswith(("R", "C")) else 1
        if index + count > len(tokens):
            raise ValueError("Truncated git name-status diff")
        for path in tokens[index : index + count]:
            if path in PROTECTED_POLICIES:
                touched.add(path)
        index += count
    return sorted(touched)


def main(argv: list[str]) -> int:
    if len(argv) != 2 or not all(re.fullmatch(r"[0-9a-fA-F]{40}", sha) for sha in argv):
        print("::error title=Policy review check unavailable::Expected exact PR base/head commit SHAs")
        return 3
    base, head = argv
    try:
        diff = subprocess.run(
            ["git", "diff", "--name-status", "-z", "--find-renames=50%", f"{base}...{head}"],
            capture_output=True,
            check=True,
        )
        touched = protected_changes(diff.stdout)
    except (OSError, UnicodeError, ValueError, subprocess.CalledProcessError) as error:
        print(f"::error title=Policy review check unavailable::Fail closed: {type(error).__name__}")
        return 3

    if not touched:
        print("Canonical policy JSON review: PASS (neither protected policy changed)")
        return 0

    for path in touched:
        print(
            f"::error file={path},title=Owner policy review required::"
            "Protected behavioral policy JSON changed. Review AGENTS.md, "
            "docs/POLICY-REGISTER.md and docs/POLICY-INDEX.md. "
            "Revert unintended edits; intentional changes require a separately "
            "owner-authorized policy-review process. Do not bypass this check."
        )
    print("Canonical policy JSON review: BLOCKED. Nothing was reverted or published. See AGENTS.md.")
    return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
