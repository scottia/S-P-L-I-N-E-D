"""Safety and contract tests for incremental SQLite reconciliation."""

from __future__ import annotations

import os
import sqlite3
import unittest
from contextlib import closing
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import patch

from webgui.reconciliation import (
    SQLiteReconciler,
    _normalize_sqlite_row,
    classify_difference,
    map_library_path,
)


class SQLiteReconciliationTests(unittest.TestCase):
    @staticmethod
    def _artist(
        key: str,
        name: str,
        mbid: str,
        path: str,
        updated_at: str = "2026-01-01T00:00:00Z",
    ) -> dict[str, str]:
        return {
            "artist_key": key,
            "artist_name": name,
            "musicbrainz_artistid": mbid,
            "primary_path": path,
            "updated_at": updated_at,
        }

    def test_windows_snapshot_paths_require_and_apply_explicit_root_mapping(self):
        mapped = map_library_path(
            r"D:\Music\Artist\Album\01.flac",
            r"D:\Music",
            r"E:\WebGUI Music",
        )
        normalized = mapped.replace("\\", "/")
        self.assertTrue(normalized.endswith("WebGUI Music/Artist/Album/01.flac"))
        with self.assertRaisesRegex(ValueError, "outside the mapped source"):
            map_library_path(r"D:\Other\01.flac", r"D:\Music", r"E:\Music")

    def test_recalculation_excludes_comp_fallback_from_artist_rollup(self):
        with closing(sqlite3.connect(":memory:")) as db:
            db.row_factory = sqlite3.Row
            db.execute(
                "CREATE TABLE artists (artist_key TEXT PRIMARY KEY,status TEXT,"
                "album_count INTEGER,unprocessed_count INTEGER,processed_count INTEGER,"
                "bypassed_count INTEGER,timeout_count INTEGER)"
            )
            db.execute(
                "CREATE TABLE albums (album_key TEXT PRIMARY KEY,artist_key TEXT,path TEXT,"
                "compilation INTEGER,musicbrainz_albumid TEXT,processed_at TEXT,"
                "selected_source TEXT,bypassed INTEGER,timeout_until TEXT,status TEXT)"
            )
            db.execute(
                "CREATE TABLE compilation_album_progress (album_path TEXT PRIMARY KEY,"
                "total_tracks INTEGER,completed_tracks INTEGER,status TEXT)"
            )
            db.execute(
                "INSERT INTO artists VALUES ('artist','partial',2,1,1,0,0)"
            )
            db.executemany(
                "INSERT INTO albums VALUES (?,?,?,?,?,?,?,?,?,?)",
                [
                    ("owned", "artist", "/music/Artist/Album", 0, "release", "now", "local", 0, "", "processed"),
                    ("comp", "artist", "/music/Various/COMP", 1, "", "", "", 0, "", "incomplete"),
                ],
            )
            db.execute(
                "INSERT INTO compilation_album_progress VALUES "
                "('/music/Various/COMP',100,29,'incomplete')"
            )

            SQLiteReconciler._recalculate(db)
            artist = db.execute(
                "SELECT status,album_count,unprocessed_count,processed_count "
                "FROM artists WHERE artist_key='artist'"
            ).fetchone()
            albums = {
                row["album_key"]: row["status"]
                for row in db.execute("SELECT album_key,status FROM albums")
            }

        self.assertEqual(tuple(artist), ("complete", 1, 0, 1))
        self.assertEqual(albums, {"owned": "processed", "comp": "incomplete"})

    def test_production_bom_musicbrainz_identities_normalize_without_changing_metadata(self):
        mbid = "b9a06530-1241-4162-836f-7b8e79deaa58"
        row = _normalize_sqlite_row(
            {
                "artist_key": "mbid:\ufeff" + mbid,
                "artist_name": "\ufeffD-Rop Dow 2",
                "musicbrainz_artistid": "\ufeff" + mbid,
            }
        )
        self.assertEqual(row["artist_key"], "mbid:" + mbid)
        self.assertEqual(row["musicbrainz_artistid"], mbid)
        self.assertEqual(row["artist_name"], "\ufeffD-Rop Dow 2")
        self.assertEqual(
            row["_reconciliation_original_identity_values"],
            {
                "artist_key": "mbid:\ufeff" + mbid,
                "musicbrainz_artistid": "\ufeff" + mbid,
            },
        )

    def test_production_bom_alias_pair_coalesces_when_identity_is_identical(self):
        mbid = "6958e806-1e14-4844-a0a7-07a27756aa0f"
        clean = _normalize_sqlite_row(
            {
                "artist_key": "mbid:" + mbid,
                "artist_name": "A Great Big World",
                "musicbrainz_artistid": mbid,
            }
        )
        bom = _normalize_sqlite_row(
            {
                "artist_key": "mbid:\ufeff" + mbid,
                "artist_name": "A Great Big World",
                "musicbrainz_artistid": "\ufeff" + mbid,
            }
        )
        aliases = SQLiteReconciler._coalesce_transport_artist_aliases([clean, bom])
        self.assertEqual(len(aliases), 1)
        self.assertEqual(aliases[0]["artist_key"], "mbid:" + mbid)
        self.assertEqual(
            aliases[0]["_reconciliation_transport_aliases"],
            ["mbid:" + mbid, "mbid:\ufeff" + mbid],
        )

    def test_bom_key_collision_with_different_artist_identity_remains_conflict(self):
        mbid = "6958e806-1e14-4844-a0a7-07a27756aa0f"
        clean = _normalize_sqlite_row({
            "artist_key": "mbid:" + mbid,
            "artist_name": "A Great Big World",
            "musicbrainz_artistid": mbid,
            "primary_path": "/music/A Great Big World",
        })
        bom = _normalize_sqlite_row({
            "artist_key": "mbid:\ufeff" + mbid,
            "artist_name": "Different Artist",
            "musicbrainz_artistid": "\ufeff" + mbid,
            "primary_path": "/music/Different Artist",
        })
        aliases = SQLiteReconciler._coalesce_transport_artist_aliases([clean, bom])
        self.assertEqual(len(aliases), 2)
        rows = []
        for row in aliases:
            row["_reconciliation_source_key"] = row["artist_key"]
            rows.append((row, None))
        rejected = SQLiteReconciler._reject_normalized_source_collisions("artists", rows)
        self.assertEqual(rejected, {"mbid:" + mbid})

    def test_album_validation_migrations_do_not_trust_historical_counters(self):
        migrations = Path(__file__).parents[1] / "migrations"
        original = (migrations / "013_album_embedded_validation.sql").read_text(
            encoding="utf-8"
        )
        correction = (
            migrations / "014_reset_projected_album_validation.sql"
        ).read_text(encoding="utf-8")

        self.assertNotIn("UPDATE albums", original)
        self.assertIn("embedded_validation_status = 'unknown'", correction)
        self.assertIn("embedded_validated_at = NULL", correction)
        self.assertIn("embedded_validation_status = 'verified'", correction)
        self.assertNotIn("embedded_tracks =", correction)

    def test_path_backed_track_key_is_mapped_with_track_path(self):
        source = {
            "track_key": r"D:\Music\Adele\30\01.flac",
            "path": r"D:\Music\Adele\30\01.flac",
        }
        mapped = SQLiteReconciler._map_row(
            "tracks", source, r"D:\Music", r"E:\WebGUI Music"
        )
        self.assertEqual(mapped["track_key"], mapped["path"])
        self.assertTrue(
            mapped["track_key"].replace("\\", "/").endswith(
                "WebGUI Music/Adele/30/01.flac"
            )
        )

    def test_track_identity_never_falls_back_to_another_track_by_artist_name(self):
        target = {
            "track_key": r"E:\Music\Billboard\Adele.mp3",
            "path": r"E:\Music\Billboard\Adele.mp3",
            "artist_name": "Adele",
            "album_key": "billboard",
        }
        maps = SQLiteReconciler._match_maps("tracks", [target])
        source = {
            "track_key": r"E:\Music\Adele\30\01.mp3",
            "path": r"E:\Music\Adele\30\01.mp3",
            "artist_name": "Adele",
            "album_key": "30",
        }
        self.assertIsNone(SQLiteReconciler._find_match("tracks", source, maps))

    def test_artist_unique_mbid_precedes_duplicate_name_and_is_order_independent(self):
        first = self._artist(
            "tag:duplicate-one",
            "Duplicate Name",
            "11111111-1111-4111-8111-111111111111",
            r"E:\Music\Duplicate One",
        )
        second = self._artist(
            "tag:duplicate-two",
            "Duplicate Name",
            "22222222-2222-4222-8222-222222222222",
            r"E:\Music\Duplicate Two",
        )
        source = self._artist(
            "mbid:22222222-2222-4222-8222-222222222222",
            "Duplicate Name",
            "22222222-2222-4222-8222-222222222222",
            r"E:\Other\Duplicate Two",
        )
        for rows in ([first, second], [second, first]):
            match, identity = SQLiteReconciler._find_artist_match(
                source, SQLiteReconciler._match_maps("artists", rows)
            )
            self.assertIs(match, second)
            self.assertEqual(identity["evidence"], "musicbrainz-artist-id")

    def test_artist_duplicate_normalized_names_are_ambiguous(self):
        rows = [
            self._artist("tag:first", "AC/DC", "first-mbid", r"E:\Music\First"),
            self._artist("tag:second", "AC DC", "second-mbid", r"E:\Music\Second"),
        ]
        source = self._artist(
            "tag:unmatched",
            "AC-DC",
            "source-mbid",
            r"E:\Music\Unmatched",
        )
        match, identity = SQLiteReconciler._find_artist_match(
            source, SQLiteReconciler._match_maps("artists", rows)
        )
        self.assertIsNone(match)
        self.assertEqual(identity["status"], "ambiguous")
        self.assertEqual(identity["evidence"], "artist-name")
        self.assertEqual(
            [item["artist_key"] for item in identity["candidates"]],
            ["tag:first", "tag:second"],
        )

    def test_artist_keys_do_not_use_lossy_punctuation_or_spacing_normalization(self):
        first = self._artist("tag:ab c", "First", "", r"E:\Music\First")
        second = self._artist("tag:a bc", "Second", "", r"E:\Music\Second")
        maps = SQLiteReconciler._match_maps("artists", [first, second])
        exact, identity = SQLiteReconciler._find_artist_match(
            self._artist("tag:a bc", "Unknown", "", r"E:\Music\Unknown"), maps
        )
        self.assertIs(exact, second)
        self.assertEqual(identity["evidence"], "artist-key")
        lossy, identity = SQLiteReconciler._find_artist_match(
            self._artist("tag:abc", "Unknown", "", r"E:\Music\Unknown"), maps
        )
        self.assertIsNone(lossy)
        self.assertEqual(identity["status"], "unmatched")

    def test_artist_duplicate_exact_key_bucket_is_ambiguous(self):
        rows = [
            self._artist("tag:duplicate", "First", "first-mbid", r"E:\Music\First"),
            self._artist("tag:duplicate", "Second", "second-mbid", r"E:\Music\Second"),
        ]
        match, identity = SQLiteReconciler._find_artist_match(
            self._artist(
                "tag:duplicate", "Source", "source-mbid", r"E:\Music\Source"
            ),
            SQLiteReconciler._match_maps("artists", rows),
        )
        self.assertIsNone(match)
        self.assertEqual(identity["status"], "ambiguous")
        self.assertEqual(identity["evidence"], "artist-key")

    def test_artist_duplicate_mbid_can_be_disambiguated_by_unique_path(self):
        first = self._artist(
            "tag:first", "First", "shared-mbid", r"E:\Music\First"
        )
        second = self._artist(
            "tag:second", "Second", "shared-mbid", r"E:\Music\Second"
        )
        source = self._artist(
            "mbid:shared-mbid", "Second", "shared-mbid", r"e:/music/second/"
        )
        match, identity = SQLiteReconciler._find_artist_match(
            source, SQLiteReconciler._match_maps("artists", [first, second])
        )
        self.assertIs(match, second)
        self.assertEqual(identity["evidence"], "primary-path")

    def test_artist_unique_primary_path_proves_conflicting_mbid_identity(self):
        target = self._artist(
            "tag:target",
            "Correct Artist",
            "stale-target-mbid",
            r"E:\Music\Correct Artist",
        )
        source = self._artist(
            "mbid:authoritative-source-mbid",
            "Correct Artist",
            "authoritative-source-mbid",
            r"e:/music/correct artist/",
            "2026-01-02T00:00:00Z",
        )
        match, identity = SQLiteReconciler._find_artist_match(
            source, SQLiteReconciler._match_maps("artists", [target])
        )
        self.assertIs(match, target)
        self.assertEqual(identity["evidence"], "primary-path")
        projected = dict(source, artist_key=target["artist_key"])
        self.assertEqual(classify_difference("artists", projected, target), "safe")

    def test_distinct_source_mbid_does_not_consume_an_incumbent_source_artist(self):
        incumbent = self._artist(
            "mbid:incumbent-mbid",
            "Adam and the Ants",
            "incumbent-mbid",
            r"E:\Music\Adam Ant",
        )
        distinct = self._artist(
            "mbid:distinct-mbid",
            "Adam Ant",
            "distinct-mbid",
            r"E:\Music\Adam Ant",
            "2026-01-02T00:00:00Z",
        )
        match, identity = SQLiteReconciler._find_artist_match(
            distinct,
            SQLiteReconciler._match_maps("artists", [incumbent]),
            authoritative_source_keys={
                "mbid:incumbent-mbid",
                "mbid:distinct-mbid",
            },
        )
        self.assertIsNone(match)
        self.assertEqual(identity["status"], "unmatched")
        self.assertEqual(identity["candidate_count"], 0)

    def test_conflicting_mbid_can_still_correct_a_stale_path_only_target(self):
        stale_target = self._artist(
            "mbid:stale-mbid",
            "Adam Ant",
            "stale-mbid",
            r"E:\Music\Adam Ant",
        )
        authoritative = self._artist(
            "mbid:authoritative-mbid",
            "Adam Ant",
            "authoritative-mbid",
            r"E:\Music\Adam Ant",
            "2026-01-02T00:00:00Z",
        )
        match, identity = SQLiteReconciler._find_artist_match(
            authoritative,
            SQLiteReconciler._match_maps("artists", [stale_target]),
            authoritative_source_keys={"mbid:authoritative-mbid"},
        )
        self.assertEqual(match, stale_target)
        self.assertEqual(identity["status"], "matched")
        self.assertEqual(identity["evidence"], "primary-path")

    def test_artist_exact_key_cannot_override_conflicting_mbid_without_path_proof(self):
        target = self._artist(
            "mbid:authoritative",
            "Key Match",
            "stale-target-mbid",
            r"E:\Music\Target",
        )
        source = self._artist(
            "mbid:authoritative",
            "Key Match",
            "authoritative",
            r"E:\Music\Different",
            "2026-01-02T00:00:00Z",
        )
        match, identity = SQLiteReconciler._find_artist_match(
            source, SQLiteReconciler._match_maps("artists", [target])
        )
        self.assertIsNone(match)
        self.assertEqual(identity["status"], "conflict")
        self.assertEqual(identity["evidence"], "artist-key")

        source["primary_path"] = r"e:/music/target/"
        match, identity = SQLiteReconciler._find_artist_match(
            source, SQLiteReconciler._match_maps("artists", [target])
        )
        self.assertIs(match, target)
        self.assertEqual(identity["evidence"], "artist-key+primary-path")

    def test_artist_shared_primary_path_is_ambiguous(self):
        rows = [
            self._artist("tag:first", "First", "first-mbid", r"E:\Music\Shared"),
            self._artist("tag:second", "Second", "second-mbid", r"E:\Music\Shared"),
        ]
        source = self._artist(
            "tag:unmatched",
            "Unmatched",
            "source-mbid",
            r"e:/music/shared/",
        )
        match, identity = SQLiteReconciler._find_artist_match(
            source, SQLiteReconciler._match_maps("artists", rows)
        )
        self.assertIsNone(match)
        self.assertEqual(identity["status"], "ambiguous")
        self.assertEqual(identity["evidence"], "primary-path")

    def test_new_authoritative_mbid_artist_can_insert_from_shared_category_path(self):
        rows = [
            self._artist("mbid:first-mbid", "First Composer", "first-mbid", r"E:\Music\[Soundtracks]"),
            self._artist("mbid:second-mbid", "Second Composer", "second-mbid", r"E:\Music\[Soundtracks]"),
        ]
        source = self._artist(
            "mbid:new-soundtrack-mbid",
            "New Soundtrack Composer",
            "new-soundtrack-mbid",
            r"E:\Music\[Soundtracks]",
        )
        match, identity = SQLiteReconciler._find_artist_match(
            source, SQLiteReconciler._match_maps("artists", rows)
        )
        self.assertIsNone(match)
        self.assertEqual(identity["status"], "unmatched")
        self.assertEqual(identity["candidate_count"], 0)

    def test_artist_name_only_conflicting_mbids_remain_a_conflict(self):
        target = self._artist(
            "tag:target",
            "Same Name",
            "target-mbid",
            r"E:\Music\Target",
        )
        source = self._artist(
            "mbid:source-mbid",
            "Same Name",
            "source-mbid",
            r"E:\Music\Source",
            "2026-01-02T00:00:00Z",
        )
        match, identity = SQLiteReconciler._find_artist_match(
            source, SQLiteReconciler._match_maps("artists", [target])
        )
        self.assertIsNone(match)
        self.assertEqual(identity["status"], "conflict")
        self.assertEqual(
            identity["reason"], "conflicting-non-empty-musicbrainz-artist-ids"
        )

    def test_artist_source_mbid_can_fill_unique_unidentified_target(self):
        target = self._artist(
            "tag:identified-later",
            "Identified Later",
            "",
            r"E:\Music\Identified Later",
        )
        source = self._artist(
            "mbid:new-authority",
            "Identified Later",
            "new-authority",
            r"E:\Other\Identified Later",
            "2026-01-02T00:00:00Z",
        )
        match, identity = SQLiteReconciler._find_artist_match(
            source, SQLiteReconciler._match_maps("artists", [target])
        )
        self.assertIs(match, target)
        self.assertEqual(identity["evidence"], "artist-name")
        projected = dict(source, artist_key=target["artist_key"])
        self.assertEqual(classify_difference("artists", projected, target), "safe")

    def test_artist_ambiguous_diagnostic_is_stable_when_rows_are_reversed(self):
        rows = [
            self._artist("tag:z", "Same Name", "z-mbid", r"E:\Music\Z"),
            self._artist("tag:a", "Same Name", "a-mbid", r"E:\Music\A"),
        ]
        source = self._artist(
            "tag:source", "Same Name", "source-mbid", r"E:\Music\Source"
        )
        outcomes = []
        for candidates in (rows, list(reversed(rows))):
            match, identity = SQLiteReconciler._find_artist_match(
                source, SQLiteReconciler._match_maps("artists", candidates)
            )
            self.assertIsNone(match)
            outcomes.append(identity)
        self.assertEqual(outcomes[0], outcomes[1])

    def test_snapshot_fingerprint_ignores_sqlite_read_sidecars(self):
        with TemporaryDirectory() as directory:
            snapshot = Path(directory) / "snapshot.sqlite"
            snapshot.write_bytes(b"standalone snapshot")
            before = SQLiteReconciler._fingerprint(snapshot)
            Path(str(snapshot) + "-shm").write_bytes(b"reader state")
            Path(str(snapshot) + "-wal").write_bytes(b"")
            self.assertEqual(SQLiteReconciler._fingerprint(snapshot), before)

    def test_sqlite_source_normalizes_nul_separated_metadata_at_read_boundary(self):
        with closing(sqlite3.connect(":memory:")) as source:
            source.row_factory = sqlite3.Row
            source.execute(
                "CREATE TABLE artists (artist_key TEXT,artist_name TEXT,"
                "musicbrainz_artistid TEXT,primary_path TEXT)"
            )
            raw_name = "Mariah Carey\0Stefflon Don"
            raw_ids = "494e8d09-f85b-4543-892f-a5096aed1cd4\0c5a73e97-82ff-4b97-a9f7-49b1f799cb87"
            source.execute(
                "INSERT INTO artists VALUES (?,?,?,?)",
                ("mbid:" + raw_ids, raw_name, raw_ids, r"D:\Music\Mariah Carey"),
            )

            rows = SQLiteReconciler._read_source(source, "artists", {"artists"})

            self.assertEqual(rows[0]["artist_name"], "Mariah Carey; Stefflon Don")
            self.assertEqual(
                rows[0]["musicbrainz_artistid"],
                "494e8d09-f85b-4543-892f-a5096aed1cd4; "
                "c5a73e97-82ff-4b97-a9f7-49b1f799cb87",
            )
            self.assertNotIn("\0", rows[0]["artist_key"])
            self.assertIn(
                "\0",
                source.execute("SELECT artist_name FROM artists").fetchone()[0],
            )

    def test_album_without_mbid_can_match_only_within_its_artist_identity(self):
        target = {
            "album_key": "target-key",
            "artist_key": "adele",
            "album_name": "30",
            "path": r"E:\Music\Adele\30",
        }
        maps = SQLiteReconciler._match_maps("albums", [target])
        self.assertIs(
            SQLiteReconciler._find_match(
                "albums",
                {
                    "album_key": "source-key",
                    "artist_key": "adele",
                    "album_name": "30",
                    "path": r"E:\Other\30",
                },
                maps,
            ),
            target,
        )
        self.assertIsNone(
            SQLiteReconciler._find_match(
                "albums",
                {
                    "album_key": "other-source-key",
                    "artist_key": "other-artist",
                    "album_name": "30",
                    "path": r"E:\Other\Other Artist\30",
                },
                maps,
            )
        )

    def test_windows_catalog_authority_does_not_depend_on_postgresql_timestamp(self):
        newer = {"updated_at": "2026-01-02T00:00:00+00:00"}
        older = {"updated_at": "2026-01-01T00:00:00+00:00"}
        self.assertEqual(classify_difference("albums", newer, older), "safe")
        self.assertEqual(classify_difference("albums", older, newer), "safe")
        self.assertEqual(classify_difference("artists", newer, None), "safe")
        self.assertEqual(classify_difference("albums", newer, None), "safe")
        self.assertEqual(classify_difference("tracks", newer, None), "safe")
        self.assertEqual(classify_difference("cache_entries", newer, None), "safe")
        rejected = dict(
            newer,
            _reconciliation_identity={
                "status": "rejected",
                "reason": "mapped-file-not-found",
            },
        )
        self.assertEqual(classify_difference("tracks", rejected, None), "conflict")
        self.assertEqual(
            classify_difference("compilation_album_progress", newer, older),
            "derived",
        )

    def test_windows_embed_queue_completion_projects_webgui_embedded_count(self):
        mapped = SQLiteReconciler._map_row(
            "albums",
            {
                "album_key": "album",
                "path": r"D:\Music\Artist\Album",
                "status": "processed",
                "processed_at": "2026-01-02T00:00:00Z",
                "selected_source": "embed-queue",
                "track_count": 10,
                "compilation": 0,
            },
            r"D:\Music",
            r"E:\WebGUI Music",
        )
        self.assertEqual(mapped["embedded_tracks"], 10)

    def test_identified_compilation_projects_normal_album_completion(self):
        mapped = SQLiteReconciler._map_row(
            "albums",
            {
                "album_key": "identified-compilation",
                "path": r"D:\Music\Aaron Neville\Gold",
                "musicbrainz_albumid": "308a6440-3c1c-448c-85ce-84b7e53cc756",
                "status": "processed",
                "processed_at": "2026-11-02T05:58:16Z",
                "selected_source": "embed-queue",
                "track_count": 36,
                "compilation": 1,
            },
            r"D:\Music",
            r"E:\WebGUI Music",
        )
        self.assertEqual(mapped["embedded_tracks"], 36)

    def test_unidentified_compilation_keeps_comp_track_authority(self):
        mapped = SQLiteReconciler._map_row(
            "albums",
            {
                "album_key": "hidden-comp",
                "path": r"D:\Music\Various Artists\Hidden COMP",
                "musicbrainz_albumid": "",
                "status": "processed",
                "processed_at": "2026-11-02T05:58:16Z",
                "selected_source": "embed-queue",
                "track_count": 36,
                "compilation": 1,
            },
            r"D:\Music",
            r"E:\WebGUI Music",
        )
        self.assertNotIn("embedded_tracks", mapped)

    def test_windows_album_authority_is_safe_after_identity_is_proven(self):
        source = {
            "album_key": "album",
            "processed_at": "2026-01-02T00:00:00Z",
            "selected_source": "embed-queue",
            "track_count": 10,
            "embedded_tracks": 10,
            "timeout_until": None,
            "updated_at": "2026-01-02T00:00:00Z",
        }
        target = dict(source, embedded_tracks=0, timeout_until="")
        self.assertEqual(classify_difference("albums", source, target), "safe")
        self.assertEqual(
            classify_difference(
                "albums",
                source,
                dict(target, updated_at="2026-01-03T00:00:00Z"),
            ),
            "safe",
        )
        self.assertEqual(
            classify_difference("albums", source, dict(target, cover_found=0)),
            "safe",
        )

    def test_duplicate_album_and_track_candidates_never_use_first_row(self):
        albums = [
            {
                "album_key": "first",
                "artist_key": "artist",
                "album_name": "Same",
                "musicbrainz_albumid": "duplicate",
                "path": r"E:\Music\First",
            },
            {
                "album_key": "second",
                "artist_key": "artist",
                "album_name": "Same",
                "musicbrainz_albumid": "duplicate",
                "path": r"E:\Music\Second",
            },
        ]
        source_album = {
            "album_key": "source",
            "artist_key": "artist",
            "album_name": "Same",
            "musicbrainz_albumid": "duplicate",
            "path": r"E:\Music\Source",
        }
        for rows in (albums, list(reversed(albums))):
            match, identity = SQLiteReconciler._find_entity_match(
                "albums", source_album, SQLiteReconciler._match_maps("albums", rows)
            )
            self.assertIsNone(match)
            self.assertEqual(identity["status"], "ambiguous")
            self.assertEqual(identity["evidence"], "musicbrainz-album-id")

        tracks = [
            {
                "track_key": "first",
                "album_key": "album",
                "musicbrainz_recordingid": "recording",
                "path": r"E:\Music\First.flac",
            },
            {
                "track_key": "second",
                "album_key": "album",
                "musicbrainz_recordingid": "recording",
                "path": r"E:\Music\Second.flac",
            },
        ]
        source_track = {
            "track_key": "source",
            "album_key": "album",
            "musicbrainz_recordingid": "recording",
            "path": r"E:\Music\Source.flac",
        }
        match, identity = SQLiteReconciler._find_entity_match(
            "tracks", source_track, SQLiteReconciler._match_maps("tracks", tracks)
        )
        self.assertIsNone(match)
        self.assertEqual(identity["status"], "ambiguous")
        self.assertEqual(identity["evidence"], "recording-and-album")

    def test_compound_primary_key_never_matches_on_first_component_only(self):
        targets = [
            {"recording_mbid": "recording", "release_mbid": "release-one"},
            {"recording_mbid": "recording", "release_mbid": "release-two"},
        ]
        source = {"recording_mbid": "recording", "release_mbid": "release-three"}
        match, identity = SQLiteReconciler._find_entity_match(
            "recording_release_candidates",
            source,
            SQLiteReconciler._match_maps(
                "recording_release_candidates", targets
            ),
        )
        self.assertIsNone(match)
        self.assertEqual(identity["status"], "unmatched")

    def test_multiple_windows_rows_cannot_converge_on_one_target(self):
        target = {
            "artist_key": "target",
            "artist_name": "Target",
            "musicbrainz_artistid": "target-mbid",
            "primary_path": r"E:\Music\Target",
        }
        first = {
            "artist_key": "target",
            "_reconciliation_source_key": "source-one",
        }
        second = {
            "artist_key": "target",
            "_reconciliation_source_key": "source-two",
        }
        rejected = SQLiteReconciler._reject_source_fan_in(
            "artists", [(first, target), (second, target)]
        )
        self.assertEqual(rejected, {"source-one", "source-two"})
        for source in (first, second):
            identity = source["_reconciliation_identity"]
            self.assertEqual(identity["status"], "conflict")
            self.assertEqual(
                identity["reason"],
                "multiple-windows-entities-map-to-one-postgresql-entity",
            )
            self.assertEqual(identity["source_keys"], ["source-one", "source-two"])

    def test_controlled_insert_requires_complete_rows_parent_and_mapped_paths(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            artist = root / "Artist"
            album = artist / "Album"
            album.mkdir(parents=True)
            track = album / "01.flac"
            track.write_bytes(b"fixture")
            timestamp = "2026-01-01T00:00:00Z"
            artist_row = {
                "artist_key": "artist",
                "artist_name": "Artist",
                "primary_path": str(artist),
                "created_at": timestamp,
                "updated_at": timestamp,
                "last_seen_at": timestamp,
                "splined_version": "test",
            }
            album_row = {
                "album_key": "album",
                "artist_key": "artist",
                "album_name": "Album",
                "path": str(album),
                "tag_signature": "signature",
                "created_at": timestamp,
                "updated_at": timestamp,
                "last_seen_at": timestamp,
                "splined_version": "test",
            }
            track_row = {
                "track_key": "track",
                "album_key": "album",
                "path": str(track),
                "title": "Track",
                "artist_name": "Artist",
                "file_size": 7,
                "file_mtime_ns": 1,
                "updated_at": timestamp,
                "splined_version": "test",
            }
            reconciler = SQLiteReconciler(
                object(), root, root, start_scheduler=False
            )
            try:
                self.assertEqual(
                    reconciler._insert_identity("artists", artist_row)["status"],
                    "insertable",
                )
                self.assertEqual(
                    reconciler._insert_identity("albums", album_row)["status"],
                    "insertable",
                )
                self.assertEqual(
                    reconciler._insert_identity("tracks", track_row)["status"],
                    "insertable",
                )
                rejected = reconciler._insert_identity(
                    "tracks", {"path": str(album / "missing.flac")}
                )
                self.assertEqual(rejected["status"], "rejected")
                self.assertEqual(rejected["reason"], "required-insert-fields-missing")
                outside = dict(track_row, path=str(root.parent / "outside.flac"))
                rejected = reconciler._insert_identity("tracks", outside)
                self.assertEqual(rejected["status"], "rejected")
                self.assertEqual(
                    rejected["reason"], "mapped-path-outside-target-library"
                )
                blocked = reconciler._insert_identity(
                    "tracks", track_row, parent_ready=False
                )
                self.assertEqual(blocked["status"], "blocked")
            finally:
                reconciler.close()

    def test_authorized_source_inventory_is_bounded_to_cache_database_files(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            snapshot = root / "snapshot.sqlite"
            with closing(sqlite3.connect(snapshot)) as database, database:
                database.execute("PRAGMA user_version=2")
            (root / "notes.txt").write_text("not a database", encoding="utf-8")
            reconciler = SQLiteReconciler(
                object(), root, root / "music", start_scheduler=False
            )
            try:
                self.assertEqual(
                    [item["label"] for item in reconciler.sources()],
                    ["snapshot.sqlite"],
                )
                self.assertEqual(reconciler._source(snapshot), snapshot.resolve())
                with TemporaryDirectory() as outside:
                    other = Path(outside) / "outside.db"
                    other.touch()
                    with self.assertRaises(PermissionError):
                        reconciler._source(other)
            finally:
                reconciler.close()

    def test_dedicated_snapshot_root_is_authorized_without_widening_filesystem_access(self):
        with TemporaryDirectory() as cache_directory, TemporaryDirectory() as snapshot_directory, TemporaryDirectory() as outside_directory:
            cache_root = Path(cache_directory)
            snapshot_root = Path(snapshot_directory)
            cache_snapshot = cache_root / "older-cache-copy.db"
            cache_snapshot.touch()
            snapshot = snapshot_root / "splined.db"
            with closing(sqlite3.connect(snapshot)) as database, database:
                database.execute("PRAGMA user_version=2")
            outside = Path(outside_directory) / "outside.db"
            outside.touch()
            reconciler = SQLiteReconciler(
                object(),
                cache_root,
                cache_root / "music",
                snapshot_roots=(snapshot_root,),
                start_scheduler=False,
            )
            try:
                sources = reconciler.sources()
                self.assertEqual(len(sources), 1)
                self.assertEqual(sources[0]["path"], str(snapshot.resolve()))
                self.assertEqual(
                    sources[0]["label"],
                    str(Path(snapshot_root.name) / "splined.db"),
                )
                self.assertEqual(sources[0]["bytes"], snapshot.stat().st_size)
                self.assertNotIn(str(cache_snapshot.resolve()), {item["path"] for item in sources})
                self.assertEqual(reconciler._source(snapshot), snapshot.resolve())
                self.assertEqual(
                    reconciler._source(cache_snapshot),
                    cache_snapshot.resolve(),
                )
                with self.assertRaises(PermissionError):
                    reconciler._source(outside)
            finally:
                reconciler.close()

    def test_snapshot_root_environment_is_explicit_and_container_default_is_bounded(self):
        from webgui.server import _sqlite_snapshot_roots

        with TemporaryDirectory() as directory, patch.dict(
            os.environ,
            {"SPLINED_SQLITE_SNAPSHOT_ROOT": directory},
        ):
            self.assertEqual(_sqlite_snapshot_roots(), (Path(directory),))

    def test_migration_has_exact_history_name_idempotence_key_and_disabled_schedule(self):
        migration = (
            Path(__file__).parents[1] / "migrations" / "010_sqlite_reconciliation.sql"
        ).read_text(encoding="utf-8")
        self.assertIn("CREATE TABLE salite_diff", migration)
        self.assertIn("UNIQUE (\n        source_fingerprint", migration)
        self.assertIn("enabled BOOLEAN NOT NULL DEFAULT FALSE", migration)
        self.assertIn("auto_apply_safe BOOLEAN NOT NULL DEFAULT FALSE", migration)
        startup = (
            Path(__file__).parents[1] / "migrations" / "012_reconciliation_startup_mode.sql"
        ).read_text(encoding="utf-8")
        self.assertIn("startup_mode TEXT NOT NULL DEFAULT 'off'", startup)
        self.assertIn("'compare', 'apply-safe'", startup)
        diagnostics = (
            Path(__file__).parents[1]
            / "migrations"
            / "015_reconciliation_table_counts.sql"
        ).read_text(encoding="utf-8")
        self.assertIn("ADD COLUMN table_counts JSONB", diagnostics)

    def test_ui_and_apply_contract_never_offer_conflict_application(self):
        root = Path(__file__).parents[1]
        operations = (root / "static" / "operations.js").read_text(encoding="utf-8")
        reconciliation = (root / "reconciliation.py").read_text(encoding="utf-8")
        self.assertIn("Compare / Dry Run", operations)
        self.assertIn("Save Snapshot & Paths", operations)
        self.assertIn("Saved SQLite snapshot and library path mapping", operations)
        self.assertIn("Apply Safe Differences", operations)
        self.assertIn("Auto SNAP on start", operations)
        self.assertIn('input.type = "radio"', operations)
        self.assertIn("Automatically apply safe differences only", operations)
        self.assertIn("Choose an authorized snapshot", operations)
        self.assertIn("not the folder containing the .db file", operations)
        self.assertIn("resolution='safe'", reconciliation)
        self.assertIn('source.as_uri() + "?mode=ro&immutable=1"', reconciliation)
        self.assertIn('"conflicts_applied": 0', reconciliation)
        reconciled_tables = reconciliation.split("table_names = [", 1)[1].split("]", 1)[0]
        self.assertNotIn("web_jobs", reconciled_tables)


if __name__ == "__main__":
    unittest.main()
