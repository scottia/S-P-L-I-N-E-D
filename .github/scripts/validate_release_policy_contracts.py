"""Read-only pre-release check of the five canonical /schema contracts.

Use the actual WebGUI policy validators rather than inventing another policy
definition. This checks checked-in sources only; it never loads or seeds an
installed credential-directory policy or a PostgreSQL singleton row.
"""
from __future__ import annotations

import json
import sqlite3
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "build" / "WEBGui"))

from webgui.comp_report_policy import validate_comp_report_policy
from webgui.lifecycle_policy import validate_lifecycle_policy
from webgui.media_state_policy import validate_media_state_policy

CONTRACTS = ROOT / "schema"


def _read_json(filename: str) -> dict:
    value = json.loads((CONTRACTS / filename).read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{filename} must be a JSON object")
    return value


def check_behavioural_policies() -> None:
    media = _read_json("media-state-classification.json")
    if media.get("policy_id") != "splined-media-state-v2":
        raise ValueError("unexpected canonical media-state policy ID")
    validate_media_state_policy(media)

    comp = _read_json("comp-report-policy.json")
    validate_comp_report_policy(comp)

    lifecycle = _read_json("lifecycle-policy.json")
    validate_lifecycle_policy(lifecycle)


def check_sqlite_index_schema() -> None:
    """Exercise only an in-memory SQLite database; never open splined.db."""
    schema = (CONTRACTS / "media.sql").read_text(encoding="utf-8")
    with sqlite3.connect(":memory:") as database:
        database.executescript(schema)
        tables = {
            row[0] for row in database.execute(
                "SELECT name FROM sqlite_master WHERE type='table'"
            )
        }
        if not {"artists", "albums", "tracks"}.issubset(tables):
            raise ValueError("SQLite catalog schema lacks Artist/Album/Track tables")
        if database.execute("PRAGMA integrity_check").fetchone()[0] != "ok":
            raise ValueError("in-memory SQLite schema integrity check failed")


def check_settings_ui_contract() -> None:
    settings = _read_json("settings-ui.json")
    if settings.get("contract_version") != 1 or settings.get("config_version") != 5:
        raise ValueError("unexpected settings/UI contract version")
    if not isinstance(settings.get("providers"), list) or not settings["providers"]:
        raise ValueError("settings/UI providers are required")
    sections = settings.get("sections")
    if not isinstance(sections, list) or not sections:
        raise ValueError("settings/UI sections are required")
    section_ids: set[str] = set()
    field_paths: set[str] = set()
    for section in sections:
        if not isinstance(section, dict) or not section.get("id"):
            raise ValueError("invalid settings/UI section")
        if section["id"] in section_ids:
            raise ValueError("duplicate settings/UI section")
        section_ids.add(section["id"])
        if not isinstance(section.get("fields"), list):
            raise ValueError("settings/UI section fields are required")
        for field in section["fields"]:
            if not isinstance(field, dict) or not field.get("path"):
                raise ValueError("invalid settings/UI field")
            if field["path"] in field_paths:
                raise ValueError(f"duplicate settings/UI field: {field['path']}")
            field_paths.add(field["path"])
            low, high = field.get("min"), field.get("max")
            if low is not None and high is not None and low > high:
                raise ValueError(f"inverted settings/UI bounds: {field['path']}")


def check_all() -> None:
    check_behavioural_policies()
    check_sqlite_index_schema()
    check_settings_ui_contract()


if __name__ == "__main__":
    check_all()
    print("PASS: canonical media-state, COMP, lifecycle, SQLite schema and Config v5 contracts")
