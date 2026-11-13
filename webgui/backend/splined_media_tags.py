"""Mutagen tag identity and cover inspection for the SPLINED media index."""

from __future__ import annotations

import concurrent.futures
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import unicodedata
from typing import Any, Mapping

from mutagen import File as MutagenFile
from PIL import Image


VALID_ALBUM_STATUSES = {
    "unprocessed",
    "incomplete",
    "processed",
    "bypassed",
    "timeout",
}
_MBID_PATTERN = re.compile(
    r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-"
    r"[0-9a-f]{4}-[0-9a-f]{12}\b"
)
ARTIST_CREDIT_SEPARATOR = "\x1f"


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


def json_list(value: Any) -> list[str]:
    if isinstance(value, (list, tuple)):
        return [str(item) for item in value if str(item)]
    if isinstance(value, str):
        try:
            decoded = json.loads(value)
        except (TypeError, ValueError):
            return []
        return json_list(decoded)
    return []


def normalize(value: str) -> str:
    normalized = unicodedata.normalize("NFKC", str(value)).casefold()
    return " ".join(
        "".join(
            character if character.isalnum() else " "
            for character in normalized
        ).split()
    )


def _tag_text(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, bytes):
        return value.decode("utf-8", "replace").strip("\x00 ")
    text_value = getattr(value, "text", None)
    if isinstance(text_value, (list, tuple)):
        for item in text_value:
            text = _tag_text(item)
            if text:
                return text
        return ""
    return str(value).strip()


def _first_value(tags: Any, *keys: str) -> str:
    if not tags:
        return ""
    for key in keys:
        try:
            raw = tags.get(key)
        except Exception:
            raw = None
        if isinstance(raw, (list, tuple)):
            for item in raw:
                text = _tag_text(item)
                if text:
                    return text
        else:
            text = _tag_text(raw)
            if text:
                return text
    return ""


def _all_text(value: Any) -> str:
    text_values = getattr(value, "text", None)
    if text_values is not None:
        value = text_values
    values = value if isinstance(value, (list, tuple)) else [value]
    return "; ".join(
        text
        for text in (_tag_text(item) for item in values)
        if text
    )


def _all_text_multivalue(value: Any) -> str:
    """Preserve a real multi-value tag in PostgreSQL-safe text.

    PostgreSQL rejects NUL in TEXT.  The ASCII Unit Separator retains the
    authoritative value boundary without colliding with user-configured
    filename/tag splitting rules.
    """
    text_values = getattr(value, "text", None)
    if text_values is not None:
        value = text_values
    values = value if isinstance(value, (list, tuple)) else [value]
    return ARTIST_CREDIT_SEPARATOR.join(
        text
        for text in (_tag_text(item) for item in values)
        if text
    )


def _raw_alias_value(parsed: Any, *aliases: str) -> str:
    tags = getattr(parsed, "tags", None)
    if not tags:
        return ""
    wanted = {
        re.sub(r"[^a-z0-9]+", "", alias.casefold())
        for alias in aliases
    }
    try:
        items = tags.items()
    except Exception:
        return ""
    for key, value in items:
        key_text = re.sub(r"[^a-z0-9]+", "", str(key).casefold())
        if not any(alias in key_text for alias in wanted):
            continue
        if isinstance(value, (list, tuple)):
            for item in value:
                text = _tag_text(item)
                if text:
                    return text
        text = _tag_text(value)
        if text:
            return text
    return ""


def _raw_alias_values(parsed: Any, *aliases: str) -> str:
    tags = getattr(parsed, "tags", None)
    if not tags:
        return ""
    wanted = {
        re.sub(r"[^a-z0-9]+", "", alias.casefold())
        for alias in aliases
    }
    try:
        items = tags.items()
    except Exception:
        return ""
    for key, value in items:
        key_text = re.sub(r"[^a-z0-9]+", "", str(key).casefold())
        if any(alias in key_text for alias in wanted):
            return _all_text_multivalue(value)
    return ""


def _musicbrainz_recording_ufid(parsed: Any) -> str:
    tags = getattr(parsed, "tags", None)
    getall = getattr(tags, "getall", None)
    if not callable(getall):
        return ""
    for frame in getall("UFID"):
        owner = str(getattr(frame, "owner", "") or "")
        if owner.rstrip("/").casefold() != "http://musicbrainz.org":
            continue
        data = getattr(frame, "data", b"")
        if isinstance(data, bytes):
            return data.decode("ascii", "replace").strip()
        return str(data or "").strip()
    return ""


def _mbids(value: Any) -> list[str]:
    # Artist-credit MBIDs are positional: the first identifier belongs to the
    # first TPE1/ARTIST credit, the second identifier to the second credit, and
    # so on.  Sorting here silently paired featured Artists with the wrong
    # identity (for example Rob Wasserman received Aaron Neville's MBID).  Keep
    # first-seen tag order while still discarding exact duplicates.
    return list(
        dict.fromkeys(
            match.group(0).casefold()
            for match in _MBID_PATTERN.finditer(str(value or ""))
        )
    )


class _TagReader:
    """Open one local tag source once for Album and track identity fields."""

    def __init__(self, audio_path: Path):
        self.audio_path = audio_path
        self.raw: Any = None
        try:
            easy = MutagenFile(audio_path, easy=True)
        except Exception:
            easy = None
        self.tags = getattr(easy, "tags", None) if easy is not None else None

    def raw_file(self):
        if self.raw is None:
            try:
                self.raw = MutagenFile(self.audio_path, easy=False)
            except Exception:
                self.raw = False
        return self.raw

    def value(self, *keys: str) -> str:
        found = _first_value(self.tags, *keys)
        if found:
            return found
        raw = self.raw_file()
        return _raw_alias_value(raw, *keys) if raw else ""

    def values(self, *keys: str) -> str:
        if self.tags:
            for key in keys:
                try:
                    found = _all_text_multivalue(self.tags.get(key))
                except Exception:
                    found = ""
                if found:
                    return found
        raw = self.raw_file()
        return _raw_alias_values(raw, *keys) if raw else ""


def _album_tags(reader: _TagReader) -> dict[str, Any]:
    raw_year = reader.value("date", "originaldate", "original year", "year")
    year_match = re.search(r"\b(\d{4})\b", raw_year)
    compilation_text = reader.value("compilation", "itunescompilation")
    return {
        "album": reader.value("album"),
        "album_artist": reader.value("albumartist", "album artist", "artist"),
        "artist_sort": reader.value(
            "albumartistsort", "album artist sort", "artistsort"
        ),
        "album_sort": reader.value("albumsort", "album sort"),
        "album_mbid": reader.value(
            "musicbrainz_albumid",
            "musicbrainz album id",
            "musicbrainz release id",
        ),
        "release_group_mbid": reader.value(
            "musicbrainz_releasegroupid",
            "musicbrainz release group id",
        ),
        "artist_mbid": reader.value(
            "musicbrainz_albumartistid",
            "musicbrainz album artist id",
            "musicbrainz artist id",
        ),
        "year": year_match.group(1) if year_match else "",
        "compilation": compilation_text.strip().casefold()
        in {"1", "true", "yes", "y"},
    }


def read_album_tags(audio_path: Path) -> dict[str, Any]:
    return _album_tags(_TagReader(audio_path))


DEFAULT_ARTIST_SEPARATOR_REGEX = r"(?i)(?: / | feat\. | feat | ft\. | ft |; )"


def normalize_artist_credit_text(
    value: str,
    separator_regex: str = DEFAULT_ARTIST_SEPARATOR_REGEX,
    artist_mbids: str = "",
) -> str:
    """Return canonical NUL-separated credits for one indexed Artist value.

    Real multi-value tags are already NUL-separated by the readers and remain
    authoritative. The configured expression is only applied to one joined
    Artist/TPE1 (or a filename credit handled by the catalog).
    """
    text = str(value or "").replace("\0", ARTIST_CREDIT_SEPARATOR)
    explicit = tuple(
        part.strip() for part in text.split(ARTIST_CREDIT_SEPARATOR) if part.strip()
    )
    if len(explicit) > 1:
        return ARTIST_CREDIT_SEPARATOR.join(dict.fromkeys(explicit))
    if not explicit:
        return ""
    # ID3v2.3 stores a real multi-value TPE1 frame as one slash-delimited
    # string. A bare slash cannot be a global Artist separator because it is
    # also meaningful inside names such as AC/DC. Ordered MusicBrainz Artist
    # IDs provide unambiguous cardinality: split only when the slash values
    # and IDs have the same count. Windows/Lofty already decodes this form as
    # a native multi-value sequence, so PostgreSQL now projects it identically.
    slash_parts = tuple(
        part.strip() for part in explicit[0].split("/") if part.strip()
    )
    if len(slash_parts) > 1 and len(_mbids(artist_mbids)) == len(slash_parts):
        return ARTIST_CREDIT_SEPARATOR.join(dict.fromkeys(slash_parts))
    try:
        parts = re.split(separator_regex, explicit[0])
    except re.error as exc:
        raise ValueError(f"Artist separator Regex is invalid: {exc}") from exc
    return ARTIST_CREDIT_SEPARATOR.join(
        dict.fromkeys(part.strip() for part in parts if part.strip())
    )


def _track_identity_tags(
    reader: _TagReader,
    artist_separator_regex: str = DEFAULT_ARTIST_SEPARATOR_REGEX,
) -> dict[str, str]:
    recording_mbid = reader.value(
        "musicbrainz_trackid",
        "musicbrainz recording id",
        "musicbrainz track id",
    )
    if not recording_mbid:
        raw = reader.raw_file()
        recording_mbid = _musicbrainz_recording_ufid(raw) if raw else ""

    artist_mbids = reader.values(
        "musicbrainz_artistid",
        "musicbrainz artist id",
    )
    return {
        "title": reader.value("title"),
        # TPE1/ARTIST is explicitly multi-value. Keeping only its first value
        # hides Featured performers from the Artist catalog even though tools
        # such as Mp3tag and Navidrome correctly expose every credit.
        "artist": normalize_artist_credit_text(
            reader.values("artist"), artist_separator_regex, artist_mbids
        ),
        "recording_mbid": recording_mbid,
        "artist_mbid": artist_mbids,
    }


def read_track_identity_tags(
    audio_path: Path,
    artist_separator_regex: str = DEFAULT_ARTIST_SEPARATOR_REGEX,
) -> dict[str, str]:
    """Read only the local fields needed for authoritative track matching."""
    return _track_identity_tags(_TagReader(audio_path), artist_separator_regex)


def read_album_and_track_identity_tags(
    audio_path: Path,
    artist_separator_regex: str = DEFAULT_ARTIST_SEPARATOR_REGEX,
) -> tuple[dict[str, Any], dict[str, str]]:
    """Read representative Album and track identity from one opened tag view."""
    reader = _TagReader(audio_path)
    return _album_tags(reader), _track_identity_tags(reader, artist_separator_regex)


def inspect_album_tracks(
    album: Any,
    album_key: str,
    existing_by_path: Mapping[str, Mapping[str, Any]],
    *,
    force_read: bool = False,
    artist_separator_regex: str = DEFAULT_ARTIST_SEPARATOR_REGEX,
    executor: Any = None,
    preloaded: Mapping[str, Mapping[str, Any]] | None = None,
    progress: Any = None,
) -> tuple[list[dict[str, Any]], int, int]:
    """Build exact Recording/Artist relationships without remote inference."""
    rows: list[dict[str, Any]] = []
    reads = 0
    reuses = 0
    now = utc_now()
    audio_metadata = dict(getattr(album, "audio_metadata", {}) or {})
    paths = sorted(
        (Path(value) for value in album.audio_files),
        key=lambda value: str(value).casefold(),
    )

    def metadata(raw_path: Path) -> tuple[int, int]:
        path = str(raw_path)
        if path in audio_metadata:
            return audio_metadata[path]
        try:
            stat = raw_path.stat()
            return int(stat.st_size), int(stat.st_mtime_ns)
        except OSError:
            return 0, 0

    def needs_read(raw_path: Path) -> bool:
        path = str(raw_path)
        size, modified = metadata(raw_path)
        prior = existing_by_path.get(path)
        return bool(
            force_read
            or prior is None
            or int(prior["file_size"] or 0) != size
            or int(prior["file_mtime_ns"] or 0) != modified
        )

    preloaded_values = {
        str(path): dict(value) for path, value in (preloaded or {}).items()
    }
    preloaded_count = sum(str(path) in preloaded_values for path in paths)
    read_paths = [
        path for path in paths
        if needs_read(path) and str(path) not in preloaded_values
    ]

    def read(raw_path: Path) -> dict[str, Any]:
        try:
            return read_track_identity_tags(raw_path, artist_separator_regex)
        except Exception:
            return {
                "title": raw_path.stem,
                "artist": "",
                "recording_mbid": "",
                "artist_mbid": "",
            }

    read_results = dict(preloaded_values)
    if executor is not None and len(read_paths) > 1:
        futures = {executor.submit(read, path): path for path in read_paths}
        total_reads = len(read_paths) + preloaded_count
        report_every = max(16, (total_reads + 19) // 20)
        for completed, future in enumerate(
            concurrent.futures.as_completed(futures), 1
        ):
            path = futures[future]
            read_results[str(path)] = future.result()
            if progress is not None and (
                completed == len(read_paths) or completed % report_every == 0
            ):
                progress(completed + preloaded_count, total_reads)
    else:
        total_reads = len(read_paths) + preloaded_count
        for completed, path in enumerate(read_paths, 1):
            read_results[str(path)] = read(path)
            if progress is not None and (
                completed == len(read_paths) or completed % 16 == 0
            ):
                progress(completed + preloaded_count, total_reads)

    for raw_path in paths:
        path = str(raw_path)
        size, modified = metadata(raw_path)

        prior = existing_by_path.get(path)
        if path not in read_results and prior is not None:
            tags = {
                "title": str(prior["title"] or raw_path.stem),
                "artist": str(prior["artist_name"] or ""),
                "recording_mbid": str(
                    prior["musicbrainz_recordingid"] or ""
                ),
                "artist_mbid": str(prior["musicbrainz_artistid"] or ""),
            }
            reuses += 1
        else:
            tags = read_results[path]
            reads += 1

        recording_ids = _mbids(tags["recording_mbid"])
        artist_ids = _mbids(tags["artist_mbid"])
        recording_mbid = recording_ids[0] if len(recording_ids) == 1 else ""
        artist_mbid = ",".join(artist_ids)

        rows.append(
            {
                "track_key": path,
                "album_key": album_key,
                "path": path,
                "title": str(tags["title"] or raw_path.stem),
                "artist_name": str(tags["artist"] or ""),
                "musicbrainz_recordingid": recording_mbid,
                "musicbrainz_artistid": artist_mbid,
                "file_size": size,
                "file_mtime_ns": modified,
                "updated_at": now,
            }
        )
    return rows, reads, reuses


def artist_key(artist_name: str, artist_mbid: str) -> str:
    mbid = artist_mbid.strip().casefold()
    return f"mbid:{mbid}" if mbid else f"tag:{normalize(artist_name) or 'unknown artist'}"


def album_base_key(
    artist_identity: str,
    album_name: str,
    album_mbid: str,
    release_group_mbid: str,
    year: str,
    compilation: bool,
) -> str:
    mbid = album_mbid.strip().casefold()
    if mbid:
        return f"mbid:{mbid}"
    body = "\x1f".join(
        (
            artist_identity,
            normalize(album_name) or "unknown album",
            release_group_mbid.strip().casefold(),
            year.strip(),
            "1" if compilation else "0",
        )
    )
    return "tag:" + hashlib.sha256(body.encode("utf-8")).hexdigest()


def top_artist_path(library_root: Path, album_path: Path) -> Path:
    try:
        relative = album_path.relative_to(library_root)
    except ValueError:
        return album_path.parent
    return library_root / relative.parts[0] if relative.parts else album_path.parent


def cover_statistics(album: Any, cover_name: str) -> dict[str, Any]:
    audio_paths = {Path(value) for value in album.audio_files}
    sidecars: list[Path] = []
    try:
        with os.scandir(album.path) as iterator:
            for item in iterator:
                try:
                    if item.is_symlink() or not item.is_file(follow_symlinks=False):
                        continue
                except OSError:
                    continue
                path = Path(item.path)
                if path not in audio_paths:
                    sidecars.append(path)
    except OSError:
        sidecars = [Path(path) for path in album.local_art_files]

    sidecars.sort(key=lambda path: path.name.casefold())
    prefix = cover_name.strip().casefold() or "cover"
    artwork = {"JPEG": 0, "PNG": 0, "WEBP": 0, "OTHER": 0}
    covers: list[Path] = []
    others: list[Path] = []
    webps: list[Path] = []
    for path in sidecars:
        suffix = path.suffix.casefold()
        if suffix in {".jpg", ".jpeg"}:
            artwork["JPEG"] += 1
        elif suffix == ".png":
            artwork["PNG"] += 1
        elif suffix == ".webp":
            artwork["WEBP"] += 1
            webps.append(path)
        else:
            artwork["OTHER"] += 1
        (covers if path.stem.casefold().startswith(prefix) else others).append(path)

    canonical: Path | None = None
    width: int | None = None
    height: int | None = None
    image_format = ""
    if covers:
        canonical = sorted(
            covers,
            key=lambda path: (
                path.stem.casefold() != prefix,
                path.name.casefold(),
            ),
        )[0]
        try:
            with Image.open(canonical) as image:
                width = int(image.width)
                height = int(image.height)
                image_format = str(
                    image.format or canonical.suffix[1:]
                ).upper()
        except Exception:
            image_format = canonical.suffix[1:].upper()

    webp_size_mb = 0.0
    webp_resolution = ""
    webp_conversion = False
    if webps:
        def size(path: Path) -> int:
            try:
                return int(path.stat().st_size)
            except OSError:
                return 0

        webp = max(webps, key=size)
        webp_size_mb = round(size(webp) / 1_000_000, 2)
        try:
            with Image.open(webp) as image:
                webp_resolution = f"{int(image.width)}x{int(image.height)}"
                webp_conversion = True
        except Exception:
            pass

    local_art = sorted(
        {str(path) for path in (*album.local_art_files, *covers)},
        key=str.casefold,
    )
    return {
        "artwork": artwork,
        "root_files": len(sidecars),
        "cover_files": len(covers),
        "cover_names": [path.name for path in covers],
        "local_art": local_art,
        "other_filenames": [path.name for path in others],
        "cover_found": bool(canonical),
        "cover_path": str(canonical) if canonical else "",
        "cover_name": canonical.name if canonical else "",
        "cover_format": image_format,
        "cover_width": width,
        "cover_height": height,
        "webp_found": bool(webps),
        "webp_size_mb": webp_size_mb,
        "webp_resolution": webp_resolution,
        "webp_conversion": webp_conversion,
    }


def without_fallback_cover_statistics(
    cover: Mapping[str, Any], cover_name: str
) -> dict[str, Any]:
    """Remove configured ``cover.*`` material from a missing-MBID Album."""
    result = dict(cover)
    prefix = cover_name.strip().casefold() or "cover"
    cover_names = [str(value) for value in cover.get("cover_names", [])]
    local_art_values = [str(value) for value in cover.get("local_art", [])]
    excluded_names = {
        Path(value).name.casefold(): Path(value).name
        for value in (*cover_names, *local_art_values)
        if Path(value).stem.casefold() == "cover"
        or Path(value).stem.casefold().startswith(prefix)
    }
    artwork = dict(cover.get("artwork", {}))
    for name in excluded_names.values():
        suffix = Path(name).suffix.casefold()
        category = (
            "JPEG" if suffix in {".jpg", ".jpeg"}
            else "PNG" if suffix == ".png"
            else "WEBP" if suffix == ".webp"
            else "OTHER"
        )
        artwork[category] = max(0, int(artwork.get(category, 0)) - 1)
    local_art = [
        str(value)
        for value in local_art_values
        if Path(value).name.casefold() not in excluded_names
    ]
    result.update(
        artwork=artwork,
        root_files=max(
            0,
            int(cover.get("root_files", 0))
            - len(excluded_names),
        ),
        cover_files=0,
        cover_names=[],
        local_art=local_art,
        cover_found=False,
        cover_path="",
        cover_name="",
        cover_format="",
        cover_width=None,
        cover_height=None,
        webp_found=False,
        webp_size_mb=0.0,
        webp_resolution="",
        webp_conversion=False,
    )
    return result


def inspect_album(
    album: Any,
    library_root: Path,
    cover_name: str,
    existing: Mapping[str, Mapping[str, Any]],
    existing_by_path: Mapping[str, Mapping[str, Any]],
    used_keys: dict[str, str],
    *,
    include_track_identity: bool = False,
    artist_separator_regex: str = DEFAULT_ARTIST_SEPARATOR_REGEX,
) -> tuple[
    dict[str, Any],
    dict[str, Any],
    dict[str, Any] | None,
    bool,
]:
    """Inspect one Album, reusing stored tags when its representative is stable.

    Path is only an I/O shortcut here.  Logical identity remains the tagged
    Artist/Album key (MusicBrainz IDs first, normalized tags as fallback).
    """
    representative = sorted(
        (Path(path) for path in album.audio_files),
        key=lambda path: str(path).casefold(),
    )[0]
    path_text = str(album.path)
    audio_metadata = dict(getattr(album, "audio_metadata", {}) or {})
    representative_size, representative_mtime_ns = audio_metadata.get(
        str(representative), (0, 0)
    )
    if str(representative) not in audio_metadata:
        try:
            stat = representative.stat()
            representative_size = int(stat.st_size)
            representative_mtime_ns = int(stat.st_mtime_ns)
        except OSError:
            representative_size = 0
            representative_mtime_ns = 0

    prior_at_path = existing_by_path.get(path_text)
    album.representative_track_tags = None
    reuse_tags = bool(
        prior_at_path is not None
        and str(prior_at_path["representative_file"] or "")
        == str(representative)
        and int(prior_at_path["representative_size"] or 0)
        == representative_size
        and int(prior_at_path["representative_mtime_ns"] or 0)
        == representative_mtime_ns
    )

    artist_folder = top_artist_path(library_root, Path(album.path))
    if reuse_tags and prior_at_path is not None:
        artist_name = str(prior_at_path["artist_name"] or artist_folder.name)
        album_name = str(prior_at_path["album_name"] or Path(album.path).name)
        identity = str(prior_at_path["artist_key"])
        key = str(prior_at_path["album_key"])
        tags = {
            "album": album_name,
            "album_artist": artist_name,
            "artist_sort": str(
                prior_at_path["artist_sort"] or artist_name
            ),
            "album_sort": str(prior_at_path["album_sort"] or album_name),
            "album_mbid": str(prior_at_path["musicbrainz_albumid"] or ""),
            "release_group_mbid": str(
                prior_at_path["musicbrainz_releasegroupid"] or ""
            ),
            "artist_mbid": str(
                prior_at_path["musicbrainz_artistid"] or ""
            ),
            "year": str(prior_at_path["release_year"] or ""),
            "compilation": bool(prior_at_path["compilation"]),
        }
        tag_signature = str(prior_at_path["tag_signature"] or "")
    else:
        if include_track_identity:
            tags, album.representative_track_tags = (
                read_album_and_track_identity_tags(
                    representative, artist_separator_regex
                )
            )
        else:
            tags = read_album_tags(representative)
        artist_name = str(
            tags["album_artist"] or artist_folder.name or "Unknown Artist"
        )
        album_name = str(tags["album"] or Path(album.path).name or "Unknown Album")
        identity = artist_key(artist_name, str(tags["artist_mbid"]))
        key = album_base_key(
            identity,
            album_name,
            str(tags["album_mbid"]),
            str(tags["release_group_mbid"]),
            str(tags["year"]),
            bool(tags["compilation"]),
        )
        tag_body = json.dumps(tags, sort_keys=True, separators=(",", ":"))
        tag_signature = hashlib.sha256(tag_body.encode("utf-8")).hexdigest()

    review: dict[str, Any] | None = None
    prior_path = used_keys.get(key)
    if prior_path is not None and prior_path != path_text:
        # Preserve a previously assigned duplicate key when possible.  This
        # keeps multiple physical copies deterministic across refreshes.
        previous_copy = (
            str(prior_at_path["album_key"])
            if prior_at_path is not None
            and str(prior_at_path["album_key"]).startswith(key + ":copy:")
            else ""
        )
        if previous_copy:
            key = previous_copy
        else:
            try:
                relative = str(Path(path_text).relative_to(library_root))
            except ValueError:
                relative = path_text
            key = (
                key
                + ":copy:"
                + hashlib.sha256(relative.encode("utf-8")).hexdigest()[:12]
            )
        review = {
            "album_key": key,
            "source": "duplicate-tag-identity",
            "details": {
                "base_key": key.split(":copy:", 1)[0],
                "existing_path": prior_path,
                "duplicate_path": path_text,
            },
        }
    used_keys[key] = path_text

    # Missing-MBID COMP albums still use their folder cover for album-card
    # identity.  The COMP workflow independently reads and writes embedded
    # artwork per track, so retaining this inventory cannot substitute the
    # folder cover for track artwork.
    cover = cover_statistics(album, cover_name)
    previous = existing.get(key) or prior_at_path
    previous_status = (
        str(previous["status"])
        if previous is not None
        and str(previous["status"]) in VALID_ALBUM_STATUSES
        else ""
    )
    # A discovered cover is artwork inventory, not proof that SPLINED
    # processed the Album.  Early SQLite builds conflated those facts and
    # produced ``processed`` rows without either completion timestamp or
    # selected-source evidence.  Do not carry that derived state forward.
    if (
        previous_status == "processed"
        and previous is not None
        and not str(previous["processed_at"] or "").strip()
        and not str(previous["selected_source"] or "").strip()
    ):
        previous_status = ""
    now = utc_now()
    status = previous_status or "unprocessed"
    artist_row = {
        "artist_key": identity,
        "artist_name": artist_name,
        "artist_sort": str(tags["artist_sort"] or artist_name),
        "musicbrainz_artistid": str(tags["artist_mbid"]),
        "primary_path": str(artist_folder),
        "status": "unprocessed",
        "created_at": now,
        "updated_at": now,
        "last_seen_at": now,
    }
    album_row = {
        "album_key": key,
        "artist_key": identity,
        "album_name": album_name,
        "album_sort": str(tags["album_sort"] or album_name),
        "musicbrainz_albumid": str(tags["album_mbid"]),
        "musicbrainz_releasegroupid": str(tags["release_group_mbid"]),
        "release_year": str(tags["year"]),
        "compilation": int(bool(tags["compilation"])),
        "path": path_text,
        "representative_file": str(representative),
        "representative_size": representative_size,
        "representative_mtime_ns": representative_mtime_ns,
        "tag_signature": tag_signature,
        "track_count": len(album.audio_files),
        "inventory_fingerprint": album.inventory_fingerprint,
        "status": status,
        "cover_required": 1,
        "cover_found": int(bool(cover["cover_found"])),
        "cover_path": str(cover["cover_path"]),
        "cover_name": str(cover["cover_name"]),
        "cover_format": str(cover["cover_format"]),
        "cover_width": cover["cover_width"],
        "cover_height": cover["cover_height"],
        "artwork_jpeg": int(cover["artwork"]["JPEG"]),
        "artwork_png": int(cover["artwork"]["PNG"]),
        "artwork_webp": int(cover["artwork"]["WEBP"]),
        "artwork_other": int(cover["artwork"]["OTHER"]),
        "root_files": int(cover["root_files"]),
        "cover_files": int(cover["cover_files"]),
        "cover_names_json": json.dumps(
            cover["cover_names"], separators=(",", ":")
        ),
        "local_art_json": json.dumps(
            cover["local_art"], separators=(",", ":")
        ),
        "other_filenames_json": json.dumps(
            cover["other_filenames"], separators=(",", ":")
        ),
        "webp_found": int(bool(cover["webp_found"])),
        "webp_size_mb": float(cover["webp_size_mb"]),
        "webp_resolution": str(cover["webp_resolution"]),
        "webp_conversion": int(bool(cover["webp_conversion"])),
        "processed_at": (
            str(previous["processed_at"] or "") if previous else ""
        ),
        "bypassed": int(previous["bypassed"] or 0) if previous else 0,
        "timeout_until": (
            str(previous["timeout_until"] or "") if previous else ""
        ),
        "selected_source": (
            str(previous["selected_source"] or "") if previous else ""
        ),
        "created_at": str(previous["created_at"]) if previous else now,
        "updated_at": now,
        "last_seen_at": now,
    }
    return artist_row, album_row, review, reuse_tags
