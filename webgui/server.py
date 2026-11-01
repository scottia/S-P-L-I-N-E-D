"""SPLINED HTTP application: browser setup, operations, and PostgreSQL jobs."""

from __future__ import annotations

from contextlib import asynccontextmanager
from pathlib import Path
from urllib.parse import urlsplit
from uuid import uuid4
from io import BytesIO
from PIL import Image
import base64
import json
import os
import secrets
import threading
import time
import tomllib

from fastapi import FastAPI, Request
from fastapi.responses import FileResponse, JSONResponse, Response
from fastapi.staticfiles import StaticFiles

import splined as core
from webgui.catalog import ASSETS
from webgui.database import Database
from webgui.runtime import Runtime
from webgui.services import Services
from webgui.jobs import JobQueue
from webgui.credentials import Credentials
from webgui.auth import AdminAuth
from webgui.audit import sanitize, sanitize_text, utc_now


SETTINGS_UI = Path(__file__).resolve().parents[1] / "schema/settings-ui.json"


class Context:
    def __init__(self, config_file, database_url=None, *, start_worker=True):
        self.config_file = Path(config_file).resolve()
        self.explicit_url = (
            database_url
            or os.environ.get("SPLINED_DATABASE_URL", "")
            or ("postgresql://" if os.environ.get("PGHOST") else "")
        )
        self.start_worker = start_worker
        self.runtime = None
        self.services = None
        self.jobs = None
        self.url = ""
        self.error = ""
        try:
            cfg = (
                tomllib.loads(self.config_file.read_text(encoding="utf-8"))
                if self.config_file.is_file()
                else {}
            )
            self.url = self.explicit_url or str(cfg.get("database", {}).get("url", ""))
            if self.url:
                self.open(self.url)
        except Exception:
            if self.runtime:
                self.runtime.close()
            self.runtime = None
            self.services = None
            self.jobs = None
            self.error = "The database connection or saved configuration needs attention in Settings"

    def open(self, url):
        database = Database(url)
        database.migrate()
        self.runtime = Runtime(self.config_file, database)
        self.services = Services(self.runtime)
        if self.start_worker:
            self.jobs = JobQueue(self.services)
        self.url = url
        self.error = ""

    def require(self):
        if not self.services:
            raise ValueError("Complete database and library setup in Settings")
        return self.services

    def settings(self):
        if self.runtime:
            cfg = self.runtime.settings()
        elif self.config_file.is_file():
            cfg = tomllib.loads(self.config_file.read_text(encoding="utf-8"))
        else:
            cfg = tomllib.loads(
                (
                    Path(__file__).resolve().parents[1] / "docker/config.example.toml"
                ).read_text(encoding="utf-8")
            )
        cfg.setdefault("output", {}).setdefault("embedded_thumbnail_size", 1200)
        cfg.setdefault("database", {})["url"] = ""
        return {
            "config": cfg,
            "database_configured": bool(self.runtime),
            "database_managed": bool(self.explicit_url),
            "error": self.error,
        }

    def configure(self, cfg):
        if not isinstance(cfg, dict):
            raise ValueError("Settings must be an object")
        url = (
            self.explicit_url or str(cfg.get("database", {}).get("url", "")) or self.url
        )
        if not url:
            raise ValueError("Enter a PostgreSQL connection URL")
        cfg = dict(cfg)
        cfg["database"] = {"url": url}
        if self.runtime and url == self.url:
            self.runtime.save_settings(cfg)
            self.services.reload_settings()
        else:
            if self.jobs and any(
                item["state"] not in {"done", "failed", "cancelled"}
                for item in self.jobs.list()
            ):
                raise ValueError(
                    "Finish or remove queued work before changing databases"
                )
            database = Database(url)
            database.migrate()
            Runtime.validate(cfg)
            core.validate_filename(str(cfg.get("output", {}).get("file_name", "cover")))
            if not core.resolve_path(
                self.config_file, str(cfg.get("library", {}).get("music_library", ""))
            ).is_dir():
                raise ValueError("The music library must be an existing directory")
            if self.jobs:
                self.jobs.close()
            if self.runtime:
                self.runtime.close()
            runtime = Runtime(self.config_file, database, cfg)
            runtime.save_settings(cfg)
            self.runtime = runtime
            self.services = Services(runtime)
            self.jobs = JobQueue(self.services) if self.start_worker else None
            self.url = url
            self.error = ""
        return self.settings()


def create_app(config_file=None, database_url=None, *, token=None, start_worker=True):
    config_file = config_file or os.environ.get("SPLINED_CONFIG", "/config/config.toml")
    context = Context(config_file, database_url, start_worker=start_worker)
    access_token = (
        token if token is not None else os.environ.get("SPLINED_WEB_TOKEN", "")
    )
    sessions = {}
    sessions_lock = threading.Lock()
    login_attempts = {}

    def account_auth() -> AdminAuth:
        if not context.runtime:
            raise ValueError("PostgreSQL must be available before creating the WebGUI administrator")
        return AdminAuth(context.runtime.database)

    def admin_setup_required() -> bool:
        if not access_token:
            return False
        try:
            return not account_auth().account_exists()
        except Exception:
            return True

    def auth_rate_key(request: Request, purpose: str) -> str:
        host = request.client.host if request.client else "unknown"
        return f"{purpose}:{host}"

    def require_auth_rate(key: str) -> tuple[int, float]:
        with sessions_lock:
            failures, blocked_until = login_attempts.get(key, (0, 0.0))
        if time.monotonic() < blocked_until:
            raise PermissionError("Too many sign-in attempts; wait briefly before retrying")
        return failures, blocked_until

    def record_auth_failure(key: str, failures: int) -> None:
        failures += 1
        with sessions_lock:
            login_attempts[key] = (
                0,
                time.monotonic() + 30,
            ) if failures >= 5 else (failures, 0.0)
            if len(login_attempts) > 1000:
                login_attempts.clear()

    def clear_auth_failures(key: str) -> None:
        with sessions_lock:
            login_attempts.pop(key, None)

    @asynccontextmanager
    async def lifespan(_app):
        yield
        if context.jobs:
            context.jobs.close()
        if context.runtime:
            context.runtime.close()

    app = FastAPI(title="SPLINED", docs_url=None, redoc_url=None, lifespan=lifespan)
    app.state.context = context

    @app.middleware("http")
    async def browser_session(request, call_next):
        started = time.perf_counter()
        supplied_trace = sanitize_text(request.headers.get("x-splined-trace", ""), limit=100)
        request.state.trace_id = supplied_trace or uuid4().hex
        request.state.action_id = sanitize_text(
            request.headers.get("x-splined-action", ""), limit=100
        )
        origin = request.headers.get("origin")
        if (
            origin
            and urlsplit(origin).netloc.lower()
            != request.headers.get("host", "").lower()
        ):
            return JSONResponse(
                {"error": "Cross-origin requests are not allowed"}, status_code=403
            )
        host = (request.url.hostname or "").lower()
        if not access_token and host not in {
            "127.0.0.1",
            "localhost",
            "::1",
            "testserver",
        }:
            return JSONResponse(
                {"error": "Set a WebGUI access token before enabling network access"},
                status_code=403,
            )
        sid = request.cookies.get("splined_session", "")
        with sessions_lock:
            if sid not in sessions:
                if len(sessions) > 1000:
                    sessions.clear()
                sid = secrets.token_urlsafe(32)
                sessions[sid] = {
                    "csrf": secrets.token_urlsafe(32),
                    "authenticated": not bool(access_token),
                    "username": "Local administrator" if not access_token else "",
                }
            session = sessions[sid]
        request.state.session = session
        public = request.url.path in {
            "/api/status",
            "/api/login",
            "/api/admin/setup",
            "/api/admin/recover",
        } or not request.url.path.startswith("/api/")
        if not public and not session["authenticated"]:
            return JSONResponse({"error": "Sign in to SPLINED"}, status_code=401)
        if request.method not in {"GET", "HEAD", "OPTIONS"}:
            if request.headers.get("x-splined-csrf") != session["csrf"]:
                return JSONResponse(
                    {"error": "Refresh the page before retrying this operation"},
                    status_code=403,
                )
            content_length = int(request.headers.get("content-length", "0") or 0)
            if content_length > 40 * 1024 * 1024:
                return JSONResponse({"error": "Request is too large"}, status_code=413)
        response = await call_next(request)
        response.set_cookie(
            "splined_session",
            sid,
            httponly=True,
            samesite="strict",
            secure=request.url.scheme == "https",
        )
        response.headers["Cache-Control"] = "no-store"
        response.headers["X-Content-Type-Options"] = "nosniff"
        response.headers["X-SPLINED-Trace"] = request.state.trace_id
        response.headers["Content-Security-Policy"] = (
            "default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'"
        )
        path = request.url.path
        noisy = (
            path in {"/api/status", "/api/jobs", "/api/jobs/control-state"}
            or path.startswith(("/api/image/", "/api/fallback/thumbnail", "/api/audit"))
        )
        should_audit = path.startswith("/api/") and (
            request.method not in {"GET", "HEAD", "OPTIONS"}
            or path in {
                "/api/artist",
                "/api/artist/overview",
                "/api/artist/metadata",
                "/api/artist/comp-report",
                "/api/album",
                "/api/fallback/tracks",
                "/api/fallback/track",
            }
            or response.status_code >= 400
        )
        if context.services and should_audit and not noisy:
            scope = {
                "artist_id": request.query_params.get("artist_id")
                or (request.query_params.get("id") if "/artist" in path else ""),
                "album_id": request.query_params.get("album_id")
                or (request.query_params.get("id") if "/album" in path or "/fallback" in path else ""),
                "track_key": request.query_params.get("key", ""),
            }
            try:
                context.services.audit(
                    "api-request",
                    f"{request.method} {path}",
                    {"http_status": response.status_code},
                    severity="error" if response.status_code >= 500 else "warning" if response.status_code >= 400 else "debug",
                    action_id=request.state.action_id,
                    trace_id=request.state.trace_id,
                    scope={key: value for key, value in scope.items() if value},
                    stage="response",
                    request_status="failed" if response.status_code >= 400 else "completed",
                    elapsed_ms=(time.perf_counter() - started) * 1000,
                )
            except Exception as audit_error:
                core.runtime_log(
                    "warning",
                    "webgui.audit request event unavailable "
                    + core._redact_log_message(str(audit_error)),
                )
        return response

    @app.exception_handler(PermissionError)
    @app.exception_handler(KeyError)
    @app.exception_handler(ValueError)
    @app.exception_handler(core.SplinedError)
    @app.exception_handler(Exception)
    async def operation_error(request, error):
        status = (
            403
            if isinstance(error, PermissionError)
            else 404
            if isinstance(error, KeyError)
            else 400
        )
        message = core._redact_log_message(str(error))[:2000]
        if "postgresql://" in message or "postgres://" in message:
            message = "Database operation failed; check the connection in Settings"
        if context.services:
            try:
                context.services.audit(
                    "api-error",
                    f"{request.method} {request.url.path}",
                    severity="error",
                    action_id=getattr(request.state, "action_id", ""),
                    trace_id=getattr(request.state, "trace_id", ""),
                    stage="exception",
                    request_status="failed",
                    error=message,
                )
            except Exception:
                pass
        return JSONResponse(
            {"error": message or "Operation failed"}, status_code=status
        )

    @app.get("/api/status")
    def status(request: Request):
        session = request.state.session
        r = context.runtime
        result = {
            "version": core.VERSION,
            "csrf": session["csrf"],
            "authenticated": session["authenticated"],
            "auth_required": bool(access_token),
            "admin_setup_required": admin_setup_required(),
            "username": session.get("username", "") if session["authenticated"] else "",
            "setup_required": not bool(
                r and context.config_file.is_file() and r.music.is_dir()
            ),
            "database": "PostgreSQL",
            "database_ready": bool(context.services),
        }
        if session["authenticated"]:
            result.update(
                {
                    "mode": r.cfg.get("mode", "read") if r else "read",
                    "music": str(r.music) if r else "",
                    "artist_art": str(r.artist_art) if r else "",
                    "embedded_thumbnail_size": int(
                        r.cfg.get("output", {}).get(
                            "embedded_thumbnail_size", 1200
                        )
                    )
                    if r
                    else 1200,
                    "comp_repair": bool(r and r.cfg.get("mode") == "write"),
                    "animate_webp_preview": bool(
                        r and r.cfg.get("webgui_ui", {}).get("animate_webp_preview", False)
                    ),
                    "capabilities": {
                        "cover_write": True,
                        "artist_write": True,
                        "embedding": True,
                        "musicbrainz": True,
                        "configuration": True,
                    },
                    "error": context.error,
                }
            )
        return result

    @app.post("/api/login")
    async def login(request: Request):
        value = await request.json()
        session = request.state.session
        if not access_token:
            session.update(authenticated=True, username="Local administrator")
            return {"ok": True, "username": session["username"]}
        if admin_setup_required():
            raise PermissionError("Create the WebGUI administrator before signing in")
        rate_key = auth_rate_key(request, "login")
        failures, _ = require_auth_rate(rate_key)
        username = account_auth().authenticate(
            str(value.get("username", "")), str(value.get("password", ""))
        )
        if not username:
            record_auth_failure(rate_key, failures)
            raise PermissionError("Incorrect username or password")
        clear_auth_failures(rate_key)
        session.update(
            authenticated=True,
            username=username,
        )
        return {"ok": True, "username": username}

    @app.post("/api/admin/setup")
    async def setup_admin(request: Request):
        value = await request.json()
        rate_key = auth_rate_key(request, "setup")
        failures, _ = require_auth_rate(rate_key)
        try:
            username = account_auth().setup(
                str(value.get("username", "")),
                str(value.get("password", "")),
                str(value.get("confirmation", "")),
                str(value.get("token", "")),
                access_token,
            )
        except PermissionError:
            record_auth_failure(rate_key, failures)
            raise
        clear_auth_failures(rate_key)
        request.state.session.update(authenticated=True, username=username)
        return {"ok": True, "username": username}

    @app.post("/api/admin/recover")
    async def recover_admin(request: Request):
        if admin_setup_required():
            raise ValueError("Create the WebGUI administrator before using password recovery")
        value = await request.json()
        rate_key = auth_rate_key(request, "recovery")
        failures, _ = require_auth_rate(rate_key)
        try:
            username = account_auth().reset_password(
                str(value.get("username", "")),
                str(value.get("password", "")),
                str(value.get("confirmation", "")),
                str(value.get("token", "")),
                access_token,
            )
        except PermissionError:
            record_auth_failure(rate_key, failures)
            raise
        context.services.audit("reset-admin-password", username)
        with sessions_lock:
            sessions.clear()
            login_attempts.clear()
        request.state.session.update(authenticated=False, username="")
        return {"ok": True, "username": username}

    @app.post("/api/logout")
    def logout(request: Request):
        request.state.session.update(authenticated=not bool(access_token), username="")
        return {"ok": True}

    @app.get("/api/settings/contract")
    def settings_contract():
        return json.loads(SETTINGS_UI.read_text(encoding="utf-8"))

    @app.get("/api/settings")
    def settings():
        return context.settings()

    @app.post("/api/settings")
    def save_settings(payload: dict):
        return context.configure((payload).get("config"))

    @app.get("/api/diagnostics")
    def diagnostics(
        limit: int = 250,
        severity: str = "",
        action: str = "",
        correlation: str = "",
        since: str = "",
        until: str = "",
    ):
        context.require()
        return context.services.diagnostics(
            limit,
            severity=severity,
            action=action,
            correlation=correlation,
            since=since,
            until=until,
        )

    @app.post("/api/audit/events")
    def browser_audit(request: Request, payload: dict):
        services = context.require()
        value = sanitize(payload)
        return {
            "sequence": services.audit(
                str(value.get("action") or "browser-event"),
                str(value.get("subject") or "browser"),
                value.get("details") if isinstance(value.get("details"), dict) else {},
                severity=str(value.get("severity") or "info"),
                action_id=str(value.get("action_id") or request.state.action_id),
                trace_id=str(value.get("trace_id") or request.state.trace_id),
                job_id=str(value.get("job_id") or ""),
                scope=value.get("scope") if isinstance(value.get("scope"), dict) else {},
                stage=str(value.get("stage") or "browser"),
                request_status=str(value.get("request_status") or "emitted"),
                elapsed_ms=value.get("elapsed_ms"),
                state_from=str(value.get("state_from") or ""),
                state_to=str(value.get("state_to") or ""),
                error=str(value.get("error") or ""),
            )
        }

    @app.get("/api/audit/export")
    def export_audit(
        severity: str = "",
        action: str = "",
        correlation: str = "",
        since: str = "",
        until: str = "",
    ):
        services = context.require()
        events = services.diagnostics(
            50000,
            severity=severity,
            action=action,
            correlation=correlation,
            since=since,
            until=until,
            maximum=50000,
        )
        body = json.dumps(
            {
                "format": "splined-web-audit-v1",
                "generated_at": utc_now(),
                "filters": sanitize(
                    {
                        "severity": severity,
                        "action": action,
                        "correlation": correlation,
                        "since": since,
                        "until": until,
                    }
                ),
                "events": events,
            },
            ensure_ascii=False,
            indent=2,
            default=str,
        )
        return Response(
            body,
            media_type="application/json",
            headers={"Content-Disposition": 'attachment; filename="audit.json"'},
        )

    @app.get("/api/storage")
    def storage():
        context.require()
        return context.services.storage_summary()

    @app.get("/api/credentials/{provider}")
    def credentials(provider: str):
        context.require()
        return Credentials(context.runtime).describe(provider)

    @app.post("/api/credentials/{provider}")
    def save_credentials(provider: str, payload: dict):
        context.require()
        result = Credentials(context.runtime).save(provider, payload)
        context.services.audit("save-credentials", provider)
        return result

    @app.post("/api/credentials/{provider}/test")
    def test_credentials(provider: str):
        context.require()
        return Credentials(context.runtime).test(provider)

    @app.post("/api/credentials/musicbrainz/authorize")
    def authorize_musicbrainz(request: Request):
        context.require()
        return Credentials(context.runtime).authorize_musicbrainz(request.state.session)

    @app.post("/api/credentials/musicbrainz/complete")
    def complete_musicbrainz(request: Request, payload: dict):
        context.require()
        return Credentials(context.runtime).complete_musicbrainz(
            request.state.session, payload
        )

    @app.get("/api/artists")
    def artists(q: str = ""):
        return context.require().catalog.artists(q)

    @app.get("/api/artists/thumbnails")
    def artist_thumbnails():
        return context.require().catalog.artist_thumbnails()

    @app.get("/api/albums")
    def albums(q: str = "", view: str = "albums"):
        return context.require().catalog.albums(q, view == "fallback")

    @app.get("/api/library/changes")
    def library_changes():
        return context.require().index_changes()

    @app.get("/api/artist")
    def artist(id: str):
        return context.require().catalog.artist(id)

    @app.get("/api/artist/overview")
    def artist_overview(id: str):
        catalog = context.require().catalog
        artist = catalog.artist(id)
        return {"artist": artist, "comp_albums": catalog.artist_comp_albums(str(artist["id"]))}

    @app.get("/api/artist/thumbnail")
    def artist_thumbnail(id: str):
        return FileResponse(
            context.require().catalog.artist_thumbnail(id),
            headers={"Cache-Control": "private, max-age=300"},
        )

    @app.get("/api/artist/metadata")
    def artist_metadata(id: str, refresh: bool = False):
        services = context.require()
        result = services.catalog.artist_metadata(id, refresh=refresh)
        if result.get("reason"):
            services.audit(
                "artist-metadata-unavailable",
                id,
                {"artist_id": id, "reason": result["reason"]},
                severity="warning",
                stage="provider",
                request_status="unavailable",
            )
        return result

    @app.get("/api/artist/comp-report")
    def comp_report(id: str):
        return context.require().comp_report(id)

    @app.get("/api/artist/comp-albums")
    def comp_albums(id: str):
        return context.require().catalog.artist_comp_albums(id)

    @app.get("/api/album")
    def album(id: str):
        services = context.require()
        services.hydrate_tracks(id)
        return services.catalog.album(id)

    @app.get("/api/fallback/tracks")
    def tracks(id: str):
        services = context.require()
        services.hydrate_tracks(id)
        return services.catalog.fallback_tracks(id)

    @app.get("/api/fallback/track")
    def track(id: str, key: str):
        return context.require().catalog.fallback_track_preview(id, key)

    @app.get("/api/fallback/track-status")
    def track_status(id: str, key: str):
        return context.require().catalog.comp_track_status(id, key)

    @app.get("/api/fallback/thumbnail")
    def fallback_thumbnail(id: str, key: str):
        return Response(
            context.require().catalog.fallback_thumbnail(id, key),
            media_type="image/png",
            headers={"Cache-Control": "private, max-age=300"},
        )

    @app.get("/api/image/{image_id}")
    def image(image_id: str, still: bool = False):
        source = context.require().catalog.image_source("/api/image/" + image_id)
        if still:
            try:
                with Image.open(source if isinstance(source, Path) else BytesIO(source)) as opened:
                    if str(opened.format or "").casefold() == "webp" and int(getattr(opened, "n_frames", 1)) > 1:
                        opened.seek(0)
                        output = BytesIO()
                        opened.convert("RGBA").save(output, format="PNG")
                        return Response(output.getvalue(), media_type="image/png")
            except (OSError, ValueError):
                pass
        if isinstance(source, Path):
            if not source.is_file():
                raise KeyError("Artwork is unavailable")
            return FileResponse(source, headers={"Cache-Control": "private, no-cache, must-revalidate"})
        with Image.open(BytesIO(source)) as opened:
            media_type = Image.MIME.get(opened.format, "image/png")
        return Response(source, media_type=media_type)

    @app.post("/api/search/artist")
    def search_artist(payload: dict):
        return context.require().catalog.search_artist(str((payload).get("id", "")))

    @app.post("/api/search/album")
    def search_album(payload: dict):
        value = payload
        return context.require().search_album(
            str(value.get("id", "")), value.get("release_id")
        )

    @app.post("/api/musicbrainz")
    def musicbrainz(payload: dict):
        value = payload
        return context.require().musicbrainz(
            str(value.get("album_id", "")), value.get("artist"), value.get("title")
        )

    @app.post("/api/preview/render")
    def preview(payload: dict):
        return context.require().preview(payload)

    @app.post("/api/preview/upscale")
    def upscale(payload: dict):
        value = payload
        value["upscale"] = True
        return context.require().preview(value)

    @app.post("/api/artwork/cover")
    def write_cover(payload: dict):
        return context.require().write_cover(payload)

    @app.post("/api/artwork/artist")
    def write_artist(payload: dict):
        return context.require().write_artist(payload)

    @app.post("/api/bypass")
    def bypass(payload: dict):
        value = payload
        return context.require().bypass(
            str(value.get("album_id", "")), bool(value.get("enabled", True))
        )

    def repair_comp(payload: dict, *, artist_required: bool):
        services = context.require()
        context.runtime.require_write()
        artist_id = str(payload.get("artist_id", "")).strip()
        if artist_required and not artist_id:
            raise ValueError("Choose the Artist COMP Report before repairing a track")
        with context.runtime.processing:
            return services.catalog.repair_comp_track(
                str(payload.get("album_id", "")),
                str(payload.get("track_key", "")),
                artist_id if artist_required else "",
            )

    @app.post("/api/comp/repair")
    def full_comp_repair(payload: dict):
        return repair_comp(payload, artist_required=False)

    @app.post("/api/artist/comp-repair")
    def artist_comp_repair(payload: dict):
        return repair_comp(payload, artist_required=True)

    @app.post("/api/comp/embed")
    def full_comp_embed(payload: dict):
        value = dict(payload)
        value.pop("artist_id", None)
        value.pop("artist_name", None)
        return context.require().embed_comp_track(value)

    @app.post("/api/comp/thumbnail-preview")
    def full_comp_thumbnail_preview(payload: dict):
        value = dict(payload)
        value.pop("artist_id", None)
        value.pop("artist_name", None)
        return context.require().prepare_comp_thumbnail(value)

    @app.post("/api/artist/comp-embed")
    def artist_comp_embed(payload: dict):
        if not str(payload.get("artist_id", "")).strip():
            raise ValueError("Choose the Artist COMP Report before embedding a track")
        return context.require().embed_comp_track(payload)

    @app.post("/api/artist/comp-thumbnail-preview")
    def artist_comp_thumbnail_preview(payload: dict):
        if not str(payload.get("artist_id", "")).strip():
            raise ValueError("Choose the Artist COMP Report before preparing a thumbnail")
        return context.require().prepare_comp_thumbnail(payload)

    @app.get("/api/jobs")
    def jobs():
        context.require()
        return context.jobs.list() if context.jobs else []

    @app.get("/api/jobs/control-state")
    def job_control_state():
        context.require()
        return context.jobs.control_state() if context.jobs else {
            "pause_after_album": False,
            "pause_all_albums": False,
        }

    @app.post("/api/jobs")
    def stage_job(payload: dict):
        services = context.require()
        value = payload
        kind = value.get("kind")
        payload = value.get("payload", {})
        if not context.jobs:
            raise ValueError("The job worker is unavailable")
        if kind == "embed-album":
            old = next(
                (
                    job
                    for job in context.jobs.list()
                    if job["kind"] == kind
                    and job["payload"].get("album_id") == payload.get("album_id")
                    and job["state"] not in {"done", "failed", "cancelled"}
                ),
                None,
            )
            if old:
                return {"id": old["id"], "already_queued": True}
            payload = services.stage_album(payload)
            try:
                result = context.jobs.enqueue(str(kind), payload, staged=True)
            except Exception:
                services.remove_staged_artwork(payload)
                raise
            if result.get("already_queued"):
                services.remove_staged_artwork(payload)
            return result
        if kind == "comp":
            context.runtime.require_write()
            payload = services.validate_comp_payload(payload)
        return context.jobs.enqueue(str(kind), payload, staged=kind == "embed-album")

    @app.post("/api/jobs/action")
    def queue_action(payload: dict):
        context.require()
        value = payload
        if not context.jobs:
            raise ValueError("The job worker is unavailable")
        return context.jobs.action(
            str(value.get("action", "")),
            value.get("id"),
            value.get("value"),
            value.get("nonce"),
            value.get("kind"),
        )

    @app.post("/api/artwork/upload")
    def upload(payload: dict):
        services = context.require()
        body = base64.b64decode(str(payload.get("data", "")), validate=True)
        if len(body) > 25 * 1024 * 1024:
            raise ValueError("Image upload exceeds 25 MB")
        from webgui.imaging import adjusted, encode

        image = adjusted(body)
        directory = context.runtime.temporary_cache / "uploads"
        directory.mkdir(parents=True, exist_ok=True)
        path = directory / (uuid4().hex + ".png")
        path.write_bytes(encode(image))
        return {
            "image": services.catalog.image(path),
            "name": str(payload.get("name", "Uploaded artwork"))[:200],
            "source": "Local",
            "width": image.width,
            "height": image.height,
            "bytes": path.stat().st_size,
            "kind": "cover",
            "acceptable": True,
        }

    @app.post("/api/catalog/import")
    def import_catalog(payload: dict):
        services = context.require()
        value = payload
        source = Path(str(value.get("path", "")))
        if not source.is_file():
            raise ValueError("Choose an existing SQLite snapshot")
        return services.runtime.database.import_sqlite(source)

    @app.get("/guide")
    def guide():
        return FileResponse(ASSETS / "guide.html")

    app.mount("/", StaticFiles(directory=ASSETS, html=True), name="workspace")
    return app
