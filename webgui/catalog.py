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
from threading import Lock
from urllib.parse import quote, urlsplit
from uuid import UUID
import unicodedata

from PIL import Image, ImageOps
from mutagen import File as MutagenFile
from mutagen.id3 import ID3, ID3NoHeaderError
from mutagen.flac import Picture
import requests

from splined_artist_art import ArtistAlbum, ArtistArtTarget, artist_targets, local_candidates, saved_package
from splined_compilation_authority_policy import replace_embedded_artwork
from interaction.artist_search import discover_artist_images, discover_artist_profile
from webgui.database import Database
from webgui import imaging


ASSETS = Path(__file__).with_name("static")
IMAGE_EXTENSIONS = {".jpg", ".jpeg", ".png", ".webp"}
AUDIO_EXTENSIONS = {".mp3", ".flac", ".m4a", ".mp4", ".ogg", ".opus", ".wav", ".aiff", ".aif"}
LOGGER = logging.getLogger("uvicorn.error")


def artist_credit_position(track_artist: str, artist: str) -> int | None:
    """Match Windows' explicit and joined multi-artist credit rules."""
    selected = artist.strip().casefold()
    if not selected:
        return None
    explicit = [
        credit.strip()
        for credit in re.split(r"[\0;]", track_artist)
        if credit.strip()
    ]
    for position, credit in enumerate(explicit):
        if credit.casefold() == selected:
            return position
    credits = track_artist.casefold()
    for separator in (" & ", " feat. ", " featuring ", " ft. ", " with "):
        credits = credits.replace(separator, "\0")
    for position, credit in enumerate(
        item.strip() for item in re.split(r"[\0;]", credits) if item.strip()
    ):
        if credit == selected:
            return position
    return None


def normalized_artist_identity(value: str) -> str:
    """Collapse invisible/tag-only differences without merging distinct MBIDs."""
    normalized = unicodedata.normalize("NFKC", str(value or ""))
    visible = "".join(
        character for character in normalized
        if unicodedata.category(character) != "Cf"
    )
    return "".join(character.casefold() for character in visible if character.isalnum())


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


def image_metadata(path: Path) -> dict[str, object]:
    with Image.open(path) as image:
        width, height = image.size
    return {
        "name": path.name,
        "width": width,
        "height": height,
        "bytes": path.stat().st_size,
    }


def embedded_track_artwork_bytes(path: Path) -> bytes | None:
    """Read one track's embedded cover; never substitute the album folder cover."""
    artwork: bytes | None = None
    try:
        if path.suffix.casefold() == ".mp3":
            frames = ID3(path).getall("APIC")
            if frames:
                artwork = next((frame.data for frame in frames if frame.type == 3), frames[0].data)
        else:
            audio = MutagenFile(path)
            if audio is not None:
                pictures = getattr(audio, "pictures", None)
                if pictures:
                    artwork = next((picture.data for picture in pictures if picture.type == 3), pictures[0].data)
                elif audio.tags:
                    covers = audio.tags.get("covr")
                    if covers:
                        artwork = bytes(covers[0])
                    elif audio.tags.get("metadata_block_picture"):
                        raw = audio.tags["metadata_block_picture"][0]
                        artwork = Picture(base64.b64decode(raw)).data
    except (OSError, ValueError, ID3NoHeaderError):
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


@dataclass
class Catalog:
    music: Path
    database: Database | Path
    artist_art: Path
    credentials: Path
    comp_thumbnail_side: int = 1200
    allow_comp_repairs: bool = False
    cover_stem: str = "cover"
    cache: Path | None = None
    temporary_cache: Path | None = None
    artist_sources: tuple[str, ...] = ("fanarttv", "deezer", "lastfm", "discogs")
    images: dict[str, Path | bytes] = field(default_factory=dict)
    metadata_cache: dict[str, dict[str, object]] = field(default_factory=dict)
    appearance_cache: dict[str, dict[str, object]] = field(default_factory=dict)
    portrait_cache: dict[str, Path] = field(default_factory=dict)
    portrait_retry_at: dict[str, float] = field(default_factory=dict)
    portrait_failures: dict[str, int] = field(default_factory=dict)
    navidrome_artist_directory: dict[str, dict[str, object]] = field(default_factory=dict)
    navidrome_directory_retry_at: float = 0.0
    navidrome_directory_failures: int = 0
    navidrome_directory_loading: bool = False
    lock: Lock = field(default_factory=Lock)
    image_key: bytes = field(default_factory=lambda: secrets.token_bytes(32),repr=False)

    def album_cover_path(self, row) -> Path | None:
        """Return indexed cover art, repairing old fallback rows on read."""
        raw = str(row["cover_path"] or "").strip()
        if raw:
            indexed = Path(raw)
            if indexed.is_file() and inside(indexed, self.music):
                return indexed
        keys = set(row.keys())
        folder_value = row["path"] if "path" in keys else row["album_path"]
        folder = Path(str(folder_value))
        if not inside(folder, self.music) or not folder.is_dir():
            return None
        stem = (self.cover_stem or "cover").strip().casefold()
        try:
            files = [
                candidate
                for candidate in folder.iterdir()
                if candidate.is_file()
                and not candidate.is_symlink()
                and candidate.suffix.casefold() in IMAGE_EXTENSIONS
            ]
        except OSError:
            return None
        for prefix in dict.fromkeys((stem, "cover")):
            exact = next(
                (candidate for candidate in files if candidate.stem.casefold() == prefix),
                None,
            )
            if exact is not None:
                return exact
        return None

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

    def image(self, source: Path | bytes | None) -> str | None:
        if source is None:
            return None
        if isinstance(source, Path):
            if not self.supported_image_path(source) or not source.is_file():
                return None
            if not self.allowed_image_path(source):
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
            if len(source)>25*1024*1024: raise ValueError('Prepared artwork exceeds the 25 MB image limit')
            with Image.open(BytesIO(source)) as opened:
                suffix={"JPEG":".jpg","PNG":".png","WEBP":".webp"}.get(opened.format)
                if not suffix: raise ValueError('Unsupported artwork image format')
            directory=preview_cache/'web-images'
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
        if isinstance(source, Path) and not self.allowed_image_path(source):
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
    def _aggregate_artist_status(rows) -> str:
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
                # A tagged MusicBrainz identity is the stable authority even
                # when a legacy name-only duplicate currently owns more album
                # rows.  The aliases below still contribute every album.
                bool(str(row["musicbrainz_artistid"] or "").strip()),
                int(row["album_count"] or 0),
                str(row["artist_key"]),
            ),
        )

    @staticmethod
    def _artist_groups(rows):
        """Merge aliases without collapsing distinct same-name artists.

        Two different non-empty MBIDs normally remain distinct.  When every
        row has the same non-empty primary library path, however, the path and
        display name are stronger local evidence than a stray tag MBID.  This
        removes the common 19-album/1-album split while keeping genuinely
        different same-name artists separate.
        """
        by_name: dict[str, list] = {}
        for row in rows:
            by_name.setdefault(normalized_artist_identity(row["artist_name"]), []).append(row)
        groups = []
        for name_rows in by_name.values():
            by_mbid: dict[str, list] = {}
            blank = []
            for row in name_rows:
                mbid = str(row["musicbrainz_artistid"] or "").strip().casefold()
                (by_mbid.setdefault(mbid, []) if mbid else blank).append(row)
            paths = [
                str(row["primary_path"] or "").replace("\\", "/").rstrip("/")
                if "primary_path" in row.keys() else ""
                for row in name_rows
            ]
            same_primary_path = bool(paths) and all(paths) and len(set(paths)) == 1
            if len(by_mbid) <= 1 or same_primary_path:
                groups.append(name_rows)
            else:
                groups.extend(by_mbid.values())
                if blank:
                    groups.append(blank)
        return groups

    def _artist_identity_rows(self, db, artist_id: str):
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
            selected = db.execute(
                "SELECT artist_key,artist_name,'' AS musicbrainz_artistid,'' AS primary_path,"
                "0 AS album_count,'unprocessed' AS status FROM artists WHERE artist_key=?",
                (artist_id,),
            ).fetchone()
            candidates = db.execute(
                "SELECT artist_key,artist_name,'' AS musicbrainz_artistid,'' AS primary_path,"
                "0 AS album_count,'unprocessed' AS status FROM artists"
            ).fetchall()
        if selected is None:
            raise KeyError("Artist not found")
        identity = normalized_artist_identity(selected["artist_name"])
        selected_mbid = str(selected["musicbrainz_artistid"] or "").strip().casefold()
        selected_path = str(selected["primary_path"] or "").replace("\\", "/").rstrip("/")
        candidates = [
            row for row in candidates
            if normalized_artist_identity(row["artist_name"]) == identity
            or bool(selected_mbid) and str(row["musicbrainz_artistid"] or "").strip().casefold()
            == selected_mbid
        ]
        nonempty = {
            str(row["musicbrainz_artistid"] or "").strip().casefold()
            for row in candidates if str(row["musicbrainz_artistid"] or "").strip()
        }
        if selected_mbid:
            aliases = [
                row for row in candidates
                if not str(row["musicbrainz_artistid"] or "").strip()
                or str(row["musicbrainz_artistid"] or "").strip().casefold() == selected_mbid
                or bool(selected_path) and str(row["primary_path"] or "").replace("\\", "/").rstrip("/") == selected_path
            ]
        elif len(nonempty) <= 1:
            aliases = candidates
        else:
            aliases = [row for row in candidates if not str(row["musicbrainz_artistid"] or "").strip()]
        return self._canonical_artist_row(aliases), aliases

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

    def _navidrome_thumbnail_path(self, artist) -> Path | None:
        directory = self.cache / "navidrome-thumbnails" if self.cache is not None else None
        if directory is None:
            return None
        identity = sha256(str(artist["artist_key"]).encode("utf-8")).hexdigest()[:32]
        path = directory / "artists" / ("web-v1-" + identity + ".png")
        return path if path.is_file() else None

    def _cache_navidrome_thumbnail(self, artist, credential, remote_artist) -> Path | None:
        cached = self._navidrome_thumbnail_path(artist)
        if cached is not None:
            return cached
        cover_id = str(remote_artist.get("coverArt") or "").strip()
        directory = self.cache / "navidrome-thumbnails" if self.cache is not None else None
        if not cover_id or directory is None:
            return None
        base = str(credential.get("server_url") or "").rstrip("/")
        response = requests.get(
            base + "/rest/getCoverArt.view",
            params={**self._navidrome_auth_query(credential), "id": cover_id, "size": "192"},
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
        identity = sha256(str(artist["artist_key"]).encode("utf-8")).hexdigest()[:32]
        path = directory / ("web-v1-" + identity + ".png")
        descriptor, temporary = tempfile.mkstemp(prefix=".navidrome-", suffix=".png", dir=directory)
        try:
            os.close(descriptor)
            with Image.open(BytesIO(response.content)) as opened:
                opened.convert("RGBA").save(temporary, format="PNG", optimize=True)
            os.replace(temporary, path)
        finally:
            Path(temporary).unlink(missing_ok=True)
        for row_id in list(self.portrait_cache):
            self.portrait_cache.pop(row_id, None)
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
                "SELECT album_key,featured,navidrome_album_id,song_paths_json "
                "FROM artist_album_appearances WHERE artist_key=?",
                (artist_id,),
            ).fetchall()
        except Exception:
            # SQLite fixtures intentionally do not carry the WebGUI-only table.
            if isinstance(self.database, Database):
                raise
            return {"album_keys": set(), "featured_album_keys": set(), "song_paths": {}}
        return {
            "album_keys": {str(row["album_key"]) for row in rows},
            "featured_album_keys": {str(row["album_key"]) for row in rows if bool(row["featured"])},
            "song_paths": {
                str(row["album_key"]): json.loads(str(row["song_paths_json"] or "[]"))
                for row in rows
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
        """Project identified Featured releases from indexed PostgreSQL tracks.

        The candidate SQL is deliberately bounded by the selected display names;
        Windows' exact multi-credit parser remains the final authority. COMP
        albums stay in the separate track report and are never promoted here.
        """
        alias_ids = {str(row["artist_key"]) for row in aliases}
        names = list(dict.fromkeys(
            str(row["artist_name"] or "").strip() for row in aliases
            if str(row["artist_name"] or "").strip()
        ))
        matches = []
        try:
            for name in names:
                escaped = name.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_")
                matches.extend(db.execute(
                    "SELECT albums.album_key,albums.artist_key,owner.artist_name AS album_artist_name,"
                    "tracks.path,tracks.artist_name FROM tracks "
                    "JOIN albums ON albums.album_key=tracks.album_key "
                    "JOIN artists owner ON owner.artist_key=albums.artist_key "
                    "WHERE TRIM(COALESCE(albums.musicbrainz_albumid,''))<>'' "
                    "AND tracks.artist_name LIKE ? ESCAPE '\\'",
                    ("%" + escaped + "%",),
                ).fetchall())
        except sqlite3.OperationalError:
            return {"album_keys": set(), "featured_album_keys": set(), "song_paths": {}}
        album_keys: set[str] = set()
        featured: set[str] = set()
        paths: dict[str, set[str]] = {}
        normalized_names = {normalized_artist_identity(name) for name in names}
        for row in matches:
            if not any(artist_credit_position(str(row["artist_name"] or ""), name) is not None for name in names):
                continue
            if (
                str(row["artist_key"]) not in alias_ids
                and normalized_artist_identity(str(row["album_artist_name"] or ""))
                in normalized_names
            ):
                # A same-name catalog identity with a different MBID/path is
                # not a Featured appearance; the text credit cannot identify
                # which homonymous Artist it represents.
                continue
            key = str(row["album_key"])
            album_keys.add(key)
            paths.setdefault(key, set()).add(str(row["path"] or "").replace("\\", "/").strip("/"))
            if str(row["artist_key"]) not in alias_ids:
                featured.add(key)
        return {
            "album_keys": album_keys,
            "featured_album_keys": featured,
            "song_paths": {key: sorted(values) for key, values in paths.items()},
        }

    def _artist_appearance_context(self, artist, aliases, db, *, refresh=False) -> dict[str, object]:
        canonical_id = str(artist["artist_key"])
        with self.lock:
            cached = self.appearance_cache.get(canonical_id)
        if cached is not None and not refresh:
            return cached
        stored = self._stored_artist_appearances(db, canonical_id)
        indexed = self._indexed_artist_appearances(db, artist, aliases)
        baseline = self._merge_artist_appearances(stored, indexed)
        if not refresh:
            return baseline
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
                if next((str(row["artist_key"]) for row in all_albums if str(row["album_key"]) == key), "") not in alias_ids
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
            if isinstance(self.database, Database):
                db.execute("DELETE FROM artist_album_appearances WHERE artist_key=?", (canonical_id,))
                for key in sorted(song_paths):
                    db.execute(
                        "INSERT INTO artist_album_appearances"
                        "(artist_key,album_key,navidrome_album_id,featured,song_paths_json) VALUES(?,?,?,?,?)",
                        (canonical_id, key, remote_album_ids.get(key, ""), int(key in featured),
                         json.dumps(context["song_paths"][key])),
                    )
                db.commit()
            with self.lock:
                self.appearance_cache[canonical_id] = context
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
            artist, aliases = self._artist_identity_rows(db, artist_id)
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

    def artists(self, query: str) -> list[dict[str, object]]:
        with closing(self.connect()) as db:
            rows = db.execute(
                "SELECT artists.artist_key, artists.artist_name, artists.musicbrainz_artistid, "
                "artists.primary_path, COUNT(albums.album_key) AS album_count, artists.status "
                "FROM artists LEFT JOIN albums ON albums.artist_key=artists.artist_key "
                "WHERE artists.artist_name LIKE ? ESCAPE '\\' "
                "GROUP BY artists.artist_key ORDER BY artists.artist_name COLLATE NOCASE ",
                ("%" + query.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_") + "%",),
            ).fetchall()
        result = []
        for group in self._artist_groups(rows):
            canonical = self._canonical_artist_row(group)
            # Initial catalog loading is PostgreSQL-only. Local/Navidrome
            # artwork is hydrated by the dedicated lazy endpoint so hundreds
            # of artist folders never block the first useful render.
            portrait = self.portrait_cache.get(str(canonical["artist_key"]))
            portrait_url = self.image(portrait) if portrait is not None and portrait.is_file() else None
            result.append({
                "id": canonical["artist_key"],
                "name": canonical["artist_name"],
                "albums": sum(int(row["album_count"] or 0) for row in group),
                "status": self._aggregate_artist_status(group),
                "portrait": portrait_url,
            })
        return sorted(result, key=lambda item: normalized_artist_identity(item["name"]))

    def artist_thumbnails(self) -> list[dict[str, str]]:
        """Return local thumbnails immediately and Navidrome fallbacks lazily."""
        with closing(self.connect()) as db:
            rows = db.execute(
                "SELECT artists.artist_key, artists.artist_name, artists.musicbrainz_artistid, "
                "artists.primary_path, COUNT(albums.album_key) AS album_count, artists.status "
                "FROM artists LEFT JOIN albums ON albums.artist_key=artists.artist_key "
                "GROUP BY artists.artist_key ORDER BY artists.artist_name COLLATE NOCASE"
            ).fetchall()
        remote_artists = self._navidrome_artists_by_name()
        result = []
        for group in self._artist_groups(rows):
            canonical = self._canonical_artist_row(group)
            portrait_path = None
            for row in [canonical, *(item for item in group if item is not canonical)]:
                portrait_path = self.artist_thumbnail_path(row)
                if portrait_path is not None:
                    break
            portrait = self.image(portrait_path) if portrait_path is not None else ""
            if not portrait and normalized_artist_identity(canonical["artist_name"]) in remote_artists:
                portrait = self.artist_thumbnail_url(str(canonical["artist_key"]))
            if portrait:
                result.append({"id": str(canonical["artist_key"]), "portrait": portrait})
        return result

    def artist_thumbnail_path(self, artist) -> Path | None:
        artist_id = str(artist["artist_key"])
        if artist_id in self.portrait_cache:
            return self.portrait_cache[artist_id]
        direct = ArtistArtTarget(Path(str(artist["primary_path"])), "artist")
        raw_id = str(artist["musicbrainz_artistid"] or "").strip()
        try:
            identity = "mbid-" + str(UUID(raw_id))
        except ValueError:
            digest = sha256(str(artist["artist_name"]).strip().casefold().encode("utf-8"))
            identity = "name-" + digest.digest()[:16].hex()
        managed = ArtistArtTarget(self.artist_art, "artist-" + identity, True)
        portrait = saved_package((direct, managed)).portrait
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
            self.portrait_cache[artist_id] = portrait
        else:
            self.portrait_cache.pop(artist_id, None)
        return portrait

    def artist_thumbnail(self, artist_id: str) -> Path:
        """Resolve saved Artist Art without emitting speculative 404 requests."""
        with closing(self.connect()) as db:
            artist = db.execute(
                "SELECT artist_key,artist_name,musicbrainz_artistid,primary_path "
                "FROM artists WHERE artist_key=?",
                (artist_id,),
            ).fetchone()
            if artist is None:
                raise KeyError("Artist not found")
        portrait = self.artist_thumbnail_path(artist)
        if portrait is None:
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
                        self.portrait_cache[artist_key] = portrait
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

    def albums(self, query: str, fallback_only: bool = False) -> list[dict[str, object]]:
        with closing(self.connect()) as db:
            rows = db.execute(
                "SELECT albums.album_key, albums.album_name, albums.artist_key, "
                "artists.artist_name, albums.path, albums.cover_path, albums.track_count, "
                "albums.status, albums.compilation, albums.musicbrainz_albumid FROM albums "
                "JOIN artists ON artists.artist_key=albums.artist_key "
                "WHERE (albums.album_name LIKE ? ESCAPE '\\' OR artists.artist_name LIKE ? ESCAPE '\\') "
                "AND (?=0 OR TRIM(COALESCE(albums.musicbrainz_albumid,''))='') "
                "ORDER BY albums.album_name COLLATE NOCASE ",
                ("%" + query.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_") + "%",) * 2
                + (1 if fallback_only else 0,),
            ).fetchall()
        items = []
        for row in rows:
            cover = self.album_cover_path(row)
            cover_url = self.image(cover)
            items.append({
                "id": row["album_key"], "name": row["album_name"],
                "artist_id": row["artist_key"], "artist_name": row["artist_name"],
                "cover": cover_url, "tracks": row["track_count"],
                "status": row["status"], "compilation": bool(row["compilation"]),
                "album_mbid": row["musicbrainz_albumid"] or "",
            })
        return items

    def artist(self, artist_id: str) -> dict[str, object]:
        with closing(self.connect()) as db:
            artist, aliases = self._artist_identity_rows(db, artist_id)
            alias_ids = [str(row["artist_key"]) for row in aliases]
            placeholders = ",".join("?" for _ in alias_ids)
            primary_rows = db.execute(
                "SELECT albums.*,artists.artist_name AS album_artist "
                "FROM albums JOIN artists ON artists.artist_key=albums.artist_key "
                f"WHERE albums.artist_key IN ({placeholders}) ORDER BY albums.album_name COLLATE NOCASE",
                tuple(alias_ids),
            ).fetchall()
            appearance = self._artist_appearance_context(artist, aliases, db)
            appearance_keys = set(appearance.get("album_keys", set()))
            if appearance_keys:
                appearance_placeholders = ",".join("?" for _ in appearance_keys)
                appearance_rows = db.execute(
                    "SELECT albums.*,artists.artist_name AS album_artist "
                    "FROM albums JOIN artists ON artists.artist_key=albums.artist_key "
                    f"WHERE albums.album_key IN ({appearance_placeholders}) "
                    "AND TRIM(COALESCE(albums.musicbrainz_albumid,''))<>''",
                    tuple(sorted(appearance_keys)),
                ).fetchall()
            else:
                appearance_rows = []
            top_tracks = [item["title"] for item in db.execute(
                "SELECT tracks.title FROM tracks JOIN albums ON albums.album_key=tracks.album_key "
                f"WHERE albums.artist_key IN ({placeholders}) ORDER BY tracks.title COLLATE NOCASE LIMIT 10",
                tuple(alias_ids),
            ).fetchall()]
        featured_keys = set(appearance.get("featured_album_keys", set()))
        primary_keys = {str(row["album_key"]) for row in primary_rows}
        rows = [*primary_rows, *(row for row in appearance_rows if str(row["album_key"]) not in primary_keys)]
        targets = artist_targets(
            [ArtistAlbum(artist["artist_name"], Path(row["path"]),
                         row["musicbrainz_albumid"] or "", artist["musicbrainz_artistid"] or "")
             for row in primary_rows if inside(Path(row["path"]), self.music)],
            artist["artist_name"], self.artist_art,
        )
        package = saved_package(targets)
        portrait = self.artist_thumbnail_path(artist)
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
            albums.append({
                "id": row["album_key"], "name": row["album_name"],
                "tracks": row["track_count"], "status": row["status"],
                "compilation": bool(row["compilation"]),
                "featured": str(row["album_key"]) in featured_keys,
                "album_artist": row["album_artist"] or "",
                "various_artists": normalized_artist_identity(row["album_artist"] or "")
                    == "variousartists",
                "embedded": int(row["embedded_tracks"] or 0) if "embedded_tracks" in row.keys() else 0,
                "album_mbid": row["musicbrainz_albumid"] or "",
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
                "cover": self.image(cover),
            })
        return {
            "id": artist["artist_key"], "name": artist["artist_name"],
            "mbid": artist["musicbrainz_artistid"] or "",
            "status": self._aggregate_artist_status(aliases), "albums": albums,
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
            track_titles = [item["title"] for item in db.execute(
                "SELECT title FROM tracks WHERE album_key=? ORDER BY path LIMIT 12", (album_id,),
            ).fetchall()]
        if row is None:
            raise KeyError("Album not found")
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
        return {
            "id": album_id, "name": row["album_name"], "status": row["status"],
            "artist_id": row["artist_key"], "artist_name": row["artist_name"],
            "tracks": row["track_count"], "compilation": bool(row["compilation"]),
            "album_mbid": row["musicbrainz_albumid"] or "", "year": row["release_year"] or "",
            "embedded": int(row["embedded_tracks"] or 0) if "embedded_tracks" in row.keys() else 0,
            "cover": self.image(cover), "cover_meta": image_metadata(cover) if cover and cover.is_file() else None,
            "candidates": candidates, "track_titles": track_titles,
        }

    def fallback_tracks(self, album_id: str) -> list[dict[str, object]]:
        with closing(self.connect()) as db:
            album = db.execute("SELECT path, musicbrainz_albumid FROM albums WHERE album_key=?",
                               (album_id,)).fetchone()
            if album is None:
                raise KeyError("Album not found")
            if str(album["musicbrainz_albumid"] or "").strip():
                raise ValueError("This Album has a MusicBrainz Album ID and is not COMP fallback")
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
                "SELECT albums.path AS album_path,albums.musicbrainz_albumid,tracks.path "
                "FROM albums JOIN tracks ON tracks.album_key=albums.album_key "
                "WHERE albums.album_key=? AND tracks.track_key=?",
                (album_id, track_key),
            ).fetchone()
            if row is None or str(row["musicbrainz_albumid"] or "").strip():
                raise KeyError("Recorded COMP thumbnail not found")
            recorded = db.execute(
                "SELECT 1 FROM compilation_track_artwork WHERE track_path=? "
                "AND outcome IN ('embedded-replaced','embedded-existing')",
                (str(row["path"]),),
            ).fetchone()
        folder, path = Path(row["album_path"]), Path(row["path"])
        if not recorded or not inside(folder, self.music) or not inside(path, folder):
            raise KeyError("Recorded COMP thumbnail not found")
        artwork = embedded_track_image(path)
        if not artwork:
            raise KeyError("Recorded COMP thumbnail is unavailable")
        return artwork

    def artist_comp_albums(self, artist_id: str) -> list[dict[str, object]]:
        """Return indexed COMP cards without touching every track on disk."""
        return self._artist_comp_groups(artist_id, verify_tracks=False)

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
            rows = db.execute(
                "SELECT albums.album_key,albums.artist_key,tracks.artist_name "
                "FROM albums LEFT JOIN tracks ON tracks.album_key=albums.album_key "
                "WHERE TRIM(COALESCE(albums.musicbrainz_albumid,''))='' "
                f"AND (albums.artist_key IN ({placeholders}) OR tracks.artist_name LIKE ? ESCAPE '\\' "
                f"{appearance_clause})",
                (*alias_ids, "%" + name.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_") + "%", *appearance_ids),
            ).fetchall()
        matching = set()
        for row in rows:
            if (
                str(row["album_key"]) in appearance_ids
                or str(row["artist_key"]) in alias_ids
                or artist_credit_position(str(row["artist_name"] or ""), name)
                is not None
            ):
                matching.add(str(row["album_key"]))
        return sorted(matching)

    def artist_comp_report(self, artist_id: str) -> list[dict[str, object]]:
        """Project a focused artist's verified tracks for an opened COMP report."""
        return self._artist_comp_groups(artist_id, verify_tracks=True)

    def _artist_comp_groups(self, artist_id: str, *, verify_tracks: bool) -> list[dict[str, object]]:
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
            escaped_name = name.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_")
            rows = db.execute(
                "SELECT albums.album_key, albums.album_name, albums.path AS album_path, "
                "albums.cover_path, albums.release_year, albums.artist_key, tracks.track_key, tracks.path, "
                "tracks.title, tracks.artist_name FROM albums "
                "JOIN tracks ON tracks.album_key=albums.album_key "
                "WHERE TRIM(COALESCE(albums.musicbrainz_albumid,''))='' "
                "AND (tracks.artist_name LIKE ? ESCAPE '\\' "
                f"OR (albums.artist_key IN ({placeholders}) AND TRIM(COALESCE(tracks.artist_name,''))='') "
                f"{appearance_clause}) "
                "ORDER BY albums.album_name COLLATE NOCASE, tracks.path COLLATE NOCASE",
                ("%" + escaped_name + "%", *alias_ids, *appearance_ids),
            ).fetchall()
            has_ledger = db.execute(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='compilation_track_artwork'"
            ).fetchone() is not None
            completed = {str(row[0]) for row in db.execute(
                "SELECT track_path FROM compilation_track_artwork "
                "WHERE outcome IN ('embedded-replaced','embedded-existing')",
            )} if has_ledger else set()
        groups: dict[str, dict[str, object]] = {}
        seen: set[str] = set()
        appearance_paths = appearance.get("song_paths", {})
        for row in rows:
            credits = str(row["artist_name"] or "").replace("\0", ";")
            row_path = str(row["path"]).replace("\\", "/").strip("/")
            expected_paths = [
                str(value).replace("\\", "/").strip("/")
                for value in appearance_paths.get(str(row["album_key"]), [])
            ]
            appearance_match = any(
                row_path == expected or row_path.endswith("/" + expected)
                or expected.endswith("/" + row_path)
                for expected in expected_paths if expected
            )
            if not (artist_credit_position(credits, name) is not None
                    or not credits.strip() and row["artist_key"] in alias_ids
                    or appearance_match):
                continue
            folder, track_path = Path(row["album_path"]), Path(row["path"])
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
                cover = self.album_cover_path(row)
                groups[key] = {
                    "id": key, "name": row["album_name"], "year": row["release_year"] or "",
                    # The album card uses the indexed folder cover. Embedded
                    # track art remains isolated to COMP report track previews.
                    "cover": self.image(cover),
                    "compilation": True, "album_mbid": "", "tracks": [],
                }
            completed_track = str(track_path) in completed
            groups[key]["tracks"].append({
                "key": row["track_key"], "file": track_path.name, "title": row["title"],
                "completed": completed_track,
                "status": "checking" if completed_track else "blue",
                "thumbnail": None,
            })
        return list(groups.values())

    def comp_track_status(self, album_id: str, track_key: str) -> dict[str, object]:
        """Check one COMP thumbnail lazily so a large Artist report opens quickly."""
        with closing(self.connect()) as db:
            row = db.execute(
                "SELECT albums.path AS album_path, albums.musicbrainz_albumid, tracks.path "
                "FROM albums JOIN tracks ON tracks.album_key=albums.album_key "
                "WHERE albums.album_key=? AND tracks.track_key=?", (album_id, track_key),
            ).fetchone()
            if row is None or str(row["musicbrainz_albumid"] or "").strip():
                raise KeyError("COMP track not found")
            path, folder = Path(row["path"]), Path(row["album_path"])
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
            return {"status": "purple", "repairable": False, "width": 0, "height": 0,
                    "side": self.comp_thumbnail_side}
        try:
            with Image.open(BytesIO(artwork)) as opened:
                width, height = opened.size
        except (OSError, ValueError):
            return {"status": "purple", "repairable": False, "width": 0, "height": 0,
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

    def artist_metadata(self, artist_id: str, *, refresh: bool = False) -> dict[str, object]:
        if refresh:
            with self.lock:
                self.metadata_cache.pop(artist_id, None)
                self.appearance_cache.pop(artist_id, None)
        with self.lock:
            cached = self.metadata_cache.get(artist_id)
        if cached is not None:
            return {
                **cached,
                "artist": self.artist(artist_id),
                "comp_albums": self.artist_comp_albums(artist_id),
            }
        artist = self.artist(artist_id)
        stored_profile = self._stored_artist_profile(artist_id)
        result: dict[str, object] = {"source": "unavailable", "biography": "", "similar": [], "top_songs": artist["top_tracks"]}
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
                match = next((item for item in matches if str(item.get("name", "")).casefold() == str(artist["name"]).casefold()), None)
                if match:
                    info = call("getArtistInfo2", {"id": str(match["id"])}).get("artistInfo2", {})
                    biography = re.sub(r"<[^>]+>", " ", str(info.get("biography") or ""))
                    result["biography"] = " ".join(unescape(biography).split())
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
                            try:
                                portrait = self._cache_navidrome_similar_thumbnail(item, credential)
                            except (OSError, ValueError, KeyError, TypeError, requests.RequestException):
                                portrait = None
                        projected.append(
                            {
                                "name": name,
                                "local_id": str(local["artist_key"]) if local is not None else "",
                                "image": self.image(portrait) if portrait is not None else "",
                                "provider_id": str(item.get("id") or ""),
                            }
                        )
                        if len(projected) >= 10:
                            break
                    result["similar"] = projected
                    if not projected:
                        result["similar_reason"] = "Navidrome returned no Similar Artists for this Artist"
                    result["source"] = "Navidrome"
                    try:
                        top = call("getTopSongs", {"artist": str(artist["name"]), "count": "10"}).get("topSongs", {}).get("song", [])
                        if isinstance(top, dict):
                            top = [top]
                        songs = [str(item.get("title")) for item in top if isinstance(item, dict) and item.get("title")]
                        if songs:
                            result["top_songs"] = list(dict((song.casefold(), song) for song in songs).values())[:10]
                    except (OSError, ValueError, KeyError, TypeError, AttributeError, requests.RequestException):
                        pass  # Biography and similar artists remain useful without top songs.
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
        with self.lock:
            self.metadata_cache[artist_id] = result
        try:
            self.refresh_artist_appearances(artist_id)
        except (OSError, ValueError, KeyError, TypeError, requests.RequestException):
            pass
        return {
            **result,
            "artist": self.artist(artist_id),
            "comp_albums": self.artist_comp_albums(artist_id),
        }

    def search_artist(self, artist_id: str) -> dict[str, object]:
        artist = self.artist(artist_id)
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
        return {"candidates": candidates, "warnings": warnings}







PreviewApp = Catalog  # Read-only SQLite fixture compatibility.
