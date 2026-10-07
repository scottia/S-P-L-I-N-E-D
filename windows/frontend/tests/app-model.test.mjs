import test from "node:test";
import assert from "node:assert/strict";
import {
  buildMusicBrainzView,
  canPersistPortableUi,
  checkAlbum,
  checkTrack,
  createSelectionState,
  filterMusicBrainzRows,
  focusTrack,
  releaseTypesForArtists,
  scanRequestFromSelection,
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
