from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest

from PIL import Image

from splined_media_fast_index_policy import (
    _cover_statistics,
    _discover_index_albums,
    _index_track_count,
)


@dataclass
class FakeAlbumDir:
    path: Path
    audio_files: list[Path]
    local_art_files: list[Path] = field(default_factory=list)
    inventory_fingerprint: str | None = None


class RepresentativeTrackIndexTests(unittest.TestCase):
    def test_discovery_keeps_one_track_path_and_counts_the_album(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "music"
            album_path = root / "Artist" / "Album"
            album_path.mkdir(parents=True)
            tracks = [
                album_path / "01.flac",
                album_path / "02.flac",
                album_path / "03.flac",
            ]
            for number, track in enumerate(tracks, 1):
                track.write_bytes(f"track-{number}".encode())

            cover = album_path / "Cover.jpeg"
            Image.new("RGB", (64, 64)).save(cover, format="JPEG")
            playlist = album_path / "Album.m3u"
            playlist.write_text("01.flac\n", encoding="utf-8")

            core = SimpleNamespace(
                SplinedError=RuntimeError,
                TuiSessionExit=RuntimeError,
                AUDIO_EXTENSIONS={".flac", ".mp3"},
                AlbumDir=FakeAlbumDir,
                should_ignore=lambda _name, _patterns: False,
                _is_inventory_local_art=lambda path, configured: (
                    path.suffix.casefold() in {".jpg", ".jpeg", ".png", ".webp"}
                    and path.stem.casefold().startswith(configured.casefold())
                ),
                _fingerprint_from_metadata=lambda rows: "|".join(
                    f"{path.name}:{size}:{modified}"
                    for path, size, modified in rows
                ),
            )

            albums, ignored = _discover_index_albums(
                core,
                root,
                [],
                "Cover",
                fingerprint_paths={str(album_path)},
                workers=1,
            )

            self.assertEqual(ignored, [])
            self.assertEqual(len(albums), 1)
            album = albums[0]
            self.assertEqual(album.audio_files, [tracks[0]])
            self.assertEqual(_index_track_count(album), 3)
            self.assertIsNotNone(album.inventory_fingerprint)
            self.assertEqual(
                [path.name for path in album._splined_index_sidecars],
                ["Album.m3u", "Cover.jpeg"],
            )

            statistics = _cover_statistics(album, "Cover")
            self.assertEqual(statistics["root_files"], 2)
            self.assertEqual(statistics["artwork"]["JPEG"], 1)
            self.assertEqual(statistics["artwork"]["OTHER"], 1)
            self.assertEqual(statistics["cover_files"], 1)
            self.assertEqual(statistics["cover_width"], 64)
            self.assertEqual(statistics["cover_height"], 64)
            self.assertEqual(statistics["other_filenames"], ["Album.m3u"])


if __name__ == "__main__":
    unittest.main()
