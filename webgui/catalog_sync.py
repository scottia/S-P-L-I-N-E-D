"""Durable Windows-authoritative live catalog synchronization.

The transport is intentionally one-way.  Windows publishes complete,
allow-listed entity projections with a monotonic SQLite revision.  WebGUI
applies them only after an explicit immutable-SNAPSHOT baseline has proven the
two catalogs equal.  PostgreSQL-only and derived presentation fields never
travel in this stream.
"""

from __future__ import annotations

import json
import sqlite3
from contextlib import closing
from datetime import datetime, timezone
from hashlib import sha256
from pathlib import Path
from threading import Event, Lock, Thread
from typing import Any, Callable

from psycopg import sql
from psycopg.types.json import Jsonb

from webgui.audit import sanitize_text
from webgui.catalog_authority import (
    TABLE_KEYS,
    WINDOWS_SYNCHRONIZED_FIELDS,
    mark_windows_authority,
)
from webgui.coordination import canonical_album_identity
from webgui.reconciliation import SQLiteReconciler, map_library_path


MODES = frozenset({"live", "manual-snap", "off"})
APPLY_BATCH_LIMIT = 100
PATH_COLUMNS = {
    "artists": frozenset({"primary_path"}),
    "albums": frozenset({"path", "representative_file", "cover_path"}),
    "tracks": frozenset({"path"}),
}
ARTWORK_PROCESSING_COLUMNS = frozenset(
    {
        "cover_found",
        "cover_path",
        "cover_name",
        "cover_format",
        "cover_width",
        "cover_height",
        "artwork_jpeg",
        "artwork_png",
        "artwork_webp",
        "artwork_other",
        "webp_found",
        "webp_size_mb",
        "webp_resolution",
        "webp_conversion",
        "processed_at",
        "selected_source",
    }
)


def _canonical(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), default=str)


def _digest(value: Any) -> str:
    return sha256(_canonical(value).encode("utf-8", "surrogateescape")).hexdigest()


def _is_windows_synchronized_difference(difference: dict[str, Any]) -> bool:
    """Ignore WebGUI-owned presentation/validation values during integrity checks."""
    table = str(difference.get("entity_table") or "")
    allowed = WINDOWS_SYNCHRONIZED_FIELDS.get(table)
    if not allowed:
        return False
    source = difference.get("source_value") or {}
    target = difference.get("target_value") or {}
    return any(source.get(name) != target.get(name) for name in allowed)


class CatalogSynchronizer:
    """Consume the durable PostgreSQL inbox after a proven baseline."""

    def __init__(
        self,
        database,
        reconciler: SQLiteReconciler,
        audit: Callable[..., Any] | None = None,
        *,
        start_worker: bool = True,
    ):
        self.database = database
        self.reconciler = reconciler
        self.audit = audit
        self._stop = Event()
        self._lock = Lock()
        self._thread = None
        if start_worker:
            self._thread = Thread(target=self._worker, name="catalog-sync", daemon=True)
            self._thread.start()

    def close(self) -> None:
        self._stop.set()
        if self._thread:
            self._thread.join(timeout=3)

    def _event(self, action: str, details: dict[str, Any], **fields: Any) -> None:
        if self.audit:
            self.audit(action, "Live catalog synchronization", details, **fields)

    def status(self) -> dict[str, Any]:
        with closing(self.database.connect()) as db:
            settings = db.raw.execute(
                "SELECT * FROM catalog_sync_settings WHERE singleton=TRUE"
            ).fetchone()
            sources = list(
                db.raw.execute(
                    "SELECT source_instance_id,source_label,baseline_state,baseline_fingerprint,"
                    "baseline_revision,last_received_revision,last_applied_revision,source_root,"
                    "target_root,baseline_generation,baseline_change_revision,"
                    "last_integrity_digest,last_integrity_at,last_error,updated_at "
                    "FROM catalog_sync_sources ORDER BY updated_at DESC"
                )
            )
            pending = db.raw.execute(
                "SELECT count(*) AS n FROM catalog_sync_events WHERE status='received'"
            ).fetchone()
            failed = db.raw.execute(
                "SELECT count(*) AS n FROM catalog_sync_events WHERE status IN ('blocked','failed')"
            ).fetchone()
        return {
            "settings": dict(settings) if settings else {"mode": "manual-snap"},
            "sources": [dict(item) for item in sources],
            "pending": int(pending["n"] if pending else 0),
            "failed": int(failed["n"] if failed else 0),
        }

    def save_settings(self, payload: dict[str, Any]) -> dict[str, Any]:
        mode = str(payload.get("mode") or "manual-snap").strip().casefold()
        if mode not in MODES:
            raise ValueError("Catalog Synchronization Mode must be Live Synchronization, Manual SQLite SNAP, or Off")
        source_root = str(payload.get("source_root") or "").strip()
        target_root = str(payload.get("target_root") or self.reconciler.target_root).strip()
        requested_source = str(payload.get("active_source_instance_id") or "").strip()
        if Path(target_root).resolve() != self.reconciler.target_root:
            raise PermissionError("Live catalog target must be the configured WebGUI music library")
        with closing(self.database.connect()) as db, db:
            current = db.raw.execute(
                "SELECT active_source_instance_id FROM catalog_sync_settings WHERE singleton=TRUE"
            ).fetchone()
            active_source = requested_source or str(
                (current or {}).get("active_source_instance_id") or ""
            )
            if mode == "live":
                ready = list(db.raw.execute(
                    "SELECT source_instance_id FROM catalog_sync_sources "
                    "WHERE baseline_state='ready' AND source_root=%s AND target_root=%s "
                    "ORDER BY source_instance_id",
                    (source_root, str(self.reconciler.target_root)),
                ))
                if not source_root or not ready:
                    raise ValueError(
                        "Live Synchronization requires a current zero-difference SNAP baseline for these library roots"
                    )
                ready_ids = {str(row["source_instance_id"]) for row in ready}
                if not active_source:
                    if len(ready_ids) != 1:
                        raise ValueError(
                            "Choose the one authorized Windows source that may publish this Live catalog"
                        )
                    active_source = next(iter(ready_ids))
                if active_source not in ready_ids:
                    raise ValueError(
                        "The selected Windows source is not ready for these authorized library roots"
                    )
            db.raw.execute(
                "UPDATE catalog_sync_settings SET mode=%s,source_root=%s,target_root=%s,"
                "active_source_instance_id=%s,updated_at=now() "
                "WHERE singleton=TRUE",
                (mode, source_root, str(self.reconciler.target_root), active_source),
            )
            if mode != "manual-snap":
                db.raw.execute(
                    "UPDATE sqlite_reconciliation_settings SET enabled=FALSE,auto_apply_safe=FALSE,"
                    "startup_mode='off',next_run_at=NULL,updated_at=now() WHERE singleton=TRUE"
                )
        return self.status()

    def authorize_baseline(self, source_path: str, source_root: str) -> dict[str, Any]:
        """Register a specific zero-difference immutable SNAP revision."""
        source = self.reconciler._source(source_path)
        fingerprint = self.reconciler._fingerprint(source)
        source_root = str(source_root or "").strip()
        if not source_root:
            raise ValueError("The SQLite source library root is required")
        uri = source.resolve().as_uri() + "?mode=ro"
        with closing(sqlite3.connect(uri, uri=True)) as snapshot:
            snapshot.row_factory = sqlite3.Row
            tables = self.reconciler._source_tables(snapshot)
            if "catalog_sync_source" not in tables:
                raise ValueError("This SNAP predates live catalog synchronization; create a fresh Windows SNAP")
            source_columns = {
                str(column[1])
                for column in snapshot.execute("PRAGMA table_info(catalog_sync_source)")
            }
            required = {"baseline_generation", "change_revision"}
            if not required.issubset(source_columns):
                raise ValueError(
                    "This SNAP predates Live recovery hardening; create a fresh Windows SNAP"
                )
            row = snapshot.execute(
                "SELECT source_instance_id,protocol_version,next_revision,"
                "baseline_generation,change_revision FROM catalog_sync_source WHERE singleton=1"
            ).fetchone()
            if not row:
                raise ValueError("The SNAP has no live catalog source identity")
            source_id = str(row["source_instance_id"])
            protocol = int(row["protocol_version"])
            revision = int(row["next_revision"])
            baseline_generation = int(row["baseline_generation"])
            baseline_change_revision = int(row["change_revision"])
        with closing(self.database.connect()) as db, db:
            compared = db.raw.execute(
                "SELECT run_id,status,table_counts FROM sqlite_reconciliation_runs "
                "WHERE source_fingerprint=%s ORDER BY started_at DESC LIMIT 1",
                (fingerprint,),
            ).fetchone()
            catalog_tables = ("artists", "albums", "tracks")
            table_counts = dict((compared or {}).get("table_counts") or {})
            catalog_failures = sum(
                int((table_counts.get(table) or {}).get("failed", 0))
                for table in catalog_tables
            )
            compared_differences = []
            if compared and compared["status"] == "compared":
                compared_differences = [
                    difference
                    for difference in (
                        dict(item)
                        for item in db.raw.execute(
                            "SELECT entity_table,source_value,target_value FROM salite_diff "
                            "WHERE last_run_id=%s AND entity_table=ANY(%s)",
                            (compared["run_id"], list(catalog_tables)),
                        )
                    )
                    if _is_windows_synchronized_difference(difference)
                ]
            if (
                not compared
                or compared["status"] != "compared"
                or catalog_failures
                or compared_differences
            ):
                raise ValueError(
                    "Compare this exact SNAP again after applying/reviewing Windows-synchronized catalog differences; a Live baseline requires zero synchronized differences"
                )
            existing = db.raw.execute(
                "SELECT baseline_revision,last_received_revision,last_applied_revision "
                "FROM catalog_sync_sources WHERE source_instance_id=%s",
                (source_id,),
            ).fetchone()
            if existing and revision < max(
                int(existing["baseline_revision"] or 0),
                int(existing["last_received_revision"] or 0),
                int(existing["last_applied_revision"] or 0),
            ):
                raise ValueError(
                    "The selected SNAP revision predates live catalog history already retained in PostgreSQL"
                )
            newer_event = db.raw.execute(
                "SELECT local_revision FROM catalog_sync_events "
                "WHERE source_instance_id=%s AND local_revision>%s "
                "ORDER BY local_revision LIMIT 1",
                (source_id, revision),
            ).fetchone()
            if newer_event:
                raise ValueError(
                    "The selected SNAP predates a durable live event; create and compare a newer immutable SNAP"
                )
            target_root = str(self.reconciler.target_root)
            db.raw.execute(
                "INSERT INTO catalog_sync_sources(source_instance_id,source_label,baseline_state,"
                "baseline_fingerprint,baseline_revision,last_received_revision,last_applied_revision,"
                "source_root,target_root,protocol_version,baseline_generation,"
                "baseline_change_revision,last_error) "
                "VALUES(%s,%s,'ready',%s,%s,%s,%s,%s,%s,%s,%s,%s,'') "
                "ON CONFLICT(source_instance_id) DO UPDATE SET source_label=excluded.source_label,"
                "baseline_state='ready',baseline_fingerprint=excluded.baseline_fingerprint,"
                "baseline_revision=excluded.baseline_revision,"
                "last_received_revision=excluded.baseline_revision,"
                "last_applied_revision=excluded.baseline_revision,"
                "source_root=excluded.source_root,target_root=excluded.target_root,"
                "protocol_version=excluded.protocol_version,"
                "baseline_generation=excluded.baseline_generation,"
                "baseline_change_revision=excluded.baseline_change_revision,"
                "last_error='',updated_at=now()",
                (
                    source_id,
                    self.reconciler._source_label(source),
                    fingerprint,
                    revision,
                    revision,
                    revision,
                    source_root,
                    target_root,
                    protocol,
                    baseline_generation,
                    baseline_change_revision,
                ),
            )
            db.raw.execute(
                "UPDATE catalog_sync_events SET status='noop',error='',applied_at=now() "
                "WHERE source_instance_id=%s AND local_revision<=%s "
                "AND status IN ('received','blocked','failed')",
                (source_id, revision),
            )
        result = self.status()
        self._event(
            "catalog-sync-baseline",
            {
                "source_instance_id": source_id,
                "revision": revision,
                "baseline_generation": baseline_generation,
                "baseline_change_revision": baseline_change_revision,
                "fingerprint": fingerprint,
            },
            stage="baseline",
            request_status="completed",
        )
        return result

    def verify_integrity(self, source_path: str, source_root: str) -> dict[str, Any]:
        """Compare one fresh immutable SNAP without repairing detected drift."""
        source = self.reconciler._source(source_path)
        source_root = str(source_root or "").strip()
        if not source_root:
            raise ValueError("The SQLite source library root is required")
        fingerprint = self.reconciler._fingerprint(source)
        uri = source.resolve().as_uri() + "?mode=ro"
        with closing(sqlite3.connect(uri, uri=True)) as snapshot:
            snapshot.row_factory = sqlite3.Row
            tables = self.reconciler._source_tables(snapshot)
            if "catalog_sync_source" not in tables:
                raise ValueError("This SNAP predates live catalog synchronization")
            source_columns = {
                str(column[1])
                for column in snapshot.execute("PRAGMA table_info(catalog_sync_source)")
            }
            if not {"baseline_generation", "change_revision"}.issubset(source_columns):
                raise ValueError(
                    "This integrity SNAP predates Live recovery hardening"
                )
            identity = snapshot.execute(
                "SELECT source_instance_id,next_revision,baseline_generation,change_revision "
                "FROM catalog_sync_source WHERE singleton=1"
            ).fetchone()
            if not identity:
                raise ValueError("The SNAP has no live catalog source identity")
            source_id = str(identity["source_instance_id"])
            revision = int(identity["next_revision"])
            baseline_generation = int(identity["baseline_generation"])
            change_revision = int(identity["change_revision"])
        with closing(self.database.connect()) as db:
            registered = db.raw.execute(
                "SELECT source.baseline_state,source.last_received_revision,"
                "source.last_applied_revision,source.source_root,source.target_root,"
                "source.baseline_generation,settings.active_source_instance_id,settings.mode "
                "FROM catalog_sync_sources source CROSS JOIN catalog_sync_settings settings "
                "WHERE source.source_instance_id=%s",
                (source_id,),
            ).fetchone()
        if not registered or registered["baseline_state"] != "ready":
            raise ValueError("This Windows source does not have a ready Live baseline")
        if str(registered["source_root"]) != source_root or Path(
            str(registered["target_root"])
        ).resolve() != self.reconciler.target_root:
            raise ValueError("The integrity SNAP library roots do not match the Live baseline")
        if str(registered["mode"]) != "live" or str(
            registered["active_source_instance_id"] or ""
        ) != source_id:
            raise ValueError("This Windows source is not the active Live source")
        if baseline_generation != int(registered["baseline_generation"] or 0):
            raise ValueError("The integrity SNAP baseline generation does not match")
        if revision != int(registered["last_received_revision"] or 0) or revision != int(
            registered["last_applied_revision"] or 0
        ):
            raise ValueError(
                "Wait for every live revision to be received and applied before checking integrity"
            )

        result = self.reconciler.compare(
            str(source),
            source_root,
            str(self.reconciler.target_root),
            trigger="catalog-integrity",
        )
        catalog_tables = ("artists", "albums", "tracks")
        source_counts = {
            name: int(result["tables"][name]["source_rows"]) for name in catalog_tables
        }
        target_counts = {
            name: int(result["tables"][name]["target_rows"]) for name in catalog_tables
        }
        failed = sum(int(result["tables"][name].get("failed", 0)) for name in catalog_tables)
        with closing(self.database.connect()) as db, db:
            differences = [
                difference
                for difference in (
                    dict(row)
                    for row in db.raw.execute(
                    "SELECT entity_table,entity_key,difference_type,old_digest,new_digest "
                    ",source_value,target_value "
                    "FROM salite_diff WHERE last_run_id=%s AND entity_table=ANY(%s) "
                    "ORDER BY entity_table,entity_key",
                    (result["run_id"], list(catalog_tables)),
                )
                )
                if _is_windows_synchronized_difference(difference)
            ]
            status = "match" if not differences and not failed else "drifted"
            source_digest = _digest(
                {
                    "snapshot_fingerprint": fingerprint,
                    "source_revision": revision,
                    "counts": source_counts,
                }
            )
            target_digest = (
                source_digest
                if status == "match"
                else _digest({"counts": target_counts, "differences": differences})
            )
            message = "" if status == "match" else (
                f"Catalog integrity drift: {len(differences)} difference(s), {failed} failure(s)"
            )
            db.raw.execute(
                "INSERT INTO catalog_sync_integrity_checks(source_instance_id,source_revision,"
                "source_counts,target_counts,source_digest,target_digest,status,error) "
                "VALUES(%s,%s,%s,%s,%s,%s,%s,%s)",
                (
                    source_id,
                    revision,
                    Jsonb(source_counts),
                    Jsonb(target_counts),
                    source_digest,
                    target_digest,
                    status,
                    message,
                ),
            )
            db.raw.execute(
                "UPDATE catalog_sync_sources SET baseline_state=%s,last_integrity_digest=%s,"
                "last_integrity_at=now(),last_error=%s,updated_at=now() "
                "WHERE source_instance_id=%s",
                ("ready" if status == "match" else "drifted", source_digest, message, source_id),
            )
        report = {
            "source_instance_id": source_id,
            "source_revision": revision,
            "change_revision": change_revision,
            "status": status,
            "differences": len(differences),
            "failures": failed,
            "source_counts": source_counts,
            "target_counts": target_counts,
            "run_id": result["run_id"],
        }
        self._event(
            "catalog-sync-integrity",
            report,
            severity="info" if status == "match" else "warning",
            stage="integrity",
            request_status="completed" if status == "match" else "failed",
        )
        return report

    def _mapped_payload(
        self, table: str, payload: dict[str, Any], source_root: str, target_root: str
    ) -> dict[str, Any]:
        allowed = WINDOWS_SYNCHRONIZED_FIELDS[table]
        result = {name: value for name, value in payload.items() if name in allowed}
        for name in PATH_COLUMNS[table]:
            value = result.get(name)
            if value:
                result[name] = map_library_path(value, source_root, target_root)
        return result

    @staticmethod
    def _protected_by_newer_webgui_write(raw, event: dict[str, Any]) -> bool:
        if event["entity_table"] != "albums":
            return False
        coordination = raw.execute(
            "SELECT writer,revision FROM album_artwork_write_coordination WHERE album_identity=%s",
            (canonical_album_identity(event["entity_key"]),),
        ).fetchone()
        return bool(
            coordination
            and coordination["writer"] == "webgui"
            and int(coordination["revision"] or 0) > int(event["observed_artwork_revision"] or 0)
        )

    def _apply_event(self, raw, source: dict[str, Any], event: dict[str, Any]) -> str:
        table = str(event["entity_table"])
        key_name = TABLE_KEYS[table]
        key = str(event["entity_key"])
        prior = raw.execute(
            "SELECT last_revision,payload_digest,operation FROM catalog_sync_entity_state "
            "WHERE source_instance_id=%s AND entity_table=%s AND entity_key=%s",
            (source["source_instance_id"], table, key),
        ).fetchone()
        if prior and int(prior["last_revision"]) >= int(event["local_revision"]):
            return "noop"
        if event["operation"] == "delete":
            if self._protected_by_newer_webgui_write(raw, event):
                outcome = "noop"
            else:
                managed = raw.execute(
                    "SELECT 1 FROM catalog_sync_authority WHERE entity_table=%s AND entity_key=%s "
                    "AND authority='windows' AND source_fingerprint=%s",
                    (table, key, source["source_instance_id"]),
                ).fetchone()
                if managed:
                    raw.execute(
                        sql.SQL("DELETE FROM {} WHERE {}=%s").format(
                            sql.Identifier(table), sql.Identifier(key_name)
                        ),
                        (key,),
                    )
                outcome = "applied" if managed else "noop"
        else:
            payload = dict(event["payload"] or {})
            if _digest(payload) != str(event["payload_digest"]):
                raise ValueError("Catalog event payload digest does not match its immutable envelope")
            mapped = self._mapped_payload(
                table, payload, str(source["source_root"]), str(source["target_root"])
            )
            mapped[key_name] = key
            if self._protected_by_newer_webgui_write(raw, event):
                mapped = {
                    name: value
                    for name, value in mapped.items()
                    if name not in ARTWORK_PROCESSING_COLUMNS
                }
            columns = list(mapped)
            updates = [name for name in columns if name != key_name]
            statement = sql.SQL("INSERT INTO {} ({}) VALUES ({}) ON CONFLICT ({}) DO UPDATE SET {}").format(
                sql.Identifier(table),
                sql.SQL(",").join(map(sql.Identifier, columns)),
                sql.SQL(",").join(sql.Placeholder() for _ in columns),
                sql.Identifier(key_name),
                sql.SQL(",").join(
                    sql.SQL("{}=excluded.{}").format(sql.Identifier(name), sql.Identifier(name))
                    for name in updates
                ),
            )
            raw.execute(statement, [mapped[name] for name in columns])
            mark_windows_authority(
                raw,
                table,
                key,
                str(source["source_instance_id"]),
                str(source["source_label"]),
            )
            outcome = "applied"
        raw.execute(
            "INSERT INTO catalog_sync_entity_state(source_instance_id,entity_table,entity_key,"
            "last_revision,payload_digest,operation) VALUES(%s,%s,%s,%s,%s,%s) "
            "ON CONFLICT(source_instance_id,entity_table,entity_key) DO UPDATE SET "
            "last_revision=excluded.last_revision,payload_digest=excluded.payload_digest,"
            "operation=excluded.operation,applied_at=now()",
            (
                source["source_instance_id"],
                table,
                key,
                event["local_revision"],
                event["payload_digest"],
                event["operation"],
            ),
        )
        return outcome

    def apply_pending(self, limit: int = APPLY_BATCH_LIMIT) -> dict[str, int]:
        result = {"applied": 0, "noop": 0, "blocked": 0, "failed": 0}
        maximum = max(1, min(int(limit), APPLY_BATCH_LIMIT))
        with self._lock, closing(self.database.connect()) as db:
            for _ in range(maximum):
                with db:
                    settings = db.raw.execute(
                        "SELECT * FROM catalog_sync_settings WHERE singleton=TRUE FOR UPDATE"
                    ).fetchone()
                    active_source = str(
                        (settings or {}).get("active_source_instance_id") or ""
                    )
                    if not settings or settings["mode"] != "live" or not active_source:
                        break
                    source_row = db.raw.execute(
                        "SELECT * FROM catalog_sync_sources WHERE source_instance_id=%s FOR UPDATE",
                        (active_source,),
                    ).fetchone()
                    if (
                        not source_row
                        or source_row["baseline_state"] != "ready"
                        or str(source_row["source_root"]) != str(settings["source_root"])
                        or str(source_row["target_root"]) != str(settings["target_root"])
                    ):
                        break
                    source = dict(source_row)
                    row = db.raw.execute(
                        "SELECT * FROM catalog_sync_events WHERE source_instance_id=%s "
                        "AND status IN ('received','blocked','failed') "
                        "ORDER BY local_revision LIMIT 1 FOR UPDATE SKIP LOCKED",
                        (active_source,),
                    ).fetchone()
                    if not row:
                        break
                    event = dict(row)
                    if event["status"] in {"blocked", "failed"}:
                        # The original attempt already recorded the durable
                        # diagnostic.  Leave the failed revision as the
                        # ordering barrier without emitting a duplicate Audit
                        # event on every worker poll.
                        break
                    revision = int(event["local_revision"])
                    expected = int(
                        source["last_applied_revision"] or source["baseline_revision"]
                    ) + 1
                    if revision <= int(source["baseline_revision"]):
                        outcome = "noop"
                    elif revision != expected:
                        message = f"Catalog revision gap: expected {expected}, received {revision}"
                        db.raw.execute(
                            "UPDATE catalog_sync_events SET status='blocked',error=%s,"
                            "attempt_count=attempt_count+1,last_attempt_at=now() "
                            "WHERE source_instance_id=%s AND local_revision=%s",
                            (message, active_source, revision),
                        )
                        db.raw.execute(
                            "UPDATE catalog_sync_sources SET baseline_state='drifted',last_error=%s,updated_at=now() "
                            "WHERE source_instance_id=%s",
                            (message, active_source),
                        )
                        result["blocked"] += 1
                        break
                    else:
                        try:
                            # A savepoint keeps one invalid event from
                            # poisoning the transaction used to persist its
                            # diagnostic and recovery state.
                            with db.raw.transaction():
                                outcome = self._apply_event(db.raw, source, event)
                        except Exception as error:
                            message = sanitize_text(error, limit=1000)
                            db.raw.execute(
                                "UPDATE catalog_sync_events SET status='failed',error=%s,"
                                "attempt_count=attempt_count+1,last_attempt_at=now() "
                                "WHERE source_instance_id=%s AND local_revision=%s",
                                (message, active_source, revision),
                            )
                            db.raw.execute(
                                "UPDATE catalog_sync_sources SET baseline_state='drifted',last_error=%s,updated_at=now() "
                                "WHERE source_instance_id=%s",
                                (message, active_source),
                            )
                            result["failed"] += 1
                            break
                    db.raw.execute(
                        "UPDATE catalog_sync_events SET status=%s,error='',applied_at=now(),"
                        "attempt_count=attempt_count+1,last_attempt_at=now() "
                        "WHERE source_instance_id=%s AND local_revision=%s",
                        (outcome, active_source, revision),
                    )
                    db.raw.execute(
                        "UPDATE catalog_sync_sources SET last_applied_revision=GREATEST(last_applied_revision,%s),"
                        "last_error='',updated_at=now() WHERE source_instance_id=%s",
                        (revision, active_source),
                    )
                    result[outcome] += 1
            if result["applied"]:
                with db:
                    SQLiteReconciler._recalculate(db)
                    SQLiteReconciler._recalculate_comp_progress(db)
        if any(result.values()):
            self._event(
                "catalog-sync-apply",
                result,
                severity="warning" if result["failed"] or result["blocked"] else "info",
                stage="apply",
                request_status="failed"
                if result["failed"] or result["blocked"]
                else "completed",
            )
        return result

    def _worker(self) -> None:
        while not self._stop.wait(2):
            try:
                self.apply_pending()
            except Exception as error:
                self._event(
                    "catalog-sync-worker",
                    {},
                    severity="error",
                    stage="apply",
                    request_status="failed",
                    error=sanitize_text(error, limit=1000),
                )

