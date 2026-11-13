"""Focused regressions for the reported WebGUI processing defects."""

from __future__ import annotations

from contextlib import closing
from pathlib import Path
from io import BytesIO
import sqlite3
from tempfile import TemporaryDirectory
from types import SimpleNamespace
from threading import Lock
from unittest.mock import patch
import unittest

from webgui.services import Services
from webgui.catalog import Catalog
from webgui.imaging import framed
from PIL import Image


class _SqliteDatabase:
    def __init__(self, path: Path):
        self.path = path

    def connect(self):
        connection = sqlite3.connect(self.path)
        connection.row_factory = sqlite3.Row
        return connection


class ReportedDefectTests(unittest.TestCase):
    def test_container_context_includes_settings_contract(self):
        repository = Path(__file__).parents[2]
        dockerignore = (repository / ".dockerignore").read_text(encoding="utf-8")
        dockerfile = (repository / "webgui" / "Dockerfile").read_text(encoding="utf-8")

        self.assertIn("COPY schema/media.sql schema/settings-ui.json /app/schema/", dockerfile)
        self.assertIn("!schema/settings-ui.json", dockerignore.splitlines())

    def test_artist_metadata_projects_only_genuine_provider_similarity_with_local_image(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            credentials = root / "credentials"
            credentials.mkdir()
            (credentials / "navidrome.json").write_text(
                '{"server_url":"https://navidrome.invalid","auth_method":"api_key","api_key":"secret"}',
                encoding="utf-8",
            )
            database = root / "catalog.sqlite"
            with closing(sqlite3.connect(database)) as db:
                db.execute(
                    "CREATE TABLE artists (artist_key TEXT,artist_name TEXT,musicbrainz_artistid TEXT,primary_path TEXT)"
                )
                db.executemany(
                    "INSERT INTO artists VALUES (?,?,?,?)",
                    [("selected", "Selected Artist", "", str(root)), ("similar", "Real Similar", "", str(root))],
                )
                db.commit()
            portrait = root / "similar.png"
            Image.new("RGB", (16, 16), "green").save(portrait)

            class Response:
                status_code = 200

                def __init__(self, payload):
                    self.payload = payload

                def json(self):
                    return {"subsonic-response": {"status": "ok", **self.payload}}

            requests_seen = []

            def response(url, **_kwargs):
                requests_seen.append(url)
                if "search3" in url:
                    return Response({"searchResult3": {"artist": [{"id": "remote-selected", "name": "Selected Artist"}]}})
                if "getArtistInfo2" in url:
                    return Response({"artistInfo2": {"biography": "Provider biography", "similarArtist": [
                        {"id": "remote-similar", "name": "Real Similar"},
                        {"id": "remote-only", "name": "Remote Only", "coverArt": "cover-1"},
                    ]}})
                if "getArtist.view" in url:
                    return Response({"artist": {"genre": "Alternative", "genres": [{"name": "Rock"}]}})
                return Response({"topSongs": {"song": []}})

            catalog = Catalog.__new__(Catalog)
            catalog.credentials = credentials
            catalog.metadata_cache = {}
            catalog.appearance_cache = {}
            catalog.lock = Lock()
            catalog.artist = lambda _artist_id: {"id": "selected", "name": "Selected Artist", "mbid": "", "top_tracks": []}
            catalog.artist_comp_albums = lambda _artist_id: []
            catalog.refresh_artist_appearances = lambda _artist_id: {}
            catalog._stored_artist_profile = lambda _artist_id: None
            catalog.connect = lambda: sqlite3.connect(database)
            original_connect = catalog.connect

            def connect():
                value = original_connect()
                value.row_factory = sqlite3.Row
                return value

            catalog.connect = connect
            catalog.artist_thumbnail_path = lambda row: portrait if row["artist_key"] == "similar" else None
            catalog.image = lambda path: "/api/image/similar" if path else None

            with patch("webgui.catalog.requests.get", side_effect=response):
                result = catalog.artist_metadata("selected")

            self.assertEqual(result["source"], "Navidrome")
            self.assertEqual(
                result["similar"],
                [
                    {"name": "Real Similar", "local_id": "similar", "image": "/api/image/similar", "provider_id": "remote-similar"},
                    {
                        "name": "Remote Only",
                        "local_id": "",
                        "image": "/api/artist/similar-thumbnail?provider_id=remote-only&name=Remote%20Only&cover_art=cover-1",
                        "provider_id": "remote-only",
                    },
                ],
            )
            self.assertEqual(
                [url.rsplit("/", 1)[-1] for url in requests_seen],
                ["search3.view", "getArtistInfo2.view", "getArtist.view"],
                "Artist Overview must not wait for top-song or Similar Artist image requests",
            )
            self.assertEqual(result["artist"]["genres"], ["Alternative", "Rock"])

    def test_artist_metadata_uses_identity_bound_library_genres_when_artist_detail_has_none(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            credentials = root / "credentials"
            credentials.mkdir()
            (credentials / "navidrome.json").write_text(
                '{"server_url":"https://navidrome.invalid","auth_method":"api_key","api_key":"secret"}',
                encoding="utf-8",
            )
            database = root / "catalog.sqlite"
            with closing(sqlite3.connect(database)) as db:
                db.execute(
                    "CREATE TABLE artists (artist_key TEXT,artist_name TEXT,musicbrainz_artistid TEXT,primary_path TEXT)"
                )
                db.execute("INSERT INTO artists VALUES (?,?,?,?)", ("selected", "Selected Artist", "", str(root)))
                db.commit()

            class Response:
                status_code = 200

                def __init__(self, payload):
                    self.payload = payload

                def json(self):
                    return {"subsonic-response": {"status": "ok", **self.payload}}

            def response(url, **_kwargs):
                if "search3" in url:
                    return Response({"searchResult3": {"artist": [{"id": "remote-selected", "name": "Selected Artist"}]}})
                if "getArtistInfo2" in url:
                    return Response({"artistInfo2": {"biography": "Provider biography", "similarArtist": []}})
                if "getArtist.view" in url:
                    return Response({"artist": {}})
                raise AssertionError(url)

            catalog = Catalog.__new__(Catalog)
            catalog.credentials = credentials
            catalog.metadata_cache = {}
            catalog.appearance_cache = {}
            catalog.lock = Lock()
            catalog.artist = lambda _artist_id: {"id": "selected", "name": "Selected Artist", "mbid": "", "top_tracks": [], "genres": []}
            catalog.artist_comp_albums = lambda _artist_id: []
            catalog.refresh_artist_appearances = lambda _artist_id: {"genres": ["Blues", "Soul"]}
            catalog._stored_artist_profile = lambda _artist_id: None
            catalog._save_artist_profile = lambda *_args: None
            catalog.connect = lambda: sqlite3.connect(database)
            original_connect = catalog.connect

            def connect():
                value = original_connect()
                value.row_factory = sqlite3.Row
                return value

            catalog.connect = connect
            catalog.artist_thumbnail_path = lambda _row: None
            catalog.image = lambda _path: None

            with patch("webgui.catalog.requests.get", side_effect=response):
                result = catalog.artist_metadata("selected")

            self.assertEqual(result["artist"]["genres"], ["Blues", "Soul"])

    def test_neutral_banner_fit_preserves_the_complete_source(self):
        source = Image.new("RGBA", (600, 600), "red")
        source.paste(Image.new("RGBA", (300, 600), "blue"), (300, 0))

        banner = framed(source, "banner")

        self.assertEqual(banner.size, (600, 257))
        self.assertEqual(banner.getpixel((0, 128))[3], 0)
        self.assertEqual(banner.getpixel((171, 128))[3], 0)
        self.assertEqual(banner.getpixel((172, 128))[:3], (255, 0, 0))
        self.assertEqual(banner.getpixel((428, 128))[:3], (0, 0, 255))
        self.assertEqual(banner.getpixel((599, 128))[3], 0)

    def test_webgui_formatting_uses_visible_settings_and_configured_artwork_range(self):
        repository = Path(__file__).parents[2]
        stylesheet = (repository / "webgui" / "static" / "app.css").read_text(encoding="utf-8")
        application = (repository / "webgui" / "static" / "app.js").read_text(encoding="utf-8")
        operations = (repository / "webgui" / "static" / "operations.js").read_text(encoding="utf-8")

        self.assertIn("top:64px", stylesheet)
        self.assertIn("resize:both", stylesheet)
        self.assertIn("text-align:justify", stylesheet)
        self.assertIn("font-size:15px", stylesheet)
        self.assertIn("metric-below-minimum", stylesheet)
        self.assertIn("settings.config.range", operations)
        self.assertIn('document.addEventListener("visibilitychange"', operations)
        self.assertIn("state.libraryChanges?.checking", operations)
        self.assertIn("metric: metadata", application)
        self.assertNotIn('"Artist " + state.artist.portrait_meta.width', application)

    def test_cross_client_override_visual_contract_is_presentation_only(self):
        repository = Path(__file__).parents[2]
        static = repository / "webgui" / "static"
        document = (static / "index.html").read_text(encoding="utf-8")
        application = (static / "app.js").read_text(encoding="utf-8")
        operations = (static / "operations.js").read_text(encoding="utf-8")
        styles = (static / "app.css").read_text(encoding="utf-8")

        self.assertIn('id="overrideNotice" class="override-notice" hidden', document)
        self.assertIn('"Overridden by opposing application"', application)
        self.assertIn('"Windows override"', application)
        self.assertIn('"Overridden"', application)
        self.assertIn('albumOverrides: new Map()', application)
        self.assertIn('/api/coordination/album-overrides', operations)
        self.assertIn("async function refreshOpenCompReportAfterCoordination()", operations)
        self.assertIn("await refreshOpenCompReportAfterCoordination();", operations)
        self.assertIn('/api/coordination/comp-track-overrides', operations)
        self.assertIn('/api/artist/comp-report?id=', operations)
        self.assertIn('/api/fallback/tracks?id=', operations)
        self.assertIn(
            ".album-card.windows-card.opposing-override .album-card-picture img"
            "{filter:grayscale(1)}",
            styles,
        )
        self.assertNotIn(".opposing-override{filter:grayscale", styles)
        self.assertIn('const frame = overridden ? "#6b737b"', application)
        self.assertIn(
            'override: {name: "OVERRIDE", background: "#D3D3D3", frame: "#FFFFFF", letters: "#FF344F"}',
            application,
        )
        self.assertIn(
            'albumOverride(album)?.writer === "windows" ? "override" : albumBannerKind(album, artist)',
            application,
        )
        self.assertNotIn('["Missing", "Missing or incomplete", "#ff5268"]', application)
        self.assertIn('detail: "Validating embedded thumbnails"', application)
        self.assertIn('detail: "Processed album; thumbnail check"', application)
        self.assertIn('detail: "COMP tracks done; thumbnail check"', application)
        self.assertIn('["COMP track rows", [', application)
        self.assertIn("grid-template-columns:repeat(4,minmax(0,1fr))", styles)
        self.assertIn(": processed ? frames.in_process.color : mediaStatusColor(status)", application)
        self.assertIn('const staleUnprocessed = overridden && status === "unprocessed"', application)
        self.assertIn('checkEmbedded || status === "partial" ? "partial"', application)
        self.assertIn(': processed || validating ? "processed" : incomplete || missingCover ? "incomplete" : "unprocessed"', application)
        self.assertIn('if (!staleUnprocessed && (missingCover || incomplete))', application)

    def test_comp_report_backfills_only_albums_in_selected_artist_scope(self):
        with TemporaryDirectory() as directory:
            database_path = Path(directory) / "catalog.sqlite"
            with closing(sqlite3.connect(database_path)) as db:
                db.execute("CREATE TABLE albums (album_key TEXT PRIMARY KEY,track_count INTEGER,musicbrainz_albumid TEXT)")
                db.execute("CREATE TABLE tracks (track_key TEXT PRIMARY KEY,album_key TEXT)")
                db.executemany("INSERT INTO albums VALUES (?,?, '')", [("target", 2), ("unrelated", 3)])
                db.commit()

            class Catalog:
                def artist_comp_album_ids(self, artist_id):
                    self.requested = artist_id
                    return ["target"]

                def artist_comp_report(self, artist_id):
                    return [{"id": "target", "tracks": []}]

            services = Services.__new__(Services)
            services.runtime = SimpleNamespace(database=_SqliteDatabase(database_path))
            services.catalog = Catalog()
            hydrated = []
            services.hydrate_tracks = hydrated.append

            report = services.comp_report("artist-1")

            self.assertEqual(report, [{"id": "target", "tracks": []}])
            self.assertEqual(hydrated, ["target"])

    def test_scan_skip_advances_batch_and_reports_progress_without_writing_in_read_mode(self):
        services = Services.__new__(Services)
        services.runtime = SimpleNamespace(cfg={"mode": "read"})
        rows = {
            "one": {"album_key": "one", "album_name": "One", "artist_name": "Artist"},
            "two": {"album_key": "two", "album_name": "Two", "artist_name": "Artist"},
        }
        services.album_row = rows.__getitem__
        services.search_album = lambda album_id: {
            "candidates": [{"image": album_id, "acceptable": True}], "warnings": []
        }
        writes = []
        services.write_cover = writes.append

        class Adapter:
            def __init__(self):
                self.answers = iter(("k", "s"))
                self.progress_values = []
                self.context = {}

            def cancelled(self):
                return False

            def emit(self, _event, _payload):
                pass

            def read(self, _prompt, _context):
                return next(self.answers)

            def progress(self, value):
                self.progress_values.append(value)

        adapter = Adapter()
        result = services.run_scan({"album_ids": ["one", "two"]}, adapter)

        self.assertEqual(result, {"reviewed": 1, "total": 2, "mode": "read"})
        self.assertEqual(writes, [])
        self.assertEqual(adapter.progress_values[0]["completed"], 0)
        self.assertEqual(adapter.progress_values[0]["total"], 2)
        self.assertEqual(adapter.progress_values[-1]["completed"], 2)

    def test_static_workspace_has_structured_artist_information_and_library_change_notice(self):
        static = Path(__file__).parents[1] / "static"
        document = (static / "index.html").read_text(encoding="utf-8")
        application = (static / "app.js").read_text(encoding="utf-8")
        operations = (static / "operations.js").read_text(encoding="utf-8")
        styles = (static / "app.css").read_text(encoding="utf-8")
        native = (Path(__file__).parents[2] / "windows" / "ui" / "app.slint").read_text(
            encoding="utf-8"
        )
        for identity in (
            'id="heroPortrait"',
            'id="artistInfoRows"',
            'id="artistInfoArtist"',
            'id="artistInfoAlbums"',
            'id="artistInfoStatus"',
            'id="artistInfoMbid"',
            'id="artistInfoGenres"',
            'id="artistInfoRoles"',
            'id="artistInfoToggle"',
            'id="refreshArtistInfo"',
            'id="musicbrainzArtistButton"',
        ):
            self.assertIn(identity, document)
        initialize = operations.split("async function initialize", 1)[1].split(
            "function showAuthentication", 1
        )[0]
        self.assertIn("requestLibraryChangeProbe", initialize)
        self.assertIn("void requestLibraryChangeProbe().catch", initialize)
        self.assertIn("async function checkLibraryChanges", operations)
        self.assertIn(
            ".library-row.album-row .library-thumb,.library-row.album-child "
            ".library-thumb{border-radius:50%}",
            styles,
        )
        self.assertIn(
            "border-radius: self.width / 2; background: root.item.kind == \"artist\"",
            native,
        )
        self.assertIn("track.thumbnail", application)
        self.assertIn("COMP status", application)
        self.assertIn('id="artistInfoRows" class="artist-info-rows" aria-label="Selected Artist information" hidden', document)
        self.assertIn(
            'id="artistInfoMbid" target="_blank" rel="noopener noreferrer"',
            document,
        )
        self.assertLess(document.index('id="artistBiography"'), document.index('id="artistInfoRows"'))
        self.assertIn(".artist-info-rows{position:absolute", styles)
        self.assertIn(".hero-portrait{width:86px;height:86px;flex:none;border:2px solid var(--cyan);border-radius:50%", styles)
        self.assertIn("grid-template-columns:minmax(0,1fr)", styles)
        self.assertIn("inset:34px auto auto 0", styles)
        self.assertIn("width:clamp(340px,50%,520px)", styles)
        self.assertIn("grid-template-columns:116px minmax(0,1fr)", styles)
        self.assertIn("width:16px;height:16px", styles)
        self.assertIn("background:color-mix(in srgb,var(--panel) 88%,transparent)", styles)
        self.assertIn(
            ".portrait-launch h1{width:180px;min-height:30px;margin:5px 0 0;"
            "padding:4px 0;background:transparent;color:var(--yellow)",
            styles,
        )
        self.assertIn(
            "state.artistInfoExpanded = !state.artistInfoExpanded;\n"
            "  renderArtistInfoVisibility();",
            application,
        )
        self.assertNotIn(
            "state.artistInfoExpanded = !state.artistInfoExpanded;\n"
            "  renderHero();",
            application,
        )
        self.assertIn(
            'artistMbidLink.href = "https://musicbrainz.org/artist/" + encodeURIComponent(artistMbid)',
            application,
        )
        self.assertIn(".similar-artists{display:flex;justify-content:flex-start;align-items:flex-start;gap:6px", styles)
        self.assertIn(".similar-card{width:72px", styles)
        self.assertIn(".similar-avatar{display:grid;place-items:center;width:58px;height:58px", styles)
        self.assertIn(".similar-status{position:absolute;right:6px;top:43px", styles)
        self.assertIn('id="mediaFilter" class="filter-body" hidden', document)
        self.assertIn('id="mediaFilterToggle" type="button" aria-expanded="false"', document)
        self.assertIn('id="aboutButton"', document)
        self.assertNotIn("state.artists.filter(item => item.id !== artist.id)", application)
        self.assertNotIn("services.hydrate_tracks(id)\n        return services.catalog.album(id)", (Path(__file__).parents[1] / "server.py").read_text(encoding="utf-8"))
        self.assertIn(
            '"similar-artists-unavailable"',
            (Path(__file__).parents[1] / "server.py").read_text(encoding="utf-8"),
        )

    def test_explicit_cover_replacement_preserves_old_file_but_updates_canonical_name(self):
        with TemporaryDirectory() as directory:
            folder = Path(directory)
            Image.new("RGB", (20, 20), "red").save(folder / "cover.jpg")
            output = BytesIO()
            Image.new("RGB", (30, 30), "blue").save(output, format="PNG")
            body = output.getvalue()

            services = Services.__new__(Services)
            services.runtime = SimpleNamespace(
                cfg={"output": {"file_name": "cover", "preserve_file": True}},
                require_write=lambda: None,
            )
            services.album_row = lambda _album_id: {
                "album_key": "album-1",
                "musicbrainz_albumid": "11111111-1111-4111-8111-111111111111",
                "path": str(folder),
            }
            services.preview = lambda _payload: {
                "image": "/prepared",
                "format": "png",
                "width": 30,
                "height": 30,
                "bytes": len(body),
            }
            services.catalog = SimpleNamespace(image_bytes=lambda _image: body)
            services.refresh_album = lambda *_args, **_kwargs: None
            recorded = []
            services.audit = lambda *args, **kwargs: recorded.append((args, kwargs))
            services.record_album_write = lambda *_args: {
                "durable": True,
                "writer": "webgui",
            }

            result = Services.write_cover.__wrapped__(services, {"album_id": "album-1"})

            self.assertEqual((folder / "cover.png").read_bytes(), body)
            self.assertFalse((folder / "cover.jpg").exists())
            self.assertTrue((folder / "cover-(2).jpg").is_file())
            self.assertEqual(result["preserved"], ["cover-(2).jpg"])
            self.assertEqual(result["coordination"]["writer"], "webgui")
            self.assertEqual(recorded[0][1]["request_status"], "verified")


if __name__ == "__main__":
    unittest.main()
