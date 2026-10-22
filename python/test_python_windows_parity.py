from __future__ import annotations

import base64
from contextlib import closing
import hashlib
import io
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import splined as core
import splined_scan
from splined_backup import export_backup, read_backup, restore_backup
from splined_update import check_latest_release, newer_version, semantic_version
from tui.result_filters import (
    CandidateFilter,
    MusicBrainzFilter,
    comparison_pair,
    order_musicbrainz_results,
)
from tui.runtime_settings import (
    RuntimeSettingsDraft,
    SETTING_SPECS,
    persist_runtime_settings,
)
from tui.splined_tui import CandidateView, InputRequest, TuiAdapter, TuiState, handle_key


def output_config(*, upscale: bool = True, maximum: int = 200) -> dict[str, object]:
    return {
        "output": {
            "square": False,
            "square_mode": "off",
            "square_round_to": 0,
            "upscale_below_ideal": upscale,
            "upscale_max_percent": maximum,
            "evaluate_final_image": True,
        },
        "range": {"min": 1200, "ideal": 1800, "max": 2400, "ladder": 3600},
    }


class ArtworkProjectionParityTests(unittest.TestCase):
    def test_native_resolution_above_ideal_is_preserved(self) -> None:
        width, height, operations = core.project_dimensions(
            3200, 2400, output_config()
        )
        self.assertEqual((width, height), (3200, 2400))
        self.assertFalse(operations["resized"])

    def test_upscale_limit_is_enforced(self) -> None:
        self.assertEqual(
            core.project_dimensions(1200, 1200, output_config(maximum=200))[:2],
            (1800, 1800),
        )
        width, height, operations = core.project_dimensions(
            800, 800, output_config(maximum=200)
        )
        self.assertEqual((width, height), (800, 800))
        self.assertTrue(operations["upscale_limited"])
        self.assertFalse(operations["upscaled"])

    def test_upscale_limit_matches_windows_bounds(self) -> None:
        with self.assertRaisesRegex(core.SplinedError, "between 100 and 800"):
            core.output_settings(output_config(maximum=99))
        with self.assertRaisesRegex(core.SplinedError, "between 100 and 800"):
            core.output_settings(output_config(maximum=801))


class ResultPresentationParityTests(unittest.TestCase):
    def setUp(self) -> None:
        self.rows = [
            {
                "id": "later-us",
                "artist": "Beyoncé",
                "date": "2024-02",
                "country": "US",
                "release_class": "Album",
                "title": "Later",
            },
            {
                "id": "earlier-gb",
                "artist": "Beyoncé",
                "date": "2020",
                "country": "GB",
                "release_class": "Album",
                "title": "Earlier",
            },
            {
                "id": "soundtrack",
                "artist": "Various Artists",
                "date": "2025",
                "country": "US",
                "release_class": "Soundtrack",
                "title": "Film",
            },
            {
                "id": "various",
                "artist": "Various Artists",
                "date": "2026",
                "country": "US",
                "release_class": "EP",
                "title": "Sampler",
            },
        ]

    def test_musicbrainz_order_matches_windows_family_rules(self) -> None:
        ordered = order_musicbrainz_results(reversed(self.rows))
        self.assertEqual(
            [item["id"] for item in ordered],
            ["later-us", "earlier-gb", "soundtrack", "various"],
        )

    def test_artist_and_release_type_filters_combine(self) -> None:
        selection = MusicBrainzFilter.from_items(self.rows)
        selection.artists = {"Various Artists"}
        selection.release_types = {"EP"}
        self.assertEqual(
            [item["id"] for item in selection.visible(self.rows)], ["various"]
        )

    def test_current_album_authority_is_not_part_of_result_filter(self) -> None:
        state = TuiState(started_at=0.0)
        state.apply(
            "input",
            {
                "prompt": "",
                "context": {
                    "kind": "musicbrainz-results",
                    "options": self.rows,
                    "authority": {
                        "artist": "Beyoncé",
                        "album": "Renaissance",
                        "artist_mbids": "artist-id",
                        "release_mbid": "release-id",
                        "recording_mbid": "recording-id",
                    },
                },
            },
        )
        assert state.musicbrainz_filter is not None
        state.musicbrainz_filter.artists = {"Various Artists"}
        self.assertEqual(state.musicbrainz_authority["release_mbid"], "release-id")
        self.assertTrue(state.musicbrainz_filter.visible(self.rows))

    def test_artwork_filter_and_comparison_are_non_destructive(self) -> None:
        candidates = [
            CandidateView(1, "Local", 1800, 1800, "jpeg", "Ideal", 0, True, True, True, "local", provenance="[LOCAL]"),
            CandidateView(2, "Discogs", 1200, 1200, "jpeg", "LowerRange", 600, True, True, False, "remote"),
            CandidateView(3, "iTunes", 800, 800, "jpeg", "BelowMinimum", 1000, True, False, False, "rejected"),
        ]
        artwork_filter = CandidateFilter.from_items(candidates)
        artwork_filter.sources = {"Discogs"}
        visible = artwork_filter.visible(candidates)
        self.assertEqual([item.number for item in visible], [2])
        self.assertEqual([item.number for item in candidates], [1, 2, 3])
        self.assertEqual(comparison_pair(candidates, 1), (0, 1))


class StructuredSettingsAndBackupTests(unittest.TestCase):
    def test_structured_settings_preserve_unexposed_config(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "config.toml"
            path.write_text(
                "config_version = 5\n"
                "custom_authority = 'keep-me'\n\n"
                "[output]\nupscale_below_ideal = false\nupscale_max_percent = 200\n",
                encoding="utf-8",
            )
            draft = RuntimeSettingsDraft.from_config(
                {"output": {"upscale_below_ideal": False, "upscale_max_percent": 200}}
            )
            maximum = next(
                spec for spec in SETTING_SPECS if spec.identity == "output.upscale_max_percent"
            )
            draft.toggle_or_cycle(maximum, 1)
            updated = persist_runtime_settings(
                path,
                draft.as_payload(),
                lambda value: self.assertEqual(value["config_version"], 5),
            )
            self.assertEqual(updated["custom_authority"], "keep-me")
            self.assertEqual(updated["output"]["upscale_max_percent"], 210)

    def test_unprotected_backup_round_trip_is_windows_compatible_and_selective(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = root / "config.toml"
            interface = root / "ui.toml"
            credentials = root / "credentials"
            credentials.mkdir()
            database = root / "splined.db"
            config.write_text("config_version = 5\n", encoding="utf-8")
            interface.write_text("theme = 'OLED'\n", encoding="utf-8")
            (credentials / "musicbrainz.json").write_text('{"token":"fixture"}', encoding="utf-8")
            with closing(sqlite3.connect(database)) as connection:
                connection.execute("CREATE TABLE fixture (value TEXT)")
                connection.execute("INSERT INTO fixture VALUES ('sqlite-fixture')")
                connection.commit()
            backup = export_backup(
                root / "portable",
                config_path=config,
                interface_path=interface,
                credential_dir=credentials,
                database_path=database,
                splined_version="1.0.82",
                components=("settings", "interface", "credentials", "database"),
            )
            envelope = json.loads(backup.read_text(encoding="utf-8"))
            self.assertEqual(envelope["format"], "SPLINED-BACKUP")
            self.assertFalse(envelope["password_protected"])
            payload = read_backup(backup)
            extracted = root / "extracted.sqlite"
            extracted.write_bytes(base64.b64decode(payload["database"]))
            with closing(sqlite3.connect(extracted)) as connection:
                value = connection.execute("SELECT value FROM fixture").fetchone()[0]
            self.assertEqual(value, "sqlite-fixture")

            restored_config = root / "restore" / "config.toml"
            restored_ui = root / "restore" / "ui.toml"
            restored_credentials = root / "restore" / "credentials"
            restored_database = root / "restore" / "splined.db"
            restored = restore_backup(
                backup,
                components=("settings", "interface"),
                config_path=restored_config,
                interface_path=restored_ui,
                credential_dir=restored_credentials,
                database_path=restored_database,
            )
            self.assertEqual(restored, {"settings", "interface"})
            self.assertTrue(restored_config.is_file())
            self.assertTrue(restored_ui.is_file())
            self.assertFalse(restored_credentials.exists())
            self.assertFalse(restored_database.exists())

    def test_backup_captures_wal_and_restores_into_active_wal_database(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = root / "config.toml"
            config.write_text("config_version = 5\n", encoding="utf-8")
            database = root / "splined.db"
            writer = sqlite3.connect(database)
            try:
                writer.execute("PRAGMA journal_mode=WAL")
                writer.execute("CREATE TABLE fixture(value TEXT)")
                writer.execute("INSERT INTO fixture VALUES ('before')")
                writer.commit()
                self.assertTrue(Path(str(database) + "-wal").is_file())
                backup = export_backup(
                    root / "library.spl",
                    config_path=config,
                    credential_dir=root / "credentials",
                    database_path=database,
                    splined_version="1.0.99",
                    components=("database",),
                )
                writer.execute("UPDATE fixture SET value = 'after'")
                writer.commit()
            finally:
                writer.close()

            restored = restore_backup(
                backup,
                components=("database",),
                config_path=config,
                credential_dir=root / "credentials",
                database_path=database,
            )
            self.assertEqual(restored, {"database"})
            with closing(sqlite3.connect(database)) as connection:
                value = connection.execute("SELECT value FROM fixture").fetchone()[0]
            self.assertEqual(value, "before")

    def test_restore_preflights_every_selected_payload_before_writing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = root / "source.toml"
            credentials = root / "credentials"
            credentials.mkdir()
            database = root / "source.db"
            config.write_text("config_version = 5\n", encoding="utf-8")
            database.write_bytes(b"sqlite")
            backup = export_backup(
                root / "state.spl",
                config_path=config,
                credential_dir=credentials,
                database_path=database,
                splined_version="1.0.82",
                components=("settings", "credentials"),
            )
            envelope = json.loads(backup.read_text(encoding="utf-8"))
            payload = json.loads(base64.b64decode(envelope["payload"]))
            payload["credentials"] = {"../unsafe.json": base64.b64encode(b"{}").decode("ascii")}
            plain = json.dumps(
                payload, ensure_ascii=False, separators=(",", ":")
            ).encode("utf-8")
            envelope["payload"] = base64.b64encode(plain).decode("ascii")
            envelope["authentication"] = base64.b64encode(
                hashlib.sha256(plain).digest()
            ).decode("ascii")
            backup.write_text(
                json.dumps(envelope, separators=(",", ":")), encoding="utf-8"
            )
            destination = root / "restore" / "config.toml"
            with self.assertRaisesRegex(ValueError, "unsafe credential"):
                restore_backup(
                    backup,
                    components=("settings", "credentials"),
                    config_path=destination,
                    credential_dir=root / "restore" / "credentials",
                    database_path=root / "restore" / "splined.db",
                )
            self.assertFalse(destination.exists())


class CompilationTargetingParityTests(unittest.TestCase):
    def test_track_expansion_materializes_embedded_preview_for_focus(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            image = io.BytesIO()
            core.Image.new("RGB", (24, 24), (25, 75, 125)).save(
                image, format="PNG"
            )
            audio = Path(directory) / "track.mp3"
            audio.write_bytes(b"fixture")
            with patch.object(
                core, "embedded_artwork_bytes", return_value=image.getvalue()
            ):
                preview = core.embedded_artwork_preview(
                    audio, Path(directory) / "preview-cache"
                )
            self.assertIsNotNone(preview)
            assert preview is not None
            self.assertEqual((preview["width"], preview["height"]), (24, 24))
            self.assertTrue(Path(preview["path"]).is_file())

    def test_explicit_completed_track_reopens_without_targeting_focused_siblings(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first = core.Track(root / "01.mp3", "One", "Artist", None, None, None, "recording-1", "1", "artist-1")
            second = core.Track(root / "02.mp3", "Two", "Artist", None, None, None, "recording-2", "1", "artist-2")
            album = core.AlbumDir(root, [first.path, second.path])
            checked: list[str] = []

            def eligible(track: core.Track) -> tuple[bool, str]:
                checked.append(str(track.path))
                return False, "fixture stop"

            summary = core.Summary()
            unresolved: list[dict[str, str]] = []
            with (
                patch.object(core, "compilation_manual_album_eligible", return_value=True, create=True),
                patch.object(core, "compilation_completed_track_paths", return_value={str(first.path), str(second.path)}, create=True),
                patch.object(core, "compilation_manual_track_eligible", side_effect=eligible, create=True),
                patch.object(splined_scan, "_manual_progress"),
            ):
                result = splined_scan.run_manual_compilation_album(
                    album,
                    [first, second],
                    http=None,
                    config_file=root / "config.toml",
                    cfg={"mode": "write"},
                    sources=[],
                    cache=root,
                    format_order=[],
                    summary=summary,
                    api_queried=set(),
                    unresolved_items=unresolved,
                    state={},
                    target_track_paths={str(first.path)},
                )
            self.assertTrue(result)
            self.assertEqual(checked, [str(first.path)])
            self.assertEqual([item["path"] for item in unresolved], [str(first.path)])


class UpdateAndDiagnosticsParityTests(unittest.TestCase):
    def test_numeric_semantic_version_is_the_only_update_authority(self) -> None:
        self.assertEqual(semantic_version("v1.0.71"), (1, 0, 71))
        self.assertFalse(newer_version("1.0.71", "1.0.71"))
        self.assertTrue(newer_version("1.0.71", "1.0.72"))
        self.assertFalse(newer_version("1.0.72", "1.0.71"))

    def test_release_check_ignores_drafts_prereleases_and_commits(self) -> None:
        class Response:
            def raise_for_status(self) -> None:
                return None

            def json(self) -> list[dict[str, object]]:
                return [
                    {"tag_name": "1.0.99", "draft": True},
                    {"tag_name": "1.0.83", "prerelease": True},
                    {
                        "tag_name": "1.0.82",
                        "html_url": "https://github.com/scottia/S-P-L-I-N-E-D/releases/tag/1.0.82",
                    },
                ]

        session = SimpleNamespace(get=lambda *_args, **_kwargs: Response())
        result = check_latest_release("1.0.82", session=session)
        self.assertFalse(result.update_available)
        self.assertNotIn("commit", result.__dict__)

    def test_about_runtime_payload_and_visual_preview_actions_remain_local(self) -> None:
        state = TuiState(started_at=0.0)
        state.apply(
            "library",
            {
                "root": "/music",
                "artists": [],
                "albums": [],
                "config": {"config_version": 5},
                "runtime_info": {
                    "version": "1.0.82",
                    "python": "3.14",
                    "platform": "fixture",
                    "database": "/cache/splined.db",
                },
            },
        )
        self.assertEqual(state.runtime_info["database"], "/cache/splined.db")
        state.candidates = [
            CandidateView(1, "Local", 1800, 1800, "jpeg", "Ideal", 0, True, True, True, "local", provenance="[LOCAL]"),
            CandidateView(2, "iTunes", 2400, 2400, "jpeg", "UpperRange", 600, True, True, True, "remote"),
        ]
        state.candidate_filter = CandidateFilter.from_items(state.candidates)
        state.input_request = InputRequest("Choice: ", "picker", {})
        adapter = TuiAdapter()
        handle_key(state, adapter, SimpleNamespace(code="c", ctrl=False, shift=False))
        self.assertTrue(state.compare_modal_open)
        state.compare_modal_open = False
        handle_key(state, adapter, SimpleNamespace(code="v", ctrl=False, shift=False))
        self.assertTrue(state.preview_modal_open)
        self.assertTrue(adapter.responses.empty())


if __name__ == "__main__":
    unittest.main()
