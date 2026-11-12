import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import test from "node:test";
import vm from "node:vm";

const appSource = readFileSync(new URL("../static/app.js", import.meta.url), "utf8");
const operationsSource = readFileSync(new URL("../static/operations.js", import.meta.url), "utf8");
const indexSource = readFileSync(new URL("../static/index.html", import.meta.url), "utf8");
const cssSource = readFileSync(new URL("../static/app.css", import.meta.url), "utf8");

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
const graphicalEnhancementFunctions = between(
  appSource,
  "const GRAPHICAL_ENHANCEMENT_DEFAULTS",
  "const state =",
);
const albumBannerFunctions = between(
  appSource,
  "function albumBannerKind",
  "function applyAlbumCoordination",
);

test("shared release types outrank secondary-credit banners", () => {
  const context = load(albumBannerFunctions, {});
  assert.equal(context.albumBannerKind({album_mbid: "release", featured: true, soundtrack: true}, {}), "soundtrack");
  assert.equal(context.albumBannerKind({album_mbid: "release", featured: true, various_artists: true}, {}), "various");
  assert.equal(context.albumBannerKind({album_mbid: "release", featured: true}, {}), "featuring");
  assert.equal(context.albumBannerKind({album_mbid: "release"}, {}), "artist");
});
const coordinationFunctions = between(
  appSource,
  "function applyAlbumCoordination",
  "function albumCardState",
);
const opposingThumbnailFunctions = between(
  operationsSource,
  "async function refreshOpposingAlbumThumbnails",
  "async function refreshQueuedAlbumAfterSave",
);
const hydrateFunction = between(
  appSource,
  "async function hydrateArtistThumbnails",
  "async function chooseArtist",
);
const artistAlbumStatusFunction = between(
  appSource,
  "async function probeArtistAlbumStatuses",
  "async function chooseArtist",
);
const queuedAlbumFunction = between(
  operationsSource,
  "async function focusQueuedAlbum",
  "async function submitQueuedDecision",
);
const compDecisionFunctions = between(
  appSource,
  "function currentCompSelectionScope",
  "function currentCandidate",
);
const editableArtworkFunctions = compDecisionFunctions + "\n" + between(
  appSource,
  "function editableArtworkSource",
  "function renderLibrary",
);
const compTrackStatusFunction = between(
  appSource,
  "function compTrackStatusClass",
  "function renderLibraryContents",
);
const queuedCandidateSelectionFunctions = between(
  appSource,
  "function candidateSelectionKey",
  "const MEDIA_STATUSES",
) + "\n" + between(
  operationsSource,
  "function queuedCandidate",
  "function clearQueuedDecision",
) + "\n" + between(
  operationsSource,
  "async function presentQueuedCandidates",
  "async function presentQueuedMusicBrainz",
);
const writeArtworkFunction = compDecisionFunctions + "\n" + between(
  operationsSource,
  "async function writeArtwork",
  "const previewReview",
);
const compThumbnailFunctions = compDecisionFunctions + "\n" + between(
  operationsSource,
  "function compContextActive",
  "function coverPayload",
);
const coverPayloadFunction = between(
  operationsSource,
  "function coverPayload",
  "async function writeArtwork",
);
const upscalePreviewStatusFunction = between(
  appSource,
  "function upscalePreviewStatus",
  '$("upscaleButton").addEventListener',
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
const lowerLaunchFunction = between(
  operationsSource,
  "async function launchCandidateWork",
  "async function stageAlbum",
);
const queuedMusicBrainzFunction = between(
  operationsSource,
  "async function presentQueuedMusicBrainz",
  "function renderQueue",
);
const renderJobFunction = between(
  operationsSource,
  "async function renderJob",
  "function settingValue",
);
const reviewJobFunction = between(
  operationsSource,
  "async function reviewJob",
  "async function renderJob",
);
const renderMusicBrainzFunction = between(
  appSource,
  "function musicBrainzFamilyKey",
  "function renderQueuedCompTools",
);
const sourceKeyFunction = between(
  appSource,
  "function sourceKey",
  "function candidateKind",
);
const renderQueueFunction = between(
  operationsSource,
  "function renderQueue",
  "async function refreshCompletedAlbum",
);
const pollJobsFunction = between(
  operationsSource,
  "let jobsPolling",
  "async function reviewJob",
);
const stageAlbumFunction = between(
  operationsSource,
  "function clearAlbumSearchSession",
  "function skipAlbumSearch",
) + "\n" + between(
  operationsSource,
  "async function stageAlbum",
  "async function queueAction",
);
const statusFunctions = between(
  appSource,
  "function setStatus",
  "async function api",
);
const heroPortraitFunction = between(
  appSource,
  "function renderHeroPortrait",
  "function renderArtistInfoVisibility",
);
const uploadedArtworkFunction = between(
  operationsSource,
  "function useUploadedArtwork",
  '$("uploadArtwork").addEventListener',
);
const albumSummaryFunctions = between(
  appSource,
  "function albumOverride",
  "function renderAlbumStatusKey",
);

test("uploaded Artist Art starts blue while retaining explicit pink role choice", () => {
  const state = {editorMode: "artist", candidates: [], radioBanner: true};
  const selections = [];
  let renders = 0;
  const context = load(uploadedArtworkFunction, {
    state,
    renderCandidates: () => { renders += 1; },
    selectCandidate: (candidate, role) => selections.push([candidate, role]),
  });
  const candidate = {name: "Uploaded banner-shaped image"};

  context.useUploadedArtwork(candidate);

  assert.equal(candidate.kind, "artist");
  assert.equal(state.candidates[0], candidate);
  assert.deepEqual(selections, [[candidate, "artist"]]);
  assert.equal(renders, 1);
  assert.match(appSource, /Every Artist Art candidate can be[\s\S]*candidate-radio artist[\s\S]*candidate-radio banner/);
  assert.doesNotMatch(appSource, /function candidateBanner\(/);
});

test("Artist metadata rerenders retain the visible portrait element", () => {
  let replacement = null;
  const current = {
    tagName: "IMG",
    className: "hero-portrait",
    alt: "",
    src: "/artist.jpg",
    getAttribute: name => name === "src" ? "/artist.jpg" : null,
    replaceWith: value => { replacement = value; },
  };
  const context = load(heroPortraitFunction, {
    $: id => {
      assert.equal(id, "heroPortrait");
      return current;
    },
    image: () => { throw new Error("unchanged portrait must not be replaced"); },
    node: () => { throw new Error("unchanged portrait must not become a placeholder"); },
  });

  context.renderHeroPortrait({name: "Test Artist", portrait: "/artist.jpg"});

  assert.equal(replacement, null);
  assert.equal(current.className, "hero-portrait");
  assert.equal(current.alt, "Test Artist artist image");
});
const stageAllAlbumsFunction = between(
  operationsSource,
  "async function stageAllArtistAlbums",
  '$("embedAllAlbums")',
);
const albumEligibilityFunctions = between(
  appSource,
  "function validMusicBrainzId",
  "function markSelectedArtist",
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
const chooseArtistFunction = between(
  appSource,
  "async function chooseArtist",
  "async function chooseAlbum",
);
const compReportOrderFunction = between(
  appSource,
  "function compareCompReportGroups",
  "function albumOverride",
);
const artistCompReportFunction = compReportOrderFunction + "\n" + between(
  appSource,
  "async function openArtistCompReport",
  "async function probeArtistCompStatuses",
);
const renderArtistCompReportFunction = compTrackStatusFunction + "\n" + between(
  appSource,
  "function renderArtistCompReport",
  "function closeArtistCompReport",
);
const repairArtistCompFunction = between(
  appSource,
  "async function repairArtistComp",
  "function renderSources",
);
const repairFallbackFunction = between(
  operationsSource,
  "async function repairFallback",
  "const quickPreview",
);

test("COMP artwork source labels map to the Windows source counters", () => {
  const context = load(sourceKeyFunction, {SOURCES: [
    ["Amazon", "amazon"], ["MusicBrainz", "musicbrainz"], ["Discogs", "discogs"], ["Fanart.tv", "fanart"],
  ]});
  assert.equal(context.sourceKey("MB / CAA"), "musicbrainz");
  assert.equal(context.sourceKey("MusicBrainz"), "musicbrainz");
  assert.equal(context.sourceKey("CoverArt"), "coverartarchive");
  assert.equal(context.sourceKey("Cover Art Archive"), "coverartarchive");
  assert.deepEqual(
    Array.from(context.distinctDiagnostics([
      ["musicbrainz", "policy-filtered"],
      ["musicbrainz", "policy-filtered"],
      {source: "amazon", message: "HTTP 503"},
    ])),
    ["musicbrainz · policy-filtered", "amazon · HTTP 503"],
  );
  assert.equal(context.sourceDiagnosticStatus([["amazon", "Amazon Store search returned HTTP 503"]], "amazon").status, "unavailable");
  assert.equal(context.sourceDiagnosticStatus([["discogs", "policy-filtered: Short side is below the global minimum"]], "discogs").status, "filtered");
  assert.equal(context.sourceDiagnosticStatus(["Fanart.tv: request failed (ConnectionError)"], "fanart").status, "unavailable");
  assert.match(appSource, /const runtimeClass = !currentCount && runtime\.status \? " " \+ runtime\.status : ""/);
  assert.match(appSource, /artistEnabled && configured \? "Artist \/ Album"/);
  assert.match(appSource, /artistEnabled \? "Artist-Only" : configured \? "Album-Only" : "Deactivated"/);
  assert.match(appSource, /node\("span", "source-search-glyph", "🔍"\)/);
  assert.match(appSource, /node\("span", "source-status-radio"\)/);
  assert.match(cssSource, /\.source-card strong\{font-size:11px;color:#fff/);
  assert.match(cssSource, /\.source-status\.mode-artist-album\{color:var\(--green\)\}/);
  assert.match(cssSource, /\.source-status\.mode-artist-only\{color:var\(--pink\)\}/);
  assert.match(cssSource, /\.source-status\.mode-album-only\{color:color-mix\(in srgb,var\(--yellow\) 72%,var\(--danger\)\)\}/);
  assert.match(cssSource, /\.source-status\.mode-deactivated\{color:var\(--danger\)\}/);
});

test("Chalk theme reaches library and action surfaces through palette variables", () => {
  assert.match(cssSource, /html\[data-theme=chalk\]\{[^}]*--violet:[^;}]+;[^}]*--yellow:[^;}]+;[^}]*--danger:/);
  assert.match(cssSource, /\.library-list\{background:var\(--panel\)\}/);
  assert.match(cssSource, /\.header-actions button,[^{]*\{border-color:var\(--line\);background:var\(--card\);color:var\(--text\)\}/);
  assert.match(cssSource, /\.workspace-dialog,\.embed-summary\{background:var\(--panel\);color:var\(--text\)\}/);
  assert.match(cssSource, /\.review-actions \.action-purple\{border-color:var\(--violet\);background:color-mix/);
  assert.match(cssSource, /\.overview-actions #embedAllAlbums\{border-color:var\(--yellow\);background:color-mix/);
});

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

test("queued Artist scan keeps the full Artist Album selection visible", async () => {
  const decision = {
    context: {album_id: "album-1"},
    jobKind: "scan",
    payload: {artist_id: "artist-1", album_ids: ["album-1", "album-2"]},
  };
  const selected = new Set(["stale"]);
  const state = {
    artist: {id: "artist-1", albums: [{id: "album-1"}, {id: "album-2"}]},
    artistMetadata: null, album: null, compAlbums: [], compGroups: [],
    compReportVisible: false, compTrack: null, editorMode: "artist",
    fallbackTracks: [], fallbackTrack: null, upscalePreview: null,
    workspace: "find", libraryMode: "albums", mediaSelected: selected,
    queuedDecision: decision, selectionRequest: 0,
  };
  const context = load(contextFunctions + queuedAlbumFunction, {
    state,
    api: async path => {
      assert.match(path, /^\/api\/album\?id=album-1$/);
      return {id: "album-1", artist_id: "artist-1", album_mbid: "release", candidates: []};
    },
    renderLibrary() {}, renderHero() {}, renderSources() {}, renderCandidates() {}, renderWorkspace() {},
    loadArtistMetadata() {}, markSelectedAlbum() {},
  });

  await context.focusQueuedAlbum(decision);

  assert.equal(state.libraryMode, "artists");
  assert.equal(state.editorMode, "album");
  assert.equal(state.album.id, "album-1");
  assert.deepEqual([...selected], ["artist-1", "album-1", "album-2"]);
  assert.equal(context.singleAlbumOverviewActive(), false);
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

test("a new queued COMP prompt clears the previous Artist track before async hydration", async () => {
  let resolveOverview;
  const overviewReady = new Promise(resolve => { resolveOverview = resolve; });
  let reviewRenders = 0;
  const decision = {
    context: {album_id: "new-comp", track_key: "new-track"},
    jobKind: "comp",
    payload: {artist_id: "new-artist", album_id: "new-comp", track_keys: ["new-track"]},
  };
  const state = {
    artist: {id: "previous-artist"}, artistMetadata: null, album: null,
    compAlbums: [], compGroups: [{id: "previous-comp", tracks: [{key: "previous-track"}]}],
    compReportVisible: true, compReportLoading: false, compPreviewMode: false,
    compTrack: {albumId: "previous-comp", key: "previous-track", file: "previous.mp3"},
    compReportRequest: 0, compStatusRequest: 0, compTrackRequest: 0,
    fallbackTracks: [], fallbackTrack: null, upscalePreview: {image: "stale-preview"},
    renderedPreview: {comp: {image: "stale-render"}}, compThumbnailPreview: {image: "stale-thumb"},
    pendingCompWrite: {owner: "stale"}, editorMode: "artist", workspace: "find",
    queuedDecision: decision, selectionRequest: 0, candidates: [],
  };
  const context = load(queuedAlbumFunction, {
    state,
    api: async path => {
      if (path.startsWith("/api/artist/overview")) return overviewReady;
      if (path.startsWith("/api/artist/comp-report")) return [
        {id: "new-comp", tracks: [{key: "new-track"}]},
      ];
      throw new Error(`unexpected API call: ${path}`);
    },
    $: () => ({textContent: "COMP Report"}),
    renderReview: () => { reviewRenders += 1; },
    renderLibrary() {}, renderHero() {}, renderSources() {}, renderCandidates() {}, renderWorkspace() {},
    loadArtistMetadata() {}, selectArtistCompTrack: async () => {},
  });

  const focusing = context.focusQueuedAlbum(decision);
  await Promise.resolve();
  assert.equal(state.compTrack, null);
  assert.equal(state.upscalePreview, null);
  assert.equal(state.renderedPreview.comp, null);
  assert.equal(state.compThumbnailPreview, null);
  assert.equal(state.pendingCompWrite, null);
  assert.equal(reviewRenders, 1);

  resolveOverview({artist: {id: "new-artist"}, comp_albums: []});
  await focusing;
  assert.equal(state.artist.id, "new-artist");
});

test("Artist and full-album COMP launches submit independent explicit scopes", async () => {
  const calls = [];
  const reviewed = [];
  const signal = {aborted: false};
  const state = {
    artist: {id: "artist-1"}, compReportVisible: true,
    compGroups: [{id: "comp-1", tracks: [
      {key: "artist-track", status: "blue"},
      {key: "completed-track", status: "green"},
    ]}],
    album: {id: "comp-1"},
    fallbackTracks: [
      {key: "artist-track", completed: false, status: "blue"},
      {key: "other-track", completed: false, status: "blue"},
      {key: "completed-track", completed: true, status: "green"},
    ],
  };
  const context = load(launchCompFunctions, {
    state,
    post: async (path, body, options) => { calls.push([path, body, options]); return {id: `job-${calls.length}`}; },
    reviewJob: async (id, quiet) => { reviewed.push([id, quiet]); },
    setStatus() {},
  });

  await context.launchArtistComp(signal);
  await context.launchFallback(signal);

  const submitted = JSON.parse(JSON.stringify(calls));
  assert.deepEqual(submitted[0].slice(0, 2), ["/api/jobs", {kind: "comp", payload: {
    artist_id: "artist-1", album_id: "comp-1", track_keys: ["artist-track"],
  }}]);
  assert.deepEqual(submitted[1].slice(0, 2), ["/api/jobs", {kind: "comp", payload: {
    album_id: "comp-1", track_keys: ["artist-track", "other-track"],
  }}]);
  assert.equal(calls[0][2], undefined);
  assert.equal(calls[1][2], undefined);
  assert.deepEqual(reviewed, [["job-1", true], ["job-2", true]]);
});

test("completed and Windows-overridden COMP tracks remain bulk reprocessable", async () => {
  const calls = [];
  const overridden = {key: "windows-track", status: "green", override: true};
  const state = {
    artist: {id: "artist-1"}, compReportVisible: true,
    compGroups: [{id: "comp-1", tracks: [overridden, {key: "finished", status: "green"}]}],
    album: {id: "comp-1"}, fallbackTracks: [overridden, {key: "finished", status: "green"}],
  };
  const context = load(launchCompFunctions, {
    state,
    compTrackOverride: track => track.override ? {writer: "windows"} : null,
    post: async (path, body) => { calls.push([path, body]); return {id: `job-${calls.length}`}; },
    reviewJob: async () => {}, setStatus() {},
  });

  await context.launchArtistComp();
  await context.launchFallback();

  assert.deepEqual(JSON.parse(JSON.stringify(calls.map(call => call[1].payload))), [
    {artist_id: "artist-1", album_id: "comp-1", track_keys: ["windows-track", "finished"]},
    {album_id: "comp-1", track_keys: ["windows-track", "finished"]},
  ]);
  assert.match(appSource, /state\.fallbackPreviewMode \|\| !reprocessable/);
  assert.match(operationsSource, /function fallbackCompLaunchTargets\(trackKey = ""\)/);
});

test("explicit blue, purple, and green tracks launch through both COMP controls without scope expansion", async () => {
  for (const status of ["blue", "purple", "green"]) {
    const calls = [];
    const selected = {key: `${status}-track`, status};
    const state = {
      artist: {id: "artist-1"}, compReportVisible: true,
      compTrack: {albumId: "comp-1", key: selected.key},
      compGroups: [{id: "comp-1", tracks: [selected, {key: "unrelated-blue", status: "blue"}]}],
      album: {id: "comp-1"}, fallbackTrack: {key: selected.key},
      fallbackTracks: [selected, {key: "other-blue", status: "blue"}],
    };
    const context = load(launchCompFunctions, {
      state,
      post: async (path, body) => { calls.push([path, body]); return {id: `job-${calls.length}`}; },
      reviewJob: async () => {}, setStatus() {},
    });
    await context.launchArtistComp(null, {albumId: "comp-1", trackKey: selected.key});
    await context.launchFallback(null, selected.key);
    assert.deepEqual(JSON.parse(JSON.stringify(calls.map(call => call[1].payload))), [
      {artist_id: "artist-1", album_id: "comp-1", track_keys: [selected.key]},
      {album_id: "comp-1", track_keys: [selected.key]},
    ]);
  }
});

test("both Artist COMP launch controls route through the scoped COMP action", async () => {
  const calls = [];
  const context = load(lowerLaunchFunction, {
    state: {editorMode: "artist", compReportVisible: true, queuedDecision: null, workspace: "find"},
    launchArtistComp: async () => { calls.push("artist-comp"); },
    launchFallback: async () => { calls.push("full-comp"); },
    launchArtist: async () => { calls.push("artist"); },
    searchArtist: async () => { calls.push("artist-search"); },
    searchAlbum: async () => { calls.push("album-search"); },
    skipQueuedAlbum: async () => { calls.push("skip"); },
  });

  await context.launchCandidateWork();
  context.state.compReportVisible = false;
  await context.launchCandidateWork();
  context.state.editorMode = "fallback";
  await context.launchCandidateWork();

  assert.deepEqual(calls, ["artist-comp", "artist", "full-comp"]);
  assert.ok((appSource.match(/launch\.addEventListener\("click", \(\) => runAction\(processing \? stopCandidateWork : activateCandidateWork\)\)/g) || []).length >= 2);
  assert.match(appSource, /searchButton"\)\.addEventListener\("click", \(\) => runAction\(activateCandidateWork\)\)/);
  assert.match(operationsSource, /async function stopCandidateWork\(\)/);
  assert.match(operationsSource, /state\.launchAbort\?\.abort\(\)/);
  assert.match(operationsSource, /action: "cancel", id: job\.id/);
  assert.match(appSource, /state\.launchStopping \? "STOPPING" : "STOP PROCESS"/);
  assert.match(appSource, /artistCompLaunch \|\| fallbackCompLaunch \? "LAUNCH COMP" : "LAUNCH"/);
  assert.match(cssSource, /\.musicbrainz-review-actions\{[^}]*position:sticky[^}]*bottom:0/);
  assert.match(appSource, /const processing = state\.launchActive \|\| activeLaunchJobs\(\)\.length > 0[\s\S]*?\["scan", "comp", "direct-musicbrainz"\]\.includes\(decision\.jobKind\)/);
  assert.match(appSource, /if \(processing\)[\s\S]*?"STOP PROCESS"[\s\S]*?stopCandidateWork/);
});

test("Artist COMP reviews the first job before later Album enqueue back-pressure", async () => {
  let releaseSecond;
  const blockedSecond = new Promise(resolve => { releaseSecond = resolve; });
  let markReviewed;
  const firstReviewed = new Promise(resolve => { markReviewed = resolve; });
  let posts = 0;
  const reviewed = [];
  const state = {
    artist: {id: "artist-1"}, compReportVisible: true, compPreviewMode: false,
    compGroups: [
      {id: "comp-1", tracks: [{key: "first", status: "green"}]},
      {id: "comp-2", tracks: [{key: "second", status: "green"}]},
    ],
  };
  const context = load(launchCompFunctions, {
    state,
    post: async () => {
      posts += 1;
      if (posts === 1) return {id: "job-first"};
      return blockedSecond;
    },
    reviewJob: async (id, quiet) => { reviewed.push([id, quiet]); markReviewed(); },
    setStatus() {},
  });

  const launch = context.launchArtistComp();
  await firstReviewed;
  await Promise.resolve();

  assert.equal(JSON.stringify(reviewed), JSON.stringify([["job-first", true]]));
  releaseSecond({id: "job-second"});
  await launch;
  assert.equal(posts, 2);
});

test("STOP cancels a COMP job accepted after its enqueue was already in flight", async () => {
  let acceptJob;
  const accepted = new Promise(resolve => { acceptJob = resolve; });
  const calls = [];
  let reviews = 0;
  const controller = new AbortController();
  const context = load(launchCompFunctions, {
    state: {sourceDiagnostics: []}, DOMException,
    post: async (path, body) => {
      calls.push([path, body]);
      return path === "/api/jobs" ? accepted : {};
    },
    reviewJob: async () => { reviews += 1; }, setStatus() {},
  });

  const launch = context.enqueueComp("comp-1", ["track-1"], "artist-1", controller.signal);
  controller.abort();
  acceptJob({id: "late-job"});

  await assert.rejects(launch, error => error?.name === "AbortError");
  assert.equal(reviews, 0);
  assert.deepEqual(JSON.parse(JSON.stringify(calls.at(-1))), [
    "/api/jobs/action", {action: "cancel", id: "late-job"},
  ]);
});

test("stopping Candidate processing cancels unseen scan and COMP queue work", () => {
  assert.match(operationsSource, /action: "cancel", kind: "scan"/);
  assert.match(operationsSource, /action: "cancel", kind: "comp"/);
  assert.match(operationsSource, /launchArtistComp\(signal\)/);
  assert.match(operationsSource, /if \(signal\?\.aborted\)/);
  assert.match(operationsSource, /action: "cancel", id: result\.id/);
  assert.match(operationsSource, /const latestJobs = await api\("\/api\/jobs"\)/);
  assert.match(operationsSource, /\+\+state\.fallbackTrackRequest/);
  assert.match(operationsSource, /\+\+state\.compTrackRequest/);
});

test("automatic COMP launch uses the status bar without a running-work dialog", async () => {
  let dialogs = 0, renders = 0, polls = 0;
  const statuses = [];
  const job = {
    id: "comp-running", kind: "comp", state: "running",
    progress: {message: "MusicBrainz identity lookup · Artist — Track · track 1 of 1"},
  };
  const context = load(reviewJobFunction, {
    state: {jobs: [job], reviewJobId: null, reviewJobState: ""},
    lastPrompt: "",
    $: () => ({open: false, close() {}}),
    setStatus: (...args) => { statuses.push(args); },
    renderJob: async () => { renders += 1; },
    dialog: () => { dialogs += 1; },
    pollJobs: async () => { polls += 1; },
  });

  await context.reviewJob(job.id, true);

  assert.equal(dialogs, 0);
  assert.equal(renders, 0);
  assert.equal(polls, 1);
  assert.equal(context.state.reviewJobId, job.id);
  assert.match(statuses[0][0], /track 1 of 1/);
});

test("an action-triggered job refresh is not dropped behind a periodic poll", async () => {
  let releaseFirst;
  let jobsCalls = 0;
  const firstJobs = new Promise(resolve => { releaseFirst = resolve; });
  const state = {
    server: {authenticated: true, setup_required: false}, jobs: [], embedQueue: [],
    queueControls: {}, reviewJobId: null, reviewJobState: "",
    albumOverrides: new Map(), compTrackOverrides: new Map(),
  };
  const context = load(pollJobsFunction, {
    state,
    api: async path => {
      if (path === "/api/jobs") {
        jobsCalls += 1;
        return jobsCalls === 1 ? firstJobs : [];
      }
      if (path === "/api/jobs/control-state") return {};
      return [];
    },
    renderQueue() {}, renderCandidateLaunchControl() {},
    applyAlbumCoordination: () => [], applyCompTrackCoordination: () => [],
    refreshOpposingAlbumThumbnails: async () => {}, refreshOpenCompReportAfterCoordination: async () => {},
    refreshCompletedAlbum: async () => {}, renderHero() {}, renderReview() {}, renderLibrary() {},
    refreshQueuedAlbumAfterSave: async () => {}, setStatus() {}, renderJob: async () => {},
    $: () => ({open: false, value: ""}),
  });

  const periodic = context.pollJobs();
  await new Promise(resolve => setImmediate(resolve));
  const actionRefresh = context.pollJobs();
  releaseFirst([]);
  await Promise.all([periodic, actionRefresh]);

  assert.equal(jobsCalls, 2);
});

test("ordinary queued scan work also stays in the status area without a modal", async () => {
  let dialogs = 0, renders = 0;
  const job = {id: "scan-queued", kind: "scan", state: "queued", progress: {}};
  const context = load(reviewJobFunction, {
    state: {jobs: [job], reviewJobId: null, reviewJobState: ""}, lastPrompt: "",
    $: () => ({open: false, close() {}}), setStatus() {},
    renderJob: async () => { renders += 1; }, dialog: () => { dialogs += 1; }, pollJobs: async () => {},
  });
  await context.reviewJob(job.id);
  assert.equal(dialogs, 0);
  assert.equal(renders, 0);
});

test("Artist COMP renders all album groups and previews only the selected current track", () => {
  const report = {
    children: [], hidden: true,
    replaceChildren() { this.children = []; },
    append(...children) { this.children.push(...children); },
  };
  const makeNode = (tag, className = "", text = "") => ({
    tag, className, text, children: [], listeners: {}, dataset: {},
    classList: {
      values: [],
      add(...values) { this.values.push(...values); },
      toggle(value, enabled) {
        this.values = this.values.filter(item => item !== value);
        if (enabled) this.values.push(value);
      },
    },
    append(...children) { this.children.push(...children); },
    setAttribute() {},
    addEventListener(event, handler) { this.listeners[event] = handler; },
  });
  const state = {
    editorMode: "artist", compReportVisible: true, compPreviewMode: true,
    compGroups: [
      {id: "comp-1", name: "Compilation One", tracks: [{key: "one", file: "one.flac", completed: true, status: "green", thumbnail: "/one.jpg"}]},
      {id: "comp-2", name: "Compilation Two", tracks: [{key: "two", file: "two.flac", completed: true, status: "green", thumbnail: "/two.jpg"}]},
      {id: "comp-3", name: "Compilation Three", tracks: [{key: "three", file: "three.flac", completed: true, status: "green", thumbnail: "/three.jpg"}]},
    ],
    compTrack: {albumId: "comp-2", key: "two", image: "/selected-current.jpg"},
    compCollapsed: new Set(), compTrackOverrides: new Map(), compBusy: false, compRepairEnabled: true,
    artist: {id: "artist-1", name: "Selected Artist"}, server: {mode: "write"},
  };
  const context = load(coordinationFunctions + "\n" + renderArtistCompReportFunction, {
    state, $: () => report, clear: element => element.replaceChildren(), node: makeNode,
    image: (src, className, alt) => ({tag: "img", src, className, alt}),
    compTrackIsReprocessable: track => ["blue", "purple", "green"].includes(track?.status),
    setStatus() {}, runAction() {}, repairArtistComp() {}, launchArtistComp() {},
    closeArtistCompReport() {}, selectArtistCompTrack() {},
  });

  context.renderArtistCompReport();

  const albumRows = report.children.filter(item => item.className === "fallback-report-album");
  const trackRows = report.children.filter(item => item.className.startsWith("fallback-track"));
  const previews = trackRows.flatMap(row => row.children.filter(item => item.tag === "img"));
  const counts = report.children.find(item => item.className.includes("comp-report-counts"));
  assert.deepEqual(albumRows.map(row => row.dataset.compAlbumId), ["comp-1", "comp-2", "comp-3"]);
  assert.ok(albumRows.every(row => row.children[0].className === "fallback-report-toggle"));
  assert.equal(albumRows[0].children[2].text, "1 track · 1 recorded, 0 need artwork");
  assert.equal(trackRows.length, 3);
  assert.equal(counts.children.find(item => item.className === "comp-count-override").text, "0 overridden");
  assert.ok(trackRows.every(row => typeof row.listeners.click === "function"));
  assert.deepEqual(previews.map(item => item.src), ["/selected-current.jpg"]);

  state.compPreviewMode = false;
  context.renderArtistCompReport();
  const previewsWhenOff = report.children
    .filter(item => item.className.startsWith("fallback-track"))
    .flatMap(row => row.children.filter(item => item.tag === "img"));
  assert.equal(previewsWhenOff.length, 0);
});

test("Artist COMP groups use the native descending fallback order", async () => {
  const state = {
    artist: {id: "artist-1", name: "Artist"}, compAlbums: [], compGroups: [],
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
    api: async () => [
      {id: "alpha", name: "Alpha Collection", year: "", tracks: []},
      {id: "zulu", name: "Zulu Collection", year: "", tracks: []},
      {id: "new", name: "Newer Collection", year: "2025", tracks: []},
    ],
    $: id => elements[id], renderHero() {}, renderCandidates() {}, renderReview() {},
    setStatus() {}, probeArtistCompStatuses() {}, requestAnimationFrame() {},
  });

  await context.openArtistCompReport();

  assert.deepEqual(Array.from(state.compGroups, group => group.id), ["new", "zulu", "alpha"]);
});

test("COMP summary and TRACK FILE columns reserve Album title space", () => {
  assert.match(cssSource, /\.comp-report-counts\{[^}]*justify-content:flex-start/);
  assert.match(appSource, /const summary = node\("div", "fallback-report-summary comp-report-counts"\)/);
  assert.match(appSource, /node\("span", "comp-count-override", `\$\{overridden\} overridden`\)/);
  assert.match(cssSource, /\.fallback-report-album\{[^}]*grid-template-columns:22px minmax\(0,1fr\) clamp\(170px,38%,310px\)/);
  assert.match(cssSource, /\.fallback-report-album-counts\{text-align:right\}/);
  assert.match(cssSource, /html\[data-theme=chalk\] \.fallback-report,html\[data-theme=light\] \.fallback-report\{background:#381821c9\}/);
});

test("all WebGUI MusicBrainz entry contexts use the canonical results workspace", () => {
  assert.equal((indexSource.match(/id="musicbrainzReview"/g) || []).length, 1);
  assert.match(appSource,
    /const active = decision\?\.jobKind === "comp" && decision\?\.kind !== "musicbrainz-results"/);
  assert.match(renderMusicBrainzFunction, /const directAlbum = decision\.jobKind === "direct-musicbrainz"/);
  assert.doesNotMatch(renderMusicBrainzFunction, /"EMBED Comp"/);
  assert.match(renderMusicBrainzFunction, /"Generate Candidate Artwork"/);
  assert.match(renderMusicBrainzFunction, /"STOP PROCESS"/);
});

test("EMBED Comp writes only the selected Artist COMP track at the preferred size", async () => {
  const calls = [];
  const state = {
    queuedDecision: null,
    editorMode: "artist",
    compReportVisible: true,
    compTrack: {albumId: "billboard-1994", key: "10k-because-the-night", image: "/track-art"},
    artist: {id: "10k-maniacs", name: "10,000 Maniacs"},
    fx: {comp: {Brightness: 12}}, upscalePreview: {image: "/upscaled"}, renderedPreview: {comp: {image: "/rendered"}},
  };
  const context = load(writeArtworkFunction, {
    state,
    post: async (path, payload) => { calls.push([path, payload]); return {file: "Because the Night.flac", width: 1200, height: 1200}; },
    componentPayload: (kind, image, extra) => ({kind, image, adjustments: state.fx.comp, ...extra}),
    refreshQueuedAlbumAfterSave: async () => {}, setStatus() {},
    submitQueuedDecision() {},
  });

  await context.writeArtwork();

  assert.deepEqual(JSON.parse(JSON.stringify(calls)), [["/api/artist/comp-embed", {
    kind: "comp", image: "/track-art", adjustments: {Brightness: 12}, artist_id: "10k-maniacs",
    album_id: "billboard-1994", track_key: "10k-because-the-night", upscale: true,
  }]]);
  assert.equal(state.upscalePreview, null);
  assert.deepEqual(JSON.parse(JSON.stringify(state.fx.comp)), {});
});

test("Full Fallback EMBED Comp uses the same direct preferred-size write", async () => {
  const calls = [];
  const state = {
    queuedDecision: null, editorMode: "fallback", compReportVisible: false,
    album: {id: "billboard-1994"},
    fallbackTrack: {key: "other-performer", image: "/other-track-art"},
    fx: {comp: {Exposure: -6}}, upscalePreview: {image: "/upscaled"}, renderedPreview: {comp: null},
  };
  const context = load(writeArtworkFunction, {
    state,
    post: async (path, payload) => { calls.push([path, payload]); return {file: "Other Performer.flac", width: 1200, height: 1200}; },
    componentPayload: (kind, image, extra) => ({kind, image, adjustments: state.fx.comp, ...extra}),
    refreshQueuedAlbumAfterSave: async () => {}, setStatus() {},
    submitQueuedDecision() {},
  });

  await context.writeArtwork();

  assert.deepEqual(JSON.parse(JSON.stringify(calls)), [["/api/comp/embed", {
    kind: "comp", image: "/other-track-art", adjustments: {Exposure: -6},
    album_id: "billboard-1994", track_key: "other-performer", upscale: true,
  }]]);
  assert.equal(state.upscalePreview, null);
  assert.deepEqual(JSON.parse(JSON.stringify(state.fx.comp)), {});
  assert.match(appSource, /const compTrackImage = state\.editorMode === "fallback"[\s\S]*?state\.fallbackPreviewMode \? state\.fallbackTrack\?\.image : ""/);
  assert.match(appSource, /state\.fallbackPreviewMode \? state\.fallbackTrack\?\.image : ""/);
});

test("COMP repair keeps full-Fallback and Artist membership validation independent", async () => {
  const fullCalls = [], messages = [];
  const fullState = {
    editorMode: "fallback", album: {id: "billboard-1994"},
    fallbackTracks: [
      {key: "10k-because-the-night", file: "Because the Night.flac", completed: true},
      {key: "other-performer", file: "Other Performer.flac", completed: true},
    ],
  };
  const full = load(repairFallbackFunction, {
    state: fullState,
    api: async path => path.startsWith("/api/fallback/tracks") ? fullState.fallbackTracks : {status: "purple", repairable: true},
    post: async (path, payload) => {
      fullCalls.push([path, payload]);
      if (payload.track_key === "10k-because-the-night") throw new Error("simulated first-track failure");
      return {status: "green", width: 1200, height: 1200};
    },
    renderHero() {}, probeFallbackStatuses: async () => {},
    setStatus: (message, kind) => messages.push([message, kind]),
  });

  await full.repairFallback();

  assert.deepEqual(JSON.parse(JSON.stringify(fullCalls)), [
    ["/api/comp/repair", {album_id: "billboard-1994", track_key: "10k-because-the-night"}],
    ["/api/comp/repair", {album_id: "billboard-1994", track_key: "other-performer"}],
  ]);
  assert.match(messages.at(-1)[0], /1 green.*1 failed.*simulated first-track failure/);

  const artistCalls = [];
  const artistState = {
    compBusy: false, compRepairEnabled: true,
    artist: {id: "10k-maniacs"}, compTrack: null,
    compGroups: [{id: "billboard-1994", tracks: [
      {key: "10k-because-the-night", file: "Because the Night.flac", status: "purple", repairable: true},
    ]}],
  };
  const artist = load(repairArtistCompFunction, {
    state: artistState, window: {confirm: () => true},
    api: async (path, options) => { artistCalls.push([path, JSON.parse(options.body)]); return {status: "green"}; },
    renderArtistCompReport() {}, setStatus() {}, selectArtistCompTrack: async () => {},
  });

  await artist.repairArtistComp();

  assert.deepEqual(JSON.parse(JSON.stringify(artistCalls)), [["/api/artist/comp-repair", {
    artist_id: "10k-maniacs", album_id: "billboard-1994", track_key: "10k-because-the-night",
  }]]);
});

test("COMP report colors only validated artwork as complete or repairable", () => {
  assert.match(appSource, /row\.classList\.add\(compTrackStatusClass\(item\)\)/);
  assert.match(appSource, /row\.classList\.add\(compTrackStatusClass\(track\)\)/);
  assert.match(appSource, /\["green", "purple"\]\.includes\(track\.status\) \? "Recorded" : "Blue"/);
});

test("completed COMP work chains only within an explicit Artist scope", async () => {
  async function completedReview(current, returned) {
    const state = {
      server: {authenticated: true, setup_required: false}, embedQueue: [],
      jobs: [{...current, state: "waiting"}], reviewJobId: current.id,
      reviewJobState: "waiting",
    };
    const context = load(pollJobsFunction, {
      state,
      api: async path => path === "/api/jobs" ? returned.map(job => ({progress: {}, ...job}))
        : {pause_after_album: false, pause_all_albums: false},
      renderQueue() {}, refreshCompletedAlbum: async () => {}, refreshQueuedAlbumAfterSave: async () => {},
      renderHero() {}, renderReview() {}, renderJob: async () => {}, setStatus() {},
      applyAlbumCoordination() { return false; },
      applyCompTrackCoordination() { return []; },
      $: () => ({open: false}),
    });
    await context.pollJobs();
    return state.reviewJobId;
  }

  const full = {id: "full-1", kind: "comp", payload: {album_id: "comp-1"}};
  assert.equal(await completedReview(full, [
    {...full, state: "done", result: {}},
    {id: "unrelated-full", kind: "comp", state: "queued", payload: {album_id: "comp-2"}},
  ]), null);

  const scoped = {id: "artist-1", kind: "comp", payload: {artist_id: "10k", album_id: "comp-1"}};
  assert.equal(await completedReview(scoped, [
    {...scoped, state: "done", result: {}},
    {id: "artist-2", kind: "comp", state: "queued", payload: {artist_id: "10k", album_id: "comp-2"}},
    {id: "other-artist", kind: "comp", state: "queued", payload: {artist_id: "other", album_id: "comp-3"}},
  ]), "artist-2");
});

test("PostgreSQL refresh completion reloads the library and reports exact projections", async () => {
  const job = {id: "index-1", kind: "index", state: "waiting", payload: {mode: "full"}, created_at: "2026-11-09T12:00:00Z", updated_at: "2026-11-09T12:00:00Z"};
  const state = {
    server: {authenticated: true, setup_required: false}, embedQueue: [],
    jobs: [job], reviewJobId: job.id, reviewJobState: "waiting", libraryMode: "artists",
  };
  const loaded = [], statuses = [];
  const context = load(pollJobsFunction, {
    state,
    api: async path => path === "/api/jobs" ? [{...job, state: "done", progress: {}, result: {
      mode: "full", artists: 51, identified_albums: 210, fallback_albums: 6,
      changed_albums: 4, removed_albums: 1,
    }, updated_at: "2026-11-09T12:04:47Z"}] : {pause_after_album: false, pause_all_albums: false},
    renderQueue() {}, refreshCompletedAlbum: async () => {}, refreshQueuedAlbumAfterSave: async () => {},
    renderHero() {}, renderReview() {}, renderJob: async () => {},
    loadLibrary: async query => { loaded.push(query); },
    setStatus: (...values) => { statuses.push(values); },
    applyAlbumCoordination() { return []; }, applyCompTrackCoordination() { return []; },
    $: id => ({open: false, value: id === "artistSearch" ? "adam" : ""}),
  });

  await context.pollJobs();

  assert.deepEqual(loaded, ["adam"]);
  assert.deepEqual(statuses.at(-1), [
    "PostgreSQL full refresh complete · 51 Artists · 210 Albums · 6 Fallback · elapsed 4m 47s · 4 changed · 1 removed",
    "ready",
  ]);
});

test("PostgreSQL refresh cancellation reports preserved published catalog", async () => {
  const job = {id: "index-1", kind: "index", state: "running", payload: {mode: "full"}, progress: {}};
  const state = {
    server: {authenticated: true, setup_required: false}, embedQueue: [],
    jobs: [job], reviewJobId: job.id, reviewJobState: "running", libraryMode: "artists",
  };
  const statuses = [];
  const context = load(pollJobsFunction, {
    state,
    api: async path => path === "/api/jobs" ? [{...job, state: "cancelled"}]
      : path === "/api/library/index-status" ? {generation: null}
      : {pause_after_album: false, pause_all_albums: false},
    renderQueue() {}, refreshCompletedAlbum: async () => {}, refreshQueuedAlbumAfterSave: async () => {},
    renderHero() {}, renderReview() {}, renderJob: async () => {}, loadLibrary: async () => {},
    setStatus: (...values) => { statuses.push(values); },
    applyAlbumCoordination() { return []; }, applyCompTrackCoordination() { return []; },
    $: () => ({open: false, value: "", hidden: false, style: {}, classList: {remove() {}, add() {}}}),
  });

  await context.pollJobs();

  assert.deepEqual(statuses.at(-1), [
    "PostgreSQL full refresh cancelled · published catalog preserved",
    "ready",
  ]);
});

test("PostgreSQL full refresh announces work before closing Settings", () => {
  assert.match(operationsSource, /PostgreSQL full refresh started · scanning the music library/);
  assert.match(operationsSource, /setStatus\("PostgreSQL full refresh started[^\n]+"busy"\);\s*\$\("workspaceDialog"\)\.close\(\)/);
  assert.match(operationsSource, /PostgreSQL \$\{job\.payload\?\.mode === "incremental" \? "incremental" : "full"\} refresh · \$\{completed\}\/\$\{total \|\| "\?"\} Albums/);
  assert.match(operationsSource, /PostgreSQL refresh complete · updating the library view/);
});

test("queued COMP candidates submit adjustment and upscale intent with the exact candidate number", async () => {
  const submitted = [];
  const state = {
    queuedDecision: {
      jobKind: "comp", payload: {artist_id: "artist-1", album_id: "comp-1", track_keys: ["track-1"]},
      context: {album_id: "comp-1", track_key: "track-1"}, selected: {queueValue: 4, image: "/candidate-4"},
    },
    editorMode: "artist", compReportVisible: true, artist: {id: "artist-1"},
    compTrack: {albumId: "comp-1", key: "track-1", image: "/current"},
    fx: {comp: {Contrast: 8}}, upscalePreview: {image: "/preview"},
  };
  const context = load(writeArtworkFunction, {
    state,
    musicBrainzExistingPreview: () => null,
    submitQueuedDecision: async value => { submitted.push(JSON.parse(value)); },
  });

  await context.writeArtwork();

  assert.deepEqual(JSON.parse(JSON.stringify(submitted)), [{
    action: "webgui-embed-track", number: 4,
    artwork: {image: "/candidate-4", adjustments: {Contrast: 8}, upscale: true},
  }]);
});

test("the internal COMP thumbnail preparation remains bound to the selected track", async () => {
  const requests = [];
  const statuses = [];
  const state = {
    queuedDecision: {
      jobKind: "comp", jobId: "job-1", nonce: "prompt-1",
      payload: {artist_id: "artist-1", album_id: "comp-1", track_keys: ["track-1"]},
      context: {album_id: "comp-1", track_key: "track-1"},
      selected: {image: "/candidate-1"},
    },
    editorMode: "artist", compReportVisible: true, artist: {id: "artist-1"},
    compTrack: {albumId: "comp-1", key: "track-1", image: "/current"}, fx: {comp: {Brightness: 7}},
    upscalePreview: {image: "/upscaled", width: 1800, height: 1800},
    compThumbnailPreview: null, pendingCompWrite: null,
  };
  const context = load(compThumbnailFunctions, {
    state,
    musicBrainzPreviewOption: () => null, musicBrainzExistingPreview: () => null,
    componentPayload: (_kind, image, extra) => ({image, adjustments: state.fx.comp, ...extra}),
    post: async (path, payload) => {
      requests.push({path, payload});
      return {prepared_id: "a".repeat(32), sha256: "b".repeat(64), image: "/prepared", width: 1200, height: 1200, bytes: 900, audio_written: false};
    },
    setStatus(...values) { statuses.push(values); }, renderPanes() {}, renderReview() {},
  });

  await context.prepareCompThumbnail({announce: false});

  assert.equal(requests[0].path, "/api/artist/comp-thumbnail-preview");
  assert.deepEqual(JSON.parse(JSON.stringify(requests[0].payload)), {
    image: "/candidate-1", adjustments: {Brightness: 7}, artist_id: "artist-1",
    album_id: "comp-1", track_key: "track-1", upscale: true,
  });
  assert.equal(state.compThumbnailPreview.image, "/prepared");
  assert.equal(state.compThumbnailPreview.audio_written, false);
  assert.deepEqual(statuses, []);
});

test("COMP Upscale Preview reports the applied scale and prepared embedded size", () => {
  const context = load(upscalePreviewStatusFunction, {});
  const status = context.upscalePreviewStatus(
    {
      width: 1800, height: 1800,
      reason: "Upscaled 1500 × 1500 → 1800 × 1800 (120% to 1800px Ideal)",
    },
    {width: 1200, height: 1200},
    true,
  );
  assert.equal(
    status,
    "Upscale preview · Upscaled 1500 × 1500 → 1800 × 1800 (120% to 1800px Ideal) · Embedded Thumb Ready · 1200 × 1200 · no audio written",
  );
  assert.doesNotMatch(status, /200%/);
});

test("queued EMBED Comp accepts an already prepared thumbnail identity and digest", async () => {
  const decisions = [];
  const prepared = {prepared_id: "a".repeat(32), sha256: "b".repeat(64), image: "/prepared", owner: "owner"};
  const state = {
    queuedDecision: {
      jobKind: "comp", jobId: "job-1",
      payload: {artist_id: "artist-1", album_id: "comp-1", track_keys: ["track-1"]},
      context: {album_id: "comp-1", track_key: "track-1"}, selected: {queueValue: 2, image: "/candidate-2"},
    },
    editorMode: "artist", compReportVisible: true, artist: {id: "artist-1"},
    compTrack: {albumId: "comp-1", key: "track-1", image: "/current"},
    fx: {comp: {}}, upscalePreview: null, pendingCompWrite: null,
  };
  const context = load(writeArtworkFunction, {
    state, activeCompPrepared: () => prepared,
    musicBrainzExistingPreview: () => null,
    submitQueuedDecision: async value => decisions.push(JSON.parse(value)),
  });

  await context.writeArtwork();

  assert.equal(decisions[0].artwork.prepared_id, prepared.prepared_id);
  assert.equal(decisions[0].artwork.prepared_sha256, prepared.sha256);
  assert.equal(state.pendingCompWrite.preview.image, "/prepared");
});

test("selected existing library artwork embeds directly without a second candidate screen", async () => {
  const decisions = [];
  const state = {
    queuedDecision: {
      kind: "musicbrainz-results", jobKind: "comp", jobId: "job-existing",
      context: {album_id: "comp-1", track_key: "track-1"}, payload: {album_id: "comp-1", track_keys: ["track-1"]},
      selectedExisting: 1, existingArtwork: [
        {image: "/existing-one", title: "One"},
        {image: "/existing-two", title: "Two"},
      ],
    },
    editorMode: "fallback", album: {id: "comp-1"}, fallbackTrack: {key: "track-1"},
    fx: {comp: {Contrast: 9}}, upscalePreview: {image: "/upscaled"}, pendingCompWrite: null,
  };
  const context = load(writeArtworkFunction, {
    state,
    musicBrainzExistingPreview: decision => decision.existingArtwork[decision.selectedExisting],
    submitQueuedDecision: async value => decisions.push(JSON.parse(value)),
  });

  await context.writeArtwork();

  assert.deepEqual(JSON.parse(JSON.stringify(decisions)), [{
    action: "webgui-embed-existing", number: 2,
    artwork: {image: "/existing-two", adjustments: {Contrast: 9}, upscale: true},
  }]);
});

test("selected MusicBrainz release embeds directly from the exact release preview", async () => {
  const decisions = [];
  const state = {
    queuedDecision: {
      kind: "musicbrainz-results", jobKind: "comp", jobId: "job-release",
      context: {album_id: "comp-1", track_key: "track-1"}, payload: {album_id: "comp-1", track_keys: ["track-1"]},
      selectedResult: 0, selectedExisting: null, existingArtwork: [],
      options: [{artwork_preview_url: "/api/image/exact-release"}],
    },
    editorMode: "fallback", album: {id: "comp-1"}, fallbackTrack: {key: "track-1"},
    fx: {comp: {Brightness: 7}}, upscalePreview: {image: "/upscaled"}, pendingCompWrite: null,
  };
  const context = load(writeArtworkFunction, {
    state,
    musicBrainzExistingPreview: () => null,
    musicBrainzArtworkSource: option => option?.artwork_preview_url || "",
    submitQueuedDecision: async value => decisions.push(JSON.parse(value)),
  });

  await context.writeArtwork();

  assert.deepEqual(JSON.parse(JSON.stringify(decisions)), [{
    action: "webgui-embed-release", number: 1,
    artwork: {
      image: "/api/image/exact-release",
      adjustments: {Brightness: 7}, upscale: true,
    },
  }]);
});

test("Fallback embedded art is hidden until explicit report preview mode", () => {
  const state = {
    editorMode: "fallback", fallbackPreviewMode: false,
    fallbackTrack: {image: "/embedded-cover"}, queuedDecision: null,
  };
  const context = load(editableArtworkFunctions, {
    state, musicBrainzPreviewOption: () => null, musicBrainzExistingPreview: () => null,
  });
  assert.equal(context.editableArtworkSource(), "");
  state.fallbackPreviewMode = true;
  assert.equal(context.editableArtworkSource(), "/embedded-cover");
  assert.match(operationsSource,
    /musicBrainzPreviewOption\(decision\) \|\| musicBrainzExistingCandidate\(decision\)\)\) return/);
  assert.match(operationsSource,
    /state\.fallbackPreviewMode \? state\.fallbackTrack\?\.image : ""/);
});

test("Artist COMP embedded art is hidden until explicit report preview mode", () => {
  const state = {
    editorMode: "artist", compReportVisible: true, compPreviewMode: false,
    compTrack: {image: "/embedded-cover"}, queuedDecision: null,
  };
  const context = load(editableArtworkFunctions, {
    state, musicBrainzPreviewOption: () => null, musicBrainzExistingPreview: () => null,
  });
  assert.equal(context.editableArtworkSource(), "");
  state.compPreviewMode = true;
  assert.equal(context.editableArtworkSource(), "/embedded-cover");
});

test("Album MusicBrainz editing cannot fall through to stale candidate artwork", () => {
  const state = {
    editorMode: "album",
    album: {id: "album-1", cover: "/current-cover"},
    selectedCover: {image: "/stale-provider-candidate"},
    queuedDecision: {
      kind: "musicbrainz-results", jobKind: "direct-musicbrainz",
      selectedResult: 0, hoveredResult: null, selectedExisting: null,
      options: [{title: "No artwork release", artwork_preview_error: "unavailable"}],
      existingArtwork: [],
    },
  };
  const context = load(editableArtworkFunctions, {
    state,
    musicBrainzArtworkSource: option => option?.artwork_preview_url || "",
  });

  assert.equal(context.editableArtworkSource(), "");
  state.queuedDecision.options[0].artwork_preview_url = "/api/image/exact-release-cover";
  assert.equal(context.editableArtworkSource(), "/api/image/exact-release-cover");
  state.queuedDecision.hoveredResult = 1;
  state.queuedDecision.options.push({title: "Hover-only missing artwork"});
  assert.equal(context.editableArtworkSource(), "/api/image/exact-release-cover");
});

test("Album embedding stages the exact editable artwork source", () => {
  const state = {album: {id: "album-1"}, upscalePreview: null};
  const context = load(coverPayloadFunction, {
    state,
    editableArtworkSource: () => "/exact-selected-release",
    componentPayload: (kind, image, extra) => ({kind, image, ...extra}),
  });

  assert.deepEqual(JSON.parse(JSON.stringify(context.coverPayload())), {
    kind: "cover", image: "/exact-selected-release", album_id: "album-1", upscale: false,
  });
});

test("Selected Artwork follows the report track instead of an unrelated queued COMP decision", async () => {
  const calls = [];
  const state = {
    queuedDecision: {
      jobKind: "comp", jobId: "old-job",
      payload: {artist_id: "old-artist", album_id: "old-comp", track_keys: ["old-track"]},
      context: {album_id: "old-comp", track_key: "old-track"},
      selected: {queueValue: 5, image: "/old-candidate"},
    },
    editorMode: "artist", compReportVisible: true,
    artist: {id: "a-ha"},
    compTrack: {albumId: "new-comp", key: "take-on-me", image: "/current-thumb"},
    fx: {comp: {}}, upscalePreview: {image: "/current-upscale"}, renderedPreview: {comp: null},
  };
  const context = load(writeArtworkFunction, {
    state,
    post: async (path, payload) => { calls.push([path, payload]); return {file: "Take on Me.flac", width: 1200, height: 1200}; },
    componentPayload: (kind, image, extra) => ({kind, image, ...extra}),
    refreshQueuedAlbumAfterSave: async () => {}, setStatus() {}, submitQueuedDecision() { throw new Error("stale queue was submitted"); },
  });

  await context.writeArtwork();

  assert.deepEqual(JSON.parse(JSON.stringify(calls)), [["/api/artist/comp-embed", {
    kind: "comp", image: "/current-thumb", artist_id: "a-ha", album_id: "new-comp",
    track_key: "take-on-me", upscale: true,
  }]]);
});

test("COMP Selected Artwork keeps the Windows button contract", () => {
  assert.match(operationsSource, /\$\("writeButton"\)\.textContent = compContext \? "EMBED Comp"/);
  assert.match(operationsSource, /\$\("embedBatchButton"\)\.textContent = "EMBED Batch"/);
  assert.match(operationsSource, /\$\("embedBatchButton"\)\.disabled = compContext \|\|/);
  assert.doesNotMatch(operationsSource, /textContent = compContext \? "EMBED Thumbs"/);
  assert.match(appSource, /\$\("writeButton"\)\.addEventListener\("click", \(\) => runAction\(writeArtwork\)\)/);
  assert.match(appSource, /\$\("embedBatchButton"\)\.addEventListener\("click", \(\) => runAction\(stageAlbum\)\)/);
  assert.match(operationsSource, /Writing selected cover…/);
  assert.match(operationsSource, /Writing selected Artist Art…/);
  assert.match(operationsSource, /Embedding selected COMP artwork…/);
  assert.match(operationsSource, /Staging cover and Album embedding…/);
  assert.match(appSource, /const previewResult = \(\) => \{[\s\S]*?previewMusicBrainzResult\(decision, resultIndex\)/);
  assert.match(appSource, /addEventListener\("mouseenter", previewResult\)/);
  assert.match(appSource, /const previewExisting = \(\) => \{[\s\S]*?clearCompDerivedPreview\(\);[\s\S]*?decision\.hoveredExisting = index; renderReview\(\);/);
  assert.match(appSource, /addEventListener\("mouseenter", previewExisting\)/);
  assert.doesNotMatch(appSource, /musicbrainz-release-actions[\s\S]*?const embed = node\("button", "primary", "EMBED Comp"\)/);
  assert.match(operationsSource, /\$\("writeButton"\)\.textContent = decision\.jobKind === "comp" \? "EMBED Comp"/);
  assert.match(appSource, /"Generate Candidate Artwork"/);
  assert.match(appSource, /Search every enabled artwork source for this selected MusicBrainz release/);
  assert.match(appSource, /candidates\.addEventListener\("click", \(\) => runAction\(\(\) => submitQueuedDecision\(resultIndex \+ 1\)\)\)/);
  assert.match(appSource, /\["scan", "comp", "direct-musicbrainz"\]\.includes\(decision\.jobKind\)/);
  assert.match(operationsSource, /\["scan", "comp", "direct-musicbrainz"\]\.includes\(state\.queuedDecision\?\.jobKind\)/);
  assert.doesNotMatch(appSource, /"Use Selected Album"/);
  assert.match(appSource, /else if \(musicBrainzPreviewOption\(decision\)\)[\s\S]*?MusicBrainz Release Cover/);
});

test("saved panel sizes restore only after the authenticated workspace is visible", () => {
  assert.doesNotMatch(appSource, /\nrestoreLayout\(\);\ninstallSplitter/);
  assert.match(operationsSource, /document\.body\.className = "authenticated";[\s\S]*renderAll\(\);[\s\S]*requestAnimationFrame\(\(\) => \{ restoreLayout\(\); fitReviewToViewport\(\); \}\)/);
  assert.match(appSource, /const layoutStorageKey = "splined\.browser-preview\.layout"/);
  assert.match(appSource, /function saveLayout\(\)[\s\S]*localStorage\.setItem\(layoutStorageKey/);
});

test("changing report tracks does not cancel an active COMP batch", async () => {
  const calls = [];
  const statuses = [];
  const state = {
    queuedDecision: {
      jobKind: "comp", jobId: "old-job",
      payload: {artist_id: "old-artist", album_id: "old-comp", track_keys: ["old-track"]},
      context: {album_id: "old-comp", track_key: "old-track"},
    },
    reviewJobId: "old-job", reviewJobState: "waiting", jobs: [],
  };
  const context = load(compDecisionFunctions, {
    state,
    post: async (path, payload) => calls.push([path, payload]),
    activeLaunchJobs: () => [{id: "old-job", kind: "comp", state: "waiting"}],
    setStatus: message => statuses.push(message),
    renderCandidateLaunchControl() {},
  });

  const allowed = await context.cancelMismatchedCompWork({artist_id: "new-artist", album_id: "new-comp", track_key: "new-track"});

  assert.equal(allowed, false);
  assert.deepEqual(calls, []);
  assert.equal(state.queuedDecision.jobId, "old-job");
  assert.equal(state.reviewJobId, "old-job");
  assert.match(statuses[0], /STOP PROCESS/);
});

test("Artist navigation cannot replace an active COMP batch", async () => {
  const statuses = [];
  let cancelled = false;
  const state = {launchActive: true, queuedDecision: {jobKind: "comp"}};
  const context = load(chooseArtistFunction, {
    state,
    activeLaunchJobs: () => [{id: "comp-1", kind: "comp", state: "waiting"}],
    setStatus: message => statuses.push(message),
    renderCandidateLaunchControl() {},
    cancelActiveCompWork: async () => { cancelled = true; },
  });

  await context.chooseArtist("other-artist");

  assert.equal(cancelled, false);
  assert.match(statuses[0], /STOP PROCESS/);
});

test("Album navigation cannot replace an active COMP batch", async () => {
  const statuses = [];
  let cancelled = false;
  const state = {launchActive: false, queuedDecision: {jobKind: "comp"}};
  const context = load(chooseAlbumFunction, {
    state,
    activeLaunchJobs: () => [],
    setStatus: message => statuses.push(message),
    renderCandidateLaunchControl() {},
    cancelActiveCompWork: async () => { cancelled = true; },
  });

  await context.chooseAlbum("other-album");

  assert.equal(cancelled, false);
  assert.match(statuses[0], /STOP PROCESS/);
});

test("MusicBrainz release prompt stays on the Artist COMP track", async () => {
  const focused = [];
  const state = {
    queuedDecision: null, candidates: [{image: "old"}], workspace: "embed",
    activeSource: "amazon", compare: new Set(["old"]),
  };
  const options = [{
    id: "release-1994", recording_id: "recording-because-the-night",
    recording_artist: "10,000 Maniacs", track: "Because the Night",
    title: "Billboard Hot 100 Singles of 1994", release_class: "Compilation", score: "100",
  }];
  const job = {
    id: "job-1", kind: "comp",
    payload: {artist_id: "10k-maniacs", album_id: "billboard-1994", track_keys: ["10k-because-the-night"]},
    progress: {current: {
      fallback_authority: {artist: "10,000 Maniacs", track: "Because the Night"},
      musicbrainz_existing: {items: [{title: "Campfire Songs", image: "/api/image/existing"}]},
    }},
  };
  const prompt = {nonce: "nonce-1", context: {
    kind: "musicbrainz-results", album_id: "billboard-1994", track_key: "10k-because-the-night",
    identity: {status: "valid", friendly_artist: "10,000 Maniacs", friendly_title: "Because the Night"},
    current_album: {artist: "Various Artists", title: "Billboard Hot 100 Singles of 1994"},
  }};
  const context = load(queuedMusicBrainzFunction, {
    state,
    $: () => ({open: false, close() {}}),
    resetCandidateFilters() {},
    focusQueuedAlbum: async decision => { focused.push(decision); },
    renderWorkspace() {}, renderSources() {}, renderCandidates() {}, renderReview() {}, setStatus() {},
  });

  await context.presentQueuedMusicBrainz(job, prompt, options);

  assert.equal(state.workspace, "find");
  assert.equal(state.candidates.length, 0);
  assert.equal(state.queuedDecision.kind, "musicbrainz-results");
  assert.equal(state.queuedDecision.context.track_artist, "10,000 Maniacs");
  assert.equal(state.queuedDecision.context.track_title, "Because the Night");
  assert.deepEqual(state.queuedDecision.payload.track_keys, ["10k-because-the-night"]);
  assert.equal(state.queuedDecision.options[0].recording_id, "recording-because-the-night");
  assert.equal(state.queuedDecision.existingArtwork[0].title, "Campfire Songs");
  assert.equal(state.queuedDecision.context.identity.status, "valid");
  assert.equal(state.queuedDecision.selectedResult, null);
  assert.equal(focused.length, 1);

  state.queuedDecision.selectedResult = 0;
  state.queuedDecision.artistFilter = "10,000 Maniacs";
  await context.presentQueuedMusicBrainz(job, prompt, options);
  assert.equal(state.queuedDecision.selectedResult, 0);
  assert.equal(state.queuedDecision.artistFilter, "10,000 Maniacs");
  assert.match(appSource, /musicbrainz-existing-select", selected \? "SELECTED" : "SELECT EXISTING"/);
  assert.match(operationsSource, /async function selectExistingMusicBrainzArtwork\(index\)/);
  assert.match(operationsSource, /async function useSelectedExistingMusicBrainzArtwork\(\)/);
});

test("existing MusicBrainz library art becomes the active COMP candidate", async () => {
  const statuses = [];
  const state = {
    queuedDecision: {
      jobId: "comp-existing", jobKind: "comp", nonce: "mb-results",
      selected: null, previousCandidates: [], previousSourceDiagnostics: [],
    },
    candidates: [], sourceDiagnostics: [], activeSource: "", compare: new Set(),
    pendingCompWrite: null, workspace: "find",
    pendingMusicBrainzExisting: {
      jobId: "comp-existing", albumId: "billboard-2004", trackKey: "here-without-you",
      title: "Away From the Sun",
    },
  };
  const context = load(queuedCandidateSelectionFunctions, {
    state,
    $: () => ({open: false}),
    resetCandidateFilters() {},
    focusQueuedAlbum: async () => {},
    renderWorkspace() {}, renderSources() {}, renderCandidates() {}, renderReview() {},
    setStatus: (...values) => statuses.push(values),
  });

  await context.presentQueuedCandidates(
    {id: "comp-existing", kind: "comp", payload: {}, progress: {current: {fallback_authority: {}, diagnostics: {items: []}}}},
    {nonce: "candidate-review", context: {album_id: "billboard-2004", track_key: "here-without-you"}},
    [{image: "/api/image/away", source: "local-library", title: "Away From the Sun", number: 1, width: 1200, height: 1200}],
  );

  assert.equal(state.queuedDecision.selected.image, "/api/image/away");
  assert.equal(state.queuedDecision.selected.queueValue, 1);
  assert.equal(state.pendingMusicBrainzExisting, null);
  assert.match(statuses.at(-1)[0], /Upscale Preview or EMBED Comp/);
});

test("MusicBrainz result decisions ignore stale image candidates and preserve result numbering", async () => {
  let presented;
  const options = [
    {id: "other-release", artist: "Other Performer", track: "Other Track", title: "Other Release"},
    {id: "release-1994", recording_artist: "10,000 Maniacs", track: "Because the Night", title: "Billboard Hot 100 Singles of 1994"},
  ];
  const job = {
    id: "job-1", kind: "comp", state: "waiting", payload: {}, error: "",
    progress: {message: "review", prompt: {nonce: "nonce-1", context: {kind: "musicbrainz-results", options}},
      current: {candidates: {items: [{image: "/stale.jpg"}]}}},
  };
  const context = load(renderJobFunction, {
    presentQueuedMusicBrainz: async (...args) => { presented = args; },
    presentQueuedCandidates: async () => { throw new Error("stale image candidates were presented"); },
  });

  await context.renderJob(job);

  assert.equal(presented[0], job);
  assert.deepEqual(presented[2], options);

  const renderer = load(renderMusicBrainzFunction, {});
  const grouped = renderer.musicBrainzFamilies([
    {...options[1], family_id: "campfire", family_title: "Campfire Songs", score: "95", release_class: "Compilation", release_artist: "10,000 Maniacs"},
    {...options[1], id: "release-2004-eu", family_id: "campfire", family_title: "Campfire Songs", score: "95", release_class: "Compilation", release_artist: "10,000 Maniacs"},
    {...options[0], family_id: "appearance", family_title: "Other Release", score: "70", release_class: "Soundtrack", release_artist: "Various Artists"},
  ]);
  assert.equal(grouped.length, 2);
  assert.equal(grouped[0].title, "Campfire Songs");
  assert.equal(grouped[0].score, 95);
  assert.equal(grouped[0].options.length, 2);
  assert.equal(grouped[0].section, 0);
  assert.equal(grouped[1].section, 2);
  assert.match(appSource, /originalIndex[\s\S]*previewMusicBrainzResult\(decision, resultIndex, true\)/);
});

test("MusicBrainz zero-result prompts open the authority workspace", async () => {
  let presented;
  const job = {id: "job-empty", kind: "comp", state: "waiting", payload: {}, error: "", progress: {
    prompt: {nonce: "nonce-empty", context: {kind: "musicbrainz-results", options: [], identity: {status: "failed"}}},
    current: {diagnostics: {items: [["musicbrainz", "No recording matched the supplied identity"]]}},
  }};
  const context = load(renderJobFunction, {
    presentQueuedMusicBrainz: async (...args) => { presented = args; },
    presentQueuedCandidates: async () => { throw new Error("candidate dock opened"); },
  });
  await context.renderJob(job);
  assert.equal(presented[0], job);
  assert.deepEqual(presented[2], []);
});

test("a MusicBrainz lookup without matches surfaces diagnostics instead of a silent prompt", async () => {
  const messages = [];
  const statuses = [];
  const body = {children: [], append(...children) { this.children.push(...children); }};
  const context = load(renderJobFunction, {
    dialog: () => body,
    node: (tag, className, text) => ({tag, className, text, children: [], append(...children) { this.children.push(...children); }}),
    field: () => ({value: ""}),
    actionButton(label) { messages.push(label); },
    setStatus: (...args) => { statuses.push(args); },
    queueAction() {}, pollJobs() {}, refreshQueuedAlbumAfterSave() {}, runAction() {},
    state: {jobs: [], artist: null, compReportVisible: false},
    distinctDiagnostics: items => items.map(item => Array.isArray(item) ? item.join(" · ") : String(item)),
    $: () => ({close() {}}),
  });
  const job = {
    id: "job-empty", kind: "comp", state: "waiting", payload: {name: "Billboard Hot 100 Singles of 1994"}, error: "",
    progress: {message: "waiting", prompt: {nonce: "nonce-empty", context: {
      kind: "fallback-picker", track_artist: "10,000 Maniacs", track_title: "Because the Night",
    }}, current: {diagnostics: {items: [["musicbrainz", "No recording matched the supplied identity"]]}}},
  };

  await context.renderJob(job);

  assert.ok(body.children.some(item => item.text?.includes("No recording matched")));
  assert.ok(body.children.some(item => item.text?.includes("No artwork candidates are available")));
  assert.ok(statuses.some(([message, level]) => message.includes("Because the Night") && level === "error"));
  assert.equal(messages.includes("Use suggested"), false);
  assert.equal(messages.includes("MusicBrainz matches"), true);
});

test("a restart pause moves provider failures to source status instead of repeating the diagnostic list", async () => {
  const body = {children: [], append(...children) { this.children.push(...children); }};
  const actions = [], state = {jobs: [], artist: null, compReportVisible: false, sourceDiagnostics: []};
  let sourceRenders = 0;
  const context = load(renderJobFunction, {
    dialog: () => body,
    node: (tag, className, text) => ({tag, className, text, children: [], append(...children) { this.children.push(...children); }}),
    field: () => ({value: ""}),
    actionButton(label) { actions.push(label); },
    setStatus() {}, queueAction() {}, pollJobs() {}, refreshQueuedAlbumAfterSave() {}, runAction() {},
    renderSources() { sourceRenders += 1; }, state,
    distinctDiagnostics: items => items.map(item => Array.isArray(item) ? item.join(" · ") : String(item)),
  });
  const diagnostics = [["amazon", "Amazon Store search returned HTTP 503"], ["discogs", "policy-filtered: below minimum"]];
  await context.renderJob({
    id: "paused-comp", kind: "comp", state: "paused", payload: {name: "Billboard Hot 100 Singles of 1994"},
    error: "Interrupted by server restart", progress: {current: {diagnostics: {items: diagnostics}}},
  });

  assert.deepEqual(state.sourceDiagnostics, diagnostics);
  assert.equal(sourceRenders, 1);
  assert.equal(body.children.some(item => item.text?.includes("HTTP 503")), false);
  assert.equal(body.children.some(item => item.text?.includes("policy-filtered")), false);
  assert.equal(actions.includes("Resume"), true);
});

test("Album Track Embedding shows album details and excludes COMP and scan jobs", () => {
  const elements = {
    embedQueue: {children: [], replaceChildren() { this.children = []; }, append(...children) { this.children.push(...children); }},
    embedTab: {classList: {toggle() {}}},
    removeAllStaged: {disabled: false}, startStaged: {disabled: false},
    pauseAfterAlbum: {disabled: false, textContent: ""},
    pauseAllAlbums: {disabled: false, textContent: ""},
    embedQueueStatus: {textContent: ""}, embedQueueFooter: {textContent: ""},
  };
  const state = {embedQueue: [], queueControls: {pause_after_album: false, pause_all_albums: false}, jobs: [
    {id: "album-1", kind: "embed-album", state: "staged", payload: {
      album_id: "psychedelic", artist: "13th Floor Elevators", name: "The Psychedelic Sounds of the 13th Floor Elevators",
      cover: "/cover.jpg", cover_width: 1800, cover_height: 1800, embedded_side: 1200, tracks: 21,
      staged_valid: true, staged_error: "", preferred_size: true,
    }, progress: {}},
    {id: "comp-1", kind: "comp", state: "waiting", payload: {name: "Billboard 1994"}, progress: {}},
    {id: "scan-1", kind: "scan", state: "paused", payload: {name: "Artist scan"}, progress: {}},
  ]};
  const node = (tag, className = "", text = "") => ({
    tag, className, text, children: [], append(...children) { this.children.push(...children); },
  });
  const context = load(renderQueueFunction, {
    state, $: id => elements[id], clear: element => element.replaceChildren(), node,
    image: (src, className, alt) => ({tag: "img", src, className, alt}),
    actionButton: (label, action, host) => host.append({tag: "button", label, action}),
    reviewJob() {}, queueAction() {},
  });

  context.renderQueue();

  assert.equal(elements.embedQueue.children.length, 1);
  const card = elements.embedQueue.children[0];
  assert.equal(card.children[0].src, "/cover.jpg");
  const copy = card.children.find(child => child.className === "row-copy");
  assert.ok(copy.children.some(child => child.text.includes("13th Floor Elevators — The Psychedelic Sounds")));
  assert.ok(copy.children.some(child => child.text.includes("0/21 tracks") && child.text.includes("1200 × 1200")));
  assert.ok(copy.children.some(child => child.text.includes("staged cover resolution 1800 × 1800")));
  assert.ok(copy.children.some(child => child.text.includes("Preferred embedded size")));
  assert.equal(state.embedQueue.length, 1);
  assert.equal(elements.embedQueueStatus.textContent, "1 active · 1 staged · 0 paused · 0 failed.");
  assert.equal(elements.embedQueueFooter.textContent, elements.embedQueueStatus.textContent);
  assert.equal(elements.startStaged.disabled, false);
  assert.equal(elements.pauseAfterAlbum.textContent, "Pause After Album");
  assert.equal(elements.pauseAllAlbums.textContent, "Pause ALL Albums");
  assert.match(operationsSource, /queueAction\("start", null, null, null, "embed-album"\)/);
  assert.match(operationsSource, /state\.queueControls\.pause_after_album \? "resume-queue" : "pause-after"/);
  assert.match(operationsSource, /state\.queueControls\.pause_all_albums \? "resume-all" : "pause-all"/);
  assert.ok(operationsSource.includes('job.state === "failed" ? "Retry" : "Resume"'));
  assert.match(appSource, /queueAction\("clear", null, null, null, "embed-album"\)/);
});

test("EMBED All Albums stages only identified Artist Albums with saved covers", async () => {
  const calls = [];
  const statuses = [];
  const state = {artist: {albums: [
    {id: "regular", name: "Regular", album_mbid: "11111111-1111-4111-8111-111111111111", tracks: 10, cover: "/regular.jpg"},
    {id: "featured", name: "Featured", album_mbid: "22222222-2222-4222-8222-222222222222", tracks: 1, cover: "/featured.jpg"},
    {id: "identified-compilation", name: "Compilation Release", album_mbid: "33333333-3333-4333-8333-333333333333", compilation: true, tracks: 2, cover: "/compilation.jpg"},
    {id: "malformed", name: "Malformed", album_mbid: "release-not-a-uuid", tracks: 3, cover: "/malformed.jpg"},
    {id: "fallback", name: "Fallback", album_mbid: "", tracks: 20, cover: "/fallback.jpg"},
    {id: "no-cover", name: "No Cover", album_mbid: "44444444-4444-4444-8444-444444444444", tracks: 8, cover: null},
  ]}};
  const context = load(albumEligibilityFunctions + stageAllAlbumsFunction, {
    state,
    post: async (_path, body) => {
      calls.push(body.payload.album_id);
      return {id: body.payload.album_id, already_queued: body.payload.album_id === "featured"};
    },
    pollJobs: async () => {},
    setStatus: (...args) => { statuses.push(args); },
  });

  await context.stageAllArtistAlbums();

  assert.deepEqual(calls, ["regular", "featured", "identified-compilation"]);
  assert.ok(statuses.at(-1)[0].includes("2 Albums staged"));
  assert.ok(statuses.at(-1)[0].includes("1 already queued"));
});

test("Album embedding eligibility requires a UUID but permits identified compilation releases", () => {
  const context = load(albumEligibilityFunctions, {});
  const valid = {album_mbid: "33333333-3333-4333-8333-333333333333", compilation: true, tracks: 2, status: "incomplete"};
  assert.equal(context.albumEmbeddingEligible(valid), true);
  assert.equal(context.artistAlbumEligible(valid), true);
  assert.equal(context.albumEmbeddingEligible({...valid, album_mbid: "release-not-a-uuid"}), false);
  assert.equal(context.artistAlbumEligible({...valid, album_mbid: "release-not-a-uuid"}), false);
  assert.equal(context.albumEmbeddingEligible({...valid, album_mbid: ""}), false);
});

test("COMP Advanced IDs omit editable Release IDs and candidate cards retain compact metadata", () => {
  assert.doesNotMatch(appSource, /placeholder = "Release MBID/);
  assert.doesNotMatch(operationsSource, /field\(details, "Release ID"/);
  assert.match(appSource, /candidate-resolution/);
  assert.match(appSource, /candidate-quality/);
  assert.match(appSource, /Diagnostics \(\$\{diagnostics\.length\}\)/);
  assert.match(appSource, /artist_id: artist\.value, recording_id: recording\.value/);
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
    setStatus: () => {},
    artistAlbumEligible: album => !!album.album_mbid && !["bypassed", "timeout"].includes(album.status)
      && !(album.status === "processed" && album.cover_found && album.embedded >= album.tracks),
  });

  await context.launchArtist();

  assert.equal(calls.length, 1);
  assert.equal(calls[0][0], "/api/jobs");
  assert.equal(calls[0][1].kind, "scan");
  assert.equal(calls[0][1].payload.artist_id, "artist-1");
  assert.deepEqual([...calls[0][1].payload.album_ids], ["own", "featured"]);
  assert.deepEqual([...selected], ["artist-1", "own", "featured", "complete", "bypassed", "timeout", "comp"]);
});

test("single Album overview is limited to the global Albums library", () => {
  const artistContext = load(contextFunctions, {
    state: {libraryMode: "artists", editorMode: "album", album: {id: "album-1"}},
  });
  assert.equal(artistContext.singleAlbumOverviewActive(), false);

  const albumContext = load(contextFunctions, {
    state: {libraryMode: "albums", editorMode: "album", album: {id: "album-1"}},
  });
  assert.equal(albumContext.singleAlbumOverviewActive(), true);
});

test("successful EMBED Batch invalidates stale handles and refreshes the Album before polling", async () => {
  const artist = {id: "artist-1", albums: [{id: "album-1", cover: "old-artist-cover"}]};
  const album = {id: "album-1", name: "Album", cover: "cover"};
  const refreshed = {...album, cover: "fresh-cover", cover_meta: {width: 1800, height: 1800}};
  const embedQueue = [{id: "persistent-job"}];
  const order = [], statuses = [];
  const state = {
    artist, album, embedQueue,
    albumsIndex: [{id: "album-1", cover: "old-index-cover"}],
    candidates: [{image: "candidate"}], selectedCover: {image: "candidate"},
    upscalePreview: {image: "upscale"}, renderedPreview: {album: {image: "rendered"}},
    compare: new Set(["candidate"]), reviewCleared: false,
    selectionRequest: 4, searchRequest: 7,
    queuedDecision: null,
  };
  const context = load(stageAlbumFunction, {
    state,
    post: async () => { order.push("post"); return {id: "job-1", already_queued: false}; },
    api: async path => {
      order.push(path);
      assert.equal(state.selectedCover, null);
      assert.equal(state.album.cover, null);
      return refreshed;
    },
    coverPayload: () => ({album_id: album.id, image: album.cover}),
    activeArtworkDecision: () => state.queuedDecision,
    pollJobs: async () => { order.push("poll"); }, renderHero() {}, renderReview() {}, renderSources() {}, renderCandidates() {}, renderPanes() {},
    renderWorkspace() {},
    resetCandidateFilters() {}, setStatus: (...values) => statuses.push(values), auditEvent() {}, encodeURIComponent,
  });

  await context.stageAlbum();

  assert.equal(state.artist, artist);
  assert.equal(state.album.cover, "fresh-cover");
  assert.equal(state.artist.albums[0].cover, "fresh-cover");
  assert.equal(state.albumsIndex[0].cover, "fresh-cover");
  assert.equal(state.embedQueue, embedQueue);
  assert.equal(state.candidates.length, 0);
  assert.equal(state.selectedCover, null);
  assert.equal(state.upscalePreview, null);
  assert.equal(Object.keys(state.renderedPreview).length, 0);
  assert.equal(state.compare.size, 0);
  assert.equal(state.reviewCleared, false);
  assert.equal(state.selectionRequest, 5);
  assert.equal(state.searchRequest, 8);
  assert.deepEqual(order, ["post", "/api/album?id=album-1", "poll"]);
  assert.match(statuses.at(-1)[0], /Cover written and album staged/);
});

test("staging or awaiting Album embedding closes direct MusicBrainz results", async () => {
  const album = {id: "album-1", name: "Album", cover: "/cover"};
  const state = {
    album, queuedDecision: {
      kind: "musicbrainz-results", jobKind: "direct-musicbrainz",
      context: {album_id: album.id}, payload: {album_id: album.id},
    },
    reviewJobId: "", reviewJobState: "", candidates: [], sourceDiagnostics: [],
  };
  let workspaceRenders = 0;
  const context = load(stageAlbumFunction, {
    state,
    post: async () => ({id: "existing-job", already_queued: true}),
    coverPayload: () => ({album_id: album.id, image: album.cover}),
    activeArtworkDecision: () => state.queuedDecision,
    pollJobs: async () => {}, renderHero() {}, renderReview() {}, renderSources() {}, renderCandidates() {}, renderPanes() {},
    renderWorkspace: () => { workspaceRenders += 1; },
    resetCandidateFilters() {}, setStatus() {}, auditEvent() {}, encodeURIComponent,
  });

  await context.stageAlbum();

  assert.equal(state.queuedDecision, null);
  assert.equal(workspaceRenders, 1);
});

test("fully embedded Album artwork remains editable and queued candidates take precedence", () => {
  const state = {
    queuedDecision: null,
    editorMode: "album",
    album: {id: "album-1", cover: "/api/image/current", tracks: 128, embedded: 128},
    selectedCover: null,
    fallbackTrack: null,
    compTrack: null,
    compReportVisible: false,
    radioCurrent: false,
    radioArtist: false,
    radioBanner: false,
    selectedPortrait: null,
    selectedBanner: null,
    artist: null,
  };
  const context = load(editableArtworkFunctions, {
    state, musicBrainzPreviewOption: () => null, musicBrainzExistingPreview: () => null,
  });

  assert.equal(context.editableArtworkSource(), "/api/image/current");
  state.queuedDecision = {
    jobId: "scan-1", jobKind: "scan", nonce: "prompt-1",
    selected: {image: "/api/image/candidate"},
  };
  assert.equal(context.editableArtworkSource(), "/api/image/candidate");
  assert.match(operationsSource, /state\.editorMode==='album'[\s\S]*?const source=editableArtworkSource\(\)/);
});

test("repeated queued-candidate polling preserves touch targets and the browser selection", async () => {
  const raw = {image: "/api/image/candidate", source: "MB / CAA", title: "Recommended", width: 1200, height: 1200, number: 1};
  const selected = {...raw, name: raw.title, kind: "cover", queueNumber: 1, queueValue: 1};
  const compare = new Set([selected.image]);
  const state = {
    queuedDecision: {
      jobId: "scan-1", jobKind: "scan", nonce: "prompt-1", selected,
      previousCandidates: [], previousSourceDiagnostics: [],
    },
    candidates: [selected], sourceDiagnostics: [], activeSource: "musicbrainz", compare,
    pendingCompWrite: null, workspace: "find",
  };
  let renders = 0;
  const context = load(queuedCandidateSelectionFunctions, {
    state,
    $: () => ({open: false}),
    resetCandidateFilters: () => { throw new Error("unchanged prompt reset candidate filters"); },
    focusQueuedAlbum: async () => { throw new Error("unchanged prompt refocused Album"); },
    renderWorkspace: () => { renders += 1; }, renderSources: () => { renders += 1; },
    renderCandidates: () => { renders += 1; }, renderReview: () => { renders += 1; },
    setStatus: () => { renders += 1; },
  });

  await context.presentQueuedCandidates(
    {id: "scan-1", kind: "scan", payload: {}, progress: {current: {fallback_authority: {}, diagnostics: {items: []}}}},
    {nonce: "prompt-1", context: {}},
    [raw],
  );

  assert.equal(state.queuedDecision.selected, selected);
  assert.equal(state.candidates[0], selected);
  assert.equal(state.activeSource, "musicbrainz");
  assert.equal(state.compare, compare);
  assert.equal(renders, 0);
});

test("a late API failure cannot overwrite a newer success and Audit retains its endpoint", () => {
  const statusText = {textContent: "Embedding completed"};
  const status = {className: "status"};
  const events = [];
  const state = {statusRevision: 9, statusType: "ready", activeActionId: ""};
  const context = load(statusFunctions, {
    state,
    $: id => id === "statusText" ? statusText : status,
    auditEvent: (...values) => events.push(values),
  });
  const error = new Error("Image preview expired");
  error.endpoint = "/api/preview";
  error.traceId = "trace-late";
  error.statusRevision = 8;

  assert.equal(context.setErrorStatus(error), false);
  assert.equal(statusText.textContent, "Embedding completed");
  assert.equal(events.length, 1);
  assert.equal(events[0][0], "late-api-error-suppressed");
  assert.equal(events[0][1], "/api/preview");
  assert.equal(events[0][2].trace_id, "trace-late");
});

test("OVERRIDE banner defaults migrate into saved palettes and follow opposing ownership", () => {
  const enhancement = load(graphicalEnhancementFunctions, {});
  const normalized = enhancement.normalizeGraphicalEnhancement({banner_styles: {
    artist: {background: "#123ABC"},
  }});
  assert.equal(normalized.banner_styles.artist.background, "#123ABC");
  assert.deepEqual(JSON.parse(JSON.stringify(normalized.banner_styles.override)), {
    name: "OVERRIDE", background: "#D3D3D3", frame: "#FFFFFF", letters: "#FF344F",
  });
  assert.deepEqual(Object.keys(normalized.banner_styles).slice(-2), ["artist", "override"]);

  const state = {albumOverrides: new Map()};
  const context = load(albumBannerFunctions, {state});
  const album = {id: "album-1", album_mbid: "release-1"};
  assert.equal(context.albumDisplayBannerKind(album, {}), "artist");
  state.albumOverrides.set("album-1", {writer: "windows"});
  assert.equal(context.albumDisplayBannerKind(album, {}), "override");
  state.albumOverrides.set("album-1", {writer: "webgui"});
  assert.equal(context.albumDisplayBannerKind(album, {}), "artist");
});

test("coordination revision changes identify only affected Albums", () => {
  const state = {albumOverrides: new Map(), albumOverrideRevision: 0};
  const context = load(coordinationFunctions, {state});
  const first = [{album_key: "album-1", writer: "windows", event_id: "event-1", revision: 38}];
  assert.deepEqual(JSON.parse(JSON.stringify(context.applyAlbumCoordination(first))), ["album-1"]);
  assert.equal(state.albumOverrideRevision, 38);
  assert.deepEqual(JSON.parse(JSON.stringify(context.applyAlbumCoordination(first))), []);
  assert.deepEqual(JSON.parse(JSON.stringify(context.applyAlbumCoordination([
    {...first[0], event_id: "event-2", revision: 39},
  ]))), ["album-1"]);
});

test("COMP track override waits for verified readable embedded bytes", () => {
  const path = "Various Artists/COMP/01.flac";
  const state = {compTrackOverrides: new Map()};
  const context = load(coordinationFunctions, {state});
  const track = {coordination_path: path};
  state.compTrackOverrides.set(path.toLowerCase(), {writer: "windows", readable: false});
  assert.equal(context.compTrackOverride(track), null);
  const verified = {writer: "windows", readable: true};
  state.compTrackOverrides.set(path.toLowerCase(), verified);
  assert.equal(context.compTrackOverride(track), verified);
});

test("opposing Windows writes refresh only visible affected WebGUI Album thumbnails", async () => {
  const calls = [];
  const old = {id: "album-1", cover: "/old-cover"};
  const hidden = {id: "album-hidden", cover: "/hidden-cover"};
  const state = {
    album: {...old},
    artist: {albums: [{...old}]},
    albumsIndex: [{...old}, {...hidden}],
    albumOverrides: new Map([
      ["album-1", {writer: "windows"}],
      ["album-hidden", {writer: "webgui"}],
    ]),
  };
  const context = load(opposingThumbnailFunctions, {
    state,
    api: async path => {
      calls.push(path);
      return {id: "album-1", cover: "/current-cover", override_writer: "windows"};
    },
  });

  await context.refreshOpposingAlbumThumbnails(["album-1", "album-hidden"]);

  assert.deepEqual(calls, ["/api/album?id=album-1"]);
  assert.equal(state.album.cover, "/current-cover");
  assert.equal(state.artist.albums[0].cover, "/current-cover");
  assert.equal(state.albumsIndex[0].cover, "/current-cover");
  assert.equal(state.albumsIndex[1].cover, "/hidden-cover");
});

test("new COMP coordination reloads an open Artist report projection", async () => {
  const path = "Various Artists/COMP/01.flac";
  const event = {writer: "windows", readable: true, relative_track_path: path};
  const calls = [];
  const state = {
    editorMode: "artist", compReportVisible: true,
    artist: {id: "artist-1"},
    compGroups: [{id: "album-1", name: "COMP", year: "2000", tracks: [
      {key: "track-1", coordination_path: path, completed: false, status: "blue"},
    ]}],
    compTrack: {albumId: "album-1", key: "track-1", completed: false, status: "blue"},
    compTrackOverrides: new Map([[path.toLowerCase(), event]]),
    fallbackTracks: [],
  };
  let renders = 0, probes = 0;
  const context = load(opposingThumbnailFunctions, {
    state, encodeURIComponent,
    normalizedCoordinationPath: value => String(value || "").toLowerCase(),
    compareCompReportGroups: () => 0,
    applyCompTrackCoordination: rows => {
      for (const group of state.compGroups) for (const track of group.tracks) {
        const override = rows.find(row => row.relative_track_path.toLowerCase()
          === track.coordination_path.toLowerCase());
        if (override?.writer === "windows" && override.readable) {
          track.completed = true;
          track.status = "green";
        }
      }
      return [];
    },
    api: async request => {
      calls.push(request);
      return [{id: "album-1", name: "COMP", year: "2000", tracks: [
        {key: "track-1", coordination_path: path, completed: true, status: "checking"},
      ]}];
    },
    renderHero: () => { renders += 1; },
    renderCandidates: () => { renders += 1; },
    renderReview: () => { renders += 1; },
    renderLibrary: () => { renders += 1; },
    probeArtistCompStatuses: async () => { probes += 1; },
  });

  await context.refreshOpenCompReportAfterCoordination();

  assert.deepEqual(calls, ["/api/artist/comp-report?id=artist-1"]);
  assert.equal(state.compGroups[0].tracks[0].completed, true);
  assert.equal(state.compGroups[0].tracks[0].status, "green");
  assert.equal(state.compTrack.completed, true);
  assert.equal(state.compTrack.status, "green");
  assert.equal(renders, 3);
  assert.equal(probes, 1);
});

test("COMP report refresh discards an older browser color snapshot", async () => {
  const state = {
    editorMode: "artist", compReportVisible: true,
    artist: {id: "artist-1"},
    compGroups: [{id: "album-1", tracks: [
      {key: "track-1", coordination_path: "COMP/01.flac", completed: true, status: "green"},
    ]}],
    compTrack: {albumId: "album-1", key: "track-1", completed: true, status: "green"},
    compTrackOverrides: new Map(), fallbackTracks: [],
  };
  const context = load(opposingThumbnailFunctions, {
    state, encodeURIComponent,
    compareCompReportGroups: () => 0,
    applyCompTrackCoordination: () => [],
    api: async () => [{id: "album-1", tracks: [
      {key: "track-1", coordination_path: "COMP/01.flac", completed: false, status: "blue"},
    ]}],
    renderHero() {}, renderCandidates() {}, renderReview() {}, renderLibrary() {},
    probeArtistCompStatuses: async () => {},
  });

  await context.refreshOpenCompReportAfterCoordination();

  assert.equal(state.compGroups[0].tracks[0].completed, false);
  assert.equal(state.compGroups[0].tracks[0].status, "blue");
  assert.equal(state.compTrack.completed, false);
  assert.equal(state.compTrack.status, "blue");
});

test("Artist Albums row reports shown, Completed, and in Process with status colors", () => {
  const children = [];
  const styles = {};
  const host = {
    className: "", children,
    style: {setProperty: (key, value) => { styles[key] = value; }},
    append: (...items) => children.push(...items),
    replaceChildren: () => { children.length = 0; },
    setAttribute(key, value) { this[key] = value; },
  };
  const node = (tag, className, text) => ({tag, className, text});
  const state = {graphicalEnhancement: {frame_styles: {
    complete: {color: "theme-complete"}, in_process: {color: "theme-process"},
    thumbnail: {color: "theme-thumbnail"},
  }}};
  const context = load(albumSummaryFunctions, {
    state, $: () => host, clear: item => item.replaceChildren(), node,
    mediaStatusKey: album => album.status,
    mediaStatusColor: status => status,
  });
  const artist = {portrait: "artist.jpg"};
  context.renderAlbumCountSummary([
    {album_mbid: "release-1", status: "processed", cover_found: true, cover: "cover.jpg", tracks: 2, embedded: 2, embedded_validation: "verified"},
    {album_mbid: "release-2", status: "incomplete", cover_found: true, cover: "cover.jpg", tracks: 2, embedded: 0},
  ], artist);

  assert.deepEqual(children.map(item => item.text), [
    "2 shown", "/", "1 Completed", "/", "1 in Process", "/", "Overridden", "0",
  ]);
  assert.equal(styles["--album-complete-color"], "theme-complete");
  assert.equal(styles["--album-process-color"], "theme-process");
  assert.equal(host["aria-label"], "2 Albums shown, 1 Completed, 1 in Process, 0 overridden by Windows");
});

test("Album cards keep unknown validation separate from confirmed missing thumbnails", () => {
  const state = {graphicalEnhancement: {frame_styles: {
    complete: {color: "theme-complete"}, in_process: {color: "theme-process"},
    thumbnail: {color: "theme-thumbnail"},
  }}};
  const context = load(albumSummaryFunctions, {
    state, mediaStatusKey: album => album.status,
    mediaStatusColor: status => status,
  });
  const artist = {portrait: "artist.jpg"};
  const albumTypes = [
    {name: "Artist", album_artist: "Artist"},
    {name: "Various Artists", various_artists: true},
    {name: "Soundtrack", soundtrack: true},
    {name: "Featured", featured: true},
  ];

  for (const type of albumTypes) {
    const base = {
      ...type, album_mbid: `release-${type.name}`, status: "processed",
      cover_found: true, cover: "cover.jpg", tracks: 2, embedded: 0,
    };
    const unknown = context.albumCardState({...base, embedded_validation: "unknown"}, artist);
    assert.equal(unknown.embeddedPending, true, `${type.name} remains pending`);
    assert.equal(unknown.checkEmbedded, false, `${type.name} is not falsely purple`);
    assert.equal(unknown.dots.some(([color]) => color === "#7c4dff"), false);

    const retry = context.albumCardState({...base, embedded_validation: "retry"}, artist);
    assert.equal(retry.embeddedRetry, true, `${type.name} exposes retry`);
    assert.equal(retry.checkEmbedded, false, `${type.name} retry is not repair`);

    const missing = context.albumCardState({...base, embedded_validation: "verified"}, artist);
    assert.equal(missing.checkEmbedded, true, `${type.name} confirmed missing is purple`);

    const partialOrWrongSize = context.albumCardState({...base, embedded: 1, embedded_validation: "verified"}, artist);
    assert.equal(partialOrWrongSize.checkEmbedded, true, `${type.name} confirmed partial or wrong-size is purple`);

    const complete = context.albumCardState({...base, embedded: 2, embedded_validation: "verified"}, artist);
    assert.equal(complete.fullyComplete, true, `${type.name} verified complete is green`);
    assert.equal(complete.dots.some(([color]) => color === "#32dda0"), true);
  }

  const fallback = context.albumCardState({
    album_mbid: "", status: "processed", fallback_complete: true,
    tracks: 2, embedded: 0, embedded_validation: "unknown",
  }, artist);
  assert.equal(fallback.fullyComplete, true);
  assert.equal(fallback.checkEmbedded, false);
});

test("Artist overview lazily verifies unresolved regular Albums without touching COMP fallback", async () => {
  let renders = 0;
  const state = {
    selectionRequest: 4,
    artist: {id: "artist-1", albums: [
      {id: "artist-album", album_mbid: "release-1", embedded: 0, embedded_validation: "unknown"},
      {id: "various-album", album_mbid: "release-2", embedded: 0, embedded_validation: "unknown", various_artists: true},
      {id: "soundtrack-album", album_mbid: "release-3", embedded: 0, embedded_validation: "retry", soundtrack: true},
      {id: "featured-album", album_mbid: "release-4", embedded: 0, embedded_validation: "unknown", featured: true},
      {id: "comp", album_mbid: "", embedded: 0, embedded_validation: "unknown"},
    ]},
    album: null,
  };
  const calls = [];
  const context = load(artistAlbumStatusFunction, {
    state,
    api: async path => {
      calls.push(path);
      const id = decodeURIComponent(path.split("id=")[1]);
      return {embedded_tracks: {
        "artist-album": 12, "various-album": 10,
        "soundtrack-album": 8, "featured-album": 6,
      }[id], status: "verified"};
    },
    renderHero: () => { renders += 1; },
  });

  await context.probeArtistAlbumStatuses("artist-1", 4);

  assert.deepEqual(calls, [
    "/api/album/embedded-status?id=artist-album",
    "/api/album/embedded-status?id=various-album",
    "/api/album/embedded-status?id=soundtrack-album",
    "/api/album/embedded-status?id=featured-album",
  ]);
  assert.deepEqual(state.artist.albums.map(album => [album.id, album.embedded, album.embedded_validation]), [
    ["artist-album", 12, "verified"],
    ["various-album", 10, "verified"],
    ["soundtrack-album", 8, "verified"],
    ["featured-album", 6, "verified"],
    ["comp", 0, "unknown"],
  ]);
  assert.equal(renders, 2);
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
  assert.match(lowerLaunchFunction, /state\.queuedDecision[^\n]+skipQueuedAlbum/);
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
    setStatus() {}, cancelActiveCompWork: async () => {}, auditEvent() {},
  });

  await context.chooseAlbum("own-1");

  assert.equal(context.albumInCurrentArtistContext("comp-1"), true);
  assert.equal(state.album.id, "own-1");
  assert.deepEqual(state.compAlbums.map(group => group.id), ["comp-1"]);
  assert.equal(state.compGroups.length, 0);
});

test("fresh Albums view selection establishes Artist context and remains responsive", async () => {
  const state = {
    artist: null, artistMetadata: null, artistMetadataRequest: 0, artists: [],
    album: null, compAlbums: [], compGroups: [], compCollapsed: new Set(), compare: new Set(),
    candidates: [], selectedPortrait: null, selectedBanner: null, selectedCover: null,
    upscalePreview: null, renderedPreview: {}, fallbackTracks: [], fallbackTrack: null,
    fallbackPreviewOnly: false, fallbackRun: false, fallbackTrackRequest: 0,
    fallbackPreviewMode: false, fallbackReportExpanded: true, fallbackHelpExpanded: false,
    compReportVisible: false, compReportLoading: false, compPreviewMode: false, compTrack: null,
    compRunIndex: -1, compTrackRequest: 0, compStatusRequest: 0, compReportRequest: 0,
    queuedDecision: null, reviewJobId: null, candidateFilterLocked: true,
    selectionRequest: 0, searchRequest: 0, reviewCleared: false, libraryMode: "albums",
  };
  const paths = [];
  const context = load(clearFocusFunction + contextFunctions + chooseAlbumFunction, {
    state,
    api: async path => {
      paths.push(path);
      if (path.startsWith("/api/album")) return {id: "album-1", artist_id: "artist-1", artist_name: "Artist", album_mbid: "release-1", candidates: [], tracks: 1};
      return {artist: {id: "artist-1", albums: [{id: "album-1"}], local_candidates: []}, comp_albums: []};
    },
    resetFocusAdjustments() {}, renderSources() {}, renderCandidates() {}, renderWorkspace() {},
    resetCandidateFilters() {}, markSelectedAlbum() {}, renderAll() {}, loadArtistMetadata() {},
    setStatus() {}, cancelActiveCompWork: async () => {}, auditEvent() {},
  });
  await context.chooseAlbum("album-1");
  assert.deepEqual(paths.map(path => path.split("?")[0]), ["/api/album", "/api/artist/overview"]);
  assert.equal(state.artist.id, "artist-1");
  assert.equal(state.album.id, "album-1");
  assert.equal(state.editorMode, "album");
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

test("Album search and Album overview use the global single-Album contract", () => {
  assert.match(appSource, /async function loadAlbumLibraryMatches/);
  assert.match(appSource, /\/api\/albums\?view=albums&q=/);
  assert.match(appSource, /const matchesByArtist = new Map\(\)/);
  assert.match(appSource, /matchesByArtist\.has\(artist\.id\)/);
  assert.match(appSource, /matchingIds\.has\(item\.id\)/);
  assert.match(appSource, /const presentedAlbums = singleAlbumOverviewActive\(\)/);
  assert.match(appSource, /allAlbums\.find\(album => album\.id === state\.album\.id\)/);
});

test("WebGUI renders native-parity overview sections and artwork footer", () => {
  assert.match(indexSource, /id="topGenres"/);
  assert.match(indexSource, /id="topSongs"/);
  assert.match(appSource, /Artist & Featured Artist Top Genres/);
  assert.match(appSource, /node\("strong", "top-songs-heading", "Top Songs"\)/);
  assert.match(appSource, /Embedded Art \(now\):/);
  assert.match(appSource, /Track\(s\) \(" \+ trackCount \+ "\)"/);
  assert.match(appSource, /Embedded Track\(s\)/);
  assert.match(cssSource, /review-extra\.embedded-metadata-row/);
  assert.match(indexSource, /id="overviewContent"[\s\S]*id="topSongs"[\s\S]*class="overview-actions"/);
  assert.match(cssSource, /overview-content>\.overview-actions/);
  assert.match(cssSource, /overview-actions #compReportButton/);
  assert.match(cssSource, /review-actions \.action-orange/);
  assert.match(appSource, /function renderCompReportButtonState\(\)/);
  assert.match(appSource, /state\.libraryMode === "artists" && state\.editorMode === "artist"/);
  assert.match(appSource, /selectFallbackTrack\(item\.key, state\.fallbackPreviewMode, !state\.fallbackPreviewMode\)/);
  assert.match(appSource, /selectArtistCompTrack\(group\.id, track\.key, !state\.compPreviewMode\)/);
  assert.match(appSource, /node\("div", "musicbrainz-release-actions"\)/);
  assert.match(appSource, /"STOP PROCESS"/);
});

test("Fallback file-list tracks use the same COMP status palette as report tracks", () => {
  const context = load(compTrackStatusFunction, {});
  assert.equal(context.compTrackStatusClass({status: "green"}), "complete");
  assert.equal(context.compTrackStatusClass({status: "blue"}), "pending");
  assert.equal(context.compTrackStatusClass({status: "purple"}), "purple");
  assert.equal(context.compTrackStatusClass({status: "checking"}), "checking");
  assert.equal(context.compTrackStatusClass({status: "check-error"}), "error");
  assert.match(appSource, /row\.classList\.add\(compTrackStatusClass\(track\)\)/);
  assert.match(appSource, /renderLibrary\(\); renderHero\(\);/);
  assert.match(cssSource, /\.fallback-library-track\.pending\{color:var\(--blue\)!important\}/);
  assert.doesNotMatch(cssSource, /\.fallback-library-track\{[^}]*color:var\(--green\)!important/);
});

test("Artist filtering survives overview selection and rerender", () => {
  assert.match(appSource, /artistMetadataRequest: 0, artistQuery: ""/);
  assert.match(appSource, /state\.artistQuery = event\.target\.value/);
  assert.match(appSource, /String\(state\.artistQuery \|\| \$\("artistSearch"\)\.value \|\| ""\)/);
  assert.match(appSource, /String\(artist\.name \|\| ""\)\.toLocaleLowerCase\(\)\.includes\(artistQuery\)/);
  const chooseArtistSource = between(appSource, "async function chooseArtist", "async function chooseAlbum");
  assert.doesNotMatch(chooseArtistSource, /artistSearch[^\n]*value\s*=\s*""/);
});

test("About and Help use the native information contract and repository guide", () => {
  assert.match(appSource, /Cross-platform media artwork and library tooling/);
  assert.match(appSource, /Python · FastAPI · PostgreSQL/);
  assert.match(appSource, /Check for Updates/);
  assert.match(appSource, /Microsoft Store ↗/);
  assert.match(appSource, /docs\/browser-workspace-guide\.md/);
  const helpSource = between(appSource, "\$(\"helpButton\")", "\$(\"closeDialog\")");
  assert.match(helpSource, /window\.open/);
  assert.doesNotMatch(helpSource, /showDialog/);
});

test("Artist enhancement rerenders restart from the explicit source", () => {
  assert.match(operationsSource, /cumulative flash was the visible Artist Art jitter\/crop regression/);
  assert.match(operationsSource, /restore\('#artistPane img',portrait\)/);
  assert.match(operationsSource, /restore\('#bannerPane img',banner\)/);
  assert.match(operationsSource, /generation!==previewGeneration/);
  assert.match(cssSource, /package-banner,.selected-package img\.banner-art\{object-fit:contain/);
});
