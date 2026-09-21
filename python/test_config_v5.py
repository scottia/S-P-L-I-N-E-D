from __future__ import annotations

import tempfile
import tomllib
import unittest
from pathlib import Path

import splined


REPOSITORY = Path(__file__).resolve().parent.parent


def load_example(relative: str) -> dict:
    with (REPOSITORY / relative).open("rb") as handle:
        return tomllib.load(handle)


class ConfigV5ParityTests(unittest.TestCase):
    def test_all_public_examples_validate_as_v5(self) -> None:
        for relative in (
            "config.example.toml",
            "windows/config.example.toml",
            "docker/config.example.toml",
        ):
            with self.subTest(relative=relative):
                config = load_example(relative)
                self.assertEqual(config["config_version"], 5)
                splined.validate_config_v5(config)

    def test_fixed_provider_filenames_ignore_legacy_overrides(self) -> None:
        config = load_example("config.example.toml")
        config["credentials"]["credential_dir"] = "private"
        config["fanarttv"] = {"credential_file": "outside.json"}
        config["lastfm"] = {"credential_file": "outside.json"}
        config["musicbrainz"] = {"token_file": "outside.json"}
        config_file = Path("portable") / "config" / "config.toml"

        self.assertEqual(
            splined.credential_file(config_file, config, "fanarttv"),
            (config_file.parent / "private").resolve() / "fanarttv.json",
        )
        self.assertEqual(
            splined.credential_file(config_file, config, "lastfm"),
            (config_file.parent / "private").resolve() / "lastfm.json",
        )
        self.assertEqual(
            splined.credential_file(config_file, config, "musicbrainz"),
            (config_file.parent / "private").resolve() / "musicbrainz.json",
        )

    def test_source_override_uses_only_adjacent_lower_fallback(self) -> None:
        config = load_example("config.example.toml")
        policy = config["source_policies"]["discogs"]
        policy["source_override"] = True
        policy["minimum_range_type"] = "Ideal"
        policy["allow_below_minimum_fallback"] = True

        self.assertEqual(
            splined.source_policy_decision(config, "discogs", 1500, 1500)[0],
            "fallback",
        )
        self.assertEqual(
            splined.source_policy_decision(config, "discogs", 900, 900)[0],
            "reject",
        )
        self.assertEqual(
            splined.source_policy_decision(config, "discogs", 1800, 1800)[0],
            "accept",
        )

    def test_source_override_applies_advanced_constraints_and_primary_metadata(self) -> None:
        config = load_example("config.example.toml")
        policy = config["source_policies"]["discogs"]
        policy.update(
            {
                "source_override": True,
                "minimum_short_side": 1000,
                "maximum_short_side": 2000,
                "minimum_width": 1200,
                "minimum_height": 1000,
                "primary_image_only": True,
            }
        )

        self.assertEqual(
            splined.source_policy_decision(config, "discogs", 1199, 1500)[0],
            "reject",
        )
        self.assertEqual(
            splined.source_policy_decision(config, "discogs", 1500, 1500)[0],
            "accept",
        )
        self.assertFalse(splined.reference_allowed(config, "discogs", False))
        self.assertTrue(splined.reference_allowed(config, "discogs", True))

    def test_disabled_provider_is_removed_without_erasing_policy(self) -> None:
        config = load_example("config.example.toml")
        config["source_policies"]["fanarttv"]["enabled"] = False
        sources = splined.resolve_sources(config, None, None, [])
        self.assertNotIn("fanarttv", sources)
        self.assertEqual(
            config["source_policies"]["fanarttv"]["minimum_range_type"],
            "LowerRange",
        )

    def test_v4_compatibility_migration_adds_v5_sections(self) -> None:
        config = {
            "config_version": 4,
            "mode": "read",
            "verbosity": "info",
            "credentials": {"credential_dir": "/credentials"},
        }
        migrated = splined.migrate_v4_config(config)
        self.assertEqual(migrated["config_version"], 5)
        self.assertIn("source_policies", migrated)
        self.assertIn("logging", migrated)
        self.assertIn("history", migrated)

    def test_musicbrainz_options_and_unknown_fields_survive_atomic_update(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "musicbrainz.json"
            original = {
                "client_id": "fixture-client",
                "refresh_token": "fixture-refresh",
                "future_field": {"keep": True},
                "options": {
                    "retry_max": 8,
                    "min_delay": 1.25,
                    "recording_timeout": 11,
                },
            }
            splined.save_json_atomic(path, original)
            loaded = splined.load_json(path, "MusicBrainz")
            loaded["access_token"] = "fixture-access"
            splined.save_json_atomic(path, loaded)
            saved = splined.load_json(path, "MusicBrainz")
            self.assertEqual(saved["refresh_token"], "fixture-refresh")
            self.assertEqual(saved["options"], original["options"])
            self.assertEqual(saved["future_field"], {"keep": True})


if __name__ == "__main__":
    unittest.main()
