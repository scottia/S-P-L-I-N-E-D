from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from webgui.coordination import (
    catalog_relative_album_path,
    canonical_album_identity,
    canonical_track_identity,
    new_event,
    new_track_event,
)


class CoordinationIdentityTests(unittest.TestCase):
    def test_catalog_relative_path_is_lexical_and_rejects_traversal(self):
        root = Path.cwd() / "catalog-root"
        self.assertEqual(
            catalog_relative_album_path(
                root / "Artist" / "Album", root
            ),
            "artist/album",
        )
        self.assertEqual(
            catalog_relative_album_path(
                root / "Artist" / ".." / "Outside", root
            ),
            "",
        )
        self.assertEqual(
            catalog_relative_album_path(root.parent / "other" / "Album", root),
            "",
        )

    def test_album_event_strips_bom_from_persisted_key_and_mbid(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            album = root / "Artist" / "Album"
            album.mkdir(parents=True)
            event = new_event(
                {
                    "album_key": "mbid:\ufeffABC-123",
                    "musicbrainz_albumid": "\u200bABC-123",
                    "path": str(album),
                },
                root,
                "webgui",
                "write-cover",
            )
        self.assertEqual(event["album_key"], "mbid:abc-123")
        self.assertEqual(event["album_identity"], "key:mbid:abc-123")

    def test_track_identity_is_album_and_relative_path_scoped(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            track = root / "Various Artists" / "COMP" / "One.flac"
            track.parent.mkdir(parents=True)
            track.touch()
            event = new_track_event(
                {"album_key": "tag:COMP"},
                {"track_key": "one", "path": str(track)},
                root,
                "webgui",
                "embed-comp-track",
                "abc123",
            )
        self.assertEqual(
            event["relative_track_path"],
            "various artists/comp/one.flac",
        )
        self.assertEqual(
            event["track_identity"],
            canonical_track_identity(
                "tag:comp", "various artists/comp/one.flac"
            ),
        )
        self.assertEqual(canonical_album_identity("tag:\ufeffCOMP"), "key:tag:comp")

    def test_migration_020_leaves_reserved_018_019_unused(self):
        migrations = Path(__file__).parents[1] / "migrations"
        names = {path.name for path in migrations.glob("*.sql")}
        self.assertIn("020_comp_track_write_coordination.sql", names)
        self.assertFalse(any(name.startswith(("018_", "019_")) for name in names))


if __name__ == "__main__":
    unittest.main()
