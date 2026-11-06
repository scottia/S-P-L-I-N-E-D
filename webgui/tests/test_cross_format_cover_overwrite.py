from __future__ import annotations

from io import BytesIO
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
from unittest import mock
import unittest

from PIL import Image

import splined as core
from webgui.catalog import Catalog
from webgui.services import Services

ISOLATED_FIXTURE_ROOT = Path("E:/") if Path("E:/").is_dir() else None


def image_bytes(fmt: str, color: str) -> bytes:
    output = BytesIO()
    Image.new("RGB", (32, 32), color).save(output, format=fmt)
    return output.getvalue()


class CanonicalCoverOverwriteTests(unittest.TestCase):
    def test_required_cross_format_and_same_format_replacements(self) -> None:
        cases = (
            ("cover.jpg", "JPEG", "cover.png", "PNG"),
            ("cover.png", "PNG", "cover.jpg", "JPEG"),
            ("cover.jpeg", "JPEG", "cover.webp", "WEBP"),
            ("cover.png", "PNG", "cover.png", "PNG"),
        )
        with TemporaryDirectory(dir=ISOLATED_FIXTURE_ROOT) as directory:
            root = Path(directory)
            for index, (old_name, old_format, new_name, new_format) in enumerate(cases):
                with self.subTest(old=old_name, new=new_name):
                    album = root / str(index)
                    album.mkdir()
                    (album / old_name).write_bytes(image_bytes(old_format, "red"))
                    body = image_bytes(new_format, "blue")

                    core.install_canonical_cover(album / new_name, body, "cover")

                    self.assertEqual((album / new_name).read_bytes(), body)
                    self.assertEqual(
                        [path.name for path in core.canonical_cover_files(album, "cover")],
                        [new_name],
                    )

    def test_installer_replaces_static_canonicals_and_preserves_webp_only(self) -> None:
        with TemporaryDirectory(dir=ISOLATED_FIXTURE_ROOT) as directory:
            album = Path(directory) / "Artist" / "Album"
            album.mkdir(parents=True)
            for name, fmt in (
                ("cover.jpg", "JPEG"),
                ("cover.jpeg", "JPEG"),
                ("cover.webp", "WEBP"),
                ("cover-(2).png", "PNG"),
                ("cover-back.jpg", "JPEG"),
                ("booklet.jpg", "JPEG"),
                ("artist.png", "PNG"),
            ):
                (album / name).write_bytes(image_bytes(fmt, "red"))

            destination = album / "cover.png"
            body = image_bytes("PNG", "blue")
            removed = core.install_canonical_cover(destination, body, "cover")

            self.assertEqual(destination.read_bytes(), body)
            self.assertEqual(
                [path.name for path in core.canonical_cover_files(album, "cover")],
                ["cover.png", "cover.webp"],
            )
            self.assertEqual(len(removed), 3)
            for unrelated in ("cover-back.jpg", "booklet.jpg", "artist.png"):
                self.assertTrue((album / unrelated).is_file())

    def test_failed_publish_keeps_every_previous_cover(self) -> None:
        with TemporaryDirectory(dir=ISOLATED_FIXTURE_ROOT) as directory:
            album = Path(directory)
            old_jpg = album / "cover.jpg"
            old_webp = album / "cover.webp"
            old_jpg.write_bytes(image_bytes("JPEG", "red"))
            old_webp.write_bytes(image_bytes("WEBP", "green"))
            before = {path.name: path.read_bytes() for path in (old_jpg, old_webp)}

            with mock.patch.object(core, "atomic_write", side_effect=OSError("fixture failure")):
                with self.assertRaisesRegex(OSError, "fixture failure"):
                    core.install_canonical_cover(
                        album / "cover.png", image_bytes("PNG", "blue"), "cover"
                    )

            self.assertEqual(
                {path.name: path.read_bytes() for path in (old_jpg, old_webp)},
                before,
            )
            self.assertFalse((album / "cover.png").exists())

    def test_failed_cleanup_rolls_back_published_cross_format_cover(self) -> None:
        with TemporaryDirectory(dir=ISOLATED_FIXTURE_ROOT) as directory:
            album = Path(directory)
            old = album / "cover.jpg"
            original = image_bytes("JPEG", "red")
            old.write_bytes(original)

            with mock.patch.object(
                core,
                "remove_superseded_canonical_covers",
                side_effect=OSError("cleanup failure"),
            ):
                with self.assertRaisesRegex(OSError, "cleanup failure"):
                    core.install_canonical_cover(
                        album / "cover.png", image_bytes("PNG", "blue"), "cover"
                    )

            self.assertEqual(old.read_bytes(), original)
            self.assertFalse((album / "cover.png").exists())

    def test_custom_basename_cleanup_does_not_touch_cover_or_other_artwork(self) -> None:
        with TemporaryDirectory(dir=ISOLATED_FIXTURE_ROOT) as directory:
            album = Path(directory)
            (album / "folder.jpg").write_bytes(image_bytes("JPEG", "red"))
            (album / "cover.jpg").write_bytes(image_bytes("JPEG", "green"))
            (album / "booklet.png").write_bytes(image_bytes("PNG", "yellow"))

            core.install_canonical_cover(
                album / "folder.webp", image_bytes("WEBP", "blue"), "folder"
            )

            self.assertFalse((album / "folder.jpg").exists())
            self.assertTrue((album / "folder.webp").is_file())
            self.assertTrue((album / "cover.jpg").is_file())
            self.assertTrue((album / "booklet.png").is_file())

    def test_catalog_skips_unreadable_index_and_resolves_readable_canonical(self) -> None:
        with TemporaryDirectory(dir=ISOLATED_FIXTURE_ROOT) as directory:
            music = Path(directory) / "music"
            album = music / "Artist" / "Album"
            album.mkdir(parents=True)
            corrupt = album / "cover.jpg"
            corrupt.write_bytes(b"not an image")
            valid = album / "cover.png"
            valid.write_bytes(image_bytes("PNG", "blue"))
            catalog = Catalog(music, Path(directory) / "unused.db", music, music)

            resolved = catalog.album_cover_path(
                {"cover_path": str(corrupt), "path": str(album)}
            )

            self.assertEqual(resolved, valid)

    def test_catalog_prefers_generated_still_over_preserved_webp_source(self) -> None:
        with TemporaryDirectory(dir=ISOLATED_FIXTURE_ROOT) as directory:
            music = Path(directory) / "music"
            album = music / "Artist" / "Album"
            album.mkdir(parents=True)
            source = album / "cover.webp"
            source.write_bytes(image_bytes("WEBP", "green"))
            still = album / "cover.png"
            still.write_bytes(image_bytes("PNG", "blue"))
            catalog = Catalog(music, Path(directory) / "unused.db", music, music)

            resolved = catalog.album_cover_path(
                {"cover_path": str(source), "path": str(album)}
            )

            self.assertEqual(resolved, still)

    def test_catalog_keeps_indexed_numbered_cover_in_preserve_mode(self) -> None:
        with TemporaryDirectory(dir=ISOLATED_FIXTURE_ROOT) as directory:
            music = Path(directory) / "music"
            album = music / "Artist" / "Album"
            album.mkdir(parents=True)
            (album / "cover.jpg").write_bytes(image_bytes("JPEG", "red"))
            preserved = album / "cover-(2).png"
            preserved.write_bytes(image_bytes("PNG", "blue"))
            catalog = Catalog(music, Path(directory) / "unused.db", music, music)

            resolved = catalog.album_cover_path(
                {"cover_path": str(preserved), "path": str(album)}
            )

            self.assertEqual(resolved, preserved)

    def _services(self, album: Path, body: bytes) -> Services:
        services = Services.__new__(Services)
        services.runtime = SimpleNamespace(
            cfg={"mode": "write", "output": {"file_name": "cover", "preserve_file": False}},
            require_write=lambda: None,
        )
        services.album_row = lambda _album_id: {
            "album_key": "album-1",
            "musicbrainz_albumid": "11111111-1111-4111-8111-111111111111",
            "path": str(album),
        }
        services.preview = lambda _payload: {"image": "/prepared", "format": "png"}
        services.catalog = SimpleNamespace(image_bytes=lambda _image: body)
        services.refresh_album = lambda *_args, **_kwargs: None
        services.audit = lambda *_args, **_kwargs: None
        services.record_album_write = lambda *_args: {"durable": True, "writer": "webgui"}
        return services

    def test_live_webgui_overwrite_cleans_only_after_verified_publish(self) -> None:
        with TemporaryDirectory(dir=ISOLATED_FIXTURE_ROOT) as directory:
            album = Path(directory)
            (album / "cover.jpg").write_bytes(image_bytes("JPEG", "red"))
            (album / "cover.webp").write_bytes(image_bytes("WEBP", "green"))
            (album / "booklet.jpg").write_bytes(image_bytes("JPEG", "yellow"))
            body = image_bytes("PNG", "blue")
            services = self._services(album, body)

            result = Services.write_cover.__wrapped__(services, {"album_id": "album-1"})

            self.assertEqual(result["written"], ["cover.png"])
            self.assertEqual(
                [path.name for path in core.canonical_cover_files(album, "cover")],
                ["cover.png", "cover.webp"],
            )
            self.assertTrue((album / "booklet.jpg").is_file())

    def test_live_webgui_preserve_keeps_webp_source_at_canonical_path(self) -> None:
        with TemporaryDirectory(dir=ISOLATED_FIXTURE_ROOT) as directory:
            album = Path(directory)
            (album / "cover.jpg").write_bytes(image_bytes("JPEG", "red"))
            webp = album / "cover.webp"
            source = image_bytes("WEBP", "green")
            webp.write_bytes(source)
            services = self._services(album, image_bytes("PNG", "blue"))
            services.runtime.cfg["output"]["preserve_file"] = True

            Services.write_cover.__wrapped__(services, {"album_id": "album-1"})

            self.assertEqual(webp.read_bytes(), source)
            self.assertTrue((album / "cover-(2).jpg").is_file())
            self.assertTrue((album / "cover.png").is_file())

    def test_live_webgui_failed_write_does_not_remove_old_cross_format_cover(self) -> None:
        with TemporaryDirectory(dir=ISOLATED_FIXTURE_ROOT) as directory:
            album = Path(directory)
            old = album / "cover.jpg"
            old.write_bytes(image_bytes("JPEG", "red"))
            services = self._services(album, image_bytes("PNG", "blue"))
            services._atomic_artwork = mock.Mock(side_effect=OSError("fixture failure"))

            with self.assertRaisesRegex(OSError, "fixture failure"):
                Services.write_cover.__wrapped__(services, {"album_id": "album-1"})

            self.assertTrue(old.is_file())
            self.assertFalse((album / "cover.png").exists())

    def test_live_webgui_failed_preserve_write_restores_original_layout(self) -> None:
        with TemporaryDirectory(dir=ISOLATED_FIXTURE_ROOT) as directory:
            album = Path(directory)
            old = album / "cover.jpg"
            original = image_bytes("JPEG", "red")
            old.write_bytes(original)
            services = self._services(album, image_bytes("PNG", "blue"))
            services.runtime.cfg["output"]["preserve_file"] = True
            atomic = Services._atomic_artwork.__get__(services, Services)
            calls = 0

            def fail_publish(destination: Path, body: bytes) -> None:
                nonlocal calls
                calls += 1
                if calls == 2:
                    raise OSError("fixture failure")
                atomic(destination, body)

            services._atomic_artwork = mock.Mock(side_effect=fail_publish)

            with self.assertRaisesRegex(OSError, "fixture failure"):
                Services.write_cover.__wrapped__(services, {"album_id": "album-1"})

            self.assertEqual(old.read_bytes(), original)
            self.assertFalse((album / "cover-(2).jpg").exists())
            self.assertFalse((album / "cover.png").exists())


if __name__ == "__main__":
    unittest.main()
