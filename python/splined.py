#!/usr/bin/env python3
from __future__ import annotations

import argparse
import base64
import getpass
import fnmatch
import hashlib
import io
import json
import os
import secrets
import re
import shutil
import signal
import sys
import tempfile
import time
import tomllib
from urllib.parse import urlencode
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import requests
from mutagen import File as MutagenFile
from mutagen.flac import FLAC
from mutagen.id3 import ID3
from mutagen.mp4 import MP4
from PIL import Image

USER_AGENT = "SPLINED/1.0.2 (https://github.com/scottia/S-P-L-I-N-E-D)"
MB_BASE = "https://musicbrainz.org/ws/2"
MB_AUTHORIZE_URL = "https://musicbrainz.org/oauth2/authorize"
MB_TOKEN_URL = "https://musicbrainz.org/oauth2/token"
LASTFM_API_URL = "https://ws.audioscrobbler.com/2.0/"
LASTFM_AUTH_URL = "https://www.last.fm/api/auth/"
REQUEST_TIMEOUT = 20
AUDIO_EXTENSIONS = {".mp3", ".flac", ".m4a", ".mp4", ".ogg", ".opus", ".wav", ".aiff", ".aif"}
SUPPORTED_SOURCES = ("deezer", "itunes", "fanarttv", "lastfm", "coverartarchive", "discogs")
EXTENSIONS = {"jpeg": "jpg", "png": "png", "webp": "webp"}


class SplinedError(RuntimeError):
    pass


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


@dataclass
class AlbumDir:
    path: Path
    audio_files: list[Path]


@dataclass
class Release:
    mbid: str
    title: str
    artist_credit: str
    release_group_id: str | None
    release_group_title: str | None
    track_count: int | None = None
    external_urls: list[str] = field(default_factory=list)


@dataclass
class Ref:
    source: str
    id: str
    url: str
    front: bool = True
    approved: bool = True
    types: list[str] = field(default_factory=lambda: ["Front"])


@dataclass
class Candidate:
    ref: Ref
    path: Path
    width: int
    height: int
    format: str
    source_priority: int

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
    return [s for s in base if s not in configured_exclude and s not in cli_exclude]


SOURCE_HISTORY_VERSION = 1
SOURCE_HISTORY_FILE = "chosen-source-history.json"


def source_history_path(cache: Path) -> Path:
    return cache / "history" / SOURCE_HISTORY_FILE


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
    if not path.exists():
        return history
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        # History is advisory optimization data. A damaged history file must
        # never block artwork discovery.
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
    # Best effort by design: history must never make a scan fail.
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        body = (json.dumps(history, indent=2, sort_keys=True) + "\n").encode("utf-8")
        atomic_write(path, body)
    except Exception:
        pass


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
SCAN_COMPLETION_HISTORY_FILE = "scan-completed-history.json"


def scan_completion_history_path(cache: Path) -> Path:
    return cache / "history" / SCAN_COMPLETION_HISTORY_FILE


def empty_scan_completion_history() -> dict[str, Any]:
    return {
        "version": SCAN_COMPLETION_HISTORY_VERSION,
        "albums": {},
    }


def load_scan_completion_history(path: Path) -> dict[str, Any]:
    history = empty_scan_completion_history()
    if not path.exists():
        return history
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
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
    return history


def save_scan_completion_history(path: Path, history: dict[str, Any]) -> None:
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        history["version"] = SCAN_COMPLETION_HISTORY_VERSION
        body = (json.dumps(history, indent=2, sort_keys=True) + "\n").encode("utf-8")
        atomic_write(path, body)
    except Exception:
        # Scan timeout state is an optimization. It must never make a scan fail.
        pass


def scan_timeout_hours(cfg: dict[str, Any]) -> float:
    raw = section(cfg, "scan").get("scan_mode_timeout", 24)
    if isinstance(raw, bool):
        raise SplinedError("[scan].scan_mode_timeout must be a non-negative number of hours.")
    try:
        value = float(raw)
    except (TypeError, ValueError) as exc:
        raise SplinedError(
            "[scan].scan_mode_timeout must be a non-negative number of hours."
        ) from exc
    if value < 0:
        raise SplinedError("[scan].scan_mode_timeout cannot be negative.")
    return value


def format_timeout_hours(value: float) -> str:
    return str(int(value)) if float(value).is_integer() else f"{value:g}"


def album_scan_fingerprint(album: AlbumDir) -> str:
    digest = hashlib.sha256()
    for path in sorted(album.audio_files, key=lambda item: str(item).lower()):
        try:
            stat = path.stat()
            payload = (
                f"{path.name}\0{stat.st_size}\0{stat.st_mtime_ns}\0"
            ).encode("utf-8", "surrogateescape")
        except OSError:
            payload = f"{path.name}\0unstatable\0".encode(
                "utf-8", "surrogateescape"
            )
        digest.update(payload)
    return digest.hexdigest()


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
) -> tuple[bool, float]:
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

    if entry.get("album_fingerprint") != album_scan_fingerprint(album):
        return False, age_seconds / 3600

    if entry.get("policy_fingerprint") != scan_policy_fingerprint(cfg, sources):
        return False, age_seconds / 3600

    return True, age_seconds / 3600


def record_scan_completion(
    path: Path,
    history: dict[str, Any],
    album: AlbumDir,
    cfg: dict[str, Any],
    sources: list[str],
    outcome: str,
) -> None:
    albums = history.setdefault("albums", {})
    albums[str(album.path)] = {
        "completed_at_unix": time.time(),
        "album_fingerprint": album_scan_fingerprint(album),
        "policy_fingerprint": scan_policy_fingerprint(cfg, sources),
        "outcome": str(outcome),
    }
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
    name = name.lower()
    return any(fnmatch.fnmatchcase(name, p.strip().lower()) for p in patterns if p.strip())


def inventory(root: Path, ignored_subs: list[str]) -> tuple[list[AlbumDir], list[Path]]:
    if not root.exists():
        raise SplinedError(f"SPLINED scan directory does not exist: {root}")
    if not root.is_dir():
        raise SplinedError(f"SPLINED scan path is not a directory: {root}")
    albums: list[AlbumDir] = []
    ignored: list[Path] = []

    def visit(directory: Path, is_root: bool) -> None:
        if not is_root and should_ignore(directory.name, ignored_subs):
            ignored.append(directory)
            return
        try:
            entries = sorted(directory.iterdir(), key=lambda p: p.name.lower())
        except OSError as exc:
            raise SplinedError(f"Unable to read SPLINED scan directory {directory}: {exc}") from exc
        audio: list[Path] = []
        children: list[Path] = []
        for p in entries:
            if p.is_symlink():
                continue
            if p.is_dir():
                children.append(p)
            elif p.is_file() and p.suffix.lower() in AUDIO_EXTENSIONS:
                audio.append(p)
        if audio:
            albums.append(AlbumDir(directory, sorted(audio)))
        for child in children:
            visit(child, False)

    visit(root, True)
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


def read_track(path: Path) -> Track:
    suffix = path.suffix.lower()
    title = artist = album = album_artist = album_mbid = recording_mbid = compilation = None
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
                elif desc in {"compilation", "itunescompilation"}: compilation = val
        elif suffix == ".flac":
            f = FLAC(path)
            title = first(f.get("title")); artist = first(f.get("artist")); album = first(f.get("album")); album_artist = first(f.get("albumartist"))
            album_mbid = first(f.get("musicbrainz_albumid")); recording_mbid = first(f.get("musicbrainz_trackid")); compilation = first(f.get("compilation"))
        elif suffix in {".m4a", ".mp4"}:
            tags = MP4(path).tags or {}
            title = first(tags.get("\xa9nam")); artist = first(tags.get("\xa9ART")); album = first(tags.get("\xa9alb")); album_artist = first(tags.get("aART"))
            album_mbid = first(tags.get("----:com.apple.iTunes:MusicBrainz Album Id")); recording_mbid = first(tags.get("----:com.apple.iTunes:MusicBrainz Track Id"))
            cpil = tags.get("cpil"); compilation = "1" if cpil and bool(cpil[0]) else None
        else:
            f = MutagenFile(path, easy=True)
            tags = getattr(f, "tags", None) if f is not None else None
            if tags:
                title = first(tags.get("title")); artist = first(tags.get("artist")); album = first(tags.get("album")); album_artist = first(tags.get("albumartist"))
                album_mbid = first(tags.get("musicbrainz_albumid")); recording_mbid = first(tags.get("musicbrainz_trackid")); compilation = first(tags.get("compilation"))
    except Exception as exc:
        raise SplinedError(f"Unable to read SPLINED audio tags from {path}: {exc}") from exc
    if not title:
        raise SplinedError(f"SPLINED scan requires TITLE in actual file tags: {path}")
    if not artist:
        raise SplinedError(f"SPLINED scan requires ARTIST in actual file tags: {path}")
    return Track(path, title, artist, album, album_artist, album_mbid, recording_mbid, compilation)


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


def compact(paths: list[Path]) -> str:
    names = [p.name for p in paths[:5]]
    if len(paths) > 5: names.append(f"+{len(paths)-5} more")
    return ", ".join(names)


_DEBUG_PATH: Path | None = None
_DEBUG_ENABLED = False


def init_debug_log(config_file: Path, cfg: dict[str, Any]) -> Path | None:
    global _DEBUG_PATH, _DEBUG_ENABLED

    verbosity = str(cfg.get("verbosity", "info")).strip().lower()
    _DEBUG_ENABLED = verbosity == "debug"
    if not _DEBUG_ENABLED:
        _DEBUG_PATH = None
        return None

    scan = section(cfg, "scan")
    cache = resolve_path(config_file, str(scan.get("cache_dir", "cache")))
    debug_dir = cache / "debug"
    debug_dir.mkdir(parents=True, exist_ok=True)
    _DEBUG_PATH = debug_dir / "splined_debug.log"

    # Append across runs with a visible session delimiter.
    with _DEBUG_PATH.open("a", encoding="utf-8") as handle:
        handle.write(
            f"\n=== SPLINED {VERSION} DEBUG SESSION "
            f"{time.strftime('%Y-%m-%d %H:%M:%S')} ===\n"
        )
    return _DEBUG_PATH


def debug_log(message: str) -> None:
    if not _DEBUG_ENABLED or _DEBUG_PATH is None:
        return
    safe = str(message).replace("\r", "\\r").replace("\n", "\\n")
    try:
        with _DEBUG_PATH.open("a", encoding="utf-8") as handle:
            handle.write(f"{time.strftime('%H:%M:%S')} {safe}\n")
    except OSError:
        # Debug output is diagnostic only and must never break a scan.
        pass



class Http:
    def __init__(self):
        self.s = requests.Session()
        self.s.headers.update({"User-Agent": USER_AGENT})
        self.last_mb_request: float | None = None

    def get(self, url: str, **kwargs: Any) -> requests.Response:
        kwargs.setdefault("timeout", REQUEST_TIMEOUT)
        return self.s.get(url, **kwargs)


def credential_file(config_file: Path, cfg: dict[str, Any], provider: str) -> Path:
    cdir = resolve_path(config_file, str(section(cfg, "credentials").get("credential_dir", "credentials")))
    pcfg = section(cfg, provider)
    key = "token_file" if provider == "musicbrainz" else "credential_file"
    default = "musicbrainz.json" if provider == "musicbrainz" else f"{provider}.json"
    raw = str(pcfg.get(key, default)).strip() or default
    p = Path(raw)
    return p if p.is_absolute() else cdir / p


def load_json(path: Path, label: str) -> dict[str, Any]:
    try: data = json.loads(path.read_text(encoding="utf-8"))
    except Exception as exc: raise SplinedError(f"Unable to read {label} credential file {path}: {exc}") from exc
    if not isinstance(data, dict): raise SplinedError(f"Invalid {label} credential file {path}")
    return data



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


def confirm_replace(path: Path) -> bool:
    if not path.exists():
        return True
    print("Credential file already exists:")
    print(cyan(str(path)))
    answer = input("Replace stored credentials? [y/N]: ").strip().lower()
    return answer in {"y", "yes"}


def _pkce_pair() -> tuple[str, str]:
    verifier = secrets.token_urlsafe(48)
    digest = hashlib.sha256(verifier.encode("ascii")).digest()
    challenge = base64.urlsafe_b64encode(digest).rstrip(b"=").decode("ascii")
    return verifier, challenge


def _mb_refresh_token(config_file: Path, cfg: dict[str, Any], cred: dict[str, Any]) -> dict[str, Any]:
    mb = section(cfg, "musicbrainz")
    client_id = str(mb.get("client_id") or "").strip()
    client_secret = str(cred.get("client_secret") or "").strip()
    refresh_token = str(cred.get("refresh_token") or "").strip()
    if not client_id or not client_secret or not refresh_token:
        return cred

    response = requests.post(
        MB_TOKEN_URL,
        data={
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": client_id,
            "client_secret": client_secret,
        },
        headers={"User-Agent": USER_AGENT},
        timeout=REQUEST_TIMEOUT,
    )
    if not response.ok:
        raise SplinedError(
            f"MusicBrainz OAuth refresh returned HTTP {response.status_code}: "
            f"{response.text[:300].strip()}"
        )
    token = response.json()
    now = int(time.time())
    updated = dict(cred)
    updated["access_token"] = str(token.get("access_token") or "").strip() or None
    updated["refresh_token"] = str(token.get("refresh_token") or refresh_token).strip() or None
    updated["token_type"] = str(token.get("token_type") or "Bearer").strip() or "Bearer"
    updated["scope"] = str(token.get("scope") or updated.get("scope") or "").strip() or None
    expires_in = int(token.get("expires_in") or 0)
    updated["expires_at_unix"] = now + expires_in if expires_in else None
    path = credential_file(config_file, cfg, "musicbrainz")
    save_json_atomic(path, updated)
    return updated


def mb_headers(config_file: Path, cfg: dict[str, Any]) -> tuple[dict[str, str], str]:
    mb = section(cfg, "musicbrainz")
    if not bool(mb.get("oauth", False)):
        return {}, "Anonymous"

    path = credential_file(config_file, cfg, "musicbrainz")
    cred = load_json(path, "MusicBrainz")
    token = str(cred.get("access_token") or "").strip()
    expires_at = cred.get("expires_at_unix")
    if expires_at is not None:
        try:
            if int(expires_at) <= int(time.time()) + 30:
                cred = _mb_refresh_token(config_file, cfg, cred)
                token = str(cred.get("access_token") or "").strip()
        except (TypeError, ValueError):
            pass

    if not token:
        if str(cred.get("refresh_token") or "").strip():
            cred = _mb_refresh_token(config_file, cfg, cred)
            token = str(cred.get("access_token") or "").strip()
    if not token:
        raise SplinedError(
            f"MusicBrainz OAuth enabled but no usable access_token exists in {path}. "
            "Run --mb-oauth-login."
        )
    return {"Authorization": f"Bearer {token}"}, "OAuthBearer"


def configure_fanarttv_credentials(config_file: Path, cfg: dict[str, Any]) -> int:
    path = credential_file(config_file, cfg, "fanarttv")
    print(cyan("SPLINED Fanart.tv Credentials"))
    print()
    print("Credential file:")
    print(cyan(str(path)))
    print()
    if not confirm_replace(path):
        print("Fanart.tv credential update cancelled.")
        return 0
    api_key = getpass.getpass("Fanart.tv API key: ").strip()
    if not api_key:
        raise SplinedError("Fanart.tv API key cannot be empty.")
    client_key = getpass.getpass("Fanart.tv client key (optional): ").strip()
    save_json_atomic(path, {"api_key": api_key, "client_key": client_key})
    credential_created_notice(path)
    return 0


def configure_lastfm_credentials(config_file: Path, cfg: dict[str, Any]) -> int:
    path = credential_file(config_file, cfg, "lastfm")
    print(cyan("SPLINED Last.fm Credentials"))
    print()
    print("Credential file:")
    print(cyan(str(path)))
    print()
    if not confirm_replace(path):
        print("Last.fm credential update cancelled.")
        return 0
    api_key = getpass.getpass("Last.fm API key: ").strip()
    if not api_key:
        raise SplinedError("Last.fm API key cannot be empty.")
    shared_secret = getpass.getpass("Last.fm shared secret: ").strip()
    if not shared_secret:
        raise SplinedError("Last.fm shared secret cannot be empty.")
    current = {}
    if path.exists():
        try:
            current = load_json(path, "Last.fm")
        except SplinedError:
            current = {}
    data = {
        "api_key": api_key,
        "shared_secret": shared_secret,
        "username": str(current.get("username") or ""),
        "session_key": str(current.get("session_key") or ""),
        "subscriber": bool(current.get("subscriber", False)),
    }
    save_json_atomic(path, data)
    credential_created_notice(path)
    print("Run --lastfm-login to authorize your Last.fm account.")
    return 0


def lastfm_signature(params: dict[str, str], secret: str) -> str:
    material = "".join(k + params[k] for k in sorted(params)) + secret
    return hashlib.md5(material.encode("utf-8")).hexdigest()


def run_lastfm_login(config_file: Path, cfg: dict[str, Any]) -> int:
    path = credential_file(config_file, cfg, "lastfm")
    cred = load_json(path, "Last.fm")
    api_key = str(cred.get("api_key") or "").strip()
    secret = str(cred.get("shared_secret") or "").strip()
    if not api_key or not secret:
        raise SplinedError("Last.fm API key/shared secret missing. Run --lastfm-credentials first.")

    token_params = {"api_key": api_key, "method": "auth.getToken"}
    token_params["api_sig"] = lastfm_signature({"api_key": api_key, "method": "auth.getToken"}, secret)
    token_params["format"] = "json"
    r = requests.get(LASTFM_API_URL, params=token_params, headers={"User-Agent": USER_AGENT}, timeout=REQUEST_TIMEOUT)
    data = r.json()
    if data.get("error"):
        raise SplinedError(f"Last.fm authorization token request failed with API error {data.get('error')}: {data.get('message')}")
    token = str(data.get("token") or "").strip()
    if not token:
        raise SplinedError("Last.fm authorization token response contained no token.")

    auth_url = LASTFM_AUTH_URL + "?" + urlencode({"api_key": api_key, "token": token})
    print(cyan("SPLINED Last.fm Login"))
    print()
    print("Open this URL in your browser:")
    print()
    print(blue(auth_url))
    print()
    input("Authorize SPLINED with Last.fm, then press Enter to continue...")

    session_base = {"api_key": api_key, "method": "auth.getSession", "token": token}
    params = dict(session_base)
    params["api_sig"] = lastfm_signature(session_base, secret)
    params["format"] = "json"
    r = requests.get(LASTFM_API_URL, params=params, headers={"User-Agent": USER_AGENT}, timeout=REQUEST_TIMEOUT)
    data = r.json()
    if data.get("error"):
        raise SplinedError(f"Last.fm session creation failed with API error {data.get('error')}: {data.get('message')}")
    session = data.get("session") or {}
    username = str(session.get("name") or "").strip()
    session_key = str(session.get("key") or "").strip()
    if not username or not session_key:
        raise SplinedError("Last.fm session response was incomplete.")
    cred["username"] = username
    cred["session_key"] = session_key
    cred["subscriber"] = bool(int(session.get("subscriber") or 0))
    save_json_atomic(path, cred)
    print(green("Last.fm authorization successful."))
    print(f"User: {green(username)}")
    print(f"Subscriber: {'yes' if cred['subscriber'] else 'no'}")
    return 0


def run_musicbrainz_login(config_file: Path, cfg: dict[str, Any]) -> int:
    mb = section(cfg, "musicbrainz")
    if not bool(mb.get("oauth", False)):
        raise SplinedError("MusicBrainz OAuth is disabled. Set [musicbrainz] oauth = true first.")

    client_id = str(mb.get("client_id") or "").strip()
    callback_uri = str(mb.get("callback_uri") or "urn:ietf:wg:oauth:2.0:oob").strip()
    scope = str(mb.get("scope") or "profile").strip()
    if not client_id:
        raise SplinedError("MusicBrainz OAuth client_id is empty in config.toml.")

    path = credential_file(config_file, cfg, "musicbrainz")
    current: dict[str, Any] = {}
    if path.exists():
        current = load_json(path, "MusicBrainz")
    client_secret = str(current.get("client_secret") or "").strip()
    if not client_secret:
        client_secret = getpass.getpass("MusicBrainz client secret: ").strip()
        if not client_secret:
            raise SplinedError("MusicBrainz client secret cannot be empty.")

    verifier, challenge = _pkce_pair()
    state = secrets.token_urlsafe(24)
    auth_params = {
        "response_type": "code",
        "client_id": client_id,
        "redirect_uri": callback_uri,
        "scope": scope,
        "state": state,
        "code_challenge": challenge,
        "code_challenge_method": "S256",
    }
    auth_url = MB_AUTHORIZE_URL + "?" + urlencode(auth_params)
    print(cyan("SPLINED MusicBrainz OAuth Login"))
    print()
    print("Open this URL in your browser:")
    print()
    print(blue(auth_url))
    print()
    code = input("Authorize SPLINED, then paste the MusicBrainz authorization code: ").strip()
    if not code:
        raise SplinedError("MusicBrainz authorization code was empty.")

    r = requests.post(
        MB_TOKEN_URL,
        data={
            "grant_type": "authorization_code",
            "code": code,
            "client_id": client_id,
            "client_secret": client_secret,
            "redirect_uri": callback_uri,
            "code_verifier": verifier,
        },
        headers={"User-Agent": USER_AGENT},
        timeout=REQUEST_TIMEOUT,
    )
    if not r.ok:
        raise SplinedError(
            f"MusicBrainz OAuth authorization-code exchange returned HTTP {r.status_code}: "
            f"{r.text[:300].strip()}"
        )
    token = r.json()
    expires_in = int(token.get("expires_in") or 0)
    data = {
        "client_secret": client_secret,
        "access_token": str(token.get("access_token") or "").strip() or None,
        "refresh_token": str(token.get("refresh_token") or "").strip() or None,
        "token_type": str(token.get("token_type") or "Bearer").strip() or "Bearer",
        "expires_at_unix": int(time.time()) + expires_in if expires_in else None,
        "scope": str(token.get("scope") or scope).strip() or None,
    }
    if not data["access_token"]:
        raise SplinedError("MusicBrainz OAuth token response contained no access_token.")
    save_json_atomic(path, data)
    credential_created_notice(path)
    print(green("MusicBrainz OAuth authorization successful."))
    print("Request mode: OAuth Bearer")
    return 0



def lookup_release(http: Http, config_file: Path, cfg: dict[str, Any], mbid: str) -> Release:
    headers, _ = mb_headers(config_file, cfg)
    mbcfg = section(cfg, "musicbrainz")
    timeout = float(mbcfg.get("mb_recording_timeout", 7.0))
    retry_max = int(mbcfg.get("retry_max", 2))
    min_delay = max(0.0, float(mbcfg.get("mb_min_delay", 1.05)))
    url = f"{MB_BASE}/release/{mbid}"

    for attempt in range(retry_max + 1):
        if http.last_mb_request is not None:
            wait = min_delay - (time.monotonic() - http.last_mb_request)
            if wait > 0:
                time.sleep(wait)

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

        return Release(
            mbid=mbid,
            title=str(d.get("title") or "").strip(),
            artist_credit="".join(parts).strip(),
            release_group_id=str(rg.get("id") or "").strip() or None,
            release_group_title=str(rg.get("title") or "").strip() or None,
            track_count=track_count if found_count else None,
            external_urls=external_urls,
        )

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
    mbcfg = section(cfg, "musicbrainz")
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

def discover_fanart(http: Http, config_file: Path, cfg: dict[str, Any], rel: Release) -> list[Ref]:
    if not rel.release_group_id: return []
    path = credential_file(config_file, cfg, "fanarttv"); cred = load_json(path, "Fanart.tv"); key = str(cred.get("api_key") or "").strip()
    if not key: raise SplinedError(f"Fanart.tv credential file contains no api_key: {path}")
    headers = {"api-key": key}; client = str(cred.get("client_key") or "").strip()
    if client: headers["client-key"] = client
    r = http.get(f"https://webservice.fanart.tv/v3/music/albums/{rel.release_group_id}", headers=headers)
    if r.status_code == 404: return []
    if r.status_code in {401, 403}: raise SplinedError("Fanart.tv credentials were rejected.")
    if r.status_code != 200: raise SplinedError(f"Fanart.tv returned HTTP {r.status_code}")
    return [Ref("fanarttv", str(x.get("id") or ""), str(x.get("url") or "")) for x in r.json().get("albumcover", []) if str(x.get("url") or "").strip()]


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
        image_pool = primary or [image for image in images if isinstance(image, dict)]

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
                    front=True,
                    approved=True,
                    types=["Front"],
                )
            )
            emitted = True

        # Search thumbnail is a last-resort candidate if release.images[] is empty.
        if not emitted:
            cover = str(result.get("cover_image") or "").strip()
            if cover:
                refs.append(Ref("discogs", release_id, cover, front=True, approved=True, types=["Front"]))

    return refs


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
    refs: list[Ref] = []
    diag: list[tuple[str, str]] = []

    for source in sources:
        try:
            if source == "deezer":
                if queried_sources is not None: queried_sources.add(source)
                refs += discover_deezer(http, synthetic)
            elif source == "itunes":
                if queried_sources is not None: queried_sources.add(source)
                refs += discover_itunes(http, config_file, cfg, synthetic)
            elif source == "lastfm":
                if queried_sources is not None: queried_sources.add(source)
                refs += discover_lastfm(http, config_file, cfg, synthetic)
            elif source == "discogs":
                if queried_sources is not None: queried_sources.add(source)
                refs += discover_discogs(http, config_file, cfg, artist, album)
            elif source == "coverartarchive" and release_mbid:
                if queried_sources is not None: queried_sources.add(source)
                refs += discover_caa(http, synthetic)
            elif source == "fanarttv":
                # Fanart.tv requires a release-group MBID. That is unavailable
                # until MusicBrainz authority is recovered.
                continue
        except Exception as exc:
            diag.append((source, str(exc)))
    return refs, diag



def discover_all(
    http: Http,
    config_file: Path,
    cfg: dict[str, Any],
    rel: Release,
    sources: list[str],
    queried_sources: set[str] | None = None,
) -> tuple[list[Ref], list[tuple[str, str]]]:
    refs: list[Ref] = []
    diag: list[tuple[str, str]] = []

    for source in sources:
        if queried_sources is not None:
            queried_sources.add(source)

        debug_log(
            f"provider.start source={source} mbid={rel.mbid} "
            f"artist={rel.artist_credit!r} album={rel.title!r}"
        )

        try:
            before = len(refs)

            if source == "deezer":
                refs += discover_deezer(http, rel)
            elif source == "itunes":
                refs += discover_itunes(http, config_file, cfg, rel)
            elif source == "fanarttv":
                refs += discover_fanart(http, config_file, cfg, rel)
            elif source == "lastfm":
                refs += discover_lastfm(http, config_file, cfg, rel)
            elif source == "coverartarchive":
                refs += discover_caa(http, rel)
            elif source == "discogs":
                refs += discover_discogs(
                    http, config_file, cfg, rel.artist_credit, rel.title
                )

            produced = len(refs) - before
            debug_log(f"provider.done source={source} refs={produced}")

        except Exception as exc:
            diag.append((source, str(exc)))
            debug_log(
                f"provider.error source={source} "
                f"error={type(exc).__name__}: {exc}"
            )

    return refs, diag



def image_format(image: Image.Image) -> str:
    f = (image.format or "").upper()
    if f == "JPEG": return "jpeg"
    if f == "PNG": return "png"
    if f == "WEBP": return "webp"
    raise SplinedError(f"Unsupported SPLINED downloaded artwork format: {f or 'unknown'}")


def clean_cache(cache: Path) -> None:
    cache.mkdir(parents=True, exist_ok=True)
    # Only ephemeral candidate files are removed. cache/history/ is persistent
    # and intentionally excluded from all per-run cleanup.
    for p in cache.iterdir():
        if p.name.startswith("splined-candidate-") and p.is_file() and not p.is_symlink():
            p.unlink()



def cache_extension_from_format(fmt: str) -> str:
    return EXTENSIONS.get(fmt, "bin")


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
    evaluate_final = bool(out.get("evaluate_final_image", True))

    return {
        "square": square,
        "square_mode": square_mode,
        "square_round_to": round_to,
        "upscale_below_ideal": upscale,
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
    w, h = width, height

    operations = {
        "squared": False,
        "cropped": False,
        "square_mode": settings["square_mode"],
        "rounded": False,
        "rounded_to": 0,
        "resized": False,
        "upscaled": False,
    }

    # 1. Preserve aspect ratio and center-crop the long edge.
    if settings["square_mode"] == "crop":
        side = min(w, h)
        operations["squared"] = True
        operations["cropped"] = (w != h)
        w = side
        h = side

    # 2. Optional square rounding happens before range resize.
    #    1426x1414 -> crop 1414x1414 -> round 1408x1408.
    round_to = settings["square_round_to"]
    if settings["square"] and w == h and round_to > 1 and w >= round_to:
        rounded_side = (w // round_to) * round_to
        if rounded_side > 0 and rounded_side != w:
            w = rounded_side
            h = rounded_side
            operations["rounded"] = True
            operations["rounded_to"] = round_to

    # 3. Apply final range resize. Downscale above ideal; never upscale unless enabled.
    short_side = min(w, h)
    if short_side > ideal:
        scale = ideal / short_side
        w = max(1, int(round(w * scale)))
        h = max(1, int(round(h * scale)))
        operations["resized"] = True
    elif short_side < ideal and settings["upscale_below_ideal"]:
        scale = ideal / short_side
        w = max(1, int(round(w * scale)))
        h = max(1, int(round(h * scale)))
        operations["resized"] = True
        operations["upscaled"] = True

    return w, h, operations


def project_candidate(c: Candidate, cfg: dict[str, Any], target: str) -> dict[str, Any]:
    settings = output_settings(cfg)
    if settings["evaluate_final_image"]:
        width, height, ops = project_dimensions(c.width, c.height, cfg)
    else:
        width, height = c.width, c.height
        ops = {
            "squared": width == height,
            "cropped": False,
            "square_mode": "off",
            "rounded": False,
            "rounded_to": 0,
            "resized": False,
            "upscaled": False,
        }

    short_side = min(width, height)
    range_type = classify_range(short_side, cfg)
    ideal = int(section(cfg, "range").get("ideal", 1800))
    return {
        "width": width,
        "height": height,
        "short_side": short_side,
        "square": width == height,
        "format": target,
        "range_type": range_type,
        "distance": abs(short_side - ideal),
        "acceptable": range_type not in {"BelowMinimum", "AboveLadder"},
        "converted": c.format != target,
        "squared": bool(ops.get("squared", False)),
        "cropped": bool(ops.get("cropped", False)),
        "square_mode": str(ops.get("square_mode", "off")),
        "rounded": bool(ops.get("rounded", False)),
        "rounded_to": int(ops.get("rounded_to", 0) or 0),
        "resized": bool(ops.get("resized", False)),
        "upscaled": bool(ops.get("upscaled", False)),
    }

def target_format_for_candidate(c: Candidate, format_order: list[str]) -> str:
    return c.format if c.format in format_order else format_order[0]


def is_discogs_placeholder(ref: Ref) -> bool:
    if ref.source != "discogs":
        return False
    url = str(ref.url or "").strip().lower().split("?", 1)[0].split("#", 1)[0]
    return url.endswith("/images/spacer.gif") or url.endswith("/spacer.gif")



def download_candidates(
    http: Http,
    refs: list[Ref],
    sources: list[str],
    cache: Path,
    clean_first: bool = True,
) -> tuple[list[Candidate], list[tuple[str, str]]]:
    if clean_first:
        clean_cache(cache)
    out: list[Candidate] = []
    diag: list[tuple[str, str]] = []
    for ref in refs:
        if not ref.front or not ref.url:
            debug_log(f"download.skip source={ref.source} id={ref.id} reason=no-front-or-url")
            continue
        if is_discogs_placeholder(ref):
            debug_log(
                f"download.skip source={ref.source} id={ref.id} "
                f"reason=provider-placeholder"
            )
            continue
        try:
            debug_log(f"download.start source={ref.source} id={ref.id}")
            r = http.get(ref.url, allow_redirects=True)
            if r.status_code != 200:
                raise SplinedError(f"artwork download returned HTTP {r.status_code}")
            if not r.content:
                raise SplinedError("artwork download was empty")
            with Image.open(io.BytesIO(r.content)) as im:
                fmt = image_format(im)
                width, height = im.size
                im.verify()
            digest = hashlib.sha256((ref.source + "\0" + ref.url).encode()).hexdigest()[:24]
            path = cache / f"splined-candidate-{digest}.{cache_extension_from_format(fmt)}"
            path.write_bytes(r.content)
            out.append(Candidate(ref, path, width, height, fmt, sources.index(ref.source)))
            debug_log(
                f"download.done source={ref.source} id={ref.id} "
                f"size={width}x{height} format={fmt}"
            )
        except Exception as exc:
            diag.append((ref.source, f"{exc} ({ref.url})"))
            debug_log(
                f"download.error source={ref.source} id={ref.id} "
                f"error={type(exc).__name__}: {exc}"
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
        clean_first=True,
    )
    diag += download_diag

    debug_log(f"normal.discovery candidates_total={len(candidates)}")
    return candidates, diag, list(sources)



def range_class(c: Candidate, cfg: dict[str, Any]) -> str:
    return classify_range(c.short_side, cfg)


def acceptable(c: Candidate, cfg: dict[str, Any]) -> bool:
    return range_class(c, cfg) not in {"BelowMinimum", "AboveLadder"}


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
        0 if projected["acceptable"] else 1,
        projected["distance"],
        range_rank,
        transform_penalty,
        c.source_priority,
        format_order.index(projected["format"]) if projected["format"] in format_order else 999999,
        -projected["short_side"],
        provider_label(c.source).lower(),
        str(c.ref.id),
    )

def select_best(candidates: list[Candidate], cfg: dict[str, Any], format_order: list[str]) -> Candidate | None:
    valid = [c for c in candidates if project_candidate(c, cfg, target_format_for_candidate(c, format_order))["acceptable"]]
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
    mbcfg = section(cfg, "musicbrainz")
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
                params={"query": query, "fmt": "json", "limit": 10},
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
            out.append({
                "id": mbid,
                "title": str(item.get("title") or "").strip(),
                "artist": "".join(artist_parts).strip(),
                "date": str(item.get("date") or "").strip(),
                "country": str(item.get("country") or "").strip(),
                "status": str(item.get("status") or "").strip(),
            })
        return out

    return []


def musicbrainz_picker(
    http: Http,
    config_file: Path,
    cfg: dict[str, Any],
    artist: str,
    album: str,
    exact_mbid: str | None = None,
) -> Release | None:
    if exact_mbid:
        print(f"  {cyan('MusicBrainz:'):13} {magenta('retrying exact release')} {magenta(exact_mbid)}")
        try:
            return lookup_release(http, config_file, cfg, exact_mbid)
        except Exception as exc:
            print(f"  {red('MusicBrainz retry failed: ' + str(exc))}")
            return None

    try:
        results = musicbrainz_search_releases(http, config_file, cfg, artist, album)
    except Exception as exc:
        print(f"  {red('MusicBrainz search failed: ' + str(exc))}")
        return None

    if not results:
        print(f"  {yellow('MusicBrainz search returned no releases.')}")
        return None

    print()
    print(f"  {cyan('MusicBrainz release search')}")
    print(
        "  "
        + ljust_color(cyan("[#]"), 5) + " "
        + ljust_color(cyan("Artist"), 24) + " "
        + ljust_color(cyan("Release"), 34) + " "
        + ljust_color(cyan("Date"), 10) + " "
        + ljust_color(cyan("Country"), 9) + " "
        + cyan("MBID")
    )
    print("  " + gray("-" * 126))
    for index, result in enumerate(results, 1):
        print(
            "  "
            + ljust_color(white(f"[{index}]"), 5) + " "
            + ljust_color(magenta(result["artist"][:24]), 24) + " "
            + ljust_color(orange(result["title"][:34]), 34) + " "
            + ljust_color(white(result["date"][:10]), 10) + " "
            + ljust_color(white(result["country"][:9]), 9) + " "
            + magenta(result["id"])
        )

    while True:
        answer = input("MusicBrainz choice [#] or [b] back: ").strip().lower()
        if answer in {"b", ""}:
            return None
        if answer.isdigit():
            index = int(answer)
            if 1 <= index <= len(results):
                try:
                    return lookup_release(http, config_file, cfg, results[index - 1]["id"])
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



def prepare_samples(cache: Path) -> Path:
    sample = cache / "samples"
    if sample.name != "samples": raise SplinedError(f"Refusing to clean unexpected SPLINED samples path: {sample}")
    if sample.exists():
        if sample.is_symlink() or not sample.is_dir(): raise SplinedError(f"Refusing to clean unsafe SPLINED samples directory: {sample}")
        shutil.rmtree(sample)
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
    content = c.path.read_bytes(); canonical = sample_dir / f"{sanitize(rel.artist_credit)}.{sanitize(rel.title)}.sample.{EXTENSIONS[c.format]}"; dest, unchanged = choose_destination(canonical, content, preserve)
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
            raise SplinedError("SPLINED --scan-dir path cannot be empty.")
        override = Path(raw_override)
        root = override if override.is_absolute() else library_root / override
        return root, library_root

    raw_root = str(scan.get("scan_library_dir", "")).strip()
    if not raw_root:
        raise SplinedError(
            "SPLINED [scan].scan_library_dir is not configured; "
            "use --scan-dir PATH or configure [scan].scan_library_dir."
        )
    return resolve_path(config_file, raw_root), library_root


def run_config_edit(path: Path) -> int:
    if not sys.stdin.isatty() or not sys.stdout.isatty():
        raise SplinedError("SPLINED --config-edit requires an interactive terminal.")

    editor = shutil.which("micro")
    if not editor:
        raise SplinedError(
            "SPLINED --config-edit requires micro inside the container. "
            "Install micro in the SPLINED image and rebuild."
        )

    if not path.exists():
        raise SplinedError(f"Configuration file not found: {path}")

    if not os.access(path, os.W_OK):
        raise SplinedError(
            f"SPLINED configuration is not writable: {path}. "
            "Mount /config read-write (:rw) to use --config-edit."
        )

    # The SPLINED container intentionally runs as an unprivileged UID/GID and
    # may not have a passwd-backed HOME. Without explicit XDG paths micro falls
    # back to /.config, which is not writable. Give the editor disposable,
    # user-writable state under /tmp; the actual SPLINED config remains the
    # mounted /config/config.toml file.
    editor_home = Path("/tmp/splined-editor")
    xdg_config = editor_home / ".config"
    xdg_data = editor_home / ".local" / "share"
    xdg_cache = editor_home / ".cache"
    for directory in (editor_home, xdg_config, xdg_data, xdg_cache):
        directory.mkdir(parents=True, exist_ok=True)

    env = os.environ.copy()
    env["HOME"] = str(editor_home)
    env["XDG_CONFIG_HOME"] = str(xdg_config)
    env["XDG_DATA_HOME"] = str(xdg_data)
    env["XDG_CACHE_HOME"] = str(xdg_cache)

    os.execve(editor, [editor, str(path)], env)
    return 0



def run_scan_dir(
    config_file: Path,
    cfg: dict[str, Any],
    sources: list[str],
    scan_words: list[str] | None = None,
) -> int:
    scan = section(cfg, "scan")
    library = section(cfg, "library")
    output = section(cfg, "output")
    samples_cfg = section(cfg, "samples")
    mbcfg = section(cfg, "musicbrainz")

    root, library_root = resolve_scan_root(config_file, cfg, scan_words)
    cache = resolve_path(config_file, str(scan.get("cache_dir", "cache")))
    ignored = [str(x) for x in library.get("ignored_subs", [])]
    fmt_order = formats(cfg)
    preserve = bool(output.get("preserve_file", True))
    mode = str(cfg.get("mode", "read")).lower()
    samples_enabled = bool(samples_cfg.get("sample_write", True))

    output_settings(cfg)
    cache.mkdir(parents=True, exist_ok=True)
    sample_dir = prepare_samples(cache)
    discovered_albums, ignored_dirs = inventory(root, ignored)

    timeout_hours = scan_timeout_hours(cfg)
    completion_path = scan_completion_history_path(cache)
    completion_history = load_scan_completion_history(completion_path)
    completion_path.parent.mkdir(parents=True, exist_ok=True)

    albums: list[AlbumDir] = []
    postponed_albums: list[tuple[AlbumDir, float]] = []
    for album in discovered_albums:
        postponed, age_hours = scan_completion_status(
            completion_history,
            album,
            cfg,
            sources,
            timeout_hours,
        )
        if postponed:
            postponed_albums.append((album, age_hours))
            debug_log(
                f"scan.postponed album={str(album.path)!r} "
                f"age_hours={age_hours:.3f} timeout_hours={timeout_hours:g}"
            )
        else:
            albums.append(album)

    http = Http()
    _, mbmode = mb_headers(config_file, cfg)
    summary = Summary(
        albums=len(discovered_albums),
        postponed=len(postponed_albums),
    )
    history_path = source_history_path(cache)
    source_history = load_source_history(history_path)
    # Ensure persistent history folders exist before the first write.
    history_path.parent.mkdir(parents=True, exist_ok=True)
    api_queried: set[str] = set()

    # ------------------------------------------------------------------
    # Pre-pass. Determine which albums need fallback BEFORE rendering.
    # This guarantees fallback albums are handled first, one at a time.
    # ------------------------------------------------------------------
    records: list[dict[str, Any]] = []
    for album in albums:
        record: dict[str, Any] = {
            "album": album,
            "tracks": None,
            "file_count": 0,
            "compilation": "Standard",
            "tag_album": None,
            "tag_artist": None,
            "valid": {},
            "missing": [],
            "invalid": [],
            "mbid": None,
            "release": None,
            "fallback_reason": None,
        }
        try:
            tracks = [read_track(path) for path in album.audio_files]
            record["tracks"] = tracks
            record["file_count"] = len(tracks)
            record["compilation"] = "Compilation" if any((t.compilation or "").strip() == "1" for t in tracks) else "Standard"
            record["tag_album"] = tagged_album(tracks)
            record["tag_artist"] = tagged_artist(tracks)
            valid, missing, invalid = audit_album_ids(tracks)
            record["valid"] = valid
            record["missing"] = missing
            record["invalid"] = invalid

            if len(valid) != 1:
                if len(valid) > 1:
                    record["fallback_reason"] = "MULTIPLE MBIDS"
                elif invalid:
                    record["fallback_reason"] = "INVALID MBID"
                else:
                    record["fallback_reason"] = "NO MBID FOUND"
            else:
                mbid = next(iter(valid))
                record["mbid"] = mbid
                try:
                    release = lookup_release(http, config_file, cfg, mbid)
                    record["release"] = release
                    if record["tag_album"] and normalize_text(record["tag_album"]) != normalize_text(release.title):
                        record["fallback_reason"] = "TAG / RELEASE MISMATCH"
                except Exception as exc:
                    record["fallback_reason"] = fallback_reason_from_error(exc)
                    record["mb_error"] = str(exc)
        except Exception as exc:
            record["fatal_error"] = str(exc)
            # A tag-read failure cannot participate in provider fallback.
            record["fallback_reason"] = "TAG READ FAILURE"
        records.append(record)

    fallback_records = [record for record in records if record.get("fallback_reason")]
    normal_records = [record for record in records if not record.get("fallback_reason")]
    ordered_records = fallback_records + normal_records

    # ------------------------------------------------------------------
    # Run header.
    # ------------------------------------------------------------------
    provider_list = [source for source in sources if source != "discogs"]
    output_list = [
        f"{str(output.get('file_name', 'cover')).strip()}.{EXTENSIONS[f]}"
        for f in fmt_order
    ]

    print(bold(cyan("SPLINED LIBRARY SCAN")))
    print()
    print(f"  {cyan('Mode:'):14} {orange(mode.capitalize())}")
    print(f"  {cyan('Mutation:'):14} {red('enabled') if mode == 'write' else green('disabled')}")
    print()
    print(f"  {cyan('Library:'):14} {white(str(library_root))}")
    print(f"  {cyan('Library Scan:'):14} {white(str(root))}")
    print()
    print(f"  {cyan('Samples:'):14} {green('enabled') if samples_enabled else red('disabled')}")
    print(f"  {cyan('Cache:'):14} {white(str(cache))}")
    print(f"  {cyan('Sample Dir:'):14} {white(str(sample_dir))}")
    if _DEBUG_ENABLED and _DEBUG_PATH is not None:
        print(f"  {cyan('Debug Log:'):14} {white(str(_DEBUG_PATH))}")
    print()
    print(
        ljust_color(cyan("Run:"), 14)
        + ljust_color(white("Preserve"), 11)
        + bracketed_text(bool_text(preserve), green if preserve else red)
        + " "
        + white("Albums") + " " + bracketed_text(str(len(discovered_albums)), white)
        + " "
        + white("Postponed") + " " + bracketed_text(
            str(len(postponed_albums)),
            cyan if postponed_albums else white,
        )
        + " "
        + white("Ignored") + " " + bracketed_text(str(len(ignored_dirs)), white)
    )
    print(
        ljust_color(cyan("Timeout:"), 14)
        + bracketed_text(format_timeout_hours(timeout_hours) + "h", green)
        + white(" completed-album reprocessing window")
    )
    print(ljust_color(cyan("Providers:"), 14) + bracketed_list(provider_list, green))
    print(ljust_color(cyan("Source:"), 14) + bracketed_list(sources, green))
    mb_rhs = (
        magenta(mbmode + ", Retry ")
        + white("[") + magenta(str(int(mbcfg.get("retry_max", 2)))) + white("]")
        + magenta(" Delay ") + white("[") + magenta(f'{float(mbcfg.get("mb_min_delay", 1.05)):.2f}s') + white("]")
        + magenta(" Timeout ") + white("[") + magenta(f'{float(mbcfg.get("mb_recording_timeout", 7.0)):.1f}s') + white("]")
    )
    print(ljust_color(cyan("MusicBrainz:"), 14) + mb_rhs)
    if output_list:
        rest = " / ".join(output_list[1:])
        print(
            ljust_color(cyan("Output:"), 14)
            + white("[") + orange(output_list[0])
            + white(" / " + rest if rest else "") + white("]")
        )
    else:
        print(ljust_color(cyan("Output:"), 14) + white("[]"))
    print()

    # ------------------------------------------------------------------
    # Process fallback queue first, then normal MB-validated albums.
    # ------------------------------------------------------------------
    for run_index, record in enumerate(ordered_records, 1):
        album: AlbumDir = record["album"]
        tracks: list[Track] | None = record.get("tracks")
        file_count = int(record.get("file_count", 0))
        compilation = str(record.get("compilation") or "Standard")
        tag_album = str(record.get("tag_album") or album.path.name)
        tag_artist = str(record.get("tag_artist") or "")
        valid = record.get("valid") or {}
        missing = record.get("missing") or []
        invalid = record.get("invalid") or []
        mbid = record.get("mbid")
        release: Release | None = record.get("release")
        fallback_reason = record.get("fallback_reason")

        print(bold(cyan(f"[{run_index}/{len(ordered_records)}] {album_path_text(album.path)}")))

        if record.get("fatal_error"):
            summary.failed += 1
            print(f"  {red('ERROR: ' + str(record['fatal_error']))}\n")
            continue

        # --------------------------------------------------------------
        # FALLBACK PRE-PASS ALBUM
        # --------------------------------------------------------------
        if fallback_reason:
            print(f"  {cyan('Tracks:'):13} {bracketed_text(str(file_count), red)}")
            print(f"  {cyan('Compilation:'):13} {white(compilation)}")
            print()
            print(f"  {cyan('Authority:'):13} {paint('PURPLE', 'N/A · FALLBACK MODE')}")
            if release is not None:
                mb_count_text = "?" if release.track_count is None else str(release.track_count)
                count_fmt = green if release.track_count == file_count else red
                print(f"  {cyan('MB Artist:'):13} {magenta(release.artist_credit or 'N/A')}")
                print(f"  {cyan('MB Release:'):13} {orange(release.title)} {bracketed_text(mb_count_text, count_fmt)}")
            else:
                print(f"  {cyan('MB Artist:'):13} {gray('N/A')}")
                print(f"  {cyan('MB Release:'):13} {gray('N/A')}")

            reason_text = str(fallback_reason)
            mbid_display = magenta(mbid) if mbid else gray("N/A")
            print(
                f"  {cyan('Tagged Album:'):13} "
                f"{orange(tag_album)} {bracketed_text('UNMATCHED', red)} / "
                f"{mbid_display} {paint('PURPLE', '· ' + reason_text)}"
            )

            # Evidence is useful for missing/conflicting/invalid tag authority.
            if len(valid) != 1 or missing or invalid:
                print(f"  {cyan('MBID Evidence:')}")
                for evidence_mbid, paths in sorted(valid.items(), key=lambda kv: (-len(kv[1]), kv[0])):
                    print(f"    - {len(paths)}/{file_count} tracks: {magenta(evidence_mbid)}")
                    print(f"      Files: {gray(compact(paths))}")
                if missing:
                    print(f"    - Missing/blank: {len(missing)}/{file_count} tracks")
                    print(f"      Files: {gray(compact(missing))}")
                for path_value, raw_value in invalid:
                    print(f"    - {yellow('Invalid MBID')}: {path_value.name}: {yellow(raw_value)}")

            if not tag_artist or not tag_album:
                summary.unresolved += 1
                print(f"  {red('Fallback requires tagged Artist + Album values.')}")
                print(f"  {cyan('Artwork:'):13} {bracketed_text('SKIPPED', yellow)}\n")
                continue

            search_artist = tag_artist
            search_album = tag_album
            recovered_release: Release | None = None
            chosen_candidate: Candidate | None = None
            chosen_pool: list[Candidate] = []
            diag: list[tuple[str, str]] = []
            completion_outcome: str | None = None

            while True:
                if recovered_release is not None:
                    refs, diag = discover_all(
                        http, config_file, cfg, recovered_release, sources,
                        queried_sources=api_queried,
                    )
                    sample_release = recovered_release
                else:
                    refs, diag = discover_fallback(
                        http,
                        config_file,
                        cfg,
                        search_artist,
                        search_album,
                        sources,
                        release_mbid=mbid,
                        queried_sources=api_queried,
                    )
                    sample_release = Release(
                        mbid=mbid or "",
                        title=search_album,
                        artist_credit=search_artist,
                        release_group_id=None,
                        release_group_title=None,
                        track_count=None,
                    )

                candidates, download_diag = download_candidates(http, refs, sources, cache)
                diag += download_diag

                # If fallback finds anything inside the normal configured range,
                # use the normal SPLINED scorer and avoid the manual picker.
                normal_best = select_best(candidates, cfg, fmt_order)
                if normal_best is not None:
                    chosen_candidate = normal_best
                    chosen_pool = candidates
                    print()
                    authority_text = "ExactAlbumId · RECOVERED" if recovered_release else "ArtistAlbumFallback · AUTO"
                    print(f"  {cyan('Authority:'):13} {green(authority_text)}")
                    preview_dest, _ = preview_destination(album, normal_best, cfg, fmt_order)
                    print(
                        f"  {cyan('Candidates:'):13} "
                        f"{white(str(len(candidates)))} {white('Chose')} "
                        f"{bracketed_text(provider_label(normal_best.source), magenta)} "
                        f"{white('File')} {bracketed_text(preview_dest.name, orange)} "
                        f"{format_source_url(normal_best.ref.url)}"
                    )
                    render_candidate_table(candidates, cfg, fmt_order, selected=normal_best)
                    if diag:
                        print(f"  {cyan('Diagnostics:'):13}")
                        for source, msg in diag:
                            print(f"    - {magenta(provider_label(source))}: {msg}")
                    if apply_selected_candidate(
                        album, sample_release, normal_best, cfg, fmt_order,
                        sample_dir, preserve, samples_enabled, summary
                    ):
                        summary.resolved += 1
                        completion_outcome = "fallback-auto-selected"
                    else:
                        summary.unresolved += 1
                    break

                # No normally acceptable result: show manual fallback picker.
                top = fallback_top_candidates(candidates, cfg, fmt_order, 10)
                suggested = fallback_suggested(top, cfg, fmt_order)

                print()
                preview_name = str(output.get("file_name", "cover")).strip() + "." + EXTENSIONS[fmt_order[0]]
                print(
                    f"  {cyan('Candidates:'):13} "
                    f"{white(str(len(top)))} {paint('PURPLE', '· FALLBACK PICKER')} "
                    f"{white('File')} {bracketed_text(preview_name, orange)} "
                    f"{format_source_url(suggested.ref.url if suggested else '') if suggested else ''}"
                )
                if top:
                    render_candidate_table(top, cfg, fmt_order, suggested=suggested, manual_fallback=True)
                else:
                    print(f"  {yellow('No fallback artwork candidates were returned.')}")

                if diag:
                    print(f"  {cyan('Diagnostics:'):13}")
                    for source, msg in diag:
                        print(f"    - {magenta(provider_label(source))}: {msg}")

                print()
                print(
                    f"  {paint('PURPLE', '[s]')} suggested exception   "
                    f"{cyan('[#]')} choose number   "
                    f"{cyan('[f]')} fuzzy search   "
                    f"{cyan('[m]')} MusicBrainz search/retry   "
                    f"{cyan('[b]')} bypass"
                )
                answer = input("  Choice: ").strip().lower()

                if answer == "b":
                    summary.unresolved += 1
                    completion_outcome = "fallback-bypassed"
                    print(f"  {cyan('Artwork:'):13} {bracketed_text('BYPASSED', yellow)}")
                    break

                if answer == "s":
                    if suggested is None:
                        print(f"  {yellow('No suggested fallback candidate is available.')}")
                        continue
                    chosen_candidate = suggested
                    chosen_pool = top
                    if apply_selected_candidate(
                        album, sample_release, suggested, cfg, fmt_order,
                        sample_dir, preserve, samples_enabled, summary,
                        allow_out_of_range=True,
                    ):
                        summary.resolved += 1
                        completion_outcome = "fallback-manual-suggested"
                    else:
                        summary.unresolved += 1
                    break

                if answer.isdigit():
                    number = int(answer)
                    if 1 <= number <= len(top):
                        selected = top[number - 1]
                        chosen_candidate = selected
                        chosen_pool = top
                        if apply_selected_candidate(
                            album, sample_release, selected, cfg, fmt_order,
                            sample_dir, preserve, samples_enabled, summary,
                            allow_out_of_range=True,
                        ):
                            summary.resolved += 1
                            completion_outcome = "fallback-manual-number"
                        else:
                            summary.unresolved += 1
                        break
                    print(f"  {yellow('Choose a listed candidate number.')}")
                    continue

                if answer == "f":
                    entered_artist = input(f"  Artist [{search_artist}]: ").strip()
                    entered_album = input(f"  Album  [{search_album}]: ").strip()
                    if entered_artist:
                        search_artist = entered_artist
                    if entered_album:
                        search_album = entered_album
                    recovered_release = None
                    continue

                if answer == "m":
                    recovered = musicbrainz_picker(
                        http,
                        config_file,
                        cfg,
                        search_artist,
                        search_album,
                        exact_mbid=mbid if mbid and release is None else None,
                    )
                    if recovered is not None:
                        recovered_release = recovered
                        mbid = recovered.mbid
                        search_artist = recovered.artist_credit or search_artist
                        search_album = recovered.title or search_album
                    continue

                print("  Choose s, a listed number, f, m, or b.")

            if completion_outcome is not None:
                record_scan_completion(
                    completion_path,
                    completion_history,
                    album,
                    cfg,
                    sources,
                    completion_outcome,
                )
            print()
            continue

        # --------------------------------------------------------------
        # NORMAL EXACT-MB ALBUM
        # --------------------------------------------------------------
        assert release is not None and mbid is not None
        mb_count_text = "?" if release.track_count is None else str(release.track_count)
        count_match = release.track_count is not None and release.track_count == file_count
        count_fmt = green if count_match else red

        print(f"  {cyan('Tracks:'):13} {bracketed_text(str(file_count), count_fmt)}")
        print(f"  {cyan('Compilation:'):13} {white(compilation)}")
        print()
        print(f"  {cyan('Authority:'):13} {green('ExactAlbumId')}")
        print(f"  {cyan('MB Artist:'):13} {green(release.artist_credit)}")
        print(f"  {cyan('MB Release:'):13} {orange(release.title)} {bracketed_text(mb_count_text, count_fmt)}")
        print(
            f"  {cyan('Tagged Album:'):13} "
            f"{orange(tag_album or release.title)} {bracketed_text('MATCHED', green)} / {magenta(mbid)}"
        )
        print()

        candidates, diag, queried_this_album = discover_normal_ranked(
            http,
            config_file,
            cfg,
            release,
            sources,
            cache,
            fmt_order,
            source_history,
            queried_sources=api_queried,
        )
        best = select_best(candidates, cfg, fmt_order)

        if best is None:
            acceptable_count = sum(
                1
                for candidate in candidates
                if project_candidate(
                    candidate,
                    cfg,
                    target_format_for_candidate(candidate, fmt_order),
                )["acceptable"]
            )
            debug_log(
                f"normal.selection album={release.title!r} "
                f"candidates={len(candidates)} acceptable={acceptable_count} chosen=none"
            )

            if not candidates:
                summary.resolved += 1
                print(
                    f"  {cyan('Candidates:'):13} {white('0')} "
                    f"{white('Chose')} {bracketed_text('none', magenta)}"
                )
                print(
                    f"  {cyan('Discovery:'):13} "
                    f"{yellow('No artwork candidates were returned by queried sources')}"
                )
                if diag:
                    print(f"  {cyan('Diagnostics:'):13}")
                    for source, msg in diag:
                        print(f"    - {magenta(provider_label(source))}: {msg}")
                print(f"  {cyan('Artwork:'):13} {bracketed_text('SKIPPED', yellow)}")
                record_scan_completion(
                    completion_path,
                    completion_history,
                    album,
                    cfg,
                    sources,
                    "normal-no-candidates",
                )
                print()
                continue

            # Exact MusicBrainz authority is valid, but every downloaded image
            # is outside the automatic output range. Keep the range policy
            # strict and require an explicit manual exception.
            manual_pool = sorted(
                candidates,
                key=lambda candidate: fallback_sort_key(
                    candidate, cfg, fmt_order
                ),
            )
            suggested = fallback_suggested(manual_pool, cfg, fmt_order)

            print(
                f"  {cyan('Candidates:'):13} {white(str(len(manual_pool)))} "
                f"{paint('PURPLE', '· OUT-OF-RANGE PICKER')}"
            )
            render_candidate_table(
                manual_pool,
                cfg,
                fmt_order,
                suggested=suggested,
                manual_fallback=True,
            )

            if diag:
                print(f"  {cyan('Diagnostics:'):13}")
                for source, msg in diag:
                    print(f"    - {magenta(provider_label(source))}: {msg}")

            while True:
                print()
                print(
                    f"  {paint('PURPLE', '[s]')} suggested closest candidate   "
                    f"{cyan('[#]')} choose exact candidate   "
                    f"{cyan('[b]')} bypass"
                )
                answer = input("  Choice: ").strip().lower()

                if answer == "b":
                    summary.resolved += 1
                    print(
                        f"  {cyan('Artwork:'):13} "
                        f"{bracketed_text('BYPASSED', yellow)}"
                    )
                    record_scan_completion(
                        completion_path,
                        completion_history,
                        album,
                        cfg,
                        sources,
                        "normal-out-of-range-bypassed",
                    )
                    break

                if answer == "s":
                    if suggested is None:
                        print(f"  {yellow('No suggested candidate is available.')}")
                        continue
                    if apply_selected_candidate(
                        album,
                        release,
                        suggested,
                        cfg,
                        fmt_order,
                        sample_dir,
                        preserve,
                        samples_enabled,
                        summary,
                        allow_out_of_range=True,
                    ):
                        summary.resolved += 1
                        record_scan_completion(
                            completion_path,
                            completion_history,
                            album,
                            cfg,
                            sources,
                            "normal-out-of-range-suggested",
                        )
                    else:
                        summary.unresolved += 1
                    break

                if answer.isdigit():
                    number = int(answer)
                    if 1 <= number <= len(manual_pool):
                        selected = manual_pool[number - 1]
                        if apply_selected_candidate(
                            album,
                            release,
                            selected,
                            cfg,
                            fmt_order,
                            sample_dir,
                            preserve,
                            samples_enabled,
                            summary,
                            allow_out_of_range=True,
                        ):
                            summary.resolved += 1
                            record_scan_completion(
                                completion_path,
                                completion_history,
                                album,
                                cfg,
                                sources,
                                "normal-out-of-range-number",
                            )
                        else:
                            summary.unresolved += 1
                        break
                    print(f"  {yellow('Choose a listed candidate number.')}")
                    continue

                print("  Choose s, a listed number, or b.")

            print()
            continue

        summary.resolved += 1
        debug_log(
            f"normal.selection album={release.title!r} "
            f"candidates={len(candidates)} chosen_source={best.source} "
            f"chosen_id={best.ref.id}"
        )
        preview_dest, _ = preview_destination(album, best, cfg, fmt_order)
        print(
            f"  {cyan('Candidates:'):13} "
            f"{white(str(len(candidates)))} {white('Chose')} "
            f"{bracketed_text(provider_label(best.source), magenta)} "
            f"{white('File')} {bracketed_text(preview_dest.name, orange)} "
            f"{format_source_url(best.ref.url)}"
        )
        render_candidate_table(candidates, cfg, fmt_order, selected=best)

        if diag:
            print(f"  {cyan('Diagnostics:'):13}")
            for source, msg in diag:
                print(f"    - {magenta(provider_label(source))}: {msg}")

        normal_apply_ok = apply_selected_candidate(
            album, release, best, cfg, fmt_order,
            sample_dir, preserve, samples_enabled, summary
        )
        if normal_apply_ok:
            record_source_selection(
                history_path,
                source_history,
                best,
                cfg,
                fmt_order,
            )
            record_scan_completion(
                completion_path,
                completion_history,
                album,
                cfg,
                sources,
                "normal-selected",
            )
        print()

    # ------------------------------------------------------------------
    # Fixed-width aligned summary. ANSI escapes are ignored for padding.
    # ------------------------------------------------------------------
    print(bold(cyan(f"SPLINED SCAN LIBRARY {mode.upper()} SUMMARY")))
    print()

    album_cells = [
        ljust_color(cyan("Albums:"), 10),
        ljust_color(white("Selected"), 11) + bracketed_text(str(summary.selected), green if summary.selected else white),
        ljust_color(white("Resolved"), 11) + bracketed_text(str(summary.resolved), green if summary.resolved else white),
        ljust_color(white("Postponed"), 12) + bracketed_text(str(summary.postponed), cyan if summary.postponed else white),
        ljust_color(white("Unresolved"), 11) + bracketed_text(str(summary.unresolved), green if summary.unresolved == 0 else red),
        ljust_color(white("Failed"), 10) + bracketed_text(str(summary.failed), green if summary.failed == 0 else red),
    ]
    print("  ".join(album_cells))

    sample_total = summary.samples_written + summary.samples_unchanged
    sample_cells = [
        ljust_color(cyan("Samples:"), 10),
        ljust_color(white("Count"), 11) + bracketed_text(str(sample_total), green if sample_total else white),
        ljust_color(white("Installed"), 11) + bracketed_text(str(summary.installed), green if summary.installed else white),
        ljust_color(white("Unchanged"), 11) + bracketed_text(str(summary.unchanged), cyan if summary.unchanged else white),
    ]
    print("  ".join(sample_cells))

    queried_list = [source for source in sources if source in api_queried]
    skipped_list = [source for source in sources if source not in api_queried]
    print(
        ljust_color(cyan("API:"), 10)
        + white("Queried") + " " + bracketed_list(queried_list, green)
        + " "
        + white("Skipped") + " " + bracketed_list(skipped_list, gray)
    )
    return 0 if summary.failed == 0 else 1


def run_release_discovery(config_file: Path, cfg: dict[str, Any], sources: list[str], release_mbid: str) -> int:
    mbid = valid_mbid(release_mbid)
    if not mbid:
        raise SplinedError(f"Invalid MusicBrainz release MBID: {release_mbid}")
    http = Http()
    rel = lookup_release(http, config_file, cfg, mbid)
    print(f"Artist:      {rel.artist_credit}")
    print(f"Release:     {rel.title}")
    if rel.release_group_title:
        print(f"Release Group: {rel.release_group_title}")
    if rel.release_group_id:
        print(f"Release Group MBID: {rel.release_group_id}")

    cache = resolve_path(config_file, str(section(cfg, "scan").get("cache_dir", "cache")))
    fmt_order = formats(cfg)
    refs, diag = discover_all(http, config_file, cfg, rel, sources)
    candidates, download_diag = download_candidates(http, refs, sources, cache)
    diag += download_diag
    best = select_best(candidates, cfg, fmt_order)

    print(f"Candidates: {len(candidates)} downloaded front image(s)")
    for n, c in enumerate(candidates, 1):
        marker = "*" if c is best else " "
        print(f"{marker} [{n}] {summary_line(c, cfg, fmt_order)}")
    if diag:
        print("\nProvider diagnostics:")
        for source, msg in diag:
            print(f"- {source}: {msg}")
    if best is None:
        print("\nSelected: none")
    else:
        print(f"\nSelected:\n{best.width}x{best.height} {best.format.upper()} from {best.source}")
        print(f"URL: {best.ref.url}")
    return 0


def run_scan_preview(
    config_file: Path,
    cfg: dict[str, Any],
    sources: list[str],
) -> int:
    scan = section(cfg, "scan")
    library = section(cfg, "library")
    root = resolve_path(
        config_file,
        str(library.get("music_library", "")),
    )
    ignored = [str(x) for x in library.get("ignored_subs", [])]
    albums, ignored_dirs = inventory(root, ignored)

    cache = resolve_path(config_file, str(scan.get("cache_dir", "cache")))
    timeout_hours = scan_timeout_hours(cfg)
    completion_history = load_scan_completion_history(
        scan_completion_history_path(cache)
    )

    eligible = 0
    postponed = 0
    for album in albums:
        is_postponed, _ = scan_completion_status(
            completion_history,
            album,
            cfg,
            sources,
            timeout_hours,
        )
        if is_postponed:
            postponed += 1
        else:
            eligible += 1

    print("SPLINED LIBRARY SCAN")
    print()
    print(f"Mode:       {str(cfg.get('mode', 'read')).capitalize()}")
    print(f"Directory:  {root}")
    print(f"Sources:    {', '.join(sources)}")
    print(f"Timeout:    {format_timeout_hours(timeout_hours)}h")
    print(f"Albums:     {len(albums)}")
    print(f"Eligible:   {eligible}")
    print(f"Postponed:  {postponed}")
    print(f"Ignored:    {len(ignored_dirs)}")
    print()
    print("Use --scan-dir for the operational album scan pipeline.")
    return 0


APP_NAME = "SPLINED"
VERSION = "1.0.2"
CONFIG_VERSION = 4
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
    if cfg.get("config_version") != CONFIG_VERSION:
        raise SplinedError(f"Unsupported config_version {cfg.get('config_version')!r}; expected {CONFIG_VERSION}.")
    mode = str(cfg.get("mode", "read")).strip().lower()
    if mode not in {"read", "write"}:
        raise SplinedError("SPLINED mode must be 'read' or 'write'.")
    r = section(cfg, "range")
    mn, ideal, mx, ladder = int(r.get("min",1200)), int(r.get("ideal",1800)), int(r.get("max",2400)), int(r.get("ladder",3600))
    if mn >= ideal: raise SplinedError("[range].min must be below [range].ideal.")
    if ideal > mx: raise SplinedError("[range].ideal cannot exceed [range].max.")
    if mx >= ladder: raise SplinedError("[range].max must be below [range].ladder.")
    formats(cfg)
    out = section(cfg, "output")
    validate_filename(str(out.get("file_name", "cover")))
    square_mode = str(out.get("square_mode", "crop" if out.get("square", False) else "off")).strip().lower()
    if square_mode not in {"off", "crop"}:
        raise SplinedError("[output].square_mode must be 'off' or 'crop'.")
    round_to = int(out.get("square_round_to", 0) or 0)
    if round_to < 0:
        raise SplinedError("[output].square_round_to cannot be negative.")
    scan_timeout_hours(cfg)
    return path, cfg


def parse_sources(value: str | None) -> list[str] | None:
    if value is None: return None
    return [x.strip() for x in value.split(",") if x.strip()]


def help_row(label: str, value: str = "") -> None:
    print(f"{label:<{HELP_COLUMN_WIDTH}}{value}" if value else label)



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
    safe_url = str(url).replace("\x1b", "").replace("\x07", "").strip()
    if not safe_url:
        return label
    # OSC 8 hyperlink. Unsupported terminals simply show the label.
    return f"\033]8;;{safe_url}\033\\{label}\033]8;;\033\\"


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
def print_help(path: Path, cfg: dict[str, Any]) -> None:
    scan = section(cfg, "scan")
    library = section(cfg, "library")
    credentials = section(cfg, "credentials")
    sources = section(cfg, "sources")
    output = section(cfg, "output")

    cache = resolve_path(path, str(scan.get("cache_dir", "cache")))
    cdir = resolve_path(path, str(credentials.get("credential_dir", "credentials")))
    ignored = library.get("ignored_subs", [])
    ignored = ignored if isinstance(ignored, list) else []

    cover = sources.get("cover_sources", [])
    cover = cover if isinstance(cover, list) else []
    excluded = sources.get("exclude_cover_sources", [])
    excluded = excluded if isinstance(excluded, list) else []
    fmts = formats(cfg)

    history = load_source_history(source_history_path(cache))

    print("SPLINED artwork discovery and evaluation engine\n")
    print("Usage: splined [OPTIONS]\n")

    print("Media Directories:")
    help_row("Library:", green(f"[{library.get('music_library', '')}]"))
    help_row("Sample Covers:", green(f"[{cache / 'samples'}]"))
    help_row("Scan Directory:", green(f"[{scan.get('scan_library_dir', '')}]"))
    help_row("Ignore Sub-Directories:", red("[" + ", ".join(map(str, ignored)) + "]"))
    print()

    print("Configuration:")
    help_row("      --config", "Show resolved SPLINED configuration")
    help_row("      --config-check", "Validate configuration and exit")
    help_row("      --config-edit", "Edit config.toml using micro")
    help_row("      Config File", green(f"[{path}]"))
    help_row("      Credential Directory", green(f"[{cdir}]"))
    print()

    print("System Modes:")
    help_row("      read", "read file(s) [always Debug verbosity]")
    help_row(
        "      write",
        f"write file(s) as 'cover.<file_formats>' using selected source [{','.join(fmts)}]",
    )
    help_row(
        "      scan_mode_timeout",
        "postpone re-processing of both library & source scanning within "
        + bracketed_text(
            format_timeout_hours(scan_timeout_hours(cfg)),
            green,
        )
        + " hours",
    )
    print()

    print("Library Scanning:")
    help_row(
        "      library_scan",
        f"configuration state for --scan [{str(bool(scan.get('library_scan', False))).lower()}]",
    )
    help_row("      --scan", "Use [library].music_library for preview/inventory")
    print()

    print("Source Scanning:")
    help_row(
        "      scan_mode",
        f"configuration state for --scan-dir [{str(bool(scan.get('scan_mode', True))).lower()}]",
    )
    help_row(
        "      --scan-dir",
        "Run [scan].scan_library_dir using top-level mode=read/write",
    )
    help_row(
        "      --scan-dir [PATH...]",
        "Scan a library-relative or absolute path",
    )
    help_row("", "Examples: quotes are not required; /music is implied for relative paths")
    help_row("", "    splined --scan-dir Aerosmith")
    help_row("", "    splined --scan-dir 3 Doors Down")
    help_row("", "    splined --scan-dir /music/3 Doors Down")
    print()

    history_items = []
    for source in SUPPORTED_SOURCES:
        history_items.append(
            paint("PURPLE", source)
            + white(" (")
            + blue(str(source_selected_count(history, source)))
            + white(")")
        )
    history_display = white("[") + white(" ").join(history_items) + white("]")
    help_row("      Chosen Source History", history_display)
    help_row(
        "      NO Chosen Fallback History",
        "Normal chosen sources only; fallback selections never increment history",
    )
    help_row(
        "      Source History Mode",
        "count only; all configured sources are queried",
    )
    print()

    print("<SOURCE> Tags:")
    help_row(
        "  -s, --cover-sources",
        "Replace configured cover sources for this run",
    )
    help_row("", green(f"[{','.join(map(str, cover))}]"))
    help_row(
        "  -o, --only-cover-sources",
        "Use only these cover sources for this run",
    )
    help_row("", green("[not set]"))
    help_row(
        "  -e, --exclude-cover-sources",
        "Exclude cover sources for this run",
    )
    help_row("", green(f"[{','.join(map(str, excluded)) if excluded else 'not set'}]"))
    print()

    print("Output Source ART Policy:")
    help_row(
        "      square",
        "Preserve aspect ratio and center-crop longer edge (squared) "
        + bracketed_text(str(bool(output.get("square", False))).lower(), green),
    )
    help_row(
        "      square_mode",
        'Supported: "crop" or "off" '
        + bracketed_text(str(output.get("square_mode", "crop" if output.get("square", False) else "off")), green),
    )
    help_row(
        "      square_round_to",
        "Round the squared side DOWN to the nearest multiple "
        + bracketed_text(str(output.get("square_round_to", 0)), green),
    )
    help_row(
        "      evaluate_final_image",
        "Rank candidates using the image SPLINED would actually write "
        + bracketed_text(str(bool(output.get("evaluate_final_image", output.get("square", False)))).lower(), green),
    )
    print()

    print("API/OAuth:")
    help_row("      --release-mbid", "MusicBrainz release MBID for artwork discovery")
    help_row("      --mb-oauth-login", "Authorize SPLINED with MusicBrainz OAuth")
    help_row("      --lastfm-credentials", "Configure SPLINED Last.fm API credentials")
    help_row("      --lastfm-login", "Authorize SPLINED with a Last.fm user account")
    help_row("      --fanarttv-credentials", "Configure SPLINED Fanart.tv API credentials")
    help_row(
        "      Discogs",
        "Uses [discogs].credential_file personal token; no Discogs application/OAuth required",
    )
    print()

    print("Fallback / Recovery:")
    help_row(
        "      Triggers",
        "No/invalid/conflicting MBID, MB 503/429/timeout, MB lookup failure, or tag/release mismatch",
    )
    help_row(
        "      Auto",
        "If fallback finds a candidate inside the normal configured range, use normal SPLINED scoring and continue without a picker",
    )
    help_row(
        "      *[s]",
        "Purple suggested exception; press s to accept SPLINED's closest-to-ideal manual fallback choice",
    )
    help_row("      [#]", "Choose an exact fallback candidate by number")
    help_row("      [f]", "Edit Artist/Album and run the fallback provider search again")
    help_row("      [m]", "Retry an existing MusicBrainz release lookup or search/pick a MusicBrainz release")
    help_row("      [b]", "Bypass the unresolved album and continue the scan")
    help_row("      " + "-" * 114)
    help_row(
        "      Sources",
        "Fallback can use Deezer, iTunes, Last.fm and Discogs without MusicBrainz authority",
    )
    help_row(
        "      Manual picks",
        "Explicit s/number selections may be outside the normal range; square/resize/output policy still applies",
    )
    help_row(
        "      API priority",
        "All configured sources are queried before final scoring",
    )
    print()

    print("Help:")
    help_row("  -h, --help", "Help, Tips & Config Assistance")
    help_row("  -V, --version", "Version")



def print_config(path: Path, cfg: dict[str, Any]) -> None:
    scan=section(cfg,"scan"); lib=section(cfg,"library"); creds=section(cfg,"credentials"); out=section(cfg,"output")
    print(
        f"Config file: {path}\n"
        f"Config version: {cfg.get('config_version')}\n"
        f"Mode: {cfg.get('mode','read')}\n"
        f"Verbosity: {cfg.get('verbosity','info')}\n"
        f"Music library: {lib.get('music_library','')}\n"
        f"Scan directory: {scan.get('scan_library_dir','')}\n"
        f"Scan mode timeout: {format_timeout_hours(scan_timeout_hours(cfg))} hours\n"
        f"Cache directory: {resolve_path(path,str(scan.get('cache_dir','cache')))}\n"
        f"Credential directory: {resolve_path(path,str(creds.get('credential_dir','credentials')))}\n"
        f"Square output: {out.get('square', False)}\n"
        f"Square mode: {out.get('square_mode', 'crop' if out.get('square', False) else 'off')}\n"
        f"Square round to: {out.get('square_round_to', 0)}\n"
        f"Evaluate final image: {out.get('evaluate_final_image', out.get('square', False))}\n"
        f"Debug log: {resolve_path(path, str(scan.get('cache_dir', 'cache'))) / 'debug' / 'splined_debug.log'}"
    )


def idle() -> int:
    stopping=False
    def stop(signum: int, frame: object) -> None:
        nonlocal stopping; stopping=True
    signal.signal(signal.SIGTERM,stop); signal.signal(signal.SIGINT,stop)
    print(f"{APP_NAME} {VERSION} container ready.\nConfig: {config_path()}\nUse: sudo docker exec -it splined splined -h")
    while not stopping: time.sleep(1)
    print("SPLINED stopping."); return 0


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(add_help=False, allow_abbrev=False)
    p.add_argument("-h", "--help", action="store_true")
    p.add_argument("-V", "--version", action="store_true")
    p.add_argument("--config", action="store_true")
    p.add_argument("--config-check", action="store_true")
    p.add_argument("--config-edit", action="store_true")
    p.add_argument("--scan", action="store_true")
    p.add_argument("--scan-dir", nargs="*", default=None, metavar="PATH")
    p.add_argument("-p", "--preserve-file", choices=["true", "false"])
    p.add_argument("-s", "--cover-sources")
    p.add_argument("-o", "--only-cover-sources")
    p.add_argument("-e", "--exclude-cover-sources")
    p.add_argument("--release-mbid")
    p.add_argument("--mb-oauth-login", action="store_true")
    p.add_argument("--lastfm-credentials", action="store_true")
    p.add_argument("--lastfm-login", action="store_true")
    p.add_argument("--fanarttv-credentials", action="store_true")
    p.add_argument("--idle", action="store_true", help=argparse.SUPPRESS)
    return p


def main() -> int:
    args=parser().parse_args()
    if args.version: print(f"{APP_NAME} {VERSION}"); return 0
    if args.idle: return idle()
    try:
        path,cfg=load_config()
        init_debug_log(path, cfg)
        if args.preserve_file is not None: section(cfg,"output")["preserve_file"] = args.preserve_file=="true"
        if args.help or len(sys.argv)==1: print_help(path,cfg); return 0
        if args.config: print_config(path,cfg); return 0
        if args.config_check: print(f"OK: {path}\nconfig_version = {CONFIG_VERSION}"); return 0
        if args.config_edit: return run_config_edit(path)
        if args.fanarttv_credentials: return configure_fanarttv_credentials(path,cfg)
        if args.lastfm_credentials: return configure_lastfm_credentials(path,cfg)
        if args.lastfm_login: return run_lastfm_login(path,cfg)
        if args.mb_oauth_login: return run_musicbrainz_login(path,cfg)
        sources=resolve_sources(cfg,parse_sources(args.cover_sources),parse_sources(args.only_cover_sources),parse_sources(args.exclude_cover_sources) or [])
        if not sources: raise SplinedError("No SPLINED cover sources remain after exclusions.")
        if args.scan: return run_scan_preview(path,cfg,sources)
        if args.scan_dir is not None: return run_scan_dir(path, cfg, sources, args.scan_dir)
        if args.release_mbid: return run_release_discovery(path,cfg,sources,args.release_mbid)
        print_help(path,cfg); return 0
    except SplinedError as exc:
        print(str(exc),file=sys.stderr); return 2
    except KeyboardInterrupt:
        print("\nSPLINED interrupted.",file=sys.stderr); return 130

if __name__=="__main__": raise SystemExit(main())
