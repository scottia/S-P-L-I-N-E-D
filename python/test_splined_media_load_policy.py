from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest

from splined_media_database import connect, utc_now
from splined_media_load_policy import project_statuses_delta


class _AlbumDir:
    def __init__(
        self,
        path: Path,
        audio_files: list[Path],
        local_art_files: list[Path],
        inventory_fingerprint: str | None,
    ) -> None:
        self.path = path
        self.audio_files = audio_files
        self.local_art_files = local_art_files
        self.inventory_fingerprint = inventory_fingerprint


class SplinedMediaLoadPolicyTests(unittest.TestCase):
    def test_unchanged_warm_load_rewrites_no_album_or_artist_rows(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            db_path = Path(directory) / "splined.db"
            connection = connect(db_path, "test")
            now = utc_now()
            artist_key = "mbid:artist"
            artist_path = "/music/Artist"
            first_path = "/music/Artist/First"
            second_path = "/music/Artist/Second"

            with connection:
                connection.execute(
                    "INSERT INTO artists"
                    "(artist_key, artist_name, artist_sort, primary_path, "
                    "status, created_at, updated_at, last_seen_at, "
                    "splined_version) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    (
                        artist_key,
                        "Artist",
                        "Artist",
                        artist_path,
                        "unprocessed",
                        now,
                        now,
                        now,
                        "test",
                    ),
                )
                connection.executemany(
                    "INSERT INTO albums"
                    "(album_key, artist_key, album_name, album_sort, path, "
                    "tag_signature, status, cover_found, local_art_json, "
                    "processed_at, selected_source, "
                    "created_at, updated_at, last_seen_at, splined_version) "
                    "VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    [
                        (
                            "mbid:first",
                            artist_key,
                            "First",
                            "First",
                            first_path,
                            "first-tags",
                            "processed",
                            1,
                            '["/music/Artist/First/Cover.jpeg"]',
                            now,
                            "itunes",
                            now,
                            now,
                            now,
                            "test",
                        ),
                        (
                            "mbid:second",
                            artist_key,
                            "Second",
                            "Second",
                            second_path,
                            "second-tags",
                            "unprocessed",
                            0,
                            "[]",
                            None,
                            None,
                            now,
                            now,
                            now,
                            "test",
                        ),
                    ],
                )

            events: list[tuple[str, dict]] = []
            core = SimpleNamespace(
                AlbumDir=_AlbumDir,
                scan_policy_fingerprint=lambda _cfg, _sources: "policy",
                scan_completion_status=lambda *_args, **_kwargs: (False, 0.0),
                emit_ui=lambda event, **payload: events.append((event, payload)),
                debug_log=lambda _message: None,
            )
            context = SimpleNamespace(
                core=core,
                completion_history={"albums": {}},
                cfg={},
                sources=[],
                timeout_hours=0.0,
                bypassed_paths=set(),
            )

            first = project_statuses_delta(context, connection)
            self.assertEqual(first["album_updates"], 0)
            self.assertEqual(first["artist_updates"], 1)

            second = project_statuses_delta(context, connection)
            self.assertEqual(second["album_updates"], 0)
            self.assertEqual(second["artist_updates"], 0)

            context.bypassed_paths.add(second_path)
            third = project_statuses_delta(context, connection)
            self.assertEqual(third["album_updates"], 1)
            self.assertEqual(third["artist_updates"], 1)
            row = connection.execute(
                "SELECT status, bypassed FROM albums WHERE album_key=?",
                ("mbid:second",),
            ).fetchone()
            self.assertEqual(str(row["status"]), "bypassed")
            self.assertEqual(int(row["bypassed"]), 1)
            self.assertTrue(
                any(
                    event == "cache_progress"
                    and payload.get("phase") == "status"
                    for event, payload in events
                )
            )
            connection.close()


if __name__ == "__main__":
    unittest.main()
