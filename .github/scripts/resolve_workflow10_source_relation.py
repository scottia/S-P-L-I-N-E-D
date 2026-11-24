"""Classify the safe Workflow 10 relationship between main and dev.

The repository promotes reviewed dev source through a merge commit on main.
That promotion commit is intentionally not merged back into dev. A later dev
commit is safe when it descends from the exact source parent whose tree the
content-preserving promotion published.
"""

from __future__ import annotations

import subprocess
import sys


def _git(*args: str) -> str:
    return subprocess.run(
        ["git", *args],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()


def _tree(commit: str) -> str:
    return _git("show", "-s", "--format=%T", commit)


def _parents(commit: str) -> tuple[str, ...]:
    value = _git("show", "-s", "--format=%P", commit)
    return tuple(value.split()) if value else ()


def _is_ancestor(ancestor: str, descendant: str) -> bool:
    result = subprocess.run(
        ["git", "merge-base", "--is-ancestor", ancestor, descendant],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode not in (0, 1):
        raise RuntimeError(result.stderr.strip() or "git merge-base failed")
    return result.returncode == 0


def classify_source_relation(main_sha: str, dev_sha: str) -> str:
    main_tree = _tree(main_sha)
    dev_tree = _tree(dev_sha)
    if main_tree == dev_tree:
        return "same tree - approved promotion merge accepted"
    if _is_ancestor(main_sha, dev_sha):
        return "different trees - origin/dev directly advances origin/main"

    parents = _parents(main_sha)
    if len(parents) > 1:
        promoted_sources = [
            parent
            for parent in parents
            if _tree(parent) == main_tree and _is_ancestor(parent, dev_sha)
        ]
        if promoted_sources:
            source = promoted_sources[0]
            return (
                "different trees - origin/dev advances content-preserving "
                f"promoted source {source}"
            )

    raise ValueError(
        "origin/main and origin/dev have different trees without a safe "
        "direct or content-preserving promotion lineage. Reconcile the "
        "branches through reviewed source promotion before running Workflow 10."
    )


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print(
            "usage: resolve_workflow10_source_relation.py MAIN_SHA DEV_SHA",
            file=sys.stderr,
        )
        return 2
    try:
        print(classify_source_relation(argv[1], argv[2]))
    except (RuntimeError, ValueError, subprocess.CalledProcessError) as error:
        print(f"::error title=Unsafe branch relationship::{error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
