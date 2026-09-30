from __future__ import annotations

import json
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest

from splined_media_build_policy import (
    _clear_stage,
    _load_stage_payloads,
    _signature_token,
    _stage_payload_matches,
    _write_stage_batch,
)
from splined_media_database import connect, signature
from splined_tui_progress_policy import _saved_checkpoint_count


class SplinedMediaBuildPolicyTests(unittest.TestCase):
    def test_album_checkpoint_is_written_and_resumable(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            library = root / "music"
            album_path = library / "Artist" / "Album"
            album_path.mkdir(parents=True)
            track = album_path / "01.flac"
            track.write_bytes(b"representative audio")
            stat = track.stat()
            album = SimpleNamespace(path=album_path, audio_files=[track])

            database = root / "splined.db"
            connection = connect(database, "test")
            expected = signature(library, [], "Cover")
            token = _signature_token(expected)
            payload = {
                "signature": token,
                "path": str(album_path),
                "representative_file": str(track),
                "representative_size": int(stat.st_size),
                "representative_mtime_ns": int(stat.st_mtime_ns),
                "artist": {
                    "artist_key": "mbid:artist",
                    "artist_name": "Artist",
                },
                "album": {
                    "album_key": "mbid:album",
                    "artist_key": "mbid:artist",
                    "album_name": "Album",
                },
                "review": None,
            }

            _write_stage_batch(
                connection,
                [payload],
                signature_token=token,
                version="test",
                reason="initial-build",
                phase="tag-index",
                processed=1,
                total=10,
                discovered=10,
                tag_reads=1,
                tag_reuses=0,
                checkpoint_reuses=0,
            )

            loaded = _load_stage_payloads(connection, token)
            self.assertIn(str(album_path), loaded)
            self.assertTrue(
                _stage_payload_matches(
                    loaded[str(album_path)],
                    token,
                    album,
                )
            )
            self.assertEqual(_saved_checkpoint_count(str(database)), 1)
            self.assertEqual(
                _saved_checkpoint_count(str(root / "missing.db")),
                0,
            )

            row = connection.execute(
                "SELECT details_json FROM db_maintenance_state "
                "WHERE action_name='media-index-build-progress'"
            ).fetchone()
            self.assertIsNotNone(row)
            details = json.loads(str(row["details_json"]))
            self.assertEqual(details["processed"], 1)
            self.assertEqual(details["total"], 10)

            track.write_bytes(b"changed representative audio")
            self.assertFalse(
                _stage_payload_matches(
                    loaded[str(album_path)],
                    token,
                    album,
                )
            )

            _clear_stage(connection)
            self.assertEqual(
                connection.execute(
                    "SELECT COUNT(*) FROM cache_entries "
                    "WHERE cache_type='media-index-stage'"
                ).fetchone()[0],
                0,
            )
            connection.close()


if __name__ == "__main__":
    unittest.main()
