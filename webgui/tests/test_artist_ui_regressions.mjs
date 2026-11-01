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
const renderMusicBrainzFunction = between(
  appSource,
  "function renderMusicBrainzResults",
  "function renderCandidates",
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
  "async function stageAlbum",
  "async function queueAction",
);
const stageAllAlbumsFunction = between(
  operationsSource,
  "async function stageAllArtistAlbums",
  '$("embedAllAlbums")',
);
const albumEligibilityFunctions = between(
  appSource,
  "function artistAlbumEligible",
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
const artistCompReportFunction = between(
  appSource,
  "async function openArtistCompReport",
  "async function probeArtistCompStatuses",
);
const renderArtistCompReportFunction = between(
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
      {key: "artist-track", completed: false, status: "blue"},
      {key: "other-track", completed: false, status: "blue"},
      {key: "completed-track", completed: true, status: "green"},
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
  assert.match(appSource, /launch\.addEventListener\("click", \(\) => runAction\(launchArtistComp\)\)/);
  assert.match(appSource, /searchButton"\)\.addEventListener\("click", \(\) => runAction\(launchCandidateWork\)\)/);
});

test("Artist COMP renders all album groups and previews only the selected current track", () => {
  const report = {
    children: [], hidden: true,
    replaceChildren() { this.children = []; },
    append(...children) { this.children.push(...children); },
  };
  const makeNode = (tag, className = "", text = "") => ({
    tag, className, text, children: [], listeners: {}, dataset: {},
    classList: {values: [], add(...values) { this.values.push(...values); }},
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
    compCollapsed: new Set(), compBusy: false, compRepairEnabled: true,
    artist: {id: "artist-1", name: "Selected Artist"}, server: {mode: "write"},
  };
  const context = load(renderArtistCompReportFunction, {
    state, $: () => report, clear: element => element.replaceChildren(), node: makeNode,
    image: (src, className, alt) => ({tag: "img", src, className, alt}),
    setStatus() {}, runAction() {}, repairArtistComp() {}, launchArtistComp() {},
    closeArtistCompReport() {}, selectArtistCompTrack() {},
  });

  context.renderArtistCompReport();

  const albumRows = report.children.filter(item => item.className === "fallback-report-album");
  const trackRows = report.children.filter(item => item.className.startsWith("fallback-track"));
  const previews = trackRows.flatMap(row => row.children.filter(item => item.tag === "img"));
  assert.deepEqual(albumRows.map(row => row.dataset.compAlbumId), ["comp-1", "comp-2", "comp-3"]);
  assert.equal(trackRows.length, 3);
  assert.ok(trackRows.every(row => typeof row.listeners.click === "function"));
  assert.deepEqual(previews.map(item => item.src), ["/selected-current.jpg"]);

  state.compPreviewMode = false;
  context.renderArtistCompReport();
  const previewsWhenOff = report.children
    .filter(item => item.className.startsWith("fallback-track"))
    .flatMap(row => row.children.filter(item => item.tag === "img"));
  assert.equal(previewsWhenOff.length, 0);
});

test("EMBED Track writes only the selected Artist COMP track with its prepared artwork", async () => {
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

test("Full Fallback EMBED Track uses the same direct prepared-track write", async () => {
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
  assert.match(appSource, /const compTrackImage = state\.editorMode === "fallback" \? state\.fallbackTrack\?\.image : state\.compTrack\?\.image/);
  assert.match(appSource, /url = state\.renderedPreview\.comp\?\.image \|\| state\.upscalePreview\?\.image \|\| state\.fallbackTrack\?\.image/);
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

test("queued COMP candidates submit adjustment and upscale intent with the exact candidate number", async () => {
  const submitted = [];
  const state = {
    queuedDecision: {jobKind: "comp", selected: {queueValue: 4, image: "/candidate-4"}},
    fx: {comp: {Contrast: 8}}, upscalePreview: {image: "/preview"},
  };
  const context = load(writeArtworkFunction, {
    state,
    submitQueuedDecision: async value => { submitted.push(JSON.parse(value)); },
  });

  await context.writeArtwork();

  assert.deepEqual(JSON.parse(JSON.stringify(submitted)), [{
    action: "webgui-embed-track", number: 4,
    artwork: {image: "/candidate-4", adjustments: {Contrast: 8}, upscale: true},
  }]);
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
    progress: {current: {fallback_authority: {artist: "10,000 Maniacs", track: "Because the Night"}}},
  };
  const prompt = {nonce: "nonce-1", context: {
    kind: "musicbrainz-results", album_id: "billboard-1994", track_key: "10k-because-the-night",
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
  assert.equal(focused.length, 1);
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

  const submitted = [];
  const elements = {candidateCount: {textContent: ""}};
  const node = (tag, className = "", text = "") => ({
    tag, className, text, children: [], listeners: {},
    append(...children) { this.children.push(...children); },
    setAttribute() {},
    addEventListener(event, handler) { this.listeners[event] = handler; },
  });
  const renderer = load(renderMusicBrainzFunction, {
    $: id => elements[id], node,
    runAction: action => action(),
    submitQueuedDecision: async value => { submitted.push(value); },
  });
  const grid = {children: [], append(...children) { this.children.push(...children); }};
  renderer.renderMusicBrainzResults(grid, {options, context: {track_title: "Because the Night"}}, "billboard");
  assert.equal(grid.children.length, 1);
  await grid.children[0].children[0].listeners.click();
  assert.deepEqual(submitted, [2]);
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
    {id: "regular", name: "Regular", album_mbid: "release-1", tracks: 10, cover: "/regular.jpg"},
    {id: "featured", name: "Featured", album_mbid: "release-2", tracks: 1, cover: "/featured.jpg"},
    {id: "fallback", name: "Fallback", album_mbid: "", tracks: 20, cover: "/fallback.jpg"},
    {id: "no-cover", name: "No Cover", album_mbid: "release-3", tracks: 8, cover: null},
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

  assert.deepEqual(calls, ["regular", "featured"]);
  assert.ok(statuses.at(-1)[0].includes("1 Albums staged"));
  assert.ok(statuses.at(-1)[0].includes("1 already queued"));
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
