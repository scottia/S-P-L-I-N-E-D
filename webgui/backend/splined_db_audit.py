"""Per-machine SQLite activity log without SQL values or credentials.

The trace callback reports issued statements, not proof of commit. SQLite's
transaction log remains authoritative; this file is for comparing Python and
Windows access to a shared database during an operational run.
"""

from __future__ import annotations

import atexit
from datetime import datetime, timezone
import os
from pathlib import Path
import re
import socket
import sqlite3
import threading
import time


_LOCK = threading.Lock()
_HANDLE = None
_EXPECTED: Path | None = None
_PATH: Path | None = None
_HOST = re.sub(r"[^A-Za-z0-9_-]", "_", socket.gethostname()) or "unknown"
_TABLE = r"([\w.\[\]\"`]+)"
_STATEMENT = re.compile(
    rf"^\s*(?:INSERT\s+(?:OR\s+\w+\s+)?INTO|REPLACE\s+INTO|UPDATE|DELETE\s+FROM|CREATE\s+(?:TEMP\s+)?TABLE(?:\s+IF\s+NOT\s+EXISTS)?|DROP\s+TABLE(?:\s+IF\s+EXISTS)?|ALTER\s+TABLE)\s+{_TABLE}",
    re.IGNORECASE,
)


def _write(event: str, **values: object) -> None:
    with _LOCK:
        if _HANDLE is None:
            return
        stamp = datetime.now(timezone.utc).isoformat(timespec="milliseconds")
        fields = " ".join(f"{key}={str(value)!r}" for key, value in values.items())
        _HANDLE.write(f"{stamp} {event} {fields}\n")
        _HANDLE.flush()


def configure(log_directory: Path, expected_database: Path, *, shared: bool, retention_days: int = 14) -> Path:
    """Create a persistent audit file outside the run-log cleanup directory."""
    global _HANDLE, _EXPECTED, _PATH
    directory = Path(log_directory) / "db-audit"
    if directory.exists() and (directory.is_symlink() or not directory.is_dir()):
        raise OSError(f"Unsafe SPLINED database audit directory: {directory}")
    directory.mkdir(parents=True, exist_ok=True)
    if retention_days > 0:
        cutoff = time.time() - retention_days * 86_400
        for prior in directory.glob("*-python-db-*.log"):
            if prior.is_file() and not prior.is_symlink() and prior.stat().st_mtime < cutoff:
                prior.unlink()
    stamp = time.strftime("%Y%m%d-%H%M%S")
    path = directory / f"[{_HOST}]-python-db-{stamp}-{time.time_ns() % 1_000_000_000:09d}-p{os.getpid()}.log"
    with _LOCK:
        if _HANDLE is not None:
            _HANDLE.close()
        _HANDLE = path.open("x", encoding="utf-8", buffering=1)
        _EXPECTED = Path(expected_database).resolve()
        _PATH = path
    _write("session", machine=_HOST, pid=os.getpid(), expected_db=_EXPECTED, shared=shared)
    return path


def current_log_path() -> Path | None:
    return _PATH


def record_operation(operation: str, database: Path) -> None:
    """Record a non-SQL SQLite write, such as the online backup API."""
    actual = Path(database).resolve()
    _write("operation", database=actual, expected=actual == _EXPECTED, operation=operation)


def audit_connection(connection: sqlite3.Connection, database: Path) -> sqlite3.Connection:
    if _HANDLE is None:
        return connection
    actual = Path(database).resolve()
    expected = _EXPECTED
    _write("open", database=actual, expected=actual == expected)

    def trace(statement: str) -> None:
        stripped = statement.lstrip()
        upper = stripped.split(None, 1)[0].upper() if stripped else ""
        if upper in {"BEGIN", "COMMIT", "ROLLBACK"}:
            _write("transaction", database=actual, operation=upper)
            return
        match = _STATEMENT.match(statement)
        if match is not None:
            # Never log expanded SQL: it can contain filesystem paths and
            # values from credential tables. Operation and table suffice.
            table = match.group(1).strip('[]`"')[:80]
            _write("write_attempt", database=actual, operation=upper, table=table)

    connection.set_trace_callback(trace)
    return connection


def connect_sqlite(database: Path, *args: object, **kwargs: object) -> sqlite3.Connection:
    connection = sqlite3.connect(database, *args, **kwargs)
    return audit_connection(connection, Path(database))


def close() -> None:
    global _HANDLE
    with _LOCK:
        if _HANDLE is not None:
            _HANDLE.close()
            _HANDLE = None


atexit.register(close)
