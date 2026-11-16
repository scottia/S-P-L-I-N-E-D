from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from webgui.catalog import Catalog


class PathAlbumGroupingTests(unittest.TestCase):
    def test_curated_child_overrides_various_artist_without_duplicates(self):
        with TemporaryDirectory() as directory:
            music = Path(directory) / "music"
            various = music / "Various Artists"
            soundtracks = music / "Soundtracks"
            curated = various / "Billboard"
            for path in (curated / "Volume 1", soundtracks / "Film", various / "Collection"):
                path.mkdir(parents=True, exist_ok=True)
            catalog = Catalog(
                music,
                Path(directory) / "unused.db",
                Path(directory),
                Path(directory),
                various_artist_path=various,
                soundtracks_path=soundtracks,
                curated_compilations_path=curated,
            )

            rows = [
                ({"path": str(curated / "Volume 1"), "artist_name": "Various Artists", "compilation": True}, "fallback"),
                ({"path": str(soundtracks / "Film"), "artist_name": "Soundtracks", "compilation": True}, "soundtracks"),
                ({"path": str(various / "Collection"), "artist_name": "Various Artists", "compilation": True}, "various-artists"),
                ({"path": str(music / "Artist" / "Release"), "artist_name": "Artist", "compilation": False}, "artist"),
            ]
            for row, expected in rows:
                self.assertEqual(catalog.album_library_group(row), expected)

    def test_shared_music_root_is_an_inherited_default_not_three_claims(self):
        with TemporaryDirectory() as directory:
            music = Path(directory) / "music"
            album = music / "Various Artists" / "Collection"
            album.mkdir(parents=True)
            catalog = Catalog(
                music,
                Path(directory) / "unused.db",
                Path(directory),
                Path(directory),
                various_artist_path=music,
                soundtracks_path=music,
                curated_compilations_path=music,
            )
            self.assertEqual(
                catalog.album_library_group({
                    "path": str(album),
                    "artist_name": "Various Artists",
                    "compilation": True,
                }),
                "various-artists",
            )

    def test_single_roots_match_imported_windows_album_paths(self):
        with TemporaryDirectory() as directory:
            music = Path(directory) / "music"
            various = music / "[Various Artists]"
            soundtracks = music / "[Soundtracks]"
            curated = various / "Billboard"
            for path in (various, soundtracks, curated):
                path.mkdir(parents=True, exist_ok=True)
            catalog = Catalog(
                music,
                Path(directory) / "unused.db",
                Path(directory),
                Path(directory),
                various_artist_path=various,
                soundtracks_path=soundtracks,
                curated_compilations_path=curated,
            )

            with patch.object(
                Path, "resolve", side_effect=AssertionError("filesystem access")
            ):
                self.assertEqual(catalog.album_library_group({
                    "path": r"K:\Music\[Soundtracks]\2 Fast 2 Furious",
                    "artist_name": "Various Artists",
                    "compilation": True,
                }), "soundtracks")
                self.assertEqual(catalog.album_library_group({
                    "path": r"K:\Music\[Various Artists]\Billboard\Hot 100",
                    "artist_name": "Various Artists",
                    "compilation": True,
                }), "fallback")
                self.assertEqual(catalog.album_library_group({
                    "path": r"K:\Music\[Various Artists]\Collection",
                    "artist_name": "Various Artists",
                    "compilation": True,
                }), "various-artists")

    def test_root_owned_va_and_soundtrack_albums_do_not_become_artist_cards(self):
        with TemporaryDirectory() as directory:
            music = Path(directory) / "music"
            various = music / "[Various Artists]"
            soundtracks = music / "[Soundtracks]"
            curated = various / "Billboard"
            for path in (various, soundtracks, curated):
                path.mkdir(parents=True, exist_ok=True)
            catalog = Catalog(
                music,
                Path(directory) / "unused.db",
                Path(directory),
                Path(directory),
                various_artist_path=various,
                soundtracks_path=soundtracks,
                curated_compilations_path=curated,
            )

            self.assertTrue(catalog.artist_album_allowed({
                "path": str(music / "Aaron Neville" / "Gold"),
                "artist_name": "Aaron Neville",
                "compilation": False,
            }))
            for path in (
                soundtracks / "2 Fast 2 Furious",
                various / "Bass in Your Face",
                curated / "Hot 100",
            ):
                self.assertFalse(catalog.artist_album_allowed({
                    "path": str(path),
                    "artist_name": "Various Artists",
                    "compilation": True,
                }))


if __name__ == "__main__":
    unittest.main()
