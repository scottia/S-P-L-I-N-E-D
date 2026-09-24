"""Crash-safe persistent folder snapshot for the Select Media picker.

The SQLite file is disposable acceleration state. It stores only folder
topology, local-art filenames, and the small fingerprint needed to reconcile
timeout history without touching the media filesystem during a warm start.
History, bypass, tags, providers, ranking, and writes remain authoritative in
the existing SPLINED engine.
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
PICKER_SCHEMA_VERSION = 2


def ignored_signature(patterns: Iterable[str]) -> str:
    normalized = [str(value).strip().casefold() for value in patterns if str(value).strip()]
    body = json.dumps(normalized, ensure_ascii=False, separators=(",", ":"))
    return hashlib.sha256(body.encode("utf-8")).hexdigest()


def should_ignore(name: str, patterns: Iterable[str]) -> bool:
    """Apply built-in POSIX-hidden exclusion plus configured wildcard rules."""
    if name.startswith("."):
        return True
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
    # Kept for the presentation/model API. Every artist exposed from a
    # complete snapshot is indexed and loaded.
    indexed: bool = True
    last_indexed: float | None = None


@dataclass(frozen=True)
class PickerAlbum:
    path: str
    artist_path: str
    name: str
    local_art: tuple[str, ...] = ()
    inventory_fingerprint: str | None = None


@dataclass(frozen=True)
class SnapshotInfo:
    generation: int
    built_at: float
    artist_count: int
    album_count: int


class PickerIndex:
    """Transactional complete-snapshot cache with disposable recovery.

    A generation is inserted and promoted in one SQLite transaction. The old
    active generation therefore remains readable if discovery fails or the
    process stops before commit; incomplete topology is never advertised as a
    complete library.
    """

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
        connection: sqlite3.Connection | None = None
        try:
            connection = sqlite3.connect(path, timeout=10)
            instance = cls(path, library_root, patterns, connection)
            instance._initialize()
            return instance
        except (sqlite3.DatabaseError, OSError):
            if connection is not None:
                try:
                    connection.close()
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
        existing_version: str | None = None
        try:
            exists = self.connection.execute(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='meta'"
            ).fetchone()
            if exists:
                row = self.connection.execute(
                    "SELECT value FROM meta WHERE key='schema_version'"
                ).fetchone()
                existing_version = str(row[0]) if row else None
        except sqlite3.DatabaseError:
            existing_version = None

        if existing_version not in {None, str(PICKER_SCHEMA_VERSION)}:
            # This cache is disposable. A schema change rebuilds it without
            # touching operational history.
            with self.connection:
                self.connection.execute("DROP TABLE IF EXISTS albums")
                self.connection.execute("DROP TABLE IF EXISTS artists")
                self.connection.execute("DROP TABLE IF EXISTS meta")

        self.connection.executescript(
            """
            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS artists (
                generation INTEGER NOT NULL,
                artist_path TEXT NOT NULL,
                artist_name TEXT NOT NULL,
                last_indexed REAL NOT NULL,
                PRIMARY KEY(generation, artist_path)
            );
            CREATE TABLE IF NOT EXISTS albums (
                generation INTEGER NOT NULL,
                album_path TEXT NOT NULL,
                artist_path TEXT NOT NULL,
                album_name TEXT NOT NULL,
                local_art_json TEXT NOT NULL DEFAULT '[]',
                inventory_fingerprint TEXT,
                PRIMARY KEY(generation, album_path),
                FOREIGN KEY(generation, artist_path)
                    REFERENCES artists(generation, artist_path) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS albums_artist_path
                ON albums(generation, artist_path, album_name);
            """
        )
        with self.connection:
            self._set_meta("schema_version", str(PICKER_SCHEMA_VERSION))

    def _set_meta(self, key: str, value: str) -> None:
        self.connection.execute(
            "INSERT INTO meta(key, value) VALUES(?, ?) "
            "ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            (key, value),
        )

    def _meta(self) -> dict[str, str]:
        return {
            str(key): str(value)
            for key, value in self.connection.execute("SELECT key, value FROM meta")
        }

    def snapshot_info(self) -> SnapshotInfo | None:
        values = self._meta()
        if values.get("snapshot_complete") != "1":
            return None
        if values.get("library_root") != str(self.library_root):
            return None
        if values.get("ignored_subs_signature") != ignored_signature(self.ignored_subs):
            return None
        try:
            generation = int(values["active_generation"])
            built_at = float(values["built_at"])
            artist_count = int(values["artist_count"])
            album_count = int(values["album_count"])
        except (KeyError, TypeError, ValueError):
            return None
        actual_artists = int(
            self.connection.execute(
                "SELECT COUNT(*) FROM artists WHERE generation=?", (generation,)
            ).fetchone()[0]
        )
        actual_albums = int(
            self.connection.execute(
                "SELECT COUNT(*) FROM albums WHERE generation=?", (generation,)
            ).fetchone()[0]
        )
        if actual_artists != artist_count or actual_albums != album_count:
            return None
        return SnapshotInfo(generation, built_at, artist_count, album_count)

    @property
    def snapshot_complete(self) -> bool:
        return self.snapshot_info() is not None

    def close(self) -> None:
        self.connection.close()

    def __enter__(self) -> "PickerIndex":
        return self

    def __exit__(self, _exc_type, _exc, _traceback) -> None:
        self.close()

    def discover_artists(self) -> list[PickerArtist]:
        """Enumerate only immediate non-hidden/non-excluded Artist folders."""
        try:
            with os.scandir(self.library_root) as iterator:
                entries = sorted(iterator, key=lambda item: item.name.casefold())
        except OSError as exc:
            raise OSError(f"Unable to read SPLINED library root {self.library_root}: {exc}") from exc
        now = time.time()
        artists: list[PickerArtist] = []
        for entry in entries:
            if entry.is_symlink() or should_ignore(entry.name, self.ignored_subs):
                continue
            try:
                if not entry.is_dir(follow_symlinks=False):
                    continue
            except OSError:
                continue
            artists.append(PickerArtist(str(Path(entry.path)), entry.name, True, now))
        return artists

    def _active_generation(self) -> int | None:
        info = self.snapshot_info()
        return info.generation if info is not None else None

    def artists(self) -> list[PickerArtist]:
        generation = self._active_generation()
        if generation is None:
            return []
        rows = self.connection.execute(
            "SELECT artist_path, artist_name, last_indexed FROM artists "
            "WHERE generation=? "
            "ORDER BY artist_name COLLATE NOCASE, artist_path COLLATE NOCASE",
            (generation,),
        )
        return [
            PickerArtist(str(path), str(name), True, float(last_indexed))
            for path, name, last_indexed in rows
        ]

    def albums(self, artist_path: str | None = None) -> list[PickerAlbum]:
        generation = self._active_generation()
        if generation is None:
            return []
        if artist_path is None:
            rows = self.connection.execute(
                "SELECT album_path, artist_path, album_name, local_art_json, inventory_fingerprint "
                "FROM albums WHERE generation=? "
                "ORDER BY artist_path COLLATE NOCASE, album_path COLLATE NOCASE",
                (generation,),
            )
        else:
            rows = self.connection.execute(
                "SELECT album_path, artist_path, album_name, local_art_json, inventory_fingerprint "
                "FROM albums WHERE generation=? AND artist_path=? "
                "ORDER BY album_path COLLATE NOCASE",
                (generation, artist_path),
            )
        result: list[PickerAlbum] = []
        for album_path, parent, name, local_art_json, fingerprint in rows:
            try:
                decoded = json.loads(str(local_art_json))
            except (TypeError, ValueError):
                decoded = []
            local_art = (
                tuple(str(value) for value in decoded if str(value))
                if isinstance(decoded, list)
                else ()
            )
            result.append(
                PickerAlbum(
                    str(album_path),
                    str(parent),
                    str(name),
                    local_art,
                    str(fingerprint) if fingerprint else None,
                )
            )
        return result

    def promote_snapshot(
        self,
        artists: Iterable[PickerArtist],
        albums: Iterable[PickerAlbum],
        *,
        built_at: float | None = None,
    ) -> SnapshotInfo:
        """Atomically install a complete generation and retire older rows."""
        artist_rows = list(artists)
        album_rows = list(albums)
        generation = time.time_ns()
        completed_at = time.time() if built_at is None else float(built_at)
        with self.connection:
            self.connection.executemany(
                "INSERT INTO artists(generation, artist_path, artist_name, last_indexed) "
                "VALUES(?, ?, ?, ?)",
                [
                    (
                        generation,
                        artist.path,
                        artist.name,
                        artist.last_indexed or completed_at,
                    )
                    for artist in artist_rows
                ],
            )
            self.connection.executemany(
                "INSERT INTO albums(generation, album_path, artist_path, album_name, "
                "local_art_json, inventory_fingerprint) VALUES(?, ?, ?, ?, ?, ?)",
                [
                    (
                        generation,
                        album.path,
                        album.artist_path,
                        album.name,
                        json.dumps(
                            list(album.local_art),
                            ensure_ascii=False,
                            separators=(",", ":"),
                        ),
                        album.inventory_fingerprint,
                    )
                    for album in album_rows
                ],
            )
            metadata = {
                "library_root": str(self.library_root),
                "ignored_subs_signature": ignored_signature(self.ignored_subs),
                "active_generation": str(generation),
                "snapshot_complete": "1",
                "built_at": str(completed_at),
                "artist_count": str(len(artist_rows)),
                "album_count": str(len(album_rows)),
                "updated_at": str(completed_at),
            }
            for key, value in metadata.items():
                self._set_meta(key, value)
            self.connection.execute(
                "DELETE FROM albums WHERE generation<>?", (generation,)
            )
            self.connection.execute(
                "DELETE FROM artists WHERE generation<>?", (generation,)
            )
        return SnapshotInfo(generation, completed_at, len(artist_rows), len(album_rows))

    def clear_inventory(self, *, commit: bool = True) -> None:
        """Remove disposable topology without affecting any SPLINED authority."""

        def write() -> None:
            self.connection.execute("DELETE FROM albums")
            self.connection.execute("DELETE FROM artists")
            for key in (
                "active_generation",
                "snapshot_complete",
                "built_at",
                "artist_count",
                "album_count",
                "library_root",
                "ignored_subs_signature",
            ):
                self.connection.execute("DELETE FROM meta WHERE key=?", (key,))

        if commit:
            with self.connection:
                write()
        else:
            write()
