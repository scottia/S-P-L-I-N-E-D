"""Browser catalog and bounded image handles for the SPLINED WebGUI."""

from __future__ import annotations

import base64
from contextlib import closing
from dataclasses import dataclass, field
import hashlib
import hmac
from hashlib import sha256
from html import unescape
from io import BytesIO
import json
import logging
import os
from pathlib import Path
import re
import secrets
import tempfile
import time
import sqlite3
from threading import BoundedSemaphore, Lock, Thread
from urllib.parse import quote, urlsplit
from uuid import UUID
import unicodedata

from PIL import Image, ImageOps
from mutagen import File as MutagenFile
from mutagen.id3 import ID3, ID3NoHeaderError
from mutagen.flac import Picture
import requests

from splined_artist_art import ArtistAlbum, ArtistArtTarget, artist_targets, local_candidates, saved_package
import splined_artist_unicode as artist_unicode
from splined_artist_separator import ArtistSeparator, parse as parse_artist_separator
from splined_compilation_authority_policy import replace_embedded_artwork
from splined_media_tags import first_artist_value
from interaction.artist_search import discover_artist_images, discover_artist_profile
from webgui.database import Database
from webgui import imaging
from webgui.comp_report_policy import artist_names_compatible, normalize_artist_name
from webgui.media_state_policy import classify_media_state
from webgui.coordination import (
    catalog_relative_album_path,
    normalized_coordination_relative,
    relative_album_path,
)


ASSETS = Path(__file__).with_name("static")
IMAGE_EXTENSIONS = {".jpg", ".jpeg", ".png", ".webp"}
AUDIO_EXTENSIONS = {".mp3", ".flac", ".m4a", ".mp4", ".ogg", ".opus", ".wav", ".aiff", ".aif"}
LOGGER = logging.getLogger("uvicorn.error")
DEFAULT_ARTIST_SEPARATOR_REGEX = r"(?i)(?: / | feat\. | feat | ft\. | ft |; )"
ARTIST_CREDIT_SEPARATOR = "\x1f"
ARTIST_PROJECTION_CACHE_KEY = "webgui:artist-projection:v2"
ARTIST_CREDIT_EVIDENCE_VERSION = 12


class ArtistProjectionPending(RuntimeError):
    """The requested inventory's local Artist projection is still warming."""


def _artist_credit_pattern(separator_regex: str) -> re.Pattern[str]:
    try:
        return re.compile(separator_regex)
    except re.error:
        return re.compile(DEFAULT_ARTIST_SEPARATOR_REGEX)


def explicit_artist_credits(
    track_artist: str,
    separator_regex: str = DEFAULT_ARTIST_SEPARATOR_REGEX,
) -> tuple[str, ...]:
    """Return ordered Artist credits without guessing joined band names.

    ASCII Unit Separator is the PostgreSQL-safe durable representation of a
    real multi-value Artist/TPE1 tag and always wins. NUL remains accepted at
    SQLite/reconciliation boundaries. The configured separator is used only
    when a tag (or a strict numbered filename credit) arrived as one joined
    string.
    """
    value = str(track_artist or "")
    normalized = value.replace("\0", ARTIST_CREDIT_SEPARATOR)
    parts = (
        normalized.split(ARTIST_CREDIT_SEPARATOR)
        if ARTIST_CREDIT_SEPARATOR in normalized
        else _artist_credit_pattern(separator_regex).split(normalized)
    )
    return tuple(dict.fromkeys(credit.strip() for credit in parts if credit.strip()))


def potential_collaboration_credits(value: str) -> tuple[str, ...]:
    """Mirror Windows' guarded Artist-presentation credit expansion.

    A real multi-value Artist tag is always separable. Human-readable ``&``
    and comma joins are only *potential* collaborations; callers must still
    require an independently indexed Artist before exposing either part.
    """
    raw = str(value or "")
    normalized = raw.replace("\0", ARTIST_CREDIT_SEPARATOR)
    explicit = ARTIST_CREDIT_SEPARATOR in normalized
    parts = normalized.split(ARTIST_CREDIT_SEPARATOR)
    if not explicit:
        parts = [piece for credit in parts for piece in credit.split(" & ")]
        parts = [piece for credit in parts for piece in credit.split(", ")]
    result: list[str] = []
    seen: set[str] = set()
    for part in parts:
        display = part.strip()
        identity = normalized_artist_identity(display)
        if display and identity and identity not in seen:
            seen.add(identity)
            result.append(display)
    return tuple(result) or ((raw.strip(),) if raw.strip() else ())


def artist_credit_position(
    track_artist: str,
    artist: str,
    separator_regex: str = DEFAULT_ARTIST_SEPARATOR_REGEX,
) -> int | None:
    """Match Windows' explicit and joined multi-artist credit rules."""
    selected = artist.strip().casefold()
    if not selected:
        return None
    for position, credit in enumerate(
        explicit_artist_credits(track_artist, separator_regex)
    ):
        if credit.casefold() == selected:
            return position
    selected_identity = normalized_artist_identity(artist)
    for position, credit in enumerate(potential_collaboration_credits(track_artist)):
        if normalized_artist_identity(credit) == selected_identity:
            return position
    return None


def projected_artist_id(name: str) -> str:
    """Stable UI identity for an Artist credited only on another owner's Album."""
    identity = normalized_artist_identity(name)
    return "credit:" + sha256(identity.encode("utf-8")).hexdigest()[:32]


def indexed_track_credit_values(
    track_artist: str,
    title: str = "",
    path: str = "",
    separator_regex: str = DEFAULT_ARTIST_SEPARATOR_REGEX,
) -> tuple[str, ...]:
    """Return explicit tags plus strict numbered-filename Artist credits.

    Some historical media stores the Album Artist in the track Artist field
    while preserving the real multi-Artist credit in the canonical
    ``disc-track - Artists - Title`` name. Windows/Navidrome exposes those
    appearances. Accept only that numbered convention so ordinary song titles
    containing an Artist name cannot manufacture a Featured appearance.
    """
    tagged = str(track_artist or "").strip()
    # A real multi-value ARTIST/TPE1 field is authoritative.  Do not append
    # the human-readable filename credit as another Artist identity: a file
    # such as ``Rob Wasserman & Aaron Neville`` would otherwise manufacture a
    # third, combined Artist beside the two correctly tagged performers.
    if len(explicit_artist_credits(tagged, separator_regex)) > 1:
        return (tagged,)

    values = [tagged]
    for candidate in (str(title or ""), Path(str(path or "")).stem):
        match = re.match(r"^\s*(?:\d+-)?\d+\s+-\s+(.+?)\s+-\s+.+$", candidate)
        if match:
            values.append(match.group(1).strip())
    return tuple(dict.fromkeys(value for value in values if value))


_CATALOG_OWNER_SEPARATOR = re.compile(r" (?:feat\.?|ft\.?|featuring|&) ", re.IGNORECASE | re.ASCII)
_CATALOG_OWNER_FEATURE = re.compile(r" (?:feat\.?|ft\.?|featuring) ", re.IGNORECASE | re.ASCII)
_CATALOG_MBID = re.compile(
    r"\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b",
    re.IGNORECASE | re.ASCII,
)


def ordered_artist_mbids(value: object) -> list[str]:
    return [match.group().lower() for match in _CATALOG_MBID.finditer(str(value or ""))]


def _numbered_catalog_credit(value: str) -> str | None:
    fields = [field.strip() for field in value.split(" - ", 3)]
    if len(fields) >= 3 and fields[1] and fields[2]:
        if re.fullmatch(r"[0-9]+(?:-[0-9]+)*", fields[0]):
            return fields[1]
    return None


def catalog_credit_slots(
    track_artist: str, artist_mbids: str, title: str, filename: str,
    owner: str, separator: ArtistSeparator,
) -> list[tuple[str, int | None, str | None, bool]]:
    """Return name, tagged position, paired ID and explicit-evidence flag."""
    def clean(parts):
        return [part.strip() for part in parts if part.strip()]

    tagged = clean(track_artist.replace("\0", ARTIST_CREDIT_SEPARATOR).split(ARTIST_CREDIT_SEPARATOR))
    ids = ordered_artist_mbids(artist_mbids)

    def bind(names, positional, explicit):
        ids_match = positional and len(names) == len(ids)
        return [
            (name, index if positional else None, ids[index] if ids_match else None, explicit)
            for index, name in enumerate(names)
        ]

    if len(tagged) > 1:
        return bind(tagged, True, True)
    joined = tagged[0] if tagged else ""
    for delimiter in ("/", " & ", ", "):
        names = clean(joined.split(delimiter))
        if len(names) > 1 and len(names) == len(ids):
            return bind(names, True, True)
    owner_key = artist_unicode.casefold(owner.strip())
    # A backslash in one local value is positional evidence only with the
    # physical owner as the first credit and exact ordered ID cardinality.
    if owner_key and len(ids) > 1 and "\\" in joined:
        names = clean(joined.split("\\"))
        if len(names) == len(ids) and artist_unicode.casefold(names[0]) == owner_key:
            return bind(names, True, True)

    def configured_names(value: str) -> list[str]:
        return clean(separator.split(value))

    configured = configured_names(joined)
    if len(configured) > 1 and len(configured) == len(ids):
        return bind(configured, True, True)

    def anchored(value: str, allow_ampersand: bool) -> list[str] | None:
        if not owner_key or artist_unicode.casefold(value.strip()) == owner_key:
            return None
        pattern = _CATALOG_OWNER_SEPARATOR if allow_ampersand else _CATALOG_OWNER_FEATURE
        for match in pattern.finditer(value):
            lead, guest = value[:match.start()].strip(), value[match.end():].strip()
            if not lead or not guest:
                continue
            ampersand = match.group() == " & "
            if not ampersand and not separator.full_match(match.group()):
                continue
            if artist_unicode.casefold(lead) == owner_key or (
                ampersand and artist_unicode.casefold(guest) == owner_key
            ):
                return [lead, guest]
        return None

    if names := anchored(joined, not ids):
        return bind(names, False, True)
    if len(configured) > 1 and not ids and artist_unicode.casefold(joined) != owner_key:
        return bind(configured, False, True)
    credits = bind(tagged, True, True)
    for text in (title, filename):
        value = _numbered_catalog_credit(text)
        if not value or any(
            artist_unicode.casefold(credit[0]) == artist_unicode.casefold(value)
            for credit in credits
        ):
            continue
        names = anchored(value, True)
        if names is None:
            names = configured_names(value)
            if " & " in value and len(names) < 2:
                continue
        credits.extend(bind(names, False, False))
    return credits


def normalized_artist_identity(value: str) -> str:
    """Return the display identity without rewriting stored Artist/Album IDs."""
    normalized = artist_unicode.normalized_fold(str(value or ""))
    return "".join(character for character in normalized if artist_unicode.alphanumeric(character))


def artist_directory_identity(value: object) -> str:
    """Return the identity used to reject synthetic direct ``artist.*`` roots.

    Synthetic roots such as ``[Soundtracks]`` and ``[Various Artists]`` can
    contain an unqualified ``artist.*`` for an entirely different performer.
    Artist folders may carry stable disambiguation suffixes (for example
    ``Beyonce-One``), so ordinary non-synthetic directories stay eligible.
    """
    return normalized_artist_identity(str(value or "").replace("&", "and")).removeprefix("the")


def provider_artist_genres(detail: object) -> list[str]:
    """Collect Artist and Album genres from one bounded OpenSubsonic result."""
    if not isinstance(detail, dict):
        return []
    albums = detail.get("album") or []
    if isinstance(albums, dict):
        albums = [albums]
    values = [detail, *(item for item in albums if isinstance(item, dict))]
    genres: set[str] = set()
    for item in values:
        genre = str(item.get("genre") or "").strip()
        if genre:
            genres.add(genre)
        listed = item.get("genres") or []
        if isinstance(listed, (str, dict)):
            listed = [listed]
        for value in listed:
            if isinstance(value, dict):
                value = value.get("name")
            value = str(value or "").strip()
            if value:
                genres.add(value)
    return sorted(genres, key=str.casefold)


def artist_search_literal(value: object) -> str:
    """Unicode search text that retains literal punctuation."""
    return artist_unicode.search_literal(str(value or ""))


def artist_search_matches(query: object, candidate: object) -> bool:
    """Match literal punctuation first, then catalog-equivalent identity."""
    literal_query = artist_search_literal(query)
    if not literal_query:
        return True
    if literal_query in artist_search_literal(candidate):
        return True
    identity_query = normalized_artist_identity(query)
    return bool(
        identity_query
        and identity_query in normalized_artist_identity(candidate)
    )


def synthetic_album_artist(value: str) -> bool:
    """Return true for catalog owners that are not selectable real Artists.

    Soundtrack and Various Artists folders remain first-class Album owners, but
    their identified releases are projected into each credited real Artist's
    overview instead of manufacturing duplicate Artist sidebar identities.
    """
    return normalized_artist_identity(value) in {
        "soundtrack",
        "soundtracks",
        "variousartist",
        "variousartists",
    }


def artist_display_sort_key(value: object) -> str:
    """Sort visible Artist names without discarding meaningful punctuation.

    Canonical identity remains deliberately punctuation-insensitive so this
    presentation change cannot split one Artist into duplicate catalog rows.
    """
    return artist_unicode.sort_key(str(value or ""))


def normalized_album_identity(value: str) -> str:
    return "".join(
        character.casefold()
        for character in unicodedata.normalize("NFKC", str(value or ""))
        if character.isalnum()
    )


def inside(path: Path, root: Path) -> bool:
    try:
        path.resolve(strict=True).relative_to(root.resolve(strict=True))
        return True
    except (OSError, ValueError):
        return False


def relative_coordination_path(path: Path, root: Path) -> str:
    try:
        relative = path.resolve(strict=False).relative_to(root.resolve(strict=False))
    except (OSError, ValueError):
        return ""
    return str(relative).replace("\\", "/").strip("/").casefold()


def image_metadata(path: Path) -> dict[str, object]:
    with Image.open(path) as image:
        width, height = image.size
        image_format = str(image.format or path.suffix.removeprefix(".")).upper()
        frames = int(getattr(image, "n_frames", 1) or 1)
    return {
        "name": path.name,
        "width": width,
        "height": height,
        "bytes": path.stat().st_size,
        "format": image_format,
        "frames": frames,
        "animated": image_format == "WEBP" and frames > 1,
    }


def embedded_track_artwork_bytes(path: Path, *, strict: bool = False) -> bytes | None:
    """Read the typed front cover that Windows/Lofty treats as CoverFront."""
    artwork: bytes | None = None
    try:
        if path.suffix.casefold() == ".mp3":
            frames = ID3(path).getall("APIC")
            artwork = next((frame.data for frame in frames if frame.type == 3), None)
        else:
            audio = MutagenFile(path)
            if audio is not None:
                pictures = getattr(audio, "pictures", None)
                if pictures:
                    artwork = next(
                        (picture.data for picture in pictures if picture.type == 3),
                        None,
                    )
                tags = audio.tags
                getall = getattr(tags, "getall", None)
                if artwork is None and callable(getall):
                    # WAV and AIFF carry ID3 APIC frames even though their
                    # audio container is not MP3.
                    artwork = next(
                        (frame.data for frame in getall("APIC") if frame.type == 3),
                        None,
                    )
                if artwork is None and tags:
                    # MP4 covr is an explicitly cover-art atom and does not
                    # expose the ID3/FLAC picture-type distinction.
                    covers = tags.get("covr")
                    if covers:
                        artwork = bytes(covers[0])
                    else:
                        for raw in tags.get("metadata_block_picture", []):
                            picture = Picture(base64.b64decode(raw))
                            if picture.type == 3:
                                artwork = picture.data
                                break
    except ID3NoHeaderError:
        return None
    except (OSError, ValueError):
        if strict:
            raise
        return None
    return artwork


def embedded_track_image(path: Path) -> bytes | None:
    artwork = embedded_track_artwork_bytes(path)
    if not artwork:
        return None
    try:
        with Image.open(BytesIO(artwork)) as opened:
            image = ImageOps.exif_transpose(opened)
            image.thumbnail((2400, 2400), Image.Resampling.LANCZOS)
            output = BytesIO()
            image.convert("RGB").save(output, format="PNG")
            return output.getvalue()
    except (OSError, ValueError):
        return None


def embedded_album_artwork_summary(
    paths: list[Path], preferred_side: int
) -> dict[str, object]:
    """Return the same bounded Selected Artwork facts shown by Windows.

    Missing or unreadable front-cover frames remain part of the track count.
    The current-art ribbon describes the largest readable frame, while the
    footer reports the aggregate embedded bytes for the selected Album only.
    """
    infos: list[tuple[int, int, int] | None] = []
    for path in paths:
        artwork = embedded_track_artwork_bytes(path)
        if not artwork:
            infos.append(None)
            continue
        try:
            with Image.open(BytesIO(artwork)) as opened:
                opened.verify()
            with Image.open(BytesIO(artwork)) as opened:
                infos.append((int(opened.width), int(opened.height), len(artwork)))
        except (OSError, ValueError):
            infos.append(None)

    readable = [info for info in infos if info is not None]
    dimensions = [(width, height) for width, height, _ in readable]
    resolution_varying = bool(dimensions) and any(
        dimension != dimensions[0] for dimension in dimensions[1:]
    )
    if not dimensions:
        resolution = "No embedded art"
    elif resolution_varying:
        resolution = "Varying Size"
    else:
        resolution = f"{dimensions[0][0]} × {dimensions[0][1]}"

    current = max(
        readable,
        key=lambda info: (info[0] * info[1], info[2]),
        default=None,
    )
    varying = bool(current) and any(info != current for info in infos)
    below_preference = any(
        info is None or info[0] < preferred_side or info[1] < preferred_side
        for info in infos
    )
    # Physical readback requires exact square dimensions. The footer must not
    # mark a large-but-wrong or nonsquare embedded image as preferred quality.
    outside_preference = any(
        info is None or info[0] != preferred_side or info[1] != preferred_side
        for info in infos
    )
    return {
        "track_count": len(infos),
        "embedded_count": len(readable),
        "resolution": resolution,
        "bytes": sum(info[2] for info in readable),
        "below_preference": below_preference,
        "outside_preference": outside_preference,
        "varying": varying,
        "current_width": current[0] if current else 0,
        "current_height": current[1] if current else 0,
        "current_bytes": current[2] if current else 0,
    }


@dataclass
class Catalog:
    music: Path
    database: Database | Path
    artist_art: Path
    credentials: Path
    various_artist_path: Path | None = None
    soundtracks_path: Path | None = None
    curated_compilations_path: Path | None = None
    category_overrides: dict[str, str] = field(default_factory=dict)
    comp_thumbnail_side: int = 1200
    allow_comp_repairs: bool = False
    cover_stem: str = "cover"
    cache: Path | None = None
    temporary_cache: Path | None = None
    preview_cache: Path | None = None
    artist_separator_regex: str = DEFAULT_ARTIST_SEPARATOR_REGEX
    artist_separator_blocked: bool = False
    artist_sources: tuple[str, ...] = (
        "fanarttv", "deezer", "lastfm", "discogs", "itunes", "navidrome"
    )
    images: dict[str, Path | bytes] = field(default_factory=dict)
    metadata_cache: dict[str, dict[str, object]] = field(default_factory=dict)
    metadata_locks: dict[str, Lock] = field(default_factory=dict, repr=False)
    appearance_cache: dict[str, tuple[str, dict[str, object]]] = field(default_factory=dict)
    artist_projection_cache: dict[str, dict[str, object]] | None = None
    artist_projection_generation: str = ""
    artist_projection_pending: bool = False
    artist_projection_build_lock: Lock = field(default_factory=Lock, repr=False)
    artist_projection_warmup_lock: Lock = field(default_factory=Lock, repr=False)
    portrait_cache: dict[str, tuple[str, Path]] = field(default_factory=dict)
    portrait_retry_at: dict[str, float] = field(default_factory=dict)
    portrait_failures: dict[str, int] = field(default_factory=dict)
    navidrome_artist_directory: dict[str, dict[str, object]] = field(default_factory=dict)
    navidrome_directory_retry_at: float = 0.0
    navidrome_directory_failures: int = 0
    navidrome_directory_loading: bool = False
    lock: Lock = field(default_factory=Lock)
    thumbnail_download_slots: BoundedSemaphore = field(
        default_factory=lambda: BoundedSemaphore(3), repr=False
    )
    image_key: bytes = field(default_factory=lambda: secrets.token_bytes(32),repr=False)

    def invalidate_artist_projection(self, *, discard_persisted: bool = True) -> None:
        """Discard the published-generation Artist credit projection.

        Local writes call this at their commit boundary.  A published index
        generation also changes the database token checked by the reader, so
        a maintenance generation can continue without hiding its predecessor
        or forcing every browser request to rebuild the complete projection.
        """
        with self.lock:
            self.artist_projection_cache = None
            self.artist_projection_generation = ""
        if not discard_persisted or not isinstance(self.database, Database):
            return
        try:
            with closing(self.connect()) as db, db:
                db.execute(
                    "DELETE FROM cache_entries WHERE cache_key=?",
                    (ARTIST_PROJECTION_CACHE_KEY,),
                )
        except Exception as error:
            # Read-only SQLite fixtures and legacy disposable schemas do not
            # carry the WebGUI projection cache. Their normal in-memory
            # invalidation remains authoritative.
            LOGGER.debug(
                "Artist projection cache invalidation unavailable: error=%s",
                type(error).__name__,
            )

    def _artist_projection_generation(self, db) -> str:
        try:
            row = db.execute(
                "SELECT active_generation_id FROM catalog_index_state "
                "WHERE singleton=TRUE"
            ).fetchone()
        except (sqlite3.OperationalError, sqlite3.DatabaseError):
            return ""
        generation = str(row["active_generation_id"] or "") if row is not None else ""
        if not isinstance(self.database, Database):
            return generation
        reconciled = db.execute(
            "SELECT run_id FROM sqlite_reconciliation_runs WHERE applied>0 "
            "ORDER BY finished_at DESC NULLS LAST,run_id DESC LIMIT 1"
        ).fetchone()
        return (generation or "unversioned") + "/" + (
            str(reconciled["run_id"]) if reconciled is not None else ""
        )

    def _artist_projection_token(self, generation: str, db=None) -> str:
        """Bind a persisted projection to its catalog and grouping inputs."""
        categories = self.category_overrides.items()
        if db is not None and isinstance(self.database, Database):
            categories = (
                (str(row["album_key"]), str(row["category"]))
                for row in db.execute("SELECT album_key,category FROM album_category_overrides").fetchall()
            )
        grouping = json.dumps(
            {
                "generation": generation,
                "credit_evidence": ARTIST_CREDIT_EVIDENCE_VERSION,
                "separator": self.artist_separator_regex,
                "separator_blocked": self.artist_separator_blocked,
                "categories": sorted(categories),
                "roots": [
                    str(path) if path is not None else None
                    for path in (
                        self.music, self.various_artist_path,
                        self.soundtracks_path, self.curated_compilations_path,
                    )
                ],
            },
            ensure_ascii=False,
            separators=(",", ":"),
        )
        return sha256(grouping.encode("utf-8")).hexdigest()

    def _load_persisted_artist_projection(
        self, db, token: str
    ) -> dict[str, dict[str, object]] | None:
        try:
            row = db.execute(
                "SELECT payload_json FROM cache_entries WHERE cache_key=?",
                (ARTIST_PROJECTION_CACHE_KEY,),
            ).fetchone()
        except Exception:
            return None
        if row is None:
            return None
        try:
            payload = json.loads(str(row["payload_json"] or "{}"))
            if payload.get("token") != token or not isinstance(
                payload.get("artists"), dict
            ):
                return None
            result = {}
            for identity, raw in payload["artists"].items():
                if not isinstance(raw, dict):
                    return None
                projection = dict(raw)
                projection["album_keys"] = set(projection.get("album_keys", ()))
                projection["artist_album_keys"] = set(
                    projection.get("artist_album_keys", ())
                )
                result[str(identity)] = projection
            return result
        except (TypeError, ValueError, json.JSONDecodeError):
            return None

    def _save_persisted_artist_projection(
        self, token: str, projections: dict[str, dict[str, object]]
    ) -> None:
        serializable = {}
        for identity, raw in projections.items():
            projection = dict(raw)
            projection["album_keys"] = sorted(projection.get("album_keys", ()))
            projection["artist_album_keys"] = sorted(
                projection.get("artist_album_keys", ())
            )
            serializable[identity] = projection
        payload = json.dumps(
            {"token": token, "artists": serializable},
            ensure_ascii=False,
            separators=(",", ":"),
        )
        now = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        try:
            with closing(self.connect()) as db, db:
                if isinstance(self.database, Database) and (
                    self._artist_projection_token(self._artist_projection_generation(db), db) != token
                ):
                    return
                db.execute(
                    "INSERT INTO cache_entries"
                    "(cache_key,cache_type,payload_json,created_at,updated_at) "
                    "VALUES(?,?,?,?,?) ON CONFLICT(cache_key) DO UPDATE SET "
                    "cache_type=excluded.cache_type,payload_json=excluded.payload_json,"
                    "updated_at=excluded.updated_at",
                    (
                        ARTIST_PROJECTION_CACHE_KEY,
                        "artist-projection",
                        payload,
                        now,
                        now,
                    ),
                )
        except Exception as error:
            # Persistence is an optimization only. Never make the catalog
            # unavailable because a reduced fixture or legacy cache table is
            # read-only.
            LOGGER.debug(
                "Artist projection cache persistence unavailable: error=%s",
                type(error).__name__,
            )
            return

    def _distinct_category_path(self, value: Path | None) -> Path | None:
        """Return a category override only when it narrows Music Library.

        The settings intentionally display Music Library as their inherited
        default.  Treating that shared root as three active overrides would
        classify every Album three times, so only a distinct descendant is a
        path claim.
        """
        if value is None:
            return None
        candidate = self._portable_path_parts(value)
        music = self._portable_path_parts(self.music)
        if len(candidate) <= len(music) or candidate[:len(music)] != music:
            return None
        return value

    @staticmethod
    def _portable_path_parts(value: Path | str) -> tuple[str, ...]:
        """Normalize Windows and container paths for category matching."""
        return tuple(
            artist_unicode.casefold(part)
            for part in str(value).strip().replace("\\", "/").split("/")
            if part and part != "."
        )

    def _category_claim_depth(
        self, folder: Path, configured: Path | None
    ) -> int | None:
        """Match one configured root to native or imported catalog paths."""
        root = self._distinct_category_path(configured)
        if root is None:
            return None
        root_parts = self._portable_path_parts(root)
        parts = self._portable_path_parts(folder)
        if len(parts) >= len(root_parts) and parts[:len(root_parts)] == root_parts:
            return len(root_parts)
        music_parts = self._portable_path_parts(self.music)
        claim = root_parts[len(music_parts):]
        if not claim or len(parts) <= len(claim):
            return None
        for start in range(0, len(parts) - len(claim) + 1):
            if parts[start:start + len(claim)] == claim:
                return len(claim)
        return None

    def album_library_group(self, row, *, artist_catalog: bool = False) -> str:
        """Return the single top-level/category placement for one Album."""
        keys = set(row.keys()) if hasattr(row, "keys") else set(row)
        album_key = str(row["album_key"] or "") if "album_key" in keys else ""
        override = (
            row["art004_category_override"] if "art004_category_override" in keys
            else self.category_overrides.get(album_key, "")
        )
        if override in {
            "artist", "various-artists", "soundtracks", "fallback",
            "misc-curated", "misc-undefined",
        }:
            return override
        if not artist_catalog and all(
            value is None
            for value in (
                self.various_artist_path,
                self.soundtracks_path,
                self.curated_compilations_path,
            )
        ):
            return (
                "fallback"
                if "musicbrainz_albumid" in keys
                and not str(row["musicbrainz_albumid"] or "").strip()
                else "artist"
            )
        folder = Path(str(row["path"] or ""))
        explicit = []
        for priority, (group, configured) in enumerate((
            ("fallback", self.curated_compilations_path),
            ("soundtracks", self.soundtracks_path),
            ("various-artists", self.various_artist_path),
        )):
            depth = self._category_claim_depth(folder, configured)
            if depth is not None:
                explicit.append((depth, -priority, group))
        if explicit:
            return max(explicit)[2]

        # Configured category roots are the sole automatic grouping authority.
        # An Album outside those roots is an Artist Album. Operator exceptions
        # are handled above by the durable per-Album override.
        return "artist"

    def reload_category_overrides(self) -> dict[str, str]:
        """Load the small operator exception map once, outside Album list loops."""
        try:
            with closing(self.connect()) as db:
                rows = db.execute(
                    "SELECT album_key,category FROM album_category_overrides"
                ).fetchall()
        except (sqlite3.OperationalError, sqlite3.DatabaseError):
            rows = []
        loaded = {
            str(row["album_key"]): str(row["category"])
            for row in rows
        }
        with self.lock:
            self.category_overrides = loaded
        # The persisted token includes the complete override map, so startup
        # can reuse a projection built from the same published generation and
        # settings instead of deleting it before the first browser request.
        self.invalidate_artist_projection(discard_persisted=False)
        return dict(loaded)

    def set_album_category(self, album_id: str, category: str) -> dict[str, object]:
        """Persist one explicit Album placement without changing tags or rescanning."""
        album_id = str(album_id or "").strip()
        category = str(category or "").strip().casefold()
        allowed = {
            "artist", "various-artists", "soundtracks", "fallback",
            "misc-curated", "misc-undefined",
        }
        if not album_id:
            raise ValueError("Choose an Album to categorize")
        if category and category not in allowed:
            raise ValueError("Choose a supported Album category")
        with closing(self.connect()) as db, db:
            row = db.execute(
                "SELECT albums.album_key,albums.path,albums.compilation,"
                "albums.musicbrainz_albumid,artists.artist_name "
                "FROM albums JOIN artists ON artists.artist_key=albums.artist_key "
                "WHERE albums.album_key=?",
                (album_id,),
            ).fetchone()
            if row is None:
                raise KeyError("Album not found")
            if category:
                db.execute(
                    "INSERT INTO album_category_overrides(album_key,category,updated_at) "
                    "VALUES(?,?,CURRENT_TIMESTAMP) "
                    "ON CONFLICT(album_key) DO UPDATE SET "
                    "category=excluded.category,updated_at=CURRENT_TIMESTAMP",
                    (album_id, category),
                )
            else:
                db.execute(
                    "DELETE FROM album_category_overrides WHERE album_key=?",
                    (album_id,),
                )
        with self.lock:
            if category:
                self.category_overrides[album_id] = category
            else:
                self.category_overrides.pop(album_id, None)
        self.invalidate_artist_projection(discard_persisted=False)
        return {
            "id": album_id,
            "category_override": category,
            "library_group": self.album_library_group(row),
        }

    def artist_album_allowed(self, row) -> bool:
        """Artist cards contain only Artist releases and their track credits."""
        return self.album_library_group(row, artist_catalog=True) == "artist"

    @staticmethod
    def readable_image_path(path: Path) -> bool:
        if (
            not path.is_file()
            or path.is_symlink()
            or path.suffix.casefold() not in IMAGE_EXTENSIONS
        ):
            return False
        try:
            with Image.open(path) as opened:
                opened.verify()
            return True
        except (OSError, ValueError):
            return False

    def album_cover_path(self, row) -> Path | None:
        """Resolve readable canonical cover art instead of trusting a stale path."""
        raw = str(row["cover_path"] or "").strip()
        indexed = Path(raw) if raw else None
        keys = set(row.keys())
        folder_value = row["path"] if "path" in keys else row["album_path"]
        folder = Path(str(folder_value))
        if not inside(folder, self.music) or not folder.is_dir():
            return None
        indexed_valid = bool(
            indexed is not None
            and self.readable_image_path(indexed)
            and inside(indexed, self.music)
            and indexed.resolve(strict=False).parent == folder.resolve(strict=False)
        )
        if indexed_valid and indexed is not None and indexed.suffix.casefold() != ".webp":
            return indexed
        stem = (self.cover_stem or "cover").strip().casefold()
        try:
            files = sorted(
                [
                    candidate
                    for candidate in folder.iterdir()
                    if self.readable_image_path(candidate)
                ],
                key=lambda candidate: candidate.name.casefold(),
            )
        except OSError:
            return None

        def modified(candidate: Path) -> int:
            try:
                return candidate.stat().st_mtime_ns
            except OSError:
                return -1

        for prefix in dict.fromkeys((stem, "cover")):
            exact = [
                candidate
                for candidate in files
                if candidate.stem.casefold() == prefix
            ]
            # cover.webp is durable primary/source artwork. When a generated
            # still exists beside it, the still is the card presentation and
            # the WebP remains available to the animation preview workflow.
            static = [
                candidate
                for candidate in exact
                if candidate.suffix.casefold() != ".webp"
            ]
            if static:
                return max(
                    static,
                    key=lambda candidate: (
                        modified(candidate),
                        candidate.name.casefold(),
                    ),
                )
            if exact:
                return max(
                    exact,
                    key=lambda candidate: (
                        modified(candidate),
                        candidate.name.casefold(),
                    ),
                )
        if indexed_valid:
            return indexed
        return None

    def _album_write_coordination(self, db, albums) -> dict[str, dict[str, object]]:
        """Project the newest write onto the current catalog Album identity.

        A tag correction can legitimately change a MusicBrainz-backed Album
        key in one client before the other client refreshes its catalog.  The
        coordination event therefore carries both the canonical key and the
        library-relative folder.  Prefer the newest matching event across
        those two identities so the same physical Album does not lose its
        opposing-writer presentation while catalogs converge.
        """
        targets = []
        for album in albums:
            if isinstance(album, str):
                key, path = album, ""
            else:
                key = str(album["album_key"] or "")
                path = str(album["path"] or "") if "path" in album.keys() else ""
            if not key.strip():
                continue
            targets.append(
                {
                    "album_key": key,
                    "relative_path": catalog_relative_album_path(path, self.music)
                    if path
                    else "",
                }
            )
        if not targets:
            return {}
        keys = list(dict.fromkeys(target["album_key"] for target in targets))
        paths = list(
            dict.fromkeys(
                target["relative_path"]
                for target in targets
                if target["relative_path"]
            )
        )
        clauses = ["album_key IN (" + ",".join("?" for _ in keys) + ")"]
        params = list(keys)
        if paths:
            clauses.append("relative_path IN (" + ",".join("?" for _ in paths) + ")")
            params.extend(paths)
        try:
            rows = db.execute(
                "SELECT album_key,relative_path,writer,operation,event_id,"
                "source_completed_ms,revision,completed_at "
                "FROM album_artwork_write_coordination "
                "WHERE " + " OR ".join(clauses),
                tuple(params),
            ).fetchall()
        except Exception:
            # Small SQLite fixtures and pre-migration disposable schemas do
            # not carry the PostgreSQL coordination table.
            return {}
        return self._project_album_write_coordination(rows, targets)

    @staticmethod
    def _project_album_write_coordination(rows, targets):
        by_key: dict[str, tuple[tuple[int, str], dict[str, object]]] = {}
        by_path: dict[str, tuple[tuple[int, str], dict[str, object]]] = {}
        for raw in rows:
            row = dict(raw)
            rank = (
                int(row.get("source_completed_ms") or 0),
                str(row.get("event_id") or ""),
            )
            key = str(row.get("album_key") or "")
            path = normalized_coordination_relative(row.get("relative_path"))
            if key and (key not in by_key or rank > by_key[key][0]):
                by_key[key] = (rank, row)
            if path and (path not in by_path or rank > by_path[path][0]):
                by_path[path] = (rank, row)
        projected = {}
        for target in targets:
            candidates = []
            if target["album_key"] in by_key:
                candidates.append(by_key[target["album_key"]])
            if target["relative_path"] in by_path:
                candidates.append(by_path[target["relative_path"]])
            if not candidates:
                continue
            _, selected = max(candidates, key=lambda item: item[0])
            selected = dict(selected)
            selected["source_album_key"] = selected["album_key"]
            selected["album_key"] = target["album_key"]
            projected[target["album_key"]] = selected
        return projected

    def album_write_coordination_rows(
        self, coordination_rows
    ) -> list[dict[str, object]]:
        """Return coordination projected onto the published WebGUI catalog."""
        with closing(self.connect()) as db:
            albums = db.execute("SELECT album_key,path FROM albums").fetchall()
        targets = [
            {
                "album_key": str(album["album_key"]),
                "relative_path": catalog_relative_album_path(album["path"], self.music),
            }
            for album in albums
        ]
        rows = self._project_album_write_coordination(coordination_rows, targets)
        return sorted(rows.values(), key=lambda row: int(row.get("revision") or 0))

    def allowed_image_path(self, source: Path) -> bool:
        return (
            inside(source, self.music)
            or inside(source, self.artist_art)
            or self.cache is not None and inside(source, self.cache)
            or self.temporary_cache is not None and inside(source, self.temporary_cache)
        )

    @staticmethod
    def supported_image_path(source: Path) -> bool:
        return source.suffix.casefold() in IMAGE_EXTENSIONS

    def connect(self) -> sqlite3.Connection:
        if isinstance(self.database, Database):
            return self.database.connect()
        connection = sqlite3.connect(self.database.resolve().as_uri() + "?mode=ro", uri=True)
        connection.row_factory = sqlite3.Row
        connection.execute("PRAGMA query_only=ON")
        return connection

    def _connect_artist_catalog(self):
        db = self.connect()
        if isinstance(self.database, Database):
            try:
                db.execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            except Exception:
                db.close()
                raise
        return db

    def image(self, source: Path | bytes | None) -> str | None:
        if source is None:
            return None
        if isinstance(source, Path):
            if not self.supported_image_path(source) or not source.is_file():
                return None
            if not self.allowed_image_path(source):
                return None
            # Operation workspaces are deleted as soon as their owner finishes.
            # Publish immutable bytes into the session preview cache before
            # returning a browser handle so active previews do not pin workdirs.
            if (
                self.temporary_cache is not None
                and inside(source, self.temporary_cache)
                and not inside(source, self.temporary_cache / "web-images")
            ):
                try:
                    return self.image(source.read_bytes())
                except OSError:
                    return None
            # Signed path handles do not consume the bounded in-memory preview
            # cache, so browsing a large library cannot expire its early covers.
            resolved = source.resolve()
            stat = resolved.stat()
            encoded=base64.urlsafe_b64encode(str(resolved).encode('utf-8')).decode().rstrip('=')
            version=f'{stat.st_mtime_ns}:{stat.st_size}'
            signature=hmac.new(self.image_key,(encoded+':'+version).encode(),hashlib.sha256).hexdigest()
            # The version changes the browser URL when the file changes.  It is
            # signed with the path, but validation deliberately does not stat
            # the file again: a live write may replace the same allowed path
            # after a job has captured its handle.
            version_token=base64.urlsafe_b64encode(version.encode()).decode().rstrip('=')
            return '/api/image/p.'+encoded+'.'+version_token+'.'+signature
        preview_cache = self.temporary_cache or self.cache
        if preview_cache is not None:
            # Candidate originals must outlive repeated enhancement renders.
            # Keep generated pixels on disk, independent from the small legacy
            # fixture cache; staged jobs hold their own verified snapshot.
            with Image.open(BytesIO(source)) as opened:
                image_format = str(opened.format or "").upper()
                # Animated local WebP sources commonly exceed the upload-sized
                # 25 MB guard. They are already validated library artwork and
                # must remain previewable; the upload endpoint keeps its own
                # independent 25 MB boundary for untrusted browser uploads.
                if len(source) > 25 * 1024 * 1024 and image_format != "WEBP":
                    raise ValueError('Prepared artwork exceeds the 25 MB image limit')
                suffix={"JPEG":".jpg","PNG":".png","WEBP":".webp"}.get(image_format)
                if not suffix: raise ValueError('Unsupported artwork image format')
            directory=self.preview_cache or preview_cache/'web-images'
            path=directory/(sha256(source).hexdigest()+suffix)
            try:
                # A preview is immutable once published. Serialize the existence
                # check and publication so concurrent requests cannot replace a
                # file that Windows is already serving to the browser.
                with self.lock:
                    directory.mkdir(parents=True,exist_ok=True)
                    if not path.is_file():
                        descriptor,temporary=tempfile.mkstemp(prefix='.prepared-',suffix=suffix,dir=directory)
                        try:
                            with os.fdopen(descriptor,'wb') as handle: handle.write(source)
                            os.replace(temporary,path)
                        finally: Path(temporary).unlink(missing_ok=True)
            except OSError as error:
                raise ValueError('Unable to prepare the artwork preview. Check write access to the preview cache and try again.') from error
            return self.image(path)
        token = secrets.token_urlsafe(18)
        with self.lock:
            self.images[token] = source
            # Tokens are short-lived UI references. Bound memory when users
            # browse a large library for an extended session.
            while len(self.images)>100 or sum(len(value) for value in self.images.values() if isinstance(value,bytes))>128*1024*1024:
                del self.images[next(iter(self.images))]
        return "/api/image/" + token

    def image_source(self, image_url: str) -> Path | bytes:
        if not image_url.startswith("/api/image/"):
            raise ValueError("Choose an image from Selected Artwork")
        token=image_url.removeprefix('/api/image/')
        if token.startswith('p.'):
            try:
                _,encoded,version_token,signature=token.split('.')
                source=Path(base64.urlsafe_b64decode(encoded+'='*(-len(encoded)%4)).decode('utf-8'))
                if not self.supported_image_path(source): raise ValueError('Unsupported artwork type')
                version=base64.urlsafe_b64decode(version_token+'='*(-len(version_token)%4)).decode('ascii')
                expected=hmac.new(self.image_key,(encoded+':'+version).encode(),hashlib.sha256).hexdigest()
                if not hmac.compare_digest(signature,expected): raise ValueError('Invalid artwork handle')
            except (OSError,ValueError,UnicodeError): raise ValueError('Invalid artwork handle') from None
        else:
            with self.lock:
                source = self.images.get(token)
        if source is None:
            raise KeyError("Image preview expired; select the artwork again")
        if isinstance(source, Path):
            # A signed handle can outlive the file it described when a cover is
            # replaced or preserved under another name.  ``inside`` resolves
            # paths strictly, so testing authorization first used to report a
            # missing, formerly valid image as a security-boundary violation.
            # Keep the two cases distinct without weakening the boundary for a
            # path which still exists outside the configured artwork roots.
            if not source.is_file():
                raise KeyError(
                    "Image preview expired after the artwork changed; "
                    "reload the Album or select the artwork again"
                )
            if not self.allowed_image_path(source):
                raise ValueError("Image is outside configured artwork locations")
        return source

    @staticmethod
    def fallback_thumbnail_url(album_id: str, track_key: str) -> str:
        return (
            "/api/fallback/thumbnail?id="
            + quote(str(album_id), safe="")
            + "&key="
            + quote(str(track_key), safe="")
        )

    @staticmethod
    def artist_thumbnail_url(artist_id: str) -> str:
        return "/api/artist/thumbnail?id=" + quote(str(artist_id), safe="")

    @staticmethod
    def album_thumbnail_url(album_id: str) -> str:
        return "/api/album/thumbnail?id=" + quote(str(album_id), safe="")

    @staticmethod
    def fallback_thumbnail_keys(db) -> dict[str, str]:
        """Return one recorded embedded-art track per fallback album."""
        tables = {
            str(row["name"])
            for row in db.execute("SELECT name FROM sqlite_master WHERE type='table'")
        }
        if not {"tracks", "compilation_track_artwork"}.issubset(tables):
            return {}
        result: dict[str, str] = {}
        for row in db.execute(
            "SELECT tracks.album_key,tracks.track_key FROM tracks "
            "JOIN compilation_track_artwork ON lower(compilation_track_artwork.track_path)=lower(tracks.path) "
            "WHERE compilation_track_artwork.outcome IN ('embedded-replaced','embedded-existing') "
            "ORDER BY tracks.album_key,tracks.path"
        ):
            result.setdefault(str(row["album_key"]), str(row["track_key"]))
        return result

    def image_bytes(self,image_url: str) -> bytes:
        source=self.image_source(image_url)
        return source.read_bytes() if isinstance(source,Path) else source

    @staticmethod
    def _aggregate_album_status(rows) -> str:
        evidence: set[str] = set()
        album_states = []
        for row in rows:
            conditions = set(row.get("conditions", ())) if hasattr(row, "get") else set()
            if not conditions:
                status = str(row["status"] or "unprocessed").casefold()
                if status == "partial":
                    conditions.add("embedding_issues")
                if status in {"incomplete", "timeout", "error"}:
                    conditions.add("incomplete_or_error")
                if status == "overridden":
                    conditions.add("opposing_override")
                if status in {"complete", "processed", "overridden"}:
                    conditions.add("complete_or_overridden")
            evidence.update(conditions)
            album_states.append(conditions)
        if album_states and all(
            "complete_or_overridden" in conditions for conditions in album_states
        ):
            evidence.add("all_albums_complete_or_overridden")
        status = classify_media_state("artist", evidence)["ui_status_key"]
        # Catalog APIs retain the established terminal label. The browser
        # translates it to the shared albumsComplete UI key when filtering.
        return "complete" if status == "albumsComplete" else status

    @staticmethod
    def _artist_album_state_conditions(
        row, coordination: dict[str, object] | None = None
    ) -> set[str]:
        """Return Artist-rollup evidence from already-indexed Album state.

        An absent static Album cover contributes Yellow to the Artist name.
        Never resolve or scan Album artwork paths while listing Artists.
        Lazy Artist portrait/banner inspection remains a separate input.
        """
        keys = set(row.keys())
        status = str(row["status"] or "unprocessed").casefold() if "status" in keys else "unprocessed"
        writer = str((coordination or {}).get("writer", "")).casefold()
        conditions: set[str] = set()
        if writer == "windows" or status == "overridden":
            # Opposing ownership is authoritative presentation evidence. Do
            # not also inherit a stale pre-write incomplete label.
            return {"opposing_override", "complete_or_overridden"}
        if "cover_path" in keys:
            # Read only indexed catalog evidence: a WebP may be previewable,
            # but only a qualifying static cover.* satisfies Artist Yellow.
            cover_path = str(row["cover_path"] or "").strip()
            static_cover = bool(cover_path and Path(cover_path).suffix.casefold() != ".webp")
            if "cover_found" in keys:
                static_cover = static_cover and bool(row["cover_found"])
            if not static_cover:
                conditions.add("missing_artist_art")
        if status == "partial":
            conditions.add("embedding_issues")
        if status in {"incomplete", "timeout", "error"}:
            conditions.add("incomplete_or_error")
        if {
            "track_count",
            "embedded_tracks",
            "embedded_validation_status",
        }.issubset(keys):
            track_count = int(row["track_count"] or 0)
            validation = str(row["embedded_validation_status"] or "").casefold()
            embedded = int(row["embedded_tracks"] or 0)
            if validation in {"retry", "error"} or (
                validation == "verified" and track_count > 0 and embedded < track_count
            ):
                conditions.add("embedding_issues")
            if validation == "verified" and track_count > 0 and embedded >= track_count:
                conditions.add("complete_or_overridden")
        return conditions

    @staticmethod
    def _effective_album_status(row, coordination: dict[str, object] | None = None) -> str:
        """Classify current Album authority without trusting a stale status column.

        An opposing Windows write is completed authority. For a local WebGUI
        Album, a present cover plus a verified full embedded-track count is
        equally authoritative even when an older refresh left ``status`` as
        ``unprocessed``. Explicit bypass/review states remain actionable and
        are never hidden by the materialized completion evidence.
        """
        keys = set(row.keys())
        status = str(row["status"] or "unprocessed").casefold() if "status" in keys else "unprocessed"
        if str((coordination or {}).get("writer", "")).casefold() == "windows":
            return "processed"
        if status in {"bypassed", "timeout", "partial", "error"}:
            return status
        if {
            "cover_found",
            "track_count",
            "embedded_tracks",
            "embedded_validation_status",
        }.issubset(keys):
            track_count = int(row["track_count"] or 0)
            if (
                bool(row["cover_found"])
                and track_count > 0
                and str(row["embedded_validation_status"] or "").casefold()
                == "verified"
                and int(row["embedded_tracks"] or 0) >= track_count
            ):
                return "processed"
        return status

    @staticmethod
    def _album_list_status_key(
        row, coordination: dict[str, object] | None = None
    ) -> str:
        """Classify an Album-list name from indexed Album evidence only."""
        keys = set(row.keys())
        raw_status = str(row["status"] or "unprocessed").casefold()
        conditions: set[str] = set()
        overridden = str((coordination or {}).get("writer", "")).casefold() == "windows"
        if overridden or raw_status == "overridden":
            conditions.add("opposing_override")
        elif raw_status in {"incomplete", "timeout", "error"}:
            conditions.add("incomplete_tracks")

        cover_path = str(row["cover_path"] or "").strip() if "cover_path" in keys else ""
        static_cover = bool(cover_path and Path(cover_path).suffix.casefold() != ".webp")
        if "cover_found" in keys:
            static_cover = static_cover and bool(row["cover_found"])
        if not overridden and (not static_cover or raw_status == "bypassed"):
            conditions.add("missing_art")

        track_count = int(row["track_count"] or 0) if "track_count" in keys else 0
        embedded = int(row["embedded_tracks"] or 0) if "embedded_tracks" in keys else 0
        validation = (
            str(row["embedded_validation_status"] or "unknown").casefold()
            if "embedded_validation_status" in keys else "unknown"
        )
        complete = validation == "verified" and track_count > 0 and embedded >= track_count
        attention = raw_status == "partial" or validation in {"retry", "error"} or (
            validation == "verified" and track_count > 0 and embedded < track_count
        )
        if attention:
            conditions.add("embedding_issues")
        if complete:
            conditions.add("tracks_complete")
        elif Catalog._effective_album_status(row, coordination) == "processed":
            conditions.add("processed")
        return str(classify_media_state("album", conditions)["ui_status_key"])

    @staticmethod
    def _aggregate_stored_artist_status(rows) -> str:
        """Compatibility fallback for reduced, pre-status Album fixtures."""
        statuses = {str(row["status"] or "unprocessed").casefold() for row in rows}
        if statuses == {"processed"}:
            return "processed"
        if "partial" in statuses or "error" in statuses or len(statuses) > 1:
            return "partial"
        return next(iter(statuses), "unprocessed")

    @staticmethod
    def _canonical_artist_row(rows):
        return max(
            rows,
            key=lambda row: (
                # A projected credit participates in presentation aliases but
                # never replaces an available durable owner as the selected
                # PostgreSQL identity.
                not str(row["artist_key"] or "").startswith("credit:"),
                # A tagged MusicBrainz identity is the stable authority even
                # when a legacy name-only duplicate currently owns more album
                # rows.  The aliases below still contribute every album.
                bool(str(row["musicbrainz_artistid"] or "").strip()),
                int(row["album_count"] or 0),
                str(row["artist_key"]),
            ),
        )

    @staticmethod
    def _artist_groups(rows, projected=None):
        """Build non-destructive presentation groups from local identity evidence.

        PostgreSQL may retain several owner rows for Albums carrying different,
        missing, or conflicting MusicBrainz Artist IDs.  Those database rows and
        their Album associations remain independent; MBID variation must not
        manufacture duplicate rows in the visible Artist list. Equal normalized
        names always share a presentation. A non-empty local MBID may also join
        differently named owner rows and positionally paired local ``credit:``
        rows. Credit rows without an unambiguous local MBID remain name-only.
        """
        rows = list(rows)
        parents = list(range(len(rows)))

        def find(index: int) -> int:
            while parents[index] != index:
                parents[index] = parents[parents[index]]
                index = parents[index]
            return index

        def union(left: int, right: int) -> None:
            left_root = find(left)
            right_root = find(right)
            if left_root != right_root:
                parents[right_root] = left_root

        by_name: dict[str, int] = {}
        ids_by_name: dict[str, set[str]] = {}
        for index, row in enumerate(rows):
            identity = normalized_artist_identity(row["artist_name"])
            if identity in by_name:
                union(index, by_name[identity])
            else:
                by_name[identity] = index
            keys = set(row.keys()) if hasattr(row, "keys") else set(row)
            mbid_field = (
                "alias_musicbrainz_artistid"
                if "alias_musicbrainz_artistid" in keys
                else "musicbrainz_artistid"
            )
            mbids = ordered_artist_mbids(row[mbid_field])
            if not synthetic_album_artist(identity):
                ids_by_name.setdefault(identity, set()).update(mbids)
            if projected:
                ids_by_name.setdefault(identity, set()).update(
                    projected.get(identity, {}).get("paired_artist_ids", ())
                )

        # Equal names always group for presentation. A name tied to conflicting
        # IDs cannot bridge a different name into that presentation.
        by_mbid: dict[str, int] = {}
        for identity, index in by_name.items():
            mbids = ids_by_name.get(identity, set())
            if len(mbids) == 1 and not synthetic_album_artist(identity):
                mbid = next(iter(mbids))
                union(index, by_mbid.setdefault(mbid, index))

        groups: dict[int, list] = {}
        for index, row in enumerate(rows):
            groups.setdefault(find(index), []).append(row)
        return list(groups.values())

    @staticmethod
    def _artist_group_projection(projected, group) -> dict[str, object]:
        """Merge presentation evidence without changing persisted identities."""
        entries = [
            projected[identity]
            for identity in dict.fromkeys(
                normalized_artist_identity(row["artist_name"]) for row in group
            )
            if identity in projected
        ]
        if not entries:
            return {}
        preferred = max(
            entries,
            key=lambda value: (
                bool(str(value.get("musicbrainz_artistid") or "").strip()),
                len(value.get("artist_album_keys", set())),
                normalized_artist_identity(str(value.get("artist_name") or "")),
            ),
        )
        merged = {
            "artist_key": preferred["artist_key"],
            "artist_name": preferred["artist_name"],
            "musicbrainz_artistid": preferred.get("musicbrainz_artistid", ""),
            "primary_path": preferred.get("primary_path", ""),
            "album_keys": set(),
            "artist_album_keys": set(),
            "statuses": {},
            "comp_statuses": {},
        }
        for value in entries:
            merged["album_keys"].update(value.get("album_keys", set()))
            merged["artist_album_keys"].update(value.get("artist_album_keys", set()))
            merged["statuses"].update(value.get("statuses", {}))
            for album, status in value.get("comp_statuses", {}).items():
                if album not in merged["comp_statuses"] or status == "incomplete":
                    merged["comp_statuses"][album] = status
        return merged

    @classmethod
    def _artist_selection_groups(cls, rows, projected):
        groups = cls._artist_groups(rows, projected)
        selections = [(cls._canonical_artist_row(group), group) for group in groups]
        key_counts: dict[str, int] = {}
        for canonical, group in selections:
            for key in {str(row["artist_key"]) for row in group}:
                key_counts[key] = key_counts.get(key, 0) + 1
        for canonical, group in selections:
            key = str(canonical["artist_key"])
            if key_counts[key] > 1:
                key = projected_artist_id(min(
                    (str(row["artist_name"]) for row in group),
                    key=lambda name: (normalized_artist_identity(name), name),
                ))
            yield key, canonical, group

    @staticmethod
    def _artist_display_name(
        group, projected, owned_album_keys: dict[str, set[str]] | None = None
    ) -> str:
        """Apply ART-004's identical local display-label precedence."""
        candidates: list[tuple[tuple, str]] = []
        ownership_by_label: dict[str, set[str]] = {}
        owner_label_albums: dict[tuple[str, str], set[str]] = {}
        owners = []
        group_identities = tuple(dict.fromkeys(
            normalized_artist_identity(row["artist_name"]) for row in group
        ))
        for row in group:
            label = str(row["artist_name"] or "").strip()
            artist_key = str(row["artist_key"] or "")
            if not label or artist_key.startswith("credit:"):
                continue
            mbid_field = "alias_musicbrainz_artistid" if "alias_musicbrainz_artistid" in row.keys() else "musicbrainz_artistid"
            mbid = str(row[mbid_field] or "").strip()
            literal = artist_search_literal(label)
            if owned_album_keys is not None:
                album_key = row.get("art004_album_key") if isinstance(row, dict) else None
                owned = owned_album_keys.get(artist_key, set())
                label_albums = owned & {album_key} if album_key is not None else owned
                ownership_by_label.setdefault(literal, set()).update(label_albums)
                owner_label_albums.setdefault((artist_key, literal), set()).update(label_albums)
            owners.append((label, literal, artist_key, mbid, int(row["album_count"] or 0)))
        for label, literal, artist_key, mbid, stored_count in owners:
            owned_count = stored_count if owned_album_keys is None else len(
                owner_label_albums.get((artist_key, literal), ())
            )
            candidates.append((
                (False, bool(ordered_artist_mbids(mbid)), owned_count,
                 artist_search_literal(artist_key), normalized_artist_identity(label), literal, label),
                label,
            ))
        for identity in group_identities:
            value = projected.get(identity)
            if not value:
                continue
            pairs = {pair["name"]: pair["mbid"] for pair in value.get("paired_credit_labels", ())}
            for label in value.get("credit_labels", (value["artist_name"],)):
                mbid = pairs.get(label)
                candidates.append(((
                    bool(mbid),
                    False,
                    len(ownership_by_label.get(artist_search_literal(label), ())),
                    f"mbid:{mbid}" if mbid else "",
                    normalized_artist_identity(label),
                    artist_search_literal(label),
                    label,
                ), label))
        if not candidates:
            return str(Catalog._canonical_artist_row(group)["artist_name"])
        return max(candidates, key=lambda item: item[0])[1]

    @staticmethod
    def _matching_file_evidence(payload, size, mtime):
        if payload:
            try:
                evidence = json.loads(payload)
                if evidence["file_size"] == size and evidence["file_mtime_ns"] == mtime:
                    return evidence
            except (KeyError, ValueError, TypeError):
                LOGGER.warning("Invalid cached original Artist credit for an indexed track")
        return None

    def _artist_owner_inputs(self, db):
        if not isinstance(self.database, Database):
            return [dict(row) for row in db.execute(
                "SELECT albums.*,owners.artist_name,owners.musicbrainz_artistid AS art004_owner_ids "
                "FROM albums JOIN artists owners ON owners.artist_key=albums.artist_key"
            ).fetchall()]
        rows = db.execute(
            "SELECT albums.*,owners.artist_name,owners.musicbrainz_artistid AS art004_owner_ids,"
            "chosen.category AS art004_category_override,"
            "representative.file_size AS art004_rep_size,"
            "representative.file_mtime_ns AS art004_rep_mtime,"
            "evidence.payload_json AS art004_evidence "
            "FROM albums JOIN artists owners ON owners.artist_key=albums.artist_key "
            "LEFT JOIN album_category_overrides chosen ON chosen.album_key=albums.album_key "
            "LEFT JOIN tracks representative ON representative.album_key=albums.album_key "
            "AND representative.path=albums.representative_file "
            "LEFT JOIN cache_entries evidence ON evidence.cache_key='art004:track:'||representative.track_key"
        ).fetchall()
        result = []
        for raw in rows:
            row = dict(raw)
            evidence = self._matching_file_evidence(
                row.pop("art004_evidence"), row.pop("art004_rep_size"), row.pop("art004_rep_mtime"),
            )
            if evidence is not None:
                row["artist_name"] = (
                    first_artist_value(evidence.get("album_artist") or "")
                    or first_artist_value(evidence.get("artist") or "")
                    or row["artist_name"]
                )
                if evidence.get("album_artist_mbids") is not None:
                    row["art004_owner_ids"] = evidence["album_artist_mbids"]
            result.append(row)
        return result

    @staticmethod
    def _artist_owner_candidates(rows, album_inputs):
        per_album = {}
        for album in album_inputs:
            per_album.setdefault(str(album["artist_key"]), []).append(album)
        return [
            {
                **dict(row),
                "artist_name": album["artist_name"],
                "alias_musicbrainz_artistid": album["art004_owner_ids"],
                "art004_album_key": str(album["album_key"]),
            }
            for row in rows
            for album in per_album.get(str(row["artist_key"]), ())
        ] + [dict(row) for row in rows if str(row["artist_key"]) not in per_album]

    def _owned_artist_album_keys(self, db, rows, album_inputs=None) -> dict[str, set[str]]:
        artist_keys = [
            str(row["artist_key"])
            for row in rows
            if not str(row["artist_key"] or "").startswith("credit:")
        ]
        if not artist_keys:
            return {}
        try:
            albums = self._artist_owner_inputs(db) if album_inputs is None else album_inputs
        except sqlite3.OperationalError:
            return {}
        group_album_keys = {
            str(row["art004_album_key"])
            for row in rows if isinstance(row, dict) and "art004_album_key" in row
        }
        allowed_owners = set(artist_keys)
        result: dict[str, set[str]] = {}
        for album in albums:
            if (str(album["artist_key"]) in allowed_owners
                    and (not group_album_keys or str(album["album_key"]) in group_album_keys)
                    and self.artist_album_allowed(album)):
                result.setdefault(str(album["artist_key"]), set()).add(
                    str(album["album_key"])
                )
        return result

    def _projected_track_artists(
        self, db, *, allow_build: bool = True
    ) -> dict[str, dict[str, object]]:
        """Project selectable Artists from Artist-library Album credits.

        This projection owns membership in the left Artist file list.  The
        selected Artist's overview uses the separate appearance projection so
        its V/A, soundtrack, Featured, and curated Album cards remain visible.
        """
        generation = self._artist_projection_generation(db)
        token = self._artist_projection_token(generation, db)
        if generation:
            with self.lock:
                if (
                    self.artist_projection_cache is not None
                    and self.artist_projection_generation == token
                ):
                    LOGGER.debug(
                        "Artist projection cache hit: generation=%s artists=%d",
                        generation,
                        len(self.artist_projection_cache),
                    )
                    return self.artist_projection_cache

            persisted = self._load_persisted_artist_projection(db, token)
            if persisted is not None:
                with self.lock:
                    self.artist_projection_cache = persisted
                    self.artist_projection_generation = token
                    self.artist_projection_pending = False
                LOGGER.info(
                    "Artist projection restored: generation=%s artists=%d",
                    generation,
                    len(persisted),
                )
                return persisted

        # Startup warms this projection on one daemon thread. Interactive
        # requests never become the cold builder, even in the small race before
        # the warmup thread acquires the lock.
        if not allow_build:
            with self.lock:
                self.artist_projection_pending = True
            if isinstance(self.database, Database):
                self.request_artist_projection_warmup()
                raise ArtistProjectionPending("Artist catalog is still loading")
            return {}

        # A second builder receives the currently published result instead of
        # duplicating catalog-wide work and exhausting the PostgreSQL pool.
        if not self.artist_projection_build_lock.acquire(blocking=isinstance(self.database, Database)):
            with self.lock:
                self.artist_projection_pending = True
            return {}
        started = time.perf_counter()
        with self.lock:
            self.artist_projection_pending = True
        try:
            with self.lock:
                if self.artist_projection_generation == token and self.artist_projection_cache is not None:
                    return self.artist_projection_cache
            result, track_count = self._build_projected_track_artists(db)
            if generation:
                with self.lock:
                    self.artist_projection_cache = result
                    self.artist_projection_generation = token
            LOGGER.info(
                "Artist projection ready: generation=%s tracks=%d artists=%d seconds=%.3f",
                generation or "unversioned",
                track_count,
                len(result),
                time.perf_counter() - started,
            )
        finally:
            with self.lock:
                self.artist_projection_pending = False
            self.artist_projection_build_lock.release()
        if generation:
            self._save_persisted_artist_projection(token, result)
        return result

    def warm_artist_projection(self) -> None:
        """Warm or restore the published Artist projection off the HTTP path."""
        try:
            with closing(self._connect_artist_catalog()) as db:
                self._projected_track_artists(db)
        except Exception as error:
            LOGGER.warning(
                "Artist projection warmup unavailable: error=%s",
                type(error).__name__,
            )

    def request_artist_projection_warmup(self) -> None:
        if not self.artist_projection_warmup_lock.acquire(blocking=False):
            return

        def warm():
            try:
                self.warm_artist_projection()
            finally:
                self.artist_projection_warmup_lock.release()

        Thread(target=warm, name="artist-catalog-warmup", daemon=True).start()

    def _build_projected_track_artists(
        self, db
    ) -> tuple[dict[str, dict[str, object]], int]:

        # Fetch the small completion ledger once.  A correlated LOWER(path)
        # EXISTS forced PostgreSQL to rescan that table for every Track because
        # it could not use the track_path primary key.  Normalizing the two
        # bounded result sets in Python is linear and preserves path semantics.
        try:
            completed_paths = {
                str(row["track_path"] or "").casefold()
                for row in db.execute(
                    "SELECT track_path FROM compilation_track_artwork "
                    "WHERE outcome IN ('embedded-replaced','embedded-existing')"
                ).fetchall()
            }
        except (sqlite3.OperationalError, sqlite3.DatabaseError):
            completed_paths = set()

        base = (
            "SELECT albums.album_key,albums.artist_key,albums.status,albums.path AS album_path,"
            "albums.compilation,albums.musicbrainz_albumid,"
            "owners.artist_name AS album_artist,tracks.artist_name,"
            "{artist_mbid} AS musicbrainz_artistid,tracks.title,tracks.path{evidence_columns} "
            "FROM tracks JOIN albums ON albums.album_key=tracks.album_key "
            "JOIN artists owners ON owners.artist_key=albums.artist_key {join_evidence} "
            "ORDER BY albums.album_key,tracks.path COLLATE NOCASE"
        )
        try:
            evidence_columns = join_evidence = ""
            if isinstance(self.database, Database):
                evidence_columns = ",tracks.file_size,tracks.file_mtime_ns,evidence.payload_json AS original_artist_credit"
                join_evidence = "LEFT JOIN cache_entries evidence ON evidence.cache_key='art004:track:'||tracks.track_key"
            rows = db.execute(
                base.format(
                    artist_mbid="tracks.musicbrainz_artistid",
                    evidence_columns=evidence_columns,
                    join_evidence=join_evidence,
                )
            ).fetchall()
        except sqlite3.OperationalError:
            try:
                # Reduced read-only fixtures can predate per-track MBIDs; the
                # real Artist credit remains sufficient for card projection.
                rows = db.execute(base.format(artist_mbid="''", evidence_columns="", join_evidence="")).fetchall()
            except sqlite3.OperationalError:
                return {}, 0
        rows = [
            {
                **dict(row),
                "comp_complete": str(row["path"] or "").casefold() in completed_paths,
            }
            for row in rows
        ]
        try:
            owner_inputs = {
                str(album["album_key"]): album for album in self._artist_owner_inputs(db)
            }
        except sqlite3.OperationalError:
            owner_inputs = {}
        group_by_album = {}

        separator = ArtistSeparator()
        if not self.artist_separator_blocked:
            try:
                separator = parse_artist_separator(self.artist_separator_regex)
            except ValueError:
                # An older publication can retain its incompatible profile.
                # Settings explains why new configured splits are paused.
                pass

        track_edges = []
        for row in rows:
            album_key = str(row["album_key"])
            owner = owner_inputs.get(album_key)
            if album_key not in group_by_album:
                group_by_album[album_key] = self.album_library_group(
                    owner or {**row, "path": row["album_path"]},
                    artist_catalog=True,
                )
            group = group_by_album[album_key]
            owner_name = str(owner["artist_name"] if owner else row["album_artist"] or "")
            filename = Path(str(row["path"] or "").replace("\\", "/")).stem
            artist = str(row["artist_name"] or "")
            mbids = str(row["musicbrainz_artistid"] or "")
            title = str(row["title"] or "")
            if row.get("original_artist_credit") and (evidence := self._matching_file_evidence(
                row["original_artist_credit"], row["file_size"], row["file_mtime_ns"],
            )):
                artist, mbids, title = evidence["artist"], evidence["mbids"], evidence["title"]
            row_credits = catalog_credit_slots(
                artist, mbids, title, filename, owner_name, separator,
            )
            if row_credits:
                track_edges.append((row, album_key, group, owner_name, row_credits))

        result: dict[str, dict[str, object]] = {}
        for row, album_key, group, owner_name, row_credits in track_edges:
            recorded = set()
            track_path = str(row["path"] or "").replace("\\", "/").strip("/")
            for credit, position, paired_mbid, explicit in row_credits:
                identity = normalized_artist_identity(credit)
                projection = result.setdefault(identity, {
                    "artist_key": projected_artist_id(credit),
                    "artist_name": credit,
                    "musicbrainz_artistid": "",
                    "musicbrainz_artistids": set(),
                    "paired_label_ids": {},
                    "credit_labels": set(),
                    "track_credits": {},
                    "album_owners": {},
                    "primary_path": "",
                    "album_keys": set(),
                    "artist_album_keys": set(),
                    "statuses": {},
                    "comp_track_states": {},
                })
                projection["credit_labels"].add(credit)
                if paired_mbid:
                    projection["musicbrainz_artistids"].add(paired_mbid)
                    projection["paired_label_ids"].setdefault(credit, set()).add(
                        paired_mbid
                    )
                projection["album_owners"][album_key] = [
                    str(row["artist_key"]), owner_name,
                ]
                projection["track_credits"].setdefault(album_key, {}).setdefault(
                    track_path, []
                ).append([credit, position, paired_mbid, explicit])
                if group == "fallback":
                    if identity not in recorded:
                        projection["comp_track_states"].setdefault(album_key, []).append(
                            bool(row["comp_complete"])
                        )
                        recorded.add(identity)
                    continue
                projection["album_keys"].add(album_key)
                if group == "artist" and explicit:
                    projection["artist_album_keys"].add(album_key)
                projection["statuses"][album_key] = str(
                    row["status"] or "unprocessed"
                )
        for projection in result.values():
            paired_ids = projection.pop("musicbrainz_artistids", set())
            projection["paired_artist_ids"] = sorted(paired_ids)
            projection["credit_labels"] = sorted(projection["credit_labels"])
            projection["musicbrainz_artistid"] = (
                next(iter(paired_ids)) if len(paired_ids) == 1 else ""
            )
            paired_labels = [
                {"name": name, "mbid": next(iter(ids))}
                for name, ids in projection.pop("paired_label_ids", {}).items()
                if len(ids) == 1
            ]
            paired_labels.sort(
                key=lambda value: (
                    ("mbid:" + value["mbid"]).casefold(),
                    normalized_artist_identity(value["name"]),
                    value["name"].casefold(),
                )
            )
            projection["paired_credit_labels"] = paired_labels
            if paired_labels:
                projection["artist_name"] = paired_labels[-1]["name"]
            projection["comp_statuses"] = {
                album_key: "complete" if states and all(states) else "incomplete"
                for album_key, states in projection.pop("comp_track_states", {}).items()
            }
        return result, len(rows)

    @staticmethod
    def _virtual_artist_row(projection: dict[str, object]) -> dict[str, object]:
        paired_alias_ids = {
            str(value.get("mbid") or "").strip().casefold()
            for value in projection.get("paired_credit_labels", ())
            if normalized_artist_identity(value.get("name", ""))
            == normalized_artist_identity(projection["artist_name"])
            and str(value.get("mbid") or "").strip()
        }
        return {
            "artist_key": projection["artist_key"],
            "artist_name": projection["artist_name"],
            "musicbrainz_artistid": projection["musicbrainz_artistid"],
            "alias_musicbrainz_artistid": (
                next(iter(paired_alias_ids)) if len(paired_alias_ids) == 1 else ""
            ),
            "primary_path": "",
            "album_count": len(projection["album_keys"]),
            "status": "unprocessed",
        }

    def _artist_identity_rows(self, db, artist_id: str, *, require_eligible: bool = False,
                              album_inputs=None):
        try:
            selected = db.execute(
                "SELECT artist_key,artist_name,musicbrainz_artistid,primary_path,"
                "(SELECT COUNT(*) FROM albums WHERE albums.artist_key=artists.artist_key) AS album_count,status "
                "FROM artists WHERE artist_key=?",
                (artist_id,),
            ).fetchone()
            candidates = db.execute(
                "SELECT artist_key,artist_name,musicbrainz_artistid,primary_path,"
                "(SELECT COUNT(*) FROM albums WHERE albums.artist_key=artists.artist_key) AS album_count,status "
                "FROM artists"
            ).fetchall()
        except sqlite3.OperationalError:
            # Small read-only SQLite fixtures predate the full catalog schema.
            try:
                selected = db.execute(
                    "SELECT artist_key,artist_name,musicbrainz_artistid,primary_path,"
                    "0 AS album_count,status FROM artists WHERE artist_key=?",
                    (artist_id,),
                ).fetchone()
                candidates = db.execute(
                    "SELECT artist_key,artist_name,musicbrainz_artistid,primary_path,"
                    "0 AS album_count,status FROM artists"
                ).fetchall()
            except sqlite3.OperationalError:
                selected = db.execute(
                    "SELECT artist_key,artist_name,'' AS musicbrainz_artistid,'' AS primary_path,"
                    "0 AS album_count,'unprocessed' AS status FROM artists WHERE artist_key=?",
                    (artist_id,),
                ).fetchone()
                candidates = db.execute(
                    "SELECT artist_key,artist_name,'' AS musicbrainz_artistid,'' AS primary_path,"
                    "0 AS album_count,'unprocessed' AS status FROM artists"
                ).fetchall()
        projected = self._projected_track_artists(db)
        if album_inputs is None:
            try:
                album_inputs = self._artist_owner_inputs(db)
            except sqlite3.OperationalError:
                album_inputs = []
        candidates = self._artist_owner_candidates(candidates, album_inputs)
        existing_identities = {
            normalized_artist_identity(row["artist_name"]) for row in candidates
        }
        candidates = [*candidates, *(
            self._virtual_artist_row(value)
            for identity, value in projected.items()
            if identity not in existing_identities
        )]
        direct = None
        for key, canonical, group in self._artist_selection_groups(candidates, projected):
            if key == str(artist_id):
                direct = key, canonical, group
                break
            if selected is not None and direct is None and any(
                str(row["artist_key"]) == str(artist_id) for row in group
            ):
                direct = key, canonical, group
        if direct is None:
            raise KeyError("Artist not found")
        selection_id, canonical, aliases = direct
        if require_eligible and synthetic_album_artist(canonical["artist_name"]):
            raise KeyError("Artist not found")
        owned_albums = self._owned_artist_album_keys(db, aliases, album_inputs)
        if require_eligible or str(artist_id).startswith("credit:"):
            appearance = self._artist_group_projection(projected, aliases)
            if not appearance.get("artist_album_keys") and not owned_albums:
                raise KeyError("Artist not found")
        display_name = self._artist_display_name(
            aliases,
            projected,
            owned_albums,
        )
        # The returned mapping overlays presentation text only; the canonical
        # PostgreSQL key, MBID, paths and Album ownership stay intact.
        canonical = {**dict(canonical), "artist_name": display_name, "selection_id": selection_id}
        return canonical, aliases

    @staticmethod
    def _navidrome_auth_query(credential: dict[str, object]) -> dict[str, str]:
        query = {"v": "1.16.1", "c": "SPLINED-webgui", "f": "json"}
        if credential.get("auth_method") == "api_key" and credential.get("api_key"):
            query["apiKey"] = str(credential["api_key"])
        else:
            salt = secrets.token_hex(6)
            password = str(credential.get("password") or "")
            query.update({
                "u": str(credential.get("username") or ""),
                "s": salt,
                "t": hashlib.md5((password + salt).encode(), usedforsecurity=False).hexdigest(),
            })
        return query

    def _navidrome_call(self, credential, method: str, parameters: dict[str, str]):
        base = str(credential.get("server_url") or "").rstrip("/")
        if urlsplit(base).scheme not in {"http", "https"}:
            raise ValueError("Invalid Navidrome URL")
        response = requests.get(
            base + "/rest/" + method + ".view",
            params={**self._navidrome_auth_query(credential), **parameters},
            timeout=15,
            allow_redirects=False,
        )
        if response.status_code != 200:
            raise ValueError("Navidrome did not return library metadata")
        envelope = response.json().get("subsonic-response", {})
        if envelope.get("status") != "ok":
            raise ValueError("Navidrome did not return library metadata")
        return envelope

    @staticmethod
    def _navidrome_song_has_artist(song, artist_id: str, artist_name: str) -> bool:
        artist_id = artist_id.strip().casefold()
        artist_name = normalized_artist_identity(artist_name)
        if artist_id and str(song.get("artistId") or "").strip().casefold() == artist_id:
            return True
        if normalized_artist_identity(song.get("artist", "")) == artist_name:
            return True
        artists = song.get("artists") or []
        if isinstance(artists, dict):
            artists = [artists]
        return any(
            isinstance(item, dict) and (
                artist_id and str(item.get("id") or "").strip().casefold() == artist_id
                or normalized_artist_identity(item.get("name", "")) == artist_name
            )
            for item in artists
        )

    @staticmethod
    def _artist_portrait_identity(artist) -> str:
        return (
            str(artist["artist_key"])
            + "\0"
            + normalized_artist_identity(artist["artist_name"])
        )

    def _navidrome_thumbnail_path(self, artist, variant: str = "web-v1") -> Path | None:
        directory = self.cache / "navidrome-thumbnails" if self.cache is not None else None
        if directory is None:
            return None
        # Bind persistent provider pixels to both the durable row and visible
        # Artist identity. A catalog refresh may legitimately reuse a
        # transport key; an old Bette Midler cache entry must never become an
        # A. R. Rahman portrait merely because that key was reused.
        identity = sha256(
            self._artist_portrait_identity(artist).encode("utf-8")
        ).hexdigest()[:32]
        path = directory / "artists" / (variant + "-" + identity + ".png")
        return path if path.is_file() else None

    def _cache_navidrome_thumbnail(
        self,
        artist,
        credential,
        remote_artist,
        *,
        size: int = 192,
        variant: str = "web-v1",
    ) -> Path | None:
        cached = self._navidrome_thumbnail_path(artist, variant)
        if cached is not None:
            return cached
        cover_id = str(remote_artist.get("coverArt") or "").strip()
        directory = self.cache / "navidrome-thumbnails" if self.cache is not None else None
        if not cover_id or directory is None:
            return None
        base = str(credential.get("server_url") or "").rstrip("/")
        with self.thumbnail_download_slots:
            cached = self._navidrome_thumbnail_path(artist, variant)
            if cached is not None:
                return cached
            response = requests.get(
                base + "/rest/getCoverArt.view",
                params={**self._navidrome_auth_query(credential), "id": cover_id, "size": str(size)},
                timeout=20,
                allow_redirects=False,
            )
            response.raise_for_status()
            if len(response.content) > 8 * 1024 * 1024:
                raise ValueError("Navidrome artist thumbnail exceeds 8 MB")
            with Image.open(BytesIO(response.content)) as opened:
                opened.verify()
            directory = directory / "artists"
            directory.mkdir(parents=True, exist_ok=True)
            identity = sha256(
                self._artist_portrait_identity(artist).encode("utf-8")
            ).hexdigest()[:32]
            path = directory / (variant + "-" + identity + ".png")
            descriptor, temporary = tempfile.mkstemp(prefix=".navidrome-", suffix=".png", dir=directory)
            try:
                os.close(descriptor)
                with Image.open(BytesIO(response.content)) as opened:
                    opened.convert("RGBA").save(temporary, format="PNG", optimize=True)
                os.replace(temporary, path)
            finally:
                Path(temporary).unlink(missing_ok=True)
        self.portrait_cache.pop(str(artist["artist_key"]), None)
        return path

    def _cache_navidrome_similar_thumbnail(
        self, remote_artist: dict[str, object], credential: dict[str, object]
    ) -> Path | None:
        """Cache provider-authorized Similar Artist imagery behind a local URL."""
        directory = self.cache / "navidrome-thumbnails" / "similar" if self.cache is not None else None
        cover_id = str(remote_artist.get("coverArt") or "").strip()
        if directory is None or not cover_id:
            return None
        identity = sha256(
            (str(remote_artist.get("id") or "") + "\0" + str(remote_artist.get("name") or "") + "\0" + cover_id).encode("utf-8")
        ).hexdigest()[:32]
        path = directory / ("web-v1-" + identity + ".png")
        if path.is_file():
            return path
        base = str(credential.get("server_url") or "").rstrip("/")
        response = requests.get(
            base + "/rest/getCoverArt.view",
            params={**self._navidrome_auth_query(credential), "id": cover_id, "size": "192"},
            timeout=20,
            allow_redirects=False,
        )
        response.raise_for_status()
        if len(response.content) > 8 * 1024 * 1024:
            raise ValueError("Navidrome Similar Artist image is too large")
        with Image.open(BytesIO(response.content)) as opened:
            opened.verify()
        directory.mkdir(parents=True, exist_ok=True)
        descriptor, temporary = tempfile.mkstemp(prefix=".navidrome-", suffix=".png", dir=directory)
        try:
            os.close(descriptor)
            with Image.open(BytesIO(response.content)) as opened:
                opened.convert("RGBA").save(temporary, format="PNG", optimize=True)
            os.replace(temporary, path)
        finally:
            Path(temporary).unlink(missing_ok=True)
        return path

    def _navidrome_similar_thumbnail_path(
        self, remote_artist: dict[str, object]
    ) -> Path | None:
        """Return an already cached Similar Artist image without blocking metadata."""
        cache = getattr(self, "cache", None)
        directory = cache / "navidrome-thumbnails" / "similar" if cache is not None else None
        cover_id = str(remote_artist.get("coverArt") or "").strip()
        if directory is None or not cover_id:
            return None
        identity = sha256(
            (str(remote_artist.get("id") or "") + "\0" + str(remote_artist.get("name") or "") + "\0" + cover_id).encode("utf-8")
        ).hexdigest()[:32]
        path = directory / ("web-v1-" + identity + ".png")
        return path if path.is_file() else None

    @staticmethod
    def _navidrome_similar_thumbnail_url(remote_artist: dict[str, object]) -> str:
        """Return a lazy, authenticated local URL for a Similar Artist image.

        Artist metadata must remain usable before provider artwork finishes.
        The browser therefore requests each visible portrait independently;
        the endpoint below validates and caches it through the configured
        Navidrome connection without exposing provider credentials.
        """
        artist_id = str(remote_artist.get("id") or "").strip()
        name = str(remote_artist.get("name") or "").strip()
        cover_art = str(remote_artist.get("coverArt") or "").strip()
        if not artist_id or not name or not cover_art:
            return ""
        return (
            "/api/artist/similar-thumbnail?provider_id=" + quote(artist_id, safe="")
            + "&name=" + quote(name, safe="")
            + "&cover_art=" + quote(cover_art, safe="")
        )

    def similar_artist_thumbnail(
        self, provider_id: str, name: str, cover_art: str
    ) -> Path:
        """Resolve one provider-authorized Similar Artist portrait lazily."""
        provider_id = str(provider_id or "").strip()
        name = str(name or "").strip()
        cover_art = str(cover_art or "").strip()
        if (
            not provider_id or not name or not cover_art
            or len(provider_id) > 256 or len(name) > 512 or len(cover_art) > 512
            or any(value in cover_art for value in ("/", "\\", ".."))
        ):
            raise FileNotFoundError("Similar Artist artwork is unavailable")
        remote_artist = {"id": provider_id, "name": name, "coverArt": cover_art}
        cached = self._navidrome_similar_thumbnail_path(remote_artist)
        if cached is not None:
            return cached
        credential_path = self.credentials / "navidrome.json"
        if not credential_path.is_file():
            raise FileNotFoundError("Navidrome credentials are unavailable")
        credential = json.loads(credential_path.read_text(encoding="utf-8"))
        # Similar Artist portraits are independently lazy-loaded by the
        # browser. Share the same bounded provider-download allowance as the
        # main Artist thumbnails so opening an overview cannot create an
        # unbounded burst of Navidrome requests.
        with self.thumbnail_download_slots:
            cached = self._navidrome_similar_thumbnail_path(remote_artist)
            if cached is None:
                cached = self._cache_navidrome_similar_thumbnail(remote_artist, credential)
        if cached is None:
            raise FileNotFoundError("Similar Artist artwork is unavailable")
        return cached

    def _navidrome_artists_by_name(self) -> dict[str, dict[str, object]]:
        """Load Navidrome's artist directory with bounded, recoverable retries."""
        credential_path = self.credentials / "navidrome.json"
        if not credential_path.is_file():
            with self.lock:
                return dict(self.navidrome_artist_directory)
        now = time.monotonic()
        with self.lock:
            if self.navidrome_directory_loading or now < self.navidrome_directory_retry_at:
                return dict(self.navidrome_artist_directory)
            self.navidrome_directory_loading = True
        try:
            credential = json.loads(credential_path.read_text(encoding="utf-8"))
            envelope = self._navidrome_call(credential, "getArtists", {})
            indexes = envelope.get("artists", {}).get("index", [])
            if isinstance(indexes, dict):
                indexes = [indexes]
            directory: dict[str, dict[str, object]] = {}
            for index in indexes:
                values = index.get("artist", []) if isinstance(index, dict) else []
                values = [values] if isinstance(values, dict) else values
                for remote in values:
                    if not isinstance(remote, dict) or not str(remote.get("coverArt") or "").strip():
                        continue
                    directory.setdefault(normalized_artist_identity(remote.get("name", "")), remote)
        except (OSError, ValueError, KeyError, TypeError, requests.RequestException, json.JSONDecodeError):
            with self.lock:
                self.navidrome_directory_failures += 1
                delay = min(300.0, 5.0 * (2 ** min(self.navidrome_directory_failures - 1, 6)))
                self.navidrome_directory_retry_at = time.monotonic() + delay
                self.navidrome_directory_loading = False
                return dict(self.navidrome_artist_directory)
        with self.lock:
            self.navidrome_artist_directory = directory
            self.navidrome_directory_failures = 0
            self.navidrome_directory_retry_at = time.monotonic() + (300.0 if directory else 30.0)
            self.navidrome_directory_loading = False
            return dict(directory)

    def _stored_artist_appearances(self, db, artist_id: str) -> dict[str, object]:
        try:
            rows = db.execute(
                "SELECT appearances.album_key,appearances.featured,"
                "appearances.navidrome_album_id,appearances.song_paths_json,"
                "albums.path,albums.compilation,"
                "owners.artist_name AS album_artist_name "
                "FROM artist_album_appearances appearances "
                "LEFT JOIN albums ON albums.album_key=appearances.album_key "
                "LEFT JOIN artists owners ON owners.artist_key=albums.artist_key "
                "WHERE appearances.artist_key=?",
                (artist_id,),
            ).fetchall()
        except Exception:
            # SQLite fixtures intentionally do not carry the WebGUI-only table.
            if isinstance(self.database, Database) and db.execute(
                "SELECT 1 FROM artists WHERE artist_key=?", (artist_id,)
            ).fetchone() is not None:
                raise
            return {"album_keys": set(), "featured_album_keys": set(), "song_paths": {}}
        eligible = [row for row in rows if row["path"] is not None]
        return {
            "album_keys": {str(row["album_key"]) for row in eligible},
            "featured_album_keys": {
                str(row["album_key"])
                for row in eligible
                if bool(row["featured"])
                and (
                    row["album_artist_name"] is None
                    or not synthetic_album_artist(row["album_artist_name"])
                )
            },
            "song_paths": {
                str(row["album_key"]): json.loads(str(row["song_paths_json"] or "[]"))
                for row in eligible
            },
        }

    @staticmethod
    def _merge_artist_appearances(*contexts) -> dict[str, object]:
        album_keys: set[str] = set()
        featured: set[str] = set()
        song_paths: dict[str, set[str]] = {}
        genres: set[str] = set()
        roles: set[str] = set()
        result: dict[str, object] = {}
        for context in contexts:
            if not context:
                continue
            album_keys.update(str(value) for value in context.get("album_keys", set()))
            featured.update(str(value) for value in context.get("featured_album_keys", set()))
            genres.update(str(value) for value in context.get("genres", []))
            roles.update(str(value) for value in context.get("roles", []))
            for key, values in context.get("song_paths", {}).items():
                song_paths.setdefault(str(key), set()).update(str(value) for value in values)
            if context.get("navidrome_artist_id"):
                result["navidrome_artist_id"] = str(context["navidrome_artist_id"])
        result.update({
            "album_keys": album_keys,
            "featured_album_keys": featured,
            "song_paths": {key: sorted(values) for key, values in song_paths.items()},
            "genres": sorted(genres),
            "roles": sorted(roles),
        })
        return result

    def _indexed_artist_appearances(self, db, artist, aliases) -> dict[str, object]:
        """Use the list's published local credit edges for the Overview."""
        identities = {normalized_artist_identity(row["artist_name"]) for row in aliases}
        projected = self._projected_track_artists(db)
        featured: set[str] = set()
        paths: dict[str, set[str]] = {}
        for identity in identities:
            evidence = projected.get(identity, {})
            for key, track_credits in evidence.get("track_credits", {}).items():
                paths.setdefault(key, set()).update(track_credits)
                _, owner_name = evidence["album_owners"][key]
                if (normalized_artist_identity(owner_name) not in identities
                        and not synthetic_album_artist(owner_name)):
                    featured.add(key)
        return {
            "album_keys": set(paths),
            "featured_album_keys": featured,
            "song_paths": {key: sorted(values) for key, values in paths.items()},
        }

    def _artist_appearance_context(self, artist, aliases, db, *, refresh=False) -> dict[str, object]:
        canonical_id = str(artist["artist_key"])
        selection_id = str(artist.get("selection_id") or canonical_id)
        generation = self._artist_projection_generation(db)
        token = self._artist_projection_token(generation, db)
        with self.lock:
            cached_entry = self.appearance_cache.get(selection_id)
            cached = cached_entry[1] if cached_entry is not None and cached_entry[0] == token else None
        # A reused durable key can address more than one presentation when
        # local IDs conflict. The durable provider cache has no way to
        # distinguish those presentations, so use their selection-scoped data.
        stored = self._stored_artist_appearances(db, canonical_id) if selection_id == canonical_id else {}
        indexed = self._indexed_artist_appearances(db, artist, aliases)
        baseline = self._merge_artist_appearances(stored, indexed)
        if not refresh:
            return self._merge_artist_appearances(baseline, cached)
        credential_path = self.credentials / "navidrome.json"
        if not credential_path.is_file():
            return baseline
        try:
            credential = json.loads(credential_path.read_text(encoding="utf-8"))
            matches = []
            artist_catalog = self._navidrome_call(credential, "getArtists", {})
            indexes = artist_catalog.get("artists", {}).get("index", [])
            if isinstance(indexes, dict):
                indexes = [indexes]
            for index in indexes:
                values = index.get("artist", []) if isinstance(index, dict) else []
                matches.extend([values] if isinstance(values, dict) else values)
            if not matches:
                search = self._navidrome_call(
                    credential,
                    "search3",
                    {"query": str(artist["artist_name"]), "artistCount": "20", "albumCount": "0", "songCount": "0"},
                )
                values = search.get("searchResult3", {}).get("artist", [])
                matches = [values] if isinstance(values, dict) else values
            remote_artist = next(
                (item for item in matches if isinstance(item, dict)
                 and normalized_artist_identity(item.get("name", ""))
                 == normalized_artist_identity(artist["artist_name"])),
                None,
            )
            if remote_artist is None:
                LOGGER.info(
                    "Artist appearance catalog has no exact Navidrome identity: artist=%s",
                    str(artist["artist_name"]),
                )
                return baseline
            remote_id = str(remote_artist.get("id") or "")
            all_albums = db.execute(
                "SELECT albums.*,artists.artist_name FROM albums "
                "JOIN artists ON artists.artist_key=albums.artist_key"
            ).fetchall()
            by_relative_path = {}
            for row in all_albums:
                try:
                    relative = Path(str(row["path"])).resolve().relative_to(self.music.resolve())
                except (OSError, ValueError):
                    continue
                key = str(relative).replace("\\", "/").strip("/")
                by_relative_path[key] = row
            song_paths: dict[str, set[str]] = {}
            remote_album_ids: dict[str, str] = {}
            genres: set[str] = set()
            roles: set[str] = set()
            for page in range(20):
                envelope = self._navidrome_call(
                    credential,
                    "search3",
                    {"query": str(artist["artist_name"]), "artistCount": "0", "albumCount": "0",
                     "songCount": "500", "songOffset": str(page * 500)},
                )
                songs = envelope.get("searchResult3", {}).get("song", [])
                if isinstance(songs, dict):
                    songs = [songs]
                for song in songs:
                    if not isinstance(song, dict) or not self._navidrome_song_has_artist(
                        song, remote_id, str(artist["artist_name"])
                    ):
                        continue
                    raw_path = str(song.get("path") or "").replace("\\", "/").strip("/")
                    parent = raw_path.rsplit("/", 1)[0] if "/" in raw_path else ""
                    row = by_relative_path.get(parent)
                    if row is None and parent:
                        matches_by_path = [
                            candidate for path, candidate in by_relative_path.items()
                            if parent.endswith("/" + path) or path.endswith("/" + parent)
                        ]
                        row = matches_by_path[0] if len(matches_by_path) == 1 else None
                    if row is None:
                        title = normalized_album_identity(song.get("album", ""))
                        album_artist = normalized_artist_identity(song.get("albumArtist", ""))
                        year = str(song.get("year") or "").strip()
                        candidates = [
                            candidate for candidate in all_albums
                            if normalized_album_identity(candidate["album_name"]) == title
                            and (not album_artist or normalized_artist_identity(candidate["artist_name"]) == album_artist)
                            and (not year or not str(candidate["release_year"] or "").strip()
                                 or str(candidate["release_year"]).strip() == year)
                        ]
                        row = candidates[0] if len(candidates) == 1 else None
                    if row is None:
                        continue
                    key = str(row["album_key"])
                    song_paths.setdefault(key, set()).add(raw_path)
                    remote_album_ids[key] = str(song.get("albumId") or "")
                    genre = str(song.get("genre") or "").strip()
                    if genre:
                        genres.add(genre)
                    for item in song.get("genres") or []:
                        if isinstance(item, dict) and item.get("name"):
                            genres.add(str(item["name"]))
                    for item in song.get("artists") or []:
                        if not isinstance(item, dict):
                            continue
                        if str(item.get("id") or "").casefold() != remote_id.casefold() \
                                and normalized_artist_identity(item.get("name", "")) != normalized_artist_identity(artist["artist_name"]):
                            continue
                        for role in item.get("roles") or []:
                            value = str(role.get("name") if isinstance(role, dict) else role).strip()
                            if value:
                                roles.add(value)
                        if item.get("role"):
                            roles.add(str(item["role"]))
                if len(songs) < 500:
                    break
            alias_ids = {str(row["artist_key"]) for row in aliases}
            featured = {
                key for key in song_paths
                if next(
                    (
                        str(row["artist_key"]) not in alias_ids
                        and not synthetic_album_artist(row["artist_name"])
                        for row in all_albums
                        if str(row["album_key"]) == key
                    ),
                    False,
                )
            }
            remote_context = {
                "album_keys": set(song_paths),
                "featured_album_keys": featured,
                "song_paths": {key: sorted(paths) for key, paths in song_paths.items()},
                "genres": sorted(genres),
                "roles": sorted(roles),
                "navidrome_artist_id": remote_id,
            }
            context = self._merge_artist_appearances(baseline, remote_context)
            try:
                self._cache_navidrome_thumbnail(artist, credential, remote_artist)
            except (OSError, ValueError, requests.RequestException):
                pass
            if isinstance(self.database, Database) and (
                self._artist_projection_token(self._artist_projection_generation(db), db) != token
            ):
                return baseline
            if selection_id == canonical_id and isinstance(self.database, Database) and db.execute(
                "SELECT 1 FROM artists WHERE artist_key=?", (canonical_id,)
            ).fetchone() is not None:
                db.execute("DELETE FROM artist_album_appearances WHERE artist_key=?", (canonical_id,))
                for key in sorted(song_paths):
                    db.execute(
                        "INSERT INTO artist_album_appearances"
                        "(artist_key,album_key,navidrome_album_id,featured,song_paths_json) "
                        "VALUES(?,?,?,?,?) ON CONFLICT(artist_key,album_key) DO UPDATE SET "
                        "navidrome_album_id=excluded.navidrome_album_id,"
                        "featured=excluded.featured,song_paths_json=excluded.song_paths_json,"
                        "updated_at=now()",
                        (canonical_id, key, remote_album_ids.get(key, ""), int(key in featured),
                         json.dumps(remote_context["song_paths"][key])),
                    )
                db.commit()
            with self.lock:
                self.appearance_cache[selection_id] = (token, remote_context)
            LOGGER.info(
                "Artist appearance catalog ready: artist=%s albums=%d featured=%d comp_paths=%d",
                str(artist["artist_name"]),
                len(context["album_keys"]),
                len(context["featured_album_keys"]),
                sum(len(paths) for paths in context["song_paths"].values()),
            )
            return context
        except (OSError, ValueError, KeyError, TypeError, requests.RequestException, json.JSONDecodeError) as error:
            # Never log credential-bearing request URLs or exception text.
            LOGGER.warning(
                "Artist appearance catalog unavailable: artist=%s error=%s; using stored projection",
                str(artist["artist_name"]),
                type(error).__name__,
            )
            return baseline

    def refresh_artist_appearances(self, artist_id: str) -> dict[str, object]:
        with closing(self.connect()) as db:
            artist, aliases = self._artist_identity_rows(db, artist_id, require_eligible=True)
            return self._artist_appearance_context(artist, aliases, db, refresh=True)

    def upscale_preview(self, image_url: str) -> dict[str, object]:
        source = self.image_source(image_url)
        with Image.open(source if isinstance(source, Path) else BytesIO(source)) as opened:
            prepared = ImageOps.exif_transpose(opened).convert("RGB")
        width, height = prepared.size
        short_side = min(width, height)
        # Mirror the default Windows 1800-pixel ideal and 200% safety ceiling.
        requested = 1800 / short_side if short_side else 1
        scale = requested if 1 < requested <= 2 else 1
        if scale > 1:
            prepared = prepared.resize((round(width * scale), round(height * scale)), Image.Resampling.LANCZOS)
        output = BytesIO()
        prepared.save(output, format="PNG")
        return {"image": self.image(output.getvalue()), "width": prepared.width,
                "height": prepared.height, "bytes": output.tell(), "upscaled": scale > 1,
                "reason": "enlarged to the 1800-pixel ideal" if scale > 1 else
                "source already meets the ideal" if requested <= 1 else
                "required enlargement exceeds the 200% limit"}

    def artists(
        self, query: str, *, nonblocking_projection: bool = False
    ) -> list[dict[str, object]]:
        rows, _ = self.artist_list(query, nonblocking_projection=nonblocking_projection)
        return rows

    def artist_list(
        self, query: str, *, nonblocking_projection: bool = False
    ) -> tuple[list[dict[str, object]], int]:
        """Return the searched rows and unfiltered eligible total from one snapshot."""
        with closing(self._connect_artist_catalog()) as db:
            rows = db.execute(
                "SELECT artists.artist_key, artists.artist_name, artists.musicbrainz_artistid, "
                "artists.primary_path, COUNT(albums.album_key) AS album_count, artists.status "
                "FROM artists LEFT JOIN albums ON albums.artist_key=artists.artist_key "
                "AND (COALESCE(albums.compilation,0)=0 "
                "OR TRIM(COALESCE(albums.musicbrainz_albumid,''))<>'') "
                "GROUP BY artists.artist_key "
                "ORDER BY artists.artist_name COLLATE NOCASE ",
            ).fetchall()
            projected = self._projected_track_artists(
                db, allow_build=not nonblocking_projection
            )
            try:
                status_rows = self._artist_owner_inputs(db)
                eligible_status_rows = [
                    row for row in status_rows if self.artist_album_allowed(row)
                ]
                coordination = self._album_write_coordination(db, eligible_status_rows)
                projected_statuses = True
            except sqlite3.OperationalError:
                # Reduced read-only fixtures can predate Album status fields.
                status_rows = []
                eligible_status_rows = []
                coordination = {}
                projected_statuses = False
        rows = self._artist_owner_candidates(rows, status_rows)
        existing_identities = {
            normalized_artist_identity(row["artist_name"]) for row in rows
        }
        rows = [*rows, *(
            self._virtual_artist_row(value)
            for identity, value in projected.items()
            if identity not in existing_identities
        )]
        statuses_by_artist: dict[str, list] = {}
        album_evidence_by_key: dict[str, dict[str, object]] = {}
        owned_album_keys: dict[str, set[str]] = {}
        for row in eligible_status_rows:
            album_key = str(row["album_key"])
            effective_status = self._effective_album_status(
                row, coordination.get(album_key, {})
            )
            album_evidence = {
                "album_key": album_key,
                "status": effective_status,
                "conditions": self._artist_album_state_conditions(
                    row, coordination.get(album_key, {})
                ),
            }
            album_evidence_by_key[album_key] = album_evidence
            statuses_by_artist.setdefault(str(row["artist_key"]), []).append(
                album_evidence
            )
            owned_album_keys.setdefault(str(row["artist_key"]), set()).add(album_key)
        result = []
        artist_total = 0
        for selection_id, canonical, group in self._artist_selection_groups(rows, projected):
            if synthetic_album_artist(canonical["artist_name"]):
                continue
            appearance = self._artist_group_projection(projected, group)
            display_name = self._artist_display_name(
                group, projected, owned_album_keys
            )
            group_album_keys = {
                str(row["art004_album_key"])
                for row in group if "art004_album_key" in row
            }
            group_owned = {
                str(row["artist_key"]): owned_album_keys.get(str(row["artist_key"]), set()) & group_album_keys
                for row in group
            }
            album_keys = {
                key
                for row in group
                for key in group_owned[str(row["artist_key"])]
            } | set(appearance.get("album_keys", set()))
            qualifying_owner = any(group_owned.values())
            if projected_statuses and not qualifying_owner and not appearance.get("artist_album_keys"):
                continue
            effective_album_count = len(album_keys)
            if not effective_album_count and not projected_statuses:
                effective_album_count = sum(int(row["album_count"] or 0) for row in group)
            if effective_album_count == 0:
                continue
            artist_total += 1
            alias_names = {str(row["artist_name"] or "") for row in group}
            for row in group:
                evidence = projected.get(normalized_artist_identity(row["artist_name"]), {})
                alias_names.update(evidence.get("credit_labels", ()))
            if query and not any(artist_search_matches(query, alias) for alias in alias_names):
                continue
            status_group = [
                status
                for row in group
                for status in statuses_by_artist.get(str(row["artist_key"]), [])
                if status["album_key"] in group_owned[str(row["artist_key"])]
            ]
            if appearance:
                for album_key, status in appearance.get("statuses", {}).items():
                    # Featured and V/A/OST track credits share the indexed
                    # Album's static-cover and embedding evidence, not only
                    # its raw processing label.
                    indexed = album_evidence_by_key.get(album_key)
                    if indexed is not None:
                        status_group.append(indexed)
                    else:
                        status_group.append({
                            "status": status,
                            "conditions": self._artist_album_state_conditions(
                                {"status": status},
                                coordination.get(album_key, {}),
                            ),
                        })
                status_group.extend(
                    {
                        "status": status,
                        "conditions": (
                            {"complete_or_overridden"}
                            if status == "complete"
                            else {"incomplete_or_error"}
                        ),
                    }
                    for status in appearance.get("comp_statuses", {}).values()
                )
            # Keep explicit Red/error evidence distinct from a default-Red
            # name, so lazy missing Artist Art cannot hide a genuine error.
            has_incomplete_error = any(
                "incomplete_or_error" in item.get("conditions", ())
                for item in status_group
            )
            if not projected_statuses:
                has_incomplete_error = any(
                    str(row["status"] or "").casefold() in {"incomplete", "timeout", "error"}
                    for row in group
                )
            # The list projection must remain PostgreSQL-only. Give the
            # browser a stable authenticated URL without resolving the file
            # here; its visibility queue requests only the portraits close to
            # the viewport. Walking every Artist Art directory (and the
            # Navidrome fallback directory) made an otherwise complete Artist
            # list wait tens of seconds on large NAS libraries.
            portrait_url = self.artist_thumbnail_url(selection_id)
            result.append({
                "id": selection_id,
                "name": display_name,
                "albums": effective_album_count,
                "status": (
                    self._aggregate_album_status(status_group)
                    if projected_statuses
                    else self._aggregate_stored_artist_status(group)
                ),
                "portrait": portrait_url,
                "has_incomplete_error": has_incomplete_error,
                "projection_pending": False,
            })
        result.sort(
            key=lambda item: (
                artist_display_sort_key(item["name"]),
                normalized_artist_identity(item["name"]),
                str(item["id"]).casefold(),
            ),
        )
        return result, artist_total

    def artist_thumbnails(self) -> list[dict[str, object]]:
        """Resolve visible Artist Art without delaying the initial list projection.

        Every visible Artist is returned, including Artists without usable Art.
        That negative result lets the browser apply the shared yellow
        ``missing_artist_art`` policy only after the lazy filesystem/provider
        lookup completes, without restoring the former startup-wide NAS walk.
        """
        maintenance_active = self.index_maintenance_active()
        with closing(self._connect_artist_catalog()) as db:
            rows = db.execute(
                "SELECT artists.artist_key, artists.artist_name, artists.musicbrainz_artistid, "
                "artists.primary_path, COUNT(albums.album_key) AS album_count, artists.status "
                "FROM artists LEFT JOIN albums ON albums.artist_key=artists.artist_key "
                "GROUP BY artists.artist_key "
                "ORDER BY artists.artist_name COLLATE NOCASE"
            ).fetchall()
            projected = self._projected_track_artists(db)
            album_inputs = self._artist_owner_inputs(db)
            rows = self._artist_owner_candidates(rows, album_inputs)
            eligible_album_keys = {
                str(row["album_key"]) for row in album_inputs if self.artist_album_allowed(row)
            }
        existing_identities = {
            normalized_artist_identity(row["artist_name"]) for row in rows
        }
        rows = [*rows, *(
            self._virtual_artist_row(value)
            for identity, value in projected.items()
            if identity not in existing_identities
        )]
        remote_artists = {} if maintenance_active else None
        result = []
        for selection_id, canonical, group in self._artist_selection_groups(rows, projected):
            if synthetic_album_artist(canonical["artist_name"]):
                continue
            appearance = self._artist_group_projection(projected, group)
            has_owner_album = any(
                row.get("art004_album_key") in eligible_album_keys for row in group
            )
            visible = has_owner_album or bool(appearance.get("artist_album_keys"))
            if not visible:
                continue
            portrait_path = None
            for row in [canonical, *(item for item in group if item is not canonical)]:
                portrait_path = self.artist_thumbnail_path(row)
                if portrait_path is not None:
                    break
            portrait = self.image(portrait_path) if portrait_path is not None else ""
            if not portrait:
                if remote_artists is None:
                    remote_artists = self._navidrome_artists_by_name()
                if normalized_artist_identity(canonical["artist_name"]) in remote_artists:
                    portrait = self.artist_thumbnail_url(selection_id)
            result.append({
                "id": selection_id,
                "portrait": portrait,
                "missing_artist_art": not bool(portrait),
            })
        return result

    def artist_thumbnail_path(self, artist) -> Path | None:
        artist_id = str(artist["artist_key"])
        portrait_identity = self._artist_portrait_identity(artist)
        raw_id = str(artist["musicbrainz_artistid"] or "").strip()
        name_identity = (
            "name-"
            + sha256(str(artist["artist_name"]).strip().casefold().encode("utf-8"))
            .digest()[:16]
            .hex()
        )
        try:
            identity = "mbid-" + str(UUID(raw_id))
        except ValueError:
            identity = name_identity
        managed = ArtistArtTarget(self.artist_art, "artist-" + identity, True)
        # A projected/featured Artist is written to the stable name-scoped
        # shared package because it has no owned Album directory.  Its ordered
        # track credit can still supply a valid MBID, so thumbnail discovery
        # must also inspect that canonical name target before falling back to a
        # cached provider portrait.  Prefer an existing MBID package when both
        # exist so owned Artists keep their stronger identity authority.
        managed_name = ArtistArtTarget(
            self.artist_art, "artist-" + name_identity, True
        )
        primary_path = str(artist["primary_path"] or "").strip()
        direct_artist_path = Path(primary_path) if primary_path else None
        direct_identity = artist_directory_identity(
            direct_artist_path.name if direct_artist_path is not None else ""
        )
        direct_owned = bool(
            direct_artist_path is not None
            and direct_identity not in {
                "soundtrack", "soundtracks", "variousartist", "variousartists",
                "compilation", "compilations", "curatedcompilation",
                "curatedcompilations",
            }
        )
        # Never let a shared synthetic root's unqualified artist.* leak across
        # identities. In the production catalog Bette Midler, A. R. Rahman,
        # and an Electric Light Orchestra projection can all legitimately
        # publish ``[Soundtracks]`` as primary_path; their managed MBID/name
        # packages remain independent.
        targets = [ArtistArtTarget(direct_artist_path, "artist")] if direct_owned else []
        targets.append(managed)
        if managed_name != managed:
            targets.append(managed_name)
        portrait = saved_package(targets).portrait
        # Re-resolve local Artist Art before consulting the session cache. A
        # Windows write or external artist.* creation must replace an older
        # provider portrait without requiring a WebGUI restart.
        cached_entry = self.portrait_cache.get(artist_id)
        if cached_entry is not None and cached_entry[0] != portrait_identity:
            self.portrait_cache.pop(artist_id, None)
            cached_entry = None
        if portrait is None and cached_entry is not None:
            cached = cached_entry[1]
            portrait = cached if cached.is_file() else None
        if portrait is None:
            portrait = self._navidrome_thumbnail_path(artist)
        if portrait is not None and (
            not portrait.is_file() or not self.allowed_image_path(portrait)
        ):
            portrait = None
        # Cache only a real file.  A missing portrait is not stable: Artist
        # Art may be written by this WebGUI or the Windows application while
        # the server remains open.
        if portrait is not None:
            self.portrait_cache[artist_id] = (portrait_identity, portrait)
        else:
            self.portrait_cache.pop(artist_id, None)
        return portrait

    def _artist_art_targets_for_identity(
        self,
        artist,
        aliases,
        album_rows,
        *,
        allow_managed_without_owned_album: bool = False,
    ) -> list[ArtistArtTarget]:
        """Resolve one ordered Artist Art package for a presentation identity.

        PostgreSQL can retain a name-only owner row beside an MBID-backed row
        for the same visible Artist.  Reads previously used only the selected
        row while writes used the physical owner rows, so a verified write
        could land in ``artist-mbid-*`` while the overview kept reading the
        older name-scoped package.  Direct, dedicated Artist folders still win;
        managed MBID packages then outrank name fallbacks for every alias in
        the presentation group.
        """
        album_rows = list(album_rows)
        base = artist_targets(
            [
                ArtistAlbum(
                    str(artist["artist_name"]),
                    Path(row["path"]),
                    str(row.get("musicbrainz_albumid") or ""),
                    str(row.get("musicbrainz_artistid") or ""),
                )
                for row in album_rows
                if inside(Path(row["path"]), self.music)
            ],
            str(artist["artist_name"]),
            self.artist_art,
            allow_managed_without_owned_album=allow_managed_without_owned_album,
        )
        if not base:
            return []

        ordered: list[ArtistArtTarget] = []
        seen: set[tuple[str, str]] = set()

        def append(target: ArtistArtTarget) -> None:
            key = (str(target.directory).casefold(), target.stem.casefold())
            if key not in seen:
                seen.add(key)
                ordered.append(target)

        for target in base:
            if not target.managed:
                append(target)

        identity_rows = [artist, *(row for row in aliases if row is not artist)]
        raw_mbids = [
            str(row.get("musicbrainz_artistid") or "").strip()
            for row in [*identity_rows, *album_rows]
        ]
        for raw_mbid in raw_mbids:
            try:
                mbid = str(UUID(raw_mbid))
            except ValueError:
                continue
            append(ArtistArtTarget(self.artist_art, "artist-mbid-" + mbid, True))

        names = [str(row.get("artist_name") or "").strip() for row in identity_rows]
        for name in names:
            if not name:
                continue
            identity = sha256(name.casefold().encode("utf-8")).digest()[:16].hex()
            append(ArtistArtTarget(self.artist_art, "artist-name-" + identity, True))

        for target in base:
            append(target)
        return ordered

    def artist_thumbnail(self, artist_id: str) -> Path:
        """Resolve saved Artist Art without emitting speculative 404 requests."""
        with closing(self._connect_artist_catalog()) as db:
            album_inputs = self._artist_owner_inputs(db)
            artist, aliases = self._artist_identity_rows(
                db, artist_id, require_eligible=True, album_inputs=album_inputs,
            )
            primary_keys = {
                str(row["art004_album_key"])
                for row in aliases if "art004_album_key" in row
            }
            primary_rows = [
                row for row in album_inputs
                if str(row["album_key"]) in primary_keys and self.artist_album_allowed(row)
            ]
        targets = self._artist_art_targets_for_identity(
            artist, aliases, primary_rows,
            allow_managed_without_owned_album=True,
        )
        portrait = saved_package(targets).portrait
        if portrait is None:
            for candidate in [artist, *(row for row in aliases if row is not artist)]:
                portrait = self.artist_thumbnail_path(candidate)
                if portrait is not None:
                    break
        if portrait is None:
            if self.index_maintenance_active():
                raise KeyError("Artist thumbnail hydration is paused while indexing")
            now = time.monotonic()
            with self.lock:
                if now < self.portrait_retry_at.get(str(artist["artist_key"]), 0.0):
                    raise KeyError("Saved Artist Art is unavailable")
            remote = self._navidrome_artists_by_name().get(
                normalized_artist_identity(artist["artist_name"])
            )
            credential_path = self.credentials / "navidrome.json"
            if remote is not None and credential_path.is_file():
                artist_key = str(artist["artist_key"])
                try:
                    credential = json.loads(credential_path.read_text(encoding="utf-8"))
                    portrait = self._cache_navidrome_thumbnail(artist, credential, remote)
                except (OSError, ValueError, KeyError, TypeError, requests.RequestException, json.JSONDecodeError):
                    portrait = None
                with self.lock:
                    if portrait is not None:
                        self.portrait_cache[artist_key] = (
                            self._artist_portrait_identity(artist), portrait
                        )
                        self.portrait_retry_at.pop(artist_key, None)
                        self.portrait_failures.pop(artist_key, None)
                    else:
                        self.portrait_cache.pop(artist_key, None)
                        failures = self.portrait_failures.get(artist_key, 0) + 1
                        self.portrait_failures[artist_key] = failures
                        delay = min(300.0, 5.0 * (2 ** min(failures - 1, 6)))
                        self.portrait_retry_at[artist_key] = time.monotonic() + delay
        if portrait is None:
            raise KeyError("Saved Artist Art is unavailable")
        return portrait

    def index_maintenance_active(self) -> bool:
        try:
            with closing(self.connect()) as db:
                row = db.execute(
                    "SELECT catalog_state FROM catalog_index_state WHERE singleton=TRUE"
                ).fetchone()
            return row is not None and str(row["catalog_state"]) in {
                "initializing",
                "maintenance",
            }
        except Exception:
            return False

    def albums(self, query: str, view: str | bool = "va-ost") -> list[dict[str, object]]:
        requested_view = (
            "fallback" if view is True else "legacy-albums" if view is False else str(view)
        )
        with closing(self.connect()) as db:
            rows = db.execute(
                "SELECT albums.*, artists.artist_name FROM albums "
                "JOIN artists ON artists.artist_key=albums.artist_key "
                "WHERE (albums.album_name LIKE ? ESCAPE '\\' OR artists.artist_name LIKE ? ESCAPE '\\') "
                "ORDER BY albums.album_name COLLATE NOCASE ",
                ("%" + query.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_") + "%",) * 2,
            ).fetchall()
            coordination = self._album_write_coordination(db, rows)
        items = []
        for row in rows:
            group = self.album_library_group(row)
            if requested_view == "fallback" and group != "fallback":
                continue
            if requested_view == "legacy-albums" and not str(
                row["musicbrainz_albumid"] or ""
            ).strip():
                continue
            if requested_view in {"albums", "va-ost"} and group not in {
                "various-artists", "soundtracks", "misc-curated", "misc-undefined"
            }:
                continue
            album_key = str(row["album_key"])
            row_keys = set(row.keys())
            indexed_cover = bool(row["cover_found"]) if "cover_found" in row_keys else bool(row["cover_path"])
            cover_url = self.album_thumbnail_url(album_key) if indexed_cover else None
            if requested_view == "fallback":
                cover_url = self.image(self.album_cover_path(row))
            album_coordination = coordination.get(album_key, {})
            cover_path = str(row["cover_path"] or "")
            items.append({
                "id": row["album_key"], "name": row["album_name"],
                "artist_id": row["artist_key"], "artist_name": row["artist_name"],
                # Keep the full Albums index database-first.  Resolving every
                # cover on a network-mounted library makes the request appear
                # empty for minutes.  Native lazy-loading fetches only visible
                # thumbnails through the authenticated per-album endpoint.
                "cover": cover_url,
                "tracks": row["track_count"],
                "status": row["status"], "compilation": bool(row["compilation"]),
                "embedded": int(row["embedded_tracks"] or 0)
                    if "embedded_tracks" in row_keys else 0,
                "embedded_validation": str(row["embedded_validation_status"] or "unknown")
                    if "embedded_validation_status" in row_keys else "unknown",
                "cover_found": indexed_cover,
                "cover_path": cover_path,
                "static_cover_found": bool(
                    indexed_cover and Path(cover_path).suffix.casefold() != ".webp"
                ),
                "webp_found": bool(row["webp_found"])
                    if "webp_found" in row_keys else False,
                "list_status_key": self._album_list_status_key(row, album_coordination),
                "album_mbid": row["musicbrainz_albumid"] or "",
                "library_group": group,
                "category_override": self.category_overrides.get(album_key, ""),
                "library_category": {
                    "various-artists": "Various Artists",
                    "soundtracks": "Soundtracks",
                    "misc-curated": "Miscellaneous Curated",
                    "misc-undefined": "Miscellaneous Undefined",
                    "fallback": "Curated Compilations",
                }.get(group, "Artist"),
                "fallback": group == "fallback",
                "override_writer": album_coordination.get("writer", ""),
                "override_operation": album_coordination.get("operation", ""),
                "override_revision": int(album_coordination.get("revision", 0) or 0),
            })
        return items

    def album_thumbnail(self, album_id: str) -> Path:
        with closing(self.connect()) as db:
            row = db.execute(
                "SELECT album_key,path,cover_path FROM albums WHERE album_key=?",
                (album_id,),
            ).fetchone()
        if row is None:
            raise KeyError("Album not found")
        cover = self.album_cover_path(row)
        if cover is None:
            raise KeyError("Saved album artwork is unavailable")
        return cover

    def artist(self, artist_id: str, *, require_eligible: bool = False) -> dict[str, object]:
        with closing(self._connect_artist_catalog()) as db:
            album_inputs = self._artist_owner_inputs(db)
            artist, aliases = self._artist_identity_rows(
                db, artist_id, require_eligible=require_eligible, album_inputs=album_inputs,
            )
            primary_keys = {
                str(row["art004_album_key"]) for row in aliases if "art004_album_key" in row
            }
            primary_rows = [
                {**row, "album_artist": row["artist_name"]}
                for row in album_inputs
                if str(row["album_key"]) in primary_keys and self.artist_album_allowed(row)
            ]
            appearance = self._artist_appearance_context(artist, aliases, db)
            appearance_keys = set(appearance.get("album_keys", set()))
            appearance_rows = [
                {**row, "album_artist": row["artist_name"]}
                for row in album_inputs if str(row["album_key"]) in appearance_keys
            ]
            primary_album_keys = {str(row["album_key"]) for row in primary_rows}
            indexed_tracks = self._indexed_artist_appearances(db, artist, aliases)["song_paths"]
            credited_tracks = {key: set(paths) for key, paths in indexed_tracks.items()}
            track_album_keys = sorted(primary_album_keys | set(credited_tracks))
            top_track_rows = db.execute(
                "SELECT album_key,title,path FROM tracks WHERE album_key IN ("
                + ",".join("?" for _ in track_album_keys)
                + ") ORDER BY title COLLATE NOCASE",
                tuple(track_album_keys),
            ).fetchall() if track_album_keys else []
            top_tracks = []
            for item in top_track_rows:
                key = str(item["album_key"])
                path = str(item["path"] or "").replace("\\", "/").strip("/")
                associated = key in primary_album_keys or path in credited_tracks.get(key, ())
                if associated and str(item["title"]) not in top_tracks:
                    top_tracks.append(str(item["title"]))
                if len(top_tracks) == 10:
                    break
            coordination = self._album_write_coordination(
                db, [*primary_rows, *appearance_rows]
            )
        featured_keys = set(appearance.get("featured_album_keys", set()))
        primary_keys = {str(row["album_key"]) for row in primary_rows}
        rows = [*primary_rows, *(row for row in appearance_rows if str(row["album_key"]) not in primary_keys)]
        rows.sort(
            key=lambda row: (
                str(row["album_name"] or "").casefold(),
                str(row["album_key"] or "").casefold(),
            )
        )
        targets = self._artist_art_targets_for_identity(
            artist, aliases, primary_rows,
            allow_managed_without_owned_album=bool(appearance_rows),
        )
        package = saved_package(targets)
        portrait = package.portrait
        if portrait is None:
            for candidate in [artist, *(row for row in aliases if row is not artist)]:
                portrait = self.artist_thumbnail_path(candidate)
                if portrait is not None:
                    break
        candidates = []
        for label, path in local_candidates(targets):
            if inside(path, self.music) or inside(path, self.artist_art):
                candidates.append({"source": "Local", "kind": label.lower(), "acceptable": True,
                                   "image": self.image(path), **image_metadata(path)})
        albums = []
        for row in rows:
            folder = Path(row["path"])
            if not inside(folder, self.music):
                continue
            cover = self.album_cover_path(row)
            override = coordination.get(str(row["album_key"]), {})
            library_group = self.album_library_group(row, artist_catalog=True)
            albums.append({
                "id": row["album_key"], "name": row["album_name"],
                "tracks": row["track_count"], "status": row["status"],
                "compilation": bool(row["compilation"]),
                "featured": str(row["album_key"]) in featured_keys,
                "album_artist": row["album_artist"] or "",
                "various_artists": normalized_artist_identity(row["album_artist"] or "")
                    == "variousartists",
                "embedded": int(row["embedded_tracks"] or 0) if "embedded_tracks" in row.keys() else 0,
                "embedded_validation": str(row["embedded_validation_status"] or "unknown")
                    if "embedded_validation_status" in row else "unknown",
                "album_mbid": row["musicbrainz_albumid"] or "",
                "library_group": library_group,
                "category_override": row.get(
                    "art004_category_override", self.category_overrides.get(str(row["album_key"]), "")
                ) or "",
                "fallback": library_group == "fallback",
                "year": row["release_year"] or "",
                "soundtrack": bool(row["musicbrainz_albumid"]) and (
                    any(part.casefold() in {"[soundtracks]", "soundtracks"} for part in folder.parts)
                    or "soundtrack" in row["album_name"].casefold()
                    or "original motion picture" in row["album_name"].casefold()
                    or "music from the motion picture" in row["album_name"].casefold()
                    or row["album_name"].casefold().endswith(" ost")
                ),
                "webp_found": bool(row["webp_found"]),
                "cover_found": cover is not None,
                # A readable cover.webp remains previewable but cannot satisfy
                # ordinary static cover.* completeness.
                "static_cover_found": cover is not None and cover.suffix.casefold() != ".webp",
                "cover": self.image(cover),
                "override_writer": override.get("writer", ""),
                "override_operation": override.get("operation", ""),
                "override_revision": int(override.get("revision", 0) or 0),
            })
        albums.sort(
            key=lambda album: (
                str(album["name"] or "").casefold(),
                str(album["id"] or "").casefold(),
            )
        )
        return {
            "id": artist["selection_id"], "name": artist["artist_name"],
            "mbid": artist["musicbrainz_artistid"] or "",
            "status": self._aggregate_album_status([
                {
                    "status": self._effective_album_status(
                        row, coordination.get(str(row["album_key"]), {})
                    ),
                    "conditions": self._artist_album_state_conditions(
                        row, coordination.get(str(row["album_key"]), {})
                    ),
                }
                for row in rows
            ]), "albums": albums,
            "top_tracks": top_tracks,
            "genres": list(appearance.get("genres", [])),
            "roles": list(appearance.get("roles", [])),
            "portrait": self.image(portrait),
            "banner": self.image(package.banner),
            "portrait_meta": image_metadata(portrait) if portrait and portrait.is_file() else None,
            "banner_meta": image_metadata(package.banner) if package.banner and package.banner.is_file() else None,
            "local_candidates": candidates,
        }

    def album(self, album_id: str) -> dict[str, object]:
        with closing(self.connect()) as db:
            row = db.execute(
                "SELECT albums.*, artists.artist_name FROM albums "
                "JOIN artists ON artists.artist_key=albums.artist_key "
                "WHERE albums.album_key=?", (album_id,),
            ).fetchone()
            track_rows = db.execute(
                "SELECT title,path FROM tracks WHERE album_key=? ORDER BY path", (album_id,),
            ).fetchall()
            track_titles = [item["title"] for item in track_rows[:12]]
        if row is None:
            raise KeyError("Album not found")
        with closing(self.connect()) as db:
            override = self._album_write_coordination(db, [row]).get(album_id, {})
        folder = Path(row["path"])
        if not inside(folder, self.music):
            raise KeyError("Album is outside the configured library")
        candidates = []
        for file in sorted(folder.iterdir(), key=lambda path: path.name.casefold()):
            if file.is_file() and not file.is_symlink() and file.suffix.casefold() in IMAGE_EXTENSIONS:
                try:
                    candidates.append({"source": "Album folder", "kind": "cover",
                                       "image": self.image(file), **image_metadata(file)})
                except (OSError, ValueError):
                    pass
        cover = self.album_cover_path(row)
        static_cover = (
            cover if cover is not None and cover.suffix.casefold() != ".webp" else None
        )
        embedded_paths = [
            Path(item["path"])
            for item in track_rows
            if inside(Path(item["path"]), folder)
        ]
        library_group = self.album_library_group(row)
        return {
            "id": album_id, "name": row["album_name"], "status": row["status"],
            "artist_id": row["artist_key"], "artist_name": row["artist_name"],
            "tracks": row["track_count"], "compilation": bool(row["compilation"]),
            "album_mbid": row["musicbrainz_albumid"] or "", "year": row["release_year"] or "",
            "library_group": library_group,
            "category_override": self.category_overrides.get(album_id, ""),
            "fallback": library_group == "fallback",
            "embedded": int(row["embedded_tracks"] or 0) if "embedded_tracks" in row.keys() else 0,
            "embedded_validation": str(row["embedded_validation_status"] or "unknown")
                if "embedded_validation_status" in row else "unknown",
            "cover_found": cover is not None,
            "static_cover_found": cover is not None and cover.suffix.casefold() != ".webp",
            "webp_found": bool(row["webp_found"]),
            "cover": self.image(cover),
            # V/A OST Album Overview backgrounds are intentionally static.
            # Keep cover.webp available to the animation workflow without
            # presenting it as this Album's requested cover.* background.
            "static_cover": self.image(static_cover),
            "cover_meta": image_metadata(cover) if cover else None,
            "embedded_meta": embedded_album_artwork_summary(
                embedded_paths, self.comp_thumbnail_side
            ),
            "candidates": candidates, "track_titles": track_titles,
            "override_writer": override.get("writer", ""),
            "override_operation": override.get("operation", ""),
            "override_revision": int(override.get("revision", 0) or 0),
        }

    def fallback_tracks(self, album_id: str) -> list[dict[str, object]]:
        with closing(self.connect()) as db:
            album = db.execute(
                "SELECT albums.album_key,albums.path,albums.compilation,albums.musicbrainz_albumid,"
                "artists.artist_name FROM albums JOIN artists ON "
                "artists.artist_key=albums.artist_key WHERE albums.album_key=?",
                (album_id,),
            ).fetchone()
            if album is None:
                raise KeyError("Album not found")
            if self.album_library_group(album) != "fallback":
                raise ValueError("This Album is not inside the Curated Compilations path")
            folder = Path(album["path"])
            if not inside(folder, self.music):
                raise ValueError("Fallback Album is outside the configured music library")
            has_ledger = db.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name='compilation_track_artwork'").fetchone() is not None
            rows = db.execute(
                "SELECT track_key, path, title, artist_name FROM tracks WHERE album_key=? ORDER BY path COLLATE NOCASE",
                (album_id,),
            ).fetchall()
            completed = set()
            if has_ledger:
                completed = {str(row[0]).casefold() for row in db.execute(
                    "SELECT track_path FROM compilation_track_artwork WHERE outcome IN ('embedded-replaced','embedded-existing')",
                )}
        tracks = [
            {
                "key": row["track_key"], "path": row["path"],
                "coordination_path": relative_coordination_path(Path(row["path"]), self.music),
                "title": row["title"], "artist": row["artist_name"],
                "file": Path(row["path"]).name,
                "completed": str(row["path"]).casefold() in completed,
                "status": (
                    "checking"
                    if str(row["path"]).casefold() in completed
                    else "blue"
                ),
                "thumbnail": None,
            }
            for row in rows if inside(Path(row["path"]), folder)
        ]
        if not tracks:
            # Windows fills an older uncached fallback from this folder only.
            tracks = [
                {
                    "key": str(path), "path": str(path), "title": path.stem,
                    "coordination_path": relative_coordination_path(path, self.music),
                    "artist": "", "file": path.name,
                    "completed": str(path).casefold() in completed,
                    "status": (
                        "checking" if str(path).casefold() in completed else "blue"
                    ),
                    "thumbnail": None,
                }
                for path in sorted(
                    folder.iterdir(), key=lambda item: item.name.casefold()
                )
                if path.is_file()
                and not path.is_symlink()
                and path.suffix.casefold() in AUDIO_EXTENSIONS
            ]
        return tracks

    def fallback_track_preview(self, album_id: str, track_key: str) -> dict[str, object]:
        track = next((item for item in self.fallback_tracks(album_id) if item["key"] == track_key), None)
        if track is None:
            raise KeyError("COMP track not found")
        artwork = embedded_track_image(Path(track["path"]))
        metadata: dict[str, object] = {"image": None, "width": 0, "height": 0, "bytes": 0}
        if artwork:
            with Image.open(BytesIO(artwork)) as opened:
                metadata = {"image": self.image(artwork), "width": opened.width,
                            "height": opened.height, "bytes": len(artwork)}
        return {**track, **metadata}

    def fallback_thumbnail(self, album_id: str, track_key: str) -> bytes:
        """Read one recorded embedded thumbnail for a lazy fallback album card."""
        with closing(self.connect()) as db:
            row = db.execute(
                "SELECT albums.album_key,albums.path AS album_path,albums.path,albums.compilation,"
                "albums.musicbrainz_albumid,artists.artist_name,tracks.path AS track_path "
                "FROM albums JOIN tracks ON tracks.album_key=albums.album_key "
                "JOIN artists ON artists.artist_key=albums.artist_key "
                "WHERE albums.album_key=? AND tracks.track_key=?",
                (album_id, track_key),
            ).fetchone()
            if row is None or self.album_library_group(row) != "fallback":
                raise KeyError("Recorded COMP thumbnail not found")
            recorded = db.execute(
                "SELECT 1 FROM compilation_track_artwork WHERE track_path=? "
                "AND outcome IN ('embedded-replaced','embedded-existing')",
                (str(row["track_path"]),),
            ).fetchone()
        folder, path = Path(row["album_path"]), Path(row["track_path"])
        if not recorded or not inside(folder, self.music) or not inside(path, folder):
            raise KeyError("Recorded COMP thumbnail not found")
        artwork = embedded_track_image(path)
        if not artwork:
            raise KeyError("Recorded COMP thumbnail is unavailable")
        return artwork

    def artist_comp_albums(self, artist_id: str) -> list[dict[str, object]]:
        """Return indexed COMP cards without touching every track on disk."""
        album_ids = self.artist_comp_album_ids(artist_id)
        return self._artist_comp_groups(
            artist_id, verify_tracks=False, album_ids=album_ids
        )

    def artist_comp_album_ids(self, artist_id: str) -> list[str]:
        """Return only fallback albums that can belong to the selected Artist.

        Owned albums are included before their tracks are indexed. Albums owned
        by another catalog identity require an indexed exact credit or an
        already-projected appearance, so opening one Artist never scans every
        fallback directory in the library.
        """
        with closing(self.connect()) as db:
            artist, aliases = self._artist_identity_rows(db, artist_id)
            name = str(artist["artist_name"]).strip()
            alias_ids = [str(row["artist_key"]) for row in aliases]
            placeholders = ",".join("?" for _ in alias_ids)
            appearance = self._artist_appearance_context(artist, aliases, db)
            appearance_ids = sorted(set(appearance.get("album_keys", set())))
            appearance_clause = ""
            if appearance_ids:
                appearance_clause = (
                    "OR albums.album_key IN (" + ",".join("?" for _ in appearance_ids) + ") "
                )
            # The SQL predicate is only a bounded candidate lookup.  Include
            # the shared-policy identity beside the display spelling so a
            # selected Artist such as ``*NSYNC`` can retrieve an indexed
            # ``NSYNC`` credit.  Exact membership is still decided below by
            # the shared COMP policy, so this cannot turn a substring hit into
            # an Artist appearance.
            search_names = list(dict.fromkeys(
                value for value in (name, normalize_artist_name(name)) if value
            ))
            escaped_names = [
                "%"
                + value.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_")
                + "%"
                for value in search_names
            ]
            credit_clause = " OR ".join(
                "tracks.artist_name LIKE ? ESCAPE '\\' "
                "OR tracks.title LIKE ? ESCAPE '\\' "
                "OR tracks.path LIKE ? ESCAPE '\\'"
                for _ in escaped_names
            )
            credit_parameters = tuple(
                parameter
                for escaped in escaped_names
                for parameter in (escaped, escaped, escaped)
            )
            rows = db.execute(
                "SELECT albums.album_key,albums.artist_key,albums.path,"
                "albums.musicbrainz_albumid,tracks.artist_name,tracks.title,"
                "tracks.path AS track_path "
                "FROM albums LEFT JOIN tracks ON tracks.album_key=albums.album_key "
                f"WHERE (albums.artist_key IN ({placeholders}) "
                f"OR {credit_clause} "
                f"{appearance_clause})",
                (*alias_ids, *credit_parameters, *appearance_ids),
            ).fetchall()
        matching = set()
        for row in rows:
            if self.album_library_group(row) != "fallback":
                continue
            if (
                str(row["album_key"]) in appearance_ids
                or str(row["artist_key"]) in alias_ids
                or any(
                    artist_names_compatible(credit, name)
                    for credit in indexed_track_credit_values(
                        str(row["artist_name"] or ""),
                        str(row["title"] or ""),
                        str(row["track_path"] or ""),
                        self.artist_separator_regex,
                    )
                )
            ):
                matching.add(str(row["album_key"]))
        return sorted(matching)

    def artist_comp_report(
        self, artist_id: str, album_ids: list[str] | None = None
    ) -> list[dict[str, object]]:
        """Project a focused artist's verified tracks for an opened COMP report."""
        selected = (
            self.artist_comp_album_ids(artist_id)
            if album_ids is None
            else list(dict.fromkeys(str(value) for value in album_ids if str(value)))
        )
        return self._artist_comp_groups(
            artist_id, verify_tracks=True, album_ids=selected
        )

    def _artist_comp_groups(
        self,
        artist_id: str,
        *,
        verify_tracks: bool,
        album_ids: list[str],
    ) -> list[dict[str, object]]:
        if not album_ids:
            return []
        with closing(self.connect()) as db:
            artist, aliases = self._artist_identity_rows(db, artist_id)
            name = str(artist["artist_name"]).strip()
            alias_ids = [str(row["artist_key"]) for row in aliases]
            appearance = self._artist_appearance_context(artist, aliases, db)
            album_placeholders = ",".join("?" for _ in album_ids)
            rows = db.execute(
                "SELECT albums.album_key, albums.album_name, albums.path AS album_path, "
                "albums.path, albums.musicbrainz_albumid, albums.cover_path, "
                "albums.release_year, albums.artist_key, tracks.track_key, "
                "tracks.path AS track_path, "
                "tracks.title, tracks.artist_name FROM albums "
                "JOIN tracks ON tracks.album_key=albums.album_key "
                f"WHERE albums.album_key IN ({album_placeholders}) "
                "ORDER BY albums.album_name COLLATE NOCASE, tracks.path COLLATE NOCASE",
                tuple(album_ids),
            ).fetchall()
            has_ledger = db.execute(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='compilation_track_artwork'"
            ).fetchone() is not None
            completed = {str(row[0]).casefold() for row in db.execute(
                "SELECT track_path FROM compilation_track_artwork "
                "WHERE outcome IN ('embedded-replaced','embedded-existing')",
            )} if has_ledger else set()
        groups: dict[str, dict[str, object]] = {}
        seen: set[str] = set()
        appearance_paths = appearance.get("song_paths", {})
        for row in rows:
            if self.album_library_group(row) != "fallback":
                continue
            credits = indexed_track_credit_values(
                str(row["artist_name"] or ""),
                str(row["title"] or ""),
                str(row["track_path"] or ""),
                self.artist_separator_regex,
            )
            row_path = str(row["track_path"]).replace("\\", "/").strip("/")
            expected_paths = [
                str(value).replace("\\", "/").strip("/")
                for value in appearance_paths.get(str(row["album_key"]), [])
            ]
            appearance_match = any(
                row_path == expected or row_path.endswith("/" + expected)
                or expected.endswith("/" + row_path)
                for expected in expected_paths if expected
            )
            if not (any(
                    artist_names_compatible(credit, name)
                    for credit in credits
                )
                    or not credits and row["artist_key"] in alias_ids
                    or appearance_match):
                continue
            folder, track_path = Path(row["album_path"]), Path(row["track_path"])
            if verify_tracks and (
                not inside(folder, self.music)
                or not inside(track_path, folder)
                or not track_path.is_file()
            ):
                continue
            path_key = str(track_path)
            if path_key in seen:
                continue
            seen.add(path_key)
            key = str(row["album_key"])
            if key not in groups:
                groups[key] = {
                    "id": key, "name": row["album_name"], "year": row["release_year"] or "",
                    # Selected Artist COMP cards lazily load the folder's
                    # cover.* through the standard Album thumbnail endpoint.
                    # Embedded track art remains exclusive to COMP Report.
                    "cover": self.album_thumbnail_url(key),
                    "compilation": True,
                    "album_mbid": row["musicbrainz_albumid"] or "",
                    "tracks": [],
                }
            completed_track = str(track_path).casefold() in completed
            groups[key]["tracks"].append({
                "key": row["track_key"], "file": track_path.name, "title": row["title"],
                "coordination_path": relative_coordination_path(track_path, self.music),
                "completed": completed_track,
                "status": "checking" if completed_track else "blue",
                "thumbnail": None,
            })
        return list(groups.values())

    def comp_track_status(self, album_id: str, track_key: str) -> dict[str, object]:
        """Check one COMP thumbnail lazily so a large Artist report opens quickly."""
        with closing(self.connect()) as db:
            row = db.execute(
                "SELECT albums.album_key,albums.path AS album_path,albums.path,"
                "albums.musicbrainz_albumid,tracks.path AS track_path "
                "FROM albums JOIN tracks ON tracks.album_key=albums.album_key "
                "WHERE albums.album_key=? AND tracks.track_key=?", (album_id, track_key),
            ).fetchone()
            if row is None or self.album_library_group(row) != "fallback":
                raise KeyError("COMP track not found")
            path, folder = Path(row["track_path"]), Path(row["album_path"])
            if not inside(folder, self.music) or not inside(path, folder) or not path.is_file():
                raise ValueError("COMP track is outside the configured music library")
            has_ledger = db.execute(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='compilation_track_artwork'"
            ).fetchone() is not None
            recorded = has_ledger and db.execute(
                "SELECT 1 FROM compilation_track_artwork WHERE track_path=? "
                "AND outcome IN ('embedded-replaced','embedded-existing')", (str(path),),
            ).fetchone() is not None
        if not recorded:
            return {"status": "blue", "repairable": False, "width": 0, "height": 0,
                    "side": self.comp_thumbnail_side}
        artwork = embedded_track_artwork_bytes(path)
        if not artwork:
            return {"status": "blue", "repairable": False, "width": 0, "height": 0,
                    "side": self.comp_thumbnail_side}
        try:
            with Image.open(BytesIO(artwork)) as opened:
                width, height = opened.size
        except (OSError, ValueError):
            return {"status": "blue", "repairable": False, "width": 0, "height": 0,
                    "side": self.comp_thumbnail_side}
        return {"status": "green" if width == height == self.comp_thumbnail_side else "purple",
                "repairable": width != self.comp_thumbnail_side or height != self.comp_thumbnail_side,
                "width": width, "height": height, "side": self.comp_thumbnail_side,
                "thumbnail": self.fallback_thumbnail_url(album_id, track_key)}

    def repair_comp_track(
        self,
        album_id: str,
        track_key: str,
        artist_id: str = "",
    ) -> dict[str, object]:
        """Repair one recorded, readable purple thumbnail in the isolated music copy."""
        if not self.allow_comp_repairs:
            raise ValueError("COMP repair is disabled for this browser service")
        if artist_id:
            allowed = any(
                group["id"] == album_id
                and any(track["key"] == track_key for track in group["tracks"])
                for group in self.artist_comp_report(artist_id)
            )
        else:
            allowed = any(
                track["key"] == track_key for track in self.fallback_tracks(album_id)
            )
        if not allowed:
            raise ValueError(
                "Track is not in this Artist's COMP report"
                if artist_id
                else "Track is not in this full Fallback COMP report"
            )
        current = self.comp_track_status(album_id, track_key)
        if current["status"] != "purple" or not current["repairable"]:
            raise ValueError("Only readable purple COMP thumbnails can be repaired")
        with closing(self.connect()) as db:
            row = db.execute("SELECT path FROM tracks WHERE album_key=? AND track_key=?",
                             (album_id, track_key)).fetchone()
        if row is None:
            raise KeyError("COMP track not found")
        path = Path(row["path"])
        if not inside(path, self.music):
            raise ValueError("COMP track is outside the configured music library")
        artwork = embedded_track_artwork_bytes(path)
        if not artwork:
            raise ValueError("The completed COMP track has no readable embedded art")
        with Image.open(BytesIO(artwork)) as opened:
            image_format = "png" if opened.format == "PNG" else "jpeg"
            thumbnail = imaging.prepare_embedded_thumbnail(
                opened, self.comp_thumbnail_side
            )
            output = BytesIO()
            thumbnail.save(output, format=image_format.upper(), **({"quality": 92, "optimize": True} if image_format == "jpeg" else {"optimize": True}))
        state_directory = self.cache or (self.database.parent if isinstance(self.database, Path) else self.artist_art.parent)
        backup_dir = state_directory / "comp-repair-backups"
        backup_dir.mkdir(parents=True, exist_ok=True)
        backup_id = sha256(str(path).casefold().encode("utf-8")).hexdigest()[:20]
        backup = backup_dir / f"{backup_id}.original-image"
        if not backup.exists():
            backup.write_bytes(artwork)
        replace_embedded_artwork(path, output.getvalue(), image_format,
                                 {"width": self.comp_thumbnail_side, "height": self.comp_thumbnail_side})
        verified = self.comp_track_status(album_id, track_key)
        if verified["status"] != "green":
            raise ValueError("Repaired COMP thumbnail did not verify at the requested size")
        log = state_directory / "comp-repair.log"
        with log.open("a", encoding="utf-8") as stream:
            stream.write(json.dumps({"artist_id": artist_id, "album_id": album_id,
                                     "track_key": track_key, "path": str(path),
                                     "original_width": current["width"],
                                     "original_height": current["height"],
                                     "repaired_side": self.comp_thumbnail_side,
                                     "original_image_backup": str(backup)}) + "\n")
        return {"status": "green", "width": self.comp_thumbnail_side,
                "height": self.comp_thumbnail_side, "backup": str(backup)}

    def repair_artist_comp_track(
        self,
        artist_id: str,
        album_id: str,
        track_key: str,
    ) -> dict[str, object]:
        return self.repair_comp_track(album_id, track_key, artist_id)

    def _stored_artist_profile(self, artist_id: str) -> dict[str, str] | None:
        if not isinstance(self.database, Database):
            return None
        with closing(self.database.connect()) as db:
            row = db.execute(
                "SELECT lookup_name,musicbrainz_artistid,biography,external_url,source "
                "FROM artist_profiles WHERE artist_key=?",
                (artist_id,),
            ).fetchone()
        return dict(row) if row is not None else None

    def _save_artist_profile(self, artist_id: str, profile: dict[str, str]) -> None:
        if not isinstance(self.database, Database):
            return
        with closing(self.database.connect()) as db, db:
            if db.execute(
                "SELECT 1 FROM artists WHERE artist_key=?", (artist_id,)
            ).fetchone() is None:
                # Appearance-only Artists remain a read-only projection over
                # another owner's Album. Their provider metadata stays in the
                # live cache without weakening Windows catalog authority.
                return
            db.execute(
                "INSERT INTO artist_profiles(artist_key,lookup_name,musicbrainz_artistid,biography,external_url,source) "
                "VALUES(?,?,?,?,?,?) ON CONFLICT(artist_key) DO UPDATE SET "
                "lookup_name=excluded.lookup_name,musicbrainz_artistid=excluded.musicbrainz_artistid,"
                "biography=excluded.biography,external_url=excluded.external_url,source=excluded.source,updated_at=now()",
                (
                    artist_id,
                    profile.get("lookup_name", ""),
                    profile.get("musicbrainz_id", ""),
                    profile.get("biography", ""),
                    profile.get("external_url", ""),
                    profile.get("source", ""),
                ),
            )

    def artist_metadata_seed(self, artist_id: str) -> dict[str, object] | None:
        """Return immediately displayable cached biography data for Artist Overview."""
        with self.lock:
            cached = self.metadata_cache.get(artist_id)
        if cached is not None:
            return {**cached, "pending": False}
        stored = self._stored_artist_profile(artist_id)
        if not stored:
            return None
        return {
            **stored,
            "similar": [],
            "top_songs": [],
            "similar_reason": "Loading genuine Similar Artists from the configured provider",
            "pending": True,
        }

    def _artist_metadata_lock(self, artist_id: str) -> Lock:
        """Return the selected Artist's single-flight metadata lock.

        Artist selection, a post-write refresh, and a delayed browser request
        can otherwise enter the provider-backed metadata path together.  The
        appearance projection replaces one Artist's rows transactionally, so
        duplicate refreshes are both wasteful and capable of racing the table's
        primary key.
        """
        with self.lock:
            locks = getattr(self, "metadata_locks", None)
            if locks is None:
                locks = {}
                self.metadata_locks = locks
            return locks.setdefault(artist_id, Lock())

    def artist_metadata(self, artist_id: str, *, refresh: bool = False) -> dict[str, object]:
        if refresh:
            with self.lock:
                self.metadata_cache.pop(artist_id, None)
                self.appearance_cache.pop(artist_id, None)
        # Recheck the cache after acquiring the per-Artist lock. A concurrent
        # request may have completed the same expensive provider refresh while
        # this caller was waiting.
        with self._artist_metadata_lock(artist_id):
            with self.lock:
                cached = self.metadata_cache.get(artist_id)
            if cached is not None:
                return {
                    **cached,
                    "artist": self.artist(artist_id, require_eligible=True),
                }
            return self._load_artist_metadata(artist_id)

    def _load_artist_metadata(self, artist_id: str) -> dict[str, object]:
        artist = self.artist(artist_id, require_eligible=True)
        stored_profile = self._stored_artist_profile(artist_id)
        result: dict[str, object] = {"source": "unavailable", "biography": "", "similar": [], "top_songs": []}
        credential_path = self.credentials / "navidrome.json"
        if not credential_path.is_file():
            result["reason"] = "Navidrome credentials are not present in this test bed"
        else:
            try:
                credential = json.loads(credential_path.read_text(encoding="utf-8"))
                base = str(credential.get("server_url") or "").rstrip("/")
                if urlsplit(base).scheme not in {"http", "https"}:
                    raise ValueError("Invalid Navidrome URL")

                def call(method: str, parameters: dict[str, str]) -> dict[str, object]:
                    query = {"v": "1.16.1", "c": "SPLINED-web-preview", "f": "json", **parameters}
                    if credential.get("auth_method") == "api_key" and credential.get("api_key"):
                        query["apiKey"] = credential["api_key"]
                    else:
                        salt = secrets.token_hex(6)
                        query.update({"u": credential["username"], "s": salt,
                                      "t": hashlib.md5((credential["password"] + salt).encode(), usedforsecurity=False).hexdigest()})
                    response = requests.get(base + "/rest/" + method + ".view", params=query,
                                            timeout=10, allow_redirects=False)
                    if response.status_code != 200:
                        raise ValueError("Navidrome did not return artist metadata")
                    envelope = response.json().get("subsonic-response", {})
                    if envelope.get("status") != "ok":
                        raise ValueError("Navidrome did not return artist metadata")
                    return envelope

                search = call("search3", {"query": str(artist["name"]), "artistCount": "20", "albumCount": "0", "songCount": "0"})
                matches = search.get("searchResult3", {}).get("artist", [])
                if isinstance(matches, dict):
                    matches = [matches]
                match = next(
                    (
                        item for item in matches
                        if normalized_artist_identity(item.get("name", ""))
                        == normalized_artist_identity(artist["name"])
                    ),
                    None,
                )
                if match:
                    info = call("getArtistInfo2", {"id": str(match["id"])}).get("artistInfo2", {})
                    biography = re.sub(r"<[^>]+>", " ", str(info.get("biography") or ""))
                    result["biography"] = " ".join(unescape(biography).split())
                    # Navidrome/OpenSubsonic exposes the selected Artist's
                    # indexed genres with getArtist. Fetching this bounded
                    # object avoids the old twenty-page song traversal merely
                    # to populate the overview heading.
                    try:
                        artist_detail = call("getArtist", {"id": str(match["id"])}).get("artist", {})
                    except (OSError, ValueError, KeyError, TypeError, requests.RequestException):
                        artist_detail = {}
                    # OpenSubsonic ArtistWithAlbumsID3 commonly publishes
                    # genres on its nested AlbumID3 rows rather than on the
                    # Artist object. Read both shapes from the same bounded
                    # request so the Overview does not need a song-page walk.
                    result["genres"] = provider_artist_genres(artist_detail)
                    similar = info.get("similarArtist", [])
                    if isinstance(similar, dict):
                        similar = [similar]
                    local_rows = {}
                    with closing(self.connect()) as db:
                        for row in db.execute(
                            "SELECT artist_key,artist_name,musicbrainz_artistid,primary_path FROM artists"
                        ):
                            local_rows.setdefault(normalized_artist_identity(row["artist_name"]), row)
                    projected = []
                    for item in similar:
                        if not isinstance(item, dict) or not str(item.get("name") or "").strip():
                            continue
                        name = str(item["name"]).strip()
                        local = local_rows.get(normalized_artist_identity(name))
                        portrait = self.artist_thumbnail_path(local) if local is not None else None
                        if portrait is None:
                            # Similar names must not wait behind as many as ten
                            # sequential 20-second artwork downloads.  Reuse an
                            # authorized cached provider image when one exists;
                            # the browser supplies an initial-letter fallback.
                            portrait = self._navidrome_similar_thumbnail_path(item)
                        portrait_url = self.image(portrait) if portrait is not None else self._navidrome_similar_thumbnail_url(item)
                        projected.append(
                            {
                                "name": name,
                                "local_id": str(local["artist_key"]) if local is not None else "",
                                "image": portrait_url,
                                "provider_id": str(item.get("id") or ""),
                            }
                        )
                        if len(projected) >= 10:
                            break
                    result["similar"] = projected
                    if not projected:
                        result["similar_reason"] = "Navidrome returned no Similar Artists for this Artist"
                    result["source"] = "Navidrome"
                    if str(result.get("biography") or "").strip():
                        self._save_artist_profile(
                            artist_id,
                            {
                                "lookup_name": str(artist["name"]),
                                "musicbrainz_id": str(artist.get("mbid") or ""),
                                "biography": str(result["biography"]),
                                "external_url": str(info.get("lastFmUrl") or ""),
                                "source": "Navidrome",
                            },
                        )
                else:
                    result["reason"] = "Artist was not found in Navidrome"
            except (OSError, ValueError, KeyError, TypeError, AttributeError, requests.RequestException):
                result["reason"] = "Navidrome metadata is unavailable for this artist"
        if not result["similar"] and not result.get("similar_reason"):
            result["similar_reason"] = result.get("reason") or "Similar Artists are unavailable"
        if not str(result.get("biography") or "").strip() and stored_profile:
            result.update(stored_profile)
            result.pop("reason", None)
        if not str(result.get("biography") or "").strip():
            try:
                profile = discover_artist_profile(
                    str(artist["name"]),
                    str(artist["mbid"]),
                    self.credentials,
                )
            except (OSError, ValueError, KeyError, TypeError, requests.RequestException):
                profile = None
            if profile:
                result.update(profile)
                result.pop("reason", None)
                self._save_artist_profile(artist_id, profile)
        # Biography presentation must not wait behind the optional provider
        # song/appearance traversal. Indexed/stored appearances already feed
        # the Overview; a deliberate appearance refresh remains independent
        # instead of holding this response for as many as twenty song pages.
        with self.lock:
            self.metadata_cache[artist_id] = result
        projected_artist = artist
        if result.get("genres"):
            projected_artist["genres"] = sorted(set(projected_artist.get("genres", [])) | set(result["genres"]))
        return {
            **result,
            "artist": projected_artist,
        }

    def search_artist(self, artist_id: str) -> dict[str, object]:
        artist = self.artist(artist_id, require_eligible=True)
        with closing(self._connect_artist_catalog()) as db:
            artist_row, _aliases = self._artist_identity_rows(db, artist_id, require_eligible=True)
        found, warnings = discover_artist_images(
            str(artist["name"]), str(artist["mbid"]), self.credentials, enabled=self.artist_sources,
        )
        candidates = []
        for label, data in found:
            with Image.open(BytesIO(data)) as image:
                width, height = image.size
            candidates.append({
                "source": label.split()[0],
                "kind": "banner" if "banner" in label.casefold() else "artist", "acceptable": True,
                "name": label, "width": width, "height": height,
                "bytes": len(data), "digest": sha256(data).hexdigest(),
                "image": self.image(data),
            })
        if "navidrome" in self.artist_sources:
            try:
                navidrome = self._navidrome_thumbnail_path(
                    artist_row, "web-artist-search-v1"
                )
                if navidrome is None:
                    remote = self._navidrome_artists_by_name().get(
                        normalized_artist_identity(artist["name"])
                    )
                    credential_path = self.credentials / "navidrome.json"
                    if remote is not None and credential_path.is_file():
                        credential = json.loads(credential_path.read_text(encoding="utf-8"))
                        navidrome = self._cache_navidrome_thumbnail(
                            artist_row,
                            credential,
                            remote,
                            size=1500,
                            variant="web-artist-search-v1",
                        )
                if navidrome is not None:
                    with Image.open(navidrome) as image:
                        width, height = image.size
                    candidates.append({
                        "source": "Navidrome",
                        "kind": "artist",
                        "acceptable": True,
                        "name": "Navidrome portrait",
                        "width": width,
                        "height": height,
                        "bytes": navidrome.stat().st_size,
                        "digest": sha256(navidrome.read_bytes()).hexdigest(),
                        "image": self.image(navidrome),
                    })
            except (OSError, ValueError, KeyError, TypeError, requests.RequestException, json.JSONDecodeError) as error:
                warnings.append(f"Navidrome: request failed ({type(error).__name__})")
        return {"candidates": candidates, "warnings": warnings}







PreviewApp = Catalog  # Read-only SQLite fixture compatibility.
