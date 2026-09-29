from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest

from splined_media_build_policy import (
    _clear_stage,
    _signature_token,
    _write_stage_batch,
)
from splined_media_database import connect, signature, utc_now
from splined_media_finalize_policy import (
    _picker_folder_count,
    _promote_initial_snapshot,
    _stage_count,
    validate_snapshot,
)


class SplinedMediaFinalizePolicyTests(unittest.TestCase):
    def test_picker_folder_count_collapses_shared_authority_path(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            connection = connect(Path(directory) / "splined.db", "test")
            now = utc_now()
            shared_path = str(Path(directory) / "music" / "Artist")
            with connection:
                connection.executemany(
                    "INSERT INTO artists"
                    "(artist_key, artist_name, artist_sort, "
                    "musicbrainz_artistid, primary_path, status, album_count, "
                    "unprocessed_count, processed_count, bypassed_count, "
                    "timeout_count, created_at, updated_at, last_seen_at, "
                    "splined_version) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    [
                        (
                            f"mbid:artist-{index}",
                            f"Authority Artist {index}",
                            f"Authority Artist {index}",
                            f"artist-{index}",
                            shared_path,
                            "unprocessed",
                            0,
                            0,
                            0,
                            0,
                            0,
                            now,
                            now,
                            now,
                            "test",
                        )
                        for index in range(2)
                    ],
                )
            self.assertEqual(_picker_folder_count(connection), 1)
            connection.close()

    def test_marker_last_promotion_keeps_checkpoints_until_validation(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            library = root / "music"
            album_path = library / "Artist" / "Album"
            album_path.mkdir(parents=True)
            track = album_path / "01.flac"
            track.write_bytes(b"representative audio")
            stat = track.stat()

            db_path = root / "splined.db"
            connection = connect(db_path, "test")
            expected = signature(library, [], "Cover")
            token = _signature_token(expected)
            now = utc_now()

            artist = {
                "artist_key": "mbid:artist",
                "artist_name": "Artist",
                "artist_sort": "Artist",
                "musicbrainz_artistid": "artist",
                "primary_path": str(library / "Artist"),
                "status": "unprocessed",
                "created_at": now,
                "updated_at": now,
                "last_seen_at": now,
            }
            album = {
                "album_key": "mbid:album",
                "artist_key": "mbid:artist",
                "album_name": "Album",
                "album_sort": "Album",
                "musicbrainz_albumid": "album",
                "musicbrainz_releasegroupid": "group",
                "release_year": "2026",
                "compilation": 0,
                "path": str(album_path),
                "representative_file": str(track),
                "representative_size": int(stat.st_size),
                "representative_mtime_ns": int(stat.st_mtime_ns),
                "tag_signature": "tag-signature",
                "track_count": 1,
                "inventory_fingerprint": None,
                "status": "unprocessed",
                "cover_required": 1,
                "cover_found": 0,
                "cover_path": "",
                "cover_name": "",
                "cover_format": "",
                "cover_width": None,
                "cover_height": None,
                "artwork_jpeg": 0,
                "artwork_png": 0,
                "artwork_webp": 0,
                "artwork_other": 0,
                "root_files": 0,
                "cover_files": 0,
                "cover_names_json": "[]",
                "local_art_json": "[]",
                "other_filenames_json": "[]",
                "webp_found": 0,
                "webp_size_mb": 0.0,
                "webp_resolution": "",
                "webp_conversion": 0,
                "processed_at": "",
                "bypassed": 0,
                "timeout_until": "",
                "selected_source": "",
                "created_at": now,
                "updated_at": now,
                "last_seen_at": now,
            }
            payload = {
                "signature": token,
                "path": str(album_path),
                "representative_file": str(track),
                "representative_size": int(stat.st_size),
                "representative_mtime_ns": int(stat.st_mtime_ns),
                "artist": artist,
                "album": album,
                "review": None,
            }
            _write_stage_batch(
                connection,
                [payload],
                signature_token=token,
                version="test",
                reason="initial-build",
                phase="tag-index-complete",
                processed=1,
                total=1,
                discovered=1,
                tag_reads=1,
                tag_reuses=0,
                checkpoint_reuses=0,
            )

            events: list[tuple[str, dict]] = []
            core = SimpleNamespace(
                emit_ui=lambda event, **data: events.append((event, data)),
            )
            _promote_initial_snapshot(
                core,
                connection,
                {artist["artist_key"]: artist},
                [album],
                [],
                expected,
                "test",
                "initial-build",
            )

            counts = validate_snapshot(
                connection,
                expected_artists=1,
                expected_albums=1,
                expected_signature=expected,
            )
            self.assertEqual(counts, {"artists": 1, "albums": 1})
            self.assertEqual(_stage_count(connection), 1)
            self.assertTrue(
                any(
                    event == "cache_progress"
                    and "Publishing validated" in data.get("status", "")
                    for event, data in events
                )
            )

            _clear_stage(connection)
            self.assertEqual(_stage_count(connection), 0)
            connection.close()


if __name__ == "__main__":
    unittest.main()
