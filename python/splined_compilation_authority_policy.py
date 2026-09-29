"""Recover artwork authority for curated compilations from recording MBIDs.

A user-curated Various Artists compilation can be correctly tagged at track
level while intentionally having no MusicBrainz release ID of its own.  When
``compilation=1`` and every Album MBID is missing, SPLINED may use one valid
MusicBrainz Recording ID to locate that recording's original releases.  An
exact release already represented in ``splined.db`` is preferred; otherwise an
official Artist Album release is selected deterministically.

The recovered release is artwork-reference authority only.  The compilation's
folder/tag Album identity is retained, the UI remains in fallback/reference
mode, and no Album MBID is written back to the music files or SQL compilation
row.
"""

from __future__ import annotations

from pathlib import Path
import sqlite3
import threading
import time
from typing import Any
from urllib.parse import quote

import requests


MAX_RECORDING_PROBES = 8

_LOCK = threading.RLock()
_RECORDING_RELEASE_CACHE: dict[str, list[dict[str, Any]]] = {}
_REFERENCE_RELEASE_IDS: set[str] = set()
_REFERENCE_RELEASES: dict[str, Any] = {}
_INSTALLED = False


def _truthy(value: Any) -> bool:
    return str(value or "").strip().casefold() in {
        "1",
        "true",
        "yes",
        "y",
        "on",
    }


def _credit_text(raw: Any) -> str:
    parts: list[str] = []
    for item in raw if isinstance(raw, list) else []:
        if isinstance(item, str):
            parts.append(item)
        elif isinstance(item, dict):
            artist = item.get("artist")
            name = str(
                item.get("name")
                or (artist.get("name") if isinstance(artist, dict) else "")
                or ""
            )
            parts.append(name)
            parts.append(str(item.get("joinphrase") or ""))
    return "".join(parts).strip()


def _artist_match(core: Any, left: str, right: str) -> bool:
    if not left or not right:
        return False
    try:
        if core.same_text(left, right):
            return True
    except Exception:
        pass
    normalize = getattr(core, "normalize_text", lambda value: str(value).casefold())
    left_normal = str(normalize(left))
    right_normal = str(normalize(right))
    return bool(
        left_normal
        and right_normal
        and (
            left_normal == right_normal
            or left_normal.startswith(right_normal + " ")
            or right_normal.startswith(left_normal + " ")
        )
    )


def _candidate_release_id(release: dict[str, Any]) -> str:
    return str(release.get("id") or "").strip().casefold()


def _candidate_score(
    core: Any,
    release: dict[str, Any],
    *,
    track_artist: str,
    sql_release_ids: set[str],
) -> tuple[Any, ...]:
    release_id = _candidate_release_id(release)
    release_artist = _credit_text(release.get("artist-credit"))
    group = release.get("release-group")
    group = group if isinstance(group, dict) else {}
    primary_type = str(group.get("primary-type") or "").strip().casefold()
    secondary = {
        str(value).strip().casefold()
        for value in group.get("secondary-types", [])
        if str(value).strip()
    }
    secondary_penalty = len(
        secondary
        & {
            "compilation",
            "dj-mix",
            "mixtape/street",
            "soundtrack",
            "live",
            "remix",
        }
    )
    status = str(release.get("status") or "").strip().casefold()
    date = str(release.get("date") or "9999-99-99")
    title = str(release.get("title") or "")
    return (
        0 if release_id in sql_release_ids else 1,
        0 if _artist_match(core, release_artist, track_artist) else 1,
        0 if primary_type == "album" else 1,
        secondary_penalty,
        0 if status == "official" else 1,
        date,
        title.casefold(),
        release_id,
    )


def _database_release_ids(
    core: Any,
    config_file: Path,
    cfg: dict[str, Any],
    release_ids: list[str],
) -> set[str]:
    values = sorted({value.casefold() for value in release_ids if value})
    if not values:
        return set()
    try:
        database_path = core.runtime_cache_dir(config_file, cfg) / "splined.db"
    except Exception:
        return set()
    if not database_path.is_file():
        return set()

    placeholders = ",".join("?" for _value in values)
    uri = f"file:{quote(str(database_path))}?mode=ro"
    connection: sqlite3.Connection | None = None
    try:
        connection = sqlite3.connect(uri, uri=True, timeout=3.0)
        connection.execute("PRAGMA query_only = ON")
        rows = connection.execute(
            "SELECT musicbrainz_albumid FROM albums "
            f"WHERE musicbrainz_albumid IN ({placeholders})",
            values,
        )
        return {
            str(row[0]).strip().casefold()
            for row in rows
            if str(row[0] or "").strip()
        }
    except sqlite3.Error:
        return set()
    finally:
        if connection is not None:
            connection.close()


def _recording_releases(
    core: Any,
    http: Any,
    config_file: Path,
    cfg: dict[str, Any],
    recording_mbid: str,
) -> list[dict[str, Any]]:
    with _LOCK:
        cached = _RECORDING_RELEASE_CACHE.get(recording_mbid)
        if cached is not None:
            return [dict(value) for value in cached]

    headers, _mode = core.mb_headers(config_file, cfg)
    settings = core.musicbrainz_settings(config_file, cfg)
    if not bool(settings.get("enabled", True)):
        return []
    timeout = float(settings.get("mb_recording_timeout", 7.0))
    retry_max = int(settings.get("retry_max", 2))
    min_delay = max(0.0, float(settings.get("mb_min_delay", 1.05)))
    url = f"{core.MB_BASE}/recording/{recording_mbid}"

    for attempt in range(retry_max + 1):
        last_request = getattr(http, "last_mb_request", None)
        if last_request is not None:
            wait = min_delay - (time.monotonic() - float(last_request))
            if wait > 0:
                time.sleep(wait)
        try:
            http.last_mb_request = time.monotonic()
            response = http.get(
                url,
                params={
                    "inc": "artist-credits+releases+release-groups",
                    "fmt": "json",
                },
                headers=headers,
                timeout=timeout,
            )
        except requests.RequestException:
            if attempt < retry_max:
                continue
            return []
        if response.status_code == 404:
            return []
        if response.status_code == 401:
            return []
        if (
            response.status_code == 429
            or 500 <= response.status_code <= 599
        ) and attempt < retry_max:
            continue
        if response.status_code != 200:
            return []
        try:
            payload = response.json()
        except ValueError:
            return []
        releases = payload.get("releases", []) if isinstance(payload, dict) else []
        recording_artist = _credit_text(
            payload.get("artist-credit") if isinstance(payload, dict) else []
        )
        result: list[dict[str, Any]] = []
        for release in releases if isinstance(releases, list) else []:
            if not isinstance(release, dict):
                continue
            value = dict(release)
            value["_recording_artist"] = recording_artist
            result.append(value)
        with _LOCK:
            _RECORDING_RELEASE_CACHE[recording_mbid] = result
        return [dict(value) for value in result]
    return []


def _choose_reference_release(
    core: Any,
    config_file: Path,
    cfg: dict[str, Any],
    releases: list[dict[str, Any]],
    *,
    track_artist: str,
) -> tuple[dict[str, Any] | None, bool]:
    usable = [
        release
        for release in releases
        if _candidate_release_id(release)
        and _artist_match(
            core,
            str(release.get("_recording_artist") or ""),
            track_artist,
        )
    ]
    if not usable:
        return None, False
    ids = [_candidate_release_id(release) for release in usable]
    sql_ids = _database_release_ids(core, config_file, cfg, ids)
    chosen = min(
        usable,
        key=lambda release: _candidate_score(
            core,
            release,
            track_artist=track_artist,
            sql_release_ids=sql_ids,
        ),
    )
    return chosen, _candidate_release_id(chosen) in sql_ids


def _recover_compilation_reference(
    core: Any,
    tracks: list[Any],
) -> tuple[str, Any, bool] | None:
    if not tracks or not any(_truthy(track.compilation) for track in tracks):
        return None

    valid, _missing, invalid = core.audit_album_ids(tracks)
    if valid or invalid:
        return None

    try:
        config_file, cfg = core.load_config()
    except Exception:
        return None
    try:
        http = core.Http()
    except Exception:
        return None

    best: tuple[tuple[Any, ...], dict[str, Any], Any, bool] | None = None
    seen_recordings: set[str] = set()
    probes = 0
    for track in tracks:
        recording_id = core.valid_mbid(track.recording_mbid)
        if not recording_id or recording_id in seen_recordings:
            continue
        seen_recordings.add(recording_id)
        probes += 1
        releases = _recording_releases(
            core,
            http,
            config_file,
            cfg,
            recording_id,
        )
        chosen, in_sql = _choose_reference_release(
            core,
            config_file,
            cfg,
            releases,
            track_artist=str(track.artist or ""),
        )
        if chosen is not None:
            score = _candidate_score(
                core,
                chosen,
                track_artist=str(track.artist or ""),
                sql_release_ids=(
                    {_candidate_release_id(chosen)} if in_sql else set()
                ),
            )
            candidate = (score, chosen, track, in_sql)
            if best is None or candidate[0] < best[0]:
                best = candidate
            if in_sql:
                break
        if probes >= MAX_RECORDING_PROBES:
            break

    if best is None:
        return None
    _score, release, matched_track, in_sql = best
    release_id = _candidate_release_id(release)
    return release_id, matched_track, in_sql


def install(core: Any) -> None:
    """Install compilation recording-reference recovery into the Python engine."""
    global _INSTALLED
    if _INSTALLED or getattr(
        core,
        "_splined_compilation_authority_policy_installed",
        False,
    ):
        return
    _INSTALLED = True

    original_read_album_tracks = core.read_album_tracks
    original_lookup_release = core.lookup_release
    original_discover_fallback = core.discover_fallback

    def read_album_tracks(album: Any) -> list[Any]:
        tracks = original_read_album_tracks(album)
        recovered = _recover_compilation_reference(core, tracks)
        if recovered is None:
            return tracks
        release_id, matched_track, in_sql = recovered
        # Inject one run-local authority hint into the already-read Track
        # object. No file tag or SQL compilation row is modified.
        matched_track.album_mbid = release_id
        with _LOCK:
            _REFERENCE_RELEASE_IDS.add(release_id)
        source = "existing SPLINED SQL release" if in_sql else "MusicBrainz recording release"
        core.debug_log(
            "compilation.reference_recovered "
            f"album={str(album.path)!r} recording={matched_track.recording_mbid!r} "
            f"release={release_id!r} source={source!r}"
        )
        core.emit_ui(
            "activity",
            category="authority",
            state="done",
            source="musicbrainz",
            message=(
                "Compilation artwork reference recovered from Recording ID · "
                f"{matched_track.artist} · {source}"
            ),
        )
        return tracks

    def lookup_release(
        http: Any,
        config_file: Path,
        cfg: dict[str, Any],
        mbid: str,
    ) -> Any:
        release = original_lookup_release(http, config_file, cfg, mbid)
        with _LOCK:
            if mbid.casefold() in _REFERENCE_RELEASE_IDS:
                _REFERENCE_RELEASES[mbid.casefold()] = release
        return release

    def discover_fallback(
        http: Any,
        config_file: Path,
        cfg: dict[str, Any],
        artist: str,
        album: str,
        sources: list[str],
        release_mbid: str | None = None,
        queried_sources: set[str] | None = None,
    ) -> tuple[list[Any], list[tuple[str, str]]]:
        key = str(release_mbid or "").strip().casefold()
        with _LOCK:
            reference = _REFERENCE_RELEASES.get(key)
        if reference is not None:
            core.debug_log(
                "compilation.reference_discovery "
                f"release={key!r} artist={reference.artist_credit!r} "
                f"album={reference.title!r}"
            )
            return core.discover_all(
                http,
                config_file,
                cfg,
                reference,
                sources,
                queried_sources=queried_sources,
            )
        return original_discover_fallback(
            http,
            config_file,
            cfg,
            artist,
            album,
            sources,
            release_mbid=release_mbid,
            queried_sources=queried_sources,
        )

    core.read_album_tracks = read_album_tracks
    core.lookup_release = lookup_release
    core.discover_fallback = discover_fallback
    core._splined_compilation_authority_policy_installed = True
