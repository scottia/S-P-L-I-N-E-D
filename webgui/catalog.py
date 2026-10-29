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
import os
from pathlib import Path
import re
import secrets
import sqlite3
from threading import Lock
from urllib.parse import urlsplit

from PIL import Image, ImageOps
from mutagen import File as MutagenFile
from mutagen.id3 import ID3, ID3NoHeaderError
from mutagen.flac import Picture
import requests

from splined_artist_art import ArtistAlbum, artist_targets, local_candidates, saved_package
from splined_compilation_authority_policy import replace_embedded_artwork
from interaction.artist_search import discover_artist_images
from webgui.database import Database


ASSETS = Path(__file__).with_name("static")
IMAGE_EXTENSIONS = {".jpg", ".jpeg", ".png", ".webp"}
AUDIO_EXTENSIONS = {".mp3", ".flac", ".m4a", ".mp4", ".ogg", ".opus", ".wav", ".aiff", ".aif"}


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
    cache: Path | None = None
    artist_sources: tuple[str,...] = ("fanarttv","deezer")
    images: dict[str, Path | bytes] = field(default_factory=dict)
    metadata_cache: dict[str, dict[str, object]] = field(default_factory=dict)
    lock: Lock = field(default_factory=Lock)
    image_key: bytes = field(default_factory=lambda: secrets.token_bytes(32),repr=False)

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
            if source.suffix.casefold() not in IMAGE_EXTENSIONS or not source.is_file():
                return None
            if not (inside(source, self.music) or inside(source, self.artist_art) or self.cache is not None and inside(source, self.cache)):
                return None
            # Signed path handles do not consume the bounded in-memory preview
            # cache, so browsing a large library cannot expire its early covers.
            encoded=base64.urlsafe_b64encode(str(source.resolve()).encode('utf-8')).decode().rstrip('=')
            signature=hmac.new(self.image_key,encoded.encode(),hashlib.sha256).hexdigest()
            return '/api/image/p.'+encoded+'.'+signature
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
                _,encoded,signature=token.split('.')
                expected=hmac.new(self.image_key,encoded.encode(),hashlib.sha256).hexdigest()
                if not hmac.compare_digest(signature,expected): raise ValueError('Invalid artwork handle')
                source=Path(base64.urlsafe_b64decode(encoded+'='*(-len(encoded)%4)).decode('utf-8'))
                if source.suffix.casefold() not in IMAGE_EXTENSIONS: raise ValueError('Unsupported artwork type')
            except (ValueError,UnicodeError): raise ValueError('Invalid artwork handle') from None
        else:
            with self.lock:
                source = self.images.get(token)
        if source is None:
            raise KeyError("Image preview expired; select the artwork again")
        if isinstance(source, Path) and not (inside(source, self.music) or inside(source, self.artist_art) or self.cache is not None and inside(source, self.cache)):
            raise ValueError("Image is outside configured artwork locations")
        return source

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
                "SELECT artists.artist_key, artists.artist_name, COUNT(albums.album_key) AS album_count, artists.status "
                "FROM artists LEFT JOIN albums ON albums.artist_key=artists.artist_key "
                "WHERE artists.artist_name LIKE ? ESCAPE '\\' "
                "GROUP BY artists.artist_key ORDER BY artists.artist_name COLLATE NOCASE ",
                ("%" + query.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_") + "%",),
            ).fetchall()
        return [
            {
                "id": row["artist_key"],
                "name": row["artist_name"],
                "albums": row["album_count"],
                "status": row["status"],
            }
            for row in rows
        ]

    def albums(self, query: str, fallback_only: bool = False) -> list[dict[str, object]]:
        with closing(self.connect()) as db:
            rows = db.execute(
                "SELECT albums.album_key, albums.album_name, albums.artist_key, "
                "artists.artist_name, albums.cover_path, albums.track_count, "
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
            cover = Path(row["cover_path"]) if row["cover_path"] else None
            if cover is not None and not inside(cover, self.music):
                cover = None
            items.append({
                "id": row["album_key"], "name": row["album_name"],
                "artist_id": row["artist_key"], "artist_name": row["artist_name"],
                "cover": self.image(cover), "tracks": row["track_count"],
                "status": row["status"], "compilation": bool(row["compilation"]),
                "album_mbid": row["musicbrainz_albumid"] or "",
            })
        return items

    def artist(self, artist_id: str) -> dict[str, object]:
        with closing(self.connect()) as db:
            artist = db.execute(
                "SELECT artist_key, artist_name, musicbrainz_artistid, album_count, status "
                "FROM artists WHERE artist_key=?", (artist_id,),
            ).fetchone()
            if artist is None:
                raise KeyError("Artist not found")
            rows = db.execute(
                "SELECT * "
                "FROM albums WHERE artist_key=? ORDER BY album_name COLLATE NOCASE",
                (artist_id,),
            ).fetchall()
            top_tracks = [item["title"] for item in db.execute(
                "SELECT tracks.title FROM tracks JOIN albums ON albums.album_key=tracks.album_key "
                "WHERE albums.artist_key=? ORDER BY tracks.title COLLATE NOCASE LIMIT 10", (artist_id,),
            ).fetchall()]
        targets = artist_targets(
            [ArtistAlbum(artist["artist_name"], Path(row["path"]),
                         row["musicbrainz_albumid"] or "", artist["musicbrainz_artistid"] or "")
             for row in rows if inside(Path(row["path"]), self.music)],
            artist["artist_name"], self.artist_art,
        )
        package = saved_package(targets)
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
            cover = Path(row["cover_path"]) if row["cover_path"] else None
            if cover is not None and not inside(cover, self.music):
                cover = None
            albums.append({
                "id": row["album_key"], "name": row["album_name"],
                "tracks": row["track_count"], "status": row["status"],
                "compilation": bool(row["compilation"]),
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
                "cover_found": bool(row["cover_found"]),
                "cover": self.image(cover),
            })
        return {
            "id": artist_id, "name": artist["artist_name"],
            "mbid": artist["musicbrainz_artistid"] or "",
            "status": artist["status"], "albums": albums,
            "top_tracks": top_tracks,
            "portrait": self.image(package.portrait),
            "banner": self.image(package.banner),
            "portrait_meta": image_metadata(package.portrait) if package.portrait and package.portrait.is_file() else None,
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
        cover = Path(row["cover_path"]) if row["cover_path"] else None
        if cover is not None and not inside(cover, self.music):
            cover = None
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
        tracks = [{"key": row["track_key"], "path": row["path"],
                   "title": row["title"], "artist": row["artist_name"],
                   "file": Path(row["path"]).name,
                   "completed": str(row["path"]).casefold() in completed}
                  for row in rows if inside(Path(row["path"]), folder)]
        if not tracks:
            # Windows fills an older uncached fallback from this folder only.
            tracks = [{"key": str(path), "path": str(path), "title": path.stem,
                       "artist": "", "file": path.name,
                       "completed": str(path).casefold() in completed}
                      for path in sorted(folder.iterdir(), key=lambda item: item.name.casefold())
                      if path.is_file() and not path.is_symlink() and path.suffix.casefold() in AUDIO_EXTENSIONS]
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

    def artist_comp_report(self, artist_id: str) -> list[dict[str, object]]:
        """Project only a focused artist's tracks from missing-ID COMP albums."""
        with closing(self.connect()) as db:
            artist = db.execute("SELECT artist_name FROM artists WHERE artist_key=?", (artist_id,)).fetchone()
            if artist is None:
                raise KeyError("Artist not found")
            name = str(artist["artist_name"]).strip()
            rows = db.execute(
                "SELECT albums.album_key, albums.album_name, albums.path AS album_path, "
                "albums.cover_path, albums.release_year, albums.artist_key, tracks.track_key, tracks.path, "
                "tracks.title, tracks.artist_name FROM albums "
                "JOIN tracks ON tracks.album_key=albums.album_key "
                "WHERE TRIM(COALESCE(albums.musicbrainz_albumid,''))='' "
                "AND (instr(lower(COALESCE(tracks.artist_name,'')), lower(?))>0 "
                "OR (albums.artist_key=? AND TRIM(COALESCE(tracks.artist_name,''))='')) "
                "ORDER BY albums.album_name COLLATE NOCASE, tracks.path COLLATE NOCASE",
                (name, artist_id),
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
        for row in rows:
            credits = str(row["artist_name"] or "").replace("\0", ";")
            if not (any(credit.strip().casefold() == name.casefold() for credit in credits.split(";"))
                    or not credits.strip() and row["artist_key"] == artist_id):
                continue
            folder, track_path = Path(row["album_path"]), Path(row["path"])
            if not inside(folder, self.music) or not inside(track_path, folder) or not track_path.is_file():
                continue
            path_key = str(track_path).casefold()
            if path_key in seen:
                continue
            seen.add(path_key)
            key = str(row["album_key"])
            if key not in groups:
                cover = Path(row["cover_path"]) if row["cover_path"] else None
                groups[key] = {
                    "id": key, "name": row["album_name"], "year": row["release_year"] or "",
                    "cover": self.image(cover) if cover and inside(cover, self.music) else None,
                    "compilation": True, "album_mbid": "", "tracks": [],
                }
            groups[key]["tracks"].append({
                "key": row["track_key"], "file": track_path.name, "title": row["title"],
                "completed": str(track_path).casefold() in completed,
                "status": "checking" if str(track_path).casefold() in completed else "blue",
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
                "SELECT 1 FROM compilation_track_artwork WHERE track_path=? COLLATE NOCASE "
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
                "width": width, "height": height, "side": self.comp_thumbnail_side}

    def repair_artist_comp_track(self, artist_id: str, album_id: str, track_key: str) -> dict[str, object]:
        """Repair one recorded, readable purple thumbnail in the isolated music copy."""
        if not self.allow_comp_repairs:
            raise ValueError("COMP repair is disabled for this browser service")
        allowed = any(group["id"] == album_id and any(track["key"] == track_key for track in group["tracks"])
                      for group in self.artist_comp_report(artist_id))
        if not allowed:
            raise ValueError("Track is not in this Artist's COMP report")
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
            thumbnail = ImageOps.fit(ImageOps.exif_transpose(opened).convert("RGB"),
                                     (self.comp_thumbnail_side, self.comp_thumbnail_side),
                                     method=Image.Resampling.LANCZOS)
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

    def artist_metadata(self, artist_id: str) -> dict[str, object]:
        with self.lock:
            cached = self.metadata_cache.get(artist_id)
        if cached is not None:
            return cached
        artist = self.artist(artist_id)
        result: dict[str, object] = {"source": "sample", "biography": "", "similar": [], "top_songs": artist["top_tracks"]}
        credential_path = self.credentials / "navidrome.json"
        if not credential_path.is_file():
            result["reason"] = "Navidrome credentials are not present in this test bed"
            return result
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
                result["similar"] = [str(item.get("name")) for item in similar if isinstance(item, dict) and item.get("name")][:10]
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
        with self.lock:
            self.metadata_cache[artist_id] = result
        return result

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
