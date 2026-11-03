"""Embedded artwork reads use the same CoverFront authority as Windows/Lofty."""

from __future__ import annotations

from io import BytesIO
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from mutagen import File as MutagenFile
from mutagen.flac import FLAC, Picture
from mutagen.id3 import APIC, ID3
import numpy as np
from PIL import Image
import soundfile as sf

from webgui.catalog import embedded_track_artwork_bytes


def image_bytes(color: str) -> bytes:
    output = BytesIO()
    Image.new("RGB", (32, 32), color).save(output, format="JPEG")
    return output.getvalue()


class EmbeddedArtworkReaderTests(unittest.TestCase):
    def setUp(self):
        self.temporary = TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.front = image_bytes("blue")
        self.back = image_bytes("red")

    def tearDown(self):
        self.temporary.cleanup()

    @staticmethod
    def apic(kind: int, description: str, data: bytes) -> APIC:
        return APIC(
            encoding=3,
            mime="image/jpeg",
            type=kind,
            desc=description,
            data=data,
        )

    def test_mp3_selects_coverfront_and_never_substitutes_first_picture(self):
        target = self.root / "typed.mp3"
        tags = ID3()
        tags.add(self.apic(4, "Back", self.back))
        tags.add(self.apic(3, "Front", self.front))
        tags.save(target)
        self.assertEqual(embedded_track_artwork_bytes(target), self.front)

        back_only = self.root / "back-only.mp3"
        tags = ID3()
        tags.add(self.apic(4, "Back", self.back))
        tags.save(back_only)
        self.assertIsNone(embedded_track_artwork_bytes(back_only))

    def test_flac_selects_coverfront_and_never_substitutes_first_picture(self):
        target = self.root / "typed.flac"
        sf.write(target, np.zeros((100, 2)), 44100)
        audio = FLAC(target)
        for kind, description, data in (
            (4, "Back", self.back),
            (3, "Front", self.front),
        ):
            picture = Picture()
            picture.type = kind
            picture.mime = "image/jpeg"
            picture.desc = description
            picture.data = data
            audio.add_picture(picture)
        audio.save()
        self.assertEqual(embedded_track_artwork_bytes(target), self.front)

        audio = FLAC(target)
        audio.clear_pictures()
        picture = Picture()
        picture.type = 4
        picture.mime = "image/jpeg"
        picture.data = self.back
        audio.add_picture(picture)
        audio.save()
        self.assertIsNone(embedded_track_artwork_bytes(target))

    def test_wav_and_aiff_read_coverfront_instead_of_missing_non_mp3_apic(self):
        for suffix in (".wav", ".aiff"):
            with self.subTest(suffix=suffix):
                target = self.root / ("typed" + suffix)
                sf.write(target, np.zeros((100, 2)), 44100)
                audio = MutagenFile(target)
                audio.add_tags()
                audio.tags.add(self.apic(4, "Back", self.back))
                audio.tags.add(self.apic(3, "Front", self.front))
                audio.save()

                self.assertEqual(embedded_track_artwork_bytes(target), self.front)

                audio = MutagenFile(target)
                audio.tags.delall("APIC")
                audio.tags.add(self.apic(4, "Back", self.back))
                audio.save()
                self.assertIsNone(embedded_track_artwork_bytes(target))


if __name__ == "__main__":
    unittest.main()
