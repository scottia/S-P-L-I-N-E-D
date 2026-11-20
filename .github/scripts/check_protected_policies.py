"""Independent, fail-closed PR diff gate for both protected behavioural JSONs.

This is a review boundary, not a way to approve, rewrite or auto-revert a
policy. Intentional policy revisions require a separate owner-reviewed process.
"""
from __future__ import annotations

import re
import subprocess
import sys

PROTECTED = (
    "schema/media-state-classification.json",
    "schema/comp-report-policy.json",
)


def check_diff(base: str, head: str) -> None:
    for name, sha in (("base", base), ("head", head)):
        if re.fullmatch(r"[0-9a-fA-F]{40}", sha) is None:
            raise ValueError(f"invalid {name} commit SHA")
    # --no-renames makes a rename out of a protected path visible as a
    # deletion and a rename into it visible as an addition.
    result = subprocess.run(
        ["git", "diff", "--name-status", "--no-renames", base, head,
         "--", *PROTECTED],
        check=True,
        text=True,
        capture_output=True,
    )
    if result.stdout.strip():
        raise RuntimeError(
            "Protected policy JSON change needs independent owner-policy review.\n"
            + result.stdout
            + "\nRead AGENTS.md, docs/POLICY-REGISTER.md and docs/POLICY-INDEX.md."
            + " Do not bypass or change this gate to make the PR pass."
        )


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit("usage: check_protected_policies.py BASE_SHA HEAD_SHA")
    try:
        check_diff(sys.argv[1], sys.argv[2])
    except (RuntimeError, ValueError, subprocess.CalledProcessError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        raise SystemExit(1) from error
    print("PASS: both protected behavioural JSON files unchanged")
