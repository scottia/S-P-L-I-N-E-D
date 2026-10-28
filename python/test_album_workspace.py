from __future__ import annotations

from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest

from PIL import Image

from tui.album_workspace import AlbumWorkspace
from tui.library import LibraryModel
from tui.splined_tui import InputRequest, TuiAdapter, TuiState, handle_key, handle_mouse, render
from tui.theme import select_theme
from pyratatui import Rect


class AlbumWorkspaceTests(unittest.TestCase):
    def test_library_shortcut_renders_without_staging_an_album(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            album = root / "Artist" / "Album"
            cache = root / "cache"
            album.mkdir(parents=True)
            cache.mkdir()
            Image.new("RGB", (80, 80), "blue").save(album / "cover.jpg")
            state = TuiState(started_at=0.0, workflow="library")
            state.library = LibraryModel.from_payload({
                "root": str(root),
                "albums": [{"path": str(album), "artist": "Artist", "album": "Album", "status": "processed"}],
            })
            state.input_request = InputRequest("", "library-selection", {})
            state.runtime_info = {"cache": str(cache)}
            adapter = TuiAdapter()
            key = lambda code: SimpleNamespace(code=code, ctrl=False, shift=False)
            handle_key(state, adapter, key("o"))
            self.assertEqual(state.workspace, "album-art")
            class Frame:
                area = Rect(0, 0, 120, 35)
                rendered = 0
                def render_widget(self, _widget, _area):
                    self.rendered += 1
            frame = Frame()
            render(frame, state, select_theme("OLED"))
            self.assertGreater(frame.rendered, 4)
            def click(value):
                region = next(item for item in state.hit_regions if item.target == "workspace-key" and item.value == value)
                handle_mouse(state, adapter, SimpleNamespace(
                    code="down", button="left", ctrl=False,
                    column=region.x + 1, row=region.y,
                ))
            click("]")
            self.assertEqual(state.album_workspace.selected_adjustment, 1)
            click("+")
            self.assertEqual(state.album_workspace.profile.sharpen, 5)
            click("u")
            self.assertTrue(state.album_workspace.upscale)
            click("i")
            self.assertEqual(state.album_workspace.input_path, "")
            render(frame, state, select_theme("OLED"))
            click("Esc")
            self.assertIsNone(state.album_workspace.input_path)
            self.assertTrue(adapter.responses.empty())
            click("Esc")
            self.assertEqual(state.workspace, "library")

    def test_adjustments_preview_then_explicit_write(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            album = root / "Album"
            cache = root / "cache"
            album.mkdir()
            cache.mkdir()
            cover = album / "cover.jpg"
            Image.new("RGB", (90, 60), (80, 100, 120)).save(cover, format="JPEG")
            before = cover.read_bytes()
            session = AlbumWorkspace.load(album, cache, writable=True)
            try:
                self.assertEqual(session.saved, cover)
                self.assertTrue((session.preview_dir / "original.png").is_file())
                with Image.open(session.preview_dir / "enhanced.png") as preview:
                    initial_pixel = preview.getpixel((20, 20))
                session.selected_adjustment = 5  # brightness
                session.adjust(20)
                with Image.open(session.preview_dir / "enhanced.png") as preview:
                    self.assertNotEqual(preview.getpixel((20, 20)), initial_pixel)
                self.assertIsNone(session.write_approved())
                self.assertEqual(cover.read_bytes(), before)
                self.assertEqual(session.write_approved(), cover)
                self.assertNotEqual(cover.read_bytes(), before)
                self.assertEqual(session.profile.brightness, 0)
            finally:
                preview = session.preview_dir
                session.close()
                self.assertFalse(preview.exists())

    def test_webp_preview_does_not_replace_animation(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            album = root / "Album"
            cache = root / "cache"
            album.mkdir()
            cache.mkdir()
            animation = album / "cover.webp"
            Image.new("RGB", (50, 50), "blue").save(animation, format="WEBP")
            original = animation.read_bytes()
            session = AlbumWorkspace.load(album, cache, writable=True)
            try:
                self.assertIsNone(session.write_approved())
                self.assertEqual(session.write_approved(), album / "cover.jpg")
                self.assertEqual(animation.read_bytes(), original)
            finally:
                session.close()


if __name__ == "__main__":
    unittest.main()
