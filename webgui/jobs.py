"""PostgreSQL-backed staged work and recoverable browser decisions."""

from __future__ import annotations

from contextlib import closing
from pathlib import Path
from uuid import uuid4
import json
import threading

from psycopg.types.json import Jsonb
import splined as core
from interaction.status import use_adapter


TERMINAL_STATES = {"done", "failed", "cancelled"}


class JobAdapter:
    def __init__(self, queue, job_id):
        self.queue = queue
        self.id = job_id
        self.events = []
        self.current = {}
        self.context = {}

    def emit(self, event, payload):
        payload = dict(payload)
        if event in {"candidates", "musicbrainz_existing"}:
            for item in payload.get("items", []):
                if item.get("path"):
                    item["image"] = self.queue.services.catalog.image(
                        Path(item["path"])
                    )
        self.current[event] = payload
        self.events.append({"event": event, "data": payload})
        self.events = self.events[-30:]
        self.progress({"events": self.events, "current": self.current})

    def progress(self, value):
        with closing(self.queue.db.connect()) as db, db:
            db.raw.execute(
                "UPDATE web_jobs SET progress=progress || %s,updated_at=now() WHERE id=%s",
                (Jsonb(value), self.id),
            )
        if value.get("message"):
            self.queue.audit_job(
                self.id,
                "job-progress",
                stage="progress",
                details={
                    key: value[key]
                    for key in ("message", "completed", "total")
                    if key in value
                },
            )

    def cancelled(self):
        return self.queue.cancel_requested(self.id)

    def read(self, prompt, context):
        nonce = uuid4().hex
        context = {**self.context, **context}
        if context.get("kind") == "batch-summary":
            return "exit"
        self.progress({"prompt": {"nonce": nonce, "label": prompt, "context": context}})
        with closing(self.queue.db.connect()) as db, db:
            db.execute(
                "UPDATE web_jobs SET state='waiting',decision=NULL,updated_at=now() WHERE id=? AND state='running'",
                (self.id,),
            )
        self.queue.audit_job(
            self.id,
            "job-transition",
            stage=str(context.get("kind") or "decision"),
            state_from="running",
            state_to="waiting",
            request_status="waiting",
        )
        while not self.cancelled():
            with closing(self.queue.db.connect()) as db, db:
                row = db.execute(
                    "SELECT decision FROM web_jobs WHERE id=?", (self.id,)
                ).fetchone()
                if row and row["decision"] and row["decision"].get("nonce") == nonce:
                    value = str(row["decision"].get("value", ""))
                    try:
                        selected = json.loads(value)
                    except (TypeError, ValueError):
                        selected = None
                    artwork = selected.get("artwork") if isinstance(selected, dict) else None
                    if isinstance(artwork, dict) and artwork.get("prepared_id"):
                        self.progress(
                            {
                                "prepared_comp": {
                                    "prepared_id": str(artwork.get("prepared_id", "")),
                                    "sha256": str(artwork.get("prepared_sha256", "")),
                                    "album_id": str(self.context.get("album_id", "")),
                                    "track_key": str(self.context.get("track_key", "")),
                                    "artist_id": str(self.context.get("scope_artist_id", "")),
                                }
                            }
                        )
                    db.execute(
                        "UPDATE web_jobs SET state='running',decision=NULL,progress=progress - 'prompt',updated_at=now() WHERE id=?",
                        (self.id,),
                    )
                    self.queue.audit_job(
                        self.id,
                        "job-transition",
                        stage=str(context.get("kind") or "decision"),
                        state_from="waiting",
                        state_to="running",
                        request_status="accepted",
                    )
                    return value
            self.queue.stopped.wait(0.25)
        raise core.InteractionCancelled()


class JobQueue:
    def __init__(self, services):
        self.services = services
        self.db = services.runtime.database
        self.stopped = threading.Event()
        self.wake = threading.Event()
        self.control_lock = threading.Lock()
        self.pause_after_album = False
        self.pause_all_albums = False
        self.lease = self.db.connect()
        acquired = self.lease.execute(
            "SELECT pg_try_advisory_lock(hashtextextended(current_schema() || ':splined-web-worker',0))"
        ).fetchone()[0]
        self.lease.commit()
        if not acquired:
            self.lease.close()
            raise ValueError("Another WebGUI worker already owns this catalog")
        with closing(self.db.connect()) as db, db:
            # COMP review is interactive, track-scoped browser work rather
            # than a durable embedding queue.  Never resume or retain it
            # across a server lifecycle: authority/candidate state must be
            # rebuilt from the current Artist report and selected track.
            comp_rows = list(
                db.execute(
                    "SELECT progress FROM web_jobs WHERE kind='comp'"
                )
            )
            db.execute("DELETE FROM web_jobs WHERE kind='comp'")
            # Durable non-COMP writes resume only after an explicit operator
            # action. Album staging and its prepared snapshots remain intact.
            db.execute(
                "UPDATE web_jobs SET state='paused',decision=NULL,error='Interrupted by server restart',updated_at=now() "
                "WHERE kind<>'comp' AND state IN ('running','waiting')"
            )
        for row in comp_rows:
            prepared = row["progress"].get("prepared_comp", {})
            if isinstance(prepared, dict):
                self.services._remove_prepared_comp(prepared.get("prepared_id", ""))
        self.thread = threading.Thread(
            target=self.run, name="splined-web-worker", daemon=True
        )
        self.thread.start()

    def close(self):
        self.stopped.set()
        self.wake.set()
        self.thread.join(timeout=15)

    def audit_job(
        self,
        job_id,
        action,
        *,
        stage="",
        details=None,
        state_from="",
        state_to="",
        request_status="",
        severity="info",
        error="",
    ):
        try:
            with closing(self.db.connect()) as db:
                row = db.execute(
                    "SELECT kind,payload FROM web_jobs WHERE id=?", (job_id,)
                ).fetchone()
            payload = row["payload"] if row else {}
            correlation = payload.get("_audit", {}) if isinstance(payload, dict) else {}
            scope = {
                key: payload.get(key)
                for key in ("artist_id", "album_id", "track_key")
                if payload.get(key)
            }
            if not scope and len(payload.get("track_keys") or []) == 1:
                scope["track_key"] = payload["track_keys"][0]
            self.services.audit(
                action,
                row["kind"] if row else "job",
                details or {},
                severity=severity,
                action_id=str(correlation.get("action_id") or ""),
                trace_id=str(correlation.get("trace_id") or ""),
                job_id=job_id,
                scope=scope,
                stage=stage,
                request_status=request_status,
                state_from=state_from,
                state_to=state_to,
                error=error,
            )
        except Exception as audit_error:
            core.runtime_log(
                "warning",
                "webgui.job audit unavailable "
                + core._redact_log_message(str(audit_error)),
            )

    def list(self):
        with closing(self.db.connect()) as db:
            rows = list(
                db.execute(
                    "SELECT * FROM web_jobs WHERE "
                    "state NOT IN ('done','failed','cancelled') "
                    "OR (kind='embed-album' AND state='failed') "
                    "OR id IN (SELECT id FROM web_jobs ORDER BY created_at DESC LIMIT 200) "
                    "ORDER BY created_at DESC"
                )
            )
        for row in rows:
            if row["kind"] == "embed-album":
                row["payload"] = self.services.staged_album_projection(
                    row["payload"], validate=row["state"] not in {"done", "cancelled"}
                )
            row["created_at"] = row["created_at"].isoformat()
            row["updated_at"] = row["updated_at"].isoformat()
        return rows

    def control_state(self):
        with self.control_lock:
            return {
                "pause_after_album": self.pause_after_album,
                "pause_all_albums": self.pause_all_albums,
            }

    def cancel_requested(self, job_id):
        if self.stopped.is_set():
            return True
        with closing(self.db.connect()) as db:
            row = db.execute(
                "SELECT kind,state FROM web_jobs WHERE id=?", (job_id,)
            ).fetchone()
        if not row or row["state"] in {"cancelled", "paused"}:
            return True
        with self.control_lock:
            return row["kind"] == "embed-album" and self.pause_all_albums

    def enqueue(self, kind, payload, *, staged=False):
        if kind not in {"embed-album", "comp", "index", "scan"}:
            raise ValueError("Unknown work type")
        if not isinstance(payload, dict):
            raise ValueError("Invalid work request")
        payload = dict(payload)
        payload.setdefault("library_root", str(self.services.runtime.music))
        job_id = uuid4().hex
        with closing(self.db.connect()) as db, db:
            if payload.get("album_id"):
                db.raw.execute(
                    "SELECT pg_advisory_xact_lock(hashtextextended(%s,0))",
                    (kind + ":" + str(payload["album_id"]),),
                )
                active = db.raw.execute(
                    "SELECT id,payload FROM web_jobs WHERE kind=%s AND payload->>'album_id'=%s AND state NOT IN ('done','failed','cancelled') ORDER BY created_at",
                    (kind, str(payload["album_id"])),
                ).fetchall()
                for old in active:
                    if kind != "comp" or (
                        old["payload"].get("artist_id") == payload.get("artist_id")
                        and old["payload"].get("track_keys") == payload.get("track_keys")
                    ):
                        correlation = payload.get("_audit", {})
                        self.services.audit(
                            "job-deduplicated",
                            kind,
                            {"already_queued": True},
                            action_id=str(correlation.get("action_id") or ""),
                            trace_id=str(correlation.get("trace_id") or ""),
                            job_id=old["id"],
                            scope={"album_id": payload.get("album_id", "")},
                            stage="queue",
                            request_status="existing",
                        )
                        return {"id": old["id"], "already_queued": True}
            db.raw.execute(
                "INSERT INTO web_jobs(id,kind,state,payload) VALUES(%s,%s,%s,%s)",
                (job_id, kind, "staged" if staged else "queued", Jsonb(payload)),
            )
        self.wake.set()
        self.audit_job(
            job_id,
            "job-transition",
            stage="queue",
            state_to="staged" if staged else "queued",
            request_status="queued",
        )
        return {"id": job_id, "state": "staged" if staged else "queued"}

    def action(self, action, job_id=None, value=None, nonce=None, kind=None):
        kind = str(kind or "").strip()
        if kind and kind not in {"embed-album", "comp", "index", "scan"}:
            raise ValueError("Unknown work type")
        if action in {"pause-after", "resume-queue", "pause-all", "resume-all"}:
            if kind != "embed-album":
                raise ValueError("Album queue controls require embed-album scope")
            if action == "pause-after":
                with self.control_lock:
                    self.pause_after_album = True
            elif action == "resume-queue":
                with self.control_lock:
                    self.pause_after_album = False
            elif action == "pause-all":
                with self.control_lock:
                    self.pause_all_albums = True
            else:
                with self.control_lock:
                    self.pause_all_albums = False
                with closing(self.db.connect()) as db, db:
                    db.execute(
                        "UPDATE web_jobs SET state='queued',decision=NULL,"
                        "progress=progress-'prompt',error='',updated_at=now() "
                        "WHERE state='paused' AND kind='embed-album'"
                    )
            self.wake.set()
            return {"ok": True, **self.control_state()}

        if action == "retry" and job_id:
            with closing(self.db.connect()) as db:
                retry_row = db.execute(
                    "SELECT kind,state,payload FROM web_jobs WHERE id=?", (job_id,)
                ).fetchone()
            if retry_row and retry_row["kind"] == "embed-album":
                try:
                    self.services.validate_staged_album(retry_row["payload"])
                except ValueError as error:
                    with closing(self.db.connect()) as db, db:
                        db.execute(
                            "UPDATE web_jobs SET state='failed',error=?,updated_at=now() WHERE id=?",
                            (str(error), job_id),
                        )
                    raise

        cleanup_payloads = []
        with closing(self.db.connect()) as db, db:
            if action == "start":
                if kind:
                    if kind == "embed-album":
                        staged = list(
                            db.execute(
                                "SELECT id,payload FROM web_jobs "
                                "WHERE state='staged' AND kind='embed-album' "
                                "ORDER BY created_at"
                            )
                        )
                        for item in staged:
                            try:
                                self.services.validate_staged_album(item["payload"])
                            except ValueError as error:
                                db.execute(
                                    "UPDATE web_jobs SET state='failed',error=?,updated_at=now() WHERE id=? AND state='staged'",
                                    (str(error), item["id"]),
                                )
                            else:
                                db.execute(
                                    "UPDATE web_jobs SET state='queued',decision=NULL,progress=progress-'prompt',"
                                    "error='',updated_at=now() WHERE id=? AND state='staged'",
                                    (item["id"],),
                                )
                    else:
                        db.execute(
                            "UPDATE web_jobs SET state='queued',decision=NULL,progress=progress-'prompt',"
                            "error='',updated_at=now() WHERE state='staged' AND kind=?",
                            (kind,),
                        )
                else:
                    staged = list(
                        db.execute(
                            "SELECT id,payload FROM web_jobs "
                            "WHERE state='staged' AND kind='embed-album' "
                            "ORDER BY created_at"
                        )
                    )
                    for item in staged:
                        try:
                            self.services.validate_staged_album(item["payload"])
                        except ValueError as error:
                            db.execute(
                                "UPDATE web_jobs SET state='failed',error=?,updated_at=now() WHERE id=? AND state='staged'",
                                (str(error), item["id"]),
                            )
                        else:
                            db.execute(
                                "UPDATE web_jobs SET state='queued',decision=NULL,progress=progress-'prompt',"
                                "error='',updated_at=now() WHERE id=? AND state='staged'",
                                (item["id"],),
                            )
                    db.execute(
                        "UPDATE web_jobs SET state='queued',decision=NULL,progress=progress-'prompt',"
                        "error='',updated_at=now() WHERE state='staged' AND kind<>'embed-album'"
                    )
            elif action == "decision":
                result = db.raw.execute(
                    "UPDATE web_jobs SET decision=%s,updated_at=now() WHERE id=%s AND state='waiting' AND progress->'prompt'->>'nonce'=%s",
                    (Jsonb({"nonce": nonce, "value": str(value)}), job_id, nonce),
                )
                if not result.rowcount:
                    raise ValueError("This decision is no longer current")
            elif action == "retry":
                db.execute(
                    "UPDATE web_jobs SET state='queued',decision=NULL,"
                    "progress=CASE WHEN state='failed' THEN progress-'prompt'-'completed'-'message' "
                    "ELSE progress-'prompt' END,"
                    "error='',updated_at=now() WHERE id=? AND state IN ('failed','paused')",
                    (job_id,),
                )
            elif action in {"pause", "cancel"}:
                state = "paused" if action == "pause" else "cancelled"
                if job_id:
                    db.execute(
                        "UPDATE web_jobs SET state=?,updated_at=now() WHERE id=? AND state NOT IN ('done','failed','cancelled')",
                        (state, job_id),
                    )
                elif kind:
                    db.execute(
                        "UPDATE web_jobs SET state=?,updated_at=now() "
                        "WHERE state NOT IN ('done','failed','cancelled') AND kind=?",
                        (state, kind),
                    )
                else:
                    db.execute(
                        "UPDATE web_jobs SET state=?,updated_at=now() WHERE state NOT IN ('done','failed','cancelled')",
                        (state,),
                    )
            elif action == "remove":
                if not job_id:
                    raise ValueError("Choose a job")
                row = db.execute(
                    "SELECT kind,state,payload FROM web_jobs WHERE id=?", (job_id,)
                ).fetchone()
                if row and row["state"] in {"running", "waiting"}:
                    raise ValueError("Pause work before removing it")
                if row and row["kind"] == "embed-album":
                    cleanup_payloads.append(row["payload"])
                db.execute("DELETE FROM web_jobs WHERE id=?", (job_id,))
            elif action == "clear":
                if kind:
                    running = db.execute(
                        "SELECT 1 FROM web_jobs WHERE kind=? AND state IN ('running','waiting') LIMIT 1",
                        (kind,),
                    ).fetchone()
                    if running:
                        raise ValueError(
                            "Pause ALL Albums and wait for the current track before removing the queue"
                        )
                    if kind == "embed-album":
                        cleanup_payloads.extend(
                            row["payload"]
                            for row in db.execute(
                                "SELECT payload FROM web_jobs WHERE kind='embed-album'"
                            )
                        )
                    db.execute(
                        "DELETE FROM web_jobs WHERE kind=?",
                        (kind,),
                    )
                else:
                    running = db.execute(
                        "SELECT 1 FROM web_jobs WHERE state IN ('running','waiting') LIMIT 1"
                    ).fetchone()
                    if running:
                        raise ValueError(
                            "Pause running work before removing the queue"
                        )
                    cleanup_payloads.extend(
                        row["payload"]
                        for row in db.execute(
                            "SELECT payload FROM web_jobs WHERE kind='embed-album'"
                        )
                    )
                    db.execute(
                        "DELETE FROM web_jobs"
                    )
            else:
                raise ValueError("Unknown queue action")
        for payload in cleanup_payloads:
            self.services.remove_staged_artwork(payload)
        self.wake.set()
        if job_id:
            self.audit_job(
                job_id,
                "job-action",
                stage="queue-control",
                details={"action": action, "kind": kind},
                request_status="accepted",
            )
        return {"ok": True, **self.control_state()}

    def claim(self):
        with self.control_lock:
            block_albums = self.pause_after_album or self.pause_all_albums
        with closing(self.db.connect()) as db, db:
            return db.raw.execute(
                "UPDATE web_jobs SET state='running',updated_at=now() WHERE id=(SELECT id FROM web_jobs WHERE state='queued' AND (%s=FALSE OR kind<>'embed-album') ORDER BY created_at FOR UPDATE SKIP LOCKED LIMIT 1) RETURNING *",
                (block_albums,),
            ).fetchone()

    def run(self):
        try:
            while not self.stopped.is_set():
                try:
                    job = self.claim()
                except Exception:
                    self.stopped.wait(2)
                    continue
                if not job:
                    self.wake.wait(1)
                    self.wake.clear()
                    continue
                    self.audit_job(
                        job["id"],
                        "job-transition",
                        stage="worker",
                        state_from="queued",
                        state_to="running",
                        request_status="running",
                    )
                adapter = JobAdapter(self, job["id"])
                try:
                    with self.services.runtime.processing, use_adapter(adapter):
                        payload = job["payload"]
                        kind = job["kind"]
                        if payload.get("library_root") and payload[
                            "library_root"
                        ] != str(self.services.runtime.music):
                            raise ValueError(
                                "The queued library has changed; add this work again"
                            )
                        if kind == "embed-album":
                            result = self.services.embed_album(
                                payload, adapter.progress, adapter.cancelled
                            )
                        elif kind == "index":
                            result = self.services.refresh_index(
                                adapter.progress,
                                adapter.cancelled,
                                str(payload.get("mode", "full")),
                            )
                        elif kind == "comp":
                            result = self.services.run_comp(payload, adapter)
                        elif kind == "scan":
                            result = self.services.run_scan(payload, adapter)
                    with closing(self.db.connect()) as db, db:
                        db.raw.execute(
                            "UPDATE web_jobs SET state='done',result=%s,progress=progress-'prompt',updated_at=now() WHERE id=%s AND state NOT IN ('cancelled','paused')",
                            (Jsonb(result), job["id"]),
                        )
                    self.audit_job(
                        job["id"],
                        "job-transition",
                        stage="worker",
                        state_from="running",
                        state_to="done",
                        request_status="completed",
                        details={"result": result},
                    )
                except (core.InteractionCancelled, core.InteractionReturn):
                    with closing(self.db.connect()) as db, db:
                        db.execute(
                            "UPDATE web_jobs SET state='paused',updated_at=now() WHERE id=? AND state NOT IN ('cancelled','paused')",
                            (job["id"],),
                        )
                    self.audit_job(
                        job["id"],
                        "job-transition",
                        stage="worker",
                        state_from="running",
                        state_to="paused",
                        request_status="paused",
                    )
                except Exception as error:
                    message = core._redact_log_message(str(error))[:2000]
                    core.runtime_log(
                        "error",
                        f"webgui.job failed id={job['id']} kind={job['kind']} error={message}",
                    )
                    with closing(self.db.connect()) as db, db:
                        db.execute(
                            "UPDATE web_jobs SET state='failed',error=?,updated_at=now() WHERE id=? AND state NOT IN ('cancelled','paused')",
                            (message, job["id"]),
                        )
                    self.audit_job(
                        job["id"],
                        "job-transition",
                        stage="worker",
                        state_from="running",
                        state_to="failed",
                        request_status="failed",
                        severity="error",
                        error=message,
                    )
        finally:
            self.lease.close()
