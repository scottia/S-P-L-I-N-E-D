"""Reproducible PR #170 PostgreSQL batch/recovery benchmark.

The media root is read-only. Every run creates and drops a disposable schema on
the already configured test PostgreSQL service.
"""

from __future__ import annotations

import argparse
from contextlib import closing
import ctypes
import json
import os
from pathlib import Path
import tempfile
import time
import tomllib
from unittest.mock import patch
from urllib.parse import parse_qsl, quote, urlencode, urlsplit, urlunsplit
from uuid import uuid4

import psycopg
from psycopg import sql
import tomlkit

from webgui.database import Database
from webgui.runtime import Runtime
from webgui.services import Services
from webgui import index_coordinator


class ProcessMemoryCounters(ctypes.Structure):
    _fields_ = [
        ("cb", ctypes.c_ulong),
        ("PageFaultCount", ctypes.c_ulong),
        ("PeakWorkingSetSize", ctypes.c_size_t),
        ("WorkingSetSize", ctypes.c_size_t),
        ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPagedPoolUsage", ctypes.c_size_t),
        ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
        ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
        ("PagefileUsage", ctypes.c_size_t),
        ("PeakPagefileUsage", ctypes.c_size_t),
    ]


def memory() -> tuple[int, int]:
    counters = ProcessMemoryCounters()
    counters.cb = ctypes.sizeof(counters)
    get_process = ctypes.windll.kernel32.GetCurrentProcess
    get_process.restype = ctypes.c_void_p
    get_memory = ctypes.windll.psapi.GetProcessMemoryInfo
    get_memory.argtypes = [
        ctypes.c_void_p,
        ctypes.POINTER(ProcessMemoryCounters),
        ctypes.c_ulong,
    ]
    get_memory.restype = ctypes.c_int
    process = get_process()
    read = get_memory(
        process, ctypes.byref(counters), counters.cb
    )
    if not read:
        return 0, 0
    return int(counters.WorkingSetSize), int(counters.PeakWorkingSetSize)


def schema_url(base_url: str, schema: str) -> str:
    parsed = urlsplit(base_url)
    parameters = dict(parse_qsl(parsed.query))
    parameters["options"] = "-c search_path=" + schema
    return urlunsplit(
        parsed._replace(query=urlencode(parameters, quote_via=quote))
    )


def run(
    music: Path, batch_size: int, base_url: str, *, adaptive: bool = False
) -> dict[str, object]:
    schema = "splined_pr170_benchmark_" + uuid4().hex
    with psycopg.connect(base_url, autocommit=True) as connection:
        connection.execute(sql.SQL("CREATE SCHEMA {}").format(sql.Identifier(schema)))
    url = schema_url(base_url, schema)
    temporary = tempfile.TemporaryDirectory(prefix="splined-pr170-postgres-")
    root = Path(temporary.name)
    runtime = None
    try:
        config = tomllib.loads(
            (Path(__file__).parents[1] / "docker" / "config.example.toml").read_text(
                encoding="utf-8"
            )
        )
        config["mode"] = "read"
        config["library"]["music_library"] = str(music)
        config["scan"].update(
            index_batch_size=batch_size,
            index_batch_adaptive=adaptive,
            cache_dir=str(root / "cache"),
            temporary_cache_dir=str(root / "temporary"),
            shared_artist_art_dir=str(root / "artist-art"),
            log_dir=str(root / "logs"),
        )
        config["credentials"]["credential_dir"] = str(root / "credentials")
        config_file = root / "config.toml"
        config_file.write_text(tomlkit.dumps(config), encoding="utf-8")

        database = Database(url)
        database.migrate()
        runtime = Runtime(config_file, database)
        services = Services(runtime)
        baseline_rss, _ = memory()
        original_stage = index_coordinator.stage_batch
        interrupted = {"generation": "", "done": False}

        def interrupt_after_first_batch(database, lease, rows, **kwargs):
            result = original_stage(database, lease, rows, **kwargs)
            if not interrupted["done"]:
                interrupted["done"] = True
                interrupted["generation"] = lease.generation_id
                raise KeyboardInterrupt("intentional benchmark worker interruption")
            return result

        interruption_started = time.perf_counter()
        try:
            with patch.object(
                index_coordinator, "stage_batch", new=interrupt_after_first_batch
            ):
                services.refresh_index(mode="full")
        except KeyboardInterrupt:
            pass
        else:
            raise RuntimeError("benchmark interruption did not occur")
        interruption_seconds = time.perf_counter() - interruption_started

        with closing(database.connect()) as connection, connection:
            staged_before_resume = connection.execute(
                "SELECT count(*) FROM catalog_generation_albums WHERE generation_id=?",
                (interrupted["generation"],),
            ).fetchone()[0]
            live_before_resume = connection.execute(
                "SELECT count(*) FROM albums"
            ).fetchone()[0]
            connection.raw.execute(
                "UPDATE catalog_generations SET lease_expires_at=now()-interval '1 second' "
                "WHERE generation_id=%s",
                (interrupted["generation"],),
            )

        resume_started = time.perf_counter()
        full = services.refresh_index(mode="full")
        resume_seconds = time.perf_counter() - resume_started
        with closing(database.connect()) as connection:
            full_timings = connection.execute(
                "SELECT timings FROM catalog_generations WHERE generation_id=?",
                (full["generation_id"],),
            ).fetchone()[0]
            tracks = connection.execute("SELECT count(*) FROM tracks").fetchone()[0]

        incremental_started = time.perf_counter()
        incremental = services.refresh_index(mode="incremental")
        incremental_seconds = time.perf_counter() - incremental_started
        with closing(database.connect()) as connection:
            incremental_timings = connection.execute(
                "SELECT timings FROM catalog_generations WHERE generation_id=?",
                (incremental["generation_id"],),
            ).fetchone()[0]
        current_rss, peak_rss = memory()
        return {
            "client": "webgui-postgresql",
            "batch_mode": "adaptive" if adaptive else "fixed",
            "batch_size": batch_size,
            "albums": full["albums"],
            "tracks": tracks,
            "interruption_seconds": round(interruption_seconds, 6),
            "staged_before_resume": staged_before_resume,
            "live_before_resume": live_before_resume,
            "resume_seconds": round(resume_seconds, 6),
            "total_index_seconds": round(
                interruption_seconds + resume_seconds, 6
            ),
            "full_timings": full_timings,
            "incremental_seconds": round(incremental_seconds, 6),
            "incremental_timings": incremental_timings,
            "baseline_rss_bytes": baseline_rss,
            "current_rss_bytes": current_rss,
            "peak_rss_bytes": peak_rss,
            "peak_rss_delta_bytes": max(0, peak_rss - baseline_rss),
        }
    finally:
        if runtime is not None:
            runtime.close()
        with psycopg.connect(base_url, autocommit=True) as connection:
            connection.execute(
                sql.SQL("DROP SCHEMA IF EXISTS {} CASCADE").format(
                    sql.Identifier(schema)
                )
            )
        temporary.cleanup()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--music", type=Path, required=True)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--batch-size", type=int, choices=range(1, 65))
    group.add_argument("--adaptive", action="store_true")
    args = parser.parse_args()
    base_url = os.environ.get("SPLINED_TEST_DATABASE_URL", "")
    if not base_url:
        raise SystemExit("SPLINED_TEST_DATABASE_URL is required")
    if not args.music.is_dir():
        raise SystemExit(f"Test media library does not exist: {args.music}")
    print(
        json.dumps(
            run(
                args.music,
                args.batch_size or 16,
                base_url,
                adaptive=args.adaptive,
            ),
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()
