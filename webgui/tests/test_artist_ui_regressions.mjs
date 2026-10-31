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
const launchArtistFunction = between(
  operationsSource,
  "async function launchArtist",
  "async function enqueueComp",
);
const launchCompFunctions = between(
  operationsSource,
  "async function enqueueComp",
  "async function stageAlbum",
);
const stageAlbumFunction = between(
  operationsSource,
  "async function stageAlbum",
  "async function queueAction",
);
const artistMetadataFunction = between(
  appSource,
  "async function loadArtistMetadata",
  "async function searchArtist",
);
const clearFocusFunction = between(
  appSource,
  "function clearFocusCandidates",
  "function resetFocusAdjustments",
);
const chooseAlbumFunction = between(
  appSource,
  "async function chooseAlbum",
  "async function selectFallbackTrack",
);
const artistCompReportFunction = between(
  appSource,
  "async function openArtistCompReport",
  "async function probeArtistCompStatuses",
);

function queuedContext({groups, album, artistResponse}) {
  const originalArtist = {id: "projected-artist", albums: []};
  const decision = {context: {album_id: album.id}, jobKind: "comp"};
  const calls = [];
  const values = {
    state: {
      artist: originalArtist,
      album: null,
      compAlbums: groups,
      compGroups: [],
      compReportVisible: false,
      compTrack: null,
      editorMode: "artist",
      queuedDecision: decision,
      selectionRequest: 0,
    },
    api: async path => {
      calls.push(path);
      if (path.startsWith("/api/album")) return album;
      if (path.startsWith("/api/artist")) return {artist: artistResponse, comp_albums: []};
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

test("Artist-scoped queued COMP review retains the report without a preselected track", async () => {
  const selected = [];
  const decision = {
    context: {album_id: "comp-1", track_key: "artist-track"},
    jobKind: "comp",
    payload: {artist_id: "artist-1", album_id: "comp-1", track_keys: ["artist-track"]},
  };
  const state = {
    artist: {id: "artist-1", albums: []}, album: null, compAlbums: [],
    compGroups: [{id: "comp-1", tracks: [{key: "artist-track"}]}],
    compReportVisible: true, compTrack: null, editorMode: "artist",
    queuedDecision: decision, selectionRequest: 0,
  };
  const context = load(queuedAlbumFunction, {
    state,
    selectArtistCompTrack: async (albumId, key) => { selected.push([albumId, key]); },
    api: async path => { throw new Error(`unexpected full-album navigation: ${path}`); },
  });

  await context.focusQueuedAlbum(decision);

  assert.equal(state.editorMode, "artist");
  assert.equal(state.compReportVisible, true);
  assert.deepEqual(selected, [["comp-1", "artist-track"]]);
});

test("Artist-scoped queued COMP review restores its Artist after switching", async () => {
  const calls = [];
  const selected = [];
  const decision = {
    context: {album_id: "comp-1", track_key: "artist-track"},
    jobKind: "comp",
    payload: {artist_id: "artist-1", album_id: "comp-1", track_keys: ["artist-track"]},
  };
  const state = {
    artist: {id: "artist-2", albums: []}, artistMetadata: null, album: null,
    compAlbums: [], compGroups: [{id: "comp-2", tracks: [{key: "other-track"}]}],
    compReportVisible: true, compReportLoading: false, compPreviewMode: false,
    compTrack: null, compReportRequest: 0, compStatusRequest: 0, compTrackRequest: 0,
    fallbackTracks: [], fallbackTrack: null, upscalePreview: null,
    editorMode: "artist", workspace: "find", queuedDecision: decision,
    selectionRequest: 0, candidates: [{image: "queued-candidate"}],
  };
  const context = load(queuedAlbumFunction, {
    state,
    api: async path => {
      calls.push(path);
      if (path.startsWith("/api/artist/overview")) return {
        artist: {id: "artist-1", albums: []},
        comp_albums: [{id: "comp-1", tracks: [{key: "artist-track"}]}],
      };
      if (path.startsWith("/api/artist/comp-report")) return [
        {id: "comp-1", tracks: [{key: "artist-track"}]},
      ];
      throw new Error(`unexpected API call: ${path}`);
    },
    $: () => ({textContent: "COMP Report"}),
    renderLibrary() {}, renderHero() {}, renderSources() {}, renderCandidates() {}, renderWorkspace() {},
    loadArtistMetadata() {},
    selectArtistCompTrack: async (albumId, key) => { selected.push([albumId, key]); },
  });

  await context.focusQueuedAlbum(decision);

  assert.equal(state.artist.id, "artist-1");
  assert.equal(state.album, null);
  assert.equal(state.editorMode, "artist");
  assert.equal(state.compReportVisible, true);
  assert.deepEqual(state.compGroups[0].tracks.map(track => track.key), ["artist-track"]);
  assert.deepEqual(selected, [["comp-1", "artist-track"]]);
  assert.equal(calls.some(path => path.startsWith("/api/album")), false);
  assert.equal(calls.some(path => path.startsWith("/api/fallback/tracks")), false);
});

test("Artist and full-album COMP launches submit independent explicit scopes", async () => {
  const calls = [];
  const reviewed = [];
  const state = {
    artist: {id: "artist-1"}, compReportVisible: true,
    compGroups: [{id: "comp-1", tracks: [
      {key: "artist-track", status: "blue"},
      {key: "completed-track", status: "green"},
    ]}],
    album: {id: "comp-1"},
    fallbackTracks: [
      {key: "artist-track", completed: false},
      {key: "other-track", completed: false},
      {key: "completed-track", completed: true},
    ],
  };
  const context = load(launchCompFunctions, {
    state,
    post: async (path, body) => { calls.push([path, body]); return {id: `job-${calls.length}`}; },
    reviewJob: async id => { reviewed.push(id); },
    setStatus() {},
  });

  await context.launchArtistComp();
  await context.launchFallback();

  const submitted = JSON.parse(JSON.stringify(calls));
  assert.deepEqual(submitted[0], ["/api/jobs", {kind: "comp", payload: {
    artist_id: "artist-1", album_id: "comp-1", track_keys: ["artist-track"],
  }}]);
  assert.deepEqual(submitted[1], ["/api/jobs", {kind: "comp", payload: {
    album_id: "comp-1", track_keys: ["artist-track", "other-track"],
  }}]);
  assert.deepEqual(reviewed, ["job-1", "job-2"]);
});

test("lazy thumbnail hydration updates matching artists and rerenders", async () => {
  let renders = 0;
  const context = load(hydrateFunction, {
    state: {
      listRequest: 4,
      libraryMode: "artists",
      artists: [{id: "artist-1", portrait: null}, {id: "artist-2", portrait: "local"}],
      artist: {id: "artist-1", portrait: null},
    },
    api: async () => [
      {id: "artist-1", portrait: "/api/artist/thumbnail?id=artist-1"},
      {id: "missing", portrait: "/missing"},
    ],
    renderLibrary: () => { renders += 1; },
  });

  await context.hydrateArtistThumbnails(4);

  assert.equal(context.state.artists[0].portrait, "/api/artist/thumbnail?id=artist-1");
  assert.equal(context.state.artist.portrait, "/api/artist/thumbnail?id=artist-1");
  assert.equal(context.state.artists[1].portrait, "local");
  assert.equal(renders, 1);
});

test("Artist LAUNCH submits every selected eligible release and preserves selection", async () => {
  const calls = [];
  const selected = new Set(["artist-1", "own", "featured", "complete", "bypassed", "timeout", "comp"]);
  const context = load(launchArtistFunction, {
    state: {
      artist: {id: "artist-1", portrait: "artist", albums: [
        {id: "own", album_mbid: "release-own", status: "incomplete", tracks: 2, embedded: 0, cover: "cover", cover_found: true},
        {id: "featured", album_mbid: "release-featured", status: "unprocessed", tracks: 1, embedded: 0, cover: "cover", cover_found: true, featured: true},
        {id: "complete", album_mbid: "release-complete", status: "processed", tracks: 2, embedded: 2, cover: "cover", cover_found: true},
        {id: "bypassed", album_mbid: "release-bypassed", status: "bypassed", tracks: 1, embedded: 0},
        {id: "timeout", album_mbid: "release-timeout", status: "timeout", tracks: 1, embedded: 0},
        {id: "comp", album_mbid: "", status: "incomplete", tracks: 1, embedded: 0},
      ]},
      mediaSelected: selected,
    },
    post: async (path, payload) => { calls.push([path, payload]); return {id: "job-1"}; },
    reviewJob: async () => {},
    artistAlbumEligible: album => !!album.album_mbid && !["bypassed", "timeout"].includes(album.status)
      && !(album.status === "processed" && album.cover_found && album.embedded >= album.tracks),
  });

  await context.launchArtist();

  assert.equal(calls.length, 1);
  assert.equal(calls[0][0], "/api/jobs");
  assert.equal(calls[0][1].kind, "scan");
  assert.deepEqual([...calls[0][1].payload.album_ids], ["own", "featured"]);
  assert.deepEqual([...selected], ["artist-1", "own", "featured", "complete", "bypassed", "timeout", "comp"]);
});

test("successful EMBED Batch staging clears stale candidates and review selection only", async () => {
  const artist = {id: "artist-1"};
  const album = {id: "album-1", name: "Album", cover: "cover"};
  const embedQueue = [{id: "persistent-job"}];
  const state = {
    artist, album, embedQueue,
    candidates: [{image: "candidate"}], selectedCover: {image: "candidate"},
    upscalePreview: {image: "upscale"}, renderedPreview: {album: {image: "rendered"}},
    compare: new Set(["candidate"]), reviewCleared: false,
  };
  const context = load(stageAlbumFunction, {
    state,
    post: async () => ({id: "job-1", already_queued: false}),
    coverPayload: () => ({album_id: album.id, image: album.cover}),
    pollJobs: async () => {}, renderHero() {}, renderReview() {}, renderSources() {}, renderCandidates() {}, renderPanes() {},
    resetCandidateFilters() {}, setStatus() {},
  });

  await context.stageAlbum();

  assert.equal(state.artist, artist);
  assert.equal(state.album, album);
  assert.equal(state.embedQueue, embedQueue);
  assert.equal(state.candidates.length, 0);
  assert.equal(state.selectedCover, null);
  assert.equal(state.upscalePreview, null);
  assert.equal(Object.keys(state.renderedPreview).length, 0);
  assert.equal(state.compare.size, 0);
  assert.equal(state.reviewCleared, true);
});

test("queued Album review exposes SKIP ALBUM and submits k without pausing the batch", async () => {
  assert.match(operationsSource, /async function skipQueuedAlbum\(\)/);
  const skipSource = between(operationsSource, "async function skipQueuedAlbum", "async function enqueueComp");
  const calls = [];
  const state = {queuedDecision: {jobId: "scan-1", jobKind: "scan", nonce: "nonce-1"}};
  const context = load(skipSource, {
    state,
    submitQueuedDecision: async value => { calls.push(value); },
  });
  await context.skipQueuedAlbum();
  assert.deepEqual(calls, ["k"]);
  assert.doesNotMatch(skipSource, /pause|cancel/);
  assert.match(appSource, /state\.queuedDecision[^\n]+skipQueuedAlbum/);
});

test("background Artist metadata refreshes Featured and COMP projections without losing focus", async () => {
  let renders = 0;
  const state = {
    artist: {id: "artist-1", albums: [{id: "old"}]},
    artistMetadata: null,
    artistMetadataRequest: 0,
    compAlbums: [],
    compGroups: [],
  };
  const context = load(artistMetadataFunction, {
    state,
    api: async () => ({
      biography: "Biography", similar: [],
      artist: {id: "artist-1", albums: [{id: "own"}, {id: "featured", featured: true}]},
      comp_albums: [{id: "comp", tracks: [{key: "track"}]}],
    }),
    renderHero: () => { renders += 1; }, renderSources() {}, renderLibrary() {},
  });

  await context.loadArtistMetadata("artist-1");

  assert.deepEqual(state.artist.albums.map(album => album.id), ["own", "featured"]);
  assert.deepEqual(state.compAlbums.map(group => group.id), ["comp"]);
  assert.equal(state.compGroups.length, 0);
  assert.equal(state.artistMetadata.biography, "Biography");
  assert.equal(renders, 1);
});

test("Album navigation preserves the selected Artist COMP card inventory", async () => {
  const comp = {id: "comp-1", tracks: [{key: "target-track"}]};
  const state = {
    artist: {id: "artist-1", albums: [{id: "own-1"}]},
    artistMetadata: null,
    album: null,
    compAlbums: [comp],
    compGroups: [comp],
    compCollapsed: new Set(),
    compare: new Set(),
    candidates: [],
    selectedPortrait: null, selectedBanner: null, selectedCover: null,
    upscalePreview: null, renderedPreview: {},
    fallbackTracks: [], fallbackTrack: null, fallbackPreviewOnly: false, fallbackRun: false,
    fallbackTrackRequest: 0, fallbackPreviewMode: false, fallbackReportExpanded: true,
    fallbackHelpExpanded: false, compReportVisible: false, compReportLoading: false,
    compPreviewMode: false, compTrack: null, compRunIndex: -1,
    compTrackRequest: 0, compStatusRequest: 0, compReportRequest: 0,
    queuedDecision: null, reviewJobId: null, candidateFilterLocked: true,
    selectionRequest: 0, searchRequest: 0, reviewCleared: false,
  };
  const context = load(clearFocusFunction + contextFunctions + chooseAlbumFunction, {
    state,
    api: async path => {
      assert.match(path, /^\/api\/album/);
      return {id: "own-1", artist_id: "artist-1", album_mbid: "release-1", candidates: []};
    },
    resetFocusAdjustments() {}, renderSources() {}, renderCandidates() {}, renderWorkspace() {},
    resetCandidateFilters() {}, markSelectedAlbum() {}, renderAll() {}, loadArtistMetadata() {},
    setStatus() {},
  });

  await context.chooseAlbum("own-1");

  assert.equal(context.albumInCurrentArtistContext("comp-1"), true);
  assert.deepEqual(state.compAlbums.map(group => group.id), ["comp-1"]);
  assert.equal(state.compGroups.length, 0);
});

test("late Artist metadata cannot replace an already scoped COMP report", async () => {
  let resolveMetadata;
  const metadataReady = new Promise(resolve => { resolveMetadata = resolve; });
  const scoped = [{id: "comp-1", tracks: [{key: "target-track"}]}];
  const unscoped = [{id: "comp-1", tracks: Array.from({length: 102}, (_, index) => ({key: `track-${index}`}))}];
  const state = {
    artist: {id: "artist-1", albums: []}, artistMetadata: null,
    artistMetadataRequest: 0,
    compAlbums: [], compGroups: [], compCollapsed: new Set(),
    compReportVisible: false, compReportLoading: false, compPreviewMode: false,
    compTrack: null, compTrackRequest: 0, compStatusRequest: 0, compReportRequest: 0,
    candidates: [], upscalePreview: null,
  };
  const elements = {
    compReportButton: {disabled: false, textContent: "COMP Report"},
    fallbackReport: {querySelectorAll: () => [], getBoundingClientRect: () => ({top: 0})},
    overviewContent: {scrollTop: 0, getBoundingClientRect: () => ({top: 0})},
  };
  const context = load(artistMetadataFunction + artistCompReportFunction, {
    state,
    api: async path => path.startsWith("/api/artist/metadata") ? metadataReady : scoped,
    $: id => elements[id],
    renderLibrary() {}, renderHero() {}, renderSources() {}, renderCandidates() {}, renderReview() {},
    resetCandidateFilters() {}, setStatus() {}, probeArtistCompStatuses() {},
    requestAnimationFrame() {},
  });

  const metadata = context.loadArtistMetadata("artist-1");
  await context.openArtistCompReport();
  resolveMetadata({
    biography: "Biography", similar: [], artist: {id: "artist-1", albums: []},
    comp_albums: unscoped,
  });
  await metadata;

  assert.deepEqual(state.compGroups[0].tracks.map(track => track.key), ["target-track"]);
  assert.equal(state.compAlbums[0].tracks.length, 102);
});

test("late COMP response cannot leak tracks into a newly selected Artist", async () => {
  let resolveFirst;
  const firstReady = new Promise(resolve => { resolveFirst = resolve; });
  const state = {
    artist: {id: "artist-1", albums: []}, compAlbums: [], compGroups: [],
    compCollapsed: new Set(), compReportVisible: false, compReportLoading: false,
    compPreviewMode: false, compTrack: null, compTrackRequest: 0,
    compStatusRequest: 0, compReportRequest: 0, candidates: [], upscalePreview: null,
  };
  const elements = {
    compReportButton: {disabled: false, textContent: "COMP Report"},
    fallbackReport: {querySelectorAll: () => [], getBoundingClientRect: () => ({top: 0})},
    overviewContent: {scrollTop: 0, getBoundingClientRect: () => ({top: 0})},
  };
  const context = load(artistCompReportFunction, {
    state,
    api: async path => path.includes("artist-1") ? firstReady
      : [{id: "comp-2", tracks: [{key: "artist-2-track"}]}],
    $: id => elements[id],
    renderHero() {}, renderCandidates() {}, renderReview() {}, setStatus() {},
    probeArtistCompStatuses() {}, requestAnimationFrame() {},
  });

  const first = context.openArtistCompReport();
  state.artist = {id: "artist-2", albums: []};
  state.compReportLoading = false;
  ++state.compReportRequest;
  await context.openArtistCompReport();
  resolveFirst([{id: "comp-1", tracks: [{key: "artist-1-track"}]}]);
  await first;

  assert.equal(state.compGroups[0].id, "comp-2");
  assert.deepEqual(state.compGroups[0].tracks.map(track => track.key), ["artist-2-track"]);
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
