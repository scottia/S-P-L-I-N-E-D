"""Fail-closed, non-publishable Workflow 20 PR evaluation source selection."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import subprocess
import sys

from source_version import current_source_version, numeric_version


VERSION = re.compile(r"[0-9]+\.[0-9]+\.[0-9]+")


def resolve_pr(pulls: list[dict], repository: str, branch: str, sha: str) -> int:
    if branch in {"main", "dev"} or not re.fullmatch(r"[0-9a-f]{40}", sha):
        raise ValueError("Select an exact PR topic-branch HEAD")
    matches = [
        pull for pull in pulls
        if pull["state"] == "open"
        and pull["base"]["ref"] == "dev"
        and pull["base"]["repo"]["full_name"] == repository
        and (pull["head"].get("repo") or {}).get("full_name") == repository
        and pull["head"]["ref"] == branch
    ]
    if len(matches) != 1:
        raise ValueError("Expected exactly one open same-repository PR targeting dev")
    pr = matches[0]
    if pr["head"]["sha"] != sha:
        raise ValueError("PR HEAD advanced; dispatch again from the current PR branch")
    number = pr["number"]
    if type(number) is not int or number <= 0:
        raise ValueError("Invalid PR number")
    return number


def validate_version(committed: str, requested: str, tags: list[str]) -> None:
    if not VERSION.fullmatch(requested):
        raise ValueError("Evaluation version must be numeric X.Y.Z")
    requested_number = numeric_version(requested)
    floors = [numeric_version(committed)]
    for tag in tags:
        candidate = tag.removeprefix("v")
        if VERSION.fullmatch(candidate):
            floors.append(numeric_version(candidate))
    if requested_number <= max(floors):
        raise ValueError("Evaluation version must exceed committed source and all numeric tags")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repository", required=True)
    parser.add_argument("--branch", required=True)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--version", required=True)
    args = parser.parse_args()
    pr = resolve_pr(json.load(sys.stdin), args.repository, args.branch, args.sha)
    tags = subprocess.check_output(["git", "tag", "--list"], text=True).splitlines()
    validate_version(current_source_version(Path.cwd()), args.version, tags)
    print(pr)


if __name__ == "__main__":
    main()
