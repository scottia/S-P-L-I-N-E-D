"""Checkpoint completed SQLite builds before Select Media hydration.

A completed first build or explicit refresh may leave most pages in
``splined.db-wal``.  Consolidating those pages before warm hydration makes the
main database safe to stage as one sequential local snapshot and prevents the
one-time post-build handoff from falling back to slow mounted random reads.
"""

from __future__ import annotations

from pathlib import Path
import sqlite3
from typing import Any

import splined_media_index as media_index


_INSTALLED = False


def _wal_size(path: Path) -> int:
    try:
        return int(Path(str(path) + "-wal").stat().st_size)
    except OSError:
        return 0


def install(core: Any) -> None:
    global _INSTALLED
    if _INSTALLED:
        return
    _INSTALLED = True

    original_build = media_index.build_index

    def build_index(context: Any, connection: sqlite3.Connection, reason: str) -> None:
        original_build(context, connection, reason)
        before = _wal_size(Path(context.db_path))
        core.emit_ui(
            "cache_progress",
            phase="commit",
            reason=reason,
            status=(
                "Consolidating completed SQLite index"
                + (f" · {before / 1_048_576:.1f} MiB" if before else "")
            ),
            processed=0,
            total=0,
            percent=100.0,
            albums=0,
            staged=0,
            recovered=0,
            current_artist="",
        )
        try:
            row = connection.execute("PRAGMA wal_checkpoint(TRUNCATE)").fetchone()
            busy, log_pages, checkpointed = (
                (int(row[0]), int(row[1]), int(row[2]))
                if row is not None and len(row) >= 3
                else (0, 0, 0)
            )
            core.debug_log(
                "splined.db.postbuild_checkpoint "
                f"reason={reason!r} busy={busy} log_pages={log_pages} "
                f"checkpointed={checkpointed} before_bytes={before} "
                f"after_bytes={_wal_size(Path(context.db_path))}"
            )
        except sqlite3.Error as exc:
            core.debug_log(
                "splined.db.postbuild_checkpoint_error "
                f"reason={reason!r} error={type(exc).__name__}: {exc}"
            )

    media_index.build_index = build_index
    core._splined_media_checkpoint_policy_installed = True
