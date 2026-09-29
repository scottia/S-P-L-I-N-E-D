from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import splined_compilation_authority_policy as policy


class CompilationAuthorityPolicyTests(unittest.TestCase):
    def setUp(self) -> None:
        policy._RECORDING_RELEASE_CACHE.clear()
        policy._REFERENCE_RELEASE_IDS.clear()
        policy._REFERENCE_RELEASES.clear()
        policy._INSTALLED = False

    def test_sql_represented_release_is_preferred(self) -> None:
        core = SimpleNamespace(
            same_text=lambda left, right: left.casefold() == right.casefold(),
            normalize_text=lambda value: str(value).casefold(),
        )
        releases = [
            {
                "id": "release-other",
                "title": "Compilation Edition",
                "status": "Official",
                "artist-credit": [{"name": "50 Cent"}],
                "release-group": {
                    "primary-type": "Album",
                    "secondary-types": ["Compilation"],
                },
                "_recording_artist": "50 Cent",
            },
            {
                "id": "release-sql",
                "title": "Get Rich or Die Tryin'",
                "status": "Official",
                "artist-credit": [{"name": "50 Cent"}],
                "release-group": {
                    "primary-type": "Album",
                    "secondary-types": [],
                },
                "_recording_artist": "50 Cent",
            },
        ]
        with patch.object(
            policy,
            "_database_release_ids",
            return_value={"release-sql"},
        ):
            chosen, in_sql = policy._choose_reference_release(
                core,
                Path("/config/config.toml"),
                {},
                releases,
                track_artist="50 Cent",
            )
        self.assertIsNotNone(chosen)
        self.assertEqual(chosen["id"], "release-sql")
        self.assertTrue(in_sql)

    def test_non_compilation_is_not_recovered(self) -> None:
        track = SimpleNamespace(
            compilation="0",
            album_mbid=None,
            recording_mbid="00000000-0000-0000-0000-000000000001",
            artist="Artist",
        )
        core = SimpleNamespace()
        self.assertIsNone(policy._recover_compilation_reference(core, [track]))

    def test_install_injects_run_local_reference_and_uses_exact_discovery(self) -> None:
        track = SimpleNamespace(
            compilation="1",
            album_mbid=None,
            recording_mbid="recording-id",
            artist="50 Cent",
        )
        album = SimpleNamespace(path=Path("/music/[Various Artists]/Compilation"))
        release = SimpleNamespace(
            mbid="release-sql",
            artist_credit="50 Cent",
            title="Get Rich or Die Tryin'",
        )
        calls: list[str] = []

        core = SimpleNamespace(
            read_album_tracks=lambda _album: [track],
            lookup_release=lambda _http, _config, _cfg, _mbid: release,
            discover_fallback=lambda *_args, **_kwargs: ([], [("fallback", "used")]),
            discover_all=lambda *_args, **_kwargs: (["exact"], []),
            debug_log=lambda message: calls.append(message),
            emit_ui=lambda *_args, **_kwargs: None,
        )

        with patch.object(
            policy,
            "_recover_compilation_reference",
            return_value=("release-sql", track, True),
        ):
            policy.install(core)
            tracks = core.read_album_tracks(album)

        self.assertEqual(tracks[0].album_mbid, "release-sql")
        resolved = core.lookup_release(None, Path("/config/config.toml"), {}, "release-sql")
        self.assertIs(resolved, release)
        refs, diagnostics = core.discover_fallback(
            None,
            Path("/config/config.toml"),
            {},
            "Various Artists",
            "Compilation",
            ["itunes"],
            release_mbid="release-sql",
        )
        self.assertEqual(refs, ["exact"])
        self.assertEqual(diagnostics, [])
        self.assertTrue(any("reference_recovered" in value for value in calls))


if __name__ == "__main__":
    unittest.main()
