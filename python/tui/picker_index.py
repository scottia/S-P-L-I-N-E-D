"""Disposable persistent folder index for the Select Media picker.

This database accelerates presentation only.  It stores folder topology and
local-art filenames; completion, bypass, timeout, tags, and processing policy
remain owned by the existing SPLINED authorities.
"""

from __future__ import annotations

from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import re
import sqlite3
import time
from typing import Iterable


PICKER_DB_NAME = "splined-picker.sqlite3"
PICKER_SCHEMA_VERSION = 1


def ignored_signature(patterns: Iterable[str]) -> str:
    normalized = [str(value).strip().casefold() for value in patterns if str(value).strip()]
    body = json.dumps(normalized, ensure_ascii=False, separators=(",", ":"))
    return hashlib.sha256(body.encode("utf-8")).hexdigest()


def should_ignore(name: str, patterns: Iterable[str]) -> bool:
    """Match only ``*`` and ``?`` as wildcards; square brackets stay literal."""
    for pattern in patterns:
        value = str(pattern).strip()
        if not value:
            continue
        expression = "^" + re.escape(value).replace(r"\*", ".*").replace(r"\?", ".") + "$"
        if re.fullmatch(expression, name, flags=re.IGNORECASE):
            return True
    return False


@dataclass(frozen=True)
class PickerArtist:
    path: str
    name: str
    indexed: bool
    last_indexed: float | None = None


@dataclass(frozen=True)
class PickerAlbum:
    path: str
    artist_path: str
    name: str
    local_art: tuple[str, ...] = ()


class PickerIndex:
    """Small transactional SQLite cache with resilient disposable recovery."""

    def __init__(
        self,
        path: Path,
        library_root: Path,
        ignored_subs: Iterable[str],
        connection: sqlite3.Connection,
        *,
        recovered: bool = False,
    ) -> None:
        self.path = path
        self.library_root = library_root
        self.ignored_subs = tuple(str(value) for value in ignored_subs)
        self.connection = connection
        self.recovered = recovered

    @classmethod
    def open(
        cls,
        path: Path,
        library_root: Path,
        ignored_subs: Iterable[str],
    ) -> "PickerIndex":
        path.parent.mkdir(parents=True, exist_ok=True)
        patterns = tuple(str(value) for value in ignored_subs)
        try:
            connection = sqlite3.connect(path, timeout=10)
            instance = cls(path, library_root, patterns, connection)
            instance._initialize()
            return instance
        except (sqlite3.DatabaseError, OSError):
            try:
                connection.close()  # type: ignore[possibly-undefined]
            except Exception:
                pass
            cls._quarantine(path)
            connection = sqlite3.connect(path, timeout=10)
            instance = cls(path, library_root, patterns, connection, recovered=True)
            instance._initialize()
            return instance

    @staticmethod
    def _quarantine(path: Path) -> None:
        if not path.exists():
            return
        suffix = time.strftime("%Y%m%d-%H%M%S")
        target = path.with_name(f"{path.name}.corrupt-{suffix}")
        counter = 1
        while target.exists():
            target = path.with_name(f"{path.name}.corrupt-{suffix}-{counter}")
            counter += 1
        try:
            path.replace(target)
        except OSError:
            path.unlink(missing_ok=True)

    def _initialize(self) -> None:
        self.connection.execute("PRAGMA foreign_keys = ON")
        self.connection.execute("PRAGMA journal_mode = WAL")
        self.connection.executescript(
            """
            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS artists (
                artist_path TEXT PRIMARY KEY,
                artist_name TEXT NOT NULL,
                indexed INTEGER NOT NULL DEFAULT 0,
                last_indexed REAL
            );
            CREATE TABLE IF NOT EXISTS albums (
                album_path TEXT PRIMARY KEY,
                artist_path TEXT NOT NULL REFERENCES artists(artist_path) ON DELETE CASCADE,
                album_name TEXT NOT NULL,
                local_art_json TEXT NOT NULL DEFAULT '[]'
            );
            CREATE INDEX IF NOT EXISTS albums_artist_path ON albums(artist_path, album_name);
            """
        )
        values = dict(self.connection.execute("SELECT key, value FROM meta"))
        expected = {
            "schema_version": str(PICKER_SCHEMA_VERSION),
            "library_root": str(self.library_root),
            "ignored_subs_signature": ignored_signature(self.ignored_subs),
        }
        invalid = any(values.get(key) not in {None, value} for key, value in expected.items())
        with self.connection:
            if invalid:
                self.connection.execute("DELETE FROM albums")
                self.connection.execute("DELETE FROM artists")
                self.connection.execute("DELETE FROM meta")
            for key, value in expected.items():
                self.connection.execute(
                    "INSERT INTO meta(key, value) VALUES(?, ?) "
                    "ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    (key, value),
                )
            self.connection.execute(
                "INSERT INTO meta(key, value) VALUES('updated_at', ?) "
                "ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                (str(time.time()),),
            )

    def close(self) -> None:
        self.connection.close()

    def __enter__(self) -> "PickerIndex":
        return self

    def __exit__(self, _exc_type, _exc, _traceback) -> None:
        self.close()

    def reconcile_root(self) -> list[PickerArtist]:
        """Read only the library root and reconcile its immediate Artist dirs."""
        try:
            with os.scandir(self.library_root) as iterator:
                entries = sorted(iterator, key=lambda item: item.name.casefold())
        except OSError as exc:
            raise OSError(f"Unable to read SPLINED library root {self.library_root}: {exc}") from exc

        roots: list[tuple[str, str]] = []
        for entry in entries:
            if entry.is_symlink() or should_ignore(entry.name, self.ignored_subs):
                continue
            try:
                if not entry.is_dir(follow_symlinks=False):
                    continue
            except OSError:
                continue
            roots.append((str(Path(entry.path)), entry.name))

        paths = {path for path, _name in roots}
        with self.connection:
            existing = {
                str(row[0])
                for row in self.connection.execute("SELECT artist_path FROM artists")
            }
            for stale in existing - paths:
                self.connection.execute(
                    "DELETE FROM artists WHERE artist_path = ?", (stale,)
                )
            for artist_path, artist_name in roots:
                self.connection.execute(
                    "INSERT INTO artists(artist_path, artist_name, indexed) VALUES(?, ?, 0) "
                    "ON CONFLICT(artist_path) DO UPDATE SET artist_name=excluded.artist_name",
                    (artist_path, artist_name),
                )
        return self.artists()

    def artists(self) -> list[PickerArtist]:
        rows = self.connection.execute(
            "SELECT artist_path, artist_name, indexed, last_indexed "
            "FROM artists ORDER BY artist_name COLLATE NOCASE, artist_path COLLATE NOCASE"
        )
        return [
            PickerArtist(str(path), str(name), bool(indexed), last_indexed)
            for path, name, indexed, last_indexed in rows
        ]

    def albums(self, artist_path: str | None = None) -> list[PickerAlbum]:
        if artist_path is None:
            rows = self.connection.execute(
                "SELECT album_path, artist_path, album_name, local_art_json "
                "FROM albums ORDER BY artist_path COLLATE NOCASE, album_path COLLATE NOCASE"
            )
        else:
            rows = self.connection.execute(
                "SELECT album_path, artist_path, album_name, local_art_json "
                "FROM albums WHERE artist_path = ? ORDER BY album_path COLLATE NOCASE",
                (artist_path,),
            )
        result: list[PickerAlbum] = []
        for album_path, parent, name, local_art_json in rows:
            try:
                decoded = json.loads(str(local_art_json))
            except (TypeError, ValueError):
                decoded = []
            local_art = tuple(str(value) for value in decoded if str(value)) if isinstance(decoded, list) else ()
            result.append(PickerAlbum(str(album_path), str(parent), str(name), local_art))
        return result

    def replace_artist_albums(
        self,
        artist: PickerArtist,
        albums: Iterable[PickerAlbum],
        *,
        commit: bool = True,
    ) -> None:
        rows = list(albums)
        now = time.time()

        def write() -> None:
            self.connection.execute(
                "DELETE FROM albums WHERE artist_path = ?", (artist.path,)
            )
            self.connection.executemany(
                "INSERT INTO albums(album_path, artist_path, album_name, local_art_json) "
                "VALUES(?, ?, ?, ?)",
                [
                    (
                        album.path,
                        artist.path,
                        album.name,
                        json.dumps(list(album.local_art), ensure_ascii=False, separators=(",", ":")),
                    )
                    for album in rows
                ],
            )
            self.connection.execute(
                "UPDATE artists SET indexed=1, last_indexed=? WHERE artist_path=?",
                (now, artist.path),
            )
            self.connection.execute(
                "INSERT INTO meta(key, value) VALUES('updated_at', ?) "
                "ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                (str(now),),
            )

        if commit:
            with self.connection:
                write()
        else:
            write()

    def clear_inventory(self, *, commit: bool = True) -> None:
        def write() -> None:
            self.connection.execute("DELETE FROM albums")
            self.connection.execute("UPDATE artists SET indexed=0, last_indexed=NULL")

        if commit:
            with self.connection:
                write()
        else:
            write()
