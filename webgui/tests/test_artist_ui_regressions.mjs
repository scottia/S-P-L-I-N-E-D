import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import test from "node:test";
import vm from "node:vm";

const appSource = readFileSync(new URL("../static/app.js", import.meta.url), "utf8");
const operationsSource = readFileSync(new URL("../static/operations.js", import.meta.url), "utf8");

function between(source, start, end) {
  const first = source.indexOf(start);
  const last = source.indexOf(end, first + start.length);
  assert.notEqual(first, -1, `missing source marker: ${start}`);
  assert.notEqual(last, -1, `missing source marker: ${end}`);
  return source.slice(first, last);
}

function load(source, values) {
  const context = vm.createContext({...values});
  vm.runInContext(source, context);
  return context;
}

const contextFunctions = between(
  appSource,
  "function albumInCurrentArtistContext",
  "function albumBannerKind",
);
const hydrateFunction = between(
  appSource,
  "async function hydrateArtistThumbnails",
  "async function chooseArtist",
);
const queuedAlbumFunction = between(
  operationsSource,
  "async function focusQueuedAlbum",
  "async function submitQueuedDecision",
);
const writeArtworkFunction = between(
  operationsSource,
  "async function writeArtwork",
  "const previewReview",
);

function queuedContext({groups, album, artistResponse}) {
  const originalArtist = {id: "projected-artist", albums: []};
  const decision = {context: {album_id: album.id}, jobKind: "comp"};
  const calls = [];
  const values = {
    state: {
      artist: originalArtist,
      album: null,
      compGroups: groups,
      compReportVisible: false,
      compTrack: null,
      editorMode: "artist",
      queuedDecision: decision,
      selectionRequest: 0,
    },
    api: async path => {
      calls.push(path);
      if (path.startsWith("/api/album")) return album;
      if (path.startsWith("/api/artist")) return artistResponse;
      throw new Error(`unexpected API call: ${path}`);
    },
    markSelectedAlbum() {},
    renderLibrary() {},
    renderHero() {},
    renderSources() {},
    renderCandidates() {},
    renderWorkspace() {},
    loadArtistMetadata() {},
  };
  const context = load(contextFunctions + queuedAlbumFunction, values);
  return {context, decision, originalArtist, calls};
}

test("queued COMP album retains its projected artist context", async () => {
  const album = {id: "comp-1", artist_id: "physical-owner", album_mbid: "release"};
  const fixture = queuedContext({
    groups: [{id: "comp-1", tracks: []}],
    album,
    artistResponse: {id: "physical-owner", albums: []},
  });

  assert.equal(fixture.context.albumInCurrentArtistContext("comp-1"), true);
  await fixture.context.focusQueuedAlbum(fixture.decision);

  assert.equal(fixture.context.state.artist, fixture.originalArtist);
  assert.equal(fixture.calls.filter(path => path.startsWith("/api/artist")).length, 0);
});

test("queued album for another artist still changes artist context", async () => {
  const replacement = {id: "other-artist", albums: []};
  const fixture = queuedContext({
    groups: [{id: "different-comp", tracks: []}],
    album: {id: "other-album", artist_id: "other-artist", album_mbid: "release"},
    artistResponse: replacement,
  });

  await fixture.context.focusQueuedAlbum(fixture.decision);

  assert.equal(fixture.context.state.artist, replacement);
  assert.equal(fixture.calls.filter(path => path.startsWith("/api/artist")).length, 1);
});

test("lazy thumbnail hydration updates matching artists and rerenders", async () => {
  let renders = 0;
  const context = load(hydrateFunction, {
    state: {
      listRequest: 4,
      libraryMode: "artists",
      artists: [{id: "artist-1", portrait: null}, {id: "artist-2", portrait: "local"}],
    },
    api: async () => [
      {id: "artist-1", portrait: "/api/artist/thumbnail?id=artist-1"},
      {id: "missing", portrait: "/missing"},
    ],
    renderLibrary: () => { renders += 1; },
  });

  await context.hydrateArtistThumbnails(4);

  assert.equal(context.state.artists[0].portrait, "/api/artist/thumbnail?id=artist-1");
  assert.equal(context.state.artists[1].portrait, "local");
  assert.equal(renders, 1);
});

test("empty and offline thumbnail hydration leave local results usable", async () => {
  let renders = 0;
  const state = {listRequest: 2, libraryMode: "artists", artists: [{id: "artist-1", portrait: "local"}]};
  const empty = load(hydrateFunction, {
    state,
    api: async () => [],
    renderLibrary: () => { renders += 1; },
  });
  await empty.hydrateArtistThumbnails(2);

  const offline = load(hydrateFunction, {
    state,
    api: async () => { throw new Error("offline"); },
    renderLibrary: () => { renders += 1; },
  });
  await offline.hydrateArtistThumbnails(2);

  assert.equal(state.artists[0].portrait, "local");
  assert.equal(renders, 0);
});

test("artist artwork write refreshes the corresponding list row", async () => {
  let libraryRenders = 0;
  const listed = {id: "artist-1", portrait: "old", status: "unprocessed"};
  const state = {
    queuedDecision: null,
    editorMode: "artist",
    compReportVisible: false,
    compTrack: null,
    fallbackTrack: null,
    workspace: "find",
    selectedPortrait: {image: "candidate"},
    selectedBanner: null,
    radioCurrent: false,
    radioArtist: true,
    radioBanner: false,
    artist: {id: "artist-1", portrait: "old", banner: null},
    artists: [listed],
    renderedPreview: {artist: "stale"},
    fx: {current: {}, artist: {}, banner: {}, package: {}},
    fit: {current: {zoom: 1, x: 0, y: 0}, artist: {}, banner: {}},
    themedPackaging: false,
  };
  const context = load(writeArtworkFunction, {
    state,
    post: async () => ({written: ["artist.png"]}),
    api: async () => ({id: "artist-1", portrait: "fresh", banner: null, status: "processed"}),
    componentPayload: kind => ({kind}),
    renderLibrary: () => { libraryRenders += 1; },
    renderHero() {},
    renderPanes() {},
    renderReview() {},
    setStatus() {},
    enterArtistArt() {},
    enqueueComp() {},
    submitQueuedDecision() {},
  });

  await context.writeArtwork();

  assert.equal(listed.portrait, "fresh");
  assert.equal(listed.status, "processed");
  assert.equal(libraryRenders, 1);
});
