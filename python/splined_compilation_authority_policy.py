"""Manual, local-first artwork recovery for curated compilation tracks.

The policy is inert during normal scans. A caller must explicitly select
Compilation track-art recovery after the representative track has no Album MBID and is tagged as
a compilation. Curated Album identity is never inferred or changed; only an
approved embedded front image may be replaced.
"""

from __future__ import annotations

import base64
import copy
from dataclasses import dataclass
from functools import partial
import hashlib
import math
import os
from pathlib import Path
import re
import shutil
import sqlite3
import tempfile
import time
from typing import Any, Iterable

from mutagen import File as MutagenFile
from mutagen.flac import FLAC, Picture
from mutagen.id3 import APIC, ID3, ID3NoHeaderError
from mutagen.mp4 import MP4, MP4Cover
import requests

from splined_media_tags import inspect_album_tracks


_MBID_PATTERN = re.compile(
    r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-"
    r"[0-9a-f]{4}-[0-9a-f]{12}\b"
)
_EXCLUDED_SECONDARY = {"dj-mix", "live", "mixtape/street", "remix"}
_INSTALLED = False


@dataclass(frozen=True)
class MusicBrainzOptions:
    retry_max: int
    min_delay: float
    recording_timeout: float


@dataclass(frozen=True)
class ReleaseCandidate:
    recording_mbid: str
    release_mbid: str
    release_group_mbid: str
    release_class: str
    class_rank: int
    candidate_rank: int
    release_title: str
    release_artist: str
    artist_mbids_key: str
    release_date: str


@dataclass(frozen=True)
class ReleaseResolution:
    candidates: tuple[ReleaseCandidate, ...]
    source: str
    error: str = ""
    recording_title: str = ""
    recording_artist: str = ""


@dataclass(frozen=True)
class DiscoveryCandidate:
    """One operator-reviewable Recording search result and its release."""

    recording_mbid: str
    recording_title: str
    recording_artist: str
    artist_mbids_key: str
    release_mbid: str
    release_group_mbid: str
    release_class: str
    class_rank: int
    release_title: str
    release_artist: str
    release_date: str
    country: str
    score: int


def _truthy(value: Any) -> bool:
    return str(value or "").strip().casefold() in {
        "1", "true", "yes", "y", "on"
    }


def _mbids(value: Any) -> set[str]:
    return {
        match.group(0).casefold()
        for match in _MBID_PATTERN.finditer(str(value or ""))
    }


def _artist_key(values: Iterable[str]) -> str:
    return ",".join(sorted({value.casefold() for value in values if value}))


def manual_album_eligible(tracks: list[Any]) -> bool:
    """Eligibility is decided from the representative track only."""
    if not tracks:
        return False
    track = tracks[0]
    return (
        not str(getattr(track, "album_mbid", "") or "").strip()
        and _truthy(getattr(track, "compilation", None))
    )


def manual_track_eligible(track: Any) -> tuple[bool, str]:
    if str(getattr(track, "album_mbid", "") or "").strip():
        return False, "Track has an Album MBID and belongs in normal scan mode"
    if not _truthy(getattr(track, "compilation", None)):
        return False, "Track is not tagged compilation=1"
    return True, ""


def _lucene_phrase(value: Any) -> str:
    """Quote local text for a MusicBrainz/Lucene phrase query."""
    return str(value or "").replace("\\", "\\\\").replace('"', '\\"').strip()


def credential_options(
    core: Any,
    config_file: Path,
    cfg: dict[str, Any],
) -> MusicBrainzOptions:
    """Read the exact credential JSON option names without fallback aliases."""
    path = core.credential_file(config_file, cfg, "musicbrainz")
    credential = core.load_json(path, "MusicBrainz")
    options = credential.get("options")
    if not isinstance(options, dict):
        raise core.SplinedError(
            "MusicBrainz credential options must be a JSON object."
        )
    names = ("retry_max", "min_delay", "recording_timeout")
    missing = [name for name in names if name not in options]
    if missing:
        raise core.SplinedError(
            "MusicBrainz credential options are missing: " + ", ".join(missing)
        )
    if isinstance(options["retry_max"], bool):
        raise core.SplinedError(
            "MusicBrainz credential option retry_max must be an integer."
        )
    try:
        retry_max = int(options["retry_max"])
        min_delay = float(options["min_delay"])
        recording_timeout = float(options["recording_timeout"])
    except (TypeError, ValueError) as exc:
        raise core.SplinedError(
            "MusicBrainz credential options contain invalid numeric values."
        ) from exc
    if retry_max < 0:
        raise core.SplinedError(
            "MusicBrainz credential option retry_max cannot be negative."
        )
    if not math.isfinite(min_delay) or min_delay <= 0:
        raise core.SplinedError(
            "MusicBrainz credential option min_delay must be finite and greater than 0."
        )
    if not math.isfinite(recording_timeout) or recording_timeout <= 0:
        raise core.SplinedError(
            "MusicBrainz credential option recording_timeout must be finite and greater than 0."
        )
    return MusicBrainzOptions(retry_max, min_delay, recording_timeout)


def _database_path(core: Any, config_file: Path, cfg: dict[str, Any]) -> Path:
    return core.runtime_cache_dir(config_file, cfg) / "splined.db"


def local_artwork_rows(
    core: Any,
    config_file: Path,
    cfg: dict[str, Any],
    track: Any,
    *,
    current_album_path: Path,
    release_mbid: str = "",
) -> list[dict[str, Any]]:
    """Resolve exact local IDs lazily, only inside compilation track-art work.

    The normal media index must remain representative-track based. This helper
    first checks its persistent SQL cache, then limits tag reads to albums
    already indexed under the requested Artist MBID.
    """
    recordings = _mbids(getattr(track, "recording_mbid", None))
    artists = _mbids(getattr(track, "artist_mbid", None))
    if len(recordings) != 1 or not artists:
        return []
    database = _database_path(core, config_file, cfg)
    if not database.is_file():
        return []
    recording_mbid = next(iter(recordings))
    artist_mbids = sorted(artists)
    artist_mbids_key = _artist_key(artist_mbids)
    release_ids = _mbids(release_mbid)
    exact_release_mbid = next(iter(release_ids), "")
    artist_placeholders = ",".join("?" for _value in artist_mbids)
    connection: sqlite3.Connection | None = None
    try:
        connection = sqlite3.connect(database, timeout=10.0)
        connection.row_factory = sqlite3.Row
        release_clause = (
            "AND lower(al.musicbrainz_albumid)=? "
            if exact_release_mbid
            else ""
        )
        base_select = (
            "SELECT t.path AS track_path, t.title AS track_title, "
            "t.artist_name AS track_artist, al.album_key, al.album_name, "
            "al.musicbrainz_albumid, al.musicbrainz_releasegroupid, "
            "al.release_year, al.compilation, al.path AS album_path, "
            "al.cover_path, al.cover_format, al.cover_width, al.cover_height, "
            "ar.artist_name AS album_artist, t.file_size, t.file_mtime_ns "
            "FROM tracks t "
            "JOIN albums al ON al.album_key=t.album_key "
            "JOIN artists ar ON ar.artist_key=al.artist_key "
            "WHERE lower(t.musicbrainz_recordingid)=? "
            "AND lower(t.musicbrainz_artistid)=? "
            f"AND lower(ar.musicbrainz_artistid) IN ({artist_placeholders}) "
            "AND trim(al.musicbrainz_albumid)<>'' "
            f"{release_clause}"
            "AND al.cover_found=1 AND trim(al.cover_path)<>'' "
            "AND lower(al.path)<>lower(?) "
            "ORDER BY al.compilation ASC, al.release_year ASC, "
            "al.album_name COLLATE NOCASE, al.path COLLATE NOCASE"
        )
        params = (
            recording_mbid,
            artist_mbids_key,
            *artist_mbids,
            *((exact_release_mbid,) if exact_release_mbid else ()),
            str(current_album_path),
        )

        def usable(rows: Iterable[sqlite3.Row]) -> list[dict[str, Any]]:
            found: list[dict[str, Any]] = []
            for row in rows:
                track_path = Path(str(row["track_path"] or ""))
                cover_path = Path(str(row["cover_path"] or ""))
                try:
                    stat = track_path.stat()
                except OSError:
                    continue
                if (
                    int(row["file_size"] or 0) != int(stat.st_size)
                    or int(row["file_mtime_ns"] or 0) != int(stat.st_mtime_ns)
                    or not cover_path.is_file()
                ):
                    continue
                found.append(dict(row))
            return found

        cached = usable(connection.execute(base_select, params))
        if cached:
            return cached[:1]

        candidate_albums = list(
            connection.execute(
                "SELECT al.album_key, al.album_name, al.musicbrainz_albumid, "
                "al.musicbrainz_releasegroupid, al.release_year, "
                "al.compilation, al.path AS album_path, al.cover_path, "
                "al.cover_format, al.cover_width, al.cover_height, "
                "ar.artist_name AS album_artist "
                "FROM albums al JOIN artists ar "
                "ON ar.artist_key=al.artist_key "
                f"WHERE lower(ar.musicbrainz_artistid) IN ({artist_placeholders}) "
                "AND trim(al.musicbrainz_albumid)<>'' "
                f"{release_clause}"
                "AND al.cover_found=1 AND trim(al.cover_path)<>'' "
                "AND lower(al.path)<>lower(?) "
                "ORDER BY al.compilation ASC, al.release_year ASC, "
                "al.album_name COLLATE NOCASE, al.path COLLATE NOCASE",
                (
                    *artist_mbids,
                    *((exact_release_mbid,) if exact_release_mbid else ()),
                    str(current_album_path),
                ),
            )
        )
        extensions = {
            str(value).casefold()
            for value in getattr(core, "AUDIO_EXTENSIONS", set())
        }
        version = str(core.display_version())
        for album_row in candidate_albums:
            album_path = Path(str(album_row["album_path"] or ""))
            cover_path = Path(str(album_row["cover_path"] or ""))
            if not album_path.is_dir() or not cover_path.is_file():
                continue
            try:
                audio_files = sorted(
                    (
                        path
                        for path in album_path.iterdir()
                        if path.is_file()
                        and path.suffix.casefold() in extensions
                    ),
                    key=lambda path: path.name.casefold(),
                )
            except OSError:
                continue
            if not audio_files:
                continue
            existing = {
                str(row["path"]): row
                for row in connection.execute(
                    "SELECT * FROM tracks WHERE album_key=?",
                    (str(album_row["album_key"]),),
                )
            }
            lazy_album = type(
                "LazyManualAlbum",
                (),
                {"audio_files": audio_files},
            )()
            track_rows, _reads, _reuses = inspect_album_tracks(
                lazy_album,
                str(album_row["album_key"]),
                existing,
            )
            with connection:
                connection.execute(
                    "DELETE FROM tracks WHERE album_key=?",
                    (str(album_row["album_key"]),),
                )
                connection.executemany(
                    "INSERT INTO tracks"
                    "(track_key, album_key, path, title, artist_name, "
                    "musicbrainz_recordingid, musicbrainz_artistid, "
                    "file_size, file_mtime_ns, updated_at, splined_version) "
                    "VALUES(:track_key, :album_key, :path, :title, "
                    ":artist_name, :musicbrainz_recordingid, "
                    ":musicbrainz_artistid, :file_size, :file_mtime_ns, "
                    ":updated_at, :splined_version)",
                    [
                        dict(row, splined_version=version)
                        for row in track_rows
                    ],
                )
            matches = [
                row
                for row in track_rows
                if row["musicbrainz_recordingid"] == recording_mbid
                and row["musicbrainz_artistid"] == artist_mbids_key
            ]
            if matches:
                result = dict(album_row)
                result.update(
                    track_path=matches[0]["path"],
                    track_title=matches[0]["title"],
                    track_artist=matches[0]["artist_name"],
                    file_size=matches[0]["file_size"],
                    file_mtime_ns=matches[0]["file_mtime_ns"],
                )
                return [result]
        return []
    except sqlite3.Error:
        return []
    finally:
        if connection is not None:
            connection.close()


def _cached_releases(
    core: Any,
    config_file: Path,
    cfg: dict[str, Any],
    recording_mbid: str,
    artist_mbids_key: str,
) -> tuple[ReleaseCandidate, ...]:
    database = _database_path(core, config_file, cfg)
    if not database.is_file():
        return ()
    connection: sqlite3.Connection | None = None
    try:
        connection = sqlite3.connect(database, timeout=3.0)
        connection.row_factory = sqlite3.Row
        rows = connection.execute(
            "SELECT c.* FROM recording_release_candidates c "
            "JOIN recording_release_lookups l "
            "ON l.recording_mbid=c.recording_mbid "
            "WHERE lower(c.recording_mbid)=lower(?) "
            "AND c.artist_mbids_key=? AND l.artist_mbids_key=? "
            "ORDER BY c.class_rank, c.candidate_rank",
            (recording_mbid, artist_mbids_key, artist_mbids_key),
        )
        return tuple(
            ReleaseCandidate(
                str(row["recording_mbid"]),
                str(row["release_mbid"]),
                str(row["release_group_mbid"] or ""),
                str(row["release_class"]),
                int(row["class_rank"]),
                int(row["candidate_rank"]),
                str(row["release_title"]),
                str(row["release_artist"]),
                str(row["artist_mbids_key"]),
                str(row["release_date"] or ""),
            )
            for row in rows
        )
    except sqlite3.Error:
        return ()
    finally:
        if connection is not None:
            connection.close()


def _cache_releases(
    core: Any,
    config_file: Path,
    cfg: dict[str, Any],
    candidates: tuple[ReleaseCandidate, ...],
) -> None:
    if not candidates:
        return
    database = _database_path(core, config_file, cfg)
    connection = sqlite3.connect(database, timeout=10.0)
    try:
        first = candidates[0]
        now = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        with connection:
            connection.execute(
                "INSERT INTO recording_release_lookups"
                "(recording_mbid, artist_mbids_key, fetched_at, splined_version) "
                "VALUES(?, ?, ?, ?) "
                "ON CONFLICT(recording_mbid) DO UPDATE SET "
                "artist_mbids_key=excluded.artist_mbids_key, "
                "fetched_at=excluded.fetched_at, "
                "splined_version=excluded.splined_version",
                (
                    first.recording_mbid,
                    first.artist_mbids_key,
                    now,
                    str(core.display_version()),
                ),
            )
            connection.execute(
                "DELETE FROM recording_release_candidates "
                "WHERE lower(recording_mbid)=lower(?)",
                (first.recording_mbid,),
            )
            connection.executemany(
                "INSERT INTO recording_release_candidates"
                "(recording_mbid, release_mbid, release_group_mbid, "
                "release_class, class_rank, candidate_rank, release_title, "
                "release_artist, artist_mbids_key, release_date) "
                "VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                [
                    (
                        item.recording_mbid,
                        item.release_mbid,
                        item.release_group_mbid,
                        item.release_class,
                        item.class_rank,
                        item.candidate_rank,
                        item.release_title,
                        item.release_artist,
                        item.artist_mbids_key,
                        item.release_date,
                    )
                    for item in candidates
                ],
            )
    finally:
        connection.close()


def _credit_text(raw: Any) -> str:
    parts: list[str] = []
    for item in raw if isinstance(raw, list) else []:
        if isinstance(item, str):
            parts.append(item)
        elif isinstance(item, dict):
            artist = item.get("artist")
            parts.append(
                str(
                    item.get("name")
                    or (artist.get("name") if isinstance(artist, dict) else "")
                    or ""
                )
            )
            parts.append(str(item.get("joinphrase") or ""))
    return "".join(parts).strip()


def _credit_mbids(raw: Any) -> set[str]:
    result: set[str] = set()
    for item in raw if isinstance(raw, list) else []:
        if isinstance(item, dict) and isinstance(item.get("artist"), dict):
            result.update(_mbids(item["artist"].get("id")))
    return result


def _release_class(release: dict[str, Any]) -> tuple[str, int] | None:
    group = release.get("release-group")
    group = group if isinstance(group, dict) else {}
    primary = str(group.get("primary-type") or "").strip().casefold()
    secondary = {
        str(value).strip().casefold()
        for value in group.get("secondary-types", [])
        if str(value).strip()
    }
    if secondary & _EXCLUDED_SECONDARY:
        return None
    if "soundtrack" in secondary:
        return "soundtrack", 1
    if "compilation" in secondary:
        return "compilation", 2
    if primary == "album" and not secondary:
        return "album", 0
    return None


def _rank_releases(
    payload: dict[str, Any],
    recording_mbid: str,
    artist_mbids_key: str,
) -> tuple[ReleaseCandidate, ...]:
    ranked: list[tuple[tuple[Any, ...], dict[str, Any], str, int]] = []
    for release in payload.get("releases", []):
        if not isinstance(release, dict):
            continue
        release_ids = _mbids(release.get("id"))
        classification = _release_class(release)
        if (
            len(release_ids) != 1
            or classification is None
            or str(release.get("status") or "").strip().casefold() != "official"
        ):
            continue
        release_class, class_rank = classification
        title = str(release.get("title") or "").strip()
        date = str(release.get("date") or "")
        release_id = next(iter(release_ids))
        ranked.append(
            (
                (class_rank, date or "9999-99-99", title.casefold(), release_id),
                release,
                release_class,
                class_rank,
            )
        )
    ranked.sort(key=lambda value: value[0])
    selected: list[ReleaseCandidate] = []
    seen_classes: set[str] = set()
    for _score, release, release_class, class_rank in ranked:
        if release_class in seen_classes:
            continue
        seen_classes.add(release_class)
        group = release.get("release-group")
        group = group if isinstance(group, dict) else {}
        selected.append(
            ReleaseCandidate(
                recording_mbid,
                next(iter(_mbids(release.get("id")))),
                next(iter(_mbids(group.get("id"))), ""),
                release_class,
                class_rank,
                len(selected),
                str(release.get("title") or "").strip(),
                _credit_text(release.get("artist-credit")),
                artist_mbids_key,
                str(release.get("date") or ""),
            )
        )
    return tuple(selected)


def resolve_release_candidates(
    core: Any,
    http: Any,
    config_file: Path,
    cfg: dict[str, Any],
    track: Any,
    options: MusicBrainzOptions,
    *,
    force_refresh: bool = False,
) -> ReleaseResolution:
    recordings = _mbids(getattr(track, "recording_mbid", None))
    artists = _mbids(getattr(track, "artist_mbid", None))
    if len(recordings) != 1 or not artists:
        return ReleaseResolution((), "none", "Missing authoritative local IDs")
    recording_mbid = next(iter(recordings))
    artist_mbids_key = _artist_key(artists)
    if not force_refresh:
        cached = _cached_releases(
            core, config_file, cfg, recording_mbid, artist_mbids_key
        )
        if cached:
            return ReleaseResolution(cached, "sql-cache")
    if options.retry_max == 0:
        return ReleaseResolution(
            (), "musicbrainz", "MusicBrainz attempt budget is zero"
        )

    try:
        headers, _mode = core.mb_headers(config_file, cfg)
    except Exception as exc:
        return ReleaseResolution(
            (),
            "musicbrainz",
            "MusicBrainz authentication/refresh failed: "
            f"{type(exc).__name__}",
        )
    url = f"{core.MB_BASE}/recording/{recording_mbid}"
    last_error = ""
    for attempt in range(options.retry_max):
        last_request = getattr(http, "last_mb_request", None)
        if last_request is not None:
            wait = options.min_delay - (time.monotonic() - float(last_request))
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
                timeout=options.recording_timeout,
            )
        except requests.RequestException as exc:
            last_error = f"MusicBrainz transport failure: {type(exc).__name__}"
            if attempt + 1 < options.retry_max:
                continue
            break
        status = int(response.status_code)
        if status == 404:
            return ReleaseResolution(
                (), "musicbrainz", "MusicBrainz Recording ID was not found"
            )
        if status in {401, 403}:
            return ReleaseResolution(
                (), "musicbrainz", "MusicBrainz authentication was rejected"
            )
        if status == 429 or 500 <= status <= 599:
            last_error = f"MusicBrainz returned HTTP {status}"
            if attempt + 1 < options.retry_max:
                continue
            break
        if status != 200:
            return ReleaseResolution(
                (), "musicbrainz", f"MusicBrainz returned HTTP {status}"
            )
        try:
            payload = response.json()
        except ValueError:
            return ReleaseResolution(
                (), "musicbrainz", "MusicBrainz returned invalid JSON"
            )
        if not isinstance(payload, dict):
            return ReleaseResolution(
                (), "musicbrainz", "MusicBrainz returned invalid Recording data"
            )
        if not artists.issubset(_credit_mbids(payload.get("artist-credit"))):
            return ReleaseResolution(
                (),
                "musicbrainz",
                "Recording Artist ID does not match the local Artist ID",
            )
        candidates = _rank_releases(
            payload, recording_mbid, artist_mbids_key
        )
        if not candidates:
            return ReleaseResolution(
                (),
                "musicbrainz",
                "No Album, Soundtrack, or Compilation release was found",
            )
        try:
            _cache_releases(core, config_file, cfg, candidates)
        except sqlite3.Error:
            # A busy or unavailable cache must not discard an otherwise valid
            # bounded lookup. The next compilation scan may perform it again.
            pass
        return ReleaseResolution(
            candidates,
            "musicbrainz",
            recording_title=str(payload.get("title") or "").strip(),
            recording_artist=_credit_text(payload.get("artist-credit")),
        )
    return ReleaseResolution(
        (),
        "musicbrainz",
        last_error or "MusicBrainz attempt budget exhausted",
    )


def discover_recording_releases(
    core: Any,
    http: Any,
    config_file: Path,
    cfg: dict[str, Any],
    track: Any,
    options: MusicBrainzOptions,
) -> tuple[tuple[DiscoveryCandidate, ...], str]:
    """Search by local Artist/Title for an explicit Manual-mode decision.

    The curated compilation Album is deliberately excluded from the query.
    MusicBrainz's default result limit keeps this single request bounded; the
    caller caches the returned choices for back-navigation within the run.
    """
    artist = _lucene_phrase(getattr(track, "artist", ""))
    title = _lucene_phrase(getattr(track, "title", ""))
    if not artist or not title:
        return (), "Manual MusicBrainz search requires local Artist and Title"
    if options.retry_max == 0:
        return (), "MusicBrainz attempt budget is zero"
    try:
        headers, _mode = core.mb_headers(config_file, cfg)
    except Exception as exc:
        return (), (
            "MusicBrainz authentication/refresh failed: "
            f"{type(exc).__name__}"
        )

    query = f'recording:"{title}" AND artist:"{artist}" AND status:official'
    url = f"{core.MB_BASE}/recording/"
    last_error = ""
    for attempt in range(options.retry_max):
        last_request = getattr(http, "last_mb_request", None)
        if last_request is not None:
            wait = options.min_delay - (time.monotonic() - float(last_request))
            if wait > 0:
                time.sleep(wait)
        try:
            http.last_mb_request = time.monotonic()
            response = http.get(
                url,
                params={"query": query, "fmt": "json"},
                headers=headers,
                timeout=options.recording_timeout,
            )
        except requests.RequestException as exc:
            last_error = f"MusicBrainz transport failure: {type(exc).__name__}"
            if attempt + 1 < options.retry_max:
                continue
            break
        status = int(response.status_code)
        if status in {401, 403}:
            return (), "MusicBrainz authentication was rejected"
        if status == 429 or 500 <= status <= 599:
            last_error = f"MusicBrainz returned HTTP {status}"
            if attempt + 1 < options.retry_max:
                continue
            break
        if status != 200:
            return (), f"MusicBrainz returned HTTP {status}"
        try:
            payload = response.json()
        except ValueError:
            return (), "MusicBrainz returned invalid JSON"
        if not isinstance(payload, dict):
            return (), "MusicBrainz returned invalid Recording search data"

        found: list[DiscoveryCandidate] = []
        seen: set[tuple[str, str]] = set()
        for recording in payload.get("recordings", []):
            if not isinstance(recording, dict):
                continue
            recording_ids = _mbids(recording.get("id"))
            artist_ids = _credit_mbids(recording.get("artist-credit"))
            if len(recording_ids) != 1 or not artist_ids:
                continue
            recording_id = next(iter(recording_ids))
            recording_artist = _credit_text(recording.get("artist-credit"))
            artist_key = _artist_key(artist_ids)
            try:
                score = int(recording.get("score") or 0)
            except (TypeError, ValueError):
                score = 0
            for release in recording.get("releases", []):
                if not isinstance(release, dict):
                    continue
                release_ids = _mbids(release.get("id"))
                classification = _release_class(release)
                if (
                    len(release_ids) != 1
                    or classification is None
                    or str(release.get("status") or "").strip().casefold()
                    != "official"
                ):
                    continue
                release_id = next(iter(release_ids))
                identity = (recording_id, release_id)
                if identity in seen:
                    continue
                seen.add(identity)
                release_class, class_rank = classification
                group = release.get("release-group")
                group = group if isinstance(group, dict) else {}
                release_artist = _credit_text(release.get("artist-credit"))
                found.append(
                    DiscoveryCandidate(
                        recording_id,
                        str(recording.get("title") or "").strip(),
                        recording_artist,
                        artist_key,
                        release_id,
                        next(iter(_mbids(group.get("id"))), ""),
                        release_class,
                        class_rank,
                        str(release.get("title") or "").strip(),
                        release_artist or recording_artist,
                        str(release.get("date") or "").strip(),
                        str(release.get("country") or "").strip(),
                        score,
                    )
                )
        found.sort(
            key=lambda item: (
                item.recording_artist.casefold(),
                (item.release_date[:3] + "0s")
                if len(item.release_date) >= 4 and item.release_date[:4].isdigit()
                else "Unknown",
                item.class_rank,
                item.release_date or "9999-99-99",
                -item.score,
                item.release_title.casefold(),
                item.release_mbid,
            )
        )
        if not found:
            return (), (
                "No Official Album, Soundtrack, or Compilation match was found"
            )
        return tuple(found), ""
    return (), last_error or "MusicBrainz attempt budget exhausted"


def release_candidate_from_discovery(
    item: DiscoveryCandidate,
) -> ReleaseCandidate:
    return ReleaseCandidate(
        item.recording_mbid,
        item.release_mbid,
        item.release_group_mbid,
        item.release_class,
        item.class_rank,
        0,
        item.release_title,
        item.release_artist,
        item.artist_mbids_key,
        item.release_date,
    )


def resolve_release_by_id(
    core: Any,
    http: Any,
    config_file: Path,
    cfg: dict[str, Any],
    track: Any,
    release_mbid: str,
    options: MusicBrainzOptions,
) -> ReleaseResolution:
    """Resolve one operator-supplied Release MBID without changing tags."""
    releases = _mbids(release_mbid)
    recordings = _mbids(getattr(track, "recording_mbid", None))
    artists = _mbids(getattr(track, "artist_mbid", None))
    if len(releases) != 1 or len(recordings) != 1 or not artists:
        return ReleaseResolution((), "none", "Missing authoritative compilation IDs")
    if options.retry_max == 0:
        return ReleaseResolution(
            (), "musicbrainz", "MusicBrainz attempt budget is zero"
        )
    release_id = next(iter(releases))
    recording_id = next(iter(recordings))
    artist_mbids_key = _artist_key(artists)
    try:
        headers, _mode = core.mb_headers(config_file, cfg)
    except Exception as exc:
        return ReleaseResolution(
            (),
            "musicbrainz",
            "MusicBrainz authentication/refresh failed: "
            f"{type(exc).__name__}",
        )
    url = f"{core.MB_BASE}/release/{release_id}"
    last_error = ""
    for attempt in range(options.retry_max):
        last_request = getattr(http, "last_mb_request", None)
        if last_request is not None:
            wait = options.min_delay - (time.monotonic() - float(last_request))
            if wait > 0:
                time.sleep(wait)
        try:
            http.last_mb_request = time.monotonic()
            response = http.get(
                url,
                params={
                    "inc": "artist-credits+recordings+release-groups",
                    "fmt": "json",
                },
                headers=headers,
                timeout=options.recording_timeout,
            )
        except requests.RequestException as exc:
            last_error = f"MusicBrainz transport failure: {type(exc).__name__}"
            if attempt + 1 < options.retry_max:
                continue
            break
        status = int(response.status_code)
        if status == 404:
            return ReleaseResolution(
                (), "musicbrainz", "MusicBrainz Release ID was not found"
            )
        if status in {401, 403}:
            return ReleaseResolution(
                (), "musicbrainz", "MusicBrainz authentication was rejected"
            )
        if status == 429 or 500 <= status <= 599:
            last_error = f"MusicBrainz returned HTTP {status}"
            if attempt + 1 < options.retry_max:
                continue
            break
        if status != 200:
            return ReleaseResolution(
                (), "musicbrainz", f"MusicBrainz returned HTTP {status}"
            )
        try:
            payload = response.json()
        except ValueError:
            return ReleaseResolution(
                (), "musicbrainz", "MusicBrainz returned invalid JSON"
            )
        if not isinstance(payload, dict):
            return ReleaseResolution(
                (), "musicbrainz", "MusicBrainz returned invalid Release data"
            )
        if str(payload.get("status") or "").strip().casefold() != "official":
            return ReleaseResolution(
                (),
                "musicbrainz",
                "Release is not Official",
            )
        matched_recording: dict[str, Any] | None = None
        for medium in payload.get("media", []):
            if not isinstance(medium, dict):
                continue
            for item in medium.get("tracks", []):
                if not isinstance(item, dict):
                    continue
                recording = item.get("recording")
                if (
                    isinstance(recording, dict)
                    and recording_id in _mbids(recording.get("id"))
                ):
                    matched_recording = recording
                    break
            if matched_recording is not None:
                break
        if matched_recording is None:
            return ReleaseResolution(
                (),
                "musicbrainz",
                "Release does not contain the selected Recording ID",
            )
        if not artists.issubset(
            _credit_mbids(matched_recording.get("artist-credit"))
        ):
            return ReleaseResolution(
                (),
                "musicbrainz",
                "Release recording Artist ID does not match the selected Artist ID",
            )
        group = payload.get("release-group")
        group = group if isinstance(group, dict) else {}
        classification = _release_class(payload)
        if classification is not None:
            release_class, class_rank = classification
        else:
            primary_type = (
                str(group.get("primary-type") or "release")
                .strip()
                .casefold()
            )
            release_class = primary_type or "release"
            class_rank = 3
        candidate = ReleaseCandidate(
            recording_id,
            release_id,
            next(iter(_mbids(group.get("id"))), ""),
            release_class,
            class_rank,
            0,
            str(payload.get("title") or "").strip(),
            _credit_text(payload.get("artist-credit")),
            artist_mbids_key,
            str(payload.get("date") or ""),
        )
        return ReleaseResolution(
            (candidate,),
            "musicbrainz-manual-release",
            recording_title=str(matched_recording.get("title") or "").strip(),
            recording_artist=_credit_text(
                matched_recording.get("artist-credit")
            ),
        )
    return ReleaseResolution(
        (),
        "musicbrainz",
        last_error or "MusicBrainz attempt budget exhausted",
    )


def release_from_local_row(core: Any, row: dict[str, Any]) -> Any:
    return core.Release(
        str(row.get("musicbrainz_albumid") or ""),
        str(row.get("album_name") or ""),
        str(row.get("album_artist") or ""),
        str(row.get("musicbrainz_releasegroupid") or "") or None,
        str(row.get("album_name") or "") or None,
        None,
    )


def release_from_candidate(core: Any, item: ReleaseCandidate) -> Any:
    return core.Release(
        item.release_mbid,
        item.release_title,
        item.release_artist,
        item.release_group_mbid or None,
        item.release_title or None,
        None,
    )


def prepare_embedded_artwork(
    core: Any,
    candidate: Any,
    cfg: dict[str, Any],
) -> tuple[bytes, dict[str, Any], str]:
    """Apply normal image policy while capping output at configured ladder."""
    range_cfg = core.section(cfg, "range")
    if "ladder" not in range_cfg:
        raise core.SplinedError(
            "Config v5 [range].ladder is required for compilation track art."
        )
    try:
        ladder = int(range_cfg["ladder"])
    except (TypeError, ValueError) as exc:
        raise core.SplinedError(
            "Config v5 [range].ladder must be an integer."
        ) from exc
    if ladder <= 0:
        raise core.SplinedError(
            "Config v5 [range].ladder must be greater than 0."
        )
    manual_cfg = copy.deepcopy(cfg)
    manual_cfg.setdefault("range", {})["ideal"] = ladder
    manual_cfg.setdefault("output", {})["upscale_below_ideal"] = False
    manual_cfg["output"]["evaluate_final_image"] = True
    target = (
        str(candidate.format).casefold()
        if str(candidate.format).casefold() in {"jpeg", "png"}
        else core.formats(cfg)[0]
    )
    content, info = core.prepare_final(
        candidate, manual_cfg, target, allow_out_of_range=True
    )
    return content, info, target


def _replace_id3_front(path: Path, data: bytes, mime: str) -> None:
    try:
        tags = ID3(path)
    except ID3NoHeaderError:
        tags = ID3()
    preserved = [
        value for value in tags.getall("APIC")
        if int(getattr(value, "type", 0) or 0) != 3
    ]
    tags.delall("APIC")
    for value in preserved:
        tags.add(value)
    tags.add(APIC(encoding=3, mime=mime, type=3, desc="Cover", data=data))
    tags.save(path, v2_version=3)


def _replace_flac_front(
    path: Path,
    data: bytes,
    mime: str,
    info: dict[str, Any],
) -> None:
    audio = FLAC(path)
    preserved = [
        value for value in audio.pictures
        if int(getattr(value, "type", 0) or 0) != 3
    ]
    picture = Picture()
    picture.type = 3
    picture.mime = mime
    picture.desc = "Cover"
    picture.width = int(info.get("width", 0) or 0)
    picture.height = int(info.get("height", 0) or 0)
    picture.depth = 24
    picture.data = data
    audio.clear_pictures()
    for value in preserved:
        audio.add_picture(value)
    audio.add_picture(picture)
    audio.save()


def _replace_mp4_front(path: Path, data: bytes, image_format: str) -> None:
    audio = MP4(path)
    if audio.tags is None:
        audio.add_tags()
    assert audio.tags is not None
    kind = (
        MP4Cover.FORMAT_PNG
        if image_format == "png"
        else MP4Cover.FORMAT_JPEG
    )
    audio.tags["covr"] = [MP4Cover(data, imageformat=kind)]
    audio.save()


def _replace_vorbis_front(
    path: Path,
    data: bytes,
    mime: str,
    info: dict[str, Any],
) -> None:
    audio = MutagenFile(path)
    if audio is None:
        raise ValueError("unsupported Vorbis-family file")
    if audio.tags is None:
        audio.add_tags()
    picture = Picture()
    picture.type = 3
    picture.mime = mime
    picture.desc = "Cover"
    picture.width = int(info.get("width", 0) or 0)
    picture.height = int(info.get("height", 0) or 0)
    picture.depth = 24
    picture.data = data
    preserved: list[str] = []
    for encoded in list(audio.tags.get("metadata_block_picture", [])):
        try:
            existing = Picture(base64.b64decode(encoded))
        except Exception:
            continue
        if int(existing.type or 0) != 3:
            preserved.append(encoded)
    preserved.append(base64.b64encode(picture.write()).decode("ascii"))
    audio.tags["metadata_block_picture"] = preserved
    for key in ("coverart", "coverartmime"):
        if key in audio.tags:
            del audio.tags[key]
    audio.save()


def replace_embedded_artwork(
    track_path: Path,
    data: bytes,
    image_format: str,
    info: dict[str, Any],
) -> None:
    """Mutate a staged copy and atomically replace the complete audio file."""
    path = Path(track_path)
    suffix = path.suffix.casefold()
    descriptor, staged_name = tempfile.mkstemp(
        prefix=f".{path.name}.splined-",
        suffix=suffix,
        dir=path.parent,
    )
    os.close(descriptor)
    staged = Path(staged_name)
    mime = "image/png" if image_format == "png" else "image/jpeg"
    try:
        shutil.copy2(path, staged)
        if suffix in {".mp3", ".wav", ".aiff", ".aif"}:
            _replace_id3_front(staged, data, mime)
        elif suffix == ".flac":
            _replace_flac_front(staged, data, mime, info)
        elif suffix in {".m4a", ".mp4"}:
            _replace_mp4_front(staged, data, image_format)
        elif suffix in {".ogg", ".opus"}:
            _replace_vorbis_front(staged, data, mime, info)
        else:
            raise ValueError(f"unsupported embedded-artwork format: {suffix}")
        os.replace(staged, path)
    finally:
        staged.unlink(missing_ok=True)


def record_application(
    core: Any,
    config_file: Path,
    cfg: dict[str, Any],
    track: Any,
    *,
    source_kind: str,
    source_locator: str,
    release_mbid: str,
    artwork: bytes,
    outcome: str,
    album_path: str = "",
    total_tracks: int = 0,
    completed_tracks: int = 0,
) -> None:
    recording = next(iter(_mbids(getattr(track, "recording_mbid", None))), "")
    artist = _artist_key(_mbids(getattr(track, "artist_mbid", None)))
    if not recording or not artist:
        return
    connection = sqlite3.connect(
        _database_path(core, config_file, cfg), timeout=10.0
    )
    try:
        with connection:
            _ensure_progress_table(connection)
            connection.execute(
                "INSERT INTO compilation_track_artwork"
                "(track_path, recording_mbid, artist_mbid, source_kind, "
                "source_locator, release_mbid, artwork_sha256, outcome, "
                "applied_at, splined_version) "
                "VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?) "
                "ON CONFLICT(track_path) DO UPDATE SET "
                "recording_mbid=excluded.recording_mbid, "
                "artist_mbid=excluded.artist_mbid, "
                "source_kind=excluded.source_kind, "
                "source_locator=excluded.source_locator, "
                "release_mbid=excluded.release_mbid, "
                "artwork_sha256=excluded.artwork_sha256, "
                "outcome=excluded.outcome, "
                "applied_at=excluded.applied_at, "
                "splined_version=excluded.splined_version",
                (
                    str(track.path),
                    recording,
                    artist,
                    source_kind,
                    source_locator,
                    release_mbid or None,
                    hashlib.sha256(artwork).hexdigest(),
                    outcome,
                    time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                    str(core.display_version()),
                ),
            )
            if album_path and total_tracks > 0 and completed_tracks > 0:
                _write_progress(
                    connection,
                    album_path,
                    total_tracks,
                    completed_tracks,
                    str(core.display_version()),
                )
    finally:
        connection.close()


def _ensure_progress_table(connection: sqlite3.Connection) -> None:
    connection.execute(
        "CREATE TABLE IF NOT EXISTS compilation_album_progress ("
        "album_path TEXT PRIMARY KEY COLLATE NOCASE, "
        "total_tracks INTEGER NOT NULL, "
        "completed_tracks INTEGER NOT NULL, "
        "status TEXT NOT NULL CHECK(status IN ('incomplete', 'complete')), "
        "updated_at TEXT NOT NULL, "
        "splined_version TEXT NOT NULL)"
    )


def _write_progress(
    connection: sqlite3.Connection,
    album_path: str,
    total_tracks: int,
    completed_tracks: int,
    version: str,
) -> None:
    total = max(0, int(total_tracks))
    completed = min(total, max(0, int(completed_tracks)))
    if not album_path or total <= 0:
        return
    status = "complete" if completed >= total else "incomplete"
    connection.execute(
        "INSERT INTO compilation_album_progress"
        "(album_path, total_tracks, completed_tracks, status, updated_at, "
        "splined_version) VALUES(?, ?, ?, ?, ?, ?) "
        "ON CONFLICT(album_path) DO UPDATE SET "
        "total_tracks=excluded.total_tracks, "
        "completed_tracks=excluded.completed_tracks, "
        "status=excluded.status, updated_at=excluded.updated_at, "
        "splined_version=excluded.splined_version",
        (
            album_path,
            total,
            completed,
            status,
            time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            version,
        ),
    )


def record_progress(
    core: Any,
    config_file: Path,
    cfg: dict[str, Any],
    album_path: str,
    total_tracks: int,
    completed_tracks: int,
) -> None:
    """Persist compilation progress independently of an artwork write."""
    connection = sqlite3.connect(
        _database_path(core, config_file, cfg), timeout=10.0
    )
    try:
        with connection:
            _ensure_progress_table(connection)
            _write_progress(
                connection,
                album_path,
                total_tracks,
                completed_tracks,
                str(core.display_version()),
            )
    finally:
        connection.close()


def completed_track_paths(
    core: Any,
    config_file: Path,
    cfg: dict[str, Any],
    album_path: str,
    tracks: list[Any],
) -> set[str]:
    """Return only prior successful writes whose authoritative IDs still match.

    Compilation resume is deliberately lazy. It reads the small write
    ledger only when compilation work starts; the fast Album index remains one
    representative-track inspection per album.
    """
    database = _database_path(core, config_file, cfg)
    if not database.exists() or not tracks:
        return set()
    connection = sqlite3.connect(database, timeout=10.0)
    connection.row_factory = sqlite3.Row
    try:
        _ensure_progress_table(connection)
        saved = {
            str(row["track_path"]): row
            for row in connection.execute(
                "SELECT track_path, recording_mbid, artist_mbid, outcome "
                "FROM compilation_track_artwork "
                "WHERE outcome='embedded-replaced'"
            )
        }
        completed: set[str] = set()
        for track in tracks:
            path = str(track.path)
            row = saved.get(path)
            if row is None:
                continue
            recording = next(
                iter(_mbids(getattr(track, "recording_mbid", None))), ""
            )
            artists = _artist_key(
                _mbids(getattr(track, "artist_mbid", None))
            )
            if (
                recording
                and artists
                and str(row["recording_mbid"]).casefold() == recording
                and str(row["artist_mbid"]).casefold() == artists
            ):
                completed.add(path)
        with connection:
            if completed:
                _write_progress(
                    connection,
                    album_path,
                    len(tracks),
                    len(completed),
                    str(core.display_version()),
                )
            else:
                connection.execute(
                    "DELETE FROM compilation_album_progress "
                    "WHERE album_path=?",
                    (album_path,),
                )
        return completed
    finally:
        connection.close()


def install(core: Any) -> None:
    """Expose compilation track-art APIs without wrapping Album scan functions."""
    global _INSTALLED
    if _INSTALLED or getattr(
        core, "_splined_compilation_authority_policy_installed", False
    ):
        return
    _INSTALLED = True
    core.compilation_manual_album_eligible = manual_album_eligible
    core.compilation_manual_track_eligible = manual_track_eligible
    core.compilation_credential_options = partial(credential_options, core)
    core.compilation_local_artwork_rows = partial(local_artwork_rows, core)
    core.compilation_resolve_releases = partial(
        resolve_release_candidates, core
    )
    core.compilation_resolve_release_id = partial(
        resolve_release_by_id, core
    )
    core.compilation_discover_releases = partial(
        discover_recording_releases, core
    )
    core.compilation_release_candidate_from_discovery = (
        release_candidate_from_discovery
    )
    core.compilation_release_from_local = partial(
        release_from_local_row, core
    )
    core.compilation_release_from_candidate = partial(
        release_from_candidate, core
    )
    core.compilation_prepare_embedded = partial(
        prepare_embedded_artwork, core
    )
    core.compilation_replace_embedded = replace_embedded_artwork
    core.compilation_record_application = partial(record_application, core)
    core.compilation_record_progress = partial(record_progress, core)
    core.compilation_completed_track_paths = partial(
        completed_track_paths, core
    )
    core.compilation_mbids = _mbids
    core._splined_compilation_authority_policy_installed = True
