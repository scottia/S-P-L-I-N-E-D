from __future__ import annotations

import io
import contextlib
import tempfile
import time
import tomllib
import unittest
from pathlib import Path
from unittest.mock import patch

from PIL import Image

import splined
import splined_scan
from tui.aispline import (
    AISPLINE_TITLE,
    AiCandidate,
    EnhancementSelection,
    validated_enhanced_results,
)
from tui.capabilities import MOUSE_EVENTS_AVAILABLE, MOUSE_LIMITATION
from tui.library import (
    AlbumItem,
    AlbumStatus,
    ArtistStatus,
    LibraryModel,
    artist_status,
)
from tui.source_settings import PolicyDraft, persist_policy_draft
from tui.splined_tui import CandidateView, TuiState, _candidate_groups


REPOSITORY = Path(__file__).resolve().parent.parent


def example_config() -> dict:
    with (REPOSITORY / "config.example.toml").open("rb") as handle:
        return tomllib.load(handle)


class LibraryWorkspaceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.model = LibraryModel(
            "/music",
            [
                AlbumItem("/music/maniacs/ruins", "10,000 Maniacs", "Love Among the Ruins", AlbumStatus.UNPROCESSED),
                AlbumItem("/music/maniacs/eden", "10,000 Maniacs", "Our Time in Eden", AlbumStatus.PROCESSED),
                AlbumItem("/music/acdc/back", "AC/DC", "Back in Black", AlbumStatus.BYPASSED),
                AlbumItem("/music/adele/21", "Adele", "21", AlbumStatus.TIMEOUT),
            ],
        )

    def test_live_filters_are_case_insensitive_and_in_memory(self) -> None:
        self.model.set_filters(artist="MANIACS", album="rUiNs")
        self.assertEqual([item.title for item in self.model.visible_albums()], ["Love Among the Ruins"])
        self.assertEqual(self.model.inventory_loads, 1)

    def test_artist_status_precedence(self) -> None:
        self.assertEqual(artist_status([self.model.albums[0]]), ArtistStatus.UNPROCESSED)
        self.assertEqual(artist_status(self.model.albums[:2]), ArtistStatus.PARTIAL)
        self.assertEqual(artist_status([self.model.albums[1]]), ArtistStatus.COMPLETE)
        self.assertEqual(artist_status(self.model.albums[1:3]), ArtistStatus.CONTAINS_BYPASS)

    def test_artist_cascade_selects_only_unprocessed_children(self) -> None:
        self.model.toggle_artist("10,000 Maniacs")
        self.assertTrue(self.model.albums[0].selected)
        self.assertFalse(self.model.albums[1].selected)
        self.assertEqual(self.model.toggle_album(self.model.albums[2]), "bypass-confirmation-required")
        self.assertEqual(self.model.toggle_album(self.model.albums[3]), "timeout-active")

    def test_filtered_selection_preserves_hidden_rows(self) -> None:
        self.model.set_filters(artist="maniacs", album="ruins")
        self.model.select_all(filtered=True)
        self.assertTrue(self.model.albums[0].selected)
        self.assertFalse(self.model.albums[1].selected)
        self.assertEqual(len(self.model.albums), 4)


class SourcePolicyDraftTests(unittest.TestCase):
    def test_atomic_save_preserves_unknown_config_and_explicitly_applies(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "config.toml"
            body = (REPOSITORY / "config.example.toml").read_text(encoding="utf-8")
            path.write_text(
                body + "\n# preserve-me\n[future_extension]\nkeep = \"yes\"\n",
                encoding="utf-8",
            )
            draft = PolicyDraft.from_config(example_config())
            draft.toggle("discogs", "source_override")
            draft.policies["discogs"]["minimum_short_side"] = 1000
            draft.move("discogs", -1)
            saved = persist_policy_draft(path, draft.as_payload(), splined.validate_config_v5)
            self.assertEqual(saved["future_extension"]["keep"], "yes")
            self.assertTrue(saved["source_policies"]["discogs"]["source_override"])
            self.assertEqual(saved["source_policies"]["discogs"]["minimum_short_side"], 1000)
            self.assertEqual(saved["config_version"], 5)
            self.assertIn("# preserve-me", path.read_text(encoding="utf-8"))

    def test_early_filter_uses_only_reliable_metadata(self) -> None:
        config = example_config()
        policy = config["source_policies"]["discogs"]
        policy.update({"source_override": True, "minimum_short_side": 1000})
        small = splined.Ref("discogs", "small", "https://x/small.jpg", width=500, height=500)
        unknown = splined.Ref("discogs", "unknown", "https://x/unknown.jpg")
        filtered, diagnostics = splined.filter_download_references([small, unknown], config)
        self.assertEqual([item.id for item in filtered], ["unknown"])
        self.assertIn("policy-filtered", diagnostics[0][1])


class AiContractTests(unittest.TestCase):
    def test_disabled_ai_has_no_selection_surface(self) -> None:
        state = TuiState()
        state.apply(
            "candidates",
            {
                "aisplined": {"enabled": False},
                "items": [
                    {
                        "number": 1,
                        "source": "Local",
                        "width": 1500,
                        "height": 1500,
                        "format": "jpeg",
                        "range_type": "LowerRange",
                        "distance": 300,
                        "square": True,
                        "acceptable": True,
                        "approved": True,
                        "provenance": "[LOCAL]",
                    }
                ],
            },
        )
        self.assertFalse(state.ai_enabled)
        self.assertIsNone(state.ai_selection)

    def test_one_selection_and_runtime_only_override(self) -> None:
        state = EnhancementSelection(True, True, ideal=1800)
        state.register(AiCandidate("local", 1500, "[LOCAL]", True))
        state.register(AiCandidate("remote", 600, "[URL]", True))
        self.assertEqual(state.request("local", upscale_below_ideal=False), "upscale-confirmation-required")
        self.assertEqual(state.confirm_upscale(True), "selected-runtime-override")
        self.assertTrue(state.disabled("remote"))
        self.assertEqual(state.request("remote", upscale_below_ideal=True), "selection-locked")
        state.finish_attempt()
        self.assertIsNone(state.runtime_upscale_override)
        self.assertEqual(AISPLINE_TITLE, "A:I:S:P:L:I:N:E:D")

    def test_floor_and_runtime_unavailable_never_fabricate_review(self) -> None:
        state = EnhancementSelection(True, False)
        state.register(AiCandidate("tiny", 34, "[URL]", None))
        self.assertEqual(state.request("tiny", upscale_below_ideal=True), "runtime-unavailable")
        self.assertEqual(state.label("tiny"), "N/A")

    def test_enhanced_history_requires_existing_valid_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            album = Path(directory)
            valid = album / "enhanced.png"
            Image.new("RGB", (32, 32), "red").save(valid)
            history = {"enhanced_results": [
                {"path": "enhanced.png", "validated": True},
                {"path": "missing.png", "validated": True},
                {"path": "enhanced.png", "validated": False},
            ]}
            result = validated_enhanced_results(history, album)
            self.assertEqual(len(result), 1)
            self.assertEqual(result[0]["path"], valid.resolve())


class PresentationAndPerformanceTests(unittest.TestCase):
    def test_help_documents_real_tui_modes_and_themes(self) -> None:
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            splined.print_help(REPOSITORY / "config.example.toml", example_config())
        text = output.getvalue()
        for value in ("--tui", "--no-tui", "--tui-theme OLED", "--tui-theme CHALK", "advanced verbose/diagnostic"):
            self.assertIn(value, text)

    def test_candidate_groups_and_hidden_url_contract(self) -> None:
        state = TuiState()
        state.candidates = [
            CandidateView(1, "Local", 1500, 1500, "jpeg", "LowerRange", 300, True, True, True, "internal", provenance="[LOCAL]"),
            CandidateView(2, "iTunes", 1800, 1800, "jpeg", "Ideal", 0, True, True, True, "private-id", url="https://example.test/art.jpg", provenance="[URL]"),
            CandidateView(3, "Enhanced", 1800, 1800, "png", "Ideal", 0, True, True, True, "history", provenance="[Enhanced]"),
        ]
        groups = _candidate_groups(state)
        self.assertEqual([name for name, _items, _semantic in groups], ["LOCAL", "iTunes", "ENHANCED"])
        self.assertEqual(state.candidates[1].provenance, "[URL]")
        self.assertNotEqual(state.candidates[1].url, state.candidates[1].provenance)

    def test_activity_feed_records_real_events_only(self) -> None:
        state = TuiState()
        state.apply("activity", {"category": "provider", "state": "done", "source": "itunes", "message": "1 reference"})
        self.assertEqual(len(state.activity), 1)
        self.assertEqual(state.activity[0].message, "1 reference")

    def test_provider_discovery_is_bounded_concurrent_and_ordered(self) -> None:
        release = splined.Release("mbid", "Album", "Artist", "rg", "Album")
        sources = ["deezer", "itunes", "lastfm", "discogs"]
        def delayed(source: str):
            def call(*_args, **_kwargs):
                time.sleep(0.05)
                return [splined.Ref(source, source, f"https://x/{source}.jpg")]
            return call
        with patch.object(splined, "discover_deezer", delayed("deezer")), patch.object(splined, "discover_itunes", delayed("itunes")), patch.object(splined, "discover_lastfm", delayed("lastfm")), patch.object(splined, "discover_discogs", delayed("discogs")):
            started = time.perf_counter()
            refs, diagnostics = splined.discover_all(object(), Path("config.toml"), {}, release, sources)
            elapsed = time.perf_counter() - started
        self.assertLess(elapsed, 0.16)
        self.assertEqual([item.source for item in refs], sources)
        self.assertEqual(diagnostics, [])
        self.assertLessEqual(splined.PROVIDER_DISCOVERY_WORKERS, 4)

    def test_candidate_downloads_are_bounded_concurrent_and_keep_ref_order(self) -> None:
        image_buffer = io.BytesIO()
        Image.new("RGB", (1200, 1200), "blue").save(image_buffer, format="JPEG")
        artwork = image_buffer.getvalue()

        class Response:
            status_code = 200
            headers = {}
            def __enter__(self):
                return self
            def __exit__(self, *_args):
                return False
            def iter_content(self, chunk_size: int):
                del chunk_size
                yield artwork

        class Http:
            def get(self, *_args, **_kwargs):
                time.sleep(0.05)
                return Response()

        refs = [
            splined.Ref("itunes", str(index), f"https://x/{index}.jpg")
            for index in range(4)
        ]
        with tempfile.TemporaryDirectory() as directory:
            started = time.perf_counter()
            candidates, diagnostics = splined.download_candidates(
                Http(), refs, ["itunes"], Path(directory), None
            )
            elapsed = time.perf_counter() - started
        self.assertLess(elapsed, 0.16)
        self.assertEqual([candidate.ref.id for candidate in candidates], ["0", "1", "2", "3"])
        self.assertEqual(diagnostics, [])
        self.assertLessEqual(splined.CANDIDATE_DOWNLOAD_WORKERS, 4)

    def test_duplicate_url_fetch_is_reused_without_collapsing_candidates(self) -> None:
        image_buffer = io.BytesIO()
        Image.new("RGB", (1200, 1200), "blue").save(image_buffer, format="JPEG")
        artwork = image_buffer.getvalue()

        class Response:
            status_code = 200
            headers = {}
            def __enter__(self):
                return self
            def __exit__(self, *_args):
                return False
            def iter_content(self, chunk_size: int):
                del chunk_size
                yield artwork

        class Http:
            calls = 0
            def get(self, *_args, **_kwargs):
                self.calls += 1
                return Response()

        http = Http()
        refs = [
            splined.Ref("itunes", "z-id", "https://x/shared.jpg", approved=True),
            splined.Ref("itunes", "a-id", "https://x/shared.jpg", approved=False),
        ]
        with tempfile.TemporaryDirectory() as directory:
            candidates, diagnostics = splined.download_candidates(
                http, refs, ["itunes"], Path(directory), None
            )
        self.assertEqual(http.calls, 1)
        self.assertEqual([item.ref.id for item in candidates], ["z-id", "a-id"])
        self.assertEqual([item.ref.approved for item in candidates], [True, False])
        self.assertEqual(diagnostics, [])
        best = splined.select_best(candidates, example_config(), ["jpeg", "png", "webp"])
        self.assertIsNotNone(best)
        self.assertEqual(best.ref.id, "a-id")

    def test_mouse_limitation_is_explicit_not_fake_support(self) -> None:
        self.assertFalse(MOUSE_EVENTS_AVAILABLE)
        self.assertIn("keyboard events only", MOUSE_LIMITATION)

    def test_scan_entrypoint_processes_nonempty_inventory(self) -> None:
        cfg = example_config()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            album_path = root / "Artist" / "Album"
            album_path.mkdir(parents=True)
            album = splined.AlbumDir(album_path, [album_path / "broken.mp3"])
            config_file = root / "config.toml"
            output = io.StringIO()
            with patch.object(splined_scan.core, "inventory", return_value=([album], [])), patch.object(
                splined_scan.core, "read_track", side_effect=splined.SplinedError("tag broken")
            ), contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
                result = splined_scan.run_scan_dir(
                    config_file, cfg, ["itunes"], [str(root)]
                )
        self.assertEqual(result, 1)
        self.assertIn("tag broken", output.getvalue())


if __name__ == "__main__":
    unittest.main()
