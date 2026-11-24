"""Fail-closed reader for the shared Windows/WebGUI lifecycle contract.

The policy is a local behavioral contract.  Loading it performs no database,
network, provider, filesystem-inventory, or policy-persistence work.
"""

from __future__ import annotations

from functools import lru_cache
import json
from typing import Any

from webgui import source_root


DEFAULT_POLICY = source_root() / "schema" / "lifecycle-policy.json"
POLICY_ID = "splined-shared-lifecycle-v1"
RUNTIMES = {"windows", "webgui"}
REQUIREMENT_IDS = tuple(f"LIFE-{number:03d}" for number in range(1, 10))


def validate_lifecycle_policy(value: object) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ValueError("Lifecycle policy must be a JSON object")
    if value.get("schema_version") != 1:
        raise ValueError("unsupported lifecycle policy schema_version")
    if value.get("policy_id") != POLICY_ID:
        raise ValueError("unexpected lifecycle policy_id")
    if set(value.get("runtimes", [])) != RUNTIMES:
        raise ValueError("lifecycle policy must apply to Windows and WebGUI")

    scope = value.get("application_scope")
    if not isinstance(scope, dict) or set(scope) != RUNTIMES:
        raise ValueError("lifecycle policy application_scope is invalid")
    if any(not isinstance(scope[name], str) or not scope[name].strip() for name in RUNTIMES):
        raise ValueError("lifecycle policy application_scope labels are required")

    requirements = value.get("requirements")
    if not isinstance(requirements, list):
        raise ValueError("lifecycle policy requirements are required")
    ids = tuple(rule.get("id") for rule in requirements if isinstance(rule, dict))
    if ids != REQUIREMENT_IDS or len(requirements) != len(REQUIREMENT_IDS):
        raise ValueError("lifecycle policy requires ordered LIFE-001..009")
    for rule in requirements:
        rule_id = rule["id"]
        if set(rule.get("applies_to", [])) != RUNTIMES:
            raise ValueError(f"{rule_id} must apply to Windows and WebGUI")
        if rule.get("webgui_refinement") != rule_id.replace("LIFE-", "WEB-LIFE-"):
            raise ValueError(f"{rule_id} has an invalid WebGUI refinement")
        for field in ("title", "rule"):
            if not isinstance(rule.get(field), str) or not rule[field].strip():
                raise ValueError(f"{rule_id} {field} is required")
        acceptance = rule.get("acceptance")
        if (
            not isinstance(acceptance, list)
            or len(acceptance) != 3
            or any(not isinstance(item, str) or not item.strip() for item in acceptance)
        ):
            raise ValueError(f"{rule_id} requires three acceptance statements")
    return value


@lru_cache(maxsize=1)
def lifecycle_policy() -> dict[str, Any]:
    """Load and validate the exact checked-in policy once per process."""
    try:
        value = json.loads(DEFAULT_POLICY.read_text(encoding="utf-8"))
        return validate_lifecycle_policy(value)
    except (OSError, json.JSONDecodeError, ValueError, TypeError) as error:
        raise RuntimeError(f"Unable to load shared lifecycle policy: {error}") from error


def lifecycle_requirement(requirement_id: str) -> dict[str, Any]:
    """Return one validated rule for native behavior/source binding."""
    if requirement_id not in REQUIREMENT_IDS:
        raise KeyError(f"Unknown lifecycle requirement: {requirement_id}")
    return next(
        rule for rule in lifecycle_policy()["requirements"]
        if rule["id"] == requirement_id
    )
