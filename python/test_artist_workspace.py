from __future__ import annotations

from io import BytesIO
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest
from unittest import mock

from PIL import Image

from splined_media_database import connect
from tui.artist_workspace import ArtistWorkspace
from tui.library import LibraryModel
from tui.splined_tui import (
    InputRequest, TuiAdapter, TuiState, _clear_workspace_overlays,
    _draw_workspace_overlays, handle_key, handle_mouse, render,
)
from tui.theme import select_theme
from pyratatui import Rect


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

    def test_tui_artist_shortcut_opens_and_returns_without_writing(self) -> None:
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            cache = root / "cache"
            cache.mkdir()
            album = root / "music" / "Air" / "Moon Safari"
            album.mkdir(parents=True)
            (album.parent / "artist.png").write_bytes(_png("red"))
            (album.parent / "banner.png").write_bytes(_png("blue"))
            database = cache / "splined.db"
            connection = connect(database, "test")
            with connection:
                connection.execute(
                    "INSERT INTO artists(artist_key, artist_name, primary_path, musicbrainz_artistid, "
                    "created_at, updated_at, last_seen_at, splined_version) "
                    "VALUES('air', 'Air', ?, ?, 'now', 'now', 'now', 'test')",
                    (str(album.parent), ARTIST_ID),
                )
                connection.execute(
                    "INSERT INTO albums(album_key, artist_key, album_name, path, musicbrainz_albumid, "
                    "tag_signature, created_at, updated_at, last_seen_at, splined_version) "
                    "VALUES('air-moon', 'air', 'Moon Safari', ?, ?, 'tag', 'now', 'now', 'now', 'test')",
                    (str(album), ALBUM_ID),
                )
            connection.close()
            state = TuiState(started_at=0.0, workflow="library")
            state.image_protocol = "Sixel"
            state.library = LibraryModel.from_payload({
                "root": str(root / "music"),
                "albums": [{"path": str(album), "artist": "Air", "album": "Moon Safari", "status": "processed"}],
            })
            state.input_request = InputRequest("", "library-selection", {})
            state.runtime_info = {"database": str(database), "cache": str(cache), "artist_art": str(root / "managed")}
            state.artist_write_mode = True
            adapter = TuiAdapter()
            key = lambda code: SimpleNamespace(code=code, ctrl=False, shift=False)
            state.library_focus = 4
            handle_key(state, adapter, key("g"))
            self.assertEqual(state.workspace, "library")
            self.assertEqual(state.library.artist_filter, "g")
            state.library_focus = 0
            handle_key(state, adapter, key("g"))
            self.assertEqual(state.workspace, "artist-art")
            self.assertIsNotNone(state.artist_workspace)
            class Frame:
                area = Rect(0, 0, 120, 35)
                rendered = 0
                def render_widget(self, _widget, _area):
                    self.rendered += 1
            frame = Frame()
            overlays = []
            class Overlay:
                def __init__(self, width, height):
                    self.width, self.height = width, height
                    self.draws = []
                def draw(self, x, y):
                    self.draws.append((x, y))
            def prepare(_body, width, height, _limit):
                overlay = Overlay(width, height)
                overlays.append(overlay)
                return overlay
            with mock.patch("tui.splined_tui.native_prepare_image_overlay", side_effect=prepare), mock.patch("tui.splined_tui.native_clear_image_area") as clear:
                render(frame, state, select_theme("OLED"))
                self.assertEqual(len(state.workspace_overlay_pending), 4)
                _draw_workspace_overlays(state)
                self.assertEqual(sum(len(item.draws) for item in overlays), 4)
                render(frame, state, select_theme("OLED"))
                _draw_workspace_overlays(state)
                self.assertEqual(len(overlays), 4)
                self.assertEqual(sum(len(item.draws) for item in overlays), 4)
                state.artist_workspace.adjust(10)
                render(frame, state, select_theme("OLED"))
                _draw_workspace_overlays(state)
                self.assertGreater(sum(len(item.draws) for item in overlays), 4)
                _clear_workspace_overlays(state)
                self.assertTrue(clear.called)
                self.assertFalse(state.workspace_overlay_drawn)
                prepared_count = len(overlays)
                state.image_protocol = "Halfblocks"
                render(frame, state, select_theme("OLED"))
                self.assertFalse(state.workspace_overlay_pending)
                self.assertEqual(len(overlays), prepared_count)
            def click(target, *, index=None, value=None):
                region = next(item for item in state.hit_regions if item.target == target
                              and (index is None or item.index == index)
                              and (value is None or item.value == value))
                handle_mouse(state, adapter, SimpleNamespace(
                    code="down", button="left", ctrl=False,
                    column=region.x + 1, row=region.y,
                ))
            click("workspace-key", value="]")
            self.assertEqual(state.artist_workspace.selected_adjustment, 1)
            click("workspace-key", value="+")
            self.assertEqual(state.artist_workspace.portrait_adjustments.sharpen, 5)
            image_region = next(item for item in state.hit_regions if item.target == "workspace-image" and item.index == 2)
            handle_mouse(state, adapter, SimpleNamespace(
                code="scroll_up", button="none", ctrl=False,
                column=image_region.x + 2, row=image_region.y + 2,
            ))
            self.assertEqual(state.artist_workspace.portrait_fit.zoom, 1.05)
            handle_mouse(state, adapter, SimpleNamespace(
                code="down", button="left", ctrl=False,
                column=image_region.x + 2, row=image_region.y + 2,
            ))
            handle_mouse(state, adapter, SimpleNamespace(
                code="drag", button="left", ctrl=False,
                column=image_region.x + 4, row=image_region.y + 2,
            ))
            handle_mouse(state, adapter, SimpleNamespace(code="up", button="left", ctrl=False))
            self.assertLess(state.artist_workspace.portrait_fit.x, 0)
            click("workspace-candidate", index=1)
            self.assertEqual(state.artist_workspace.selected_candidate, 1)
            click("workspace-key", value="3")
            self.assertEqual(state.artist_workspace.selected_component, "banner")
            click("workspace-key", value="w")
            self.assertTrue(state.artist_workspace.confirm_write)
            click("workspace-key", value="Esc")
            self.assertFalse(state.artist_workspace.confirm_write)
            self.assertEqual(state.workspace, "artist-art")
            self.assertGreater(frame.rendered, 4)
            handle_key(state, adapter, key("esc"))
            self.assertEqual(state.workspace, "library")
            self.assertIsNone(state.artist_workspace)
            self.assertEqual((album.parent / "artist.png").read_bytes(), _png("red"))


if __name__ == "__main__":
    unittest.main()
