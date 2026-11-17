"""Shared Artist and Album name-color classification policy.

The policy is usable by a PostgreSQL-only WebGUI, a SQLite-only Windows
installation, or both clients together. Cross-client coordination contributes
opposing-writer evidence; it is not required to classify local catalog state.
"""

from __future__ import annotations

from functools import lru_cache
import json
import os
from pathlib import Path
import tempfile
from typing import Iterable


POLICY_ENV = "SPLINED_MEDIA_STATE_POLICY"
DEFAULT_POLICY = Path(__file__).resolve().parents[1] / "schema" / "media-state-classification.json"


def _candidate_paths() -> tuple[Path, ...]:
    configured = os.environ.get(POLICY_ENV, "").strip()
    values = []
    if configured:
        values.append(Path(configured))
    config_file = os.environ.get("SPLINED_CONFIG", "").strip()
    if config_file:
        values.append(Path(config_file).parent / "media-state-classification.json")
    values.extend((Path("/config/media-state-classification.json"), DEFAULT_POLICY))
    return tuple(dict.fromkeys(values))


def validate_media_state_policy(value: object) -> dict:
    if not isinstance(value, dict):
        raise ValueError("Media-state policy must be a JSON object")
    if int(value.get("schema_version", 0)) != 1:
        raise ValueError("unsupported schema_version")
    colors = value.get("colors")
    if not isinstance(colors, dict) or not colors:
        raise ValueError("colors are required")
    for name, color in colors.items():
        if not isinstance(name, str) or not isinstance(color, str) or len(color) != 7 or not color.startswith("#"):
            raise ValueError("every color must be a named #RRGGBB value")
        try:
            int(color[1:], 16)
        except ValueError as error:
            raise ValueError(f"invalid color {name}: {color}") from error
    for scope, rule_name in (("artist", "ranked_states"), ("album", "ranked_name_states")):
        section = value.get(scope)
        rules = section.get(rule_name) if isinstance(section, dict) else None
        if not isinstance(rules, list) or not rules:
            raise ValueError(f"{scope} {rule_name} are required")
        if not any(isinstance(rule, dict) and rule.get("condition") == "default" for rule in rules):
            raise ValueError(f"{scope} policy requires a default rule")
        for rule in rules:
            if not isinstance(rule, dict) or not all(rule.get(key) for key in ("condition", "key", "ui_status_key", "label", "color")):
                raise ValueError(f"{scope} contains an incomplete state rule")
            if rule["color"] not in colors:
                raise ValueError(f"{scope} rule references unknown color {rule['color']}")
    status_colors = value.get("ui_status_colors")
    if not isinstance(status_colors, dict) or any(color not in colors for color in status_colors.values()):
        raise ValueError("ui_status_colors must reference named colors")
    dots = value.get("album", {}).get("status_dots")
    if not isinstance(dots, list) or not dots:
        raise ValueError("album status_dots are required")
    for dot in dots:
        if not isinstance(dot, dict) or not dot.get("condition") or not dot.get("label") or dot.get("color") not in colors:
            raise ValueError("album contains an invalid status dot")
    presentation = value.get("override_presentation")
    if (
        not isinstance(presentation, dict)
        or presentation.get("condition") != "opposing_override"
        or presentation.get("frame_color") not in colors
        or presentation.get("status_dot_color") not in colors
    ):
        raise ValueError("override_presentation is required")
    return value


def editable_policy_path() -> Path:
    configured = os.environ.get(POLICY_ENV, "").strip()
    if configured:
        return Path(configured)
    config_file = os.environ.get("SPLINED_CONFIG", "").strip()
    if config_file:
        return Path(config_file).parent / "media-state-classification.json"
    return Path("/config/media-state-classification.json") if Path("/config").is_dir() else DEFAULT_POLICY


def save_media_state_policy(value: object) -> dict:
    policy = validate_media_state_policy(value)
    target = editable_policy_path()
    target.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(
        prefix=".media-state-classification-", suffix=".json", dir=target.parent
    )
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8", newline="\n") as output:
            json.dump(policy, output, indent=2)
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, target)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)
    media_state_policy.cache_clear()
    return media_state_policy()


@lru_cache(maxsize=1)
def media_state_policy() -> dict:
    errors = []
    for path in _candidate_paths():
        try:
            value = json.loads(path.read_text(encoding="utf-8"))
            return validate_media_state_policy(value)
        except (OSError, ValueError, TypeError) as error:
            errors.append(f"{path}: {error}")
    raise RuntimeError("Unable to load media-state classification policy: " + "; ".join(errors))


def classify_media_state(scope: str, conditions: Iterable[str]) -> dict[str, str]:
    policy = media_state_policy()
    section = policy[scope]
    rules = section["ranked_states" if scope == "artist" else "ranked_name_states"]
    evidence = {str(value) for value in conditions}
    selected = next(
        (rule for rule in rules if rule["condition"] == "default" or rule["condition"] in evidence),
        None,
    )
    if selected is None:
        raise RuntimeError(f"{scope} classification has no default rule")
    color_name = str(selected["color"])
    if color_name in set(section.get("forbidden_name_colors", [])):
        raise RuntimeError(f"{scope} classification selected forbidden color {color_name}")
    return {
        "key": str(selected["key"]),
        "ui_status_key": str(selected["ui_status_key"]),
        "label": str(selected["label"]),
        "color_name": color_name,
        "color": str(policy["colors"][color_name]),
    }


def status_color(status_key: str) -> str:
    policy = media_state_policy()
    color_name = policy["ui_status_colors"].get(status_key, "white")
    return str(policy["colors"][color_name])
