from __future__ import annotations

from io import BytesIO
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest
from unittest import mock

from PIL import Image

from splined_media_database import connect
from splined_artist_art import ArtistPackage
from interaction.artist_workspace import ArtistWorkspace


ARTIST_ID = "e0bba708-bdd3-478d-84ea-c706413bedab"
ALBUM_ID = "cf1da014-7b14-4ace-b231-271013d04625"


def _png(color: str) -> bytes:
    output = BytesIO()
    Image.new("RGB", (80, 60), color).save(output, format="PNG")
    return output.getvalue()


class ArtistWorkspaceTests(unittest.TestCase):
    def test_local_package_preview_and_approved_component_write(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            cache = root / "cache"
            cache.mkdir()
            album = root / "music" / "Air" / "Moon Safari"
            album.mkdir(parents=True)
            (album.parent / "artist.jpg").write_bytes(_png("red"))
            database = cache / "splined.db"
            connection = connect(database, "test")
            with connection:
                connection.execute(
                    "INSERT INTO artists(artist_key, artist_name, primary_path, "
                    "musicbrainz_artistid, created_at, updated_at, last_seen_at, splined_version) "
                    "VALUES(?, ?, ?, ?, 'now', 'now', 'now', 'test')",
                    ("air", "Air", str(album.parent), ARTIST_ID),
                )
                connection.execute(
                    "INSERT INTO albums(album_key, artist_key, album_name, path, "
                    "musicbrainz_albumid, tag_signature, created_at, updated_at, last_seen_at, splined_version) "
                    "VALUES(?, ?, ?, ?, ?, 'tag', 'now', 'now', 'now', 'test')",
                    ("air-moon", "air", "Moon Safari", str(album), ALBUM_ID),
                )
            connection.close()
            session = ArtistWorkspace.load("Air", database, root / "managed", cache, writable=True)
            try:
                self.assertEqual(session.artist_mbid, ARTIST_ID)
                self.assertTrue((session.preview_dir / "current.png").is_file())
                self.assertEqual(session.selected_component, "portrait")
                candidate = root / "blue.png"
                candidate.write_bytes(_png("blue"))
                session.add_local_candidate(candidate)
                session.choose_candidate("portrait")
                session.adjust(10)
                self.assertEqual(session.portrait_adjustments.picture, 10)
                session.select_radios(False, True)
                self.assertEqual(session.portrait_adjustments.picture, 10)
                session.choose_candidate("banner")
                session.select_radios(True, True)
                with self.assertRaisesRegex(ValueError, "Themed Packaging"):
                    session.adjust(5)
                session.theme_enabled = True
                session.adjust(5)
                self.assertTrue((session.preview_dir / "package.png").is_file())
                self.assertEqual(session.write_approved(), [])
                self.assertFalse((album.parent / "banner.png").exists())
                written = session.write_approved()
                self.assertEqual({path.name for path in written}, {"artist.png", "banner.png"})
                with Image.open(album.parent / "artist.png") as image:
                    self.assertEqual(image.size, (60, 60))
                    self.assertEqual(image.getpixel((0, 0))[3], 255)
                self.assertEqual(session.saved.portrait, album.parent / "artist.png")
                self.assertEqual(session.saved.banner, album.parent / "banner.png")
            finally:
                preview = session.preview_dir
                session.close()
                self.assertFalse(preview.exists())




if __name__ == "__main__":
    unittest.main()
