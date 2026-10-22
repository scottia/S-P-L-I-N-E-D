"""Cross-platform selective ``.spl`` backup/restore.

Unprotected envelopes are byte-compatible with the Windows BackupService v1
format. Password-protected Windows backups are detected and rejected clearly;
Python never pretends to decrypt or silently weakens them.
"""

from __future__ import annotations

import base64
from contextlib import closing, contextmanager
import hashlib
import hmac
import json
import os
import sqlite3
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable


FORMAT = "SPLINED-BACKUP"
VERSION = 1
COMPONENTS = frozenset({"settings", "interface", "credentials", "database", "diagnostics"})


@contextmanager
def _database_client_lock(path: Path, shared: bool):
    if not shared:
        yield
        return
    from splined_media_database import _claim_shared_database, _release_shared_database

    path.parent.mkdir(parents=True, exist_ok=True)
    lock_path = _claim_shared_database(path)
    try:
        yield
    finally:
        _release_shared_database(lock_path)


def _verify_sqlite(path: Path) -> None:
    with closing(sqlite3.connect(path.resolve().as_uri() + "?mode=ro", uri=True)) as connection:
        result = connection.execute("PRAGMA quick_check").fetchone()
        if result is None or result[0] != "ok":
            raise ValueError("The backup database failed SQLite integrity validation.")


def _database_snapshot_bytes(path: Path, shared: bool) -> bytes:
    with _database_client_lock(path, shared), tempfile.TemporaryDirectory() as directory:
        snapshot = Path(directory) / "splined.sqlite"
        with closing(sqlite3.connect(path.resolve().as_uri() + "?mode=ro", uri=True)) as source:
            with closing(sqlite3.connect(snapshot)) as destination:
                source.backup(destination)
        _verify_sqlite(snapshot)
        return snapshot.read_bytes()


def _atomic_bytes(path: Path, content: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temp_name = tempfile.mkstemp(
        prefix=f".{path.name}.", suffix=".tmp", dir=path.parent
    )
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(content)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temp_name, path)
    except Exception:
        try:
            os.unlink(temp_name)
        except OSError:
            pass
        raise


def _selection(values: Iterable[str]) -> set[str]:
    selected = {str(value).strip().casefold() for value in values if str(value).strip()}
    unknown = selected.difference(COMPONENTS)
    if unknown:
        raise ValueError(f"Unknown backup component(s): {', '.join(sorted(unknown))}")
    if not selected:
        raise ValueError("Select at least one backup component.")
    return selected


def export_backup(
    destination: Path,
    *,
    config_path: Path,
    credential_dir: Path,
    database_path: Path,
    splined_version: str,
    components: Iterable[str],
    interface_path: Path | None = None,
    diagnostics: str = "",
    shared: bool = False,
) -> Path:
    selected = _selection(components)
    path = Path(destination)
    if path.suffix.casefold() != ".spl":
        path = path.with_suffix(path.suffix + ".spl" if path.suffix else ".spl")
    payload: dict[str, Any] = {
        "version": VERSION,
        "created_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "splined_version": splined_version,
        "settings": config_path.read_text(encoding="utf-8") if "settings" in selected else None,
        "interface_settings": (
            interface_path.read_text(encoding="utf-8")
            if "interface" in selected and interface_path is not None and interface_path.is_file()
            else None
        ),
        "credentials": {},
        "database": None,
        "diagnostics": diagnostics if "diagnostics" in selected else None,
    }
    if "credentials" in selected and credential_dir.is_dir():
        credentials: dict[str, str] = {}
        for item in sorted(credential_dir.glob("*.json"), key=lambda value: value.name.casefold()):
            if item.is_file() and not item.is_symlink():
                credentials[item.name] = base64.b64encode(item.read_bytes()).decode("ascii")
        payload["credentials"] = credentials
    if "database" in selected and database_path.is_file() and not database_path.is_symlink():
        payload["database"] = base64.b64encode(
            _database_snapshot_bytes(database_path, shared)
        ).decode("ascii")

    plain = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    envelope = {
        "format": FORMAT,
        "version": VERSION,
        "password_protected": False,
        "iterations": 0,
        "salt": None,
        "iv": None,
        "payload": base64.b64encode(plain).decode("ascii"),
        "authentication": base64.b64encode(hashlib.sha256(plain).digest()).decode("ascii"),
    }
    _atomic_bytes(
        path,
        json.dumps(envelope, ensure_ascii=False, separators=(",", ":")).encode("utf-8"),
    )
    return path


def read_backup(path: Path) -> dict[str, Any]:
    envelope = json.loads(Path(path).read_text(encoding="utf-8"))
    if not isinstance(envelope, dict) or envelope.get("format") != FORMAT or envelope.get("version") != VERSION:
        raise ValueError("This is not a supported SPLINED .spl backup.")
    if bool(envelope.get("password_protected")):
        raise ValueError(
            "This .spl backup is password protected. Restore it with the Windows client, or export an unprotected cross-platform backup."
        )
    try:
        plain = base64.b64decode(str(envelope.get("payload") or ""), validate=True)
        expected = base64.b64decode(str(envelope.get("authentication") or ""), validate=True)
    except (ValueError, TypeError) as exc:
        raise ValueError("The SPLINED backup envelope is damaged.") from exc
    if not hmac.compare_digest(expected, hashlib.sha256(plain).digest()):
        raise ValueError("The SPLINED backup failed its integrity check.")
    try:
        payload = json.loads(plain.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ValueError("The SPLINED backup payload is invalid.") from exc
    if not isinstance(payload, dict) or payload.get("version") != VERSION:
        raise ValueError("The SPLINED backup payload is invalid.")
    return payload


def restore_backup(
    source: Path,
    *,
    components: Iterable[str],
    config_path: Path,
    credential_dir: Path,
    database_path: Path,
    interface_path: Path | None = None,
    shared: bool = False,
) -> set[str]:
    selected = _selection(components)
    payload = read_backup(source)
    settings_bytes = (
        payload["settings"].encode("utf-8")
        if "settings" in selected
        and isinstance(payload.get("settings"), str)
        and payload["settings"].strip()
        else None
    )
    interface_bytes = (
        payload["interface_settings"].encode("utf-8")
        if "interface" in selected
        and interface_path is not None
        and isinstance(payload.get("interface_settings"), str)
        and payload["interface_settings"].strip()
        else None
    )
    decoded_credentials: dict[str, bytes] = {}
    credentials = payload.get("credentials")
    if "credentials" in selected and isinstance(credentials, dict):
        for name, encoded in credentials.items():
            safe_name = Path(str(name)).name
            if safe_name != str(name) or not safe_name.casefold().endswith(".json"):
                raise ValueError("Backup contains an unsafe credential filename.")
            try:
                decoded_credentials[safe_name] = base64.b64decode(
                    str(encoded), validate=True
                )
            except (ValueError, TypeError) as exc:
                raise ValueError(
                    f"Backup credential {safe_name!r} is damaged."
                ) from exc
    database_bytes: bytes | None = None
    database = payload.get("database")
    if "database" in selected and isinstance(database, str) and database:
        try:
            database_bytes = base64.b64decode(database, validate=True)
        except (ValueError, TypeError) as exc:
            raise ValueError("The backup database payload is damaged.") from exc

    database_staging = tempfile.TemporaryDirectory() if database_bytes is not None else None
    try:
        if database_staging is not None:
            staged_database = Path(database_staging.name) / "splined.sqlite"
            staged_database.write_bytes(database_bytes)
            _verify_sqlite(staged_database)

        # Validate every selected payload before writing any destination. SQLite's
        # online backup API restores the database safely even when WAL is active.
        with _database_client_lock(database_path, shared and database_bytes is not None):
            restored: set[str] = set()
            if settings_bytes is not None:
                _atomic_bytes(config_path, settings_bytes)
                restored.add("settings")
            if interface_bytes is not None and interface_path is not None:
                _atomic_bytes(interface_path, interface_bytes)
                restored.add("interface")
            if "credentials" in selected and isinstance(credentials, dict):
                for safe_name, content in decoded_credentials.items():
                    _atomic_bytes(credential_dir / safe_name, content)
                restored.add("credentials")
            if database_staging is not None:
                database_path.parent.mkdir(parents=True, exist_ok=True)
                with closing(sqlite3.connect(staged_database)) as source:
                    with closing(sqlite3.connect(database_path)) as destination:
                        source.backup(destination)
                restored.add("database")
            if "diagnostics" in selected and payload.get("diagnostics") is not None:
                restored.add("diagnostics")
            return restored
    finally:
        if database_staging is not None:
            database_staging.cleanup()
