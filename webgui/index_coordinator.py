"""Durable, bounded catalog-index generation coordination.

Filesystem and tag work happens outside long database transactions.  Each
completed Album is checkpointed in a short transaction.  Reader-facing catalog
tables are changed only by the final publication transaction owned by the
generation lease holder.
"""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from contextlib import closing
from typing import Any, Iterable, Mapping
from uuid import uuid4
import json

from psycopg.types.json import Jsonb


ACTIVE_STATES = ("building", "validating", "publishing")
TERMINAL_STATES = ("published", "cancelled", "failed", "abandoned")
LEASE_SECONDS = 90


def utc_now() -> datetime:
    return datetime.now(timezone.utc)


@dataclass(frozen=True)
class GenerationLease:
    generation_id: str
    owner_token: str
    mode: str
    library_root: str
    base_generation_id: str
    resumed: bool = False


class GenerationBusy(RuntimeError):
    def __init__(self, generation_id: str):
        super().__init__(f"Catalog indexing is already active ({generation_id})")
        self.generation_id = generation_id


class GenerationLeaseLost(RuntimeError):
    pass


def _lease_deadline() -> datetime:
    return utc_now() + timedelta(seconds=LEASE_SECONDS)


def _mode(mode: str, has_published: bool) -> str:
    if not has_published:
        return "initial"
    if mode not in {"full", "incremental"}:
        raise ValueError("Library index mode must be full or incremental")
    return mode


def claim(database, requested_mode: str, library_root: str) -> GenerationLease:
    """Claim or recover one PostgreSQL generation under an advisory lock."""
    owner = str(uuid4())
    with closing(database.connect()) as db, db:
        db.raw.execute("SELECT pg_advisory_xact_lock(741520206)")
        state = db.raw.execute(
            "SELECT active_generation_id,catalog_state "
            "FROM catalog_index_state WHERE singleton=TRUE FOR UPDATE"
        ).fetchone()
        if state is None:
            db.raw.execute(
                "INSERT INTO catalog_index_state(singleton,catalog_state) "
                "VALUES(TRUE,'initializing')"
            )
            active_generation = ""
        else:
            active_generation = str(state["active_generation_id"] or "")
        mode = _mode(requested_mode, bool(active_generation))
        active = db.raw.execute(
            "SELECT * FROM catalog_generations "
            "WHERE backend='postgresql' "
            "AND state IN ('building','validating','publishing') "
            "ORDER BY created_at DESC LIMIT 1 FOR UPDATE"
        ).fetchone()
        now = utc_now()
        if active is not None:
            expired = active["lease_expires_at"] <= now
            same_work = (
                str(active["library_root"]) == library_root
                and str(active["mode"]) == mode
            )
            if not expired:
                raise GenerationBusy(str(active["generation_id"]))
            if same_work:
                generation_id = str(active["generation_id"])
                db.raw.execute(
                    "UPDATE catalog_generations SET owner_token=%s,"
                    "lease_expires_at=%s,state='building',phase='recovering',"
                    "discovered_albums=0,completed_albums=0,changed_albums=0,"
                    "unchanged_albums=0,removed_albums=0,error_count=0,"
                    "current_path='',updated_at=now(),error='' WHERE generation_id=%s",
                    (owner, _lease_deadline(), generation_id),
                )
                db.raw.execute(
                    "UPDATE catalog_index_state SET maintenance_generation_id=%s,"
                    "catalog_state=CASE WHEN active_generation_id='' "
                    "THEN 'initializing' ELSE 'maintenance' END,updated_at=now() "
                    "WHERE singleton=TRUE",
                    (generation_id,),
                )
                return GenerationLease(
                    generation_id,
                    owner,
                    mode,
                    library_root,
                    active_generation,
                    True,
                )
            db.raw.execute(
                "UPDATE catalog_generations SET state='abandoned',"
                "error='Lease expired before different index work was claimed',"
                "updated_at=now() WHERE generation_id=%s",
                (active["generation_id"],),
            )
        generation_id = str(uuid4())
        db.raw.execute(
            "INSERT INTO catalog_generations("
            "generation_id,backend,mode,state,base_generation_id,library_root,"
            "owner_token,lease_expires_at) "
            "VALUES(%s,'postgresql',%s,'building',%s,%s,%s,%s)",
            (
                generation_id,
                mode,
                active_generation,
                library_root,
                owner,
                _lease_deadline(),
            ),
        )
        db.raw.execute(
            "UPDATE catalog_index_state SET maintenance_generation_id=%s,"
            "catalog_state=CASE WHEN active_generation_id='' "
            "THEN 'initializing' ELSE 'maintenance' END,updated_at=now() "
            "WHERE singleton=TRUE",
            (generation_id,),
        )
    return GenerationLease(
        generation_id, owner, mode, library_root, active_generation, False
    )


def heartbeat(database, lease: GenerationLease, *, phase: str, current_path: str = ""):
    with closing(database.connect()) as db, db:
        updated = db.raw.execute(
            "UPDATE catalog_generations SET lease_expires_at=%s,phase=%s,"
            "current_path=%s,updated_at=now() WHERE generation_id=%s "
            "AND owner_token=%s AND state='building' AND lease_expires_at>now()",
            (
                _lease_deadline(),
                phase,
                current_path,
                lease.generation_id,
                lease.owner_token,
            ),
        ).rowcount
    if updated != 1:
        raise GenerationLeaseLost("Catalog index generation lease was lost")


def completed_paths(database, lease: GenerationLease) -> dict[str, str]:
    with closing(database.connect()) as db:
        return {
            str(row["album_path"]): str(row["inventory_signature"])
            for row in db.execute(
                "SELECT album_path,inventory_signature "
                "FROM catalog_generation_albums WHERE generation_id=? "
                "AND disposition<>'error'",
                (lease.generation_id,),
            )
        }


def completed_checkpoints(
    database, lease: GenerationLease
) -> dict[str, tuple[str, str]]:
    """Load recovered checkpoint identity once instead of querying per Album."""
    with closing(database.connect()) as db:
        return {
            str(row["album_path"]): (
                str(row["inventory_signature"]),
                str(row["disposition"]),
            )
            for row in db.execute(
                "SELECT album_path,inventory_signature,disposition "
                "FROM catalog_generation_albums WHERE generation_id=? "
                "AND disposition<>'error'",
                (lease.generation_id,),
            )
        }


def checkpoint_counts(database, lease: GenerationLease) -> dict[str, int]:
    with closing(database.connect()) as db:
        row = db.raw.execute(
            "SELECT count(*) AS completed,"
            "count(*) FILTER (WHERE disposition='changed') AS changed,"
            "count(*) FILTER (WHERE disposition='unchanged') AS unchanged,"
            "count(*) FILTER (WHERE disposition='error') AS errors "
            "FROM catalog_generation_albums WHERE generation_id=%s",
            (lease.generation_id,),
        ).fetchone()
    return {
        "completed": int(row["completed"] or 0),
        "changed": int(row["changed"] or 0),
        "unchanged": int(row["unchanged"] or 0),
        "errors": int(row["errors"] or 0),
    }


def touch_completed(
    database,
    lease: GenerationLease,
    album_path: str,
    inventory_signature: str,
) -> str | None:
    """Attach a recovered checkpoint to the current enumeration pass."""
    with closing(database.connect()) as db, db:
        row = db.raw.execute(
            "UPDATE catalog_generation_albums SET scan_token=%s,"
            "completed_at=now() WHERE generation_id=%s AND album_path=%s "
            "AND inventory_signature=%s AND disposition<>'error' "
            "AND EXISTS(SELECT 1 FROM catalog_generations generation "
            "WHERE generation.generation_id=catalog_generation_albums.generation_id "
            "AND generation.owner_token=%s AND generation.state='building' "
            "AND generation.lease_expires_at>now()) "
            "RETURNING disposition",
            (
                lease.owner_token,
                lease.generation_id,
                album_path,
                inventory_signature,
                lease.owner_token,
            ),
        ).fetchone()
    return str(row["disposition"]) if row is not None else None


def touch_completed_batch(
    database,
    lease: GenerationLease,
    checkpoints: Iterable[tuple[str, str]],
    *,
    discovered: int,
    completed: int,
    changed: int,
    unchanged: int,
    current_path: str,
) -> None:
    """Attach recovered checkpoints with one transaction and one update round trip."""
    rows = list(checkpoints)
    if not rows:
        return
    paths = [row[0] for row in rows]
    signatures = [row[1] for row in rows]
    with closing(database.connect()) as db, db:
        recovered = db.raw.execute(
            "WITH candidates(album_path,inventory_signature) AS ("
            "SELECT * FROM unnest(%s::text[],%s::text[])) "
            "UPDATE catalog_generation_albums checkpoint SET scan_token=%s,"
            "completed_at=now() FROM candidates "
            "WHERE checkpoint.generation_id=%s "
            "AND checkpoint.album_path=candidates.album_path "
            "AND checkpoint.inventory_signature=candidates.inventory_signature "
            "AND checkpoint.disposition<>'error' "
            "AND EXISTS(SELECT 1 FROM catalog_generations generation "
            "WHERE generation.generation_id=checkpoint.generation_id "
            "AND generation.owner_token=%s AND generation.state='building' "
            "AND generation.lease_expires_at>now()) "
            "RETURNING checkpoint.album_path",
            (
                paths,
                signatures,
                lease.owner_token,
                lease.generation_id,
                lease.owner_token,
            ),
        ).fetchall()
        if len(recovered) != len(rows):
            raise GenerationLeaseLost(
                "Catalog index generation lease was lost while recovering checkpoints"
            )
        owned = db.raw.execute(
            "UPDATE catalog_generations SET lease_expires_at=%s,"
            "discovered_albums=%s,completed_albums=%s,changed_albums=%s,"
            "unchanged_albums=%s,current_path=%s,phase='recovering',"
            "updated_at=now() WHERE generation_id=%s AND owner_token=%s "
            "AND state='building' AND lease_expires_at>now()",
            (
                _lease_deadline(),
                discovered,
                completed,
                changed,
                unchanged,
                current_path,
                lease.generation_id,
                lease.owner_token,
            ),
        ).rowcount
        if owned != 1:
            raise GenerationLeaseLost("Catalog index generation lease was lost")


def stage_batch(
    database,
    lease: GenerationLease,
    rows: Iterable[Mapping[str, Any]],
    *,
    discovered: int,
    completed: int,
    changed: int,
    unchanged: int,
    current_path: str,
):
    rows = list(rows)
    with closing(database.connect()) as db, db:
        owned = db.raw.execute(
            "UPDATE catalog_generations SET lease_expires_at=%s,"
            "discovered_albums=%s,completed_albums=%s,changed_albums=%s,"
            "unchanged_albums=%s,current_path=%s,phase='indexing',"
            "updated_at=now() WHERE generation_id=%s AND owner_token=%s "
            "AND state='building' AND lease_expires_at>now()",
            (
                _lease_deadline(),
                discovered,
                completed,
                changed,
                unchanged,
                current_path,
                lease.generation_id,
                lease.owner_token,
            ),
        ).rowcount
        if owned != 1:
            raise GenerationLeaseLost("Catalog index generation lease was lost")
        for row in rows:
            db.raw.execute(
                "INSERT INTO catalog_generation_albums("
                "generation_id,album_path,scan_token,album_key,artist_key,"
                "inventory_signature,artwork_signature,tag_signature,"
                "track_count,total_size,newest_mtime_ns,"
                "disposition,artist_payload,album_payload,tracks_payload,"
                "review_payload,error,completed_at) "
                "VALUES(%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,now()) "
                "ON CONFLICT(generation_id,album_path) DO UPDATE SET "
                "scan_token=excluded.scan_token,"
                "album_key=excluded.album_key,artist_key=excluded.artist_key,"
                "inventory_signature=excluded.inventory_signature,"
                "artwork_signature=excluded.artwork_signature,"
                "tag_signature=excluded.tag_signature,"
                "track_count=excluded.track_count,"
                "total_size=excluded.total_size,"
                "newest_mtime_ns=excluded.newest_mtime_ns,"
                "disposition=excluded.disposition,"
                "artist_payload=excluded.artist_payload,"
                "album_payload=excluded.album_payload,"
                "tracks_payload=excluded.tracks_payload,"
                "review_payload=excluded.review_payload,error=excluded.error,"
                "completed_at=now()",
                (
                    lease.generation_id,
                    row["album_path"],
                    lease.owner_token,
                    row.get("album_key", ""),
                    row.get("artist_key", ""),
                    row["inventory_signature"],
                    row.get("artwork_signature", ""),
                    row.get("tag_signature", ""),
                    int(row.get("track_count", 0)),
                    int(row.get("total_size", 0)),
                    int(row.get("newest_mtime_ns", 0)),
                    row["disposition"],
                    Jsonb(row.get("artist_payload"))
                    if row.get("artist_payload") is not None
                    else None,
                    Jsonb(row.get("album_payload"))
                    if row.get("album_payload") is not None
                    else None,
                    Jsonb(row.get("tracks_payload"))
                    if row.get("tracks_payload") is not None
                    else None,
                    Jsonb(row.get("review_payload"))
                    if row.get("review_payload") is not None
                    else None,
                    row.get("error", ""),
                ),
            )


def update_change_summary(database, summary: Mapping[str, Any]):
    with closing(database.connect()) as db, db:
        db.raw.execute(
            "UPDATE catalog_index_state SET change_summary=%s,updated_at=now() "
            "WHERE singleton=TRUE",
            (Jsonb(dict(summary)),),
        )


def change_summary(database) -> dict[str, Any]:
    with closing(database.connect()) as db:
        row = db.raw.execute(
            "SELECT change_summary,catalog_state,active_generation_id,"
            "maintenance_generation_id FROM catalog_index_state "
            "WHERE singleton=TRUE"
        ).fetchone()
    if row is None:
        return {
            "count": 0,
            "added": 0,
            "changed": 0,
            "moved": 0,
            "removed": 0,
            "examples": [],
            "catalog_state": "initializing",
        }
    summary = dict(row["change_summary"] or {})
    summary.update(
        catalog_state=str(row["catalog_state"]),
        active_generation_id=str(row["active_generation_id"] or ""),
        maintenance_generation_id=str(row["maintenance_generation_id"] or ""),
    )
    return summary


def status(database) -> dict[str, Any]:
    summary = change_summary(database)
    generation_id = summary.get("maintenance_generation_id") or summary.get(
        "active_generation_id"
    )
    if not generation_id:
        return summary
    with closing(database.connect()) as db:
        row = db.raw.execute(
            "SELECT generation_id,mode,state,phase,discovered_albums,"
            "completed_albums,changed_albums,unchanged_albums,removed_albums,"
            "error_count,current_path,error,created_at,updated_at,published_at "
            "FROM catalog_generations WHERE generation_id=%s",
            (generation_id,),
        ).fetchone()
    if row is not None:
        summary["generation"] = dict(row)
    return summary


def finish(database, lease: GenerationLease, state: str, error: str = ""):
    if state not in {"cancelled", "failed", "abandoned"}:
        raise ValueError("Invalid terminal generation state")
    with closing(database.connect()) as db, db:
        updated = db.raw.execute(
            "UPDATE catalog_generations SET state=%s,phase=%s,current_path='',"
            "error=%s,updated_at=now() "
            "WHERE generation_id=%s AND owner_token=%s "
            "AND state IN ('building','validating','publishing')",
            (state, state, error[:2000], lease.generation_id, lease.owner_token),
        ).rowcount
        if updated:
            db.raw.execute(
                "UPDATE catalog_index_state SET maintenance_generation_id='',"
                "catalog_state=CASE WHEN active_generation_id='' "
                "THEN 'failed' ELSE 'ready' END,updated_at=now() "
                "WHERE singleton=TRUE AND maintenance_generation_id=%s",
                (lease.generation_id,),
            )


def json_text(value: Any) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"))
