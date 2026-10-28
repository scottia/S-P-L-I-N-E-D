"""Structured Config v5 settings draft for the Python Ratatui client."""

from __future__ import annotations

import copy
import os
import tempfile
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable

import tomlkit


@dataclass(frozen=True)
class SettingSpec:
    section: str
    key: str
    label: str
    kind: str
    default: Any
    choices: tuple[str, ...] = ()
    minimum: int = 0
    step: int = 1
    maximum: int | None = None

    @property
    def identity(self) -> str:
        return f"{self.section}.{self.key}"


SETTING_SPECS = (
    SettingSpec("library", "music_library", "Music library", "text", ""),
    SettingSpec("library", "ignored_subs", "Ignored sub-directories", "list", []),
    SettingSpec("scan", "cache_dir", "Persistent cache / SQLite", "text", "_cache"),
    SettingSpec("scan", "shared_artist_art_dir", "V/A / OST Artist images", "text", ""),
    SettingSpec("scan", "temporary_cache_dir", "Temporary candidate cache", "text", "_cache"),
    SettingSpec("scan", "sqlite_shared", "Shared/network SQLite mode", "bool", False),
    SettingSpec("scan", "log_dir", "Run log directory", "text", "_logs"),
    SettingSpec("scan", "scan_mode_timeout", "Auto-scan timeout hours", "int", 24, minimum=0),
    SettingSpec("output", "file_name", "Artwork file name", "text", "cover"),
    SettingSpec("output", "file_formats", "Artwork formats", "list", ["jpeg", "png", "webp"]),
    SettingSpec("output", "preserve_file", "Preserve existing file", "bool", True),
    SettingSpec("output", "square", "Square artwork", "bool", True),
    SettingSpec("output", "square_mode", "Square mode", "choice", "crop", ("off", "crop")),
    SettingSpec("output", "square_round_to", "Square round-to", "int", 16, minimum=0, step=16),
    SettingSpec("output", "upscale_below_ideal", "Upscale below Ideal", "bool", False),
    SettingSpec("output", "upscale_max_percent", "Maximum upscale percent", "int", 200, minimum=100, step=10, maximum=800),
    SettingSpec("output", "evaluate_final_image", "Evaluate projected final image", "bool", True),
    SettingSpec("credentials", "credential_dir", "Credential directory", "text", "credentials"),
    SettingSpec("logging", "retention_days", "Log retention days", "int", 14, minimum=0),
    SettingSpec("logging", "database_audit", "Separate database activity log", "bool", False),
    SettingSpec("history", "enabled", "Completion history", "bool", True),
    SettingSpec("history", "retention_days", "History retention days", "int", 0, minimum=0),
)


@dataclass
class RuntimeSettingsDraft:
    values: dict[str, Any]
    dirty: bool = False

    @classmethod
    def from_config(cls, cfg: dict[str, Any]) -> "RuntimeSettingsDraft":
        values: dict[str, Any] = {}
        for spec in SETTING_SPECS:
            section = cfg.get(spec.section, {})
            raw = section.get(spec.key, spec.default) if isinstance(section, dict) else spec.default
            values[spec.identity] = copy.deepcopy(raw)
        return cls(values)

    def value(self, spec: SettingSpec) -> Any:
        return self.values.get(spec.identity, copy.deepcopy(spec.default))

    def display(self, spec: SettingSpec) -> str:
        value = self.value(spec)
        if spec.kind == "bool":
            return "On" if bool(value) else "Off"
        if spec.kind == "list":
            return ", ".join(str(item) for item in value) if isinstance(value, list) else str(value)
        return str(value)

    def toggle_or_cycle(self, spec: SettingSpec, delta: int = 1) -> None:
        if spec.kind == "bool":
            self.values[spec.identity] = not bool(self.value(spec))
        elif spec.kind == "choice":
            current = str(self.value(spec))
            index = spec.choices.index(current) if current in spec.choices else 0
            self.values[spec.identity] = spec.choices[(index + delta) % len(spec.choices)]
        elif spec.kind == "int":
            current = int(self.value(spec) or 0)
            updated = max(spec.minimum, current + delta * spec.step)
            if spec.maximum is not None:
                updated = min(spec.maximum, updated)
            self.values[spec.identity] = updated
        else:
            return
        self.dirty = True

    def set_text(self, spec: SettingSpec, value: str) -> None:
        if spec.kind == "list":
            self.values[spec.identity] = [
                item.strip() for item in value.split(",") if item.strip()
            ]
        elif spec.kind == "int":
            parsed = int(value)
            if parsed < spec.minimum or (
                spec.maximum is not None and parsed > spec.maximum
            ):
                maximum = (
                    f" and at most {spec.maximum}"
                    if spec.maximum is not None
                    else ""
                )
                raise ValueError(
                    f"{spec.identity} must be at least {spec.minimum}{maximum}."
                )
            self.values[spec.identity] = parsed
        else:
            self.values[spec.identity] = value.strip()
        self.dirty = True

    def as_payload(self) -> dict[str, Any]:
        return copy.deepcopy(self.values)


def _ensure_table(document: Any, key: str) -> Any:
    if key not in document or not isinstance(document[key], dict):
        document[key] = tomlkit.table()
    return document[key]


def persist_runtime_settings(
    path: Path,
    payload: dict[str, Any],
    validator: Callable[[dict[str, Any]], None],
) -> dict[str, Any]:
    """Atomically update only fields exposed by the structured editor."""

    document = tomlkit.parse(path.read_text(encoding="utf-8"))
    if document.get("config_version") != 5:
        raise ValueError("Structured settings may only update Config v5 documents.")
    known = {spec.identity: spec for spec in SETTING_SPECS}
    unknown = set(payload).difference(known)
    if unknown:
        raise ValueError(f"Unknown structured setting(s): {', '.join(sorted(unknown))}")
    for identity, value in payload.items():
        spec = known[identity]
        table = _ensure_table(document, spec.section)
        if spec.kind == "bool":
            table[spec.key] = bool(value)
        elif spec.kind == "int":
            parsed = int(value)
            if parsed < spec.minimum:
                raise ValueError(f"{identity} must be at least {spec.minimum}.")
            if spec.maximum is not None and parsed > spec.maximum:
                raise ValueError(f"{identity} must be at most {spec.maximum}.")
            table[spec.key] = parsed
        elif spec.kind == "list":
            if not isinstance(value, list):
                raise ValueError(f"{identity} must be a list.")
            table[spec.key] = [str(item) for item in value]
        else:
            table[spec.key] = str(value)

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
