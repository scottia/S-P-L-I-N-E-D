"""Resolve and validate an exact, non-publishable Workflow 20 evaluation source."""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys


NUMERIC = re.compile(r"[0-9]+\.[0-9]+\.[0-9]+")
CANONICAL = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)")


def numeric_version(value: str) -> tuple[int, int, int]:
    if not NUMERIC.fullmatch(value):
        raise ValueError("Expected numeric X.Y.Z")
    return tuple(map(int, value.split(".")))


def resolve_pr(pull: dict, repository: str, number: int) -> str:
    head = pull["head"]
    if (
        number <= 0
        or pull["number"] != number
        or pull["state"] != "open"
        or pull["base"]["ref"] != "dev"
        or pull["base"]["repo"]["full_name"] != repository
        or (head.get("repo") or {}).get("full_name") != repository
        or not head["ref"]
        or head["ref"] in {"main", "dev"}
        or not re.fullmatch(r"[0-9a-f]{40}", head["sha"])
    ):
        raise ValueError("Expected an open same-repository topic PR targeting dev with an exact HEAD")
    return head["sha"]


def validate_version(committed: str, requested: str, tags: list[str]) -> None:
    if not CANONICAL.fullmatch(requested):
        raise ValueError("Evaluation version must be canonical numeric X.Y.Z")
    floors = [numeric_version(committed)]
    for tag in tags:
        candidate = tag.removeprefix("v")
        if NUMERIC.fullmatch(candidate):
            floors.append(numeric_version(candidate))
    if numeric_version(requested) <= max(floors):
        raise ValueError("Evaluation version must exceed committed source and all numeric tags")


def main() -> None:
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="command", required=True)
    resolve = commands.add_parser("resolve-pr")
    resolve.add_argument("--repository", required=True)
    resolve.add_argument("--number", type=int, required=True)
    validate = commands.add_parser("validate-version")
    validate.add_argument("--committed", required=True)
    validate.add_argument("--version", required=True)
    args = parser.parse_args()
    if args.command == "resolve-pr":
        print(resolve_pr(json.load(sys.stdin), args.repository, args.number))
    else:
        tags = subprocess.check_output(["git", "tag", "--list"], text=True).splitlines()
        validate_version(args.committed, args.version, tags)


if __name__ == "__main__":
    main()
