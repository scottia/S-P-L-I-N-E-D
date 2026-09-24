"""Editable Config v5 source-policy draft and safe persistence.

Navigation changes only this in-memory draft.  The engine calls
``persist_policy_draft`` after an explicit Save/Apply response from the TUI.
``tomlkit`` preserves comments, table order, and unrelated/unknown keys.
"""

from __future__ import annotations

import copy
import os
import tempfile
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable

import tomlkit


SOURCE_NAMES = (
    "deezer",
    "itunes",
    "fanarttv",
    "lastfm",
    "coverartarchive",
    "discogs",
)
RANGE_TYPES = (
    "BelowMinimum",
    "LowerRange",
    "Ideal",
    "UpperRange",
    "Ladder",
    "AboveLadder",
)
OPTIONAL_DIMENSIONS = (
    "minimum_short_side",
    "maximum_short_side",
    "minimum_width",
    "minimum_height",
)
# Only these providers expose a differentiated front/primary flag in current
# Python discovery. Other providers return one logical album-cover class.
PRIMARY_METADATA_SOURCES = frozenset({"coverartarchive", "discogs"})


def _section(cfg: dict[str, Any], name: str) -> dict[str, Any]:
    value = cfg.get(name, {})
    return value if isinstance(value, dict) else {}


def _policy_defaults(raw: dict[str, Any]) -> dict[str, Any]:
    return {
        "enabled": bool(raw.get("enabled", True)),
        "source_override": bool(raw.get("source_override", False)),
        "minimum_range_type": str(raw.get("minimum_range_type", "LowerRange")),
        "allow_below_minimum_fallback": bool(
            raw.get("allow_below_minimum_fallback", False)
        ),
        "minimum_short_side": raw.get("minimum_short_side"),
        "maximum_short_side": raw.get("maximum_short_side"),
        "minimum_width": raw.get("minimum_width"),
        "minimum_height": raw.get("minimum_height"),
        "primary_image_only": bool(raw.get("primary_image_only", True)),
    }


@dataclass
class PolicyDraft:
    source_order: list[str]
    policies: dict[str, dict[str, Any]]
    ranges: dict[str, int]
    square_round_to: int
    dirty: bool = False

    @classmethod
    def from_config(cls, cfg: dict[str, Any]) -> "PolicyDraft":
        sources = _section(cfg, "sources")
        order = [
            str(value).strip().lower()
            for value in sources.get("cover_sources", [])
            if str(value).strip().lower() in SOURCE_NAMES
        ]
        for source in SOURCE_NAMES:
            if source not in order:
                order.append(source)
        raw_policies = _section(cfg, "source_policies")
        policies = {
            source: _policy_defaults(
                raw_policies.get(source, {})
                if isinstance(raw_policies.get(source, {}), dict)
                else {}
            )
            for source in SOURCE_NAMES
        }
        ranges = _section(cfg, "range")
        return cls(
            source_order=order,
            policies=policies,
            ranges={
                "min": int(ranges.get("min", 1200)),
                "ideal": int(ranges.get("ideal", 1800)),
                "max": int(ranges.get("max", 2400)),
                "ladder": int(ranges.get("ladder", 3600)),
            },
            square_round_to=int(
                _section(cfg, "output").get("square_round_to", 16) or 0
            ),
        )

    def move(self, source: str, delta: int) -> None:
        index = self.source_order.index(source)
        target = max(0, min(len(self.source_order) - 1, index + delta))
        if target != index:
            self.source_order[index], self.source_order[target] = (
                self.source_order[target],
                self.source_order[index],
            )
            self.dirty = True

    def toggle(self, source: str, key: str) -> None:
        self.policies[source][key] = not bool(self.policies[source][key])
        self.dirty = True

    def cycle_range(self, source: str, delta: int) -> None:
        current = self.policies[source]["minimum_range_type"]
        index = RANGE_TYPES.index(current) if current in RANGE_TYPES else 1
        self.policies[source]["minimum_range_type"] = RANGE_TYPES[
            (index + delta) % len(RANGE_TYPES)
        ]
        self.dirty = True

    def adjust_number(self, source: str | None, key: str, delta: int) -> None:
        if source is None:
            current = self.square_round_to if key == "square_round_to" else self.ranges[key]
            value = max(0 if key == "square_round_to" else 1, int(current) + delta)
            if key == "square_round_to":
                self.square_round_to = value
            else:
                if key == "min":
                    value = min(value, self.ranges["ideal"] - 1)
                elif key == "ideal":
                    value = max(self.ranges["min"] + 1, min(value, self.ranges["max"]))
                elif key == "max":
                    value = max(self.ranges["ideal"], min(value, self.ranges["ladder"] - 1))
                elif key == "ladder":
                    value = max(self.ranges["max"] + 1, value)
                self.ranges[key] = value
        else:
            current = self.policies[source].get(key)
            base = 0 if current is None else int(current)
            value = max(1, base + delta)
            policy = self.policies[source]
            if key == "minimum_short_side" and policy.get("maximum_short_side") is not None:
                value = min(value, int(policy["maximum_short_side"]))
            elif key == "maximum_short_side" and policy.get("minimum_short_side") is not None:
                value = max(value, int(policy["minimum_short_side"]))
            policy[key] = value
        self.dirty = True

    def clear_optional(self, source: str, key: str) -> None:
        if key in OPTIONAL_DIMENSIONS:
            self.policies[source][key] = None
            self.dirty = True

    def effective_preview(self, source: str) -> str:
        policy = self.policies[source]
        if not policy["enabled"]:
            return "DISABLED · provider is not queried"
        if not policy["source_override"]:
            return (
                "GLOBAL · "
                f"{self.ranges['min']} / {self.ranges['ideal']} / "
                f"{self.ranges['max']} / {self.ranges['ladder']}"
            )
        details = [f"minimum {policy['minimum_range_type']}"]
        if policy["allow_below_minimum_fallback"]:
            details.append("adjacent-lower fallback")
        for key, label in (
            ("minimum_short_side", "short≥"),
            ("maximum_short_side", "short≤"),
            ("minimum_width", "width≥"),
            ("minimum_height", "height≥"),
        ):
            if policy.get(key) is not None:
                details.append(f"{label}{policy[key]}")
        if source in PRIMARY_METADATA_SOURCES:
            details.append(
                "primary only" if policy["primary_image_only"] else "all images"
            )
        else:
            details.append("primary metadata not differentiated")
        return "OVERRIDE · " + " · ".join(details)

    def as_payload(self) -> dict[str, Any]:
        return {
            "source_order": list(self.source_order),
            "policies": copy.deepcopy(self.policies),
            "range": dict(self.ranges),
            "square_round_to": self.square_round_to,
        }


def _ensure_table(document: Any, key: str) -> Any:
    if key not in document or not isinstance(document[key], dict):
        document[key] = tomlkit.table()
    return document[key]


def persist_policy_draft(
    path: Path,
    payload: dict[str, Any],
    validator: Callable[[dict[str, Any]], None],
) -> dict[str, Any]:
    """Atomically persist only source/range controls and preserve all else."""
    original = path.read_text(encoding="utf-8")
    document = tomlkit.parse(original)
    if document.get("config_version") != 5:
        raise ValueError("Source settings may only update Config v5 documents.")

    order = [str(value).lower() for value in payload.get("source_order", [])]
    if sorted(order) != sorted(SOURCE_NAMES):
        raise ValueError("Source priority must contain each supported source exactly once.")
    sources = _ensure_table(document, "sources")
    sources["cover_sources"] = order

    policy_payload = payload.get("policies", {})
    policies = _ensure_table(document, "source_policies")
    for source in SOURCE_NAMES:
        values = policy_payload.get(source)
        if not isinstance(values, dict):
            raise ValueError(f"Missing source policy draft for {source}.")
        table = _ensure_table(policies, source)
        for key in (
            "enabled",
            "source_override",
            "minimum_range_type",
            "allow_below_minimum_fallback",
            "primary_image_only",
        ):
            table[key] = values[key]
        for key in OPTIONAL_DIMENSIONS:
            value = values.get(key)
            if value is None:
                table.pop(key, None)
            else:
                table[key] = int(value)

    range_values = payload.get("range", {})
    ranges = _ensure_table(document, "range")
    for key in ("min", "ideal", "max", "ladder"):
        ranges[key] = int(range_values[key])
    output = _ensure_table(document, "output")
    output["square_round_to"] = int(payload.get("square_round_to", 0))

    rendered = tomlkit.dumps(document)
    parsed = tomllib.loads(rendered)
    validator(parsed)

    path.parent.mkdir(parents=True, exist_ok=True)
    mode = path.stat().st_mode if path.exists() else None
    descriptor, temp_name = tempfile.mkstemp(
        prefix=f".{path.name}.", suffix=".tmp", dir=path.parent
    )
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(rendered)
            handle.flush()
            os.fsync(handle.fileno())
        if mode is not None:
            try:
                os.chmod(temp_name, mode)
            except OSError:
                pass
        os.replace(temp_name, path)
    except Exception:
        try:
            os.unlink(temp_name)
        except OSError:
            pass
        raise
    return parsed
