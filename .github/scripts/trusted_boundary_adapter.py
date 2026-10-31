"""Evaluate and report a trusted boundary decision for the current pull request."""

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
from urllib.parse import quote, urlparse
from urllib.request import Request, urlopen

from final_validation_gate import evaluate_trusted_gate
from validation_scope import select_lanes


SHA_PATTERN = re.compile(r"[0-9a-fA-F]{40}")
REPOSITORY_PATTERN = re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+")
TRUSTED_PERMISSIONS = {"admin", "maintain"}
ALLOWED_BASE_REFS = {"main", "dev"}
BOUNDARY_WORKFLOW_PATH = ".github/workflows/boundary.yml"
VALIDATION_WORKFLOW_PATH = ".github/workflows/validate.yml"
VALIDATION_WORKFLOW_NAME = "Validate supported runtimes"
STATUS_CONTEXT = "SPLINED / trusted boundary"
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
    BOUNDARY_WORKFLOW_PATH,
    VALIDATION_WORKFLOW_PATH,
    ".github/boundaries/check_boundary.py",
    ".github/boundaries/policy.json",
    ".github/scripts/trusted_boundary_adapter.py",
    ".github/scripts/validation_scope.py",
    ".github/scripts/final_validation_gate.py",
)


class AdapterError(RuntimeError):
    pass


class ObsoleteEvent(AdapterError):
    pass


def require_sha(value: object, label: str) -> str:
    rendered = str(value or "").lower()
    if not SHA_PATTERN.fullmatch(rendered):
        raise AdapterError(f"{label} must be a full commit SHA")
    return rendered


def positive_integer(value: object, label: str) -> int:
    if isinstance(value, bool):
        raise AdapterError(f"{label} must be a positive integer")
    try:
        rendered = int(value)
    except (TypeError, ValueError) as error:
        raise AdapterError(f"{label} must be a positive integer") from error
    if rendered <= 0:
        raise AdapterError(f"{label} must be a positive integer")
    return rendered


def is_enforcement_path(path: str) -> bool:
    return (
        path in {BOUNDARY_WORKFLOW_PATH, VALIDATION_WORKFLOW_PATH}
        or path.startswith(".github/scripts/")
        or path.startswith(".github/boundaries/")
    )


def event_identity(event: Mapping[str, object]) -> dict[str, object]:
    pull_request = event.get("pull_request")
    if isinstance(pull_request, Mapping):
        base = pull_request.get("base")
        head = pull_request.get("head")
        if not isinstance(base, Mapping) or not isinstance(head, Mapping):
            raise AdapterError("Pull request event base/head data is incomplete")
        return {
            "event_kind": "pull_request_target",
            "number": positive_integer(event.get("number") or pull_request.get("number"), "PR number"),
            "base_sha": require_sha(base.get("sha"), "Event PR base"),
            "head_sha": require_sha(head.get("sha"), "Event PR head"),
            "base_ref": str(base.get("ref", "")),
        }

    workflow_run = event.get("workflow_run")
    if not isinstance(workflow_run, Mapping):
        raise AdapterError("Event is neither pull_request_target nor workflow_run")
    pull_requests = workflow_run.get("pull_requests")
    if not isinstance(pull_requests, list) or len(pull_requests) != 1:
        raise AdapterError("Validation workflow event must identify exactly one pull request")
    association = pull_requests[0]
    if not isinstance(association, Mapping):
        raise AdapterError("Validation workflow pull-request association is malformed")
    base = association.get("base")
    head = association.get("head")
    if not isinstance(base, Mapping) or not isinstance(head, Mapping):
        raise AdapterError("Validation workflow PR base/head association is incomplete")
    return {
        "event_kind": "workflow_run",
        "number": positive_integer(association.get("number"), "PR number"),
        "base_sha": require_sha(base.get("sha"), "Event validation base"),
        "head_sha": require_sha(head.get("sha"), "Event validation head"),
        "base_ref": str(base.get("ref", "")),
        "validation_run_id": positive_integer(workflow_run.get("id"), "Validation run ID"),
        "validation_run_attempt": positive_integer(
            workflow_run.get("run_attempt"), "Validation run attempt"
        ),
    }


def proposal_from_event(event: Mapping[str, object]) -> dict[str, object]:
    identity = event_identity(event)
    return {
        "number": identity["number"],
        "base_sha": identity["base_sha"],
        "head_sha": identity["head_sha"],
        "base_ref": identity["base_ref"],
        "head_ref": str(
            event.get("pull_request", {}).get("head", {}).get("ref", "")
            if isinstance(event.get("pull_request"), Mapping)
            else ""
        ),
    }


def proposal_from_pull_request(
    number: int,
    pull_request: Mapping[str, object],
    repository: str,
    repository_id: int,
) -> dict[str, object]:
    if pull_request.get("state") != "open":
        raise AdapterError("Pull request is not open")
    base = pull_request.get("base")
    head = pull_request.get("head")
    if not isinstance(base, Mapping) or not isinstance(head, Mapping):
        raise AdapterError("Current pull request base/head data is incomplete")
    base_repository = base.get("repo")
    head_repository = head.get("repo")
    if not isinstance(base_repository, Mapping) or not isinstance(head_repository, Mapping):
        raise AdapterError("Current pull request repository identity is incomplete")
    if (
        base_repository.get("full_name") != repository
        or base_repository.get("id") != repository_id
    ):
        raise AdapterError("Pull request target repository does not match the trusted repository")
    base_ref = str(base.get("ref", ""))
    if base_ref not in ALLOWED_BASE_REFS:
        raise AdapterError(f"Pull request targets unsupported branch: {base_ref!r}")
    return {
        "number": positive_integer(number, "PR number"),
        "base_sha": require_sha(base.get("sha"), "Current PR base"),
        "head_sha": require_sha(head.get("sha"), "Current PR head"),
        "base_ref": base_ref,
        "head_ref": str(head.get("ref", "")),
        "base_repository": repository,
        "base_repository_id": repository_id,
        "head_repository": str(head_repository.get("full_name", "")),
        "head_repository_id": positive_integer(
            head_repository.get("id"), "Head repository ID"
        ),
    }


def event_matches_proposal(
    identity: Mapping[str, object], proposal: Mapping[str, object]
) -> bool:
    return all(
        identity.get(field) == proposal.get(field)
        for field in ("number", "base_sha", "head_sha", "base_ref")
    )


def current_scope_labels(pull_request_or_event: Mapping[str, object]) -> dict[str, str]:
    pull_request = pull_request_or_event.get("pull_request")
    if isinstance(pull_request, Mapping):
        source = pull_request
    else:
        source = pull_request_or_event
    labels = source.get("labels", [])
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
    pull_request_or_event: Mapping[str, object],
    timeline: Sequence[Mapping[str, object]],
    actor_permissions: Mapping[str, str],
) -> tuple[list[str], list[dict[str, object]]]:
    labels = current_scope_labels(pull_request_or_event)
    scopes: list[str] = []
    records: list[dict[str, object]] = []
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
        event_id = positive_integer(latest.get("id"), f"Timeline event ID for {label}")
        created_at = str(latest.get("created_at", ""))
        if not created_at:
            raise AdapterError(f"Timeline event timestamp is missing for {label}")
        scopes.append(scope)
        records.append(
            {
                "label": label,
                "scope": scope,
                "actor": login,
                "permission": permission,
                "event_id": event_id,
                "created_at": created_at,
            }
        )
    return sorted(set(scopes)), records


def authorization_fingerprint(
    proposal: Mapping[str, object], records: Sequence[Mapping[str, object]]
) -> str:
    payload = {
        "repository": proposal.get("base_repository", ""),
        "repository_id": proposal.get("base_repository_id", 0),
        "pull_request_number": proposal["number"],
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
    def __init__(self, repository: str, token: str, api_url: str = "https://api.github.com"):
        if not REPOSITORY_PATTERN.fullmatch(repository):
            raise AdapterError("Repository must use owner/name form")
        if not token:
            raise AdapterError("GitHub token is required")
        parsed = urlparse(api_url)
        if parsed.scheme != "https" or not parsed.netloc:
            raise AdapterError("GitHub API URL must be HTTPS")
        self.repository = repository
        self.token = token
        self.api_url = api_url.rstrip("/")

    def request(
        self, method: str, path: str, payload: Mapping[str, object] | None = None
    ) -> tuple[object, Mapping[str, str]]:
        if path.startswith(self.api_url + "/"):
            url = path
        elif path.startswith("/"):
            url = self.api_url + path
        else:
            raise AdapterError("GitHub API path is malformed")
        data = None
        headers = {
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {self.token}",
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "splined-trusted-boundary",
        }
        if payload is not None:
            data = json.dumps(payload).encode("utf-8")
            headers["Content-Type"] = "application/json"
        request = Request(url, data=data, method=method, headers=headers)
        try:
            with urlopen(request, timeout=30) as response:
                body = response.read()
                value = json.loads(body) if body else {}
                return value, dict(response.headers.items())
        except (HTTPError, URLError, TimeoutError, json.JSONDecodeError) as error:
            raise AdapterError(f"GitHub API request failed closed: {error}") from error

    def get(self, path: str) -> tuple[object, Mapping[str, str]]:
        return self.request("GET", path)

    @staticmethod
    def next_link(headers: Mapping[str, str]) -> str | None:
        link = next((value for key, value in headers.items() if key.lower() == "link"), "")
        for item in str(link).split(","):
            match = re.match(r'\s*<([^>]+)>;\s*rel="([^"]+)"', item)
            if match and match.group(2) == "next":
                return match.group(1)
        return None

    def paginated(self, path: str, key: str | None = None) -> list[Mapping[str, object]]:
        values: list[Mapping[str, object]] = []
        next_path: str | None = path
        for _ in range(10):
            if next_path is None:
                return values
            payload, headers = self.get(next_path)
            page = payload.get(key) if key and isinstance(payload, Mapping) else payload
            if not isinstance(page, list) or not all(isinstance(item, Mapping) for item in page):
                raise AdapterError("Paginated GitHub API response is malformed")
            values.extend(page)
            next_path = self.next_link(headers)
            if next_path is not None and not next_path.startswith(self.api_url + "/"):
                raise AdapterError("GitHub pagination attempted to leave the trusted API origin")
        raise AdapterError("GitHub API evidence exceeds the supported complete pagination limit")

    def repository_data(self) -> Mapping[str, object]:
        payload, _ = self.get(f"/repos/{self.repository}")
        if not isinstance(payload, Mapping):
            raise AdapterError("Repository response is malformed")
        return payload

    def branch(self, name: str) -> Mapping[str, object]:
        payload, _ = self.get(f"/repos/{self.repository}/branches/{quote(name, safe='')}")
        if not isinstance(payload, Mapping):
            raise AdapterError("Branch response is malformed")
        return payload

    def pull_request(self, number: int) -> Mapping[str, object]:
        payload, _ = self.get(f"/repos/{self.repository}/pulls/{number}")
        if not isinstance(payload, Mapping):
            raise AdapterError("Pull request response is malformed")
        return payload

    def timeline(self, number: int) -> list[Mapping[str, object]]:
        return self.paginated(
            f"/repos/{self.repository}/issues/{number}/timeline?per_page=100"
        )

    def permission(self, login: str) -> str:
        payload, _ = self.get(
            f"/repos/{self.repository}/collaborators/{quote(login, safe='')}/permission"
        )
        if not isinstance(payload, Mapping):
            raise AdapterError(f"Permission response for {login} is malformed")
        return repository_role(payload)

    def workflow(self) -> Mapping[str, object]:
        payload, _ = self.get(
            f"/repos/{self.repository}/actions/workflows/{quote('validate.yml', safe='')}"
        )
        if not isinstance(payload, Mapping):
            raise AdapterError("Validation workflow response is malformed")
        return payload

    def workflow_runs(self, workflow_id: int) -> list[Mapping[str, object]]:
        return self.paginated(
            f"/repos/{self.repository}/actions/workflows/{workflow_id}/runs"
            "?event=pull_request&per_page=100",
            "workflow_runs",
        )

    def run_attempt(self, run_id: int, attempt: int) -> Mapping[str, object]:
        payload, _ = self.get(
            f"/repos/{self.repository}/actions/runs/{run_id}/attempts/{attempt}"
        )
        if not isinstance(payload, Mapping):
            raise AdapterError("Validation workflow run-attempt response is malformed")
        return payload

    def jobs(self, run_id: int, attempt: int) -> list[Mapping[str, object]]:
        return self.paginated(
            f"/repos/{self.repository}/actions/runs/{run_id}/attempts/{attempt}"
            "/jobs?per_page=100",
            "jobs",
        )

    def create_status(
        self,
        sha: str,
        *,
        state: str,
        description: str,
        context: str,
        target_url: str,
    ) -> None:
        if state not in {"error", "failure", "pending", "success"}:
            raise AdapterError(f"Invalid commit status state: {state}")
        payload, _ = self.request(
            "POST",
            f"/repos/{self.repository}/statuses/{require_sha(sha, 'Status commit')}",
            {
                "state": state,
                "description": description[:140],
                "context": context,
                "target_url": target_url,
            },
        )
        if not isinstance(payload, Mapping):
            raise AdapterError("Commit status response is malformed")


def git_output(repository: Path, arguments: Sequence[str]) -> str:
    completed = subprocess.run(
        ["git", *arguments],
        cwd=repository,
        text=True,
        encoding="utf-8",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if completed.returncode != 0:
        raise AdapterError(
            completed.stderr.strip() or f"git {' '.join(arguments)} failed"
        )
    return completed.stdout.strip()


def verify_trusted_context(
    client: GitHubClient,
    repository_id: int,
    trusted_root: Path,
    trusted_sha: str,
    workflow_ref: str,
    *,
    require_all_files: bool = True,
) -> dict[str, object]:
    if require_all_files:
        require_trusted_files(trusted_root)
    elif not (trusted_root / BOUNDARY_WORKFLOW_PATH).is_file():
        raise AdapterError("Trusted boundary workflow is missing")
    repository = client.repository_data()
    if repository.get("full_name") != client.repository or repository.get("id") != repository_id:
        raise AdapterError("Trusted repository name/ID does not match the workflow context")
    default_branch = str(repository.get("default_branch", ""))
    if not default_branch:
        raise AdapterError("Repository default branch is missing")
    branch = client.branch(default_branch)
    commit = branch.get("commit")
    if not isinstance(commit, Mapping):
        raise AdapterError("Default branch commit identity is missing")
    default_sha = require_sha(commit.get("sha"), "Current default branch")
    trusted_sha = require_sha(trusted_sha, "Trusted infrastructure")
    expected_ref = (
        f"{client.repository}/{BOUNDARY_WORKFLOW_PATH}@refs/heads/{default_branch}"
    )
    if workflow_ref != expected_ref:
        raise AdapterError(f"Trusted workflow ref mismatch: {workflow_ref!r}")
    if trusted_sha != default_sha:
        raise AdapterError(
            "Trusted workflow revision is stale relative to the current default branch"
        )
    checkout_sha = require_sha(git_output(trusted_root, ["rev-parse", "HEAD"]), "Trusted checkout")
    if checkout_sha != trusted_sha:
        raise AdapterError("Trusted checkout does not match github.workflow_sha")
    return {
        "sha": trusted_sha,
        "workflow_ref": workflow_ref,
        "default_branch": default_branch,
    }


def verified_authorization(
    client: GitHubClient,
    proposal: Mapping[str, object],
    pull_request: Mapping[str, object],
) -> tuple[list[str], list[dict[str, object]], str]:
    timeline = client.timeline(int(proposal["number"]))
    labels = current_scope_labels(pull_request)
    actors: set[str] = set()
    for label in labels:
        actor = latest_label_event(label, timeline).get("actor")
        if isinstance(actor, Mapping) and actor.get("login"):
            actors.add(str(actor["login"]))
    permissions = {actor: client.permission(actor) for actor in actors}
    scopes, records = verify_authorizations(pull_request, timeline, permissions)
    return scopes, records, authorization_fingerprint(proposal, records)


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


def normalize_workflow_path(value: object) -> str:
    return str(value or "").split("@", 1)[0]


def association_matches(
    association: Mapping[str, object], proposal: Mapping[str, object]
) -> bool:
    base = association.get("base")
    head = association.get("head")
    if not isinstance(base, Mapping) or not isinstance(head, Mapping):
        return False
    return (
        association.get("number") == proposal.get("number")
        and str(base.get("sha", "")).lower() == proposal.get("base_sha")
        and str(head.get("sha", "")).lower() == proposal.get("head_sha")
        and base.get("ref") == proposal.get("base_ref")
    )


def validation_run_matches(
    run: Mapping[str, object],
    proposal: Mapping[str, object],
    repository: str,
    repository_id: int,
    workflow_id: int,
) -> bool:
    repository_data = run.get("repository")
    head_repository = run.get("head_repository")
    associations = run.get("pull_requests")
    if not isinstance(repository_data, Mapping) or not isinstance(head_repository, Mapping):
        return False
    if not isinstance(associations, list):
        return False
    return (
        run.get("workflow_id") == workflow_id
        and run.get("name") == VALIDATION_WORKFLOW_NAME
        and normalize_workflow_path(run.get("path")) == VALIDATION_WORKFLOW_PATH
        and run.get("event") == "pull_request"
        and repository_data.get("full_name") == repository
        and repository_data.get("id") == repository_id
        and head_repository.get("full_name") == proposal.get("head_repository")
        and head_repository.get("id") == proposal.get("head_repository_id")
        and sum(
            1
            for association in associations
            if isinstance(association, Mapping)
            and association_matches(association, proposal)
        )
        == 1
    )


def select_canonical_validation_run(
    runs: Sequence[Mapping[str, object]],
    proposal: Mapping[str, object],
    repository: str,
    repository_id: int,
    workflow_id: int,
) -> Mapping[str, object] | None:
    candidates = [
        run
        for run in runs
        if validation_run_matches(run, proposal, repository, repository_id, workflow_id)
    ]
    if not candidates:
        return None
    return max(
        candidates,
        key=lambda run: (
            positive_integer(run.get("run_number"), "Validation run number"),
            positive_integer(run.get("id"), "Validation run ID"),
        ),
    )


def validation_expected_identity(
    workflow: Mapping[str, object],
    proposal: Mapping[str, object],
    repository: str,
    repository_id: int,
    target_workflow_blob: str,
) -> dict[str, object]:
    workflow_id = positive_integer(workflow.get("id"), "Validation workflow ID")
    if workflow.get("name") != VALIDATION_WORKFLOW_NAME:
        raise AdapterError("Validation workflow name does not match the approved workflow")
    if normalize_workflow_path(workflow.get("path")) != VALIDATION_WORKFLOW_PATH:
        raise AdapterError("Validation workflow path does not match the approved workflow")
    if workflow.get("state") != "active":
        raise AdapterError("Validation workflow is not active")
    return {
        "repository": repository,
        "repository_id": repository_id,
        "pull_request_number": proposal["number"],
        "base_sha": proposal["base_sha"],
        "head_sha": proposal["head_sha"],
        "workflow_id": workflow_id,
        "workflow_path": VALIDATION_WORKFLOW_PATH,
        "workflow_name": VALIDATION_WORKFLOW_NAME,
        "target_workflow_blob": target_workflow_blob,
    }


def build_validation_evidence(
    client: GitHubClient,
    proposal: Mapping[str, object],
    repository_id: int,
    target_workflow_blob: str,
) -> tuple[dict[str, object], dict[str, object]]:
    workflow = client.workflow()
    expected = validation_expected_identity(
        workflow, proposal, client.repository, repository_id, target_workflow_blob
    )
    workflow_id = int(expected["workflow_id"])
    selected = select_canonical_validation_run(
        client.workflow_runs(workflow_id),
        proposal,
        client.repository,
        repository_id,
        workflow_id,
    )
    if selected is None:
        return {"availability": "missing"}, expected

    run_id = positive_integer(selected.get("id"), "Validation run ID")
    run_number = positive_integer(selected.get("run_number"), "Validation run number")
    run_attempt = positive_integer(selected.get("run_attempt"), "Validation run attempt")
    attempt = dict(selected)
    attempt.update(client.run_attempt(run_id, run_attempt))
    if not validation_run_matches(
        attempt, proposal, client.repository, repository_id, workflow_id
    ):
        raise AdapterError("Current validation run attempt identity is inconsistent")
    if positive_integer(attempt.get("id"), "Validation run-attempt ID") != run_id:
        raise AdapterError("Validation run-attempt ID changed")
    if positive_integer(attempt.get("run_attempt"), "Validation run attempt") != run_attempt:
        raise AdapterError("Validation run attempt changed during evidence collection")

    evidence: dict[str, object] = {
        "availability": "present",
        **expected,
        "run_id": run_id,
        "run_number": run_number,
        "run_attempt": run_attempt,
        "status": str(attempt.get("status", "")),
        "conclusion": attempt.get("conclusion"),
        "check_suite_id": positive_integer(
            attempt.get("check_suite_id"), "Validation check-suite ID"
        ),
        "jobs": [],
    }
    if evidence["status"] == "completed":
        evidence["jobs"] = [
            {
                "id": positive_integer(job.get("id"), "Validation job ID"),
                "name": str(job.get("name", "")),
                "status": str(job.get("status", "")),
                "conclusion": job.get("conclusion"),
                "run_attempt": run_attempt,
            }
            for job in client.jobs(run_id, run_attempt)
        ]
    return evidence, expected


def target_workflow_blob(repository: Path, proposal: Mapping[str, object]) -> str:
    return require_sha(
        git_output(
            repository,
            ["rev-parse", f"{proposal['base_sha']}:{VALIDATION_WORKFLOW_PATH}"],
        ),
        "Target validation workflow blob",
    )


def stable_json(value: object) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"))


def decision_fingerprint(decision: Mapping[str, object]) -> str:
    payload = dict(decision)
    payload.pop("description", None)
    payload.pop("evidence_fingerprint", None)
    return hashlib.sha256(stable_json(payload).encode("utf-8")).hexdigest()


def decision_description(state: str, errors: Sequence[str], fingerprint: str) -> str:
    if state == "success":
        return f"Authorized scope and selected validation passed ({fingerprint[:12]})"
    if state == "pending":
        return f"Waiting for current validation evidence ({fingerprint[:12]})"
    if errors:
        return str(errors[0])[:140]
    return "Trusted boundary evaluation failed closed"


def write_github_outputs(path: Path | None, values: Mapping[str, object]) -> None:
    if path is None:
        return
    with path.open("a", encoding="utf-8", newline="\n") as output:
        for key, value in values.items():
            output.write(f"{key}={value}\n")


def evaluate_control(args: argparse.Namespace) -> dict[str, object]:
    event = json.loads(args.event.read_text(encoding="utf-8"))
    if not isinstance(event, Mapping):
        raise AdapterError("Event payload is malformed")
    identity = event_identity(event)
    trusted_root = args.trusted_root.resolve()
    repository_id = positive_integer(args.repository_id, "Repository ID")
    client = GitHubClient(args.repository, args.token, args.api_url)
    trusted = verify_trusted_context(
        client,
        repository_id,
        trusted_root,
        args.trusted_sha,
        args.workflow_ref,
    )
    pull_request = client.pull_request(int(identity["number"]))
    proposal = proposal_from_pull_request(
        int(identity["number"]), pull_request, args.repository, repository_id
    )
    if not event_matches_proposal(identity, proposal):
        raise ObsoleteEvent("Event identity is obsolete for the current pull request")

    repository = args.git_repository.resolve()
    if require_sha(git_output(repository, ["rev-parse", "HEAD"]), "Proposal checkout") != proposal["head_sha"]:
        raise ObsoleteEvent("Proposal data checkout is stale for the current pull request")
    git_output(repository, ["cat-file", "-e", f"{proposal['base_sha']}^{{commit}}"])

    authorization_error = ""
    try:
        scopes, records, fingerprint = verified_authorization(
            client, proposal, pull_request
        )
    except AdapterError as error:
        scopes, records, fingerprint = [], [], ""
        authorization_error = str(error)

    checker_code, boundary = run_checker(trusted_root, repository, proposal, scopes)
    boundary["proposal"] = proposal
    boundary["repository"] = {
        "full_name": args.repository,
        "id": repository_id,
    }
    boundary["authorization_records"] = records
    boundary["authorization_fingerprint"] = fingerprint
    boundary["trusted_infrastructure"] = trusted
    if authorization_error:
        boundary["allowed"] = False
        boundary["errors"] = [*list(boundary.get("errors", [])), authorization_error]
    apply_enforcement_isolation(boundary)
    if checker_code != 0:
        boundary["allowed"] = False

    selection = select_lanes(boundary)
    workflow_blob = target_workflow_blob(repository, proposal)
    evidence, expected = build_validation_evidence(
        client, proposal, repository_id, workflow_blob
    )
    state, errors = evaluate_trusted_gate(boundary, selection, evidence, expected)

    # Recheck mutable GitHub state before handing a decision to the reporting job.
    verify_trusted_context(
        client,
        repository_id,
        trusted_root,
        args.trusted_sha,
        args.workflow_ref,
    )
    refreshed_pr = client.pull_request(int(proposal["number"]))
    refreshed_proposal = proposal_from_pull_request(
        int(proposal["number"]), refreshed_pr, args.repository, repository_id
    )
    if refreshed_proposal != proposal:
        raise ObsoleteEvent("Pull request changed during trusted evaluation")
    try:
        _, refreshed_records, refreshed_fingerprint = verified_authorization(
            client, refreshed_proposal, refreshed_pr
        )
    except AdapterError as error:
        refreshed_records, refreshed_fingerprint = [], ""
        if not authorization_error:
            state, errors = "failure", [str(error)]
    stale_errors: list[str] = []
    if refreshed_fingerprint != fingerprint or refreshed_records != records:
        stale_errors.append("Scope authorization changed during evaluation")
    refreshed_evidence, refreshed_expected = build_validation_evidence(
        client, refreshed_proposal, repository_id, workflow_blob
    )
    if refreshed_expected != expected:
        stale_errors.append("Validation workflow identity changed during evaluation")
    if stale_errors:
        state, errors = "failure", stale_errors
    else:
        evidence = refreshed_evidence
        state, errors = evaluate_trusted_gate(boundary, selection, evidence, expected)

    decision: dict[str, object] = {
        "state": state,
        "errors": errors,
        "context": STATUS_CONTEXT,
        "repository": args.repository,
        "repository_id": repository_id,
        "proposal": proposal,
        "trusted_infrastructure": {
            **trusted,
            "control_run_id": positive_integer(args.control_run_id, "Control run ID"),
            "control_run_attempt": positive_integer(
                args.control_run_attempt, "Control run attempt"
            ),
            "control_url": args.control_url,
        },
        "boundary": boundary,
        "selection": selection,
        "expected_validation": expected,
        "validation": evidence,
        "authorization_fingerprint": fingerprint,
    }
    fingerprint_value = decision_fingerprint(decision)
    decision["evidence_fingerprint"] = fingerprint_value
    decision["description"] = decision_description(state, errors, fingerprint_value)
    return decision


def report_control(args: argparse.Namespace, decision: Mapping[str, object]) -> str:
    repository = str(decision.get("repository", ""))
    repository_id = positive_integer(decision.get("repository_id"), "Repository ID")
    if repository != args.repository:
        raise AdapterError("Decision repository does not match the reporting context")
    if decision_fingerprint(decision) != decision.get("evidence_fingerprint"):
        raise AdapterError("Decision evidence fingerprint is invalid")
    trusted = decision.get("trusted_infrastructure")
    proposal = decision.get("proposal")
    boundary = decision.get("boundary")
    selection = decision.get("selection")
    expected = decision.get("expected_validation")
    if not all(isinstance(value, Mapping) for value in (trusted, proposal, boundary, selection, expected)):
        raise AdapterError("Decision evidence is incomplete")
    if trusted.get("control_run_id") != positive_integer(args.control_run_id, "Control run ID"):
        raise AdapterError("Decision belongs to a different trusted control run")
    if trusted.get("control_run_attempt") != positive_integer(
        args.control_run_attempt, "Control run attempt"
    ):
        raise AdapterError("Decision belongs to a different trusted control attempt")
    client = GitHubClient(args.repository, args.token, args.api_url)
    verify_trusted_context(
        client,
        repository_id,
        args.trusted_root.resolve(),
        args.trusted_sha,
        args.workflow_ref,
    )
    pull_request = client.pull_request(int(proposal["number"]))
    current = proposal_from_pull_request(
        int(proposal["number"]), pull_request, args.repository, repository_id
    )
    if current["head_sha"] != proposal["head_sha"]:
        raise ObsoleteEvent("Reporting decision is obsolete for a newer PR head")

    state = str(decision.get("state", "error"))
    errors = [str(value) for value in decision.get("errors", [])]
    stale_errors: list[str] = []
    if current != proposal:
        stale_errors.append("Pull request target changed after evaluation")
    try:
        _, records, fingerprint = verified_authorization(client, current, pull_request)
    except AdapterError as error:
        records, fingerprint = [], ""
        state, errors = "failure", [str(error)]
    if fingerprint != decision.get("authorization_fingerprint"):
        stale_errors.append("Scope authorization is stale or was changed")
    elif records != boundary.get("authorization_records"):
        stale_errors.append("Scope authorization evidence changed")

    evidence, refreshed_expected = build_validation_evidence(
        client,
        current,
        repository_id,
        str(expected.get("target_workflow_blob", "")),
    )
    if refreshed_expected != expected:
        stale_errors.append("Validation workflow identity changed before reporting")
    if stale_errors:
        state, errors = "failure", stale_errors
    else:
        state, errors = evaluate_trusted_gate(boundary, selection, evidence, expected)

    report_record = {
        "state": state,
        "errors": errors,
        "proposal": current,
        "authorization_fingerprint": fingerprint,
        "expected_validation": expected,
        "validation": evidence,
        "trusted_infrastructure": trusted,
    }
    fingerprint_value = hashlib.sha256(
        stable_json(report_record).encode("utf-8")
    ).hexdigest()
    description = decision_description(state, errors, fingerprint_value)
    client.create_status(
        str(current["head_sha"]),
        state=state,
        description=description,
        context=STATUS_CONTEXT,
        target_url=str(trusted.get("control_url", "")),
    )
    print(
        json.dumps(
            {
                "reported": True,
                "state": state,
                "head_sha": current["head_sha"],
                "context": STATUS_CONTEXT,
                "description": description,
                "evidence_fingerprint": fingerprint_value,
            },
            indent=2,
            sort_keys=True,
        )
    )
    return state


def report_missing_evaluation(args: argparse.Namespace) -> str:
    if args.event is None:
        raise AdapterError("Event path is required for fail-closed reporting")
    event = json.loads(args.event.read_text(encoding="utf-8"))
    if not isinstance(event, Mapping):
        raise AdapterError("Event payload is malformed")
    identity = event_identity(event)
    repository_id = positive_integer(args.repository_id, "Repository ID")
    client = GitHubClient(args.repository, args.token, args.api_url)
    verify_trusted_context(
        client,
        repository_id,
        args.trusted_root.resolve(),
        args.trusted_sha,
        args.workflow_ref,
        require_all_files=False,
    )
    pull_request = client.pull_request(int(identity["number"]))
    proposal = proposal_from_pull_request(
        int(identity["number"]), pull_request, args.repository, repository_id
    )
    if not event_matches_proposal(identity, proposal):
        raise ObsoleteEvent("Failed evaluation belongs to an obsolete event")
    client.create_status(
        str(proposal["head_sha"]),
        state="error",
        description="Trusted boundary evaluation produced no verifiable decision",
        context=STATUS_CONTEXT,
        target_url=args.control_url or "",
    )
    print(
        json.dumps(
            {
                "reported": True,
                "state": "error",
                "head_sha": proposal["head_sha"],
                "context": STATUS_CONTEXT,
            },
            indent=2,
            sort_keys=True,
        )
    )
    return "error"


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--phase", choices=("event", "evaluate", "report"), required=True)
    parser.add_argument("--event", type=Path)
    parser.add_argument("--repository")
    parser.add_argument("--repository-id")
    parser.add_argument("--token", default=os.environ.get("GH_TOKEN", ""))
    parser.add_argument("--api-url", default=os.environ.get("GITHUB_API_URL", "https://api.github.com"))
    parser.add_argument("--git-repository", type=Path)
    parser.add_argument("--trusted-root", type=Path)
    parser.add_argument("--trusted-sha")
    parser.add_argument("--workflow-ref")
    parser.add_argument("--control-run-id")
    parser.add_argument("--control-run-attempt")
    parser.add_argument("--control-url")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--github-output", type=Path)
    parser.add_argument("--decision-json", default=os.environ.get("DECISION_JSON", ""))
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        if args.phase == "event":
            if args.event is None:
                raise AdapterError("Event path is required")
            event = json.loads(args.event.read_text(encoding="utf-8"))
            if not isinstance(event, Mapping):
                raise AdapterError("Event payload is malformed")
            identity = event_identity(event)
            write_github_outputs(
                args.github_output,
                {"pr_number": identity["number"], "event_kind": identity["event_kind"]},
            )
            print(json.dumps(identity, indent=2, sort_keys=True))
            return 0

        required = {
            "repository": args.repository,
            "repository_id": args.repository_id,
            "trusted_root": args.trusted_root,
            "trusted_sha": args.trusted_sha,
            "workflow_ref": args.workflow_ref,
        }
        missing = [name for name, value in required.items() if not value]
        if missing:
            raise AdapterError("Missing required arguments: " + ", ".join(missing))

        if args.phase == "evaluate":
            evaluate_required = {
                "event": args.event,
                "git_repository": args.git_repository,
                "output": args.output,
                "control_run_id": args.control_run_id,
                "control_run_attempt": args.control_run_attempt,
                "control_url": args.control_url,
            }
            missing = [name for name, value in evaluate_required.items() if not value]
            if missing:
                raise AdapterError("Missing evaluation arguments: " + ", ".join(missing))
            decision = evaluate_control(args)
            args.output.write_text(
                json.dumps(decision, indent=2, sort_keys=True) + "\n", encoding="utf-8"
            )
            write_github_outputs(
                args.github_output,
                {
                    "report": "true",
                    "decision_json": stable_json(decision),
                    "state": decision["state"],
                    "head_sha": decision["proposal"]["head_sha"],
                },
            )
            print(json.dumps(decision, indent=2, sort_keys=True))
            return 0

        if not args.decision_json:
            state = report_missing_evaluation(args)
            return 0 if state == "success" else 2
        decision = json.loads(args.decision_json)
        if not isinstance(decision, Mapping):
            raise AdapterError("Decision JSON is malformed")
        state = report_control(args, decision)
        return 0 if state == "success" else 2
    except ObsoleteEvent as error:
        print(f"trusted boundary ignored obsolete event: {error}", file=sys.stderr)
        write_github_outputs(args.github_output, {"report": "false"})
        return 0
    except (AdapterError, OSError, ValueError, json.JSONDecodeError) as error:
        print(f"trusted boundary adapter failed closed: {error}", file=sys.stderr)
        return 3


if __name__ == "__main__":
    raise SystemExit(main())
