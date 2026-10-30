"""PostgreSQL-backed staged work and recoverable browser decisions."""

from __future__ import annotations

from contextlib import closing
from pathlib import Path
from uuid import uuid4
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
        if event == "candidates":
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

    def cancelled(self):
        if self.queue.stopped.is_set():
            return True
        with closing(self.queue.db.connect()) as db:
            row = db.execute(
                "SELECT state FROM web_jobs WHERE id=?", (self.id,)
            ).fetchone()
        return not row or row["state"] in {"cancelled", "paused"}

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
        while not self.cancelled():
            with closing(self.queue.db.connect()) as db, db:
                row = db.execute(
                    "SELECT decision FROM web_jobs WHERE id=?", (self.id,)
                ).fetchone()
                if row and row["decision"] and row["decision"].get("nonce") == nonce:
                    value = str(row["decision"].get("value", ""))
                    db.execute(
                        "UPDATE web_jobs SET state='running',decision=NULL,progress=progress - 'prompt',updated_at=now() WHERE id=?",
                        (self.id,),
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
        self.lease = self.db.connect()
        acquired = self.lease.execute(
            "SELECT pg_try_advisory_lock(hashtextextended(current_schema() || ':splined-web-worker',0))"
        ).fetchone()[0]
        self.lease.commit()
        if not acquired:
            self.lease.close()
            raise ValueError("Another WebGUI worker already owns this catalog")
        with closing(self.db.connect()) as db, db:
            # Interrupted writes resume only after an explicit operator start.
            db.execute(
                "UPDATE web_jobs SET state='paused',decision=NULL,error='Interrupted by server restart',updated_at=now() WHERE state IN ('running','waiting')"
            )
        self.thread = threading.Thread(
            target=self.run, name="splined-web-worker", daemon=True
        )
        self.thread.start()

    def close(self):
        self.stopped.set()
        self.wake.set()
        self.thread.join(timeout=15)

    def list(self):
        with closing(self.db.connect()) as db:
            rows = list(
                db.execute("SELECT * FROM web_jobs ORDER BY created_at DESC LIMIT 200")
            )
        for row in rows:
            row["created_at"] = row["created_at"].isoformat()
            row["updated_at"] = row["updated_at"].isoformat()
        return rows

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
                old = db.raw.execute(
                    "SELECT id FROM web_jobs WHERE kind=%s AND payload->>'album_id'=%s AND state NOT IN ('done','failed','cancelled')",
                    (kind, str(payload["album_id"])),
                ).fetchone()
                if old:
                    return {"id": old["id"], "already_queued": True}
            db.raw.execute(
                "INSERT INTO web_jobs(id,kind,state,payload) VALUES(%s,%s,%s,%s)",
                (job_id, kind, "staged" if staged else "queued", Jsonb(payload)),
            )
        self.wake.set()
        return {"id": job_id, "state": "staged" if staged else "queued"}

    def action(self, action, job_id=None, value=None, nonce=None):
        with closing(self.db.connect()) as db, db:
            if action == "start":
                db.execute(
                    "UPDATE web_jobs SET state='queued',error='',updated_at=now() WHERE state IN ('staged','paused')"
                )
            elif action == "pause-after":
                db.execute(
                    "UPDATE web_jobs SET state='paused',updated_at=now() WHERE state IN ('queued','staged')"
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
                    "UPDATE web_jobs SET state='queued',error='',updated_at=now() WHERE id=? AND state IN ('failed','paused')",
                    (job_id,),
                )
            elif action in {"pause", "cancel"}:
                state = "paused" if action == "pause" else "cancelled"
                if job_id:
                    db.execute(
                        "UPDATE web_jobs SET state=?,updated_at=now() WHERE id=? AND state NOT IN ('done','failed','cancelled')",
                        (state, job_id),
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
                    "SELECT state FROM web_jobs WHERE id=?", (job_id,)
                ).fetchone()
                if row and row["state"] in {"running", "waiting"}:
                    raise ValueError("Pause work before removing it")
                db.execute("DELETE FROM web_jobs WHERE id=?", (job_id,))
            elif action == "clear":
                db.execute(
                    "DELETE FROM web_jobs WHERE state NOT IN ('running','waiting')"
                )
            else:
                raise ValueError("Unknown queue action")
        self.wake.set()
        return {"ok": True}

    def claim(self):
        with closing(self.db.connect()) as db, db:
            return db.raw.execute(
                "UPDATE web_jobs SET state='running',updated_at=now() WHERE id=(SELECT id FROM web_jobs WHERE state='queued' ORDER BY created_at FOR UPDATE SKIP LOCKED LIMIT 1) RETURNING *"
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
                except (core.InteractionCancelled, core.InteractionReturn):
                    with closing(self.db.connect()) as db, db:
                        db.execute(
                            "UPDATE web_jobs SET state='paused',updated_at=now() WHERE id=? AND state NOT IN ('cancelled','paused')",
                            (job["id"],),
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
        finally:
            self.lease.close()
