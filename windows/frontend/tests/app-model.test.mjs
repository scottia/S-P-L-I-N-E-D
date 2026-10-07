import test from "node:test";
import assert from "node:assert/strict";
import {
  buildMusicBrainzView,
  albumStatusClass,
  applyArtistSelection,
  applyBulkSelection,
  canPersistPortableUi,
  checkAlbum,
  checkTrack,
  configuredScanMode,
  createSelectionState,
  filterMusicBrainzRows,
  focusTrack,
  launchSelection,
  releaseTypesForArtists,
  restoreSafeSelection,
  scanRequestFromSelection,
  selectAlbum,
  shouldLoadLibraryAfterBootstrap,
} from "../app-model.mjs";

test("first-run setup does not create portable state before save", () => {
  assert.equal(shouldLoadLibraryAfterBootstrap(true), false);
  assert.equal(canPersistPortableUi({ first_run: true }), false);
  assert.equal(shouldLoadLibraryAfterBootstrap(false), true);
  assert.equal(canPersistPortableUi({ first_run: false }), true);
});

const rows = [
  { index: 1, release_artist: "Various Artists", release_class: "SOUNDTRACK", release_date: "1998", country: "GB", release_title: "Zed", release_mbid: "5" },
  { index: 2, release_artist: "Various Artists", release_class: "COMPILATION", release_date: "2001", country: "US", release_title: "Hits", release_mbid: "4" },
  { index: 3, release_artist: "Various Artists", release_class: "ALBUM", release_date: "2002", country: "US", release_title: "Collection", release_mbid: "3" },
  { index: 4, release_artist: "Ben E. King", release_class: "ALBUM", release_date: "1961", country: "US", release_title: "Spanish Harlem", release_mbid: "2" },
  { index: 5, release_artist: "Ben E. King", release_class: "ALBUM", release_date: "", country: "", release_title: "Unknown", release_mbid: "1" },
];

test("CURRENT ALBUM is one separate unfiltered authority row", () => {
  const view = buildMusicBrainzView({
    authority_recording_mbid: "recording",
    authority_artist_mbids: "artist",
    authority_release_mbid: "release",
    authority_release_title: "Applied Release",
    authority_release_artist: "Applied Artist",
    album: "Current",
    artist: "Current Artist",
    items: rows,
  }, ["No Such Artist"], ["ALBUM"]);
  assert.equal(view.authority.label, "[CURRENT ALBUM]");
  assert.equal(view.authority.release_type, "");
  assert.equal(view.authority.release_mbid, "release");
  assert.equal(view.authority.release_title, "Applied Release");
  assert.equal(view.authority.release_artist, "Applied Artist");
  assert.equal(view.rows.length, 0);
  assert.equal(JSON.stringify(view).match(/\[CURRENT ALBUM\]/g).length, 1);
});

test("MusicBrainz results follow named artist and special-family order without decade grouping", () => {
  const sorted = filterMusicBrainzRows(rows);
  assert.deepEqual(sorted.map(row => row.index), [4, 5, 1, 2, 3]);
});

test("release type choices narrow to active artists and combined filters use actual row values", () => {
  assert.deepEqual(releaseTypesForArtists(rows, ["Ben E. King"]), ["ALBUM"]);
  assert.deepEqual(filterMusicBrainzRows(rows, ["Various Artists"], ["COMPILATION"]).map(row => row.index), [2]);
});

test("track focus never becomes an explicit target but checking does", () => {
  const initial = checkAlbum(createSelectionState(), "album", true);
  const focused = focusTrack(initial, "album", "track-52.mp3");
  assert.equal(scanRequestFromSelection(focused).compilationTrackPath, null);
  const targeted = checkTrack(focused, "album", "track-01.mp3", true);
  assert.equal(scanRequestFromSelection(targeted).compilationTrackPath, "track-01.mp3");
  const resumed = checkAlbum(targeted, "album", true);
  assert.equal(scanRequestFromSelection(resumed).compilationTrackPath, null);
});

test("Config v5 mode, not stale interface state, initializes the launch mode", () => {
  assert.equal(configuredScanMode('config_version = 5\nmode = "write"\n'), "write");
  assert.equal(configuredScanMode('config_version = 5\nmode = "read"\n'), "read");
  assert.equal(configuredScanMode(""), "read");
});

const mediaAlbums = [
  { path: "a-new", album_key: "key-new", artist: "Artist A", status: "unprocessed", compilation_track_art_eligible: false, compilation_tracks: [] },
  { path: "a-incomplete", album_key: "key-incomplete", artist: "Artist A", status: "incomplete", compilation_track_art_eligible: true, compilation_tracks: [{ path: "track-a", embedded_artwork_recorded: false }] },
  { path: "a-processed", album_key: "key-processed", artist: "Artist A", status: "processed", compilation_track_art_eligible: false, compilation_tracks: [] },
  { path: "a-bypassed", album_key: "key-bypassed", artist: "Artist A", status: "bypassed", compilation_track_art_eligible: false, compilation_tracks: [] },
  { path: "a-timeout", album_key: "key-timeout", artist: "Artist A", status: "timeout", compilation_track_art_eligible: false, compilation_tracks: [] },
  { path: "b-new", album_key: "key-b-new", artist: "Artist B", status: "unprocessed", compilation_track_art_eligible: false, compilation_tracks: [] },
];

test("Album folder-status colors preserve the established contract", () => {
  assert.equal(albumStatusClass("unprocessed"), "white");
  assert.equal(albumStatusClass("incomplete"), "blue");
  assert.equal(albumStatusClass("processed"), "orange");
  assert.equal(albumStatusClass("complete"), "orange");
  assert.equal(albumStatusClass("bypassed"), "red");
  assert.equal(albumStatusClass("timeout"), "purple");
});

test("Artist selection cascades only to eligible children unless bypass is explicitly overridden", () => {
  const initial = createSelectionState();
  const ordinary = applyArtistSelection(initial, mediaAlbums.slice(0, 5), { additive: false, includeBypassed: false });
  assert.deepEqual([...ordinary.albumPaths].sort(), ["a-incomplete", "a-new"]);
  assert.deepEqual([...ordinary.bypassOverrides], []);

  const overridden = applyArtistSelection(initial, mediaAlbums.slice(0, 5), { additive: false, includeBypassed: true });
  assert.deepEqual([...overridden.albumPaths].sort(), ["a-bypassed", "a-incomplete", "a-new"]);
  assert.deepEqual([...overridden.bypassOverrides], ["a-bypassed"]);
  assert.equal(overridden.albumPaths.has("a-timeout"), false);
});

test("Select ALL and FILTERED preserve processed, bypass, and timeout protections", () => {
  const all = applyBulkSelection(createSelectionState(), mediaAlbums.slice(0, 5), "all");
  assert.deepEqual([...all.albumPaths].sort(), ["a-incomplete", "a-new"]);

  const filtered = applyBulkSelection(createSelectionState(), mediaAlbums.slice(0, 5), "filtered");
  assert.deepEqual([...filtered.albumPaths].sort(), ["a-incomplete", "a-new", "a-processed"]);
  assert.equal(filtered.albumPaths.has("a-bypassed"), false);
  assert.equal(filtered.albumPaths.has("a-timeout"), false);
});

test("direct selection requires a temporary bypass override and refuses active timeout", () => {
  const bypassed = selectAlbum(createSelectionState(), mediaAlbums[3]);
  assert.equal(bypassed.protection, "bypass");
  assert.equal(bypassed.selection.albumPaths.size, 0);

  const overridden = selectAlbum(createSelectionState(), mediaAlbums[3], { allowBypass: true });
  assert.equal(overridden.protection, null);
  assert.deepEqual([...overridden.selection.albumPaths], ["a-bypassed"]);
  assert.deepEqual([...overridden.selection.bypassOverrides], ["a-bypassed"]);

  const timeout = selectAlbum(createSelectionState(), mediaAlbums[4]);
  assert.equal(timeout.protection, "timeout");
  assert.equal(timeout.selection.albumPaths.size, 0);
});

test("restored UI selection cannot bypass saved bypass or active timeout authority", () => {
  const stored = createSelectionState();
  stored.albumPaths = new Set(mediaAlbums.map(album => album.path));
  const restored = restoreSafeSelection(stored, mediaAlbums);
  assert.deepEqual([...restored.albumPaths].sort(), ["a-incomplete", "a-new", "a-processed", "b-new"]);
});

test("Auto Scan ALL adds only globally unprocessed or incomplete Albums to explicit selections", () => {
  const selection = createSelectionState();
  selection.albumPaths.add("a-processed");
  const launched = launchSelection(selection, mediaAlbums, "all");
  assert.deepEqual([...launched.albumPaths].sort(), ["a-incomplete", "a-new", "a-processed", "b-new"]);
});

test("pending compilation artwork is the narrow automatic bypass exception", () => {
  const pendingBypass = {
    path: "pending-bypass",
    album_key: "pending-key",
    artist: "Artist C",
    status: "bypassed",
    compilation_track_art_eligible: true,
    compilation_tracks: [{ path: "unfinished.mp3", embedded_artwork_recorded: false }],
  };
  const launched = launchSelection(createSelectionState(), [pendingBypass], "all");
  assert.deepEqual([...launched.albumPaths], ["pending-bypass"]);
  assert.deepEqual([...launched.bypassOverrides], ["pending-bypass"]);
});

test("multi-Album requests carry an independent SQLite identity for every Album", () => {
  const selection = createSelectionState();
  selection.albumPaths = new Set(["a-new", "b-new"]);
  selection.focusedAlbumPath = "a-new";
  const request = scanRequestFromSelection(selection, { albums: mediaAlbums });
  assert.deepEqual(request.albums, [
    { path: "a-new", indexedAlbumPath: "a-new", indexedAlbumKey: "key-new", bypassOverride: false, compilationTrackPath: null },
    { path: "b-new", indexedAlbumPath: "b-new", indexedAlbumKey: "key-b-new", bypassOverride: false, compilationTrackPath: null },
  ]);
  assert.equal("indexedAlbumPath" in request, false);
  assert.equal("indexedAlbumKey" in request, false);
});
