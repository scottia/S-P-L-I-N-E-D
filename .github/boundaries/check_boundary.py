#!/usr/bin/env python3
"""Authorize changed-file scope and select validation as separate decisions."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from fnmatch import fnmatchcase
import json
from pathlib import Path
import re
import subprocess
import sys
from typing import Callable, Iterable, Mapping


POLICY_PATH = ".github/boundaries/policy.json"


@dataclass(frozen=True)
class Change:
    status: str
    path: str
    old_path: str | None = None

    @property
    def is_move(self) -> bool:
        return self.status[:1] in {"R", "C"} and self.old_path is not None

    @property
    def paths(self) -> tuple[str, ...]:
        if self.old_path is None:
            return (self.path,)
        return (self.old_path, self.path)


@dataclass
class Analysis:
    categories: set[str]
    classified_paths: dict[str, str]
    protected_fields: list[dict[str, str]]
    structural_errors: list[str]


def normalize_path(value: str) -> str:
    value = value.replace("\\", "/")
    while value.startswith("./"):
        value = value[2:]
    return value


class Policy:
    def __init__(self, document: Mapping[str, object]):
        if document.get("schema") != 1:
            raise ValueError("Boundary policy schema must be 1")
        self.document = document
        self.categories = tuple(str(value) for value in document["categories"])
        if len(self.categories) != len(set(self.categories)):
            raise ValueError("Boundary policy categories must be unique")
        required = {
            "windows",
            "ubuntu-webgui",
            "shared-contract",
            "release-version",
            "terminology-docs",
            "root-native-other",
            "boundary-policy",
        }
        if set(self.categories) != required:
            raise ValueError("Boundary policy categories do not match the required set")
        self.rules = list(document["rules"])
        self.protected_content = list(document.get("protected_content", []))
        self.validation = dict(document["validation"])
        for rule in self.rules:
            if rule["category"] not in required:
                raise ValueError(f"Unknown rule category: {rule['category']}")
        if set(self.validation) != required:
            raise ValueError("Every category must define validation selection")

    @classmethod
    def from_text(cls, text: str) -> "Policy":
        return cls(json.loads(text))

    @classmethod
    def from_file(cls, path: Path) -> "Policy":
        return cls.from_text(path.read_text(encoding="utf-8"))

    def classify(self, path: str) -> str | None:
        path = normalize_path(path)
        for rule in self.rules:
            if any(fnmatchcase(path, pattern) for pattern in rule["patterns"]):
                return str(rule["category"])
        return None


def parse_name_status_z(payload: bytes) -> list[Change]:
    tokens = payload.decode("utf-8").split("\0")
    if tokens and tokens[-1] == "":
        tokens.pop()
    changes: list[Change] = []
    index = 0
    while index < len(tokens):
        status = tokens[index]
        index += 1
        if status[:1] in {"R", "C"}:
            if index + 1 >= len(tokens):
                raise ValueError("Truncated rename/copy entry in git diff output")
            old_path, new_path = tokens[index : index + 2]
            index += 2
            changes.append(Change(status=status, path=normalize_path(new_path), old_path=normalize_path(old_path)))
        else:
            if index >= len(tokens):
                raise ValueError("Truncated path entry in git diff output")
            changes.append(Change(status=status, path=normalize_path(tokens[index])))
            index += 1
    return changes


def git_output(repository: Path, arguments: list[str], *, allow_missing: bool = False) -> bytes:
    result = subprocess.run(
        ["git", *arguments],
        cwd=repository,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        if allow_missing:
            return b""
        detail = result.stderr.decode("utf-8", errors="replace").strip()
        raise RuntimeError(detail or f"git {' '.join(arguments)} failed")
    return result.stdout


def git_changes(repository: Path, base: str, head: str) -> list[Change]:
    payload = git_output(
        repository,
        ["diff", "--name-status", "-z", "--find-renames=50%", f"{base}...{head}"],
    )
    return parse_name_status_z(payload)


def git_text(repository: Path, revision: str, path: str) -> str:
    payload = git_output(repository, ["show", f"{revision}:{normalize_path(path)}"], allow_missing=True)
    return payload.decode("utf-8", errors="replace")


def protected_values(pattern: str, text: str) -> list[object]:
    return re.findall(pattern, text)


def analyze_changes(
    policy: Policy,
    changes: Iterable[Change],
    content_loader: Callable[[str], tuple[str, str]] | None = None,
) -> Analysis:
    changes = list(changes)
    categories: set[str] = set()
    classified_paths: dict[str, str] = {}
    protected_fields: list[dict[str, str]] = []
    errors: list[str] = []

    for change in changes:
        path_categories: list[str | None] = []
        for path in change.paths:
            category = policy.classify(path)
            path_categories.append(category)
            if category is None:
                errors.append(f"Unclassified path: {path}")
                continue
            classified_paths[path] = category
            categories.add(category)
        if change.is_move and len(path_categories) == 2 and path_categories[0] != path_categories[1]:
            errors.append(
                "Cross-boundary rename/copy is forbidden: "
                f"{change.old_path} ({path_categories[0] or 'unclassified'}) -> "
                f"{change.path} ({path_categories[1] or 'unclassified'})"
            )

    if content_loader is not None:
        changed_paths = {path for change in changes for path in change.paths}
        for protected in policy.protected_content:
            path = normalize_path(str(protected["path"]))
            if path not in changed_paths:
                continue
            before, after = content_loader(path)
            pattern = str(protected["pattern"])
            if protected_values(pattern, before) != protected_values(pattern, after):
                categories.add("release-version")
                protected_fields.append({"path": path, "field": str(protected["field"])})

    if "boundary-policy" in categories and categories != {"boundary-policy"}:
        errors.append(
            "Boundary-policy changes must be isolated and cannot authorize unrelated edits in the same change"
        )

    return Analysis(
        categories=categories,
        classified_paths=classified_paths,
        protected_fields=protected_fields,
        structural_errors=errors,
    )


def authorize_scope(policy: Policy, analysis: Analysis, authorized: Iterable[str]) -> list[str]:
    authorized = set(authorized)
    errors: list[str] = []
    unknown = sorted(authorized - set(policy.categories))
    if unknown:
        errors.append("Unknown authorization categories: " + ", ".join(unknown))
    missing = sorted(analysis.categories - authorized)
    if missing:
        errors.append("Missing scope authorization: " + ", ".join(missing))
    return errors


def select_validations(policy: Policy, categories: Iterable[str]) -> list[str]:
    selected: set[str] = set()
    for category in categories:
        selected.update(str(value) for value in policy.validation.get(category, []))
    return sorted(selected)


def evaluate(
    policy: Policy,
    changes: Iterable[Change],
    authorized: Iterable[str],
    content_loader: Callable[[str], tuple[str, str]] | None = None,
) -> dict[str, object]:
    analysis = analyze_changes(policy, changes, content_loader)
    authorization_errors = authorize_scope(policy, analysis, authorized)
    errors = [*analysis.structural_errors, *authorization_errors]
    return {
        "allowed": not errors,
        "categories": sorted(analysis.categories),
        "authorized": sorted(set(authorized)),
        "validations": select_validations(policy, analysis.categories),
        "classified_paths": dict(sorted(analysis.classified_paths.items())),
        "protected_fields": analysis.protected_fields,
        "errors": errors,
    }


def load_policy(repository: Path, base: str, policy_file: Path | None) -> Policy:
    if policy_file is not None:
        return Policy.from_file(policy_file)
    text = git_text(repository, base, POLICY_PATH)
    if not text:
        raise RuntimeError(
            f"Trusted base {base} does not contain {POLICY_PATH}; "
            "bootstrap review must use --policy-file and remain boundary-policy-only"
        )
    return Policy.from_text(text)


def render_text(result: Mapping[str, object]) -> str:
    outcome = "ALLOW" if result["allowed"] else "REJECT"
    lines = [f"change-boundary: {outcome}"]
    lines.append("categories: " + (", ".join(result["categories"]) or "none"))
    lines.append("authorized: " + (", ".join(result["authorized"]) or "none"))
    lines.append("validations: " + (", ".join(result["validations"]) or "none"))
    for item in result["protected_fields"]:
        lines.append(f"protected version field changed: {item['path']} :: {item['field']}")
    for error in result["errors"]:
        lines.append(f"error: {error}")
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", type=Path, default=Path.cwd())
    parser.add_argument("--base", required=True, help="Trusted base commit")
    parser.add_argument("--head", required=True, help="Proposed head commit")
    parser.add_argument("--authorize", action="append", default=[], help="Approved category; repeat as needed")
    parser.add_argument("--policy-file", type=Path, help="Bootstrap/local policy only; normal checks load policy from --base")
    parser.add_argument("--json", action="store_true", help="Emit machine-readable JSON")
    args = parser.parse_args(argv)

    try:
        repository = args.repository.resolve()
        policy = load_policy(repository, args.base, args.policy_file)
        changes = git_changes(repository, args.base, args.head)

        def content_loader(path: str) -> tuple[str, str]:
            return git_text(repository, args.base, path), git_text(repository, args.head, path)

        result = evaluate(policy, changes, args.authorize, content_loader)
    except (OSError, ValueError, RuntimeError, json.JSONDecodeError) as error:
        print(f"change-boundary operational error: {error}", file=sys.stderr)
        return 3

    print(json.dumps(result, indent=2, sort_keys=True) if args.json else render_text(result))
    return 0 if result["allowed"] else 2


if __name__ == "__main__":
    raise SystemExit(main())
