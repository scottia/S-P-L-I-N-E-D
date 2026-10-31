"""Verify maintainer scope authorization and invoke the trusted boundary checker."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from typing import Mapping, Sequence
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


SHA_PATTERN = re.compile(r"[0-9a-fA-F]{40}")
REPOSITORY_PATTERN = re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+")
TRUSTED_PERMISSIONS = {"admin", "maintain"}
LABEL_TO_SCOPE = {
    "scope:windows": "windows",
    "scope:ubuntu-webgui": "ubuntu-webgui",
    "scope:shared-contract": "shared-contract",
    "scope:release-version": "release-version",
    "scope:terminology-docs": "terminology-docs",
    "scope:root-native-other": "root-native-other",
    "scope:boundary-policy": "boundary-policy",
}
REQUIRED_TRUSTED_FILES = (
    ".github/boundaries/check_boundary.py",
    ".github/boundaries/policy.json",
    ".github/scripts/trusted_boundary_adapter.py",
    ".github/scripts/validation_scope.py",
    ".github/scripts/final_validation_gate.py",
)


class AdapterError(RuntimeError):
    pass


def is_enforcement_path(path: str) -> bool:
    return (
        path == ".github/workflows/validate.yml"
        or path.startswith(".github/scripts/")
        or path.startswith(".github/boundaries/")
    )


def proposal_from_event(event: Mapping[str, object]) -> dict[str, object]:
    pull_request = event.get("pull_request")
    if not isinstance(pull_request, Mapping):
        raise AdapterError("Event is missing pull_request data")
    base = pull_request.get("base")
    head = pull_request.get("head")
    if not isinstance(base, Mapping) or not isinstance(head, Mapping):
        raise AdapterError("Pull request base/head data is incomplete")
    base_sha = str(base.get("sha", ""))
    head_sha = str(head.get("sha", ""))
    if not SHA_PATTERN.fullmatch(base_sha) or not SHA_PATTERN.fullmatch(head_sha):
        raise AdapterError("Pull request base/head must be full commit SHAs")
    number = event.get("number")
    if not isinstance(number, int) or number <= 0:
        raise AdapterError("Pull request number is missing")
    return {
        "number": number,
        "base_sha": base_sha.lower(),
        "head_sha": head_sha.lower(),
        "base_ref": str(base.get("ref", "")),
        "head_ref": str(head.get("ref", "")),
    }


def current_scope_labels(event: Mapping[str, object]) -> dict[str, str]:
    pull_request = event.get("pull_request")
    labels = pull_request.get("labels", []) if isinstance(pull_request, Mapping) else []
    if not isinstance(labels, list):
        raise AdapterError("Pull request labels are malformed")
    selected: dict[str, str] = {}
    for item in labels:
        if not isinstance(item, Mapping):
            raise AdapterError("Pull request label entry is malformed")
        name = str(item.get("name", ""))
        if not name.startswith("scope:"):
            continue
        if name not in LABEL_TO_SCOPE:
            raise AdapterError(f"Unknown scope authorization label: {name}")
        selected[name] = LABEL_TO_SCOPE[name]
    if not selected:
        raise AdapterError("No explicit scope authorization labels are present")
    return selected


def latest_label_event(label: str, timeline: Sequence[Mapping[str, object]]) -> Mapping[str, object]:
    matches = []
    for item in timeline:
        event_name = str(item.get("event", ""))
        label_data = item.get("label")
        if event_name not in {"labeled", "unlabeled"} or not isinstance(label_data, Mapping):
            continue
        if str(label_data.get("name", "")) == label:
            matches.append(item)
    if not matches:
        raise AdapterError(f"No verifiable timeline event exists for {label}")
    return matches[-1]


def verify_authorizations(
    event: Mapping[str, object],
    timeline: Sequence[Mapping[str, object]],
    actor_permissions: Mapping[str, str],
) -> tuple[list[str], list[dict[str, str]]]:
    labels = current_scope_labels(event)
    scopes: list[str] = []
    records: list[dict[str, str]] = []
    for label, scope in sorted(labels.items()):
        latest = latest_label_event(label, timeline)
        if latest.get("event") != "labeled":
            raise AdapterError(f"Latest authorization event removed {label}")
        actor = latest.get("actor")
        login = str(actor.get("login", "")) if isinstance(actor, Mapping) else ""
        permission = str(actor_permissions.get(login, ""))
        if not login or permission not in TRUSTED_PERMISSIONS:
            raise AdapterError(
                f"Authorization label {label} was not issued by a verified maintainer"
            )
        scopes.append(scope)
        records.append({"label": label, "scope": scope, "actor": login, "permission": permission})
    return sorted(set(scopes)), records


def authorization_fingerprint(
    proposal: Mapping[str, object], records: Sequence[Mapping[str, str]]
) -> str:
    payload = {
        "base_sha": proposal["base_sha"],
        "head_sha": proposal["head_sha"],
        "base_ref": proposal["base_ref"],
        "authorizations": list(records),
    }
    encoded = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def require_trusted_files(root: Path) -> None:
    missing = [path for path in REQUIRED_TRUSTED_FILES if not (root / path).is_file()]
    if missing:
        raise AdapterError(
            "Trusted checking infrastructure is incomplete; bootstrap required: "
            + ", ".join(missing)
        )


def apply_enforcement_isolation(result: dict[str, object]) -> None:
    classified = result.get("classified_paths", {})
    if not isinstance(classified, Mapping):
        raise AdapterError("Boundary result did not classify changed paths")
    paths = {str(path) for path in classified}
    enforcement = {path for path in paths if is_enforcement_path(path)}
    if not enforcement:
        result["requires_bootstrap"] = False
        return
    unrelated = sorted(paths - enforcement)
    if unrelated:
        result["allowed"] = False
        errors = list(result.get("errors", []))
        errors.append(
            "Validation enforcement changes must be isolated from unrelated paths: "
            + ", ".join(unrelated)
        )
        result["errors"] = errors
    result["requires_bootstrap"] = True


def repository_role(payload: Mapping[str, object]) -> str:
    """Return the precise role; legacy `permission` collapses maintain to write."""

    return str(payload.get("role_name") or payload.get("permission", ""))


class GitHubClient:
    def __init__(self, repository: str, token: str):
        if not REPOSITORY_PATTERN.fullmatch(repository):
            raise AdapterError("Repository must use owner/name form")
        if not token:
            raise AdapterError("GitHub token is required to verify authorization issuers")
        self.repository = repository
        self.token = token

    def get(self, path: str) -> tuple[object, Mapping[str, str]]:
        request = Request(
            "https://api.github.com" + path,
            headers={
                "Accept": "application/vnd.github+json",
                "Authorization": f"Bearer {self.token}",
                "X-GitHub-Api-Version": "2022-11-28",
                "User-Agent": "splined-trusted-boundary",
            },
        )
        try:
            with urlopen(request, timeout=30) as response:
                return json.load(response), dict(response.headers.items())
        except (HTTPError, URLError, TimeoutError, json.JSONDecodeError) as error:
            raise AdapterError(f"GitHub authorization verification failed: {error}") from error

    def timeline(self, number: int) -> list[Mapping[str, object]]:
        payload, headers = self.get(
            f"/repos/{self.repository}/issues/{number}/timeline?per_page=100"
        )
        link = str(headers.get("Link", ""))
        if 'rel="next"' in link:
            raise AdapterError("Authorization timeline is incomplete; refusing paginated evidence")
        if not isinstance(payload, list) or not all(isinstance(item, Mapping) for item in payload):
            raise AdapterError("Authorization timeline response is malformed")
        return payload

    def permission(self, login: str) -> str:
        payload, _ = self.get(
            f"/repos/{self.repository}/collaborators/{login}/permission"
        )
        if not isinstance(payload, Mapping):
            raise AdapterError(f"Permission response for {login} is malformed")
        return repository_role(payload)


def run_checker(
    trusted_root: Path,
    repository: Path,
    proposal: Mapping[str, object],
    scopes: Sequence[str],
) -> tuple[int, dict[str, object]]:
    checker = trusted_root / ".github" / "boundaries" / "check_boundary.py"
    command = [
        sys.executable,
        str(checker),
        "--repository",
        str(repository),
        "--base",
        str(proposal["base_sha"]),
        "--head",
        str(proposal["head_sha"]),
        "--json",
    ]
    for scope in scopes:
        command.extend(["--authorize", scope])
    completed = subprocess.run(command, text=True, capture_output=True, check=False)
    if completed.returncode not in {0, 2}:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise AdapterError(f"Trusted boundary checker failed operationally: {detail}")
    try:
        result = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise AdapterError("Trusted boundary checker returned invalid JSON") from error
    if not isinstance(result, dict):
        raise AdapterError("Trusted boundary checker result is malformed")
    return completed.returncode, result


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--event", type=Path, required=True)
    parser.add_argument("--repository", required=True, help="GitHub owner/name")
    parser.add_argument("--token", default=os.environ.get("GH_TOKEN", ""))
    parser.add_argument("--git-repository", type=Path, required=True)
    parser.add_argument("--trusted-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)

    try:
        event = json.loads(args.event.read_text(encoding="utf-8"))
        if not isinstance(event, Mapping):
            raise AdapterError("Event payload is malformed")
        proposal = proposal_from_event(event)
        trusted_root = args.trusted_root.resolve()
        require_trusted_files(trusted_root)
        client = GitHubClient(args.repository, args.token)
        timeline = client.timeline(int(proposal["number"]))
        labels = current_scope_labels(event)
        latest_actors = set()
        for label in labels:
            actor = latest_label_event(label, timeline).get("actor")
            if isinstance(actor, Mapping) and actor.get("login"):
                latest_actors.add(str(actor["login"]))
        permissions = {actor: client.permission(actor) for actor in latest_actors}
        scopes, records = verify_authorizations(event, timeline, permissions)
        checker_code, result = run_checker(
            trusted_root, args.git_repository.resolve(), proposal, scopes
        )
        result["proposal"] = proposal
        result["authorization_records"] = records
        result["authorization_fingerprint"] = authorization_fingerprint(proposal, records)
        apply_enforcement_isolation(result)
        args.output.write_text(
            json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        print(json.dumps(result, indent=2, sort_keys=True))
        return 0 if checker_code == 0 and result.get("allowed") is True else 2
    except (AdapterError, OSError, ValueError, json.JSONDecodeError) as error:
        print(f"trusted boundary adapter failed closed: {error}", file=sys.stderr)
        return 3


if __name__ == "__main__":
    raise SystemExit(main())
