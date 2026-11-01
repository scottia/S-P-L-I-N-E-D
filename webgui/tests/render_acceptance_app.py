"""Disposable, read-only browser harness for WebGUI rendered acceptance.

Run only against localhost.  It serves the real static application with a
small in-memory API projection and never opens PostgreSQL or media files.
"""

from __future__ import annotations

from copy import deepcopy
from pathlib import Path
import json
import tomllib

from fastapi import FastAPI
from fastapi.responses import JSONResponse
from fastapi.staticfiles import StaticFiles


REPOSITORY = Path(__file__).resolve().parents[2]
STATIC = REPOSITORY / "webgui" / "static"
CONTRACT = json.loads((REPOSITORY / "schema" / "settings-ui.json").read_text(encoding="utf-8"))
CONFIG = tomllib.loads((REPOSITORY / "docker" / "config.example.toml").read_text(encoding="utf-8"))
CONFIG["library"]["music_library"] = r"E:\Disposable SPLINED Acceptance\Music"
CONFIG["scan"]["cache_dir"] = r"E:\Disposable SPLINED Acceptance\Cache"
CONFIG["scan"]["temporary_cache_dir"] = r"E:\Disposable SPLINED Acceptance\Temp"
CONFIG["credentials"]["credential_dir"] = r"E:\Disposable SPLINED Acceptance\Credentials"
CONFIG["mode"] = "read"

ALBUM = {
    "id": "album-identified",
    "artist_id": "artist-acceptance",
    "artist_name": "Acceptance Signals",
    "name": "Recovered Frequencies",
    "year": "2026",
    "status": "processed",
    "tracks": 8,
    "embedded": 8,
    "album_mbid": "11111111-1111-4111-8111-111111111111",
    "cover": "/assets/splined-dashboard-icon.png",
    "cover_found": True,
    "candidates": [],
    "track_titles": ["Recovery", "Trace", "Reconcile"],
}
ARTIST = {
    "id": "artist-acceptance",
    "name": "Acceptance Signals",
    "status": "complete",
    "albums": [ALBUM],
    "mbid": "22222222-2222-4222-8222-222222222222",
    "genres": ["Electronic", "Ambient"],
    "roles": ["Primary Artist"],
    "local_candidates": [],
    "portrait": "/assets/splined-app-icon.png",
    "banner": "",
    "top_tracks": ["Recovery", "Trace", "Reconcile"],
}

app = FastAPI(title="SPLINED disposable rendered acceptance", docs_url=None, redoc_url=None)


@app.get("/api/status")
def status():
    return {
        "version": "2.0.18",
        "csrf": "disposable-render-harness",
        "authenticated": True,
        "auth_required": False,
        "admin_setup_required": False,
        "username": "Disposable acceptance",
        "setup_required": False,
        "database": "PostgreSQL",
        "database_ready": True,
        "mode": "read",
        "music": CONFIG["library"]["music_library"],
        "artist_art": CONFIG["scan"]["cache_dir"],
        "embedded_thumbnail_size": 1200,
        "comp_repair": False,
        "capabilities": {"configuration": True},
        "error": "",
    }


@app.get("/api/settings")
def settings():
    return {"config": deepcopy(CONFIG), "database_configured": True, "database_managed": True, "error": ""}


@app.post("/api/settings")
def save_settings(payload: dict):
    return {"config": payload.get("config", deepcopy(CONFIG)), "database_configured": True, "database_managed": True, "error": ""}


@app.get("/api/settings/contract")
def settings_contract():
    return CONTRACT


@app.get("/api/artists")
def artists(q: str = ""):
    return [{"id": ARTIST["id"], "name": ARTIST["name"], "albums": 1, "status": "complete", "portrait": ARTIST["portrait"]}]


@app.get("/api/artists/thumbnails")
def artist_thumbnails():
    return [{"id": ARTIST["id"], "portrait": ARTIST["portrait"]}]


@app.get("/api/albums")
def albums(view: str = "albums", q: str = ""):
    return [ALBUM] if view != "fallback" else []


@app.get("/api/artist/overview")
def artist_overview(id: str):
    return {"artist": ARTIST, "comp_albums": []}


@app.get("/api/artist")
def artist(id: str):
    return ARTIST


@app.get("/api/artist/metadata")
def artist_metadata(id: str, refresh: bool = False):
    return {
        "artist": ARTIST,
        "comp_albums": [],
        "source": "Navidrome",
        "biography": "A disposable Artist projection used to verify the recovered WebGUI layout without touching production music or databases.",
        "similar": [{"name": "Signal Coast", "local_id": "", "image": "", "provider_id": "provider-signal-coast"}],
        "top_songs": ["Recovery", "Trace", "Reconcile"],
    }


@app.get("/api/album")
def album(id: str):
    return ALBUM


@app.get("/api/jobs")
def jobs():
    return []


@app.get("/api/jobs/control-state")
def controls():
    return {"pause_after_album": False, "pause_all_albums": False}


@app.post("/api/audit/events")
def browser_event(payload: dict):
    return {"ok": True}


@app.get("/api/diagnostics")
def diagnostics():
    return [
        {"sequence": 1, "created_at": "2026-11-01T12:00:00Z", "severity": "info", "action": "selection-change", "subject": "artist", "job_id": "", "trace_id": "trace-acceptance", "state_from": "", "state_to": "", "error": "", "details": {"artist_id": ARTIST["id"]}},
        {"sequence": 2, "created_at": "2026-11-01T12:00:01Z", "severity": "warning", "action": "provider-result", "subject": "Navidrome", "job_id": "", "trace_id": "trace-acceptance", "state_from": "requested", "state_to": "completed", "error": "", "details": {"similar": 1}},
    ]


@app.get("/api/storage")
def storage():
    return {
        "version": "2.0.18",
        "mode": "read",
        "database": "PostgreSQL · disposable render harness",
        "database_bytes": 0,
        "cache": CONFIG["scan"]["cache_dir"],
        "artist_art": CONFIG["scan"]["cache_dir"],
        "credentials": CONFIG["credentials"]["credential_dir"],
        "configuration": "Disposable in-memory Config v5",
    }


@app.get("/api/reconciliation/sources")
def reconciliation_sources():
    return [{"path": r"E:\Disposable SPLINED Acceptance\Cache\snapshot.sqlite", "label": "snapshot.sqlite", "bytes": 4096, "modified_at": "2026-11-01T12:00:00Z"}]


@app.get("/api/reconciliation")
def reconciliation(resolution: str = ""):
    rows = [
        {"id": 1, "run_id": "acceptance-run", "source_label": "snapshot.sqlite", "entity_table": "albums", "entity_key": '["album-identified"]', "difference_type": "update", "resolution": "conflict", "application_outcome": "pending", "error": ""}
    ]
    if resolution and resolution != "conflict":
        rows = []
    return {
        "differences": rows,
        "last_run": {"run_id": "acceptance-run", "status": "compared", "differences": 1, "safe_differences": 0, "conflicts": 1, "applied": 0, "failures": 0},
        "pending_safe": 0,
        "settings": {"enabled": False, "auto_apply_safe": False, "interval_minutes": 1440, "source_path": "", "source_root": r"D:\Music", "target_root": CONFIG["library"]["music_library"]},
    }


@app.exception_handler(Exception)
async def error_handler(_request, error):
    return JSONResponse({"error": str(error)}, status_code=400)


app.mount("/", StaticFiles(directory=STATIC, html=True), name="static")
