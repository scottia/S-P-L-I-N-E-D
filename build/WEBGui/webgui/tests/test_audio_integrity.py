"""P0: real Mutagen artwork writer must never mutate non-artwork media tags."""
from __future__ import annotations

from io import BytesIO
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from mutagen.id3 import APIC, ID3, TPE1, TPE2, TIT2, TXXX, UFID
from PIL import Image
import numpy as np
import soundfile as sf
from mutagen.flac import FLAC, Picture

from splined_audio_integrity import AudioMetadataChanged, assert_artwork_only
from splined_compilation_authority_policy import replace_embedded_artwork


class ArtworkOnlyIntegrityTests(unittest.TestCase):
    @staticmethod
    def image() -> bytes:
        out = BytesIO()
        Image.new("RGB", (64, 64), (30, 100, 170)).save(out, "PNG")
        return out.getvalue()

    @staticmethod
    def mp3(path: Path, version: int) -> None:
        tag = ID3()
        encoding = 3 if version == 4 else 1
        tag.add(TPE1(encoding=encoding, text=["Sammy Hagar", "Taj Mahal"]))
        tag.add(TPE2(encoding=encoding, text=["Sammy Hagar", "Sammy Hagar & Friends"]))
        tag.add(TIT2(encoding=encoding, text=["Fooey"]))
        tag.add(TXXX(encoding=encoding, desc="MusicBrainz Artist Id",
                     text=["11ab", "22cd"]))
        tag.add(TXXX(encoding=encoding, desc="MY CUSTOM FIELD",
                     text=["first", "second"]))
        tag.add(UFID(owner="http://musicbrainz.org", data=b"recording-original"))
        # Non-front artwork must survive just as text frames do.
        tag.add(APIC(encoding=0, mime="image/jpeg", type=4, desc="Back",
                     data=b"other-picture-original"))
        tag.save(path, v2_version=version)
        with path.open("ab") as stream:
            stream.write(b"\xff\xfb" + b"this-is-a-synthetic-audio-payload" * 9)

    def test_real_mp3_writer_preserves_id3v24_multiple_values_and_extended_tags(self):
        with TemporaryDirectory() as directory:
            target = Path(directory) / "song.mp3"
            self.mp3(target, 4)
            replace_embedded_artwork(target, self.image(), "png",
                                     {"width": 64, "height": 64})
            tags = ID3(target)
            self.assertEqual(tags.version, (2, 4, 0))
            self.assertEqual(tags.getall("TPE1")[0].text, ["Sammy Hagar", "Taj Mahal"])
            self.assertEqual(tags.getall("TPE2")[0].text,
                             ["Sammy Hagar", "Sammy Hagar & Friends"])
            self.assertEqual(tags.getall("TXXX:MY CUSTOM FIELD")[0].text,
                             ["first", "second"])
            self.assertEqual(tags.getall("TXXX:MusicBrainz Artist Id")[0].text,
                             ["11ab", "22cd"])
            self.assertEqual(tags.getall("UFID")[0].data, b"recording-original")
            self.assertEqual(tags.getall("APIC:Back")[0].data, b"other-picture-original")
            self.assertEqual([p.data for p in tags.getall("APIC") if p.type == 3],
                             [self.image()])

    def test_real_mp3_writer_preserves_id3v23_without_upgrading(self):
        with TemporaryDirectory() as directory:
            target = Path(directory) / "song.mp3"
            self.mp3(target, 3)
            before = ID3(target).getall("TPE1")[0].text
            replace_embedded_artwork(target, self.image(), "png",
                                     {"width": 64, "height": 64})
            self.assertEqual(ID3(target).version, (2, 3, 0))
            self.assertEqual(ID3(target).getall("TPE1")[0].text, before)

    def test_previous_v24_to_v23_writer_is_detected_and_rejected(self):
        with TemporaryDirectory() as directory:
            original = Path(directory) / "original.mp3"
            staged = Path(directory) / "staged.mp3"
            self.mp3(original, 4)
            staged.write_bytes(original.read_bytes())
            old_writer = ID3(staged)
            old_writer.add(APIC(encoding=3, mime="image/png", type=3, desc="Cover",
                                data=self.image()))
            old_writer.save(staged, v2_version=3)
            with self.assertRaises(AudioMetadataChanged):
                assert_artwork_only(original, staged)
            self.assertEqual(ID3(original).version, (2, 4, 0))

    def test_metadata_and_stream_mutation_both_fail_closed(self):
        with TemporaryDirectory() as directory:
            original = Path(directory) / "original.mp3"
            staged = Path(directory) / "staged.mp3"
            self.mp3(original, 4)
            before = original.read_bytes()
            for mutation in ("artist", "musicbrainz", "audio", "version"):
                with self.subTest(mutation=mutation):
                    staged.write_bytes(before)
                    if mutation in ("artist", "musicbrainz"):
                        tags = ID3(staged)
                        if mutation == "artist":
                            tags.getall("TPE1")[0].text = ["WRONG", "ARTIST"]
                        else:
                            tags.getall("TXXX:MusicBrainz Artist Id")[0].text = ["wrong-id"]
                        tags.save(staged, v2_version=4)
                    elif mutation == "audio":
                        with staged.open("ab") as stream:
                            stream.write(b"unauthorized-stream-change")
                    else:
                        ID3(staged).save(staged, v2_version=3)
                    with self.assertRaises(AudioMetadataChanged):
                        assert_artwork_only(original, staged)
                    self.assertEqual(original.read_bytes(), before)

    def test_flac_artist_and_musicbrainz_values_survive_real_writer(self):
        with TemporaryDirectory() as directory:
            target = Path(directory) / "song.flac"
            sf.write(target, np.zeros((2000, 2)), 44100)
            audio = FLAC(target)
            audio["artist"] = ["Beyonce", "Jack White"]
            audio["musicbrainz_artistid"] = ["original-artist-id"]
            old_back = Picture()
            old_back.type = 4
            old_back.mime = "image/png"
            old_back.data = b"non-front-original"
            audio.add_picture(old_back)
            audio.save()
            replace_embedded_artwork(target, self.image(), "png",
                                     {"width": 64, "height": 64})
            verified = FLAC(target)
            self.assertEqual(verified["artist"], ["Beyonce", "Jack White"])
            self.assertEqual(verified["musicbrainz_artistid"], ["original-artist-id"])
            self.assertEqual([p.data for p in verified.pictures if p.type == 4],
                             [b"non-front-original"])

    def test_unverified_container_rejected_without_touching_original(self):
        with TemporaryDirectory() as directory:
            original = Path(directory) / "song.ogg"
            staged = Path(directory) / "copy.ogg"
            original.write_bytes(b"original")
            staged.write_bytes(b"candidate")
            with self.assertRaises(AudioMetadataChanged):
                assert_artwork_only(original, staged)
            self.assertEqual(original.read_bytes(), b"original")


if __name__ == "__main__":
    unittest.main()
