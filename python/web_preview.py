"""Read-only browser prototype for the isolated SPLINED Python test library.

The index is opened read-only. Artwork is served only when its path is inside the
configured copied music tree or the isolated Artist Art directory. No endpoint
writes music, artwork, tags, or database rows.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass, field
from hashlib import sha256
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from io import BytesIO
import json
import os
from pathlib import Path
import secrets
import sqlite3
from threading import Lock
from urllib.parse import parse_qs, urlsplit

from PIL import Image, ImageOps

from splined_artist_art import ArtistAlbum, artist_targets, local_candidates, saved_package
from tui.artist_search import discover_artist_images


ASSETS = Path(__file__).with_name("web_preview")
IMAGE_EXTENSIONS = {".jpg", ".jpeg", ".png", ".webp"}


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


@dataclass
class PreviewApp:
    music: Path
    database: Path
    artist_art: Path
    credentials: Path
    images: dict[str, Path | bytes] = field(default_factory=dict)
    lock: Lock = field(default_factory=Lock)

    def connect(self) -> sqlite3.Connection:
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
            if not (inside(source, self.music) or inside(source, self.artist_art)):
                return None
        token = secrets.token_urlsafe(18)
        with self.lock:
            self.images[token] = source
            # Tokens are short-lived UI references. Bound memory when users
            # browse a large library for an extended session.
            if len(self.images) > 400:
                for old in list(self.images)[:100]:
                    del self.images[old]
        return "/api/image/" + token

    def artists(self, query: str) -> list[dict[str, object]]:
        with self.connect() as db:
            rows = db.execute(
                "SELECT artists.artist_key, artists.artist_name, COUNT(albums.album_key) AS album_count, artists.status "
                "FROM artists LEFT JOIN albums ON albums.artist_key=artists.artist_key "
                "WHERE artists.artist_name LIKE ? ESCAPE '\\' "
                "GROUP BY artists.artist_key ORDER BY artists.artist_name COLLATE NOCASE LIMIT 150",
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
        with self.connect() as db:
            rows = db.execute(
                "SELECT albums.album_key, albums.album_name, albums.artist_key, "
                "artists.artist_name, albums.cover_path, albums.track_count, "
                "albums.status, albums.compilation FROM albums "
                "JOIN artists ON artists.artist_key=albums.artist_key "
                "WHERE (albums.album_name LIKE ? ESCAPE '\\' OR artists.artist_name LIKE ? ESCAPE '\\') "
                "AND (?=0 OR albums.compilation=1) "
                "ORDER BY albums.album_name COLLATE NOCASE LIMIT 250",
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
            })
        return items

    def artist(self, artist_id: str) -> dict[str, object]:
        with self.connect() as db:
            artist = db.execute(
                "SELECT artist_key, artist_name, musicbrainz_artistid, album_count, status "
                "FROM artists WHERE artist_key=?", (artist_id,),
            ).fetchone()
            if artist is None:
                raise KeyError("Artist not found")
            rows = db.execute(
                "SELECT album_key, album_name, musicbrainz_albumid, path, cover_path, "
                "track_count, artwork_jpeg, artwork_png, artwork_webp, status, compilation "
                "FROM albums WHERE artist_key=? ORDER BY album_name COLLATE NOCASE",
                (artist_id,),
            ).fetchall()
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
                candidates.append({"source": "Local", "kind": label.lower(),
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
                "embedded": int(row["artwork_jpeg"] or 0) + int(row["artwork_png"] or 0) + int(row["artwork_webp"] or 0),
                "cover": self.image(cover),
            })
        return {
            "id": artist_id, "name": artist["artist_name"],
            "mbid": artist["musicbrainz_artistid"] or "",
            "status": artist["status"], "albums": albums,
            "portrait": self.image(package.portrait),
            "banner": self.image(package.banner),
            "local_candidates": candidates,
        }

    def album(self, album_id: str) -> dict[str, object]:
        with self.connect() as db:
            row = db.execute(
                "SELECT albums.album_key, albums.album_name, albums.artist_key, "
                "artists.artist_name, albums.path, albums.cover_path, albums.track_count, "
                "albums.artwork_jpeg, albums.artwork_png, albums.artwork_webp, "
                "albums.status, albums.compilation FROM albums "
                "JOIN artists ON artists.artist_key=albums.artist_key "
                "WHERE albums.album_key=?", (album_id,),
            ).fetchone()
        if row is None:
            raise KeyError("Album not found")
        folder = Path(row["path"])
        if not inside(folder, self.music):
            raise KeyError("Album is outside the test library")
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
            "embedded": int(row["artwork_jpeg"] or 0) + int(row["artwork_png"] or 0) + int(row["artwork_webp"] or 0),
            "cover": self.image(cover), "candidates": candidates,
        }

    def search_artist(self, artist_id: str) -> dict[str, object]:
        artist = self.artist(artist_id)
        found, warnings = discover_artist_images(
            str(artist["name"]), str(artist["mbid"]), self.credentials,
        )
        candidates = []
        for label, data in found:
            with Image.open(BytesIO(data)) as image:
                width, height = image.size
            candidates.append({
                "source": label.split()[0],
                "kind": "banner" if "banner" in label.casefold() else "artist",
                "name": label, "width": width, "height": height,
                "bytes": len(data), "digest": sha256(data).hexdigest(),
                "image": self.image(data),
            })
        return {"candidates": candidates, "warnings": warnings}


class Handler(BaseHTTPRequestHandler):
    app: PreviewApp

    def send_bytes(self, body: bytes, mime: str, status: int = 200) -> None:
        self.send_response(status)
        self.send_header("Content-Type", mime)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Content-Security-Policy", "default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'self'")
        self.end_headers()
        self.wfile.write(body)

    def send_json(self, value: object, status: int = 200) -> None:
        self.send_bytes(json.dumps(value).encode("utf-8"), "application/json; charset=utf-8", status)

    def do_GET(self) -> None:
        url = urlsplit(self.path)
        try:
            if url.path in {"/", "/app.css", "/app.js"}:
                name = "index.html" if url.path == "/" else url.path.lstrip("/")
                mime = "text/html; charset=utf-8" if name.endswith(".html") else (
                    "text/css; charset=utf-8" if name.endswith(".css") else "text/javascript; charset=utf-8"
                )
                self.send_bytes((ASSETS / name).read_bytes(), mime)
            elif url.path.startswith("/assets/"):
                asset = (ASSETS / url.path.lstrip("/")).resolve()
                if not asset.is_relative_to((ASSETS / "assets").resolve()) or not asset.is_file():
                    raise KeyError("Asset not found")
                mime = {".svg": "image/svg+xml", ".png": "image/png", ".jpg": "image/jpeg"}.get(asset.suffix.lower())
                if mime is None:
                    raise KeyError("Asset not found")
                self.send_bytes(asset.read_bytes(), mime)
            elif url.path == "/api/status":
                self.send_json({"mode": "read-only", "database": "isolated Python index",
                                "artists": len(self.app.artists(""))})
            elif url.path == "/api/artists":
                query = parse_qs(url.query).get("q", [""])[0][:100]
                self.send_json(self.app.artists(query))
            elif url.path == "/api/albums":
                params = parse_qs(url.query)
                query = params.get("q", [""])[0][:100]
                self.send_json(self.app.albums(query, params.get("view", [""])[0] == "fallback"))
            elif url.path == "/api/artist":
                self.send_json(self.app.artist(parse_qs(url.query).get("id", [""])[0]))
            elif url.path == "/api/album":
                self.send_json(self.app.album(parse_qs(url.query).get("id", [""])[0]))
            elif url.path.startswith("/api/image/"):
                token = url.path.removeprefix("/api/image/")
                with self.app.lock:
                    source = self.app.images.get(token)
                if source is None:
                    raise KeyError("Image preview expired")
                if isinstance(source, Path):
                    if not (inside(source, self.app.music) or inside(source, self.app.artist_art)):
                        raise KeyError("Image is outside the test library")
                    with Image.open(source) as image:
                        image = ImageOps.exif_transpose(image)
                        image.thumbnail((1400, 1400), Image.Resampling.LANCZOS)
                        output = BytesIO()
                        image.convert("RGB").save(output, format="JPEG", quality=88)
                        data = output.getvalue()
                else:
                    data = source
                self.send_bytes(data, "image/png" if isinstance(source, bytes) else "image/jpeg")
            else:
                self.send_json({"error": "Not found"}, HTTPStatus.NOT_FOUND)
        except (KeyError, FileNotFoundError) as error:
            self.send_json({"error": str(error)}, HTTPStatus.NOT_FOUND)
        except (OSError, sqlite3.Error, ValueError) as error:
            self.send_json({"error": str(error)}, HTTPStatus.INTERNAL_SERVER_ERROR)

    def do_POST(self) -> None:
        if urlsplit(self.path).path != "/api/search/artist":
            self.send_json({"error": "Not found"}, HTTPStatus.NOT_FOUND)
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if not 0 < length < 4096:
                raise ValueError("Invalid request size")
            payload = json.loads(self.rfile.read(length))
            if not isinstance(payload, dict) or not isinstance(payload.get("id"), str):
                raise ValueError("Choose an indexed artist")
            self.send_json(self.app.search_artist(payload["id"]))
        except (ValueError, KeyError, OSError, sqlite3.Error) as error:
            self.send_json({"error": str(error)}, HTTPStatus.BAD_REQUEST)

    def log_message(self, format: str, *args: object) -> None:
        print("[web-preview] " + format % args)


def main() -> None:
    base = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8080)
    parser.add_argument("--music", type=Path, default=base / "music")
    parser.add_argument("--database", type=Path, default=base / "state/cache-python-local-index-20261027/splined.db")
    parser.add_argument("--artist-art", type=Path, default=base / "state/artist-art")
    parser.add_argument("--credentials", type=Path, default=base / "state/credentials")
    args = parser.parse_args()
    if not args.music.is_dir() or not args.database.is_file():
        parser.error("The isolated music directory and Python index must exist")
    app = PreviewApp(args.music.resolve(), args.database.resolve(),
                     args.artist_art.resolve(), args.credentials.resolve())
    Handler.app = app
    with ThreadingHTTPServer((args.host, args.port), Handler) as server:
        print(f"SPLINED browser preview: http://{args.host}:{args.port}/")
        print("Read-only: no artwork, tags, or database rows can be written.")
        server.serve_forever()


if __name__ == "__main__":
    main()
