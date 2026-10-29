from __future__ import annotations

from io import BytesIO
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from PIL import Image

from splined_artist_art import (
    ArtistAlbum, artist_targets, choose_banner, local_candidates,
    prepare_banner, prepare_portrait, saved_package, write_image,
)
from webgui import imaging


ARTIST_ID = "e0bba708-bdd3-478d-84ea-c706413bedab"
ALBUM_ID = "cf1da014-7b14-4ace-b231-271013d04625"


def image_bytes(color: str, size: tuple[int, int] = (80, 60)) -> bytes:
    output = BytesIO()
    Image.new("RGB", size, color).save(output, format="PNG")
    return output.getvalue()


class ArtistArtStorageTests(unittest.TestCase):
    def test_soundtrack_artist_uses_managed_identity_without_shared_folder_art(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            soundtrack = root / "Music" / "[Soundtracks]"
            soundtrack.mkdir(parents=True)
            (soundtrack / "artist.jpg").write_bytes(image_bytes("red"))
            albums = [ArtistAlbum("Air", soundtrack / "Film", ALBUM_ID, ARTIST_ID)]
            targets = artist_targets(albums, "Air", root / "artist-art")
            self.assertEqual(len(targets), 1)
            self.assertEqual(targets[0].stem, f"artist-mbid-{ARTIST_ID}")
            self.assertEqual(local_candidates(targets), [])
            portrait = write_image(targets[0], prepare_portrait(image_bytes("blue"), 64))
            banner = write_image(targets[0].for_banner(), prepare_banner(image_bytes("green"), width=160, height=60))
            self.assertEqual(saved_package(targets).portrait, portrait)
            self.assertEqual(saved_package(targets).banner, banner)
            self.assertEqual({label for label, _ in local_candidates(targets)}, {"Artist", "Banner"})
            self.assertEqual(choose_banner(banner, None, portrait), (banner, False))

    def test_fallback_album_cannot_claim_artist_art(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            albums = [ArtistAlbum("Air", root / "Music" / "Air" / "No ID", "", ARTIST_ID)]
            self.assertEqual(artist_targets(albums, "Air", root / "artist-art"), [])

    def test_dedicated_artist_folder_precedes_managed_fallback(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            artist_dir = root / "Music" / "Air"
            artist_dir.mkdir(parents=True)
            albums = [ArtistAlbum("Air", artist_dir / "Moon Safari", ALBUM_ID, ARTIST_ID)]
            targets = artist_targets(albums, "Air", root / "artist-art")
            self.assertEqual([target.stem for target in targets], ["artist", f"artist-mbid-{ARTIST_ID}"])
            self.assertEqual(targets[0].directory, artist_dir)

    def test_prepared_portrait_is_square_and_banner_fit_tracks_position(self) -> None:
        portrait = prepare_portrait(image_bytes("purple"), 64)
        with Image.open(BytesIO(portrait)) as image:
            self.assertEqual(image.size, (64, 64))
            self.assertEqual(image.getpixel((0, 0))[3], 255)
            self.assertEqual(image.getpixel((32, 32))[3], 255)
        source = BytesIO()
        image = Image.new("RGB", (200, 80), "red")
        image.paste("blue", (100, 0, 200, 80))
        image.save(source, format="PNG")
        left = prepare_banner(source.getvalue(), width=100, height=60, zoom=1.5, x=-1)
        right = prepare_banner(source.getvalue(), width=100, height=60, zoom=1.5, x=1)
        self.assertNotEqual(left, right)

    def test_successful_write_replaces_superseded_file_only_after_validation(self) -> None:
        with TemporaryDirectory() as temporary:
            target = artist_targets(
                [ArtistAlbum("Air", Path(temporary) / "Air" / "Album", ALBUM_ID, ARTIST_ID)],
                "Air", Path(temporary) / "artist-art",
            )[0]
            old = target.directory / "artist.jpg"
            target.directory.mkdir()
            old.write_bytes(image_bytes("red"))
            with self.assertRaises(Exception):
                write_image(target, b"not a PNG")
            self.assertTrue(old.exists())
            new = write_image(target, prepare_portrait(image_bytes("blue"), 64))
            self.assertTrue(new.exists())
            self.assertFalse(old.exists())

    def test_webgui_writes_the_adjusted_banner_preview_pixels(self) -> None:
        with TemporaryDirectory() as temporary:
            target = artist_targets(
                [ArtistAlbum("Air", Path(temporary) / "Air" / "Album", ALBUM_ID, ARTIST_ID)],
                "Air", Path(temporary) / "artist-art",
            )[0].for_banner()
            source = BytesIO()
            image = Image.new("RGB", (240, 100), "red")
            image.paste("blue", (120, 0, 240, 100))
            image.save(source, format="PNG")
            preview = imaging.render(
                source.getvalue(),
                values={"Brightness": -35, "Vivid Color": 20},
                fit={"zoom": 1.4, "x": 20, "y": -10},
                kind="banner",
            )
            prepared = imaging.encode(preview)
            written = write_image(target, prepared)
            with Image.open(BytesIO(prepared)) as expected, Image.open(written) as actual:
                self.assertEqual(expected.size, actual.size)
                self.assertEqual(expected.tobytes(), actual.tobytes())


if __name__ == "__main__":
    unittest.main()
