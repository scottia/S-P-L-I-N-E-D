"""Shared settings presentation contract and cross-runtime parity guards."""

from __future__ import annotations

import json
import tomllib
import unittest
from pathlib import Path

REPOSITORY = Path(__file__).resolve().parents[2]


class SettingsContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.contract = json.loads(
            (REPOSITORY / "schema/settings-ui.json").read_text(encoding="utf-8")
        )

    def test_contract_is_config_v5_and_has_unique_sections(self) -> None:
        self.assertEqual(self.contract["config_version"], 5)
        section_ids = [section["id"] for section in self.contract["sections"]]
        self.assertEqual(len(section_ids), len(set(section_ids)))
        self.assertIn("enhancement", section_ids)
        self.assertIn("catalog", section_ids)

    def test_webgui_exposes_native_upscale_policy_without_sqlite_controls(self) -> None:
        web_paths = {
            field["path"]
            for section in self.contract["sections"]
            if not section.get("runtimes") or "webgui" in section["runtimes"]
            for field in section.get("fields", [])
            if not field.get("runtimes") or "webgui" in field["runtimes"]
        }
        self.assertIn("output.upscale_below_ideal", web_paths)
        self.assertIn("output.upscale_max_percent", web_paths)
        self.assertIn("database.url", web_paths)
        self.assertNotIn("scan.sqlite_shared", web_paths)
        self.assertNotIn("scan.windows_client_lock", web_paths)

    def test_contract_upscale_defaults_match_webgui_docker(self) -> None:
        defaults = {
            field["path"]: field.get("default")
            for section in self.contract["sections"]
            for field in section.get("fields", [])
        }
        with (REPOSITORY / "docker/config.example.toml").open("rb") as handle:
            docker = tomllib.load(handle)
        for name in (
            "upscale_below_ideal",
            "upscale_max_percent",
            "upscale_adaptive_defaults",
            "upscale_picture_percent",
            "upscale_sharpen_percent",
            "upscale_softness_percent",
            "upscale_contrast_percent",
            "upscale_exposure_percent",
            "upscale_brightness_percent",
            "upscale_gamma_percent",
            "upscale_color_temperature",
            "upscale_hue_degrees",
            "upscale_darkness_percent",
            "upscale_tint_percent",
            "upscale_tint_color",
        ):
            with self.subTest(name=name):
                expected = defaults[f"output.{name}"]
                self.assertEqual(docker["output"][name], expected)

    def test_webgui_settings_fixed_output_labels_and_global_controls(self) -> None:
        fields = {
            field["path"]: field
            for section in self.contract["sections"]
            for field in section.get("fields", [])
        }
        self.assertEqual(fields["output.file_name"]["type"], "statement")
        self.assertEqual(fields["output.file_name"]["display"], "cover · artist · banner")
        self.assertEqual(fields["output.file_formats"]["type"], "statement")
        self.assertIn("banner.*", fields["output.preserve_file"]["label"])
        self.assertTrue(fields["output.upscale_below_ideal"]["default"])
        operations = (REPOSITORY / "webgui/static/operations.js").read_text(encoding="utf-8")
        self.assertIn("Settings HELP", operations)
        self.assertIn(
            "https://github.com/scottia/S-P-L-I-N-E-D/blob/main/docs/config-v5-reference.md",
            operations,
        )
        self.assertIn("Advanced ON · All Sections", operations)
        self.assertIn("applySettingsInputs(draft, bindings, false)", operations)

    def test_webgui_library_and_logging_fields_match_reviewed_sections(self) -> None:
        sections = {section["id"]: section for section in self.contract["sections"]}
        library_paths = [field["path"] for field in sections["library"]["fields"]]
        runtime_paths = [field["path"] for field in sections["runtime"]["fields"]]
        fields = {
            field["path"]: field
            for section in self.contract["sections"]
            for field in section.get("fields", [])
        }
        self.assertIn("scan.scan_mode", library_paths)
        self.assertIn("scan.scan_library_dir", library_paths)
        self.assertNotIn("scan.scan_mode", runtime_paths)
        self.assertNotIn("scan.library_scan", runtime_paths)
        self.assertIn("scan.log_dir", runtime_paths)
        self.assertFalse(fields["scan.scan_library_dir"].get("required", False))
        self.assertEqual(fields["scan.scan_library_dir"]["default"], "")
        self.assertEqual(fields["scan.shared_artist_art_dir"]["default"], "")
        self.assertLess(
            library_paths.index("scan.cache_dir"),
            library_paths.index("scan.temporary_cache_dir"),
        )
        self.assertEqual(sections["enhancement"]["icon"], "✦")
        self.assertEqual(
            fields["verbosity"]["choices"],
            ["none", "info", "debug", "warning", "error"],
        )
        operations = (REPOSITORY / "webgui/static/operations.js").read_text(
            encoding="utf-8"
        )
        self.assertIn('scanDirectory.placeholder = "Optional"', operations)
        self.assertIn('descriptor.path === "scan.log_dir"', operations)
        self.assertIn(
            'draft.scan.temporary_cache_dir ??= draft.scan.cache_dir || ""',
            operations,
        )
        entrypoint = (REPOSITORY / "webgui/__main__.py").read_text(encoding="utf-8")
        self.assertIn('"none": "critical"', entrypoint)
        self.assertIn('access_log=verbosity != "none"', entrypoint)

    def test_windows_labels_remain_present(self) -> None:
        slint = (REPOSITORY / "windows/ui/app.slint").read_text(encoding="utf-8")
        for label in ("Upscale Below Ideal", "Max Upscale Percent", "Evaluate Final Image"):
            self.assertIn(f'label: "{label}"', slint)
        self.assertIn('["None", "Current", "Verbose", "Debugger", "Error"]', slint)
        self.assertNotIn('label: "Full Library Scan"', slint)

    def test_webgui_settings_workspace_keeps_windows_parity_sections(self) -> None:
        operations = (REPOSITORY / "webgui/static/operations.js").read_text(
            encoding="utf-8"
        )
        app = (REPOSITORY / "webgui/static/app.js").read_text(encoding="utf-8")
        for label in (
            "OLED Chroma Wheel",
            "Album Card Frames",
            "Artwork Search Order",
            "Source Provider Credentials",
            "Restore Selected Settings File",
            "WebGUI Events",
            "SPLINED File Storage",
            "Update Selected Source Policy",
            "Catalog Maintenance",
        ):
            self.assertIn(label, operations)
        for key in ("compiled", "featuring", "soundtrack", "various", "artist"):
            self.assertIn(f"{key}: {{name:", app)

    def test_webgui_persists_navidrome_in_artwork_source_order(self) -> None:
        app = (REPOSITORY / "webgui/static/app.js").read_text(encoding="utf-8")
        operations = (REPOSITORY / "webgui/static/operations.js").read_text(
            encoding="utf-8"
        )
        self.assertIn('if (!configured.includes("navidrome"))', operations)
        self.assertIn(
            'setSettingValue(draft, "sources.cover_sources", configured)',
            operations,
        )
        self.assertIn("sourceOrder", app)
        self.assertIn("for (const configuredKey of state.sourceOrder", app)
        self.assertIn("state.sourceOrder = [...(settings.config.sources?.cover_sources", operations)

    def test_direct_album_search_can_be_skipped_without_selecting_another_album(self) -> None:
        app = (REPOSITORY / "webgui/static/app.js").read_text(encoding="utf-8")
        operations = (REPOSITORY / "webgui/static/operations.js").read_text(
            encoding="utf-8"
        )
        self.assertIn("albumSearchActive", app)
        self.assertIn('$("searchButton").textContent = skipping ? "SKIP ALBUM"', app)
        self.assertIn("function skipAlbumSearch()", operations)
        self.assertIn("return skipAlbumSearch()", operations)

    def test_direct_musicbrainz_uses_persistent_main_workspace(self) -> None:
        app = (REPOSITORY / "webgui/static/app.js").read_text(encoding="utf-8")
        operations = (REPOSITORY / "webgui/static/operations.js").read_text(
            encoding="utf-8"
        )
        self.assertIn('jobKind: "direct-musicbrainz"', operations)
        self.assertIn('state.workspace = "find"', operations)
        self.assertIn('decision.jobKind === "direct-musicbrainz"', operations)
        self.assertNotIn('dialog("MusicBrainz release search")', operations)
        self.assertIn('directAlbum ? "ALBUM" : "TRACK"', app)

    def test_selected_artwork_omits_redundant_write_warning(self) -> None:
        app = (REPOSITORY / "webgui/static/app.js").read_text(encoding="utf-8")
        operations = (REPOSITORY / "webgui/static/operations.js").read_text(
            encoding="utf-8"
        )
        self.assertNotIn("Live Write · WRITE saves artwork", operations)
        self.assertIn('$("writeNotice").hidden = true', operations)
        self.assertIn('$("writeNotice").hidden = true', app)
        self.assertIn('overrideNotice.textContent = override?.writer === "windows"', app)

    def test_windows_candidate_filter_exposes_every_resolution_range(self) -> None:
        main = (REPOSITORY / "windows/src/main.rs").read_text(encoding="utf-8")
        slint = (REPOSITORY / "windows/ui/app.slint").read_text(encoding="utf-8")
        for key in (
            '"Ideal"',
            '"UpperRange"',
            '"Ladder"',
            '"AboveLadder"',
            '"LowerRange"',
            '"BelowMinimum"',
        ):
            self.assertIn(key, main)
        self.assertIn("height: 520px", slint)

    def test_windows_selected_album_shows_processing_and_embedding_facts(self) -> None:
        main = (REPOSITORY / "windows/src/main.rs").read_text(encoding="utf-8")
        slint = (REPOSITORY / "windows/ui/app.slint").read_text(encoding="utf-8")
        self.assertIn("selected-album-processing-state", slint)
        self.assertIn("selected-album-track-count", slint)
        self.assertIn("selected-album-embedded-count", slint)
        self.assertIn("ui.set_selected_album_processing_state", main)
        self.assertIn("ui.set_selected_album_track_count", main)
        self.assertIn("ui.set_selected_album_embedded_count", main)

    def test_webgui_samples_and_future_ai_defaults_are_explicit(self) -> None:
        fields = {
            field["path"]: field
            for section in self.contract["sections"]
            for field in section.get("fields", [])
        }
        self.assertFalse(fields["samples.sample_write"]["default"])
        self.assertFalse(fields["aisplined.enabled"]["default"])
        self.assertTrue(fields["aisplined.enabled"]["future"])
        with (REPOSITORY / "docker/config.example.toml").open("rb") as handle:
            docker = tomllib.load(handle)
        self.assertFalse(docker["samples"]["sample_write"])
        self.assertEqual(docker["samples"]["sample_dir"], "")


if __name__ == "__main__":
    unittest.main()
