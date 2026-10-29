"""Shared provider, imaging, configuration, and processing engine for WebGUI."""

from __future__ import annotations

import atexit
import base64
import concurrent.futures
from collections import deque
import faulthandler
import html as html_lib
import hashlib
import importlib.util
import io
import json
import os
import platform
import secrets
import re
import shutil
import signal
import sqlite3
import sys
import tempfile
import threading
import time
import tomllib
import traceback
from urllib.parse import urlencode, urlsplit
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable

import requests
from mutagen import File as MutagenFile
from mutagen.flac import FLAC
from mutagen.id3 import ID3
from mutagen.mp4 import MP4
from PIL import Image

from interaction.status import active as ui_active
from interaction.status import cancelled as ui_cancelled
from interaction.status import emit as emit_ui
from interaction.status import read_input
from interaction.picker_index import (
    should_ignore as picker_should_ignore,
)
from interaction.result_filters import order_musicbrainz_results, musicbrainz_title_match
from splined_db_audit import connect_sqlite

USER_AGENT = "SPLINED/2.0.6 (https://github.com/scottia/S-P-L-I-N-E-D)"
MB_BASE = "https://musicbrainz.org/ws/2"
MB_AUTHORIZE_URL = "https://musicbrainz.org/oauth2/authorize"
MB_OAUTH_ENDPOINT = "https://musicbrainz.org/oauth2/token"
LASTFM_API_URL = "https://ws.audioscrobbler.com/2.0/"
LASTFM_AUTH_URL = "https://www.last.fm/api/auth/"
FANARTTV_API_VERSION = "v3.2"
FANARTTV_API_BASE = f"https://webservice.fanart.tv/{FANARTTV_API_VERSION}"
REQUEST_TIMEOUT = 20
MAX_DOWNLOAD_BYTES = 25 * 1024 * 1024
INVENTORY_WORKERS = 8
DOWNLOAD_CHUNK_BYTES = 64 * 1024
MAX_IMAGE_PIXELS = 64 * 1024 * 1024
PROVIDER_DISCOVERY_WORKERS = 4
CANDIDATE_DOWNLOAD_WORKERS = 4
SELECTED_STATS_LIMIT = 10
AUDIO_EXTENSIONS = {".mp3", ".flac", ".m4a", ".mp4", ".ogg", ".opus", ".wav", ".aiff", ".aif"}
SUPPORTED_SOURCES = (
    "deezer",
    "itunes",
    "fanarttv",
    "lastfm",
    "musicbrainz",
    "coverartarchive",
    "discogs",
    "amazon",
    "navidrome",
)
SUPPORTED_SOURCE_POLICIES = SUPPORTED_SOURCES
FIXED_CREDENTIAL_FILES = {
    "fanarttv": "fanarttv.json",
    "lastfm": "lastfm.json",
    "discogs": "discogs.json",
    "musicbrainz": "musicbrainz.json",
    "navidrome": "navidrome.json",
}
RANGE_TYPES = (
    "BelowMinimum",
    "LowerRange",
    "Ideal",
    "UpperRange",
    "Ladder",
    "AboveLadder",
)
EXTENSIONS = {"jpeg": "jpg", "png": "png", "webp": "webp"}
NAVIDROME_INTERNAL_SCHEME = "splined+navidrome://"
NAVIDROME_PROTOCOL_VERSION = "1.16.1"
NAVIDROME_CLIENT_NAME = "SPLINED"
_NAVIDROME_SESSION_LOCK = threading.Lock()
_NAVIDROME_SESSIONS: dict[int, dict[str, str]] = {}
_NAVIDROME_NEXT_SESSION = 1


class SplinedError(RuntimeError):
    pass


class InteractionCancelled(RuntimeError):
    """Internal control signal for an explicit Select Media session exit."""


class InteractionReturn(RuntimeError):
    """Return an active Candidate Decision batch to its report without ending the browser session."""




@dataclass
class Track:
    path: Path
    title: str
    artist: str
    album: str | None
    album_artist: str | None
    album_mbid: str | None
    recording_mbid: str | None
    compilation: str | None
    artist_mbid: str | None = None


@dataclass
class AlbumDir:
    path: Path
    audio_files: list[Path]
    # Populated by the lightweight filesystem inventory.  These paths are
    # detected by filename/extension only; no image is opened before Launch.
    local_art_files: list[Path] = field(default_factory=list)
    # When recent retained history requires timeout validation, inventory can
    # derive the audio fingerprint from the same os.scandir metadata pass.
    # Completion writes deliberately recompute it so execution authority never
    # relies on stale preselection metadata.
    inventory_fingerprint: str | None = None




@dataclass
class Release:
    mbid: str
    title: str
    artist_credit: str
    release_group_id: str | None
    release_group_title: str | None
    track_count: int | None = None
    external_urls: list[str] = field(default_factory=list)
    asin: str | None = None


@dataclass
class Ref:
    source: str
    id: str
    url: str
    front: bool = True
    approved: bool = True
    types: list[str] = field(default_factory=lambda: ["Front"])
    width: int | None = None
    height: int | None = None
    # Final URL after redirects.  Browser presentation should use this
    # instead of the discovery URL when available.
    browser_url: str = ""


@dataclass
class Candidate:
    ref: Ref
    path: Path
    width: int
    height: int
    format: str
    source_priority: int
    # Populated by splined_strict_source_policy when a configured source uses
    # content evidence as its Preferred/Auto authority.  Defaults preserve the
    # historical ranking path for every non-strict source.
    strict_status: str = "not-applicable"
    strict_reason: str = ""
    strict_preferred_eligible: bool = True
    strict_auto_eligible: bool = True
    strict_match_distance: int | None = None
    strict_match_source: str = ""
    strict_local_validated: bool = False

    @property
    def source(self) -> str:
        return self.ref.source

    @property
    def short_side(self) -> int:
        return min(self.width, self.height)

    @property
    def square(self) -> bool:
        return self.width == self.height


@dataclass
class Summary:
    albums: int = 0
    resolved: int = 0
    unresolved: int = 0
    failed: int = 0
    postponed: int = 0
    selected: int = 0
    samples_written: int = 0
    samples_unchanged: int = 0
    installed: int = 0
    unchanged: int = 0
    read_only: int = 0


def section(cfg: dict[str, Any], name: str) -> dict[str, Any]:
    v = cfg.get(name, {})
    return v if isinstance(v, dict) else {}


def resolve_path(config_file: Path, raw: str) -> Path:
    p = Path(raw.strip())
    return p if p.is_absolute() else (config_file.parent / p).resolve()


DEFAULT_CACHE_DIR = Path("/_cache")
DEFAULT_LOG_DIR = Path("/_logs")
DEFAULT_CREDENTIAL_DIR = Path("/credentials")


def runtime_cache_dir(config_file: Path, cfg: dict[str, Any]) -> Path:
    configured = str(section(cfg, "scan").get("cache_dir", "")).strip()
    if configured:
        return resolve_path(config_file, configured)
    return DEFAULT_CACHE_DIR


def runtime_artist_art_dir(config_file: Path, cfg: dict[str, Any]) -> Path:
    configured = str(section(cfg, "scan").get("shared_artist_art_dir", "")).strip()
    if configured:
        return resolve_path(config_file, configured)
    return runtime_cache_dir(config_file, cfg) / "artist-art"


def runtime_credential_dir(config_file: Path, cfg: dict[str, Any]) -> Path:
    configured = str(section(cfg, "credentials").get("credential_dir", "")).strip()
    if configured:
        return resolve_path(config_file, configured)
    return DEFAULT_CREDENTIAL_DIR


def runtime_log_dir(config_file: Path, cfg: dict[str, Any]) -> Path:
    env = os.environ.get("SPLINED_LOG_DIR", "").strip()
    if env:
        return Path(env)
    configured = str(section(cfg, "scan").get("log_dir", "")).strip()
    if configured:
        return resolve_path(config_file, configured)
    return DEFAULT_LOG_DIR


def ensure_runtime_directories(
    config_file: Path,
    cfg: dict[str, Any],
    cache: Path,
) -> Path:
    logs = runtime_log_dir(config_file, cfg)
    for path in (cache, logs):
        if path.exists() and (path.is_symlink() or not path.is_dir()):
            raise SplinedError(f"Unsafe SPLINED runtime directory: {path}")
        path.mkdir(parents=True, exist_ok=True)
    return logs


def normalize_sources(values: Any, label: str, allow_empty: bool) -> list[str]:
    out: list[str] = []
    if not isinstance(values, list):
        values = []
    for raw in values:
        value = str(raw).strip().lower()
        if not value:
            continue
        if value not in SUPPORTED_SOURCES:
            raise SplinedError(f"Unsupported SPLINED cover source in {label}: {raw}")
        if value not in out:
            out.append(value)
    if not out and not allow_empty:
        raise SplinedError(f"SPLINED {label} cannot be empty.")
    return out


def resolve_sources(cfg: dict[str, Any], cover: list[str] | None, only: list[str] | None, exclude: list[str]) -> list[str]:
    scfg = section(cfg, "sources")
    configured = normalize_sources(scfg.get("cover_sources", []), "configured cover_sources", False)
    configured_exclude = normalize_sources(scfg.get("exclude_cover_sources", []), "configured exclude_cover_sources", True)
    cli_exclude = normalize_sources(exclude, "--exclude-cover-sources", True)
    if only is not None:
        base = normalize_sources(only, "--only-cover-sources", False)
    elif cover is not None:
        base = normalize_sources(cover, "--cover-sources", False)
    else:
        base = configured
    return [
        source
        for source in base
        if source not in configured_exclude
        and source not in cli_exclude
        and source_policy(cfg, source)["enabled"]
    ]


def _bool_value(value: Any, label: str) -> bool:
    if not isinstance(value, bool):
        raise SplinedError(f"{label} must be true or false.")
    return value


def _optional_positive_int(value: Any, label: str) -> int | None:
    if value is None:
        return None
    if isinstance(value, bool):
        raise SplinedError(f"{label} must be a positive integer when configured.")
    try:
        parsed = int(value)
    except (TypeError, ValueError) as exc:
        raise SplinedError(f"{label} must be a positive integer when configured.") from exc
    if parsed <= 0:
        raise SplinedError(f"{label} must be greater than zero when configured.")
    return parsed


def source_policy(cfg: dict[str, Any], source: str) -> dict[str, Any]:
    source_key = source.strip().lower()
    policies = section(cfg, "source_policies")
    raw = policies.get(source_key, {})
    if raw is None:
        raw = {}
    if not isinstance(raw, dict):
        raise SplinedError(f"[source_policies.{source_key}] must be a table.")

    minimum_range_type = str(raw.get("minimum_range_type", "LowerRange")).strip()
    aliases = {item.lower(): item for item in RANGE_TYPES}
    aliases.update({
        "below_minimum": "BelowMinimum",
        "lower_range": "LowerRange",
        "upper_range": "UpperRange",
        "above_ladder": "AboveLadder",
    })
    normalized_range = aliases.get(minimum_range_type.lower())
    if normalized_range is None:
        raise SplinedError(
            f"[source_policies.{source_key}].minimum_range_type must be one of "
            + ", ".join(RANGE_TYPES)
            + "."
        )

    policy = {
        "enabled": _bool_value(
            raw.get("enabled", source_key != "amazon"),
            f"[source_policies.{source_key}].enabled",
        ),
        "source_override": _bool_value(
            raw.get("source_override", False),
            f"[source_policies.{source_key}].source_override",
        ),
        "strict_override": _bool_value(
            raw.get("strict_override", source_key == "amazon"),
            f"[source_policies.{source_key}].strict_override",
        ),
        "minimum_range_type": normalized_range,
        "allow_below_minimum_fallback": _bool_value(
            raw.get("allow_below_minimum_fallback", False),
            f"[source_policies.{source_key}].allow_below_minimum_fallback",
        ),
        "minimum_short_side": _optional_positive_int(
            raw.get("minimum_short_side"),
            f"[source_policies.{source_key}].minimum_short_side",
        ),
        "maximum_short_side": _optional_positive_int(
            raw.get("maximum_short_side"),
            f"[source_policies.{source_key}].maximum_short_side",
        ),
        "minimum_width": _optional_positive_int(
            raw.get("minimum_width"),
            f"[source_policies.{source_key}].minimum_width",
        ),
        "minimum_height": _optional_positive_int(
            raw.get("minimum_height"),
            f"[source_policies.{source_key}].minimum_height",
        ),
        "primary_image_only": _bool_value(
            raw.get("primary_image_only", True),
            f"[source_policies.{source_key}].primary_image_only",
        ),
    }
    minimum = policy["minimum_short_side"]
    maximum = policy["maximum_short_side"]
    if minimum is not None and maximum is not None and minimum > maximum:
        raise SplinedError(
            f"[source_policies.{source_key}].minimum_short_side cannot exceed maximum_short_side."
        )
    return policy


def source_policy_decision(
    cfg: dict[str, Any],
    source: str,
    width: int,
    height: int,
) -> tuple[str, str]:
    policy = source_policy(cfg, source)
    short_side = min(width, height)
    range_type = classify_range(short_side, cfg)

    # Strict content evidence supersedes source-specific scale authority.  The
    # candidate still uses the global range for display and output policy, but
    # its Preferred/Auto eligibility is decided by decoded image comparison.
    if policy["strict_override"] or not policy["source_override"]:
        if range_type not in {"BelowMinimum", "AboveLadder"}:
            return "accept", "Accepted by the global artwork range"
        if range_type == "BelowMinimum":
            return "reject", f"Short side {short_side}px is below the global minimum"
        return "reject", f"Short side {short_side}px is above the global ladder"

    minimum_short_side = policy["minimum_short_side"]
    maximum_short_side = policy["maximum_short_side"]
    minimum_width = policy["minimum_width"]
    minimum_height = policy["minimum_height"]
    if minimum_short_side is not None and short_side < minimum_short_side:
        return "reject", f"Short side {short_side}px is below source minimum {minimum_short_side}px"
    if maximum_short_side is not None and short_side > maximum_short_side:
        return "reject", f"Short side {short_side}px is above source maximum {maximum_short_side}px"
    if minimum_width is not None and width < minimum_width:
        return "reject", f"Width {width}px is below source minimum {minimum_width}px"
    if minimum_height is not None and height < minimum_height:
        return "reject", f"Height {height}px is below source minimum {minimum_height}px"

    range_rank = RANGE_TYPES.index(range_type)
    minimum_rank = RANGE_TYPES.index(policy["minimum_range_type"])
    if range_rank < minimum_rank:
        if policy["allow_below_minimum_fallback"] and range_rank + 1 == minimum_rank:
            return "fallback", "Candidate is the single range immediately below the source minimum"
        return "reject", "Candidate range is below the source minimum"
    return "accept", "Candidate meets the source minimum"


def reference_allowed(cfg: dict[str, Any], source: str, front: bool) -> bool:
    policy = source_policy(cfg, source)
    if policy["source_override"] and not policy["strict_override"]:
        return not policy["primary_image_only"] or front
    return front


SOURCE_HISTORY_VERSION = 1
SOURCE_HISTORY_CACHE_KEY = "source-selection-stats"


def source_history_path(cache_dir: Path) -> Path:
    return Path(cache_dir) / "splined.db"


def _load_cache_state(database: Path, key: str) -> dict[str, Any] | None:
    if not database.exists():
        return None
    try:
        connection = connect_sqlite(database, timeout=30.0)
        row = connection.execute(
            "SELECT payload_json FROM cache_entries WHERE cache_key=?",
            (key,),
        ).fetchone()
    except sqlite3.Error:
        return None
    finally:
        if "connection" in locals():
            connection.close()
    if row is None:
        return None
    try:
        value = json.loads(str(row[0]))
    except (TypeError, ValueError, json.JSONDecodeError):
        return None
    return value if isinstance(value, dict) else None


def _save_cache_state(
    database: Path,
    key: str,
    cache_type: str,
    value: dict[str, Any],
) -> None:
    if not database.exists():
        raise SplinedError(f"SPLINED database is missing: {database}")
    now = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    body = json.dumps(value, sort_keys=True, separators=(",", ":"))
    try:
        connection = connect_sqlite(database, timeout=30.0)
        with connection:
            connection.execute(
                "INSERT INTO cache_entries"
                "(cache_key, cache_type, album_key, payload_json, "
                "splined_version, created_at, updated_at) "
                "VALUES(?, ?, NULL, ?, ?, ?, ?) "
                "ON CONFLICT(cache_key) DO UPDATE SET "
                "cache_type=excluded.cache_type, "
                "payload_json=excluded.payload_json, "
                "splined_version=excluded.splined_version, "
                "updated_at=excluded.updated_at",
                (key, cache_type, body, display_version(), now, now),
            )
    except sqlite3.Error as exc:
        raise SplinedError(
            f"Unable to persist SPLINED SQLite runtime state: {exc}"
        ) from exc
    finally:
        if "connection" in locals():
            connection.close()


def empty_source_history() -> dict[str, Any]:
    return {
        "version": SOURCE_HISTORY_VERSION,
        "sources": {
            source: {
                "selected": 0,
                "ideal": 0,
                "acceptable": 0,
                "last_selected_unix": None,
            }
            for source in SUPPORTED_SOURCES
        },
    }


def load_source_history(path: Path) -> dict[str, Any]:
    history = empty_source_history()
    raw = _load_cache_state(path, SOURCE_HISTORY_CACHE_KEY)
    if raw is None:
        return history
    if not isinstance(raw, dict):
        return history
    raw_sources = raw.get("sources")
    if not isinstance(raw_sources, dict):
        return history

    for source in SUPPORTED_SOURCES:
        entry = raw_sources.get(source)
        if not isinstance(entry, dict):
            continue
        target = history["sources"][source]
        for key in ("selected", "ideal", "acceptable"):
            try:
                target[key] = max(0, int(entry.get(key, 0)))
            except (TypeError, ValueError):
                target[key] = 0
        last = entry.get("last_selected_unix")
        try:
            target["last_selected_unix"] = float(last) if last is not None else None
        except (TypeError, ValueError):
            target["last_selected_unix"] = None
    return history


def save_source_history(path: Path, history: dict[str, Any]) -> None:
    _save_cache_state(path, SOURCE_HISTORY_CACHE_KEY, "source_history", history)


def source_selected_count(history: dict[str, Any], source: str) -> int:
    try:
        return max(0, int(history["sources"][source]["selected"]))
    except (KeyError, TypeError, ValueError):
        return 0


def source_ideal_count(history: dict[str, Any], source: str) -> int:
    try:
        return max(0, int(history["sources"][source]["ideal"]))
    except (KeyError, TypeError, ValueError):
        return 0


def ranked_sources_from_history(sources: list[str], history: dict[str, Any]) -> list[str]:
    # History determines request priority. Configured order is only the tie-break
    # for sources with equal history, so no provider is hard-coded first.
    configured_index = {source: index for index, source in enumerate(sources)}
    return sorted(
        sources,
        key=lambda source: (
            -source_selected_count(history, source),
            -source_ideal_count(history, source),
            configured_index[source],
        ),
    )


def record_source_selection(
    path: Path,
    history: dict[str, Any],
    candidate: Candidate,
    cfg: dict[str, Any],
    format_order: list[str],
) -> None:
    if not bool(section(cfg, "history").get("enabled", True)):
        return
    source = candidate.source
    if source not in SUPPORTED_SOURCES:
        return
    entry = history.setdefault("sources", {}).setdefault(
        source,
        {"selected": 0, "ideal": 0, "acceptable": 0, "last_selected_unix": None},
    )
    projected = project_candidate(candidate, cfg, target_format_for_candidate(candidate, format_order))
    entry["selected"] = max(0, int(entry.get("selected", 0))) + 1
    if projected["range_type"] == "Ideal":
        entry["ideal"] = max(0, int(entry.get("ideal", 0))) + 1
    if projected["acceptable"]:
        entry["acceptable"] = max(0, int(entry.get("acceptable", 0))) + 1
    entry["last_selected_unix"] = time.time()
    history["version"] = SOURCE_HISTORY_VERSION
    save_source_history(path, history)



SCAN_COMPLETION_HISTORY_VERSION = 1
SCAN_COMPLETION_CACHE_KEY = "album-completion-state"


def scan_completion_history_path(cache_dir: Path) -> Path:
    return Path(cache_dir) / "splined.db"


def empty_scan_completion_history() -> dict[str, Any]:
    return {
        "version": SCAN_COMPLETION_HISTORY_VERSION,
        "albums": {},
    }


def load_scan_completion_history(
    path: Path,
    cfg: dict[str, Any] | None = None,
) -> dict[str, Any]:
    history = empty_scan_completion_history()
    if cfg is not None and not bool(section(cfg, "history").get("enabled", True)):
        return history
    raw = _load_cache_state(path, SCAN_COMPLETION_CACHE_KEY)
    if raw is None:
        return history
    if not isinstance(raw, dict):
        return history
    albums = raw.get("albums")
    if not isinstance(albums, dict):
        return history
    history["albums"] = {
        str(key): value
        for key, value in albums.items()
        if isinstance(value, dict)
    }
    if cfg is not None:
        retention_days = _non_negative_int(
            section(cfg, "history").get("retention_days", 0),
            "[history].retention_days",
        )
        if retention_days > 0:
            cutoff = time.time() - retention_days * 86_400
            history["albums"] = {
                key: value
                for key, value in history["albums"].items()
                if _history_entry_timestamp(value) >= cutoff
            }
    return history


def _history_entry_timestamp(entry: dict[str, Any]) -> float:
    try:
        return float(entry.get("completed_at_unix", 0.0))
    except (TypeError, ValueError):
        return 0.0


def save_scan_completion_history(path: Path, history: dict[str, Any]) -> None:
    history["version"] = SCAN_COMPLETION_HISTORY_VERSION
    _save_cache_state(path, SCAN_COMPLETION_CACHE_KEY, "album_history", history)



def scan_timeout_hours(cfg: dict[str, Any]) -> float:
    raw = section(cfg, "scan").get("scan_mode_timeout", 24)
    if isinstance(raw, bool):
        if raw is False:
            return 0.0
        raise SplinedError("[scan].scan_mode_timeout must be a non-negative number of hours, 'off', or false.")
    if isinstance(raw, str):
        value = raw.strip().lower()
        if value == "off":
            return 0.0
        raw = value
    try:
        value = float(raw)
    except (TypeError, ValueError) as exc:
        raise SplinedError("[scan].scan_mode_timeout must be a non-negative number of hours, 'off', or false.") from exc
    if value < 0:
        raise SplinedError("[scan].scan_mode_timeout cannot be negative.")
    return value


def format_timeout_hours(value: float) -> str:
    if value <= 0:
        return "off"
    return str(int(value)) if float(value).is_integer() else f"{value:g}"

def _fingerprint_from_metadata(
    values: list[tuple[Path, int | None, int | None]],
) -> str:
    digest = hashlib.sha256()
    for path, size, modified_ns in sorted(
        values, key=lambda item: str(item[0]).lower()
    ):
        if size is not None and modified_ns is not None:
            payload = (
                f"{path.name}\0{size}\0{modified_ns}\0"
            ).encode("utf-8", "surrogateescape")
        else:
            payload = f"{path.name}\0unstatable\0".encode(
                "utf-8", "surrogateescape"
            )
        digest.update(payload)
    return digest.hexdigest()


def album_scan_fingerprint(
    album: AlbumDir,
    *,
    use_inventory_cache: bool = False,
) -> str:
    if use_inventory_cache and album.inventory_fingerprint is not None:
        return album.inventory_fingerprint
    values: list[tuple[Path, int | None, int | None]] = []
    for path in album.audio_files:
        try:
            stat = path.stat()
            values.append((path, stat.st_size, stat.st_mtime_ns))
        except OSError:
            values.append((path, None, None))
    return _fingerprint_from_metadata(values)


def scan_policy_fingerprint(
    cfg: dict[str, Any],
    sources: list[str],
) -> str:
    # A timeout entry is reusable only while the processing policy is unchanged.
    # This prevents a prior read run, source change, or range/output change from
    # suppressing a newly meaningful scan.
    payload = {
        "mode": str(cfg.get("mode", "read")).strip().lower(),
        "sources": list(sources),
        "range": section(cfg, "range"),
        "source_policies": section(cfg, "source_policies"),
        "output": section(cfg, "output"),
        "samples": section(cfg, "samples"),
    }
    encoded = json.dumps(
        payload,
        sort_keys=True,
        separators=(",", ":"),
        default=str,
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def scan_completion_status(
    history: dict[str, Any],
    album: AlbumDir,
    cfg: dict[str, Any],
    sources: list[str],
    timeout_hours: float,
    now: float | None = None,
    *,
    policy_fingerprint: str | None = None,
) -> tuple[bool, float]:
    if not bool(section(cfg, "history").get("enabled", True)):
        return False, 0.0
    if timeout_hours <= 0:
        return False, 0.0

    entry = history.get("albums", {}).get(str(album.path))
    if not isinstance(entry, dict):
        return False, 0.0

    try:
        completed_at = float(entry.get("completed_at_unix"))
    except (TypeError, ValueError):
        return False, 0.0

    current = time.time() if now is None else now
    age_seconds = max(0.0, current - completed_at)
    if age_seconds >= timeout_hours * 3600:
        return False, age_seconds / 3600

    expected_policy = policy_fingerprint or scan_policy_fingerprint(cfg, sources)
    if entry.get("policy_fingerprint") != expected_policy:
        return False, age_seconds / 3600

    if entry.get("album_fingerprint") != album_scan_fingerprint(
        album, use_inventory_cache=True
    ):
        return False, age_seconds / 3600

    return True, age_seconds / 3600


def timeout_fingerprint_paths(
    history: dict[str, Any],
    cfg: dict[str, Any],
    sources: list[str],
    timeout_hours: float,
    now: float | None = None,
) -> set[str]:
    """Return only history paths whose audio metadata must be validated.

    Policy and age checks are independent of the filesystem. Applying them
    before inventory avoids stat calls for expired or policy-incompatible
    history while preserving the existing timeout decision.
    """
    if not bool(section(cfg, "history").get("enabled", True)):
        return set()
    if timeout_hours <= 0:
        return set()
    entries = history.get("albums", {})
    if not isinstance(entries, dict):
        return set()
    current = time.time() if now is None else now
    expected_policy = scan_policy_fingerprint(cfg, sources)
    paths: set[str] = set()
    for path, entry in entries.items():
        if not isinstance(entry, dict):
            continue
        try:
            completed_at = float(entry.get("completed_at_unix"))
        except (TypeError, ValueError):
            continue
        age_seconds = max(0.0, current - completed_at)
        if age_seconds >= timeout_hours * 3600:
            continue
        if entry.get("policy_fingerprint") != expected_policy:
            continue
        paths.add(str(path))
    return paths


def record_scan_completion(
    path: Path,
    history: dict[str, Any],
    album: AlbumDir,
    cfg: dict[str, Any],
    sources: list[str],
    outcome: str,
) -> None:
    # Presentation receives the same authoritative completion outcome even in
    # read mode, where persistent completion history intentionally remains
    # untouched.
    persistent = (
        bool(section(cfg, "history").get("enabled", True))
        and str(cfg.get("mode", "read")).strip().lower() != "read"
    )
    emit_ui(
        "history",
        album=str(album.path),
        outcome=str(outcome),
        persistent=persistent,
    )
    if not persistent:
        return
    albums = history.setdefault("albums", {})
    prior = albums.get(str(album.path), {})
    retained_enhanced = (
        prior.get("enhanced_results", []) if isinstance(prior, dict) else []
    )
    albums[str(album.path)] = {
        "completed_at_unix": time.time(),
        "album_fingerprint": album_scan_fingerprint(album),
        "policy_fingerprint": scan_policy_fingerprint(cfg, sources),
        "outcome": str(outcome),
    }
    # A normal SPLINED completion refreshes scan state, but must not erase
    # independently validated AISPLINE provenance recorded by a future adapter.
    if retained_enhanced:
        albums[str(album.path)]["enhanced_results"] = retained_enhanced
    history["version"] = SCAN_COMPLETION_HISTORY_VERSION
    save_scan_completion_history(path, history)
    debug_log(
        f"scan.complete album={str(album.path)!r} outcome={outcome}"
    )



def formats(cfg: dict[str, Any]) -> list[str]:
    raw = section(cfg, "output").get("file_formats", [])
    if not isinstance(raw, list):
        raise SplinedError("[output].file_formats must be a list.")
    out: list[str] = []
    for v in raw:
        v = str(v).strip().lower()
        if v not in {"jpeg", "png", "webp"}:
            raise SplinedError(f"Unsupported SPLINED static output format: {v}")
        if v not in out:
            out.append(v)
    if not out:
        raise SplinedError("SPLINED output file_formats cannot be empty.")
    return out


def should_ignore(name: str, patterns: list[str]) -> bool:
    return picker_should_ignore(name, patterns)


def _is_inventory_local_art(path: Path, configured_file_name: str) -> bool:
    if path.suffix.lower() not in {".jpg", ".jpeg", ".png", ".webp"}:
        return False
    stem = path.stem.casefold()
    configured = (configured_file_name.strip() or "cover").casefold()
    return stem.startswith("cover") or stem.startswith(configured)


def inventory(
    root: Path,
    ignored_subs: list[str],
    configured_file_name: str = "cover",
    *,
    fingerprint_paths: set[str] | None = None,
    progress: Callable[[int, int], None] | None = None,
    workers: int = INVENTORY_WORKERS,
    cancelled: Callable[[], bool] | None = None,
) -> tuple[list[AlbumDir], list[Path]]:
    if not root.exists():
        raise SplinedError(f"SPLINED scan directory does not exist: {root}")
    if not root.is_dir():
        raise SplinedError(f"SPLINED scan path is not a directory: {root}")
    albums: list[AlbumDir] = []
    ignored: list[Path] = []
    fingerprint_required = fingerprint_paths or set()
    directories_seen = 0
    worker_count = max(1, min(32, int(workers)))

    def inspect_directory(
        directory: Path,
    ) -> tuple[AlbumDir | None, list[Path]]:
        try:
            with os.scandir(directory) as iterator:
                entries = sorted(iterator, key=lambda entry: entry.name.lower())
        except OSError as exc:
            raise SplinedError(f"Unable to read SPLINED scan directory {directory}: {exc}") from exc
        audio: list[Path] = []
        audio_metadata: list[tuple[Path, int | None, int | None]] = []
        local_art: list[Path] = []
        children: list[Path] = []
        collect_fingerprint = str(directory) in fingerprint_required
        for entry in entries:
            if entry.is_symlink():
                continue
            path = Path(entry.path)
            try:
                if entry.is_dir(follow_symlinks=False):
                    children.append(path)
                    continue
                if not entry.is_file(follow_symlinks=False):
                    continue
            except OSError:
                # Match the prior Path.is_dir()/is_file() behavior: an entry
                # that cannot be classified is skipped without aborting the
                # rest of the readable directory.
                continue
            if path.suffix.lower() in AUDIO_EXTENSIONS:
                audio.append(path)
                if collect_fingerprint:
                    try:
                        stat = entry.stat(follow_symlinks=False)
                        audio_metadata.append(
                            (path, stat.st_size, stat.st_mtime_ns)
                        )
                    except OSError:
                        audio_metadata.append((path, None, None))
            elif _is_inventory_local_art(path, configured_file_name):
                local_art.append(path)
        album: AlbumDir | None = None
        if audio:
            album = AlbumDir(
                directory,
                sorted(audio),
                sorted(local_art),
                (
                    _fingerprint_from_metadata(audio_metadata)
                    if collect_fingerprint
                    else None
                ),
            )
        return album, children

    # Directory opens dominate large NAS/CIFS/NFS libraries. Keep only a small,
    # bounded number in flight while retaining the same final path ordering and
    # per-directory evaluation rules as the former recursive traversal.
    queued: deque[Path] = deque([root])
    pending: dict[
        concurrent.futures.Future[tuple[AlbumDir | None, list[Path]]],
        Path,
    ] = {}
    with concurrent.futures.ThreadPoolExecutor(
        max_workers=worker_count,
        thread_name_prefix="splined-inventory",
    ) as executor:
        while queued or pending:
            if cancelled is not None and cancelled():
                raise InteractionCancelled()
            while queued and len(pending) < worker_count * 2:
                if cancelled is not None and cancelled():
                    raise InteractionCancelled()
                directory = queued.popleft()
                pending[executor.submit(inspect_directory, directory)] = directory
            done, _ = concurrent.futures.wait(
                pending,
                return_when=concurrent.futures.FIRST_COMPLETED,
            )
            if cancelled is not None and cancelled():
                raise InteractionCancelled()
            for future in done:
                pending.pop(future)
                album, children = future.result()
                directories_seen += 1
                if album is not None:
                    albums.append(album)
                for child in children:
                    if should_ignore(child.name, ignored_subs):
                        ignored.append(child)
                    else:
                        queued.append(child)
                if progress is not None and directories_seen % 250 == 0:
                    progress(directories_seen, len(albums))

    if progress is not None:
        progress(directories_seen, len(albums))
    albums.sort(key=lambda a: str(a.path).lower())
    ignored.sort(key=lambda p: str(p).lower())
    return albums, ignored


def first(value: Any) -> str | None:
    if value is None:
        return None
    if isinstance(value, (list, tuple)):
        if not value:
            return None
        value = value[0]
    if isinstance(value, bytes):
        value = value.decode("utf-8", "replace")
    text = str(value).strip()
    return text or None


def all_values(value: Any) -> str | None:
    """Preserve multi-value identifier tags as one parseable text value."""
    text_values = getattr(value, "text", None)
    if text_values is not None:
        value = text_values
    values = value if isinstance(value, (list, tuple)) else [value]
    result: list[str] = []
    for item in values:
        if isinstance(item, bytes):
            item = item.decode("utf-8", "replace")
        text = str(item or "").strip()
        if text:
            result.append(text)
    return "; ".join(result) or None


def musicbrainz_recording_ufid(tags: Any) -> str | None:
    """Read MusicBrainz Recording identity from its standard ID3 UFID."""
    getall = getattr(tags, "getall", None)
    if not callable(getall):
        return None
    for frame in getall("UFID"):
        owner = str(getattr(frame, "owner", "") or "")
        if owner.rstrip("/").casefold() != "http://musicbrainz.org":
            continue
        data = getattr(frame, "data", b"")
        if isinstance(data, bytes):
            return data.decode("ascii", "replace").strip() or None
        return str(data or "").strip() or None
    return None


def read_track(path: Path) -> Track:
    suffix = path.suffix.lower()
    title = artist = album = album_artist = album_mbid = recording_mbid = compilation = artist_mbid = None
    try:
        if suffix == ".mp3":
            tags = ID3(path)
            title = first(tags.get("TIT2")); artist = first(tags.get("TPE1")); album = first(tags.get("TALB")); album_artist = first(tags.get("TPE2"))
            compilation = first(tags.get("TCMP"))
            for f in tags.getall("TXXX"):
                desc = (getattr(f, "desc", "") or "").strip().lower()
                val = first(getattr(f, "text", None))
                if desc in {"musicbrainz album id", "musicbrainz_albumid"}: album_mbid = val
                elif desc in {"musicbrainz recording id", "musicbrainz track id", "musicbrainz_trackid"}: recording_mbid = val
                elif desc in {"musicbrainz artist id", "musicbrainz_artistid"}: artist_mbid = all_values(getattr(f, "text", None))
                elif desc in {"compilation", "itunescompilation"}: compilation = val
            if not recording_mbid:
                recording_mbid = musicbrainz_recording_ufid(tags)
        elif suffix == ".flac":
            f = FLAC(path)
            title = first(f.get("title")); artist = first(f.get("artist")); album = first(f.get("album")); album_artist = first(f.get("albumartist"))
            album_mbid = first(f.get("musicbrainz_albumid")); recording_mbid = first(f.get("musicbrainz_trackid")); artist_mbid = all_values(f.get("musicbrainz_artistid")); compilation = first(f.get("compilation"))
        elif suffix in {".m4a", ".mp4"}:
            tags = MP4(path).tags or {}
            title = first(tags.get("\xa9nam")); artist = first(tags.get("\xa9ART")); album = first(tags.get("\xa9alb")); album_artist = first(tags.get("aART"))
            album_mbid = first(tags.get("----:com.apple.iTunes:MusicBrainz Album Id")); recording_mbid = first(tags.get("----:com.apple.iTunes:MusicBrainz Track Id"))
            artist_mbid = all_values(tags.get("----:com.apple.iTunes:MusicBrainz Artist Id"))
            cpil = tags.get("cpil"); compilation = "1" if cpil and bool(cpil[0]) else None
        else:
            f = MutagenFile(path, easy=True)
            tags = getattr(f, "tags", None) if f is not None else None
            if tags:
                title = first(tags.get("title")); artist = first(tags.get("artist")); album = first(tags.get("album")); album_artist = first(tags.get("albumartist"))
                album_mbid = first(tags.get("musicbrainz_albumid")); recording_mbid = first(tags.get("musicbrainz_trackid")); artist_mbid = all_values(tags.get("musicbrainz_artistid")); compilation = first(tags.get("compilation"))
    except Exception as exc:
        raise SplinedError(f"Unable to read SPLINED audio tags from {path}: {exc}") from exc
    if not title:
        raise SplinedError(f"SPLINED scan requires TITLE in actual file tags: {path}")
    if not artist:
        raise SplinedError(f"SPLINED scan requires ARTIST in actual file tags: {path}")
    return Track(path, title, artist, album, album_artist, album_mbid, recording_mbid, compilation, artist_mbid)


def read_album_tracks(album: AlbumDir) -> list[Track]:
    """Read every authoritative track only after the user launches the album."""
    return [read_track(path) for path in album.audio_files]


def valid_mbid(value: str | None) -> str | None:
    if not value: return None
    value = value.strip().lower()
    return value if re.fullmatch(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}", value) else None


def audit_album_ids(tracks: list[Track]) -> tuple[dict[str, list[Path]], list[Path], list[tuple[Path, str]]]:
    valid: dict[str, list[Path]] = {}; missing: list[Path] = []; invalid: list[tuple[Path, str]] = []
    for t in tracks:
        raw = (t.album_mbid or "").strip()
        if not raw: missing.append(t.path); continue
        mbid = valid_mbid(raw)
        if mbid: valid.setdefault(mbid, []).append(t.path)
        else: invalid.append((t.path, raw))
    return valid, missing, invalid


def normalize_text(value: str) -> str:
    return " ".join("".join(c.lower() if c.isalnum() else " " for c in value).split())


def tagged_album(tracks: list[Track]) -> str | None:
    selected: str | None = None
    for t in tracks:
        v = (t.album or "").strip()
        if not v: continue
        if selected is None: selected = v
        elif normalize_text(selected) != normalize_text(v): return None
    return selected


def tagged_artist(tracks: list[Track]) -> str | None:
    # Prefer a coherent ALBUMARTIST chain. Fall back to coherent ARTIST, then
    # the first usable tagged value so fallback can still search.
    album_artists = [(t.album_artist or "").strip() for t in tracks if (t.album_artist or "").strip()]
    if album_artists:
        first_value = album_artists[0]
        if all(normalize_text(v) == normalize_text(first_value) for v in album_artists):
            return first_value

    artists = [(t.artist or "").strip() for t in tracks if (t.artist or "").strip()]
    if artists:
        first_value = artists[0]
        if all(normalize_text(v) == normalize_text(first_value) for v in artists):
            return first_value
        return first_value
    return None


def embedded_artwork_bytes(path: Path) -> bytes | None:
    """Read the preferred embedded front/cover image from a supported track."""

    try:
        suffix = path.suffix.casefold()
        if suffix == ".mp3":
            pictures = list(ID3(path).getall("APIC"))
            picture = next(
                (item for item in pictures if int(getattr(item, "type", 0) or 0) == 3),
                pictures[0] if pictures else None,
            )
            return bytes(picture.data) if picture is not None else None
        if suffix == ".flac":
            pictures = list(FLAC(path).pictures)
            picture = next(
                (item for item in pictures if int(getattr(item, "type", 0) or 0) == 3),
                pictures[0] if pictures else None,
            )
            return bytes(picture.data) if picture is not None else None
        if suffix in {".m4a", ".mp4"}:
            pictures = list((MP4(path).tags or {}).get("covr") or [])
            return bytes(pictures[0]) if pictures else None
        parsed = MutagenFile(path)
        pictures = getattr(parsed, "pictures", None)
        if pictures:
            picture = next(
                (item for item in pictures if int(getattr(item, "type", 0) or 0) == 3),
                pictures[0],
            )
            data = getattr(picture, "data", None)
            return bytes(data) if data else None
        tags = getattr(parsed, "tags", None)
        if tags:
            encoded = tags.get("coverart") or tags.get("metadata_block_picture")
            if isinstance(encoded, list):
                encoded = encoded[0] if encoded else None
            if encoded:
                return base64.b64decode(str(encoded), validate=True)
    except Exception:
        return None
    return None


def embedded_artwork_present(path: Path) -> bool:
    """Return whether a supported track has an embedded front/cover image."""

    return embedded_artwork_bytes(path) is not None


def embedded_artwork_preview(path: Path, cache: Path) -> dict[str, Any] | None:
    """Materialize one deterministic, disposable embedded-art preview."""

    data = embedded_artwork_bytes(path)
    if not data:
        return None
    try:
        with Image.open(io.BytesIO(data)) as image:
            fmt = image_format(image)
            width, height = image.size
            image.verify()
    except Exception:
        return None
    if cache.exists() and (cache.is_symlink() or not cache.is_dir()):
        return None
    cache.mkdir(parents=True, exist_ok=True)
    digest = hashlib.sha256(
        str(path).encode("utf-8", "surrogateescape") + data
    ).hexdigest()[:24]
    preview = cache / f"splined-track-preview-{digest}.{EXTENSIONS[fmt]}"
    if preview.is_symlink():
        return None
    if not preview.is_file():
        preview.write_bytes(data)
    return {
        "path": str(preview),
        "width": int(width),
        "height": int(height),
        "format": fmt,
    }



def selected_album_statistics(
    album_path: Path,
    *,
    cover_name: str = "cover",
    config_file: Path | None = None,
    cfg: dict[str, Any] | None = None,
    include_tracks: bool = False,
) -> dict[str, Any]:
    """Read on-demand statistics for one explicitly selected Album.

    This is deliberately outside normal lazy Artist discovery. Tag/image work
    begins only after the user checks an Album for processing.
    """
    path = Path(album_path)
    result: dict[str, Any] = {
        "path": str(path),
        "album": path.name,
        "artist": path.parent.name,
        "year": "",
        "tracks": 0,
        "artwork": {"JPEG": 0, "PNG": 0, "WEBP": 0, "OTHER": 0},
        "root_files": 0,
        "cover_files": 0,
        "cover_names": [],
        "cover_resolution": "",
        "cover_path": "",
        "other_filenames": [],
        "webp_found": False,
        "webp_size_mb": 0.0,
        "webp_resolution": "",
        "webp_conversion": False,
        "compilation": False,
        "album_mbid_missing": False,
        "manual_compilation_eligible": False,
        "track_items": [],
    }
    try:
        root_files = sorted(
            (
                entry
                for entry in path.iterdir()
                if entry.is_file() and not entry.is_symlink()
            ),
            key=lambda item: item.name.casefold(),
        )
    except OSError:
        return result

    audio_files = [
        item for item in root_files if item.suffix.lower() in AUDIO_EXTENSIONS
    ]
    mp3_files = [item for item in audio_files if item.suffix.lower() == ".mp3"]
    # Album/Artist/Year authority for this panel comes from Mutagen. Prefer
    # MP3 tags as requested; if an Album has no MP3 tracks, fall back to the
    # other supported audio files through the same Mutagen interface.
    tag_files = mp3_files if mp3_files else audio_files
    result["tracks"] = len(audio_files)

    parsed_tracks: list[Track] = []
    track_items: list[dict[str, Any]] = []
    if include_tracks:
        preview_cache = (
            runtime_cache_dir(config_file, cfg) / "track-preview"
            if config_file is not None and cfg is not None
            else None
        )
        for number, track_path in enumerate(
            sorted(audio_files, key=lambda value: str(value).casefold()), 1
        ):
            try:
                track = read_track(track_path)
            except SplinedError:
                track = Track(
                    track_path,
                    track_path.stem,
                    "",
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                )
            parsed_tracks.append(track)
            preview = (
                embedded_artwork_preview(track.path, preview_cache)
                if preview_cache is not None
                else None
            )
            track_items.append(
                {
                    "number": number,
                    "path": str(track.path),
                    "artist": str(track.artist or ""),
                    "title": str(track.title or track.path.stem),
                    "embedded_art": preview is not None or embedded_artwork_present(track.path),
                    "preview_path": str((preview or {}).get("path", "")),
                    "preview_width": int((preview or {}).get("width", 0) or 0),
                    "preview_height": int((preview or {}).get("height", 0) or 0),
                    "completed": False,
                }
            )
        result["track_items"] = track_items

    # Compilation track-art recovery is exposed from the representative track
    # only after an Album is explicitly selected.  Do not inspect every track
    # and do not contact MusicBrainz while Select Media is open.
    if tag_files:
        try:
            representative_track = read_track(
                sorted(tag_files, key=lambda value: str(value).casefold())[0]
            )
            compilation = str(
                representative_track.compilation or ""
            ).strip().casefold() in {"1", "true", "yes", "y", "on"}
            album_mbid_missing = not str(
                representative_track.album_mbid or ""
            ).strip()
            result["compilation"] = compilation
            result["album_mbid_missing"] = album_mbid_missing
            result["manual_compilation_eligible"] = (
                album_mbid_missing and compilation
            )
        except SplinedError:
            # Scan launch performs the authoritative read and reports the
            # specific failure. Selection statistics remain non-authoritative.
            pass

    if (
        include_tracks
        and result["manual_compilation_eligible"]
        and config_file is not None
        and cfg is not None
        and callable(globals().get("compilation_completed_track_paths"))
    ):
        try:
            completed_paths = compilation_completed_track_paths(  # type: ignore[name-defined]
                config_file,
                cfg,
                str(path),
                parsed_tracks,
            )
            for item in track_items:
                item["completed"] = str(item["path"]) in completed_paths
        except Exception as exc:
            debug_log(
                "selected_stats.track_progress_error "
                f"path={str(path)!r} error={type(exc).__name__}: {exc}"
            )

    debug_log(
        "selected_stats.begin "
        f"path={str(path)!r} audio_files={len(audio_files)} "
        f"mp3_files={len(mp3_files)} tag_reader=mutagen "
        f"tag_files={len(tag_files)}"
    )

    tagged_album_found = False
    tagged_artist_found = False
    tagged_year_found = False
    for track_path in tag_files:
        try:
            parsed = MutagenFile(track_path, easy=True)
        except Exception as exc:
            debug_log(
                "selected_stats.mutagen_error "
                f"file={str(track_path)!r} "
                f"error={type(exc).__name__}: {exc}"
            )
            parsed = None
        tags = getattr(parsed, "tags", None) if parsed is not None else None
        if not tags:
            debug_log(
                f"selected_stats.mutagen_no_tags file={str(track_path)!r}"
            )
            continue

        def first_tag(*keys: str) -> str:
            for key in keys:
                raw = tags.get(key)
                if isinstance(raw, (list, tuple)) and raw:
                    value = str(raw[0]).strip()
                elif raw is not None:
                    value = str(raw).strip()
                else:
                    value = ""
                if value:
                    return value
            return ""

        if not tagged_album_found:
            tagged_album = first_tag("album")
            if tagged_album:
                result["album"] = tagged_album
                tagged_album_found = True
        if not tagged_artist_found:
            tagged_artist = first_tag("albumartist", "artist")
            if tagged_artist:
                result["artist"] = tagged_artist
                tagged_artist_found = True
        if not tagged_year_found:
            raw_year = first_tag("date", "originaldate", "year")
            match = re.search(r"\b(\d{4})\b", raw_year)
            if match:
                result["year"] = match.group(1)
                tagged_year_found = True
        if tagged_album_found and tagged_artist_found and tagged_year_found:
            break

    debug_log(
        "selected_stats.tags "
        f"path={str(path)!r} reader=mutagen "
        f"album={str(result['album'])!r} artist={str(result['artist'])!r} "
        f"year={str(result['year'])!r}"
    )

    sidecars = [item for item in root_files if item not in audio_files]
    cover_prefix = cover_name.strip().casefold() or "cover"
    cover_files: list[Path] = []
    other_files: list[Path] = []
    webps: list[Path] = []
    artwork = {"JPEG": 0, "PNG": 0, "WEBP": 0, "OTHER": 0}
    for item in sidecars:
        suffix = item.suffix.lower()
        if suffix in {".jpg", ".jpeg"}:
            artwork["JPEG"] += 1
        elif suffix == ".png":
            artwork["PNG"] += 1
        elif suffix == ".webp":
            artwork["WEBP"] += 1
            webps.append(item)
        else:
            artwork["OTHER"] += 1
        if item.stem.casefold().startswith(cover_prefix):
            cover_files.append(item)
        else:
            other_files.append(item)

    result["artwork"] = artwork
    result["root_files"] = len(sidecars)
    result["cover_files"] = len(cover_files)
    result["cover_names"] = [item.name for item in cover_files]
    if cover_files:
        # Prefer the canonical cover.* image and expose its actual saved
        # dimensions for the Selected Album Statistics panel.
        cover = sorted(
            cover_files,
            key=lambda item: (
                item.stem.casefold() != cover_prefix,
                item.name.casefold(),
            ),
        )[0]
        result["cover_path"] = str(cover)
        try:
            with Image.open(cover) as image:
                result["cover_resolution"] = f"{int(image.width)}x{int(image.height)}"
        except Exception:
            result["cover_resolution"] = ""
    result["other_filenames"] = [item.name for item in other_files]
    result["webp_found"] = bool(webps)

    if webps:
        def webp_size(item: Path) -> int:
            try:
                return int(item.stat().st_size)
            except OSError:
                return 0

        webp = max(webps, key=webp_size)
        size_bytes = webp_size(webp)
        result["webp_size_mb"] = round(size_bytes / 1_000_000, 2)
        try:
            with Image.open(webp) as image:
                result["webp_resolution"] = f"{int(image.width)}x{int(image.height)}"
                # The operational local-art path converts a decodable WEBP to
                # SPLINED's canonical still when it is selected.
                result["webp_conversion"] = True
        except Exception:
            result["webp_resolution"] = ""
            result["webp_conversion"] = False

    debug_log(
        "selected_stats.done "
        f"path={str(path)!r} root_files={result['root_files']} "
        f"cover_files={result['cover_files']} artwork={result['artwork']} "
        f"webp_found={result['webp_found']} "
        f"webp_resolution={str(result['webp_resolution'])!r}"
    )
    return result




def compact(paths: list[Path]) -> str:
    names = [p.name for p in paths[:5]]
    if len(paths) > 5: names.append(f"+{len(paths)-5} more")
    return ", ".join(names)


_RUNTIME_LOG_PATH: Path | None = None
_RUNTIME_LOG_HANDLE: io.TextIOBase | None = None
_RUNTIME_LOG_LOCK = threading.Lock()
_RUNTIME_LOG_PENDING = 0
_RUNTIME_LOG_LAST_FLUSH = 0.0
_RUNTIME_FAULTHANDLER_ENABLED = False
_RUNTIME_VERBOSITY = "info"
_DEBUG_ENABLED = False
RUNTIME_LOG_MESSAGE_LIMIT = 2048
_LOG_SECRET_ASSIGNMENT = re.compile(
    r"""(?ix)
    (
        ["']?
        (?:access_token|refresh_token|client_secret|client_id|client_key|
           consumer_secret|consumer_key|shared_secret|api_key|session_key|
           oauth_token_secret|oauth_token|authorization|password|token)
        ["']?
        \s*[:=]\s*
    )
    (["']?)
    ([^\s,}\]"']+)
    \2
    """
)
_LOG_BEARER_VALUE = re.compile(
    r"(?i)\b(Bearer|Discogs\s+token=)\s*[A-Za-z0-9._~+/=-]+"
)

LOG_LEVELS = {
    "debug": 10,
    "info": 20,
    "warning": 30,
    "error": 40,
}


def _redact_log_message(message: Any) -> str:
    """Remove credential material before it reaches UI or persistent logs."""
    safe = str(message)
    safe = _LOG_BEARER_VALUE.sub(r"\1 [REDACTED]", safe)
    safe = _LOG_SECRET_ASSIGNMENT.sub(r"\1[REDACTED]", safe)
    return safe


def _bounded_log_message(message: Any) -> str:
    """Keep every runtime record single-line and reasonably sized."""
    safe = _redact_log_message(message).replace("\r", "\\r").replace("\n", "\\n")
    marker = " … [truncated]"
    if len(safe) <= RUNTIME_LOG_MESSAGE_LIMIT:
        return safe
    return safe[: RUNTIME_LOG_MESSAGE_LIMIT - len(marker)] + marker


def logging_verbosity(cfg: dict[str, Any]) -> str:
    verbosity = str(cfg.get("verbosity", "info")).strip().lower()
    if verbosity not in LOG_LEVELS:
        raise SplinedError(
            "SPLINED verbosity must be one of: debug, info, warning, error."
        )
    return verbosity


def runtime_log_path() -> Path | None:
    """Return the current run log path for diagnostics/presentation."""
    return _RUNTIME_LOG_PATH


def _close_runtime_log() -> None:
    global _RUNTIME_FAULTHANDLER_ENABLED, _RUNTIME_LOG_HANDLE, _RUNTIME_LOG_PENDING
    with _RUNTIME_LOG_LOCK:
        handle = _RUNTIME_LOG_HANDLE
        _RUNTIME_LOG_HANDLE = None
        _RUNTIME_LOG_PENDING = 0
        if handle is None:
            return
        try:
            if _RUNTIME_FAULTHANDLER_ENABLED and faulthandler.is_enabled():
                faulthandler.disable()
            _RUNTIME_FAULTHANDLER_ENABLED = False
            handle.flush()
            handle.close()
        except OSError:
            pass


def runtime_log(level: str, message: str) -> None:
    """Write one filtered line without reopening NAS-backed storage per event."""
    global _RUNTIME_LOG_PENDING, _RUNTIME_LOG_LAST_FLUSH
    handle = _RUNTIME_LOG_HANDLE
    if _RUNTIME_LOG_PATH is None or handle is None:
        return

    normalized = str(level).strip().lower()
    if normalized == "warn":
        normalized = "warning"
    if normalized not in LOG_LEVELS:
        normalized = "info"
    if LOG_LEVELS[normalized] < LOG_LEVELS[_RUNTIME_VERBOSITY]:
        return

    safe = _bounded_log_message(message)
    now = time.monotonic()
    try:
        with _RUNTIME_LOG_LOCK:
            current = _RUNTIME_LOG_HANDLE
            if current is None:
                return
            current.write(
                f"{time.strftime('%Y-%m-%d %H:%M:%S')} "
                f"{normalized.upper():<7} {safe}\n"
            )
            _RUNTIME_LOG_PENDING += 1
            if (
                normalized in {"warning", "error"}
                or _RUNTIME_LOG_PENDING >= 32
                or now - _RUNTIME_LOG_LAST_FLUSH >= 1.0
            ):
                current.flush()
                _RUNTIME_LOG_PENDING = 0
                _RUNTIME_LOG_LAST_FLUSH = now
    except OSError:
        # Runtime diagnostics must never make an operational scan fail.
        pass


def init_debug_log(config_file: Path, cfg: dict[str, Any]) -> Path:
    """Initialize the one-file-per-run diagnostic log.

    The historical function name is retained for compatibility with existing
    callers/tests. Runtime logs now live under <log_dir>/run and the prior run
    is removed at the beginning of the next invocation.
    """
    global _RUNTIME_FAULTHANDLER_ENABLED
    global _RUNTIME_LOG_PATH, _RUNTIME_LOG_HANDLE, _RUNTIME_LOG_PENDING
    global _RUNTIME_LOG_LAST_FLUSH, _RUNTIME_VERBOSITY, _DEBUG_ENABLED

    log_dir = runtime_log_dir(config_file, cfg)
    if log_dir.exists() and (log_dir.is_symlink() or not log_dir.is_dir()):
        raise SplinedError(f"Unsafe SPLINED log directory: {log_dir}")
    log_dir.mkdir(parents=True, exist_ok=True)

    retention_days = _non_negative_int(
        section(cfg, "logging").get("retention_days", 14),
        "[logging].retention_days",
    )
    if retention_days > 0:
        cutoff = time.time() - retention_days * 86_400
        for entry in log_dir.iterdir():
            try:
                if (
                    entry.is_file()
                    and not entry.is_symlink()
                    and entry.stat().st_mtime < cutoff
                ):
                    entry.unlink()
            except OSError:
                # Diagnostic retention is best-effort and must not stop scans.
                pass

    run_dir = log_dir / "run"
    if run_dir.exists() and (run_dir.is_symlink() or not run_dir.is_dir()):
        raise SplinedError(f"Unsafe SPLINED runtime log directory: {run_dir}")
    run_dir.mkdir(parents=True, exist_ok=True)

    # /run intentionally contains this invocation only. Never follow symlinks.
    for entry in run_dir.iterdir():
        try:
            if entry.is_symlink() or entry.is_file():
                entry.unlink()
            elif entry.is_dir():
                shutil.rmtree(entry)
        except OSError:
            # A stale diagnostic must not prevent SPLINED from starting.
            pass

    _RUNTIME_VERBOSITY = logging_verbosity(cfg)
    _DEBUG_ENABLED = _RUNTIME_VERBOSITY == "debug"

    stamp = time.strftime("%Y%m%d-%H%M%S")
    unique = f"{time.time_ns() % 1_000_000_000:09d}"
    _RUNTIME_LOG_PATH = (
        run_dir
        / f"splined-{_RUNTIME_VERBOSITY}-{stamp}-{unique}-p{os.getpid()}.log"
    )
    _close_runtime_log()
    _RUNTIME_LOG_HANDLE = _RUNTIME_LOG_PATH.open(
        "x",
        encoding="utf-8",
        buffering=64 * 1024,
    )
    _RUNTIME_LOG_HANDLE.write(
        f"=== SPLINED {display_version()} RUNTIME LOG ===\n"
        f"started={time.strftime('%Y-%m-%d %H:%M:%S')}\n"
        f"verbosity={_RUNTIME_VERBOSITY}\n"
        f"config={config_file}\n"
        f"pid={os.getpid()}\n"
        f"argv={_redact_log_message(json.dumps(sys.argv, ensure_ascii=False))}\n"
        "========================================\n"
    )
    _RUNTIME_LOG_HANDLE.flush()
    if bool(section(cfg, "logging").get("database_audit", False)):
        from splined_db_audit import configure as configure_db_audit

        configure_db_audit(
            log_dir,
            runtime_cache_dir(config_file, cfg) / "splined.db",
            shared=bool(section(cfg, "scan").get("sqlite_shared", False)),
            retention_days=retention_days,
        )
    # Native library failures bypass Python exception
    # handling. Record their thread stacks in the same per-run log once, only
    # on a fatal interpreter signal; routine debug output remains compact.
    try:
        faulthandler.enable(file=_RUNTIME_LOG_HANDLE, all_threads=True)
        _RUNTIME_FAULTHANDLER_ENABLED = True
    except (OSError, RuntimeError):
        _RUNTIME_FAULTHANDLER_ENABLED = False
        pass
    _RUNTIME_LOG_PENDING = 0
    _RUNTIME_LOG_LAST_FLUSH = time.monotonic()

    runtime_log(
        "info",
        f"run.start verbosity={_RUNTIME_VERBOSITY} "
        f"log={str(_RUNTIME_LOG_PATH)!r}",
    )
    return _RUNTIME_LOG_PATH


atexit.register(_close_runtime_log)


def debug_log(message: str) -> None:
    if not _DEBUG_ENABLED:
        return
    safe = _bounded_log_message(message)
    emit_ui("log", level="DEBUG", message=safe)
    runtime_log("debug", safe)



class Http:
    def __init__(self):
        self.s = requests.Session()
        self.s.headers.update({"User-Agent": USER_AGENT})
        self._owner_thread = threading.get_ident()
        self._thread_local = threading.local()
        self.last_mb_request: float | None = None

    def get(self, url: str, **kwargs: Any) -> requests.Response:
        kwargs.setdefault("timeout", REQUEST_TIMEOUT)
        if threading.get_ident() == self._owner_thread:
            session = self.s
        else:
            session = getattr(self._thread_local, "session", None)
            if session is None:
                session = requests.Session()
                session.headers.update({"User-Agent": USER_AGENT})
                self._thread_local.session = session
        return session.get(url, **kwargs)


def credential_file(config_file: Path, cfg: dict[str, Any], provider: str) -> Path:
    cdir = runtime_credential_dir(config_file, cfg)
    provider_key = provider.strip().lower()
    try:
        filename = FIXED_CREDENTIAL_FILES[provider_key]
    except KeyError as exc:
        raise SplinedError(f"Unsupported SPLINED credential provider: {provider}") from exc
    return cdir / filename


def load_json(path: Path, label: str) -> dict[str, Any]:
    try: data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise SplinedError(f"Unable to read {label} credential file {path}: {exc}") from exc
    if not isinstance(data, dict): raise SplinedError(f"Invalid {label} credential file {path}")
    return data


def authentication_statuses(
    config_file: Path,
    cfg: dict[str, Any],
) -> tuple[tuple[str, str], ...]:
    def credential(provider: str, label: str) -> dict[str, Any] | None:
        path = credential_file(config_file, cfg, provider)
        if not path.is_file():
            return None
        try:
            return load_json(path, label)
        except SplinedError:
            return {}

    discogs = credential("discogs", "Discogs")
    fanarttv = credential("fanarttv", "Fanart.tv")
    lastfm = credential("lastfm", "Last.fm")
    musicbrainz = credential("musicbrainz", "MusicBrainz")
    navidrome = credential("navidrome", "Navidrome")

    discogs_mode = (
        "Personal Token"
        if discogs and str(discogs.get("token") or "").strip()
        else "Not Configured"
    )
    fanarttv_mode = (
        "API Key v3.2"
        if fanarttv and str(fanarttv.get("api_key") or "").strip()
        else "Not Configured"
    )
    if lastfm and str(lastfm.get("api_key") or "").strip():
        lastfm_mode = (
            "API Key / Session"
            if str(lastfm.get("session_key") or "").strip()
            else "API Key"
        )
    else:
        lastfm_mode = "Not Configured"
    musicbrainz_mode = (
        "OAuth Bearer"
        if musicbrainz and str(musicbrainz.get("access_token") or "").strip()
        else "Anonymous"
    )
    navidrome_auth_method = (
        str(navidrome.get("auth_method") or "").strip().casefold()
        if navidrome
        else ""
    )
    navidrome_api_key = bool(
        navidrome
        and str(navidrome.get("api_key") or "").strip()
        and (
            navidrome_auth_method == "api_key"
            or (
                navidrome_auth_method != "password"
                and not (
                    str(navidrome.get("username") or "").strip()
                    and str(navidrome.get("password") or "")
                )
            )
        )
    )
    navidrome_password = bool(
        navidrome
        and str(navidrome.get("username") or "").strip()
        and str(navidrome.get("password") or "")
        and not navidrome_api_key
    )
    navidrome_complete = bool(
        navidrome
        and str(navidrome.get("server_url") or "").strip()
        and (navidrome_api_key or navidrome_password)
    )
    navidrome_mode = (
        "OpenSubsonic API Key"
        if navidrome_complete and navidrome_api_key
        else "OpenSubsonic Username / Password"
        if navidrome_complete
        else "Not Configured"
    )

    return (
        ("Discogs", discogs_mode),
        ("Fanart.tv", fanarttv_mode),
        ("Last.fm", lastfm_mode),
        ("MusicBrainz", musicbrainz_mode),
        ("Navidrome", navidrome_mode),
        ("iTunes", "Anonymous"),
        ("CoverArt", "Anonymous"),
        ("Amazon", "Anonymous / opt-in"),
    )



def save_json_atomic(path: Path, data: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    body = json.dumps(data, indent=2, ensure_ascii=False) + "\n"
    fd, temp_name = tempfile.mkstemp(prefix=f".{path.name}.", suffix=".tmp", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(body)
            handle.flush()
            os.fsync(handle.fileno())
        try:
            os.chmod(temp_name, 0o600)
        except OSError:
            # Some NAS/network-backed mounts do not implement POSIX chmod.
            # The credential write is still protected by the mounted storage's
            # own permissions/ACLs and remains atomic because the temp file is
            # created in the destination directory and replaced in-place.
            pass

        os.replace(temp_name, path)

        try:
            os.chmod(path, 0o600)
        except OSError:
            # Best-effort only for filesystems without POSIX chmod support.
            pass
    except Exception:
        try:
            os.unlink(temp_name)
        except OSError:
            pass
        raise


def credential_created_notice(path: Path) -> None:
    print()
    print(green("Credential file written successfully."))
    print(cyan(str(path)))
    try:
        mode = path.stat().st_mode & 0o777
        if mode & 0o077:
            print(yellow("Credential file created; user-specific filesystem ACL protection is unavailable on this storage location."))
    except OSError:
        pass




def load_navidrome_credentials(
    config_file: Path,
    cfg: dict[str, Any],
) -> dict[str, str]:
    """Load the cross-runtime Navidrome credential contract.

    Username/password and API-key authentication are mutually exclusive.
    Legacy bearer_token values are retained when an existing JSON document is
    read, but native Navidrome JWTs are not sent to OpenSubsonic endpoints.
    """
    path = credential_file(config_file, cfg, "navidrome")
    raw = load_json(path, "Navidrome")
    credential = {
        "server_url": str(raw.get("server_url") or "").strip().rstrip("/"),
        "auth_method": str(raw.get("auth_method") or "").strip().casefold(),
        "username": str(raw.get("username") or "").strip(),
        "password": str(raw.get("password") or ""),
        "api_key": str(raw.get("api_key") or "").strip(),
        "bearer_token": str(raw.get("bearer_token") or "").strip(),
    }
    parsed = urlsplit(credential["server_url"])
    if parsed.scheme.lower() not in {"http", "https"} or not parsed.netloc:
        raise SplinedError(
            "Navidrome server_url must be an absolute HTTP or HTTPS URL."
        )
    use_api_key = credential["auth_method"] == "api_key" or (
        credential["auth_method"] != "password"
        and bool(credential["api_key"])
        and not (credential["username"] and credential["password"])
    )
    credential["auth_method"] = "api_key" if use_api_key else "password"
    if use_api_key and not credential["api_key"]:
        raise SplinedError(
            "Navidrome api_key is required for the selected authentication method."
        )
    if not use_api_key and not (
        credential["username"] and credential["password"]
    ):
        raise SplinedError(
            "Navidrome username and password are required, or select API key authentication."
        )
    return credential




def _pkce_pair() -> tuple[str, str]:
    verifier = secrets.token_urlsafe(48)
    digest = hashlib.sha256(verifier.encode("ascii")).digest()
    challenge = base64.urlsafe_b64encode(digest).rstrip(b"=").decode("ascii")
    return verifier, challenge


def musicbrainz_settings(config_file: Path, cfg: dict[str, Any]) -> dict[str, Any]:
    policy = source_policy(cfg, "musicbrainz")
    path = credential_file(config_file, cfg, "musicbrainz")
    credential: dict[str, Any] = {}
    if path.exists():
        credential = load_json(path, "MusicBrainz")

    settings: dict[str, Any] = {
        "enabled": policy["enabled"],
        "source_override": (
            policy["source_override"] and not policy["strict_override"]
        ),
        "retry_max": 4,
        "mb_min_delay": 1.05,
        "mb_recording_timeout": 7.0,
        "oauth": bool(
            credential.get("oauth_enabled", False)
            or str(credential.get("access_token") or "").strip()
        ),
        "client_id": str(credential.get("client_id") or "").strip(),
        "callback_uri": str(
            credential.get("callback_uri") or "urn:ietf:wg:oauth:2.0:oob"
        ).strip(),
        "scope": str(credential.get("oauth_scope") or "profile").strip(),
        "credential": credential,
        "credential_path": path,
    }

    if policy["source_override"] and not policy["strict_override"]:
        options = credential.get("options", {})
        if not isinstance(options, dict):
            raise SplinedError("MusicBrainz credential options must be a JSON object.")
        retry_max = options.get("retry_max", 4)
        min_delay = options.get("min_delay", 1.05)
        recording_timeout = options.get("recording_timeout", 7)
        if isinstance(retry_max, bool):
            raise SplinedError("MusicBrainz retry_max must be between 0 and 20.")
        try:
            retry_max = int(retry_max)
            min_delay = float(min_delay)
            recording_timeout = float(recording_timeout)
        except (TypeError, ValueError) as exc:
            raise SplinedError("MusicBrainz runtime options contain invalid numeric values.") from exc
        if not 0 <= retry_max <= 20:
            raise SplinedError("MusicBrainz retry_max must be between 0 and 20.")
        if not min_delay > 0:
            raise SplinedError("MusicBrainz min_delay must be greater than 0.")
        if not recording_timeout > 0:
            raise SplinedError("MusicBrainz recording_timeout must be greater than 0.")
        settings["retry_max"] = retry_max
        settings["mb_min_delay"] = min_delay
        settings["mb_recording_timeout"] = recording_timeout

    return settings


def _mb_refresh_token(config_file: Path, cfg: dict[str, Any], cred: dict[str, Any]) -> dict[str, Any]:
    client_id = str(cred.get("client_id") or "").strip()
    client_secret = str(cred.get("client_secret") or "").strip()
    refresh_token = str(cred.get("refresh_token") or "").strip()
    missing = [
        name
        for name, value in (
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", refresh_token),
        )
        if not value
    ]
    if missing:
        raise SplinedError(
            "MusicBrainz OAuth automatic refresh cannot continue because "
            f"musicbrainz.json is missing {', '.join(missing)}. "
            "Run Settings > Provider Credentials > MusicBrainz > Authorize, then Settings > Provider Credentials > Test Connection."
        )

    try:
        response = requests.post(
            MB_OAUTH_ENDPOINT,
            data={
                "grant_type": "refresh_token",
                "refresh_token": refresh_token,
                "client_id": client_id,
                "client_secret": client_secret,
            },
            headers={"User-Agent": USER_AGENT},
            timeout=REQUEST_TIMEOUT,
        )
    except requests.RequestException as exc:
        raise SplinedError(
            "MusicBrainz OAuth refresh request failed. Check network connectivity, "
            "then run Settings > Provider Credentials > Test Connection again."
        ) from exc
    if not response.ok:
        raise SplinedError(
            f"MusicBrainz OAuth refresh was rejected with HTTP {response.status_code}. "
            "Run Settings > Provider Credentials > MusicBrainz > Authorize, then Settings > Provider Credentials > Test Connection."
        )
    try:
        token = response.json()
    except ValueError as exc:
        raise SplinedError(
            "MusicBrainz OAuth refresh returned invalid JSON. "
            "Run Settings > Provider Credentials > Test Connection again; reauthorize if the failure continues."
        ) from exc

    access_token = str(token.get("access_token") or "").strip()
    if not access_token:
        raise SplinedError(
            "MusicBrainz OAuth refresh returned no access_token. "
            "Run Settings > Provider Credentials > MusicBrainz > Authorize, then Settings > Provider Credentials > Test Connection."
        )
    try:
        expires_in = int(token.get("expires_in") or 0)
    except (TypeError, ValueError) as exc:
        raise SplinedError(
            "MusicBrainz OAuth refresh returned an invalid expires_in value."
        ) from exc
    if expires_in <= 0:
        raise SplinedError(
            "MusicBrainz OAuth refresh returned no usable token lifetime. "
            "Run Settings > Provider Credentials > MusicBrainz > Authorize, then Settings > Provider Credentials > Test Connection."
        )

    now = int(time.time())
    updated = dict(cred)
    updated["access_token"] = access_token
    updated["refresh_token"] = str(token.get("refresh_token") or refresh_token).strip() or None
    updated["token_type"] = str(token.get("token_type") or "Bearer").strip() or "Bearer"
    updated["scope"] = str(token.get("scope") or updated.get("scope") or "").strip() or None
    updated["expires_at_unix"] = now + expires_in
    path = credential_file(config_file, cfg, "musicbrainz")
    save_json_atomic(path, updated)
    return updated


def mb_headers(
    config_file: Path,
    cfg: dict[str, Any],
    *,
    force_refresh: bool = False,
) -> tuple[dict[str, str], str]:
    mb = musicbrainz_settings(config_file, cfg)
    if not bool(mb.get("enabled", True)):
        return {}, "Disabled"
    if not bool(mb.get("oauth", False)):
        return {}, "Anonymous"

    path = credential_file(config_file, cfg, "musicbrainz")
    cred = load_json(path, "MusicBrainz")
    token = str(cred.get("access_token") or "").strip()
    if force_refresh:
        cred = _mb_refresh_token(config_file, cfg, cred)
        token = str(cred.get("access_token") or "").strip()
    else:
        expires_at = cred.get("expires_at_unix")
        expired = False
        if expires_at is not None:
            try:
                expired = int(expires_at) <= int(time.time()) + 30
            except (TypeError, ValueError) as exc:
                raise SplinedError(
                    "MusicBrainz expires_at_unix is invalid. "
                    "Run Settings > Provider Credentials > MusicBrainz > Authorize, then Settings > Provider Credentials > Test Connection."
                ) from exc
        if expired:
            cred = _mb_refresh_token(config_file, cfg, cred)
            token = str(cred.get("access_token") or "").strip()

    if not token and str(cred.get("refresh_token") or "").strip():
        cred = _mb_refresh_token(config_file, cfg, cred)
        token = str(cred.get("access_token") or "").strip()
    if not token:
        try:
            display_path = path.resolve()
        except OSError:
            display_path = path
        raise SplinedError(
            f"MusicBrainz OAuth enabled but no usable access_token exists in {display_path}. "
            "Run Settings > Provider Credentials > MusicBrainz > Authorize, then Settings > Provider Credentials > Test Connection."
        )
    return {"Authorization": f"Bearer {token}"}, "OAuthBearer"






def lastfm_signature(params: dict[str, str], secret: str) -> str:
    material = "".join(k + params[k] for k in sorted(params)) + secret
    # Last.fm's public API requires this exact MD5 signing algorithm. It is
    # protocol compatibility, not password hashing or a security primitive.
    return hashlib.md5(material.encode("utf-8"), usedforsecurity=False).hexdigest()







def lookup_release(http: Http, config_file: Path, cfg: dict[str, Any], mbid: str) -> Release:
    lookup_started = time.perf_counter()
    headers, _ = mb_headers(config_file, cfg)
    mbcfg = musicbrainz_settings(config_file, cfg)
    if not mbcfg["enabled"]:
        raise SplinedError("MusicBrainz is disabled by [source_policies.musicbrainz].")
    timeout = float(mbcfg.get("mb_recording_timeout", 7.0))
    retry_max = int(mbcfg.get("retry_max", 2))
    min_delay = max(0.0, float(mbcfg.get("mb_min_delay", 1.05)))
    url = f"{MB_BASE}/release/{mbid}"

    emit_ui(
        "activity",
        category="authority",
        state="start",
        source="musicbrainz",
        message=f"MusicBrainz authority lookup started · {mbid}",
    )

    for attempt in range(retry_max + 1):
        if http.last_mb_request is not None:
            wait = min_delay - (time.monotonic() - http.last_mb_request)
            if wait > 0:
                time.sleep(wait)

        attempt_started = time.perf_counter()
        try:
            http.last_mb_request = time.monotonic()
            r = http.get(
                url,
                params={"inc": "artist-credits+release-groups+media+url-rels", "fmt": "json"},
                headers=headers,
                timeout=timeout,
            )
        except requests.RequestException as exc:
            if attempt < retry_max:
                emit_ui(
                    "activity",
                    category="authority",
                    state="retry",
                    source="musicbrainz",
                    message=f"MusicBrainz retry {attempt + 2}/{retry_max + 1}",
                )
                continue
            raise SplinedError(
                f"Unable to query MusicBrainz release {mbid} after {attempt + 1} attempt(s): {exc}"
            ) from exc

        if r.status_code == 404:
            raise SplinedError(f"MusicBrainz release not found: {mbid}")
        if r.status_code == 401:
            raise SplinedError("MusicBrainz OAuth authentication was rejected.")
        if (r.status_code == 429 or 500 <= r.status_code <= 599) and attempt < retry_max:
            continue
        if r.status_code != 200:
            raise SplinedError(
                f"MusicBrainz release {mbid} returned HTTP {r.status_code} after {attempt + 1} attempt(s)"
            )

        try:
            d = r.json()
        except ValueError as exc:
            raise SplinedError(f"Invalid MusicBrainz JSON for release {mbid}: {exc}") from exc

        parts: list[str] = []
        for item in d.get("artist-credit", []):
            if isinstance(item, str):
                parts.append(item)
            elif isinstance(item, dict):
                parts.append(str(item.get("name") or (item.get("artist") or {}).get("name") or ""))
                parts.append(str(item.get("joinphrase") or ""))

        media = d.get("media") or []
        track_count = 0
        found_count = False
        if isinstance(media, list):
            for m in media:
                if not isinstance(m, dict):
                    continue
                value = m.get("track-count")
                if value is None:
                    tracks = m.get("tracks")
                    if isinstance(tracks, list):
                        track_count += len(tracks)
                        found_count = True
                    continue
                try:
                    track_count += int(value)
                    found_count = True
                except (TypeError, ValueError):
                    continue

        rg = d.get("release-group") or {}
        external_urls: list[str] = []
        for relation in d.get("relations", []) or []:
            if not isinstance(relation, dict):
                continue
            url_data = relation.get("url")
            if not isinstance(url_data, dict):
                continue
            resource = str(url_data.get("resource") or "").strip()
            if resource and resource not in external_urls:
                external_urls.append(resource)

        release = Release(
            mbid=mbid,
            title=str(d.get("title") or "").strip(),
            artist_credit="".join(parts).strip(),
            release_group_id=str(rg.get("id") or "").strip() or None,
            release_group_title=str(rg.get("title") or "").strip() or None,
            track_count=track_count if found_count else None,
            external_urls=external_urls,
            asin=str(d.get("asin") or "").strip().upper() or None,
        )
        attempt_elapsed = time.perf_counter() - attempt_started
        total_elapsed = time.perf_counter() - lookup_started
        debug_log(
            "musicbrainz.lookup.done "
            f"mbid={mbid} attempt={attempt + 1}/{retry_max + 1} "
            f"attempt_elapsed={attempt_elapsed:.3f}s "
            f"total_elapsed={total_elapsed:.3f}s"
        )
        emit_ui(
            "activity",
            category="authority",
            state="done",
            source="musicbrainz",
            message=(
                f"Authority resolved in {total_elapsed:.2f}s · "
                f"{release.artist_credit} · {release.title}"
            ),
        )
        return release

    raise SplinedError(f"MusicBrainz release lookup failed: {mbid}")


def norm_provider(v: str) -> str: return "".join(c.lower() for c in v if c.isalnum())
def same_text(a: str, b: str) -> bool: return norm_provider(a) == norm_provider(b)
def title_match(a: str, b: str) -> bool:
    a, b = norm_provider(a), norm_provider(b); return bool(a and b and (a == b or a.startswith(b) or b.startswith(a)))


def discover_deezer(http: Http, rel: Release) -> list[Ref]:
    titles = ([rel.release_group_title] if rel.release_group_title else [])
    if not any(same_text(t, rel.title) for t in titles): titles.append(rel.title)
    refs: list[Ref] = []; seen: set[str] = set()
    for album in titles:
        for q in (f'artist:"{rel.artist_credit}" album:"{album}"', f"{rel.artist_credit} {album}", album):
            r = http.get("https://api.deezer.com/search/album", params={"q": q, "limit": 25})
            if r.status_code != 200: raise SplinedError(f"Deezer returned HTTP {r.status_code}")
            for item in r.json().get("data", []):
                iid = str(item.get("id") or "")
                if not iid or iid in seen: continue
                if not same_text(str((item.get("artist") or {}).get("name") or ""), rel.artist_credit): continue
                if not title_match(str(item.get("title") or ""), album): continue
                seen.add(iid); refs.append(Ref("deezer", iid, f"https://api.deezer.com/album/{iid}/image?size=xl"))
            if refs: break
    return refs


def itunes_artist_match(result_artist: str, authority_artist: str) -> bool:
    if same_text(result_artist, authority_artist):
        return True

    # Apple can append or omit secondary credits. Permit only whole normalized
    # prefix expansion, not arbitrary substring matches (e.g. Smith/Aerosmith).
    result_words = normalize_text(result_artist)
    authority_words = normalize_text(authority_artist)
    return bool(
        result_words
        and authority_words
        and (
            result_words.startswith(authority_words + " ")
            or authority_words.startswith(result_words + " ")
        )
    )


def itunes_artwork_url(url: str, size: int = 3000) -> str:
    return re.sub(
        r"/\d+x\d+bb(?=[./])",
        f"/{size}x{size}bb",
        url,
        count=1,
        flags=re.IGNORECASE,
    )


def apple_album_id_from_url(url: str) -> str | None:
    value = str(url or "").strip()
    if not value:
        return None

    # Apple/MusicBrainz URLs appear in several valid forms:
    #   https://music.apple.com/us/album/1658654996
    #   https://music.apple.com/us/album/just-push-play/1658654996
    #   https://itunes.apple.com/us/album/just-push-play/id571803416
    #
    # MusicBrainz commonly stores the canonical no-slug form. The previous
    # parser required a slug between /album/ and the numeric ID, which caused
    # current Apple relationships such as /album/1658654996 to be ignored.
    match = re.search(
        r"https?://(?:music|itunes)\.apple\.com/"
        r"[^?#]*?/album/"
        r"(?:[^/?#]+/)?"
        r"(?:id)?(\d+)"
        r"(?:[/?#]|$)",
        value,
        flags=re.IGNORECASE,
    )
    if match:
        return match.group(1)

    # Older generic iTunes forms can contain /id123456789 outside /album/.
    match = re.search(
        r"https?://(?:music|itunes)\.apple\.com/[^?#]*?/id(\d+)(?:[/?#]|$)",
        value,
        flags=re.IGNORECASE,
    )
    return match.group(1) if match else None


def _mb_release_group_apple_ids(
    http: Http,
    config_file: Path,
    cfg: dict[str, Any],
    rel: Release,
) -> list[str]:
    if not rel.release_group_id:
        return []

    headers, _ = mb_headers(config_file, cfg)
    mbcfg = musicbrainz_settings(config_file, cfg)
    if not mbcfg["enabled"]:
        return []
    timeout = float(mbcfg.get("mb_recording_timeout", 7.0))
    retry_max = int(mbcfg.get("retry_max", 2))
    min_delay = max(0.0, float(mbcfg.get("mb_min_delay", 1.05)))

    url = f"{MB_BASE}/release"
    for attempt in range(retry_max + 1):
        if http.last_mb_request is not None:
            wait = min_delay - (time.monotonic() - http.last_mb_request)
            if wait > 0:
                time.sleep(wait)

        try:
            http.last_mb_request = time.monotonic()
            response = http.get(
                url,
                params={
                    "release-group": rel.release_group_id,
                    "inc": "url-rels",
                    "limit": 100,
                    "fmt": "json",
                },
                headers=headers,
                timeout=timeout,
            )
        except requests.RequestException as exc:
            if attempt < retry_max:
                continue
            raise SplinedError(
                f"MusicBrainz release-group URL lookup failed after {attempt + 1} attempt(s): {exc}"
            ) from exc

        if response.status_code == 401:
            raise SplinedError("MusicBrainz OAuth authentication was rejected.")
        if (response.status_code == 429 or 500 <= response.status_code <= 599) and attempt < retry_max:
            continue
        if response.status_code != 200:
            raise SplinedError(
                f"MusicBrainz release-group URL lookup returned HTTP {response.status_code}"
            )

        ids: list[str] = []
        data = response.json()
        releases = data.get("releases", []) if isinstance(data, dict) else []
        for sibling in releases if isinstance(releases, list) else []:
            if not isinstance(sibling, dict):
                continue
            for relation in sibling.get("relations", []) or []:
                if not isinstance(relation, dict):
                    continue
                url_data = relation.get("url")
                if not isinstance(url_data, dict):
                    continue
                apple_id = apple_album_id_from_url(str(url_data.get("resource") or ""))
                if apple_id and apple_id not in ids:
                    ids.append(apple_id)
        return ids

    return []


def _itunes_refs_from_ids(
    http: Http,
    rel: Release,
    ids: list[str],
) -> list[Ref]:
    refs: list[Ref] = []
    seen: set[str] = set()

    for apple_id in ids:
        if apple_id in seen:
            continue
        seen.add(apple_id)

        response = http.get(
            "https://itunes.apple.com/lookup",
            params={"id": apple_id, "country": "US"},
        )
        if response.status_code != 200:
            continue

        payload = response.json()
        results = payload.get("results", []) if isinstance(payload, dict) else []
        for item in results if isinstance(results, list) else []:
            if not isinstance(item, dict):
                continue

            collection_id = str(item.get("collectionId") or item.get("trackId") or "").strip()
            artist = str(item.get("artistName") or "").strip()
            title = str(item.get("collectionName") or item.get("trackName") or "").strip()
            artwork = str(
                item.get("artworkUrl100")
                or item.get("artworkUrl60")
                or item.get("artworkUrl30")
                or ""
            ).strip()

            if not collection_id or not artwork:
                continue
            if not itunes_artist_match(artist, rel.artist_credit):
                continue
            if not title_match(title, rel.release_group_title or rel.title):
                continue

            refs.append(
                Ref(
                    "itunes",
                    collection_id,
                    itunes_artwork_url(artwork, 3000),
                )
            )
            break

    return refs


def discover_itunes(
    http: Http,
    config_file: Path,
    cfg: dict[str, Any],
    rel: Release,
) -> list[Ref]:
    # First use any Apple relationship already attached to the exact MB release.
    exact_ids: list[str] = []
    for url in rel.external_urls:
        apple_id = apple_album_id_from_url(url)
        if apple_id and apple_id not in exact_ids:
            exact_ids.append(apple_id)
    if exact_ids:
        debug_log(f"itunes.mb_exact_ids ids={','.join(exact_ids)}")
        refs = _itunes_refs_from_ids(http, rel, exact_ids)
        if refs:
            return refs

    titles = ([rel.release_group_title] if rel.release_group_title else [])
    if not any(same_text(t, rel.title) for t in titles):
        titles.append(rel.title)

    refs: list[Ref] = []
    seen: set[str] = set()

    # Keep the keyless iTunes Search API as the normal path.
    for album in titles:
        searches = [
            {"term": f"{rel.artist_credit} {album}"},
            {"term": album, "attribute": "albumTerm"},
            {"term": rel.artist_credit, "attribute": "artistTerm"},
        ]

        for search in searches:
            params = {
                **search,
                "country": "US",
                "media": "music",
                "entity": "album",
                "limit": 50,
            }
            response = http.get("https://itunes.apple.com/search", params=params)
            if response.status_code != 200:
                raise SplinedError(f"iTunes returned HTTP {response.status_code}")

            payload = response.json()
            results = payload.get("results", [])
            if not isinstance(results, list):
                continue

            for item in results:
                if not isinstance(item, dict):
                    continue

                iid = str(item.get("collectionId") or "").strip()
                art = str(item.get("artworkUrl100") or "").strip()
                result_artist = str(item.get("artistName") or "").strip()
                result_title = str(item.get("collectionName") or "").strip()

                if not iid or iid in seen or not art:
                    continue
                if not itunes_artist_match(result_artist, rel.artist_credit):
                    continue
                if not title_match(result_title, album):
                    continue

                seen.add(iid)
                refs.append(Ref("itunes", iid, itunes_artwork_url(art, 3000)))

            if refs:
                return refs

    # Search can miss catalog albums that are nevertheless linked from
    # MusicBrainz. Browse sibling releases in the exact release group and use
    # their Apple Music/iTunes relationships as direct collection IDs.
    sibling_ids = _mb_release_group_apple_ids(http, config_file, cfg, rel)
    debug_log(
        f"itunes.mb_sibling_ids release_group={rel.release_group_id or ''} "
        f"count={len(sibling_ids)} "
        f"ids={','.join(sibling_ids) if sibling_ids else 'none'}"
    )
    if sibling_ids:
        refs = _itunes_refs_from_ids(http, rel, sibling_ids)
        if refs:
            return refs

    return []



def discover_caa(http: Http, rel: Release) -> list[Ref]:
    r = http.get(f"https://coverartarchive.org/release/{rel.mbid}/", headers={"Accept": "application/json"})
    if r.status_code == 404: return []
    if r.status_code != 200: raise SplinedError(f"Cover Art Archive returned HTTP {r.status_code}")
    return [Ref("coverartarchive", str(x.get("id") or ""), str(x.get("image") or ""), bool(x.get("front", False)), bool(x.get("approved", False)), [str(v) for v in x.get("types", [])]) for x in r.json().get("images", [])]


def discover_musicbrainz_artwork(http: Http, rel: Release) -> list[Ref]:
    """Return MusicBrainz-selected Cover Art Archive artwork.

    MusicBrainz is the release authority; Cover Art Archive hosts the bytes.
    Prefer the release-group representative so this priority entry remains
    distinct from the exact-release ``coverartarchive`` provider.
    """
    if rel.release_group_id:
        entity = "release-group"
        identity = rel.release_group_id
    elif rel.mbid:
        entity = "release"
        identity = rel.mbid
    else:
        return []
    response = http.get(
        f"https://coverartarchive.org/{entity}/{identity}/",
        headers={"Accept": "application/json"},
    )
    if response.status_code == 404:
        return []
    if response.status_code != 200:
        raise SplinedError(
            f"MusicBrainz / Cover Art Archive returned HTTP {response.status_code}"
        )
    payload = response.json()
    images = payload.get("images", []) if isinstance(payload, dict) else []
    return [
        Ref(
            "musicbrainz",
            str(item.get("id") or ""),
            str(item.get("image") or ""),
            bool(item.get("front", False)),
            bool(item.get("approved", False)),
            [str(value) for value in item.get("types", [])],
        )
        for item in images
        if isinstance(item, dict) and str(item.get("image") or "").strip()
    ]


def amazon_original_image_url(value: str) -> str | None:
    """Normalize an Amazon primary-image URL to its untransformed asset."""
    raw = html_lib.unescape(str(value or "").strip()).replace("\\/", "/")
    if not raw:
        return None
    parsed = urlsplit(raw)
    if parsed.scheme != "https" or parsed.hostname != "m.media-amazon.com":
        return None
    match = re.fullmatch(
        r"/images/I/(?P<asset>[^./?#]+?)(?:\._[^/?#]+_)?\."
        r"(?P<extension>jpe?g|png)",
        parsed.path,
        flags=re.IGNORECASE,
    )
    if match is None:
        return None
    extension = match.group("extension").lower()
    return (
        "https://m.media-amazon.com/images/I/"
        f"{match.group('asset')}.{extension}"
    )


def _plain_html_text(value: str) -> str:
    return re.sub(
        r"\s+",
        " ",
        html_lib.unescape(re.sub(r"<[^>]+>", " ", value)),
    ).strip()


def discover_amazon(http: Http, rel: Release) -> list[Ref]:
    """Search Amazon Store HTML and expose original primary product images.

    This intentionally follows the operator-visible Store workflow rather
    than the Amazon Music player or its small artwork. Markup changes and
    automated-request blocking are surfaced as ordinary provider diagnostics.
    """
    linked_asin = str(rel.asin or "").strip().upper()
    if not linked_asin:
        for resource in rel.external_urls:
            match = re.search(
                r"(?i)amazon\.[^/]+/(?:dp|gp/product)/([A-Z0-9]{10})(?:[/?]|$)",
                resource,
            )
            if match is not None:
                linked_asin = match.group(1).upper()
                break
    query = linked_asin or " ".join(
        value
        for value in (rel.artist_credit, rel.release_group_title or rel.title)
        if str(value or "").strip()
    )
    if not query:
        return []
    debug_log(
        f"amazon.discovery authority={'musicbrainz-asin' if linked_asin else 'artist-title'} "
        f"asin={linked_asin or 'none'}"
    )
    response = http.get(
        "https://www.amazon.com/s",
        params={"k": query, "i": "popular"},
        headers={
            "Accept": "text/html,application/xhtml+xml",
            "Accept-Language": "en-US,en;q=0.8",
        },
    )
    if response.status_code != 200:
        raise SplinedError(f"Amazon Store search returned HTTP {response.status_code}")
    body = str(response.text or "")
    if "validateCaptcha" in body or "api-services-support@amazon.com" in body:
        raise SplinedError("Amazon Store blocked the public search request")

    block_pattern = re.compile(
        r"(?is)<div\b[^>]*data-component-type=[\"']s-search-result[\"']"
        r"[^>]*>.*?(?=<div\b[^>]*data-component-type=[\"']s-search-result[\"']|\Z)"
    )
    image_pattern = re.compile(
        r"(?is)<img\b[^>]*class=[\"'][^\"']*\bs-image\b[^\"']*[\"'][^>]*>"
    )
    src_pattern = re.compile(r"(?is)\bsrc=[\"']([^\"']+)[\"']")
    title_pattern = re.compile(r"(?is)<h2\b[^>]*>(.*?)</h2>")
    asin_pattern = re.compile(r"(?is)\bdata-asin=[\"']([A-Z0-9]{10})[\"']")
    expected_title = rel.release_group_title or rel.title
    refs: list[Ref] = []
    seen: set[str] = set()
    for block in block_pattern.findall(body):
        image_tag = image_pattern.search(block)
        source_match = src_pattern.search(image_tag.group(0)) if image_tag else None
        original = amazon_original_image_url(
            source_match.group(1) if source_match else ""
        )
        if not original or original in seen:
            continue
        title_match_value = title_pattern.search(block)
        product_title = _plain_html_text(
            title_match_value.group(1) if title_match_value else ""
        )
        if product_title and expected_title and not title_match(
            product_title, expected_title
        ):
            continue
        asin_match = asin_pattern.search(block)
        asin = asin_match.group(1) if asin_match else ""
        if linked_asin and asin.upper() != linked_asin:
            continue
        seen.add(original)
        refs.append(
            Ref(
                "amazon",
                " · ".join(value for value in (asin, product_title) if value),
                original,
                front=True,
                approved=True,
                types=["Front"],
            )
        )
        if len(refs) >= 6:
            break
    return refs

def discover_fanart(http: Http, config_file: Path, cfg: dict[str, Any], rel: Release) -> list[Ref]:
    if not rel.release_group_id: return []
    path = credential_file(config_file, cfg, "fanarttv"); cred = load_json(path, "Fanart.tv"); key = str(cred.get("api_key") or "").strip()
    if not key: raise SplinedError(f"Fanart.tv credential file contains no api_key: {path}")
    headers = {"api-key": key}; client = str(cred.get("client_key") or "").strip()
    if client: headers["client-key"] = client
    r = http.get(f"{FANARTTV_API_BASE}/music/albums/{rel.release_group_id}", headers=headers)
    if r.status_code == 404: return []
    if r.status_code in {401, 403}: raise SplinedError("Fanart.tv credentials were rejected.")
    if r.status_code != 200: raise SplinedError(f"Fanart.tv returned HTTP {r.status_code}")
    data = r.json()
    albums = data.get("albums") if isinstance(data, dict) else None
    if not isinstance(albums, list):
        raise SplinedError("Fanart.tv v3.2 response did not contain an albums array")
    covers: list[dict[str, Any]] = []
    for album in albums:
        if not isinstance(album, dict):
            continue
        release_group_id = str(album.get("release_group_id") or "")
        if release_group_id.casefold() != rel.release_group_id.casefold():
            continue
        raw_covers = album.get("albumcover")
        if isinstance(raw_covers, list):
            covers.extend(item for item in raw_covers if isinstance(item, dict))
    return [Ref("fanarttv", str(x.get("id") or ""), str(x.get("url") or "")) for x in covers if str(x.get("url") or "").strip()]


def lastfm_refs(album: dict[str, Any]) -> list[Ref]:
    seen: set[str] = set(); refs: list[Ref] = []; artist = str(album.get("artist") or ""); name = str(album.get("name") or "")
    for x in album.get("image", []):
        url = str(x.get("#text") or "").strip()
        if not url or url in seen: continue
        seen.add(url); refs.append(Ref("lastfm", f"{artist}:{name}:{x.get('size','')}", url))
    return refs


def discover_lastfm(http: Http, config_file: Path, cfg: dict[str, Any], rel: Release) -> list[Ref]:
    path = credential_file(config_file, cfg, "lastfm"); key = str(load_json(path, "Last.fm").get("api_key") or "").strip()
    if not key: raise SplinedError(f"Last.fm credential file contains no api_key: {path}")
    def request(params: dict[str, str]) -> dict[str, Any] | None:
        q = {"method": "album.getinfo", "api_key": key, "format": "json", "autocorrect": "1", **params}; r = http.get("https://ws.audioscrobbler.com/2.0/", params=q); d = r.json()
        if d.get("error") in {6, 7}: return None
        if d.get("error") == 10 or r.status_code in {401, 403}: raise SplinedError("Last.fm API credentials were rejected.")
        if d.get("error"): raise SplinedError(f"Last.fm API error {d.get('error')}: {d.get('message','unknown error')}")
        return d.get("album") if isinstance(d.get("album"), dict) else None
    album = request({"mbid": rel.mbid})
    if album:
        refs = lastfm_refs(album)
        if refs: return refs
    titles = ([rel.release_group_title] if rel.release_group_title else [])
    if not any(same_text(t, rel.title) for t in titles): titles.append(rel.title)
    for title in titles:
        album = request({"artist": rel.artist_credit, "album": title})
        if album and same_text(str(album.get("artist") or ""), rel.artist_credit) and title_match(str(album.get("name") or ""), title):
            refs = lastfm_refs(album)
            if refs: return refs
    return []


def discover_discogs(
    http: Http,
    config_file: Path,
    cfg: dict[str, Any],
    artist: str,
    album: str,
) -> list[Ref]:
    path = credential_file(config_file, cfg, "discogs")
    cred = load_json(path, "Discogs")
    token = str(cred.get("token") or "").strip()
    if not token:
        raise SplinedError(f"Discogs credential file contains no token: {path}")

    headers = {"Authorization": f"Discogs token={token}"}
    response = http.get(
        "https://api.discogs.com/database/search",
        params={
            "artist": artist,
            "release_title": album,
            "type": "release",
            "per_page": 10,
        },
        headers=headers,
    )
    if response.status_code in {401, 403}:
        raise SplinedError("Discogs personal access token was rejected.")
    if response.status_code != 200:
        raise SplinedError(f"Discogs search returned HTTP {response.status_code}")

    results = response.json().get("results", [])
    refs: list[Ref] = []
    seen_release: set[str] = set()

    # Inspect several editions. The full release endpoint exposes the original
    # image dimensions/URIs, unlike the search thumbnail.
    for result in results[:6]:
        release_id = str(result.get("id") or "").strip()
        if not release_id or release_id in seen_release:
            continue
        seen_release.add(release_id)

        resource_url = str(result.get("resource_url") or "").strip()
        if not resource_url:
            resource_url = f"https://api.discogs.com/releases/{release_id}"

        release_response = http.get(resource_url, headers=headers)
        if release_response.status_code != 200:
            continue
        release_data = release_response.json()
        images = release_data.get("images") or []

        primary = [
            image for image in images
            if isinstance(image, dict) and str(image.get("type") or "").lower() == "primary"
        ]
        all_images = [image for image in images if isinstance(image, dict)]
        discogs_policy = source_policy(cfg, "discogs")
        primary_only = (
            not discogs_policy["source_override"]
            or discogs_policy["strict_override"]
            or discogs_policy["primary_image_only"]
        )
        image_pool = (primary or all_images) if primary_only else all_images

        emitted = False
        for image_index, image in enumerate(image_pool[:2], 1):
            uri = str(image.get("uri") or image.get("resource_url") or "").strip()
            if not uri:
                continue
            image_id = str(image.get("id") or f"{release_id}:{image_index}")
            refs.append(
                Ref(
                    "discogs",
                    f"{release_id}:{image_id}",
                    uri,
                    front=(
                        str(image.get("type") or "").lower() == "primary"
                        if primary
                        else True
                    ),
                    approved=True,
                    types=["Front"],
                    width=(
                        int(image.get("width"))
                        if str(image.get("width") or "").isdigit()
                        else None
                    ),
                    height=(
                        int(image.get("height"))
                        if str(image.get("height") or "").isdigit()
                        else None
                    ),
                )
            )
            emitted = True

        # Search thumbnail is a last-resort candidate if release.images[] is empty.
        if not emitted:
            cover = str(result.get("cover_image") or "").strip()
            if cover:
                refs.append(Ref("discogs", release_id, cover, front=True, approved=True, types=["Front"]))

    return refs


def _register_navidrome_session(credential: dict[str, str]) -> int:
    global _NAVIDROME_NEXT_SESSION
    with _NAVIDROME_SESSION_LOCK:
        session_id = _NAVIDROME_NEXT_SESSION
        _NAVIDROME_NEXT_SESSION += 1
        _NAVIDROME_SESSIONS[session_id] = dict(credential)
    return session_id


def _navidrome_internal_cover_url(session_id: int, cover_id: str) -> str:
    return (
        f"{NAVIDROME_INTERNAL_SCHEME}{session_id}/"
        + cover_id.encode("utf-8").hex()
    )


def _navidrome_internal_cover(
    value: str,
) -> tuple[dict[str, str], str]:
    if not value.startswith(NAVIDROME_INTERNAL_SCHEME):
        raise SplinedError("Invalid internal Navidrome artwork reference.")
    body = value[len(NAVIDROME_INTERNAL_SCHEME) :]
    try:
        session_text, cover_hex = body.split("/", 1)
        session_id = int(session_text)
        cover_id = bytes.fromhex(cover_hex).decode("utf-8")
    except (ValueError, UnicodeDecodeError) as exc:
        raise SplinedError(
            "Invalid internal Navidrome artwork reference."
        ) from exc
    with _NAVIDROME_SESSION_LOCK:
        credential = _NAVIDROME_SESSIONS.get(session_id)
        if credential is not None:
            credential = dict(credential)
    if credential is None:
        raise SplinedError("Navidrome artwork credential session has expired.")
    if not cover_id:
        raise SplinedError("Invalid internal Navidrome artwork identity.")
    return credential, cover_id


def _navidrome_auth(
    credential: dict[str, str],
) -> tuple[dict[str, str], dict[str, str]]:
    params = {
        "v": NAVIDROME_PROTOCOL_VERSION,
        "c": NAVIDROME_CLIENT_NAME,
        "f": "json",
    }
    if credential.get("auth_method") == "api_key":
        # OpenSubsonic API-key authentication must not also send `u`.
        params["apiKey"] = credential["api_key"]
    else:
        salt = secrets.token_hex(6)
        # MD5 is required by the compatible Subsonic challenge protocol; it is
        # not used as a password-storage primitive.
        params.update(
            {
                "u": credential["username"],
                "s": salt,
                "t": hashlib.md5(
                    (credential["password"] + salt).encode("utf-8"),
                    usedforsecurity=False,
                ).hexdigest(),
            }
        )
    # X-ND-Authorization belongs to Navidrome's separate native Web API.
    # OpenSubsonic /rest requests use exactly one mechanism above.
    return params, {}


def _navidrome_get(
    http: Http,
    credential: dict[str, str],
    method: str,
    parameters: dict[str, str],
    *,
    stream: bool = False,
) -> requests.Response:
    query, headers = _navidrome_auth(credential)
    query.update(parameters)
    url = f"{credential['server_url']}/rest/{method}.view"
    try:
        return http.get(
            url,
            params=query,
            headers=headers,
            allow_redirects=True,
            stream=stream,
        )
    except requests.RequestException as exc:
        # requests exceptions often include a prepared URL. Never include the
        # exception string because that URL can contain API authentication.
        raise SplinedError(
            f"Unable to reach Navidrome/OpenSubsonic ({type(exc).__name__})."
        ) from None


def _navidrome_json(response: requests.Response) -> dict[str, Any]:
    if response.status_code in {401, 403}:
        raise SplinedError(
            "Navidrome rejected the saved authentication credentials."
        )
    if response.status_code != 200:
        raise SplinedError(
            f"Navidrome/OpenSubsonic returned HTTP {response.status_code}."
        )
    try:
        payload = response.json()
    except (ValueError, json.JSONDecodeError) as exc:
        raise SplinedError(
            "Navidrome/OpenSubsonic returned invalid JSON."
        ) from exc
    if not isinstance(payload, dict):
        raise SplinedError("Navidrome/OpenSubsonic returned an invalid response.")
    envelope = payload.get("subsonic-response")
    if not isinstance(envelope, dict):
        raise SplinedError(
            "Navidrome/OpenSubsonic response envelope is missing."
        )
    if str(envelope.get("status") or "").strip().lower() != "ok":
        raise SplinedError("Navidrome/OpenSubsonic request failed.")
    return envelope


def _navidrome_album_title(album: dict[str, Any]) -> str:
    for key in ("album", "title", "name"):
        value = str(album.get(key) or "").strip()
        if value:
            return value
    return ""


def _navidrome_album_mbid(album: dict[str, Any]) -> str:
    for key, value in album.items():
        if str(key).replace("-", "").replace("_", "").casefold() in {
            "musicbrainzid",
            "musicbrainzreleaseid",
        }:
            return str(value or "").strip()
    return ""


def _navidrome_identity(value: str) -> str:
    return "".join(character.casefold() for character in value if character.isalnum())


def match_navidrome_albums(
    rel: Release,
    albums: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    """Use MBID first, otherwise require one unique exact identity match."""
    release_mbid = rel.mbid.strip()
    if release_mbid:
        by_mbid = [
            album
            for album in albums
            if _navidrome_album_mbid(album).casefold() == release_mbid.casefold()
        ]
        if by_mbid:
            return by_mbid
    artist = _navidrome_identity(rel.artist_credit)
    title = _navidrome_identity(rel.title)
    exact = [
        album
        for album in albums
        if _navidrome_identity(str(album.get("artist") or "")) == artist
        and _navidrome_identity(_navidrome_album_title(album)) == title
    ]
    return exact if len(exact) == 1 else []


def discover_navidrome(
    http: Http,
    config_file: Path,
    cfg: dict[str, Any],
    rel: Release,
) -> list[Ref]:
    credential = load_navidrome_credentials(config_file, cfg)
    response = _navidrome_get(
        http,
        credential,
        "search3",
        {
            "query": f"{rel.artist_credit} {rel.title}",
            "artistCount": "0",
            "albumCount": "50",
            "songCount": "0",
        },
    )
    try:
        envelope = _navidrome_json(response)
    finally:
        close = getattr(response, "close", None)
        if callable(close):
            close()
    result = envelope.get("searchResult3")
    albums_raw = result.get("album", []) if isinstance(result, dict) else []
    if not isinstance(albums_raw, list):
        return []
    albums = [album for album in albums_raw if isinstance(album, dict)]
    matched = match_navidrome_albums(rel, albums)
    matched = [
        album for album in matched if str(album.get("coverArt") or "").strip()
    ]
    if not matched:
        return []
    session_id = _register_navidrome_session(credential)
    refs: list[Ref] = []
    for album in matched:
        cover_id = str(album.get("coverArt") or "").strip()
        album_id = str(album.get("id") or cover_id).strip()
        refs.append(
            Ref(
                "navidrome",
                album_id,
                _navidrome_internal_cover_url(session_id, cover_id),
                front=True,
                approved=True,
                types=["Front"],
            )
        )
    return refs


def download_navidrome_cover(http: Http, internal_url: str) -> requests.Response:
    credential, cover_id = _navidrome_internal_cover(internal_url)
    response = _navidrome_get(
        http,
        credential,
        "getCoverArt",
        {"id": cover_id},
        stream=True,
    )
    if response.status_code in {401, 403}:
        response.close()
        raise SplinedError(
            "Navidrome rejected the saved authentication credentials."
        )
    if response.status_code != 200:
        status = response.status_code
        response.close()
        raise SplinedError(
            f"Navidrome cover-art download returned HTTP {status}."
        )
    try:
        content_length = int(response.headers.get("Content-Length") or 0)
    except (TypeError, ValueError):
        content_length = 0
    if content_length > MAX_DOWNLOAD_BYTES:
        response.close()
        raise SplinedError("Navidrome cover art exceeded the download size limit.")
    return response


def discover_fallback(
    http: Http,
    config_file: Path,
    cfg: dict[str, Any],
    artist: str,
    album: str,
    sources: list[str],
    release_mbid: str | None = None,
    queried_sources: set[str] | None = None,
) -> tuple[list[Ref], list[tuple[str, str]]]:
    synthetic = Release(
        mbid=release_mbid or "",
        title=album,
        artist_credit=artist,
        release_group_id=None,
        release_group_title=None,
        track_count=None,
    )
    eligible = [
        source
        for source in sources
        if source != "fanarttv"
        and (source != "coverartarchive" or bool(release_mbid))
        and (source != "musicbrainz" or bool(release_mbid))
    ]

    def work(source: str) -> tuple[list[Ref], str | None]:
        started = time.perf_counter()
        emit_ui(
            "activity",
            category="provider",
            state="start",
            source=source,
            message=f"{provider_label(source)} fallback discovery started",
        )
        try:
            if source == "deezer":
                found = discover_deezer(http, synthetic)
            elif source == "itunes":
                found = discover_itunes(http, config_file, cfg, synthetic)
            elif source == "lastfm":
                found = discover_lastfm(http, config_file, cfg, synthetic)
            elif source == "discogs":
                found = discover_discogs(http, config_file, cfg, artist, album)
            elif source == "coverartarchive":
                found = discover_caa(http, synthetic)
            elif source == "musicbrainz":
                found = discover_musicbrainz_artwork(http, synthetic)
            elif source == "amazon":
                found = discover_amazon(http, synthetic)
            elif source == "navidrome":
                found = discover_navidrome(http, config_file, cfg, synthetic)
            else:
                found = []
            elapsed = time.perf_counter() - started
            emit_ui(
                "activity",
                category="provider",
                state="done",
                source=source,
                count=len(found),
                elapsed=elapsed,
                message=f"{provider_label(source)} returned {len(found)} reference(s) in {elapsed:.2f}s",
            )
            return found, None
        except Exception as exc:
            elapsed = time.perf_counter() - started
            emit_ui(
                "activity",
                category="provider",
                state="error",
                source=source,
                elapsed=elapsed,
                message=f"{provider_label(source)} failed: {exc}",
            )
            return [], str(exc)

    refs: list[Ref] = []
    diag: list[tuple[str, str]] = []
    if queried_sources is not None:
        queried_sources.update(eligible)
    with concurrent.futures.ThreadPoolExecutor(
        max_workers=max(1, min(PROVIDER_DISCOVERY_WORKERS, len(eligible))),
        thread_name_prefix="splined-provider",
    ) as executor:
        scheduled = [(source, executor.submit(work, source)) for source in eligible]
        for source, future in scheduled:
            found, error = future.result()
            refs.extend(found)
            if error is not None:
                diag.append((source, error))
    return refs, diag



def discover_all(
    http: Http,
    config_file: Path,
    cfg: dict[str, Any],
    rel: Release,
    sources: list[str],
    queried_sources: set[str] | None = None,
) -> tuple[list[Ref], list[tuple[str, str]]]:
    def work(source: str) -> tuple[list[Ref], str | None]:
        started = time.perf_counter()
        debug_log(
            f"provider.start source={source} mbid={rel.mbid} "
            f"artist={rel.artist_credit!r} album={rel.title!r}"
        )
        emit_ui(
            "activity",
            category="provider",
            state="start",
            source=source,
            message=f"{provider_label(source)} discovery started",
        )
        try:
            if source == "deezer":
                found = discover_deezer(http, rel)
            elif source == "itunes":
                found = discover_itunes(http, config_file, cfg, rel)
            elif source == "fanarttv":
                found = discover_fanart(http, config_file, cfg, rel)
            elif source == "lastfm":
                found = discover_lastfm(http, config_file, cfg, rel)
            elif source == "coverartarchive":
                found = discover_caa(http, rel)
            elif source == "musicbrainz":
                found = discover_musicbrainz_artwork(http, rel)
            elif source == "discogs":
                found = discover_discogs(
                    http, config_file, cfg, rel.artist_credit, rel.title
                )
            elif source == "amazon":
                found = discover_amazon(http, rel)
            elif source == "navidrome":
                found = discover_navidrome(http, config_file, cfg, rel)
            else:
                found = []
            elapsed = time.perf_counter() - started
            debug_log(
                f"provider.done source={source} refs={len(found)} elapsed={elapsed:.3f}s"
            )
            emit_ui(
                "activity",
                category="provider",
                state="done",
                source=source,
                count=len(found),
                elapsed=elapsed,
                message=f"{provider_label(source)} returned {len(found)} reference(s) in {elapsed:.2f}s",
            )
            return found, None
        except Exception as exc:
            elapsed = time.perf_counter() - started
            debug_log(
                f"provider.error source={source} "
                f"error={type(exc).__name__}: {exc}"
            )
            emit_ui(
                "activity",
                category="provider",
                state="error",
                source=source,
                elapsed=elapsed,
                message=f"{provider_label(source)} failed: {exc}",
            )
            return [], str(exc)

    refs: list[Ref] = []
    diag: list[tuple[str, str]] = []
    if queried_sources is not None:
        queried_sources.update(sources)
    with concurrent.futures.ThreadPoolExecutor(
        max_workers=max(1, min(PROVIDER_DISCOVERY_WORKERS, len(sources))),
        thread_name_prefix="splined-provider",
    ) as executor:
        scheduled = [(source, executor.submit(work, source)) for source in sources]
        # Merge in configured source order so candidate ordering and every
        # downstream tie-break remain identical to the sequential baseline.
        for source, future in scheduled:
            found, error = future.result()
            refs.extend(found)
            if error is not None:
                diag.append((source, error))
    return refs, diag



def image_format(image: Image.Image) -> str:
    f = (image.format or "").upper()
    if f == "JPEG": return "jpeg"
    if f == "PNG": return "png"
    if f == "WEBP": return "webp"
    raise SplinedError(f"Unsupported SPLINED downloaded artwork format: {f or 'unknown'}")


def prepare_run_cache(cache: Path) -> None:
    """Reset transient scan/candidate data before a new operational run."""
    if cache.exists():
        if cache.is_symlink() or not cache.is_dir():
            raise SplinedError(f"Refusing to clean unsafe SPLINED cache directory: {cache}")
        if cache.parent == cache or not cache.name:
            raise SplinedError(f"Refusing to clean unsafe SPLINED cache path: {cache}")
        for path in cache.iterdir():
            if path.is_symlink():
                path.unlink()
            elif path.is_dir():
                shutil.rmtree(path)
            else:
                path.unlink()
    cache.mkdir(parents=True, exist_ok=True)


def clean_cache(cache: Path) -> None:
    """Remove only transient downloaded provider candidates during a running scan."""
    cache.mkdir(parents=True, exist_ok=True)
    for p in cache.iterdir():
        if p.name.startswith("splined-candidate-") and p.is_file() and not p.is_symlink():
            p.unlink()



def cache_extension_from_format(fmt: str) -> str:
    return EXTENSIONS.get(fmt, "bin")



SQUARE_EQUIVALENT_TOLERANCE = 0.005
MAX_AUTO_CROP_DEVIATION = 0.02


def aspect_deviation(width: int, height: int) -> float:
    longest = max(int(width), int(height))
    if longest <= 0:
        return 0.0
    return abs(int(width) - int(height)) / longest


def aspect_ratio(width: int, height: int) -> float:
    return (float(width) / float(height)) if int(height) else 0.0


def output_settings(cfg: dict[str, Any]) -> dict[str, Any]:
    out = section(cfg, "output")

    # Approved projected-final artwork policy.
    square = bool(out.get("square", True))
    square_mode = str(out.get("square_mode", "crop" if square else "off")).strip().lower()
    if not square:
        square_mode = "off"
    if square_mode not in {"off", "crop"}:
        raise SplinedError("[output].square_mode must be 'off' or 'crop'.")

    try:
        round_to = int(out.get("square_round_to", 16) or 0)
    except (TypeError, ValueError) as exc:
        raise SplinedError("[output].square_round_to must be an integer.") from exc
    if round_to < 0:
        raise SplinedError("[output].square_round_to cannot be negative.")

    upscale = bool(out.get("upscale_below_ideal", False))
    try:
        upscale_max_percent = int(out.get("upscale_max_percent", 200) or 0)
    except (TypeError, ValueError) as exc:
        raise SplinedError("[output].upscale_max_percent must be an integer.") from exc
    if not 100 <= upscale_max_percent <= 800:
        raise SplinedError(
            "[output].upscale_max_percent must be between 100 and 800."
        )
    evaluate_final = bool(out.get("evaluate_final_image", True))

    return {
        "square": square,
        "square_mode": square_mode,
        "square_round_to": round_to,
        "upscale_below_ideal": upscale,
        "upscale_max_percent": upscale_max_percent,
        "evaluate_final_image": evaluate_final,
    }

def classify_range(short_side: int, cfg: dict[str, Any]) -> str:
    r = section(cfg, "range")
    mn = int(r.get("min", 1200))
    ideal = int(r.get("ideal", 1800))
    mx = int(r.get("max", 2400))
    ladder = int(r.get("ladder", 3600))
    if short_side < mn:
        return "BelowMinimum"
    if short_side < ideal:
        return "LowerRange"
    if short_side == ideal:
        return "Ideal"
    if short_side <= mx:
        return "UpperRange"
    if short_side <= ladder:
        return "Ladder"
    return "AboveLadder"



def project_dimensions(width: int, height: int, cfg: dict[str, Any]) -> tuple[int, int, dict[str, Any]]:
    settings = output_settings(cfg)
    ideal = int(section(cfg, "range").get("ideal", 1800))
    w, h = int(width), int(height)
    source_deviation = aspect_deviation(w, h)

    operations = {
        "squared": False,
        "cropped": False,
        "square_mode": settings["square_mode"],
        "rounded": False,
        "rounded_to": 0,
        "resized": False,
        "upscaled": False,
        "aspect_deviation": source_deviation,
        "square_equivalent": source_deviation <= SQUARE_EQUIVALENT_TOLERANCE,
        "crop_guarded": False,
    }

    # Evaluate shape before any squaring operation.
    if settings["square_mode"] == "crop" and w != h:
        if source_deviation <= SQUARE_EQUIVALENT_TOLERANCE:
            pass
        elif source_deviation <= MAX_AUTO_CROP_DEVIATION:
            side = min(w, h)
            w = side
            h = side
            operations["squared"] = True
            operations["cropped"] = True
        else:
            operations["crop_guarded"] = True

    # square_round_to is crop/output normalization only; it never reclassifies
    # an existing source and cannot turn exact 1800x1800 into 1792x1792.
    round_to = settings["square_round_to"]
    if operations["cropped"] and w == h and round_to > 1 and w >= round_to:
        rounded_side = (w // round_to) * round_to
        if rounded_side > 0 and rounded_side != w:
            w = rounded_side
            h = rounded_side
            operations["rounded"] = True
            operations["rounded_to"] = round_to

    # Ideal is a minimum quality target, not an output ceiling. Accepted
    # artwork already at or above Ideal keeps its native resolution. Below-
    # Ideal enlargement remains proportional and is allowed only when the
    # requested scale is within the explicit Config v5 safety limit.
    short_side = min(w, h)
    if short_side < ideal and settings["upscale_below_ideal"]:
        scale = ideal / short_side
        requested_percent = scale * 100.0
        operations["upscale_percent"] = requested_percent
        operations["upscale_limited"] = (
            requested_percent > settings["upscale_max_percent"]
        )
        if not operations["upscale_limited"]:
            w = max(1, int(round(w * scale)))
            h = max(1, int(round(h * scale)))
            operations["resized"] = True
            operations["upscaled"] = True

    return w, h, operations


def project_candidate(c: Candidate, cfg: dict[str, Any], target: str) -> dict[str, Any]:
    settings = output_settings(cfg)
    source_deviation = aspect_deviation(c.width, c.height)

    if settings["evaluate_final_image"]:
        width, height, ops = project_dimensions(c.width, c.height, cfg)
    else:
        width, height = c.width, c.height
        ops = {
            "squared": False,
            "cropped": False,
            "square_mode": "off",
            "rounded": False,
            "rounded_to": 0,
            "resized": False,
            "upscaled": False,
            "aspect_deviation": source_deviation,
            "square_equivalent": source_deviation <= SQUARE_EQUIVALENT_TOLERANCE,
            "crop_guarded": False,
        }

    short_side = min(width, height)
    range_type = classify_range(short_side, cfg)
    ideal = int(section(cfg, "range").get("ideal", 1800))
    policy_status, policy_reason = source_policy_decision(
        cfg,
        c.source,
        c.width,
        c.height,
    )
    return {
        "width": width,
        "height": height,
        "short_side": short_side,
        "square": source_deviation <= SQUARE_EQUIVALENT_TOLERANCE,
        "format": target,
        "range_type": range_type,
        "distance": abs(short_side - ideal),
        "acceptable": policy_status != "reject",
        "policy_status": policy_status,
        "policy_reason": policy_reason,
        "converted": c.format != target,
        "squared": bool(ops.get("squared", False)),
        "cropped": bool(ops.get("cropped", False)),
        "square_mode": str(ops.get("square_mode", "off")),
        "rounded": bool(ops.get("rounded", False)),
        "rounded_to": int(ops.get("rounded_to", 0) or 0),
        "resized": bool(ops.get("resized", False)),
        "upscaled": bool(ops.get("upscaled", False)),
        "aspect_deviation": source_deviation,
        "aspect_ratio": aspect_ratio(c.width, c.height),
        "square_equivalent": source_deviation <= SQUARE_EQUIVALENT_TOLERANCE,
        "crop_guarded": bool(ops.get("crop_guarded", False)),
    }

def target_format_for_candidate(c: Candidate, format_order: list[str]) -> str:
    return c.format if c.format in format_order else format_order[0]


def is_discogs_placeholder(ref: Ref) -> bool:
    if ref.source != "discogs":
        return False
    url = str(ref.url or "").strip().lower().split("?", 1)[0].split("#", 1)[0]
    return url.endswith("/images/spacer.gif") or url.endswith("/spacer.gif")


def read_bounded_artwork_response(response: requests.Response) -> bytes:
    raw_length = str(response.headers.get("Content-Length") or "").strip()
    if raw_length:
        try:
            content_length = int(raw_length)
        except ValueError:
            content_length = None
        if content_length is not None and content_length > MAX_DOWNLOAD_BYTES:
            raise SplinedError(
                "artwork download exceeds the 25 MiB limit "
                f"(Content-Length: {content_length} bytes)"
            )

    chunks: list[bytes] = []
    total = 0
    for chunk in response.iter_content(chunk_size=DOWNLOAD_CHUNK_BYTES):
        if not chunk:
            continue
        total += len(chunk)
        if total > MAX_DOWNLOAD_BYTES:
            raise SplinedError("artwork download exceeds the 25 MiB limit")
        chunks.append(chunk)

    if total == 0:
        raise SplinedError("artwork download was empty")
    return b"".join(chunks)


def validate_image_dimensions(width: int, height: int) -> None:
    if width <= 0 or height <= 0:
        raise SplinedError(f"artwork has invalid dimensions: {width}x{height}")
    if width > MAX_IMAGE_PIXELS // height:
        raise SplinedError(
            "artwork exceeds the decoded-image limit "
            f"({width}x{height}; maximum {MAX_IMAGE_PIXELS} pixels)"
        )


def reference_policy_decision(
    cfg: dict[str, Any] | None,
    ref: Ref,
) -> tuple[str, str]:
    """Apply only policy supported by reliable discovery metadata."""
    if cfg is None:
        return ("accept", "No source policy supplied") if ref.front else (
            "reject",
            "Reference is not a front image",
        )
    if not reference_allowed(cfg, ref.source, ref.front):
        return "reject", "Reference is not primary/front under active source policy"
    if ref.width is None or ref.height is None:
        return "unknown", "Downloaded dimensions are required"
    # Source-policy dimension constraints are defined on the obtained source,
    # exactly as in project_candidate after download. Reliable provider
    # metadata can therefore reject here without changing later semantics.
    return source_policy_decision(cfg, ref.source, ref.width, ref.height)


def filter_download_references(
    refs: list[Ref],
    cfg: dict[str, Any] | None,
) -> tuple[list[Ref], list[tuple[str, str]]]:
    """Filter only references that can be rejected before downloading."""
    filtered: list[Ref] = []
    diagnostics: list[tuple[str, str]] = []
    for ref in refs:
        if not ref.url:
            reason = "reference has no URL"
            diagnostics.append((ref.source, reason))
            emit_ui(
                "activity",
                category="download",
                state="skipped",
                source=ref.source,
                message=f"{provider_label(ref.source)} reference skipped: {reason}",
            )
            continue
        if is_discogs_placeholder(ref):
            reason = "provider placeholder"
            diagnostics.append((ref.source, reason))
            emit_ui(
                "activity",
                category="download",
                state="skipped",
                source=ref.source,
                message=f"{provider_label(ref.source)} placeholder skipped",
            )
            continue
        decision, reason = reference_policy_decision(cfg, ref)
        if decision == "reject":
            diagnostics.append((ref.source, f"policy-filtered: {reason}"))
            debug_log(
                f"download.policy_filtered source={ref.source} id={ref.id} reason={reason}"
            )
            emit_ui(
                "activity",
                category="policy",
                state="filtered",
                source=ref.source,
                message=f"{provider_label(ref.source)} candidate policy-filtered: {reason}",
            )
            continue
        filtered.append(ref)
    return filtered, diagnostics



def download_candidates(
    http: Http,
    refs: list[Ref],
    sources: list[str],
    cache: Path,
    cfg: dict[str, Any] | None = None,
    clean_first: bool = True,
) -> tuple[list[Candidate], list[tuple[str, str]]]:
    if clean_first:
        clean_cache(cache)
    eligible_refs, diag = filter_download_references(refs, cfg)
    scheduled_refs: list[Ref] = []
    seen_urls: set[tuple[str, str]] = set()
    for ref in eligible_refs:
        identity = (ref.source, ref.url.strip())
        if identity in seen_urls:
            emit_ui(
                "activity",
                category="download",
                state="skipped",
                source=ref.source,
                message=(
                    f"{provider_label(ref.source)} duplicate fetch reused; "
                    "candidate metadata retained"
                ),
            )
            continue
        seen_urls.add(identity)
        scheduled_refs.append(ref)

    def work(ref: Ref) -> tuple[Candidate | None, str | None]:
        started = time.perf_counter()
        try:
            debug_log(f"download.start source={ref.source} id={ref.id}")
            emit_ui(
                "activity",
                category="download",
                state="start",
                source=ref.source,
                message=f"Downloading {provider_label(ref.source)} candidate",
            )
            response_context = (
                download_navidrome_cover(http, ref.url)
                if ref.url.startswith(NAVIDROME_INTERNAL_SCHEME)
                else http.get(ref.url, allow_redirects=True, stream=True)
            )
            with response_context as response:
                if response.status_code != 200:
                    raise SplinedError(
                        f"artwork download returned HTTP {response.status_code}"
                    )
                resolved_url = str(getattr(response, "url", "") or ref.url).strip()
                if resolved_url and not ref.url.startswith(NAVIDROME_INTERNAL_SCHEME):
                    ref.browser_url = resolved_url
                artwork = read_bounded_artwork_response(response)
            with Image.open(io.BytesIO(artwork)) as im:
                fmt = image_format(im)
                width, height = im.size
                validate_image_dimensions(width, height)
                im.verify()
            digest = hashlib.sha256((ref.source + "\0" + ref.url).encode()).hexdigest()[:24]
            path = cache / f"splined-candidate-{digest}.{cache_extension_from_format(fmt)}"
            path.write_bytes(artwork)
            candidate = Candidate(
                ref, path, width, height, fmt, sources.index(ref.source)
            )
            elapsed = time.perf_counter() - started
            debug_log(
                f"download.done source={ref.source} id={ref.id} "
                f"size={width}x{height} format={fmt} elapsed={elapsed:.3f}s"
            )
            emit_ui(
                "activity",
                category="download",
                state="done",
                source=ref.source,
                elapsed=elapsed,
                message=f"{provider_label(ref.source)} {width}×{height} downloaded in {elapsed:.2f}s",
            )
            return candidate, None
        except Exception as exc:
            elapsed = time.perf_counter() - started
            debug_log(
                f"download.error source={ref.source} id={ref.id} "
                f"error={type(exc).__name__}: {exc}"
            )
            emit_ui(
                "activity",
                category="download",
                state="error",
                source=ref.source,
                elapsed=elapsed,
                message=f"{provider_label(ref.source)} download failed: {exc}",
            )
            return None, f"{exc} ({ref.url})"

    downloaded: dict[tuple[str, str], tuple[Candidate | None, str | None]] = {}
    started_all = time.perf_counter()
    with concurrent.futures.ThreadPoolExecutor(
        max_workers=max(1, min(CANDIDATE_DOWNLOAD_WORKERS, len(scheduled_refs))),
        thread_name_prefix="splined-download",
    ) as executor:
        scheduled = [(ref, executor.submit(work, ref)) for ref in scheduled_refs]
        for ref, future in scheduled:
            downloaded[(ref.source, ref.url.strip())] = future.result()

    # Recreate a candidate for every original reference in original order.
    # Proven duplicate URLs share bytes and image probing, but their provider
    # IDs, approval flags and all existing ranking inputs remain authoritative.
    out: list[Candidate] = []
    for ref in eligible_refs:
        candidate, error = downloaded[(ref.source, ref.url.strip())]
        if candidate is not None:
            # Duplicate discovery references retain their own identity/ranking
            # metadata but share the final URL proven by the successful fetch.
            if not ref.browser_url:
                ref.browser_url = candidate.ref.browser_url or ref.url
            out.append(
                Candidate(
                    ref,
                    candidate.path,
                    candidate.width,
                    candidate.height,
                    candidate.format,
                    sources.index(ref.source),
                )
            )
        if error is not None:
            diag.append((ref.source, error))
    debug_log(
        f"download.batch refs={len(eligible_refs)} unique_fetches={len(scheduled_refs)} "
        f"candidates={len(out)} "
        f"workers={min(CANDIDATE_DOWNLOAD_WORKERS, len(scheduled_refs)) if scheduled_refs else 0} "
        f"elapsed={time.perf_counter() - started_all:.3f}s"
    )
    return out, diag


def discover_normal_ranked(
    http: Http,
    config_file: Path,
    cfg: dict[str, Any],
    rel: Release,
    sources: list[str],
    cache: Path,
    format_order: list[str],
    history: dict[str, Any],
    queried_sources: set[str] | None = None,
) -> tuple[list[Candidate], list[tuple[str, str]], list[str]]:
    # DEV18 stabilization:
    # Source history remains persistent/statistical, but it no longer changes
    # provider request order or early-stops discovery. This intentionally
    # restores the proven pre-history behavior: discover every configured
    # source together, then score the complete candidate pool.
    del history, format_order

    debug_log(
        "normal.discovery mode=all-sources "
        f"sources={','.join(sources)} mbid={rel.mbid}"
    )

    refs, diag = discover_all(
        http,
        config_file,
        cfg,
        rel,
        sources,
        queried_sources=queried_sources,
    )
    debug_log(f"normal.discovery refs_total={len(refs)}")

    candidates, download_diag = download_candidates(
        http,
        refs,
        sources,
        cache,
        cfg,
        clean_first=True,
    )
    diag += download_diag

    debug_log(f"normal.discovery candidates_total={len(candidates)}")
    return candidates, diag, list(sources)



def range_class(c: Candidate, cfg: dict[str, Any]) -> str:
    return classify_range(c.short_side, cfg)


def acceptable(c: Candidate, cfg: dict[str, Any]) -> bool:
    return (
        c.strict_preferred_eligible
        and source_policy_decision(cfg, c.source, c.width, c.height)[0] != "reject"
    )


def candidate_key(c: Candidate, cfg: dict[str, Any], format_order: list[str]):
    projected = project_candidate(c, cfg, target_format_for_candidate(c, format_order))
    range_rank = {
        "Ideal": 0,
        "UpperRange": 1,
        "LowerRange": 2,
        "Ladder": 3,
        "BelowMinimum": 4,
        "AboveLadder": 5,
    }.get(projected["range_type"], 99)

    transform_penalty = (
        1 if projected["upscaled"] else 0,
        1 if projected["cropped"] else 0,
        1 if projected["resized"] else 0,
        1 if projected["converted"] else 0,
    )

    return (
        {"accept": 0, "fallback": 1, "reject": 2}.get(projected["policy_status"], 2),
        projected["distance"],
        range_rank,
        transform_penalty,
        -projected["short_side"],
        c.source_priority,
        format_order.index(projected["format"]) if projected["format"] in format_order else 999999,
        provider_label(c.source).lower(),
        str(c.ref.id),
    )

def select_best(candidates: list[Candidate], cfg: dict[str, Any], format_order: list[str]) -> Candidate | None:
    valid = [
        c
        for c in candidates
        if c.strict_preferred_eligible
        and project_candidate(c, cfg, target_format_for_candidate(c, format_order))["acceptable"]
    ]
    return min(valid, key=lambda c: candidate_key(c, cfg, format_order)) if valid else None


def summary_line(c: Candidate, cfg: dict[str, Any], format_order: list[str]) -> str:
    projected = project_candidate(c, cfg, target_format_for_candidate(c, format_order))
    return (
        f"{c.width}x{c.height} {c.format.upper()} {projected['range_type']} "
        f"distance={projected['distance']} square={str(projected['square']).lower()} "
        f"acceptable={str(projected['acceptable']).lower()} approved={str(c.ref.approved).lower()} id={c.ref.id}"
    )


def musicbrainz_search_releases(
    http: Http,
    config_file: Path,
    cfg: dict[str, Any],
    artist: str,
    album: str,
) -> list[dict[str, str]]:
    headers, _ = mb_headers(config_file, cfg)
    mbcfg = musicbrainz_settings(config_file, cfg)
    if not mbcfg["enabled"]:
        return []
    timeout = float(mbcfg.get("mb_recording_timeout", 7.0))
    retry_max = int(mbcfg.get("retry_max", 2))
    min_delay = max(0.0, float(mbcfg.get("mb_min_delay", 1.05)))
    query = f'artist:"{artist}" AND release:"{album}"'

    for attempt in range(retry_max + 1):
        if http.last_mb_request is not None:
            wait = min_delay - (time.monotonic() - http.last_mb_request)
            if wait > 0:
                time.sleep(wait)
        try:
            http.last_mb_request = time.monotonic()
            response = http.get(
                f"{MB_BASE}/release/",
                # One bounded request supplies the flat deterministic match
                # list without paging the MusicBrainz API.
                params={"query": query, "fmt": "json", "limit": 99},
                headers=headers,
                timeout=timeout,
            )
        except requests.RequestException as exc:
            if attempt < retry_max:
                continue
            raise SplinedError(f"MusicBrainz release search failed after {attempt + 1} attempt(s): {exc}") from exc

        if (response.status_code == 429 or 500 <= response.status_code <= 599) and attempt < retry_max:
            continue
        if response.status_code != 200:
            raise SplinedError(f"MusicBrainz release search returned HTTP {response.status_code}")

        out: list[dict[str, str]] = []
        for item in response.json().get("releases", []):
            if not isinstance(item, dict):
                continue
            mbid = valid_mbid(str(item.get("id") or ""))
            if not mbid:
                continue
            artist_parts: list[str] = []
            for credit in item.get("artist-credit", []) or []:
                if isinstance(credit, str):
                    artist_parts.append(credit)
                elif isinstance(credit, dict):
                    artist_parts.append(str(credit.get("name") or (credit.get("artist") or {}).get("name") or ""))
                    artist_parts.append(str(credit.get("joinphrase") or ""))
            release_group = item.get("release-group") or {}
            secondary_types = [
                str(value).strip()
                for value in release_group.get("secondary-types", []) or []
                if str(value).strip()
            ]
            secondary_lookup = {value.casefold(): value for value in secondary_types}
            release_class = next(
                (
                    secondary_lookup[key]
                    for key in ("compilation", "soundtrack")
                    if key in secondary_lookup
                ),
                str(release_group.get("primary-type") or "").strip()
                or next(iter(secondary_types), "Release"),
            )
            out.append({
                "id": mbid,
                "title": str(item.get("title") or "").strip(),
                "artist": "".join(artist_parts).strip(),
                "date": str(item.get("date") or "").strip(),
                "country": str(item.get("country") or "").strip(),
                "status": str(item.get("status") or "").strip(),
                "release_class": release_class,
                "release_group_id": str(release_group.get("id") or "").strip(),
            })
            out[-1]["track"] = ""
            out[-1]["group"] = (
                f"{out[-1]['artist']} · "
                f"{out[-1]['release_class'].title()}"
            )
        return out

    return []


def musicbrainz_result_decade(item: dict[str, Any]) -> str:
    year = str(item.get("date") or "")[:4]
    return f"{year[:3]}0s" if len(year) == 4 and year.isdigit() else "Unknown"


def musicbrainz_result_type(item: dict[str, Any]) -> str:
    raw = str(item.get("release_class") or "Release").strip()
    aliases = {
        "album": "Album",
        "single": "Single",
        "ep": "EP",
        "compilation": "Compilation",
        "soundtrack": "Soundtrack",
    }
    return aliases.get(raw.casefold(), raw.title() or "Release")


def decorate_musicbrainz_results(
    results: list[dict[str, str]],
    *,
    current_release_id: str = "",
    inspected_resolutions: dict[str, str] | None = None,
    query_artist: str = "",
    query_album: str = "",
) -> list[dict[str, str]]:
    """Add stable display/cache state without performing artwork requests."""
    current = str(current_release_id or "").strip().casefold()
    inspected = {
        str(key).strip().casefold(): str(value or "")
        for key, value in (inspected_resolutions or {}).items()
    }
    decorated: list[dict[str, str]] = []
    for original_index, raw in enumerate(results):
        item = {str(key): str(value) for key, value in raw.items()}
        release_id = item.get("id", "").strip().casefold()
        item["decade"] = musicbrainz_result_decade(item)
        item["release_class"] = musicbrainz_result_type(item)
        item["selection_state"] = (
            "current"
            if release_id and release_id == current
            else "visited"
            if release_id in inspected
            else ""
        )
        item["resolution"] = inspected.get(release_id, "")
        if not item.get("score", "").strip():
            item["score"] = str(musicbrainz_title_match(query_album, item.get("title", "")))
        item["query_artist"] = query_artist
        release_group_id = item.get("release_group_id", "").strip().casefold()
        item["artwork_url"] = (
            f"https://coverartarchive.org/release-group/{release_group_id}/front"
            if release_group_id
            else f"https://coverartarchive.org/release/{release_id}/front"
            if release_id
            else ""
        )
        item["musicbrainz_url"] = (
            f"https://musicbrainz.org/release/{release_id}"
            if release_id
            else ""
        )
        item["original_index"] = str(original_index)
        decorated.append(item)

    # Presentation follows the accepted Windows family/artist/date/country
    # ordering. Decade remains harmless diagnostic metadata but is never a
    # visible category or a sort authority.
    return [dict(item) for item in order_musicbrainz_results(decorated)]


def musicbrainz_picker(
    http: Http,
    config_file: Path,
    cfg: dict[str, Any],
    artist: str,
    album: str,
    exact_mbid: str | None = None,
    *,
    current_release_id: str = "",
    current_release: Release | None = None,
    inspected_resolutions: dict[str, str] | None = None,
) -> Release | None:
    if exact_mbid:
        print(f"  {cyan('MusicBrainz:'):13} {magenta('retrying exact release')} {magenta(exact_mbid)}")
        try:
            return lookup_release(http, config_file, cfg, exact_mbid)
        except Exception as exc:
            print(f"  {red('MusicBrainz retry failed: ' + str(exc))}")
            return None

    cache_key = (normalize_text(artist), normalize_text(album))
    search_cache = getattr(http, "_splined_mb_release_search_cache", None)
    if not isinstance(search_cache, dict):
        search_cache = {}
        setattr(http, "_splined_mb_release_search_cache", search_cache)
    if cache_key in search_cache:
        results = [dict(item) for item in search_cache[cache_key]]
    else:
        try:
            results = musicbrainz_search_releases(
                http, config_file, cfg, artist, album
            )
        except Exception as exc:
            print(f"  {red('MusicBrainz search failed: ' + str(exc))}")
            return None
        search_cache[cache_key] = [dict(item) for item in results]

    current_id = str(current_release_id or "").strip().casefold()
    if (
        current_release is not None
        and current_id
        and all(
            str(result.get("id") or "").strip().casefold() != current_id
            for result in results
        )
    ):
        # Preserve exactly one green row even when the bounded MusicBrainz
        # search does not return the release whose candidates are on screen.
        results.append(
            {
                "id": current_id,
                "title": str(current_release.title or ""),
                "artist": str(current_release.artist_credit or ""),
                "date": "",
                "country": "",
                "status": "",
                "release_class": "Album",
                "release_group_id": str(
                    current_release.release_group_id or ""
                ),
            }
        )

    if not results:
        print(f"  {yellow('MusicBrainz search returned no releases.')}")
        return None

    results = decorate_musicbrainz_results(
        results,
        current_release_id=current_release_id,
        inspected_resolutions=inspected_resolutions,
        query_artist=artist,
        query_album=album,
    )

    print()
    print(f"  {cyan('MusicBrainz release search')}")
    print(
        "  "
        + ljust_color(cyan("[#]"), 5) + " "
        + ljust_color(cyan("Artist"), 24) + " "
        + ljust_color(cyan("Country"), 8) + " "
        + ljust_color(cyan("Date"), 10) + " "
        + ljust_color(cyan("Release Type"), 16) + " "
        + ljust_color(cyan("Resolution"), 12) + " "
        + ljust_color(cyan("Release"), 34) + " "
        + cyan("URL")
    )
    print("  " + gray("-" * 129))
    for index, result in enumerate(results, 1):
        state_color = (
            green
            if result.get("selection_state") == "current"
            else blue
            if result.get("selection_state") == "visited"
            else white
        )
        print(
            "  "
            + ljust_color(state_color(f"[{index}]"), 5) + " "
            + ljust_color(state_color(result["artist"][:24]), 24) + " "
            + ljust_color(state_color(result["country"][:8]), 8) + " "
            + ljust_color(state_color(result["date"][:10]), 10) + " "
            + ljust_color(state_color(result["release_class"][:16]), 16) + " "
            + ljust_color(state_color(result["resolution"][:12]), 12) + " "
            + ljust_color(state_color(result["title"][:34]), 34) + " "
            + magenta("[URL]")
        )

    while True:
        answer = read_input(
            "MusicBrainz choice [#] or [b] back: ",
            kind="musicbrainz-results",
            options=results,
            back_action="source-results",
            authority={
                "artist": str(
                    current_release.artist_credit
                    if current_release is not None
                    else artist
                ),
                "album": str(
                    current_release.title
                    if current_release is not None
                    else album
                ),
                "artist_mbids": "",
                "release_mbid": str(
                    current_release.mbid
                    if current_release is not None
                    else current_release_id
                ),
                "recording_mbid": "",
            },
        ).strip().lower()
        if answer == "__batch_return__":
            raise InteractionReturn()
        if answer == "__cancel__":
            raise InteractionCancelled()
        if answer in {"b", ""}:
            return None
        if answer.isdigit():
            index = int(answer)
            if 1 <= index <= len(results):
                release_id = results[index - 1]["id"]
                if (
                    current_release is not None
                    and release_id.casefold() == current_id
                ):
                    return current_release
                release_cache = getattr(
                    http, "_splined_mb_release_lookup_cache", None
                )
                if not isinstance(release_cache, dict):
                    release_cache = {}
                    setattr(http, "_splined_mb_release_lookup_cache", release_cache)
                if release_id in release_cache:
                    return release_cache[release_id]
                try:
                    selected = lookup_release(
                        http, config_file, cfg, release_id
                    )
                    release_cache[release_id] = selected
                    return selected
                except Exception as exc:
                    print(f"  {red('MusicBrainz release lookup failed: ' + str(exc))}")
                    return None
        print("Choose a listed number or b.")


def fallback_sort_key(c: Candidate, cfg: dict[str, Any], format_order: list[str]):
    projected = project_candidate(c, cfg, target_format_for_candidate(c, format_order))
    return (
        -projected["short_side"],
        0 if projected["square"] else 1,
        0 if c.ref.approved else 1,
        c.source_priority,
        format_order.index(projected["format"]) if projected["format"] in format_order else 999999,
        str(c.ref.id),
    )


def fallback_top_candidates(
    candidates: list[Candidate],
    cfg: dict[str, Any],
    format_order: list[str],
    limit: int = 10,
) -> list[Candidate]:
    return sorted(candidates, key=lambda c: fallback_sort_key(c, cfg, format_order))[:limit]


def fallback_suggested(
    candidates: list[Candidate],
    cfg: dict[str, Any],
    format_order: list[str],
) -> Candidate | None:
    if not candidates:
        return None

    def key(c: Candidate):
        projected = project_candidate(c, cfg, target_format_for_candidate(c, format_order))
        return (
            projected["distance"],
            0 if projected["square"] else 1,
            0 if c.ref.approved else 1,
            c.source_priority,
            format_order.index(projected["format"]) if projected["format"] in format_order else 999999,
            -projected["short_side"],
            str(c.ref.id),
        )

    return min(candidates, key=key)


def fallback_reason_from_error(exc: Exception) -> str:
    message = str(exc)
    upper = message.upper()
    if "503" in message:
        return "TEMPORARY MB FAILURE [503]"
    if "429" in message:
        return "TEMPORARY MB FAILURE [429]"
    if "TIMEOUT" in upper or "TIMED OUT" in upper:
        return "TEMPORARY MB FAILURE [timeout]"
    if "401" in message or "AUTHENTICATION" in upper:
        return "MB AUTH FAILURE"
    if "NOT FOUND" in upper or "404" in message:
        return "MB RELEASE NOT FOUND"
    return "TEMPORARY MB FAILURE"



def prepare_samples(cache: Path, configured: Path | None = None) -> Path:
    sample = configured if configured is not None else cache / "samples"
    if sample.exists():
        if sample.is_symlink() or not sample.is_dir(): raise SplinedError(f"Refusing to clean unsafe SPLINED samples directory: {sample}")
    sample.mkdir(parents=True, exist_ok=True); return sample


def sanitize(value: str) -> str:
    s = "".join("_" if ord(c)<32 or c in '<>:"/\\|?*' else c for c in value.strip()).strip().rstrip(" .")
    return s or "unknown"


def choose_destination(canonical: Path, content: bytes, preserve: bool) -> tuple[Path,bool]:
    if not preserve: return canonical, False
    if not canonical.exists(): return canonical, False
    if canonical.read_bytes() == content: return canonical, True
    for n in range(2,1_000_001):
        p = canonical.with_name(f"{canonical.stem}-({n}){canonical.suffix}")
        if not p.exists(): return p, False
        if p.read_bytes() == content: return p, True
    raise SplinedError(f"Unable to find an available preserved artwork filename for {canonical}")


def atomic_write(path: Path, content: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True); fd, tmp = tempfile.mkstemp(prefix=f".{path.name}.", suffix=".tmp", dir=path.parent)
    try:
        with os.fdopen(fd,"wb") as h: h.write(content); h.flush(); os.fsync(h.fileno())
        os.replace(tmp,path)
    except Exception:
        try: os.unlink(tmp)
        except OSError: pass
        raise


def remove_numbered_cover_variants(album_dir: Path, file_name: str) -> list[Path]:
    """Remove numbered SPLINED cover variants across supported image formats.

    Canonical files (cover.jpg, cover.png, cover.webp) are deliberately kept.
    Only cover-(N).jpg/.jpeg/.png/.webp files are removed.
    """
    escaped = re.escape(file_name)
    pattern = re.compile(
        rf"^{escaped}-\([2-9][0-9]*\)\.(?:jpg|jpeg|png|webp)$",
        re.IGNORECASE,
    )
    removed: list[Path] = []

    if not album_dir.exists() or not album_dir.is_dir():
        return removed

    for path in album_dir.iterdir():
        if path.is_symlink() or not path.is_file():
            continue
        if pattern.fullmatch(path.name):
            path.unlink()
            removed.append(path)

    return removed


def write_sample(sample_dir: Path, rel: Release, c: Candidate, preserve: bool) -> tuple[Path,bool]:
    album_dir = sample_dir / sanitize(rel.artist_credit) / sanitize(rel.title)
    album_dir.mkdir(parents=True, exist_ok=True)
    content = c.path.read_bytes(); canonical = album_dir / f"{sanitize(c.source)}.sample.{EXTENSIONS[c.format]}"; dest, unchanged = choose_destination(canonical, content, preserve)
    if not unchanged: atomic_write(dest,content)
    return dest, unchanged


def validate_file_name(name: str) -> None:
    name=name.strip()
    if not name: raise SplinedError("SPLINED output file_name cannot be empty.")
    if name in {".",".."} or "/" in name or "\\" in name: raise SplinedError("SPLINED output file_name must be a single filename stem without directories.")
    if Path(name).suffix: raise SplinedError("SPLINED output file_name must not include an extension; use output.file_formats instead.")



def prepare_final(
    c: Candidate,
    cfg: dict[str, Any],
    target: str,
    allow_out_of_range: bool = False,
) -> tuple[bytes, dict[str, Any]]:
    projected = project_candidate(c, cfg, target)
    if not projected["acceptable"] and not allow_out_of_range:
        raise SplinedError(
            f"Selected artwork candidate is outside the accepted SPLINED range: {c.width}x{c.height}."
        )

    if (
        projected["width"] == c.width
        and projected["height"] == c.height
        and not projected["converted"]
    ):
        info = dict(projected)
        return c.path.read_bytes(), info

    with Image.open(c.path) as im:
        validate_image_dimensions(im.width, im.height)
        im.load()

        if projected["squared"] and projected["square_mode"] == "crop":
            side = min(im.size)
            left = (im.width - side) // 2
            top = (im.height - side) // 2
            im = im.crop((left, top, left + side, top + side))

        if im.size != (projected["width"], projected["height"]):
            im = im.resize((projected["width"], projected["height"]), Image.Resampling.LANCZOS)

        if target == "jpeg" and im.mode not in {"RGB", "L"}:
            im = im.convert("RGB")

        b = io.BytesIO()
        save_format = {"jpeg": "JPEG", "png": "PNG", "webp": "WEBP"}[target]
        kwargs = {"quality": 95} if target == "jpeg" else {}
        im.save(b, format=save_format, **kwargs)

    info = dict(projected)
    return b.getvalue(), info


def preview_destination(
    album: AlbumDir,
    c: Candidate,
    cfg: dict[str, Any],
    fmt_order: list[str],
    allow_out_of_range: bool = False,
) -> tuple[Path, dict[str, Any]]:
    output = section(cfg, "output")
    preserve = bool(output.get("preserve_file", True))
    name = str(output.get("file_name", "cover")).strip()
    validate_file_name(name)
    target = target_format_for_candidate(c, fmt_order)
    content, info = prepare_final(c, cfg, target, allow_out_of_range=allow_out_of_range)
    canonical = album.path / f"{name}.{EXTENSIONS[target]}"
    if not preserve:
        return canonical, info
    dest, _ = choose_destination(canonical, content, True)
    return dest, info


def finalize(
    album: AlbumDir,
    c: Candidate,
    cfg: dict[str, Any],
    fmt_order: list[str],
    allow_out_of_range: bool = False,
) -> tuple[str, Path, dict[str, Any]]:
    output = section(cfg, "output")
    preserve = bool(output.get("preserve_file", True))
    name = str(output.get("file_name", "cover")).strip()
    validate_file_name(name)
    target = target_format_for_candidate(c, fmt_order)
    content, info = prepare_final(c, cfg, target, allow_out_of_range=allow_out_of_range)
    canonical = album.path / f"{name}.{EXTENSIONS[target]}"
    mode = str(cfg.get("mode", "read")).strip().lower()

    if not preserve:
        if mode == "read":
            unchanged = canonical.exists() and canonical.read_bytes() == content
            return ("UNCHANGED" if unchanged else "READ-ONLY (would install)"), canonical, info

        # Non-preserve mode:
        # - overwrite only the canonical file for the selected output format
        # - remove stale numbered cover-(N) variants across JPG/JPEG/PNG/WEBP
        # - leave other canonical cover formats untouched
        atomic_write(canonical, content)
        remove_numbered_cover_variants(album.path, name)
        return "INSTALLED", canonical, info

    dest, unchanged = choose_destination(canonical, content, True)
    if unchanged:
        return "UNCHANGED", dest, info
    if mode == "read":
        return "READ-ONLY (would install)", dest, info
    atomic_write(dest, content)
    return "INSTALLED", dest, info



def render_candidate_table(
    candidates: list[Candidate],
    cfg: dict[str, Any],
    fmt_order: list[str],
    selected: Candidate | None = None,
    suggested: Candidate | None = None,
    manual_fallback: bool = False,
) -> None:
    candidate_items: list[dict[str, Any]] = []
    for index, candidate in enumerate(candidates, 1):
        target = target_format_for_candidate(candidate, fmt_order)
        projected = project_candidate(candidate, cfg, target)
        candidate_items.append(
            {
                "number": index,
                "source": provider_label(candidate.source),
                "width": candidate.width,
                "height": candidate.height,
                "format": candidate.format,
                "range_type": projected["range_type"],
                "distance": projected["distance"],
                "square": projected["square"],
                "acceptable": projected["acceptable"],
                "approved": candidate.ref.approved,
                "id": str(candidate.ref.id),
                "url": candidate.ref.url,
                "path": str(candidate.path),
                "provenance": (
                    "[LOCAL]"
                    if candidate.source in {"local", "webpstill", "embedded"}
                    else "[Enhanced]" if candidate.source == "enhanced" else "[URL]"
                ),
                "selected": candidate is selected,
                "suggested": candidate is suggested,
            }
        )
    emit_ui(
        "candidates",
        items=candidate_items,
        manual_fallback=manual_fallback,
        aisplined=aisplined_settings(cfg),
        ai_runtime_available=False,
        ideal=int(section(cfg, "range").get("ideal", 1800)),
    )

    columns = [
        ("[#]", 5, "right"),
        ("Source", 8, "left"),
        ("Resolution", 12, "left"),
        ("Type", 6, "left"),
        ("Range Type", 13, "left"),
        ("Distance", 10, "left"),
        ("Square", 8, "left"),
        ("Acceptable", 12, "left"),
        ("Approved", 10, "left"),
        ("ID", 0, "left"),
    ]
    header_cells = []
    for label, width, align in columns:
        colored = cyan(label)
        if width:
            colored = rjust_color(colored, width) if align == "right" else ljust_color(colored, width)
        header_cells.append(colored)
    print("  " + " ".join(header_cells))
    print("  " + gray("-" * 128))

    for index, candidate in enumerate(candidates, 1):
        target = target_format_for_candidate(candidate, fmt_order)
        projected = project_candidate(candidate, cfg, target)

        if manual_fallback and candidate is suggested:
            marker = paint("PURPLE", "*[s]")
        elif candidate is selected:
            marker = green(f"*[{index}]")
        else:
            marker = white(f"[{index}]")

        cells = [
            rjust_color(marker, 5),
            ljust_color(magenta(provider_label(candidate.source)), 8),
            ljust_color(white(f"{candidate.width}x{candidate.height}"), 12),
            ljust_color(white(candidate.format.upper()), 6),
            ljust_color(color_range_type(projected["range_type"]), 13),
            ljust_color(white(str(projected["distance"])), 10),
            ljust_color(bool_color(projected["square"]), 8),
            ljust_color(bool_color(projected["acceptable"]), 12),
            ljust_color(bool_color(candidate.ref.approved), 10),
            white(str(candidate.ref.id)),
        ]
        row = "  " + " ".join(cells)
        print(bold(row) if candidate is selected else row)


def apply_selected_candidate(
    album: AlbumDir,
    sample_release: Release,
    candidate: Candidate,
    cfg: dict[str, Any],
    fmt_order: list[str],
    sample_dir: Path,
    preserve: bool,
    samples_enabled: bool,
    summary: Summary,
    allow_out_of_range: bool = False,
) -> bool:
    target = target_format_for_candidate(candidate, fmt_order)

    # Preflight the exact final transformation before writing either sample or
    # artwork. Manual fallback picks may intentionally be below/above the normal
    # acceptance range; automatic selections may not.
    try:
        prepare_final(
            candidate,
            cfg,
            target,
            allow_out_of_range=allow_out_of_range,
        )
    except Exception as exc:
        summary.failed += 1
        print(f"  {red('ERROR: final artwork validation failed: ' + str(exc))}")
        return False

    try:
        if samples_enabled:
            sample_path, sample_unchanged = write_sample(sample_dir, sample_release, candidate, preserve)
            summary.samples_unchanged += int(sample_unchanged)
            summary.samples_written += int(not sample_unchanged)
        else:
            sample_path = sample_dir / (
                f"{sanitize(sample_release.artist_credit)}."
                f"{sanitize(sample_release.title)}.sample.{EXTENSIONS[candidate.format]}"
            )
    except Exception as exc:
        summary.failed += 1
        print(f"  {red('ERROR: selected sample failed: ' + str(exc))}")
        return False

    try:
        action, dest, info = finalize(
            album,
            candidate,
            cfg,
            fmt_order,
            allow_out_of_range=allow_out_of_range,
        )
    except Exception as exc:
        summary.failed += 1
        print(f"  {red('ERROR: final artwork failed: ' + str(exc))}")
        return False

    summary.selected += 1

    if action == "INSTALLED":
        summary.installed += 1
        action_display = bracketed_text(action, green)
    elif action == "UNCHANGED":
        summary.unchanged += 1
        action_display = bracketed_text(action, cyan)
    else:
        summary.read_only += 1
        action_display = bracketed_text(action, yellow)

    print()
    print(f"  {cyan('Artwork:'):13} {action_display}")
    print(f"  {cyan('Sample:'):13} {orange(sample_path.name)}")
    print(
        f"  {cyan('Selected:'):13} "
        f"{magenta(provider_label(candidate.source))} {white('·')} "
        f"{orange(f'{candidate.width}x{candidate.height} {candidate.format.upper()}')} "
        f"{white('/')} {orange(dest.name)}"
    )
    print(
        f"  {cyan('Conversion:'):13} "
        f"{white('squared=')}{bool_color(bool(info['square']))}"
        f"{white(' · resized=')}{bool_color(bool(info['resized']))}"
        f"{white(' · converted=')}{bool_color(bool(info['converted']))}"
        f"{white(' · final=')}{orange(str(info['width']) + 'x' + str(info['height']))}"
    )
    return True

def resolve_scan_root(
    config_file: Path,
    cfg: dict[str, Any],
    scan_words: list[str] | None,
) -> tuple[Path, Path]:
    scan = section(cfg, "scan")
    library = section(cfg, "library")

    library_raw = str(library.get("music_library", "")).strip()
    library_root = resolve_path(config_file, library_raw) if library_raw else Path("/music")

    if scan_words:
        raw_override = " ".join(str(word) for word in scan_words).strip()
        if not raw_override:
            raise SplinedError("The configured scan path cannot be empty.")
        override = Path(raw_override)
        root = override if override.is_absolute() else library_root / override
        return root, library_root

    raw_root = str(scan.get("scan_library_dir", "")).strip()
    if not raw_root:
        raise SplinedError(
            "SPLINED [scan].scan_library_dir is not configured; "
            "configure the library path in Settings."
        )
    return resolve_path(config_file, raw_root), library_root















APP_NAME = "SPLINED"
VERSION = "2.0.6"
CONFIG_VERSION = 5


def display_version() -> str:
    """Return the user-visible version without changing release SemVer authority."""
    channel = os.environ.get("SPLINED_BUILD_CHANNEL", "release").strip().casefold()
    return f"{VERSION}-dev" if channel == "dev" else VERSION
DEFAULT_CONFIG = Path("/config/config.toml")
HELP_COLUMN_WIDTH = 38


def config_path() -> Path:
    raw = os.environ.get("SPLINED_CONFIG", str(DEFAULT_CONFIG)).strip()
    if not raw:
        raise SplinedError("SPLINED_CONFIG cannot be empty.")
    p = Path(raw)
    return p if p.is_absolute() else Path("/") / p


def validate_filename(name: str) -> None:
    name = name.strip()
    if not name:
        raise SplinedError("SPLINED output file_name cannot be empty.")
    if name in {".", ".."} or "/" in name or "\\" in name:
        raise SplinedError("SPLINED output file_name must be a single filename stem without directories.")
    if Path(name).suffix:
        raise SplinedError("SPLINED output file_name must not include an extension; use output.file_formats instead.")


def migrate_v4_config(cfg: dict[str, Any]) -> dict[str, Any]:
    """Load a Config v4 document with Config v5 defaults without losing data.

    Python/Docker historically had no TOML writer dependency, so compatibility
    migration is deliberately in-memory. Public examples and all newly written
    configurations are Config v5; existing v4 mounts remain usable while an
    administrator replaces the file with the v5 example.
    """
    migrated = json.loads(json.dumps(cfg))
    migrated["config_version"] = CONFIG_VERSION
    migrated.setdefault("source_policies", {})
    migrated.setdefault("logging", {"retention_days": 14})
    migrated.setdefault("history", {"enabled": True, "retention_days": 0})
    if "aisplined" not in migrated and "splineai" not in migrated:
        migrated["aisplined"] = {
            "enabled": False,
            "endpoint": "",
            "minimum_short_side": 600,
            "allow_below_minimum_override": False,
        }
    migrated.setdefault("credentials", {}).setdefault(
        "credential_dir",
        str(DEFAULT_CREDENTIAL_DIR),
    )
    return migrated


def _non_negative_int(value: Any, label: str) -> int:
    if isinstance(value, bool):
        raise SplinedError(f"{label} must be a non-negative integer.")
    try:
        parsed = int(value)
    except (TypeError, ValueError) as exc:
        raise SplinedError(f"{label} must be a non-negative integer.") from exc
    if parsed < 0:
        raise SplinedError(f"{label} cannot be negative.")
    return parsed


def _validate_aisplined_table(value: Any, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise SplinedError(f"{label} must be a TOML table.")
    enabled = _bool_value(value.get("enabled", False), f"{label}.enabled")
    endpoint = value.get("endpoint", "")
    if not isinstance(endpoint, str):
        raise SplinedError(f"{label}.endpoint must be a string.")
    minimum_short_side = _optional_positive_int(
        value.get("minimum_short_side", 600),
        f"{label}.minimum_short_side",
    )
    # The default above is non-null; keep the assertion explicit so a future
    # refactor cannot silently turn this required policy floor into None.
    if minimum_short_side is None:
        raise SplinedError(f"{label}.minimum_short_side must be greater than zero.")
    allow_override = _bool_value(
        value.get("allow_below_minimum_override", False),
        f"{label}.allow_below_minimum_override",
    )
    return {
        "enabled": enabled,
        "endpoint": endpoint,
        "minimum_short_side": minimum_short_side,
        "allow_below_minimum_override": allow_override,
    }


def aisplined_settings(cfg: dict[str, Any]) -> dict[str, Any]:
    """Resolve the canonical Config v5 companion boundary.

    ``[splineai]`` is accepted only as a compatibility alias.  The two tables
    are never merged; conflicting values are rejected instead of guessed.
    """
    canonical_present = "aisplined" in cfg
    legacy_present = "splineai" in cfg
    canonical = (
        _validate_aisplined_table(cfg["aisplined"], "[aisplined]")
        if canonical_present
        else None
    )
    legacy = (
        _validate_aisplined_table(cfg["splineai"], "[splineai]")
        if legacy_present
        else None
    )
    if canonical is not None and legacy is not None and canonical != legacy:
        raise SplinedError(
            "[aisplined] and legacy [splineai] disagree; remove the legacy "
            "table or make all integration policy fields identical."
        )
    if canonical is not None:
        return canonical
    if legacy is not None:
        return legacy
    return {
        "enabled": False,
        "endpoint": "",
        "minimum_short_side": 600,
        "allow_below_minimum_override": False,
    }


def validate_config_v5(cfg: dict[str, Any]) -> None:
    logging_verbosity(cfg)
    mode = str(cfg.get("mode", "read")).strip().lower()
    if mode not in {"read", "write"}:
        raise SplinedError("SPLINED mode must be 'read' or 'write'.")

    credentials = section(cfg, "credentials")
    if not str(credentials.get("credential_dir", "")).strip():
        raise SplinedError("[credentials].credential_dir cannot be empty.")

    ranges = section(cfg, "range")
    try:
        minimum = int(ranges.get("min", 1200))
        ideal = int(ranges.get("ideal", 1800))
        maximum = int(ranges.get("max", 2400))
        ladder = int(ranges.get("ladder", 3600))
    except (TypeError, ValueError) as exc:
        raise SplinedError("[range] values must be integers.") from exc
    if minimum >= ideal:
        raise SplinedError("[range].min must be below [range].ideal.")
    if ideal > maximum:
        raise SplinedError("[range].ideal cannot exceed [range].max.")
    if maximum >= ladder:
        raise SplinedError("[range].max must be below [range].ladder.")

    normalize_sources(
        section(cfg, "sources").get("cover_sources", []),
        "configured cover_sources",
        False,
    )
    normalize_sources(
        section(cfg, "sources").get("exclude_cover_sources", []),
        "configured exclude_cover_sources",
        True,
    )

    policies = section(cfg, "source_policies")
    for raw_source in policies:
        source_name = str(raw_source).strip().lower()
        if source_name not in SUPPORTED_SOURCE_POLICIES:
            raise SplinedError(f"Unsupported SPLINED source policy provider: {raw_source}")
        source_policy(cfg, source_name)

    logging = section(cfg, "logging")
    _non_negative_int(logging.get("retention_days", 14), "[logging].retention_days")
    history = section(cfg, "history")
    _bool_value(history.get("enabled", True), "[history].enabled")
    _non_negative_int(history.get("retention_days", 0), "[history].retention_days")
    _bool_value(section(cfg, "samples").get("sample_write", True), "[samples].sample_write")
    aisplined_settings(cfg)

    formats(cfg)
    output = section(cfg, "output")
    validate_filename(str(output.get("file_name", "cover")))
    square_mode = str(
        output.get("square_mode", "crop" if output.get("square", False) else "off")
    ).strip().lower()
    if square_mode not in {"off", "crop"}:
        raise SplinedError("[output].square_mode must be 'off' or 'crop'.")
    square_round_to = _non_negative_int(
        output.get("square_round_to", 0) or 0,
        "[output].square_round_to",
    )
    if square_round_to < 0:
        raise SplinedError("[output].square_round_to cannot be negative.")
    upscale_max_percent = _non_negative_int(
        output.get("upscale_max_percent", 200) or 0,
        "[output].upscale_max_percent",
    )
    if not 100 <= upscale_max_percent <= 800:
        raise SplinedError(
            "[output].upscale_max_percent must be between 100 and 800."
        )
    scan_timeout_hours(cfg)


def load_config() -> tuple[Path, dict[str, Any]]:
    path = config_path()
    try:
        with path.open("rb") as handle:
            cfg = tomllib.load(handle)
    except FileNotFoundError as exc:
        raise SplinedError(f"Configuration file not found: {path}") from exc
    except IsADirectoryError as exc:
        raise SplinedError(f"Configuration path is a directory, not a file: {path}") from exc
    except tomllib.TOMLDecodeError as exc:
        raise SplinedError(f"Invalid TOML in {path}: {exc}") from exc
    if cfg.get("config_version") == 4:
        cfg = migrate_v4_config(cfg)
    elif cfg.get("config_version") != CONFIG_VERSION:
        raise SplinedError(f"Unsupported config_version {cfg.get('config_version')!r}; expected {CONFIG_VERSION}.")
    validate_config_v5(cfg)
    return path, cfg


def parse_sources(value: str | None) -> list[str] | None:
    if value is None: return None
    return [x.strip() for x in value.split(",") if x.strip()]





def colors_enabled() -> bool:
    if os.environ.get("NO_COLOR"):
        return False
    if os.environ.get("SPLINED_COLOR", "").strip().lower() in {"1", "true", "yes", "always"}:
        return True
    return sys.stdout.isatty()


def rgb(text: str, r: int, g: int, b: int) -> str:
    if not colors_enabled():
        return text
    return f"\033[38;2;{r};{g};{b}m{text}\033[0m"


def bold(text: str) -> str:
    if not colors_enabled():
        return text
    return f"\033[1m{text}\033[0m"


OLED_RGB: dict[str, tuple[int, int, int]] = {
    "RED": (255, 70, 95),
    "LIGHT_RED": (255, 118, 140),
    "DARK_RED": (190, 35, 60),
    "GREEN": (60, 255, 135),
    "LIGHT_GREEN": (118, 255, 180),
    "DARK_GREEN": (0, 175, 90),
    "LIME": (160, 255, 60),
    "LIGHT_LIME": (195, 255, 110),
    "DARK_LIME": (100, 190, 0),
    "YELLOW": (255, 235, 60),
    "LIGHT_YELLOW": (255, 245, 125),
    "DARK_YELLOW": (190, 160, 0),
    "GOLD": (255, 200, 50),
    "LIGHT_GOLD": (255, 225, 110),
    "DARK_GOLD": (185, 130, 0),
    "ORANGE": (255, 145, 35),
    "LIGHT_ORANGE": (255, 185, 95),
    "DARK_ORANGE": (190, 95, 0),
    "BLUE": (70, 135, 255),
    "LIGHT_BLUE": (115, 180, 255),
    "DARK_BLUE": (20, 85, 210),
    "SKY_BLUE": (80, 170, 255),
    "LIGHT_SKY_BLUE": (135, 205, 255),
    "DARK_SKY_BLUE": (30, 120, 215),
    "CYAN": (55, 225, 255),
    "LIGHT_CYAN": (120, 240, 255),
    "DARK_CYAN": (0, 170, 205),
    "TEAL": (0, 210, 180),
    "LIGHT_TEAL": (85, 230, 205),
    "DARK_TEAL": (0, 145, 125),
    "TURQUOISE": (45, 235, 200),
    "LIGHT_TURQUOISE": (110, 245, 220),
    "DARK_TURQUOISE": (0, 175, 145),
    "MAGENTA": (255, 80, 220),
    "LIGHT_MAGENTA": (255, 135, 235),
    "DARK_MAGENTA": (190, 30, 170),
    "PINK": (255, 120, 200),
    "LIGHT_PINK": (255, 165, 220),
    "DARK_PINK": (200, 70, 150),
    "HOT_PINK": (255, 70, 170),
    "LIGHT_HOT_PINK": (255, 120, 195),
    "DARK_HOT_PINK": (195, 20, 110),
    "PURPLE": (175, 95, 255),
    "LIGHT_PURPLE": (205, 150, 255),
    "DARK_PURPLE": (115, 40, 205),
    "VIOLET": (145, 105, 255),
    "LIGHT_VIOLET": (180, 145, 255),
    "DARK_VIOLET": (95, 55, 215),
    "INDIGO": (105, 110, 255),
    "LIGHT_INDIGO": (145, 150, 255),
    "DARK_INDIGO": (55, 60, 210),
    "BROWN": (185, 120, 70),
    "LIGHT_BROWN": (210, 155, 110),
    "DARK_BROWN": (120, 75, 35),
    "TAN": (215, 180, 120),
    "LIGHT_TAN": (230, 205, 160),
    "DARK_TAN": (170, 130, 80),
    "GRAY": (145, 150, 165),
    "LIGHT_GRAY": (200, 205, 215),
    "DARK_GRAY": (105, 110, 125),
    "WHITE": (225, 230, 235),
}


def paint(name: str, text: str) -> str:
    rgb_tuple = OLED_RGB[name]
    return rgb(text, *rgb_tuple)


def green(s: str) -> str: return paint("GREEN", s)
def red(s: str) -> str: return paint("RED", s)
def yellow(s: str) -> str: return paint("YELLOW", s)
def blue(s: str) -> str: return paint("BLUE", s)
def magenta(s: str) -> str: return paint("MAGENTA", s)
def cyan(s: str) -> str: return paint("CYAN", s)
def orange(s: str) -> str: return paint("ORANGE", s)
def gray(s: str) -> str: return paint("GRAY", s)
def white(s: str) -> str: return paint("WHITE", s)
def purple(s: str) -> str: return paint("PURPLE", s)


def bool_text(value: bool) -> str:
    return "true" if value else "false"


def bool_color(value: bool) -> str:
    return green(bool_text(value)) if value else red(bool_text(value))


ANSI_RE = re.compile(r"\x1b\[[0-9;]*m")


def visible_len(value: str) -> int:
    return len(ANSI_RE.sub("", value))


def ljust_color(value: str, width: int) -> str:
    return value + (" " * max(0, width - visible_len(value)))


def rjust_color(value: str, width: int) -> str:
    return (" " * max(0, width - visible_len(value))) + value


def provider_label(source: str) -> str:
    mapping = {
        "deezer": "Deezer",
        "itunes": "iTunes",
        "fanarttv": "FanartTV",
        "lastfm": "LastFM",
        "coverartarchive": "CoverArt",
        "discogs": "Discogs",
        "musicbrainz": "MB / CAA",
        "amazon": "Amazon",
        "navidrome": "Navidrome",
    }
    value = mapping.get(source.lower(), source.title())
    return value[:8]


def color_range_type(value: str) -> str:
    if value == "BelowMinimum":
        return red(value)
    if value == "LowerRange":
        return orange(value)
    if value == "Ideal":
        return green(value)
    if value == "UpperRange":
        return yellow(value)
    if value == "Ladder":
        return magenta(value)
    if value == "AboveLadder":
        return purple(value)
    return white(value)


def bracketed_list(values: list[str], item_formatter=green) -> str:
    inner = white(", ").join(item_formatter(v) for v in values)
    return white("[") + inner + white("]")


def bracketed_text(value: str, formatter=white) -> str:
    return white("[") + formatter(value) + white("]")


def terminal_hyperlink(label: str, url: str) -> str:
    return f"{label} ({url})"


def format_source_url(url: str) -> str:
    return white("[") + terminal_hyperlink(blue("URL"), url) + white("]")


def album_path_text(path: Path) -> str:
    parts = list(path.parts)
    if not parts:
        return str(path)
    if path.is_absolute():
        body = list(path.parts[1:])
        if body:
            body[-1] = orange(body[-1])
            return path.anchor + "/".join(body)
        return str(path)
    parts[-1] = orange(parts[-1])
    return "/".join(parts)
