"use strict";

const $ = id => document.getElementById(id);
const ENHANCEMENT_VALUE_FIELDS = [
  ["Picture", "output.upscale_picture_percent", 2, -20, 20, "%"],
  ["Sharpen", "output.upscale_sharpen_percent", 4, 0, 20, "%"],
  ["Softness", "output.upscale_softness_percent", 0, 0, 20, "%"],
  ["Contrast", "output.upscale_contrast_percent", 10, -20, 20, "%"],
  ["Exposure", "output.upscale_exposure_percent", -5, -20, 20, "%"],
  ["Brightness", "output.upscale_brightness_percent", -1, -20, 20, "%"],
  ["Gamma", "output.upscale_gamma_percent", -6, -20, 20, "%"],
  ["Color", "output.upscale_color_temperature", 0, -100, 100, ""],
  ["Hue", "output.upscale_hue_degrees", 0, -180, 180, "°"],
  ["Darkness", "output.upscale_darkness_percent", 0, 0, 100, "%"],
  ["Tint", "output.upscale_tint_percent", 0, -100, 100, "%"],
];
const GRAPHICAL_ENHANCEMENT_DEFAULTS = {
  banner_sync_frame_letters: false,
  banner_random_harmony: false,
  wheel_hue_shift_degrees: 0,
  wheel_brightness_percent: 100,
  banner_styles: {
    compiled: {name: "COMPILED", background: "#3478FF", frame: "#D9AD43", letters: "#D9AD43"},
    featuring: {name: "FEATURING", background: "#B8875D", frame: "#008B8B", letters: "#008B8B"},
    soundtrack: {name: "SOUNDTRACK", background: "#4B0082", frame: "#B49B00", letters: "#B49B00"},
    various: {name: "VARIOUS", background: "#654321", frame: "#32DDA0", letters: "#32DDA0"},
    artist: {name: "ARTIST", background: "#006B3C", frame: "#D9AD43", letters: "#D9AD43"},
  },
  frame_styles: {
    complete: {name: "Complete", color: "#32DDA0"},
    in_process: {name: "In Process", color: "#EFA54E"},
    thumbnail: {name: "Thumbnail Check", color: "#7C4DFF"},
  },
  adjustment_presets: [
    {name: "Default", values: [2, 4, 0, 10, -5, -1, -6, 0, 0, 0, 0], tint_color: "#FFB36B", saved: true},
    {name: "Custom A", values: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], tint_color: "#FFB36B", saved: false},
    {name: "Custom B", values: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], tint_color: "#FFB36B", saved: false},
    {name: "Custom C", values: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], tint_color: "#FFB36B", saved: false},
    {name: "Custom D", values: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], tint_color: "#FFB36B", saved: false},
  ],
};
function cloneGraphicalEnhancement(value) {
  return JSON.parse(JSON.stringify(value));
}
function normalizeGraphicalEnhancement(value = {}) {
  const result = cloneGraphicalEnhancement(GRAPHICAL_ENHANCEMENT_DEFAULTS);
  for (const key of ["banner_sync_frame_letters", "banner_random_harmony", "wheel_hue_shift_degrees", "wheel_brightness_percent"])
    if (value[key] !== undefined) result[key] = value[key];
  for (const [key, baseline] of Object.entries(result.banner_styles)) Object.assign(baseline, value.banner_styles?.[key] || {});
  for (const [key, baseline] of Object.entries(result.frame_styles)) Object.assign(baseline, value.frame_styles?.[key] || {});
  if (Array.isArray(value.adjustment_presets)) value.adjustment_presets.slice(0, 5).forEach((preset, index) => Object.assign(result.adjustment_presets[index], preset));
  result.wheel_hue_shift_degrees = Math.max(0, Math.min(359, Number(result.wheel_hue_shift_degrees) || 0));
  result.wheel_brightness_percent = Math.max(30, Math.min(170, Number(result.wheel_brightness_percent) || 100));
  return result;
}
const state = {
  artists: [], albumsIndex: [], artist: null, artistMetadata: null, album: null,
  biographyArtistId: null, biographyExpanded: false, biographyText: "",
  libraryMode: "artists", editorMode: "artist", workspace: "find",
  candidates: [], selectedPortrait: null, selectedBanner: null, selectedCover: null, reviewCleared: false,
  upscalePreview: null, renderedPreview: {}, compThumbnailPreview: null, pendingCompWrite: null,
  fallbackTracks: [], fallbackTrack: null, fallbackPreviewOnly: false, fallbackRun: false, fallbackTrackRequest: 0, fallbackStatusRequest: 0,
  fallbackPreviewMode: false, fallbackReportExpanded: true, fallbackHelpExpanded: false,
  compAlbums: [], compGroups: [], compReportVisible: false, compReportLoading: false, compPreviewMode: false, compCollapsed: new Set(),
  compTrack: null, compTrackRequest: 0, compStatusRequest: 0, compReportRequest: 0, compRunIndex: -1,
  compRepairEnabled: false, compBusy: false,
  activeSource: "", radioCurrent: true, radioArtist: false, radioBanner: false, themedPackaging: false,
  mediaStatuses: new Set(["unprocessed", "incomplete", "processed", "bypassed", "partial", "artistComplete", "artistBypass"]),
  mediaSelected: new Set(), candidateFilters: new Map(), mediaFilterLocked: false, candidateFilterLocked: false,
  sourceDiagnostics: [],
  fx: {artist: {}, banner: {}, album: {}, comp: {}, current: {}, package: {}}, fit: {artist: {zoom: 1, x: 0, y: 0}, banner: {zoom: 1, x: 0, y: 0}},
  theme: "dark", compare: new Set(), graphicalEnhancement: normalizeGraphicalEnhancement(),
  embedQueue: [], queueControls: {pause_after_album: false, pause_all_albums: false}, queuedDecision: null, reviewJobState: "", listRequest: 0, selectionRequest: 0, searchRequest: 0,
  libraryLoading: false, artistMetadataRequest: 0,
};
function applyGraphicalEnhancement(value) {
  state.graphicalEnhancement = normalizeGraphicalEnhancement(value);
  const root = document.documentElement, frames = state.graphicalEnhancement.frame_styles;
  root.style.setProperty("--album-frame-complete", frames.complete.color);
  root.style.setProperty("--album-frame-progress", frames.in_process.color);
  root.style.setProperty("--album-frame-thumbnail", frames.thumbnail.color);
}
function albumBannerStyle(kind) {
  return state.graphicalEnhancement.banner_styles[kind] || state.graphicalEnhancement.banner_styles.artist;
}
const scrollPositions = new Map(), scrollSurfaceKeys = new Map(), restoringScroll = new Set();
const SCROLL_SURFACES = ["artistList", "overviewContent", "albumGrid", "findView"];
function rememberScroll(id) {
  const element = $(id), key = scrollSurfaceKeys.get(id);
  if (!key || restoringScroll.has(id) || !element.getClientRects().length) return;
  scrollPositions.set(key, {top: element.scrollTop, left: element.scrollLeft,
    expanded: id === "albumGrid" && element.classList.contains("expanded")});
}
function restoreScroll(ids) {
  const keys = ids.map(id => scrollSurfaceKeys.get(id));
  const restore = () => ids.forEach((id, index) => {
    if (scrollSurfaceKeys.get(id) !== keys[index]) return;
    const element = $(id), saved = scrollPositions.get(keys[index]);
    if (!saved || !element.getClientRects().length) return;
    if (id === "albumGrid") {
      element.classList.toggle("expanded", saved.expanded);
      $("showAllAlbums").textContent = saved.expanded ? "Show Less" : "Show All";
    }
    element.scrollTop = saved.top; element.scrollLeft = saved.left;
  });
  ids.forEach(id => restoringScroll.add(id));
  restore();
  requestAnimationFrame(() => {
    restore();
    requestAnimationFrame(() => ids.forEach((id, index) => {
      if (scrollSurfaceKeys.get(id) === keys[index]) restoringScroll.delete(id);
    }));
  });
}
function renderWithScrollMemory(ids, context, render) {
  for (const id of ids) {
    rememberScroll(id);
    const key = context ? id + ":" + context : "";
    scrollSurfaceKeys.set(id, key);
    if (key && !scrollPositions.has(key)) scrollPositions.set(key, {top: 0, left: 0, expanded: false});
  }
  try { render(); } finally { restoreScroll(ids); }
}
const SOURCES = [
  ["Samples", "", "⊘"], ["Local Files", "local", "local"],
  ["iTunes", "itunes", "itunes"], ["Fanart.tv", "fanart", "fanart"],
  ["Amazon", "amazon", "amazon"], ["Cover Art Archive", "coverartarchive", "coverart"],
  ["MusicBrainz", "musicbrainz", "musicbrainz"], ["Discogs", "discogs", "discogs"],
  ["Last.fm", "lastfm", "lastfm"], ["Navidrome", "navidrome", "navidrome"],
  ["Deezer", "deezer", "deezer"],
];

function setStatus(message, type = "ready") {
  $("statusText").textContent = message;
  $("status").className = "status" + (type === "ready" ? "" : " " + type);
}
async function api(path, options) {
  options = {...options, headers: {...options?.headers}};
  if (options.method && options.method !== 'GET' && state.csrf) options.headers['X-SPLINED-CSRF'] = state.csrf;
  const response = await fetch(path, options);
  const payload = await response.json();
  if (!response.ok) throw new Error(payload.error || "Request failed");
  return payload;
}
function node(tag, className = "", text) {
  const element = document.createElement(tag);
  if (className) element.className = className;
  if (text !== undefined) element.textContent = text;
  return element;
}
function image(src, className = "", alt = "") {
  const element = node("img", className);
  const dynamic = typeof src === "string" && src.startsWith("/api/image/");
  element.src = dynamic && state.server && !state.server.animate_webp_preview
    ? src + (src.includes("?") ? "&" : "?") + "still=1" : src;
  element.alt = alt; element.loading = "lazy"; element.draggable = false; return element;
}
function clear(element) { element.replaceChildren(); }
function clearFocusCandidates() {
  state.queuedDecision = null; state.reviewJobId = null;
  state.candidates = [];
  state.selectedPortrait = null; state.selectedBanner = null; state.selectedCover = null;
  state.upscalePreview = null; state.renderedPreview = {}; state.compThumbnailPreview = null; state.pendingCompWrite = null; state.fallbackTracks = []; state.fallbackTrack = null;
  state.fallbackPreviewOnly = false; state.fallbackRun = false; ++state.fallbackTrackRequest; ++state.fallbackStatusRequest;
  state.fallbackPreviewMode = false; state.fallbackReportExpanded = true; state.fallbackHelpExpanded = false;
  state.compGroups = []; state.compReportVisible = false; state.compReportLoading = false; state.compPreviewMode = false;
  state.compCollapsed.clear(); state.compTrack = null; state.compRunIndex = -1;
  ++state.compTrackRequest; ++state.compStatusRequest; ++state.compReportRequest;
  state.sourceDiagnostics = [];
  state.compare.clear();
  if (!state.candidateFilterLocked) { state.activeSource = ""; state.candidateFilters.clear(); $("candidateSearch").value = ""; }
}
function resetFocusAdjustments() {
  state.fx = {artist: {}, banner: {}, album: {}, comp: {}, current: {}, package: {}};
  state.fit = {artist: {zoom: 1, x: 0, y: 0}, banner: {zoom: 1, x: 0, y: 0}};
  state.renderedPreview = {};
  state.themedPackaging = false;
}
function size(bytes) {
  if (!bytes) return "—";
  return bytes >= 1048576 ? (bytes / 1048576).toFixed(1) + " MB" : Math.round(bytes / 1024) + " KB";
}
function sourceKey(value) {
  const source = String(value || "").toLowerCase();
  if (source.includes("fanart")) return "fanart";
  if (source.includes("deezer")) return "deezer";
  if (source.includes("album folder") || source.includes("local")) return "local";
  if (source.includes("mb / caa") || source.includes("musicbrainz")) return "musicbrainz";
  if (source.includes("coverart") || source.includes("cover art archive")) return "coverartarchive";
  return source.replace(/[^a-z]/g, "");
}
function distinctDiagnostics(items) {
  const seen = new Set(), result = [];
  for (const item of items || []) {
    const source = Array.isArray(item) ? item[0] : item?.source || "lookup";
    const message = Array.isArray(item) ? item[1] : item?.message || String(item || "");
    const text = source + " · " + message;
    if (message && !seen.has(text)) { seen.add(text); result.push(text); }
  }
  return result;
}
function sourceDiagnosticEntry(item) {
  let source = Array.isArray(item) ? item[0] : item?.source || "";
  let message = Array.isArray(item) ? item[1] : item?.message || String(item || "");
  if (!source && message) {
    const separated = message.match(/^([^:·]+)[:·]\s*(.+)$/);
    if (separated) { source = separated[1]; message = separated[2]; }
    else {
      const lowered = message.toLowerCase();
      const matched = SOURCES.find(([label, key]) => key && (lowered.startsWith(label.toLowerCase()) || lowered.startsWith(key)));
      if (matched) source = matched[1];
    }
  }
  return {key: sourceKey(source), message: String(message || "")};
}
function sourceDiagnosticStatus(items, key) {
  const priorities = {filtered: 1, warning: 2, unavailable: 3};
  let result = {status: "", message: ""};
  for (const item of items || []) {
    const entry = sourceDiagnosticEntry(item);
    if (entry.key !== key || !entry.message) continue;
    const unavailable = /\b(?:http\s*[45]\d\d|unavailable|request failed|connection|network|timed? out|timeout|unauthori[sz]ed|forbidden|rate.?limit|dns)\b/i.test(entry.message);
    const filtered = /policy-filtered|below (?:the )?(?:global )?minimum|not primary\/front|\bfiltered\b|\bexcluded\b/i.test(entry.message);
    const status = unavailable ? "unavailable" : filtered ? "filtered" : "warning";
    if ((priorities[status] || 0) >= (priorities[result.status] || 0)) result = {status, message: entry.message};
  }
  return result;
}
function candidateKind(candidate) {
  const kind = String(candidate.kind || "").toLowerCase();
  if (kind.includes("banner")) return "banner";
  if (kind.includes("cover")) return "cover";
  return state.editorMode === "album" ? "cover" : "artist";
}
const MEDIA_STATUSES = [["unprocessed", "Unprocessed"], ["incomplete", "Incomplete"], ["processed", "Processed"], ["bypassed", "Bypassed"], ["partial", "Partial / Error"], ["artistComplete", "Artist Complete"], ["artistBypass", "Artist Bypass"]];
const MEDIA_STATUS_COLORS = {all: "#eef8ff", unprocessed: "#eef8ff", incomplete: "#50b9ff", processed: "#efa54e", bypassed: "#ff5c66", partial: "#b55ce4", artistComplete: "#4cd989", artistBypass: "#4099ff"};
const MEDIA_STATUS_COLORS_LIGHT = {all: "#304b5b", unprocessed: "#304b5b", incomplete: "#12659a", processed: "#99600b", bypassed: "#b72e40", partial: "#8036a8", artistComplete: "#176a3e", artistBypass: "#185aab"};
function mediaStatusColor(key) { return MEDIA_STATUS_COLORS[key] || MEDIA_STATUS_COLORS.unprocessed; }
function applyStatusPalette(element, key) {
  element.style.setProperty("--status-color", mediaStatusColor(key));
  element.style.setProperty("--status-color-light", MEDIA_STATUS_COLORS_LIGHT[key] || MEDIA_STATUS_COLORS_LIGHT.unprocessed);
}
const CANDIDATE_TYPES = [["Recommended", "Recommended"], ["Upscalable", "Upscalable"], ["Rejected", "Rejected"], ["Local", "Local"], ["Samples", "Samples"], ["FrontCover", "Front Cover"], ["BackCover", "Back Cover"], ["ArtistImage", "Artist Image"], ["MediaBooklet", "Media / Booklet"], ["Other", "Other"]];
const CANDIDATE_RANGES = [["Ideal", "Ideal"], ["UpperRange", "Upper range"], ["Ladder", "Ladder"], ["AboveLadder", "Above ladder"], ["LowerRange", "Lower range"], ["BelowMinimum", "Below minimum"]];
function mediaStatusKey(item) {
  const raw = String(item.status || "unprocessed").toLowerCase();
  if (raw === "complete") return "artistComplete";
  if (raw.includes("bypass")) return raw.includes("artist") ? "artistBypass" : "bypassed";
  if (raw.includes("partial") || raw.includes("error") || raw.includes("timeout")) return "partial";
  return MEDIA_STATUSES.some(([key]) => key === raw) ? raw : "unprocessed";
}
function mediaMatches(item) { return state.mediaStatuses.has(mediaStatusKey(item)); }
function selectedMedia(item) { return state.mediaSelected.has(item.id); }
function validMusicBrainzId(value) {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(String(value || "").trim());
}
function artistAlbumEligible(album) {
  const status = String(album.status || "").toLowerCase();
  const complete = status === "processed" && !!album.cover_found
    && Number(album.tracks || 0) > 0 && Number(album.embedded || 0) >= Number(album.tracks || 0);
  return validMusicBrainzId(album.album_mbid) && !["bypassed", "timeout"].includes(status) && !complete;
}
function albumEmbeddingEligible(album) {
  return validMusicBrainzId(album?.album_mbid) && Number(album?.tracks || 0) > 0;
}
function markSelectedArtist(artist) {
  state.mediaSelected.clear();
  state.mediaSelected.add(artist.id);
  for (const album of artist.albums) {
    if (artistAlbumEligible(album)) state.mediaSelected.add(album.id);
  }
}
function markSelectedAlbum(album) {
  state.mediaSelected.clear();
  state.mediaSelected.add(album.id);
}
function candidateRange(candidate) {
  const shortSide = Math.min(Number(candidate.width) || 0, Number(candidate.height) || 0);
  if (shortSide < 1200) return "BelowMinimum";
  if (shortSide < 1800) return "LowerRange";
  if (shortSide === 1800) return "Ideal";
  if (shortSide <= 2400) return "UpperRange";
  if (shortSide <= 3600) return "Ladder";
  return "AboveLadder";
}
function candidateMatches(candidate, dimension, key) {
  const kind = candidateKind(candidate);
  const range = candidate.range_class || candidateRange(candidate);
  const acceptable = candidate.acceptable === undefined ? !["BelowMinimum", "AboveLadder"].includes(range) : candidate.acceptable;
  if (dimension === "range") return range.toLowerCase() === key.toLowerCase();
  if (dimension === "policy") return key === "Acceptable" ? acceptable : candidate.strict_override_active === true;
  if (dimension === "source") return sourceKey(candidate.source) === sourceKey(key);
  if (key === "Recommended") return candidate.recommended === true;
  if (key === "Upscalable") return candidate.upscaled === true && candidate.acceptable !== false;
  if (key === "Rejected") return !acceptable || candidate.policy_status === "reject";
  if (key === "Local") return sourceKey(candidate.source) === "local";
  if (key === "Samples") return sourceKey(candidate.source) === "samples";
  if (key === "FrontCover") return kind === "cover";
  if (key === "ArtistImage") return kind === "artist";
  if (key === "Other") return kind === "banner";
  return false;
}
function candidateFilterKey(dimension, key) { return dimension + ":" + key; }
function resetCandidateFilters() {
  if (state.candidateFilterLocked) return;
  state.candidateFilters.clear();
  for (const [dimension, rows] of [["type", CANDIDATE_TYPES], ["policy", [["Acceptable"], ["Strict"]]], ["range", CANDIDATE_RANGES]]) {
    for (const [key] of rows) if (state.candidates.some(candidate => candidateMatches(candidate, dimension, key))) state.candidateFilters.set(candidateFilterKey(dimension, key), "included");
  }
}
function candidateVisible(candidate) {
  if (state.activeSource && sourceKey(candidate.source) !== state.activeSource) return false;
  for (const [filter, disposition] of state.candidateFilters) {
    if (disposition !== "disregarded") continue;
    const separator = filter.indexOf(":");
    if (candidateMatches(candidate, filter.slice(0, separator), filter.slice(separator + 1))) return false;
  }
  return true;
}
function candidateBanner(candidate) {
  if (candidateKind(candidate) === "banner") return candidate;
  return state.candidates.find(item => item !== candidate && candidateKind(item) === "banner" && sourceKey(item.source) === sourceKey(candidate.source)) || null;
}
function activeFxTarget() {
  if (activeArtworkDecision()?.jobKind === "comp" || state.editorMode === "fallback" || state.editorMode === "artist" && state.compReportVisible) return "comp";
  if (state.editorMode === "album") return "album";
  if (state.radioCurrent) return "current";
  if (state.radioArtist && state.radioBanner) return state.themedPackaging ? "package" : null;
  return state.radioBanner ? "banner" : "artist";
}
function currentCompSelectionScope() {
  if (state.editorMode === "artist" && state.compReportVisible && state.compTrack) return {
    artist_id: String(state.artist?.id || ""), album_id: String(state.compTrack.albumId || ""), track_key: String(state.compTrack.key || ""),
  };
  if (state.editorMode === "fallback" && state.fallbackTrack) return {
    artist_id: "", album_id: String(state.album?.id || ""), track_key: String(state.fallbackTrack.key || ""),
  };
  return null;
}
function queuedCompDecisionScope(decision = state.queuedDecision) {
  if (decision?.jobKind !== "comp") return null;
  const keys = Array.isArray(decision.payload?.track_keys) ? decision.payload.track_keys : [];
  return {
    artist_id: String(decision.payload?.artist_id || ""),
    album_id: String(decision.context?.album_id || decision.payload?.album_id || ""),
    track_key: String(decision.context?.track_key || (keys.length === 1 ? keys[0] : "")),
  };
}
function queuedCompOwnsSelectedTrack(decision = state.queuedDecision) {
  const queued = queuedCompDecisionScope(decision);
  if (!queued) return false;
  const selected = currentCompSelectionScope();
  if (!selected) return true;
  return queued.artist_id === selected.artist_id && queued.album_id === selected.album_id
    && !!queued.track_key && queued.track_key === selected.track_key;
}
function activeArtworkDecision() {
  const decision = state.queuedDecision;
  if (!decision) return null;
  const selected = currentCompSelectionScope();
  if (decision.jobKind === "comp") return selected && queuedCompOwnsSelectedTrack(decision) ? decision : null;
  return selected ? null : decision;
}
function activeCompJobId() {
  if (state.queuedDecision?.jobKind === "comp") return state.queuedDecision.jobId || null;
  const reviewed = state.jobs?.find(job => job.id === state.reviewJobId && job.kind === "comp"
    && !["done", "failed", "cancelled"].includes(job.state));
  return reviewed?.id || null;
}
async function cancelActiveCompWork() {
  const jobId = activeCompJobId();
  if (!jobId) return;
  try { await post("/api/jobs/action", {action: "cancel", id: jobId}); }
  finally {
    if (state.queuedDecision?.jobId === jobId) state.queuedDecision = null;
    if (state.reviewJobId === jobId) { state.reviewJobId = null; state.reviewJobState = ""; }
  }
}
async function cancelMismatchedCompWork(scope) {
  const queued = queuedCompDecisionScope();
  if (!queued) return;
  if (queued.artist_id === String(scope.artist_id || "") && queued.album_id === String(scope.album_id || "")
    && queued.track_key === String(scope.track_key || "")) return;
  await cancelActiveCompWork();
}
function currentCandidate() {
  return state.editorMode === "album" ? state.selectedCover : (state.radioBanner && !state.radioArtist ? state.selectedBanner : state.selectedPortrait);
}

function renderLibrary() {
  renderWithScrollMemory(["artistList"], state.libraryLoading ? "" : state.libraryMode, renderLibraryContents);
}
function renderLibraryContents() {
  $("artistsTab").classList.toggle("active", state.libraryMode === "artists");
  $("albumsTab").classList.toggle("active", state.libraryMode === "albums");
  $("fallbackTab").classList.toggle("active", state.libraryMode === "fallback");
  $("libraryListTitle").textContent = state.libraryMode === "fallback" ? "Fallback Albums" : state.libraryMode === "albums" ? "Albums" : "Artists";
  const list = $("artistList"); clear(list);
  if (state.libraryMode === "artists") {
    $("artistCount").textContent = state.artists.filter(mediaMatches).length + " shown";
    for (const artist of state.artists.filter(mediaMatches)) {
      const button = node("button", "library-row"); button.type = "button";
      const orbit = node("span", "status-orbit" + (selectedMedia(artist) ? " selected" : ""), selectedMedia(artist) ? "●" : "○"); button.append(orbit);
      applyStatusPalette(button, mediaStatusKey(artist));
      const thumbUrl = state.artist?.id === artist.id && state.artist.portrait ? state.artist.portrait : artist.portrait;
      const thumb = thumbUrl ? image(thumbUrl, "library-thumb", artist.name + " artist image") : node("span", "library-thumb", artist.name.slice(0, 1).toUpperCase());
      if (thumbUrl) thumb.addEventListener("error", () => thumb.replaceWith(node("span", "library-thumb", artist.name.slice(0, 1).toUpperCase())), {once: true});
      button.append(thumb);
      const copy = node("span", "row-copy"); copy.append(node("strong", "", artist.name), node("small", "", "(" + artist.albums + " Albums)")); button.append(copy, node("span", "expand-mark", state.artist?.id === artist.id ? "−" : "+"));
      button.addEventListener("click", () => chooseArtist(artist.id)); list.append(button);
      if (state.artist?.id === artist.id) {
        const query = $("albumSearch").value.trim().toLowerCase();
        for (const album of state.artist.albums.filter(item => item.name.toLowerCase().includes(query))) {
          list.append(albumRow(album, true));
        }
      }
    }
  } else {
    $("artistCount").textContent = state.albumsIndex.filter(mediaMatches).length + " shown";
    for (const album of state.albumsIndex.filter(mediaMatches)) {
      list.append(albumRow(album, false));
      if (state.libraryMode === "fallback" && state.album?.id === album.id) {
        for (const track of state.fallbackTracks) {
          const row = node("button", "library-row fallback-library-track"); row.type = "button";
          row.classList.toggle("selected", state.fallbackTrack?.key === track.key);
          row.append(node("span", "fallback-library-status", "●"),
            node("span", "fallback-library-file", track.file),
            node("span", "fallback-library-select", state.fallbackTrack?.key === track.key ? "●" : "○"));
          row.addEventListener("click", () => selectFallbackTrack(track.key, false)); list.append(row);
        }
      }
    }
  }
  if (!list.childElementCount) list.append(node("p", "empty-copy",
    state.libraryMode === "fallback" && !state.albumsIndex.length
      ? "No missing-ID COMP albums in the library."
      : "No items match this filter."));
  renderMediaStatusChips();
}
function renderMediaStatusChips() {
  // The state panel describes the current selection, never the whole index.
  const focusedAlbum = state.album && selectedMedia(state.album);
  const items = focusedAlbum ? [state.album]
    : state.artist && selectedMedia(state.artist) ? [state.artist] : [];
  const counts = new Map();
  for (const item of items) { const key = mediaStatusKey(item); counts.set(key, (counts.get(key) || 0) + 1); }
  $("mediaStatusTitle").textContent = state.libraryMode === "fallback" ? "Fallback State" : focusedAlbum || state.libraryMode === "albums" ? "Album State" : "Artist State";
  const host = $("mediaStatusChips"); clear(host);
  for (const [key, label] of [["all", "All"], ...MEDIA_STATUSES]) {
    const enabled = key === "all" ? state.mediaStatuses.size === MEDIA_STATUSES.length : state.mediaStatuses.has(key);
    const button = node("button", "filter-chip media-status" + (enabled ? " active" : ""));
    button.type = "button"; button.disabled = state.mediaFilterLocked;
    applyStatusPalette(button, key);
    button.append(node("span", "filter-dot", enabled ? "●" : "○"), node("span", "filter-label", label), node("span", "filter-count", String(key === "all" ? items.length : counts.get(key) || 0)));
    button.addEventListener("click", () => { if (key === "all") state.mediaStatuses = new Set(MEDIA_STATUSES.map(([item]) => item)); else if (state.mediaStatuses.has(key)) state.mediaStatuses.delete(key); else state.mediaStatuses.add(key); renderLibrary(); }); host.append(button);
  }
}
function renderCandidateTypeChips() {
  for (const [hostId, dimension, rows] of [["candidateTypeChips", "type", CANDIDATE_TYPES], ["candidatePolicyChips", "policy", [["Acceptable", "Acceptable"], ["Strict", "Strict"]]], ["candidateRangeChips", "range", CANDIDATE_RANGES]]) {
    const host = $(hostId); clear(host);
    for (const [key, label] of rows) {
      const filterKey = candidateFilterKey(dimension, key);
      const disposition = state.candidateFilters.get(filterKey) || "neutral";
      const count = state.candidates.filter(candidate => candidateMatches(candidate, dimension, key)).length;
      const button = node("button", "filter-chip " + disposition); button.type = "button"; button.disabled = state.candidateFilterLocked;
      button.title = label + ": " + disposition + ". Click to cycle Included, Disregarded, Neutral.";
      button.append(node("span", "filter-dot", disposition === "included" ? "●" : "○"), node("span", "filter-label", label), node("span", "filter-count", String(count)));
      button.addEventListener("click", () => { const next = disposition === "neutral" ? "included" : disposition === "included" ? "disregarded" : "neutral"; if (next === "neutral") state.candidateFilters.delete(filterKey); else state.candidateFilters.set(filterKey, next); renderCandidates(); }); host.append(button);
    }
  }
  const sourceChosen = !!state.activeSource;
  $("candidateIncludeSource").disabled = !sourceChosen || state.candidateFilterLocked;
  $("candidateDisregardSource").disabled = !sourceChosen || state.candidateFilterLocked;
}
function albumRow(album, child) {
  const button = node("button", "library-row album-row" + (child ? " album-child" : "")); button.type = "button";
  const orbit = node("span", "status-orbit" + (selectedMedia(album) ? " selected" : ""), selectedMedia(album) ? "●" : "○"); button.append(orbit);
  applyStatusPalette(button, mediaStatusKey(album));
  button.append(album.cover ? image(album.cover, "library-thumb", "") : node("span", "library-thumb", "♫"));
  const copy = node("span", "row-copy"); copy.append(node("strong", "", album.name), node("small", "", child ? album.tracks + " tracks" : album.artist_name)); button.append(copy);
  button.addEventListener("click", () => chooseAlbum(album.id)); return button;
}

function albumInCurrentArtistContext(albumId) {
  return !!state.artist?.albums?.some(album => album.id === albumId)
    || state.compAlbums.some(group => group.id === albumId);
}

function albumBannerKind(album, artist) {
  if (!album.album_mbid) return "compiled";
  if (album.soundtrack) return "soundtrack";
  if (album.various_artists) return "various";
  if (album.featured) return "featuring";
  return "artist";
}
function albumPresentationGroup(album, artist) {
  const kind = albumBannerKind(album, artist);
  return {artist: 0, featuring: 1, soundtrack: 2, various: 3, compiled: 4}[kind] ?? 0;
}
function albumCardState(album, artist) {
  const status = mediaStatusKey(album);
  const fallback = !String(album.album_mbid || "").trim();
  const missingCover = !fallback && (!album.cover_found || !album.cover);
  const missingArtist = !artist?.portrait;
  const bypassed = status === "bypassed" || status === "artistBypass";
  const incomplete = fallback ? album.fallback_complete === false
    : status === "incomplete" || status === "partial" || status === "unprocessed";
  const processed = status === "processed" || status === "artistComplete";
  const embeddedComplete = Number(album.tracks) > 0 && Number(album.embedded) >= Number(album.tracks);
  const fullyComplete = fallback ? album.fallback_complete === true
    : processed && !missingCover && embeddedComplete;
  const checkEmbedded = !fallback && processed && !embeddedComplete;
  const dots = [];
  if (missingArtist) dots.push(["#f4cc4d", "Artist artwork missing"]);
  if (bypassed) dots.push(["#f4cc4d", "Bypassed"]);
  if (missingCover || incomplete) dots.push(["#ff5268", missingCover ? "Album artwork missing" : fallback ? "Selected Artist's COMP tracks incomplete" : "Album not complete"]);
  if (processed && !fullyComplete) dots.push(["#efa54e", "Processed; track artwork review pending"]);
  if ((fallback && fullyComplete) || (!fallback && processed && !missingCover && !missingArtist)) dots.push(["#32dda0", fallback ? "Selected Artist's COMP tracks complete" : "Artist and album artwork complete"]);
  if (checkEmbedded) dots.push(["#7c4dff", "Embedded thumbnail needs attention"]);
  if (album.webp_found) dots.push(["#3478ff", "cover.webp present"]);
  const frames = state.graphicalEnhancement.frame_styles;
  const frame = fullyComplete ? frames.complete.color : checkEmbedded || status === "partial" ? frames.thumbnail.color
    : processed ? frames.in_process.color : bypassed ? "#f4cc4d" : "#ff5268";
  return {dots, frame, fullyComplete, checkEmbedded};
}
function renderAlbumStatusKey(albums, artist) {
  const host = $("albumStatusKey"); clear(host);
  host.append(node("h3", "", "Album Status Key"));
  const columns = node("div", "album-key-columns");
  const frames = state.graphicalEnhancement.frame_styles;
  const sections = [
    ["Status dots", [["Red", "Missing art or unfinished album", "#ff5268"], ["Yellow", "Artist art missing or bypassed", "#f4cc4d"], ["Orange", "Processed; review still pending", "#efa54e"], ["Green", "Artist and album art complete", "#32dda0"], ["Purple", "Embedded thumbnail needs attention (Windows validation)", "#7c4dff"], ["Blue", "cover.webp present", "#3478ff"]]],
    ["Album card frames", [[frames.complete.name, "Complete", frames.complete.color], [frames.in_process.name, "Processed", frames.in_process.color], [frames.thumbnail.name, "Partial or error", frames.thumbnail.color], ["Missing", "Missing or incomplete", "#ff5268"]]],
    ["Banners", Object.entries(state.graphicalEnhancement.banner_styles).map(([key, style]) => [style.name, key === "compiled" ? "No release ID" : key === "artist" ? "Artist release" : key === "various" ? "Various Artists release" : key === "soundtrack" ? "Soundtrack release" : "Featured artist", style.letters, key])],
  ];
  for (const [title, rows] of sections) {
    const column = node("div", "album-key-column"); column.append(node("h4", "", title));
    for (const [label, detail, color, bannerKey] of rows) {
      const row = node("div", "album-key-row");
      const swatch = node("span", "album-key-swatch", "●"); swatch.style.color = color;
      const normalizedColor = String(color).toLowerCase();
      const count = title === "Banners" ? albums.filter(album => albumBannerKind(album, artist) === bannerKey).length
        : title === "Album card frames" ? albums.filter(album => albumCardState(album, artist).frame.toLowerCase() === normalizedColor).length
        : albums.filter(album => albumCardState(album, artist).dots.some(([dot]) => dot.toLowerCase() === normalizedColor)).length;
      row.append(swatch, node("strong", "", label), node("small", "", detail), node("b", "", String(count))); column.append(row);
    }
    columns.append(column);
  }
  host.append(columns, node("p", "album-key-note", "Counts reflect the current Artist Overview. Embedded counts use the PostgreSQL catalog and refresh after queued work completes."));
}

function updateBiographyToggle() {
  const biography = $("artistBiography"), text = $("biographyText"), toggle = $("biographyToggle");
  if (!biography.clientWidth) return;
  const lineHeight = parseFloat(getComputedStyle(biography).lineHeight);
  const maximumHeight = lineHeight * 5 + 1;
  text.textContent = state.biographyText;
  toggle.hidden = true;
  const overflows = biography.getBoundingClientRect().height > maximumHeight;
  toggle.hidden = !overflows;
  toggle.textContent = state.biographyExpanded ? "(less…)" : "(…more)";
  toggle.setAttribute("aria-expanded", String(state.biographyExpanded));
  if (!overflows) return;
  if (state.biographyExpanded) { text.textContent += " "; return; }

  // Fit the inline link with the text, including its width, within five lines.
  // Whole-word prefixes preserve readable endings and never cover biography text.
  const characters = Array.from(state.biographyText);
  const prefix = count => characters.slice(0, count).join("").replace(/\s+\S*$/, "").trimEnd();
  let low = 0, high = characters.length;
  while (low < high) {
    const middle = Math.ceil((low + high) / 2);
    text.textContent = prefix(middle) + " ";
    if (biography.getBoundingClientRect().height <= maximumHeight) low = middle;
    else high = middle - 1;
  }
  text.textContent = prefix(low) + " ";
}
function renderArtistBiography(artist, metadata) {
  const artistId = artist?.id || null;
  if (artistId !== state.biographyArtistId) {
    state.biographyArtistId = artistId;
    state.biographyExpanded = false;
  }
  state.biographyText = !artist ? "" : metadata?.biography || "No Artist biography available for " + artist.name + ".";
  $("biographyText").textContent = state.biographyText;
  requestAnimationFrame(updateBiographyToggle);
}
function renderHero() {
  const context = state.editorMode === "fallback" ? (state.album ? "fallback:" + state.album.id : "")
    : state.editorMode === "album" && state.album ? "album:" + state.album.id
    : state.artist ? "artist:" + state.artist.id : "";
  renderWithScrollMemory(["overviewContent", "albumGrid", "findView"], context, renderHeroContents);
}
function renderHeroContents() {
  if (state.editorMode === "fallback") { renderFallbackOverview(); return; }
  $("hero").classList.remove("fallback-overview");
  $("fallbackHelp").hidden = true;
  $("overviewHeading").textContent = "Artist Overview & Album Selection";
  $("albumStatusHelp").hidden = false;
  $("fallbackReport").hidden = true;
  document.querySelector(".overview-actions").hidden = false;
  document.querySelector(".meta-heading").textContent = "Albums";
  const artist = state.artist;
  const metadata = state.artistMetadata;
  $("artistName").textContent = artist?.name || "Select an artist";
  $("overviewArtist").textContent = artist?.name || "";
  $("artistInfoAlbums").textContent = artist ? String(artist.albums.length) : "—";
  $("artistInfoStatus").textContent = artist?.status || "Choose an Artist";
  $("artistInfoMbid").textContent = artist?.mbid || "Not indexed";
  $("artistInfoGenres").textContent = artist?.genres?.join(", ") || "Not available";
  $("artistInfoRoles").textContent = artist?.roles?.join(", ") || "Primary Artist";
  $("refreshArtistInfo").disabled = !artist;
  $("musicbrainzArtistButton").disabled = !artist?.mbid;
  renderArtistBiography(artist, metadata);
  $("similarSource").textContent = metadata?.similar?.length ? "· Navidrome"
    : metadata?.source && metadata.source !== "sample" ? "· " + metadata.source : "· sample layout";
  const similar = $("similarArtists"); clear(similar);
  const names = !artist ? [] : metadata?.similar?.length ? metadata.similar : state.artists.filter(item => item.id !== artist.id).slice(0, 6).map(item => item.name);
  for (const name of names) {
    const card = node("button", "similar-card"); card.type = "button";
    card.append(node("span", "similar-avatar", name.slice(0, 1).toUpperCase()), node("span", "similar-name", name), node("span", "similar-status", "●"));
    const local = state.artists.find(item => item.name.toLocaleLowerCase() === name.toLocaleLowerCase());
    card.disabled = !local; card.title = local ? "Open " + name : name + " is not in the indexed library";
    if (local) card.addEventListener("click", () => chooseArtist(local.id));
    similar.append(card);
  }
  const facts = $("albumOverviewFacts"); clear(facts);
  if (state.album) {
    const indicator = node("span", "status-indicator", "● " + (state.album.status || "unprocessed"));
    applyStatusPalette(indicator, mediaStatusKey(state.album));
    facts.append(node("strong", "", state.album.name), indicator, node("span", "", state.album.tracks + " tracks · " + state.album.embedded + " embedded"));
  }
  const songs = state.album ? state.album.track_titles || [] : metadata?.top_songs?.length ? metadata.top_songs : artist?.top_tracks || [];
  $("topSongs").textContent = state.album && !songs.length ? "Album track titles unavailable in the library" : songs.length ? (state.album ? "Album tracks: " : metadata?.top_songs?.length ? "Navidrome top songs: " : "Indexed songs: ") + songs.slice(0, 10).map((song, index) => (index + 1) + ". " + song).join("  ·  ") : "";
  $("hero").style.backgroundImage = artist?.banner ? `url("${artist.banner}")` : artist?.portrait ? `url("${artist.portrait}")` : "none";
  const portrait = $("heroPortrait");
  const replacement = artist?.portrait ? image(artist.portrait, "hero-portrait", artist.name + " artist image") : node("div", "hero-portrait placeholder", "♪");
  replacement.id = "heroPortrait"; portrait.replaceWith(replacement);
  $("artistArtEntry").disabled = !artist;
  $("artistArtEntry").title = "Open Artist Art Enhancements";
  $("artistArtEntry").setAttribute("aria-label", "Open Artist Art Enhancements");
  $("artistArtEntry").classList.toggle("active", !!artist && state.editorMode === "artist");
  const grid = $("albumGrid"); clear(grid);
  const compById = new Map(state.compAlbums.map(group => [group.id, group]));
  const compIds = new Set(compById.keys());
  const projected = state.compAlbums.filter(group => !(artist?.albums || []).some(album => album.id === group.id)).map(group => ({
    id: group.id, name: group.name, year: group.year, cover: group.cover, cover_found: !!group.cover,
    album_mbid: "", compilation: true, tracks: group.tracks.length,
    embedded: group.tracks.filter(track => track.completed).length,
    fallback_complete: group.tracks.length > 0 && group.tracks.every(track => track.completed),
    status: group.tracks.every(track => track.completed) ? "processed" : "incomplete",
  }));
  const indexed = (artist?.albums || []).filter(album => album.album_mbid || compIds.has(album.id)).map(album => {
    const group = compById.get(album.id);
    // COMP album cards use their indexed folder cover. Track thumbnails are
    // separate COMP-report evidence and never replace the album-card cover.
    return group ? {...album, cover: group.cover || album.cover || null,
      cover_found: !!(group.cover || album.cover), compilation: true,
      embedded: group.tracks.filter(track => track.completed).length,
      fallback_complete: group.tracks.length > 0 && group.tracks.every(track => track.completed)} : album;
  });
  const albums = [...indexed, ...projected].sort((a, b) =>
    albumPresentationGroup(a, artist) - albumPresentationGroup(b, artist)
    || (parseInt(b.year, 10) || 0) - (parseInt(a.year, 10) || 0)
    || a.name.localeCompare(b.name));
  $("albumCount").textContent = albums.length + " shown";
  renderAlbumStatusKey(albums, artist);
  if (!albums.length) { grid.append(node("p", "empty-copy", "Choose an artist to view albums.")); renderArtistCompReport(); return; }
  for (const album of albums) {
    const button = node("button", "album-card windows-card"); button.type = "button";
    button.dataset.compAlbum = compIds.has(album.id) ? "true" : "false";
    button.disabled = compIds.has(album.id) && state.compReportLoading;
    const cardState = albumCardState(album, artist); button.style.setProperty("--frame-color", cardState.frame);
    button.setAttribute("aria-label", album.name + ", " + (album.year || "year unknown") + ", " + (album.status || "unprocessed"));
    const kind = albumBannerKind(album, artist), bannerStyle = albumBannerStyle(kind);
    const bannerNode = node("span", "album-type-banner", bannerStyle.name); bannerNode.style.setProperty("--banner-bg", bannerStyle.background); bannerNode.style.setProperty("--banner-ink", bannerStyle.letters); bannerNode.style.borderColor = bannerStyle.frame;
    const body = node("span", "album-card-body");
    const picture = node("span", "album-card-picture"); picture.append(album.cover ? image(album.cover, "", album.name + " cover") : node("span", "album-placeholder", "♫"));
    const dots = node("span", "album-status-dots"); for (const [color, tip] of cardState.dots) { const dot = node("span", "", "●"); dot.style.color = color; dot.title = tip; dots.append(dot); }
    picture.append(dots);
    if (state.embedQueue.some(item => item.id === album.id)) { const staged = node("span", "album-staged-indicator", "●"); staged.title = "Staged for embedding"; picture.append(staged); }
    body.append(picture, node("strong", "", album.name), node("span", "album-card-foot", (album.year || "—") + "   ☆☆☆☆☆"));
    button.append(bannerNode, body);
    button.addEventListener("click", () => compIds.has(album.id) ? openArtistCompReport(album.id) : chooseAlbum(album.id)); grid.append(button);
  }
  renderArtistCompReport();
}

function renderFallbackOverview() {
  const album = state.album, track = state.fallbackTrack;
  const total = state.fallbackTracks.length;
  const green = state.fallbackTracks.filter(item => item.status === "green").length;
  const purple = state.fallbackTracks.filter(item => item.status === "purple").length;
  const blue = state.fallbackTracks.filter(item => item.status === "blue").length;
  const checking = state.fallbackTracks.filter(item => item.status === "checking").length;
  const checkErrors = state.fallbackTracks.filter(item => item.status === "check-error").length;
  const complete = state.fallbackTracks.filter(item => item.completed).length;
  const pending = blue;
  const hero = $("hero"); hero.classList.add("fallback-overview");
  hero.style.backgroundImage = "none";
  $("overviewHeading").textContent = "COMPILATION Fallback Track Embedding";
  $("overviewArtist").textContent = album ? `COMP · ${total} tracks below ↓  ${album.artist_name} > ${album.name}` : "";
  if (!album) {
    $("fallbackHelp").hidden = true;
    $("albumStatusHelp").hidden = true;
    $("albumStatusKey").hidden = true;
    document.querySelector(".overview-actions").hidden = true;
    document.querySelector(".meta-heading").textContent = "COMP Albums";
    $("albumCount").textContent = "0 shown";
    const grid = $("albumGrid"); clear(grid);
    grid.append(node("p", "empty-copy fallback-empty-copy",
      "No missing-ID COMP albums in this isolated test index. Select an artist to open the artist-wide COMP Report when eligible albums are available."));
    const report = $("fallbackReport"); clear(report); report.hidden = true;
    $("topSongs").textContent = "";
    return;
  }
  $("fallbackHelp").hidden = false;
  $("fallbackHelp").classList.toggle("expanded", state.fallbackHelpExpanded);
  $("fallbackHelpToggle").textContent = state.fallbackHelpExpanded ? "(less…)" : "(…more)";
  $("fallbackHelpToggle").setAttribute("aria-expanded", String(state.fallbackHelpExpanded));
  $("albumStatusHelp").hidden = false;
  document.querySelector(".overview-actions").hidden = true;
  document.querySelector(".meta-heading").textContent = "Albums";
  $("albumCount").textContent = `1 shown / ${pending ? "0 Completed / 1 in Process" : "1 Completed / 0 in Process"}`;
  const grid = $("albumGrid"); clear(grid);
  if (album) {
    const card = node("div", "album-card windows-card fallback-album-card");
    card.style.setProperty("--frame-color", state.graphicalEnhancement.frame_styles.thumbnail.color);
    const cover = node("span", "album-card-picture");
    cover.append(album.cover ? image(album.cover, "", album.name + " album card cover") : node("span", "album-placeholder", "♫"));
    const body = node("span", "album-card-body");
    body.append(cover, node("strong", "", album.name), node("span", "album-card-foot", (album.year || "—") + "   ☆☆☆☆☆"));
    const compiled = albumBannerStyle("compiled"), banner = node("span", "album-type-banner", compiled.name);
    banner.style.setProperty("--banner-bg", compiled.background); banner.style.setProperty("--banner-ink", compiled.letters); banner.style.borderColor = compiled.frame;
    card.append(banner, body); grid.append(card);
  }
  renderAlbumStatusKey(album ? [album] : [], state.artist);
  const report = $("fallbackReport"); clear(report); report.hidden = false;
  const toolbar = node("div", "fallback-toolbar");
  const preview = node("button", "fallback-preview-radio" + (state.fallbackPreviewMode ? " active" : ""));
  preview.type = "button"; preview.title = "Toggle existing embedded-art preview mode";
  preview.setAttribute("aria-label", "Toggle existing embedded-art preview mode");
  preview.setAttribute("aria-pressed", String(state.fallbackPreviewMode));
  preview.addEventListener("click", () => { state.fallbackPreviewMode = !state.fallbackPreviewMode; renderHero(); renderReview(); });
  const heading = node("strong", "", "COMP Report · " + (album?.name || ""));
  const repair = node("button", "fallback-repair", "REPAIR COMP"); repair.type = "button";
  repair.disabled = !state.compRepairEnabled || !state.fallbackTracks.some(item => item.status === "purple" && item.repairable);
  repair.title = !state.compRepairEnabled ? "COMP repairs are disabled in this test service" : purple ? "Repair readable purple thumbnails in this full Fallback album" : "No purple thumbnails identified";
  repair.addEventListener("click", () => runAction(repairFallback));
  const launch = node("button", "fallback-launch", "LAUNCH COMP"); launch.type = "button";
  launch.disabled = state.server?.mode !== "write" || !pending || state.fallbackPreviewMode;
  launch.title = state.server?.mode !== "write" ? "Enable Live Write to process COMP tracks" : "Review every pending track in this fallback album";
  launch.addEventListener("click", () => runAction(launchFallback));
  toolbar.append(preview, heading, repair, launch); report.append(toolbar);
  report.append(node("div", "fallback-report-summary", `${total} tracks: ${green} green, ${purple} purple, ${blue} blue${checking ? `, ${checking} checking` : ""}${checkErrors ? `, ${checkErrors} retry needed` : ""}`));
  const columns = node("div", "fallback-report-columns"); columns.append(node("span", "", "ALBUM"), node("span", "", "TRACK FILE")); report.append(columns);
  const albumRow = node("button", "fallback-report-album"); albumRow.type = "button";
  albumRow.append(node("strong", "", (state.fallbackReportExpanded ? "▾  " : "▸  ") + (album?.name || "Album")),
    node("span", "", `${total} tracks · ${complete} recorded, ${total - complete} need artwork`));
  albumRow.addEventListener("click", () => { state.fallbackReportExpanded = !state.fallbackReportExpanded; renderHero(); });
  report.append(albumRow);
  if (state.fallbackReportExpanded) for (const item of state.fallbackTracks) {
    const row = node("button", "fallback-track" + (track?.key === item.key ? " selected" : ""));
    row.type = "button";
    row.classList.add(item.status === "purple" ? "purple" : ["checking", "check-error"].includes(item.status) ? "checking" : item.completed ? "complete" : "pending");
    if (item.status === "check-error") row.title = item.checkError || "Thumbnail validation could not be completed; reload the album to retry";
    row.append(node("span", "fallback-track-spacer"), node("span", "fallback-track-file", item.file),
      node("span", "fallback-track-status", "COMP status · " + (item.status === "green" ? "complete" : item.status === "purple" ? "thumbnail repair" : item.status === "checking" ? "checking thumbnail" : "needs artwork")));
    row.addEventListener("click", () => selectFallbackTrack(item.key, state.fallbackPreviewMode)); report.append(row);
  }
  if (!total) report.append(node("p", "empty-copy", "No indexed audio tracks in this fallback album."));
  $("topSongs").textContent = "";
}

async function probeFallbackStatuses() {
  const request = ++state.fallbackStatusRequest, albumId = state.album?.id;
  const targets = state.fallbackTracks.filter(track => track.completed);
  if (!albumId || state.editorMode !== "fallback") return;
  if (!targets.length) {
    setStatus(`COMP report ready · ${state.fallbackTracks.length} tracks · no recorded thumbnails to validate`);
    return;
  }
  for (let offset = 0; offset < targets.length; offset += 3) {
    const batch = targets.slice(offset, offset + 3);
    const results = await Promise.allSettled(batch.map(track =>
      api("/api/fallback/track-status?id=" + encodeURIComponent(albumId) + "&key=" + encodeURIComponent(track.key))));
    if (request !== state.fallbackStatusRequest || albumId !== state.album?.id || state.editorMode !== "fallback") return;
    results.forEach((result, index) => {
      if (result.status === "fulfilled") Object.assign(batch[index], result.value);
      else Object.assign(batch[index], {status: "check-error", repairable: false, checkError: result.reason?.message || "Thumbnail validation failed"});
    });
    renderHero();
  }
  if (request !== state.fallbackStatusRequest || albumId !== state.album?.id || state.editorMode !== "fallback") return;
  const failures = targets.filter(track => track.status === "check-error").length;
  setStatus(`COMP report ready · ${state.fallbackTracks.length} tracks · ${targets.length - failures} recorded thumbnails validated${failures ? ` · ${failures} retry needed` : ""}`, failures ? "error" : "ready");
}

function renderArtistCompReport() {
  const report = $("fallbackReport");
  if (state.editorMode !== "artist" || !state.compReportVisible || !state.compGroups.length) {
    report.hidden = true; return;
  }
  clear(report); report.hidden = false;
  const tracks = state.compGroups.flatMap(group => group.tracks.map(track => ({...track, albumId: group.id})));
  const green = tracks.filter(track => track.status === "green").length;
  const purple = tracks.filter(track => track.status === "purple").length;
  const blue = tracks.filter(track => track.status === "blue").length;
  const checking = tracks.filter(track => track.status === "checking").length;
  const checkErrors = tracks.filter(track => track.status === "check-error").length;
  const toolbar = node("div", "fallback-toolbar");
  const preview = node("button", "fallback-preview-radio" + (state.compPreviewMode ? " active" : ""));
  preview.type = "button"; preview.title = "Preview existing embedded track art";
  preview.setAttribute("aria-label", "Preview existing embedded track art");
  preview.setAttribute("aria-pressed", String(state.compPreviewMode));
  preview.addEventListener("click", () => { state.compPreviewMode = !state.compPreviewMode; renderArtistCompReport(); setStatus(state.compPreviewMode ? "COMP preview on · choose any green track" : "COMP preview off"); });
  const heading = node("strong", "", "COMP Report · " + (state.artist?.name || "Artist"));
  const repair = node("button", "fallback-repair", "REPAIR COMP"); repair.type = "button";
  repair.disabled = state.compBusy || !state.compRepairEnabled || !tracks.some(track => track.status === "purple" && track.repairable);
  repair.title = !state.compRepairEnabled ? "COMP repairs are disabled in this test service" : purple ? "Repair readable purple thumbnails across this Artist's COMP albums" : "No purple thumbnails identified";
  repair.addEventListener("click", repairArtistComp);
  const launch = node("button", "fallback-launch", "LAUNCH COMP"); launch.type = "button";
  launch.disabled = state.server?.mode !== "write" || !blue || state.compPreviewMode || state.compBusy;
  launch.title = state.server?.mode !== "write" ? "Enable Live Write to process COMP tracks" : "Review this Artist's blue COMP tracks";
  launch.addEventListener("click", () => runAction(launchArtistComp));
  const close = node("button", "fallback-close", "CLOSE REPORT"); close.type = "button";
  close.addEventListener("click", closeArtistCompReport);
  toolbar.append(preview, heading, repair, launch, close); report.append(toolbar);
  report.append(node("div", "fallback-report-summary", `${state.compGroups.length} albums · ${tracks.length} tracks: ${green} green, ${purple} purple, ${blue} blue${checking ? `, ${checking} checking` : ""}${checkErrors ? `, ${checkErrors} retry needed` : ""}`));
  const columns = node("div", "fallback-report-columns"); columns.append(node("span", "", "ALBUM"), node("span", "", "TRACK FILE")); report.append(columns);
  for (const group of state.compGroups) {
    const collapsed = state.compCollapsed.has(group.id);
    const groupComplete = group.tracks.filter(track => track.completed).length;
    const albumRow = node("button", "fallback-report-album"); albumRow.type = "button";
    albumRow.dataset.compAlbumId = group.id;
    albumRow.append(node("strong", "", (collapsed ? "▸  " : "▾  ") + group.name),
      node("span", "", `${group.tracks.length} tracks · ${groupComplete} recorded, ${group.tracks.length - groupComplete} need artwork`));
    albumRow.addEventListener("click", () => { if (collapsed) state.compCollapsed.delete(group.id); else state.compCollapsed.add(group.id); renderArtistCompReport(); });
    report.append(albumRow);
    if (collapsed) continue;
    for (const track of group.tracks) {
      const selected = state.compTrack?.key === track.key && state.compTrack?.albumId === group.id;
      const row = node("button", "fallback-track" + (selected ? " selected" : "")); row.type = "button";
      row.classList.add(track.status === "purple" ? "purple" : ["checking", "check-error"].includes(track.status) ? "checking" : track.completed ? "complete" : "pending");
      if (track.status === "check-error") row.title = track.checkError || "Thumbnail validation could not be completed; reopen the report to retry";
      row.append(node("span", "fallback-track-spacer"), node("span", "fallback-track-file", track.file),
        node("span", "fallback-track-status", "COMP status · " + (track.status === "green" ? "complete" : track.status === "purple" ? "thumbnail repair" : track.status === "checking" ? "checking thumbnail" : "needs artwork")));
      const thumbnail = selected && state.compPreviewMode
        ? state.compTrack?.image || track.thumbnail : null;
      if (thumbnail) row.append(image(thumbnail, "fallback-inline-thumb", "Existing track art"));
      row.addEventListener("click", () => selectArtistCompTrack(group.id, track.key)); report.append(row);
    }
  }
}

function closeArtistCompReport() {
  if (state.compBusy) return;
  ++state.compReportRequest; ++state.compStatusRequest; ++state.compTrackRequest;
  state.compReportLoading = false; state.compReportVisible = false; state.compGroups = []; state.compTrack = null;
  state.upscalePreview = null; state.candidates = state.artist?.local_candidates || [];
  $("compReportButton").textContent = "COMP Report"; $("compReportButton").disabled = false;
  renderHero(); renderCandidates(); renderReview();
  setStatus("COMP report closed");
}

async function openArtistCompReport(albumId = "") {
  if (!state.artist) { setStatus("Choose an Artist before opening COMP Report", "error"); return; }
  if (state.compReportLoading) return;
  const request = ++state.compReportRequest, artistId = state.artist.id;
  ++state.compStatusRequest; ++state.compTrackRequest;
  state.compReportLoading = true; state.compGroups = []; state.compTrack = null;
  $("compReportButton").disabled = true;
  renderHero();
  setStatus("Loading COMP report…", "busy");
  try {
    const compGroups = await api("/api/artist/comp-report?id=" + encodeURIComponent(artistId));
    if (request !== state.compReportRequest || artistId !== state.artist?.id) return;
    state.compGroups = compGroups;
    state.compReportLoading = false;
    if (!state.compGroups.length) { state.compReportVisible = false; setStatus("This Artist has no indexed COMP tracks", "error"); renderHero(); return; }
    state.compReportVisible = true; state.compPreviewMode = false; state.compTrack = null;
    $("compReportButton").textContent = "CLOSE REPORT";
    if (albumId) state.compCollapsed.delete(albumId);
    state.candidates = []; state.upscalePreview = null; renderHero(); renderCandidates(); renderReview();
    const report = $("fallbackReport");
    requestAnimationFrame(() => requestAnimationFrame(() => {
      if (!state.compReportVisible || request !== state.compReportRequest) return;
      const viewport = $("overviewContent");
      const target = albumId
        ? [...report.querySelectorAll("[data-comp-album-id]")].find(row => row.dataset.compAlbumId === albumId)
        : report;
      viewport.scrollTop += (target || report).getBoundingClientRect().top - viewport.getBoundingClientRect().top - 12;
    }));
    setStatus(`COMP report loaded · ${state.compGroups.length} albums · validating recorded thumbnails`, "busy");
    probeArtistCompStatuses();
  } catch (error) {
    if (request === state.compReportRequest) { state.compReportLoading = false; setStatus(error.message, "error"); renderHero(); }
  } finally {
    if (request === state.compReportRequest) $("compReportButton").disabled = false;
  }
}

async function probeArtistCompStatuses() {
  const request = ++state.compStatusRequest, artistId = state.artist?.id;
  const targets = state.compGroups.flatMap(group => group.tracks.filter(track => track.completed)
    .map(track => ({group, track})));
  if (!targets.length) {
    setStatus(`COMP report ready · ${state.compGroups.length} albums · no recorded thumbnails to validate`);
    return;
  }
  for (let offset = 0; offset < targets.length; offset += 3) {
    const batch = targets.slice(offset, offset + 3);
    const results = await Promise.allSettled(batch.map(({group, track}) =>
      api("/api/fallback/track-status?id=" + encodeURIComponent(group.id) + "&key=" + encodeURIComponent(track.key))));
    if (request !== state.compStatusRequest || artistId !== state.artist?.id || !state.compReportVisible) return;
    results.forEach((result, index) => {
      if (result.status === "fulfilled") Object.assign(batch[index].track, result.value);
      else Object.assign(batch[index].track, {status: "check-error", repairable: false, checkError: result.reason?.message || "Thumbnail validation failed"});
    });
    renderArtistCompReport();
  }
  if (request !== state.compStatusRequest || artistId !== state.artist?.id || !state.compReportVisible) return;
  const failures = targets.filter(({track}) => track.status === "check-error").length;
  setStatus(`COMP report ready · ${state.compGroups.length} albums · ${targets.length - failures} recorded thumbnails validated${failures ? ` · ${failures} retry needed` : ""}`, failures ? "error" : "ready");
}

async function selectArtistCompTrack(albumId, key) {
  const request = ++state.compTrackRequest, artistId = state.artist?.id;
  await cancelMismatchedCompWork({artist_id: artistId, album_id: albumId, track_key: key});
  if (request !== state.compTrackRequest || artistId !== state.artist?.id || !state.compReportVisible) return;
  state.upscalePreview = null; state.renderedPreview.comp = null; state.compThumbnailPreview = null; state.pendingCompWrite = null; state.fx.comp = {};
  setStatus("Loading existing COMP track artwork…", "busy");
  try {
    const track = await api("/api/fallback/track?id=" + encodeURIComponent(albumId) + "&key=" + encodeURIComponent(key));
    if (request !== state.compTrackRequest || artistId !== state.artist?.id || !state.compReportVisible) return;
    state.compTrack = {...track, albumId};
    renderArtistCompReport(); renderReview();
    setStatus(`${track.completed ? "Recorded" : "Blue"} COMP track · ${track.file}${track.image ? " · existing art available" : " · artwork needed"}`);
  } catch (error) { if (request === state.compTrackRequest) setStatus(error.message, "error"); }
}


async function repairArtistComp() {
  if (state.compBusy || !state.compRepairEnabled || !state.artist) return;
  const artistId = state.artist.id;
  const targets = state.compGroups.flatMap(group => group.tracks.filter(track => track.status === "purple" && track.repairable)
    .map(track => ({group, track})));
  if (!targets.length) { setStatus("No readable purple COMP thumbnails need repair"); return; }
  if (!window.confirm(`Repair ${targets.length} purple COMP thumbnail${targets.length === 1 ? "" : "s"} in the configured music library? Original embedded images will be backed up.`)) return;
  state.compBusy = true; renderArtistCompReport();
  let repaired = 0, failed = 0;
  for (const [index, {group, track}] of targets.entries()) {
    if (artistId !== state.artist?.id) break;
    setStatus(`Repairing COMP thumbnail ${index + 1}/${targets.length} · ${track.file}`, "busy");
    try {
      const result = await api("/api/artist/comp-repair", {method: "POST", headers: {"Content-Type": "application/json"},
        body: JSON.stringify({artist_id: artistId, album_id: group.id, track_key: track.key})});
      Object.assign(track, result, {status: "green", completed: true, repairable: false});
      repaired++;
      if (state.compTrack?.albumId === group.id && state.compTrack?.key === track.key)
        await selectArtistCompTrack(group.id, track.key);
    } catch (error) { failed++; track.repairable = false; track.repairError = error.message; }
    renderArtistCompReport();
  }
  state.compBusy = false; renderArtistCompReport();
  setStatus(`COMP repair complete · ${repaired} green${failed ? ` · ${failed} failed` : ""}`, failed ? "error" : "ready");
}

function renderSources() {
  const strip = $("sourceStrip"); clear(strip);
  const decision = activeArtworkDecision();
  const staleDecision = !!state.queuedDecision && !decision;
  const candidates = staleDecision ? [] : state.candidates;
  const diagnostics = staleDecision ? [] : state.sourceDiagnostics;
  const artistImageSources = new Set(["local", "fanart", "deezer", "lastfm", "discogs", "navidrome"]);
  for (const [label, key, icon] of SOURCES) {
    const count = candidates.filter(candidate => sourceKey(candidate.source) === key).length;
    const navidromeMetadata = key === "navidrome" && state.artistMetadata?.source === "Navidrome";
    const configured = key === "local" || (state.enabledSources || []).includes(key === "fanart" ? "fanarttv" : key);
    const searchable = state.editorMode === "album" ? configured
      : artistImageSources.has(key) && (configured || navidromeMetadata);
    const runtime = sourceDiagnosticStatus(diagnostics, key);
    const runtimeClass = !count && runtime.status ? " " + runtime.status : "";
    const button = node("button", "source-card" + runtimeClass + (state.activeSource === key && key ? " active" : "")); button.type = "button";
    button.title = count ? "Show " + label + " candidates"
      : runtime.message ? label + " · " + runtime.message
      : label + (searchable ? " · ready for artist search" : " · unavailable for this selection");
    button.append(icon === "⊘" ? node("span", "source-symbol", "⊘") : image("/assets/source-icons/" + icon + ".svg", "", ""));
    const status = count ? "● Results" : runtime.status === "unavailable" ? "⚠ Unavailable"
      : runtime.status === "filtered" ? "○ Filtered" : runtime.status === "warning" ? "⚠ Warning"
      : navidromeMetadata ? "● Metadata" : searchable ? "● Ready" : "○ Inactive";
    button.append(node("strong", "", label), node("small", "", count + " found"), node("span", "source-state" + (searchable ? "" : " pending"), status));
    button.addEventListener("click", () => { state.activeSource = state.activeSource === key ? "" : key; renderSources(); renderCandidates(); }); strip.append(button);
  }
}
function musicBrainzFamilyKey(option) {
  return String(option.family_id || option.release_group_id || option.id || "").toLowerCase();
}
function musicBrainzFamilyTitle(option) {
  return option.family_title || option.release_group_title || option.title || "Unknown release family";
}
function musicBrainzScore(option) {
  const score = Number(option.score);
  return Number.isFinite(score) ? score : 0;
}
function musicBrainzSameArtist(option) {
  const recording = String(option.recording_artist || option.artist || "").trim().toLowerCase();
  const release = String(option.release_artist || recording).trim().toLowerCase();
  return !!recording && release === recording;
}
function musicBrainzFamilySection(options) {
  const score = Math.max(0, ...options.map(musicBrainzScore));
  if (score >= 90) return 0;
  if (options.some(option => musicBrainzSameArtist(option)
    && ["album", "compilation"].includes(String(option.release_class || "").toLowerCase()))) return 1;
  return 2;
}
function musicBrainzFamilies(options) {
  const families = new Map();
  options.forEach((option, index) => {
    const key = musicBrainzFamilyKey(option) || `release-${index}`;
    const family = families.get(key) || {key, title: musicBrainzFamilyTitle(option), options: []};
    family.options.push({option, index}); families.set(key, family);
  });
  return [...families.values()].map(family => ({
    ...family,
    score: Math.max(0, ...family.options.map(({option}) => musicBrainzScore(option))),
    section: musicBrainzFamilySection(family.options.map(({option}) => option)),
  })).sort((left, right) => left.section - right.section || right.score - left.score
    || left.title.localeCompare(right.title) || left.key.localeCompare(right.key));
}
function appendMusicBrainzFilters(host, decision) {
  const controls = node("div", "musicbrainz-filters");
  const artists = [...new Set((decision.options || []).map(option => option.recording_artist || option.artist || "").filter(Boolean))];
  const types = [...new Set((decision.options || []).map(option => option.release_class || "").filter(Boolean))];
  const select = (label, values, key) => {
    const owner = node("label", "musicbrainz-filter", label), input = node("select");
    const all = node("option", "", "All"); all.value = ""; input.append(all);
    values.sort().forEach(value => { const option = node("option", "", value); option.value = value; input.append(option); });
    input.value = decision[key] || "";
    input.addEventListener("change", () => { decision[key] = input.value; renderMusicBrainzWorkspace(); });
    owner.append(input); controls.append(owner);
  };
  select("Artist", artists, "artistFilter"); select("Release Type", types, "releaseTypeFilter"); host.append(controls);
}
function appendMusicBrainzAuthorityEditor(host, decision) {
  const authority = decision.authority || {}, details = node("details", "musicbrainz-authority-editor");
  details.append(node("summary", "", "Advanced IDs"));
  const fields = node("div", "musicbrainz-authority-fields");
  const recording = node("input"); recording.type = "text"; recording.placeholder = "Recording MBID"; recording.value = authority.recording_mbid || "";
  const artist = node("input"); artist.type = "text"; artist.placeholder = "Artist MBID"; artist.value = authority.artist_mbids || "";
  const apply = node("button", "", "Apply IDs"); apply.type = "button";
  apply.addEventListener("click", () => runAction(() => submitQueuedDecision(JSON.stringify({action: "manual-authority-query",
    artist_id: artist.value, recording_id: recording.value, edited: "all"}))));
  fields.append(recording, artist, apply); details.append(fields); host.append(details);
}
function renderMusicBrainzWorkspace() {
  const host = $("musicbrainzReview"), decision = activeArtworkDecision();
  clear(host);
  if (decision?.kind !== "musicbrainz-results") { host.hidden = true; return; }
  host.hidden = false;
  const identity = decision.context?.identity || {}, authority = decision.authority || {};
  const trackArtist = identity.friendly_artist || decision.context?.track_artist || authority.artist || "Unknown artist";
  const trackTitle = identity.friendly_title || decision.context?.track_title || authority.track || "Unknown recording";
  const heading = node("div", "musicbrainz-review-heading"), copy = node("div");
  copy.append(node("span", "musicbrainz-kicker", "MusicBrainz"), node("h2", "", `TRACK  ${trackArtist} — ${trackTitle}`));
  const artistId = String(identity.focused_artist_mbid || authority.artist_mbids || "").split(/[;,]/)[0].trim();
  const info = node("a", "musicbrainz-info-link", "MB INFO"); info.target = "_blank"; info.rel = "noopener";
  info.href = artistId ? "https://musicbrainz.org/artist/" + encodeURIComponent(artistId)
    : "https://musicbrainz.org/search?query=" + encodeURIComponent(trackArtist) + "&type=artist&method=indexed";
  heading.append(copy, info); host.append(heading);
  const statusClass = identity.status === "valid" ? " valid" : identity.status === "failed" ? " failed" : " warning";
  const identityBand = node("div", "musicbrainz-identity" + statusClass);
  identityBand.append(node("strong", "", `${trackArtist} — ${trackTitle}`),
    node("span", "musicbrainz-identity-states", `Artist ID · ${identity.artist_status || "unresolved"} · Recording ID · ${identity.recording_status || "unresolved"}`),
    node("span", "", identity.message || "MusicBrainz identity lookup completed.")); host.append(identityBand);
  const current = decision.context?.current_album || {}, authorityCard = node("section", "musicbrainz-current-album");
  authorityCard.append(node("strong", "", "CURRENT ALBUM · SPLINED LIBRARY AUTHORITY"));
  const currentBody = node("div", "musicbrainz-current-body");
  const currentAlbumId = decision.context?.album_id || decision.payload?.album_id || "";
  const scopedCompCover = state.compGroups.find(group => group.id === currentAlbumId)?.cover || "";
  const currentImage = scopedCompCover || state.album?.cover || state.compTrack?.image || state.fallbackTrack?.image || "";
  if (currentImage) currentBody.append(image(currentImage, "musicbrainz-current-cover", current.title || "Current compilation"));
  const currentCopy = node("div"); currentCopy.append(node("b", "", current.title || state.album?.name || "Current compilation"),
    node("span", "", current.artist || "Various Artists"),
    node("small", "", current.release_id ? `MusicBrainz Release ID · ${current.release_id}` : "No MusicBrainz Release ID"));
  currentBody.append(currentCopy); authorityCard.append(currentBody); host.append(authorityCard);
  appendMusicBrainzAuthorityEditor(host, decision);
  const controls = node("div", "musicbrainz-results-controls");
  controls.append(node("strong", "", "MusicBrainz Results · Best Match, Artist, then Other · % by family"));
  appendMusicBrainzFilters(controls, decision); host.append(controls);
  const existing = decision.existingArtwork || [];
  if (existing.length) {
    const section = node("section", "musicbrainz-result-section existing");
    section.append(node("h3", "", "BEST MATCH · EXISTING SPLINED ALBUM ARTWORK"));
    const cards = node("div", "musicbrainz-existing-grid");
    existing.forEach((item, index) => {
      const button = node("button", "musicbrainz-existing-card" + (decision.selectedExisting === index ? " selected" : "")); button.type = "button";
      if (item.image) button.append(image(item.image, "", item.title || "Existing Album artwork"));
      button.append(node("strong", "", item.title || "Existing Album"), node("span", "", item.artist || trackArtist),
        node("small", "", item.kind === "recording-match" ? "Recording ID match" : "Artist/title match"));
      button.addEventListener("click", () => { decision.selectedExisting = index; decision.selectedResult = null; renderMusicBrainzWorkspace(); }); cards.append(button);
    });
    section.append(cards); host.append(section);
  }
  const visible = (decision.options || []).map((option, originalIndex) => ({...option, originalIndex})).filter(option =>
    (!decision.artistFilter || String(option.recording_artist || option.artist || "").toLowerCase() === decision.artistFilter.toLowerCase())
    && (!decision.releaseTypeFilter || String(option.release_class || "").toLowerCase() === decision.releaseTypeFilter.toLowerCase()));
  const labels = ["BEST MATCH · 90–100%", "ARTIST RELEASES", "OTHER MATCHING RELEASES · SOUNDTRACKS, COMPILATIONS & APPEARANCES"];
  const families = musicBrainzFamilies(visible);
  labels.forEach((label, sectionIndex) => {
    const rows = families.filter(family => family.section === sectionIndex);
    if (!rows.length) return;
    const section = node("section", "musicbrainz-result-section"); section.append(node("h3", "", label));
    rows.forEach(family => {
      const familyHost = node("div", "musicbrainz-family"); familyHost.append(node("h4", "", `${family.score}% match · Release Family · ${family.title}`));
      family.options.forEach(({option, index}) => {
        const resultIndex = Number.isInteger(option.originalIndex) ? option.originalIndex : index;
        const selected = decision.selectedResult === resultIndex;
        const row = node("button", "musicbrainz-release-row" + (selected ? " selected" : "")
          + (option.selection_state ? " " + option.selection_state : "")); row.type = "button";
        row.setAttribute("aria-label", `Select MusicBrainz release ${resultIndex + 1}: ${option.title || family.title}`);
        row.append(node("span", "musicbrainz-release-radio", selected ? "●" : "○"));
        const rowCopy = node("span", "musicbrainz-release-copy");
        rowCopy.append(node("strong", "", option.title || family.title),
          node("small", "", [option.release_artist || trackArtist, option.country, option.date, option.release_class, option.id].filter(Boolean).join(" · ")),
          node("small", "musicbrainz-release-state", [option.match_label || String(option.match_basis || "").replaceAll("-", " "), option.resolution,
            option.selection_state === "current" ? "current release" : option.selection_state === "visited" ? "reviewed" : ""].filter(Boolean).join(" · ")));
        row.append(rowCopy); row.addEventListener("click", () => { decision.selectedResult = resultIndex; decision.selectedExisting = null; renderMusicBrainzWorkspace(); }); familyHost.append(row);
      });
      section.append(familyHost);
    }); host.append(section);
  });
  if (!visible.length) host.append(node("p", "musicbrainz-empty", (decision.options || []).length
    ? "No MusicBrainz release families match the current filters."
    : `MusicBrainz returned no selectable release families for ${trackArtist} — ${trackTitle}. Correct Advanced IDs or keep the current track unchanged.`));
  if (decision.authoritySaved?.message) host.append(node("p", "operation-success", decision.authoritySaved.message));
  distinctDiagnostics(decision.diagnostics).forEach(message => host.append(node("p", "operation-error", message)));
  const actions = node("div", "musicbrainz-review-actions");
  const back = node("button", "", "Back to Candidates"); back.type = "button";
  back.disabled = decision.context?.can_back === false;
  back.title = back.disabled ? "Select a release before opening artwork candidates" : "Return to the selected release's artwork candidates";
  back.addEventListener("click", () => runAction(() => submitQueuedDecision("__manual_candidates__")));
  const keep = node("button", "", "Keep Current"); keep.type = "button"; keep.addEventListener("click", () => runAction(() => submitQueuedDecision("b")));
  const selectedOption = Number.isInteger(decision.selectedResult) ? decision.options[decision.selectedResult] : null;
  if (selectedOption?.musicbrainz_url) {
    const open = node("a", "", "Open MB Page"); open.href = selectedOption.musicbrainz_url; open.target = "_blank"; open.rel = "noopener"; actions.append(open);
  }
  const useExisting = decision.selectedExisting !== null && decision.selectedExisting !== undefined;
  const use = node("button", "primary", useExisting ? "Use Existing Artwork" : "Use Selected Album"); use.type = "button";
  use.disabled = !selectedOption && !useExisting;
  use.addEventListener("click", () => runAction(() => useExisting
    ? submitQueuedDecision(JSON.stringify({action: "use-existing", number: decision.selectedExisting + 1}))
    : submitQueuedDecision(decision.selectedResult + 1)));
  actions.prepend(back, keep); actions.append(use); host.append(actions);
}
function renderQueuedCompTools() {
  const host = $("compDecisionTools"), decision = activeArtworkDecision();
  clear(host);
  const active = decision?.jobKind === "comp";
  host.hidden = !active; $("candidateDock").classList.toggle("comp-review-active", active);
  if (!active) return;
  const authority = decision.authority || {}, ids = node("div", "comp-authority-copy");
  const track = [decision.context?.track_artist || authority.artist, decision.context?.track_title || authority.track].filter(Boolean).join(" — ");
  ids.append(node("strong", "", track || "COMP track"), node("span", "", authority.recording_mbid ? "Recording ID verified/selected" : "Recording ID unresolved"),
    node("span", "", authority.artist_mbids ? "Artist ID verified/selected" : "Artist ID unresolved"));
  host.append(ids);
  const controls = node("div", "comp-authority-actions");
  const decisionButton = (label, value) => {
    const button = node("button", "", label); button.type = "button";
    button.addEventListener("click", () => runAction(() => submitQueuedDecision(value))); controls.append(button); return button;
  };
  if (decision.kind === "musicbrainz-results") decisionButton("Back to Candidates", "__manual_candidates__");
  else decisionButton(decision.context?.musicbrainz_back ? "MusicBrainz Results" : "Search MusicBrainz", decision.context?.musicbrainz_back ? "__manual_mb_results__" : "m");
  decisionButton("Keep Current", "b");
  const details = node("details", "comp-authority-editor"), summary = node("summary", "", "Advanced IDs / Apply IDs");
  details.append(summary);
  const recording = node("input"); recording.type = "text"; recording.placeholder = "Recording MBID"; recording.value = authority.recording_mbid || "";
  const artist = node("input"); artist.type = "text"; artist.placeholder = "Artist MBID"; artist.value = authority.artist_mbids || "";
  const apply = node("button", "", "Apply IDs"); apply.type = "button";
  apply.addEventListener("click", () => runAction(() => submitQueuedDecision(JSON.stringify({action: "manual-authority-query",
    artist_id: artist.value, recording_id: recording.value, edited: "all"}))));
  details.append(recording, artist, apply); controls.append(details);
  const artistId = String(authority.artist_mbids || "").split(/[;,]/)[0].trim();
  const info = node("a", "comp-mb-info", "MB INFO"); info.target = "_blank"; info.rel = "noopener";
  info.href = artistId ? "https://musicbrainz.org/artist/" + encodeURIComponent(artistId)
    : "https://musicbrainz.org/search?query=" + encodeURIComponent(authority.artist || decision.context?.track_artist || "") + "&type=artist&method=indexed";
  controls.append(info);
  if (decision.kind === "musicbrainz-results") {
    const artists = [...new Set((decision.options || []).map(option => option.recording_artist || option.artist || "").filter(Boolean))];
    const types = [...new Set((decision.options || []).map(option => option.release_class || "").filter(Boolean))];
    const select = (label, values, key) => { const owner = node("label", "comp-result-filter", label), input = node("select");
      const all = node("option", "", "All"); all.value = ""; input.append(all);
      for (const value of values) { const option = node("option", "", value); option.value = value; input.append(option); }
      input.value = decision[key] || ""; input.addEventListener("change", () => { decision[key] = input.value; renderCandidates(); }); owner.append(input); controls.append(owner); };
    select("Artist", artists, "artistFilter"); select("Release Type", types, "releaseTypeFilter");
  }
  host.append(controls);
  if (decision.authoritySaved?.message) host.append(node("p", "operation-success", decision.authoritySaved.message));
  const diagnostics = distinctDiagnostics(decision.diagnostics);
  if (diagnostics.length) {
    const details = node("details", "comp-diagnostics");
    details.append(node("summary", "", `Diagnostics (${diagnostics.length})`));
    const messages = node("div", "comp-diagnostics-list");
    diagnostics.forEach(message => messages.append(node("p", "operation-error", message)));
    details.append(messages); host.append(details);
  }
}
function renderCandidates() {
  const decision = activeArtworkDecision();
  const staleDecision = !!state.queuedDecision && !decision;
  const skipping = decision?.jobKind === "scan";
  const musicBrainzResults = decision?.kind === "musicbrainz-results";
  const artistCompLaunch = !decision && state.editorMode === "artist" && state.compReportVisible;
  $("searchButton").textContent = skipping ? "SKIP ALBUM" : artistCompLaunch ? "LAUNCH COMP" : "LAUNCH";
  $("searchButton").disabled = skipping ? false
    : decision ? true
    : artistCompLaunch ? state.server?.mode !== "write" || state.compPreviewMode || state.compBusy || !state.compGroups.some(group => group.tracks.some(track => track.status === "blue"))
    : state.editorMode === "fallback" ? state.server?.mode !== "write" || !state.fallbackTracks.some(item => !item.completed) : !state.artist && !state.album;
  const grid = $("candidateGrid"); clear(grid);
  renderQueuedCompTools();
  const query = $("candidateSearch").value.trim().toLowerCase();
  if (musicBrainzResults) {
    $("compareButton").disabled = true;
    $("candidateHint").textContent = [decision.context?.track_artist, decision.context?.track_title].filter(Boolean).join(" — ")
      + " · release families are shown in the main MusicBrainz workspace";
    $("candidateCount").textContent = (decision.options || []).length + " MusicBrainz releases";
    renderCandidateTypeChips();
    renderMusicBrainzWorkspace();
    return;
  }
  const candidateItems = staleDecision ? [] : state.candidates;
  const visible = candidateItems.filter(candidate => candidateVisible(candidate) && (!query || ((candidate.name || "") + " " + (candidate.source || "")).toLowerCase().includes(query)));
  $("candidateCount").textContent = candidateItems.length + (candidateItems.length === 1 ? " candidate" : " candidates");
  $("compareButton").disabled = state.compare.size < 2;
  $("candidateHint").textContent = decision ? "Select a candidate to preview; save from Selected Artwork"
    : artistCompLaunch ? "LAUNCH COMP reviews only this Artist's blue compilation tracks"
    : state.editorMode === "fallback" ? "LAUNCH COMP to review track artwork in queued work"
    : state.editorMode === "album" ? "Select a cover candidate to preview" : "Blue: artist · pink: banner · wheel to browse";
  renderCandidateTypeChips();
  if (!visible.length) { grid.append(node("p", "empty-copy", artistCompLaunch ? "Use LAUNCH COMP to review this Artist's matching compilation tracks."
    : state.editorMode === "fallback" ? "Select a COMP track above to inspect its embedded artwork. Use LAUNCH COMP to review track artwork in queued work."
    : candidateItems.length ? "No candidates match the filter." : "Press LAUNCH to search artist art, or select an album to inspect folder art.")); return; }
  for (const candidate of visible) {
    const kind = candidateKind(candidate);
    const active = decision?.selected === candidate || state.selectedCover === candidate || state.selectedPortrait === candidate || state.selectedBanner === candidate;
    const card = node("div", "candidate-card" + (kind === "banner" ? " banner" : "") + (decision ? " queued" : "") + (active ? " active" : ""));
    const picture = node("div", "candidate-picture");
    const main = node("button", "candidate-image-button"); main.type = "button"; main.setAttribute("aria-label", "Preview " + candidate.name);
    if (candidate.image) main.append(image(candidate.image, "", candidate.name));
    main.append(node("span", "candidate-source-label", candidate.source));
    main.addEventListener("click", () => selectCandidate(candidate, kind === "banner" ? "banner" : "artist")); picture.append(main);
    if (state.editorMode === "artist" && !decision) {
      if (kind !== "banner") {
        const portrait = node("button", "candidate-radio artist" + (state.selectedPortrait === candidate ? " active" : "")); portrait.type = "button"; portrait.setAttribute("aria-label", "Preview as artist image: " + candidate.name);
        portrait.addEventListener("click", () => selectCandidate(candidate, "artist")); picture.append(portrait);
      }
      const bannerCandidate = candidateBanner(candidate);
      if (bannerCandidate) {
        const banner = node("button", "candidate-radio banner" + (state.selectedBanner === bannerCandidate ? " active" : "")); banner.type = "button"; banner.setAttribute("aria-label", "Preview banner: " + bannerCandidate.name);
        banner.addEventListener("click", () => selectCandidate(bannerCandidate, "banner")); picture.append(banner);
      }
    }
    if (active) picture.append(node("span", "candidate-recommended", "●"));
    card.append(picture);
    const line = node("div", "candidate-card-line"); line.append(node("strong", "", candidate.recommended ? "Recommended" : candidate.name));
    if (decision) {
      const dimensions = node("small", "candidate-resolution", candidate.width + " × " + candidate.height);
      const range = candidate.range_type || candidate.range_class || candidateRange(candidate);
      const quality = [candidate.suggested ? "Suggested" : candidate.selected ? "Preferred" : candidate.acceptable === false ? "Rejected" : "Eligible", range].filter(Boolean).join(" · ");
      card.append(dimensions, line, node("small", "candidate-quality", quality)); grid.append(card);
      continue;
    }
    const action = node("button", "candidate-compare" + (state.compare.has(candidate.image) ? " active" : ""), state.compare.has(candidate.image) ? "Compare ●" : "Compare ○");
    action.type = "button";
    action.addEventListener("click", () => {
      if (state.compare.has(candidate.image)) state.compare.delete(candidate.image);
      else state.compare.add(candidate.image);
      renderCandidates();
    });
    line.append(action); card.append(line, node("small", "", candidate.width + " × " + candidate.height)); grid.append(card);
  }
}
function selectCandidate(candidate, target) {
  state.reviewCleared = false;
  state.upscalePreview = null; state.compThumbnailPreview = null; state.pendingCompWrite = null;
  state.renderedPreview = {};
  if (state.queuedDecision) {
    state.queuedDecision.selected = candidate;
    renderCandidates(); renderPanes(); renderReview();
    setStatus("Queued candidate " + candidate.queueNumber + " previewed · save from Selected Artwork");
    return;
  }
  if (state.editorMode === "album") state.selectedCover = candidate;
  else if (target === "banner") { state.selectedBanner = candidate; state.radioCurrent = false; state.radioBanner = true; }
  else { state.selectedPortrait = candidate; state.radioCurrent = false; state.radioArtist = true; }
  renderCandidates(); renderPanes(); renderReview(); renderEnhancementMenus(); setStatus(candidate.name + " selected for " + (target === "banner" ? "banner" : "artist") + " preview");
}

function packageFrame(frame, banner, portrait) {
  clear(frame);
  if (!banner && !portrait) { frame.append(node("span", "empty-image", "No current artist package")); return; }
  if (banner) frame.append(image(banner, "package-banner", "Artist banner"));
  if (portrait) frame.append(image(portrait, "package-portrait", "Artist portrait"));
}
function paneImage(frame, src, label, portrait = false) {
  clear(frame);
  if (!src) { frame.append(node("span", "empty-image", label)); return; }
  if (portrait) { const mask = node("span", "portrait-viewport"); mask.append(image(src, "portrait-preview", label)); frame.append(mask); }
  else frame.append(image(src, "", label));
}
function renderSelectedArtworkMetadata({source, type, dimensions, fileSize, prepared, preview, existing, candidate, originSource}) {
  const host = $("artMetadata"); clear(host);
  const cell = (text, className = "", id = "") => {
    const value = node("div", className, text);
    if (id) value.id = id;
    host.append(value);
  };
  if (!prepared || !preview) {
    host.classList.remove("comparison");
    for (const [label, value, id] of [
      ["Source", source, "selectedSource"], ["Type", type, "selectedType"],
      ["Dimensions", dimensions, "selectedDimensions"], ["File Size", fileSize, "selectedSize"],
    ]) {
      const wrapper = node("div"); wrapper.append(node("small", "", label), node("span", "", value));
      wrapper.lastChild.id = id; host.append(wrapper);
    }
    return;
  }
  host.classList.add("comparison");
  // Match the Windows Selected Artwork comparison: the prepared thumbnail is
  // an in-memory "After" estimate for the same track(s), while Source reports
  // the full-resolution enhancement from which it was prepared.
  const rows = [
    ["Before", "Track(s) (1)", existing?.width && existing?.height ? `${existing.width} × ${existing.height}` : "No embedded art", existing?.bytes ? size(existing.bytes) : "0 B", "meta-before"],
    ["After", "Embedded Thumb", `${prepared.width} × ${prepared.height}`, `~${size(prepared.bytes)}`, "meta-after"],
    ["Source", "Type", "Dimensions", "File Size", "meta-heading"],
    [originSource || "Enhancement", "Upscale Preview", `${preview.width} × ${preview.height}`, size(preview.bytes), "meta-origin"],
  ];
  rows.forEach(([stage, rowType, rowDimensions, rowSize, className]) => {
    cell(stage, className);
    cell(rowType, className, className === "meta-origin" ? "selectedSource" : stage === "After" ? "selectedType" : "");
    cell(rowDimensions, className, stage === "After" ? "selectedDimensions" : "");
    cell(rowSize, className, stage === "After" ? "selectedSize" : "");
  });
}
function renderPanes() {
  const decision = activeArtworkDecision();
  const isComp = decision?.jobKind === "comp" || state.editorMode === "fallback"
    || state.editorMode === "artist" && state.compReportVisible;
  const isAlbum = state.editorMode === "album" || state.editorMode === "fallback" || isComp;
  $("paneGrid").classList.toggle("album-layout", isAlbum);
  $("albumEnhancePane").hidden = !isAlbum;
  $("enhanceTab").textContent = isComp ? "COMP Track Enhancements" : isAlbum ? "Album Art Enhancements" : "Artists Art Enhancements";
  if (isAlbum) {
    $("albumPaneHeading").textContent = isComp ? "COMP TRACK PREVIEW" : "ALBUM COVER PREVIEW";
    const compTrackImage = state.editorMode === "fallback" ? state.fallbackTrack?.image : state.compTrack?.image;
    const source = isComp ? activeCompPrepared()?.image || state.renderedPreview.comp?.image || state.upscalePreview?.image || decision?.selected?.image || compTrackImage
      : state.editorMode === "fallback" ? state.fallbackTrack?.image : state.renderedPreview.album?.image || state.upscalePreview?.image || state.selectedCover?.image || state.album?.cover;
    paneImage($("albumPane"), source, isComp || state.editorMode === "fallback" ? "Choose a COMP track to preview its embedded image" : "Choose a cover candidate");
  }
  else {
    packageFrame($("currentPackage"), state.selectedBanner?.image || state.artist?.banner,
      state.selectedPortrait?.image || state.artist?.portrait);
    paneImage($("artistPane"), state.renderedPreview.artist?.image || state.selectedPortrait?.image || state.artist?.portrait, "Choose a blue artist candidate", true);
    paneImage($("bannerPane"), state.renderedPreview.banner?.image || state.selectedBanner?.image || state.artist?.banner, "Choose a pink banner candidate");
    $("currentRadio").classList.toggle("active", state.radioCurrent);
    $("artistRadio").classList.toggle("active", state.radioArtist);
    $("bannerRadio").classList.toggle("active", state.radioBanner);
  }
}
function selectedImage() {
  const frame = $("selectedFrame"); clear(frame);
  let candidate = null, url = null, type = "Current artwork", source = "Local", quality = "Current";
  const preparedComp = activeCompPrepared();
  const decision = activeArtworkDecision();
  const queuedComp = decision?.jobKind === "comp";
  if (preparedComp) {
    candidate = preparedComp;
    url = preparedComp.image;
    type = "Embedded Thumb";
    source = state.pendingCompWrite ? "Verified COMP write pending refresh" : "Prepared COMP thumbnail";
    quality = state.pendingCompWrite ? "Embedding…" : "Thumb Ready";
    if (url) frame.append(image(url, "selected-image", "Prepared COMP thumbnail"));
  } else if (decision?.selected) {
    candidate = decision.selected;
    const compCandidate = decision.jobKind === "comp";
    url = compCandidate ? state.renderedPreview.comp?.image || state.upscalePreview?.image || candidate.image : candidate.image;
    type = compCandidate ? "COMP Track Candidate" : "Queued Cover Candidate"; source = state.upscalePreview ? "Upscale preview" : candidate.source || "Queued review"; quality = state.upscalePreview ? "Upscale Preview" : "Preview";
    if (url) frame.append(image(url, "selected-image", candidate.name));
  } else if (state.editorMode === "album" && !state.reviewCleared) {
    candidate = state.selectedCover; url = state.renderedPreview.album?.image || state.upscalePreview?.image || candidate?.image || state.album?.cover;
    type = "Front Cover"; source = state.upscalePreview ? "Upscale preview" : candidate?.source || (url ? "Album folder" : "—");
    quality = state.upscalePreview ? "Upscale Preview" : candidate ? "Preview" : "Current";
    if (url) frame.append(image(url, "selected-image", state.album?.name + " cover"));
  } else if (state.editorMode === "fallback") {
    url = state.renderedPreview.comp?.image || state.upscalePreview?.image || state.fallbackTrack?.image;
    type = state.fallbackTrack ? "COMP Track" : "Current Cover";
    source = state.upscalePreview ? "Upscale preview" : state.fallbackTrack ? "Embedded track" : "Local";
    quality = state.upscalePreview ? "Upscale Preview" : "Current";
    if (url) frame.append(image(url, "selected-image", state.fallbackTrack?.file + " embedded artwork"));
  } else if (state.editorMode === "artist" && state.compReportVisible && state.compTrack) {
    url = state.renderedPreview.comp?.image || state.upscalePreview?.image || state.compTrack.image;
    type = "COMP Track"; source = state.upscalePreview ? "Upscale preview" : "Embedded track";
    quality = state.upscalePreview ? "Upscale Preview" : "Existing";
    if (url) frame.append(image(url, "selected-image", state.compTrack.file + " embedded artwork"));
  } else if (queuedComp) {
    type = "COMP Track"; source = "Loading selected track"; quality = "Loading";
  } else if ((state.workspace === "enhance" && state.radioCurrent) || (!state.radioCurrent && state.radioArtist && state.radioBanner)) {
    const banner = state.radioCurrent ? state.selectedBanner?.image || state.artist?.banner : state.renderedPreview.banner?.image || state.selectedBanner?.image || state.artist?.banner;
    const portrait = state.radioCurrent ? state.selectedPortrait?.image || state.artist?.portrait : state.renderedPreview.artist?.image || state.selectedPortrait?.image || state.artist?.portrait;
    const packageView = node("div", "selected-package");
    if (banner) packageView.append(image(banner, "banner-art", "Artist banner"));
    if (portrait) { const mask = node("span", "portrait-viewport"); mask.append(image(portrait, "portrait-art", "Artist portrait")); packageView.append(mask); }
    if (banner || portrait) frame.append(packageView);
    type = "Artist package"; source = state.radioCurrent ? "Current local art" : "Candidate preview"; quality = state.radioCurrent ? "Current" : "Preview";
  } else {
    const bannerOnly = state.radioBanner && !state.radioArtist;
    candidate = bannerOnly ? state.selectedBanner : state.selectedPortrait;
    url = (bannerOnly ? state.renderedPreview.banner?.image : state.renderedPreview.artist?.image)
      || candidate?.image || (bannerOnly ? state.artist?.banner : state.artist?.portrait);
    type = bannerOnly ? "Artist Banner" : "Artist Image"; source = candidate?.source || (url ? "Local" : "—"); quality = candidate ? "Preview" : "Current";
    if (url && bannerOnly) frame.append(image(url, "selected-image", type));
    else if (url) { const mask = node("span", "portrait-viewport"); mask.append(image(url, "selected-image portrait", type)); frame.append(mask); }
  }
  if (!frame.childElementCount && state.editorMode !== "fallback" && !(state.compReportVisible && state.compTrack))
    frame.append(image("/assets/set-your-imagination-tv.jpg", "", "Set your imagination now"));
  const existing = state.editorMode === "album" ? state.album?.cover_meta : state.editorMode === "fallback" ? state.fallbackTrack
    : state.editorMode === "artist" && state.compReportVisible && state.compTrack ? state.compTrack
    : state.radioBanner && !state.radioArtist ? state.artist?.banner_meta : state.artist?.portrait_meta;
  const rendered = decision?.jobKind === "comp" || state.editorMode === "fallback"
    || state.editorMode === "artist" && state.compReportVisible ? state.renderedPreview.comp
    : state.editorMode === "album" ? state.renderedPreview.album
    : state.radioBanner && !state.radioArtist ? state.renderedPreview.banner : state.renderedPreview.artist;
  const metadata = preparedComp || rendered || state.upscalePreview || candidate || existing;
  renderSelectedArtworkMetadata({
    source, type,
    dimensions: metadata ? metadata.width + " × " + metadata.height : "—",
    fileSize: metadata ? size(metadata.bytes) : "—",
    prepared: preparedComp, preview: state.upscalePreview, existing,
    candidate: preparedComp ? decision?.selected : candidate,
    originSource: state.upscalePreview ? "Enhancement" : decision?.selected?.source || (existing?.image ? "Embedded track" : source),
  });
  $("selectedQuality").textContent = quality;
  $("selectedQuality").classList.toggle("comp-thumb-ready", !!preparedComp && !state.pendingCompWrite);
  $("review").classList.toggle("comp-thumbnail-ready", !!preparedComp && !state.pendingCompWrite);
  const extra = $("reviewExtra"); clear(extra);
  const ribbon = $("embeddedRibbon");
  if (state.editorMode === "fallback") {
    if (state.fallbackTrack) extra.append(node("span", "", state.fallbackTrack.file), node("span", "", state.fallbackTrack.completed ? "Completed track" : "Pending track"));
    else extra.append(node("span", "", "Track(s) " + state.fallbackTracks.length), node("span", "", "Embedded Track(s) " + state.fallbackTracks.filter(item => item.completed).length));
    ribbon.hidden = true;
  } else if (state.editorMode === "artist" && state.compReportVisible && state.compTrack) {
    extra.append(node("span", "", state.compTrack.file), node("span", "", state.compTrack.completed ? "Recorded COMP artwork" : "Blue · artwork needed"));
    ribbon.hidden = true;
  } else if (state.editorMode === "album" && state.album) {
    extra.append(node("span", "", "Tracks " + state.album.tracks), node("span", "", "Embedded " + state.album.embedded + "/" + state.album.tracks));
    if (state.album.cover_meta) extra.append(node("span", "", "Existing cover " + state.album.cover_meta.width + " × " + state.album.cover_meta.height + " · " + size(state.album.cover_meta.bytes)));
    ribbon.hidden = !state.album.embedded; ribbon.textContent = "Embedded artwork on " + state.album.embedded + " of " + state.album.tracks + " tracks";
  } else {
    if (state.artist?.portrait_meta) extra.append(node("span", "", "Artist " + state.artist.portrait_meta.width + " × " + state.artist.portrait_meta.height + " · " + size(state.artist.portrait_meta.bytes)));
    if (state.artist?.banner_meta) extra.append(node("span", "", "Banner " + state.artist.banner_meta.width + " × " + state.artist.banner_meta.height + " · " + size(state.artist.banner_meta.bytes)));
    ribbon.hidden = true;
  }
  applyPreviewEffects();
}
function renderReview() {
  const artistEntry = state.editorMode === "artist" && state.workspace !== "enhance" && !state.compReportVisible && !state.selectedPortrait && !state.selectedBanner;
  const packageReady = state.editorMode === "artist" && state.workspace === "enhance" && state.radioArtist && state.radioBanner;
  if (!packageReady) state.themedPackaging = false;
  $("themedPackaging").disabled = !packageReady;
  $("themedPackaging").hidden = !packageReady;
  $("themedPackaging").classList.toggle("active", state.themedPackaging);
  $("themedPackaging").setAttribute("aria-pressed", String(state.themedPackaging));
  $("writeButton").textContent = artistEntry ? "Artist Art" : state.editorMode === "artist" && state.compReportVisible && state.compTrack ? "EMBED Comp"
    : state.editorMode === "artist" ? "WRITE Artist Art" : state.editorMode === "fallback" ? "EMBED Comp" : "WRITE Cover";
  $("writeButton").disabled = !(artistEntry && state.artist);
  $("upscaleButton").disabled = state.editorMode === "album" ? !(state.selectedCover?.image || state.album?.cover)
    : state.editorMode === "fallback" ? !state.fallbackTrack?.image
    : state.editorMode === "artist" && state.compReportVisible ? !state.compTrack?.image : false;
  $("embedBatchButton").disabled = !(state.editorMode === "album" && state.album && (state.selectedCover?.image || state.album.cover));
  const compContext = state.editorMode === "fallback" || state.editorMode === "artist" && state.compReportVisible;
  const embeddedSide = Number(state.compThumbnailPreview?.width || state.server?.embedded_thumbnail_size || 1200);
  $("writeNotice").hidden = compContext;
  $("writeNotice").textContent = compContext
    ? `COMP embed target · ${embeddedSide} × ${embeddedSide} · Upscale Preview prepares this thumbnail in memory; only EMBED Comp writes audio.`
    : "Artwork changes are saved only by WRITE or EMBED actions.";
  $("reviewChoice").hidden = state.editorMode !== "artist" || !(state.selectedPortrait || state.selectedBanner);
  $("useArtist").classList.toggle("active", !state.radioBanner || state.radioArtist);
  $("useBanner").classList.toggle("active", state.radioBanner);
  selectedImage();
  renderEnhancementMenus();
}
const FILTER_ROWS = ["Default", "Clarity", "Vivid Color", "Warm/Cool Color", "Low/Dark Light", "Soft/Sharper", "High/Low Contrast", "Portrait Mode", "Blur Background", "Custom A", "Custom B", "Custom C"];
const DETAIL_ROWS = ["Default", "Hue", "Tint", "Gamma", "Picture", "Exposure", "Brightness"];
function effectFilter(values = {}) {
  const v = name => Number(values[name] || 0);
  const brightness = Math.max(0.35, 1 + (v("Low/Dark Light") + v("Exposure") + v("Brightness") + v("Default") * .4 + v("Gamma") * .35) / 220);
  const contrast = Math.max(.35, 1 + (v("Clarity") + v("High/Low Contrast") + v("Picture") * .4 + v("Custom A") * .3) / 190);
  const saturation = Math.max(0, 1 + (v("Vivid Color") + v("Portrait Mode") * .25 + v("Custom B") * .4) / 120);
  const hue = v("Hue") * 1.8 + v("Warm/Cool Color") * .2;
  const sepia = Math.max(0, Math.min(.8, Math.abs(v("Tint")) / 140 + Math.max(0, v("Warm/Cool Color")) / 450));
  const blur = Math.max(0, (v("Blur Background") + Math.max(0, -v("Soft/Sharper"))) / 32);
  return `brightness(${brightness}) contrast(${contrast}) saturate(${saturation}) hue-rotate(${hue}deg) sepia(${sepia}) blur(${blur}px)`;
}
function transformFor(target) {
  const fit = state.fit[target];
  return fit ? `translate(${fit.x}%, ${fit.y}%) scale(${fit.zoom})` : "";
}
function fitZoom(value) {
  const position = Number(value);
  return position < 0 ? 1 + position / 200 : 1 + position * .07;
}
function fitSlider(zoom) {
  return Math.round(zoom < 1 ? (zoom - 1) * 200 : (zoom - 1) / .07);
}
function styleImage(element, target, additional = null) {
  if (!element) return;
  element.style.filter = effectFilter(state.fx[target]) + (additional ? " " + effectFilter(state.fx[additional]) : "");
  element.style.transform = transformFor(target);
}
function applyPreviewEffects() {
  styleImage($("artistPane").querySelector("img"), "artist");
  styleImage($("bannerPane").querySelector("img"), "banner");
  styleImage($("albumPane").querySelector("img"), "album");
  const frame = $("selectedFrame");
  if (state.editorMode === "artist" && state.compReportVisible) styleImage(frame.querySelector(".selected-image"), "comp");
  else if (state.editorMode === "album") styleImage(frame.querySelector(".selected-image"), "album");
  else if (state.radioCurrent) {
    for (const element of frame.querySelectorAll(".selected-package img")) styleImage(element, "current");
  } else if (state.radioArtist && state.radioBanner) {
    styleImage(frame.querySelector(".banner-art"), "banner", state.themedPackaging ? "package" : null);
    styleImage(frame.querySelector(".portrait-art"), "artist", state.themedPackaging ? "package" : null);
  } else styleImage(frame.querySelector(".selected-image"), state.radioBanner ? "banner" : "artist");
}
function sliderRow(host, label, target, field, minimum = -100, maximum = 100, initial = 0) {
  const row = node("label", "adjustment-row");
  const top = node("span", "adjustment-label"); top.append(node("strong", "", label), node("span", "adjustment-value", ""));
  const range = node("input"); range.type = "range"; range.min = String(minimum); range.max = String(maximum); range.value = String(field === "fit" ? fitSlider(state.fit[target]?.zoom || 1) : state.fx[target][label] ?? initial);
  const value = top.querySelector(".adjustment-value");
  const updateLabel = () => { value.textContent = field === "fit" ? fitZoom(range.value).toFixed(2) + "×" : (Number(range.value) > 0 ? "+" : "") + range.value; };
  updateLabel();
  range.addEventListener("input", () => {
    if (target === "comp") invalidateCompThumbnail();
    if (field === "fit") state.fit[target].zoom = fitZoom(range.value);
    else state.fx[target][label] = Number(range.value);
    updateLabel(); applyPreviewEffects();
  });
  range.addEventListener("dblclick", () => { range.value = String(initial); range.dispatchEvent(new Event("input")); });
  row.append(top, range); host.append(row);
}
function renderEnhancementMenus() {
  const target = activeFxTarget();
  const filters = $("filtersGroup"), detail = $("detailedGroup"), fitment = $("fitmentGroup");
  clear(filters); clear(detail); clear(fitment);
  if (!target) {
    for (const host of [filters, detail, fitment]) host.append(node("p", "menu-note", "Select one pane radio, or enable Themed Packaging for both."));
    return;
  }
  for (const label of FILTER_ROWS) sliderRow(filters, label, target, "fx");
  for (const label of DETAIL_ROWS) sliderRow(detail, label, target, "fx");
  if (target === "comp") { fitment.append(node("p", "menu-note", "COMP artwork is centered and prepared at the configured embedded-track size when EMBED Comp is pressed.")); return; }
  if (state.editorMode !== "artist") { fitment.append(node("p", "menu-note", "Artist and banner fitment is available in Artist Art.")); return; }
  const targets = target === "package" ? ["artist", "banner"] : target === "artist" || target === "banner" ? [target] : [];
  if (!targets.length) { fitment.append(node("p", "menu-note", "Select Artist Image or Artist Banner to adjust fitment.")); return; }
  fitment.append(node("p", "menu-note", "Wheel over the preview to zoom; drag the image to position."));
  for (const component of targets) sliderRow(fitment, component === "artist" ? "Artist" : "Banner", component, "fit", -100, 100, 0);
  const reset = node("button", "minor-button", "Reset fitment"); reset.type = "button"; reset.addEventListener("click", () => { for (const component of targets) state.fit[component] = {zoom: 1, x: 0, y: 0}; renderEnhancementMenus(); applyPreviewEffects(); }); fitment.append(reset);
}
function renderWorkspace() {
  SCROLL_SURFACES.forEach(rememberScroll);
  for (const name of ["find", "enhance", "embed"]) {
    $(name + "Tab").classList.toggle("active", state.workspace === name);
    $(name + "View").hidden = state.workspace !== name;
  }
  const decision = activeArtworkDecision();
  const musicBrainz = state.workspace === "find" && decision?.kind === "musicbrainz-results";
  $("candidateDock").hidden = state.workspace === "embed" || musicBrainz;
  $("review").hidden = state.workspace === "embed";
  document.querySelector(".workspace").classList.toggle("embed-active", state.workspace === "embed");
  document.querySelector(".workspace").classList.toggle("musicbrainz-active", musicBrainz);
  $("overviewSection").hidden = musicBrainz;
  $("musicbrainzReview").hidden = !musicBrainz;
  $("sourceHeading").textContent = musicBrainz ? "Find Releases Containing This Recording" : "Candidate Findings";
  $("sourceSubcopy").textContent = musicBrainz
    ? [decision?.context?.current_album?.artist || "Various Artists", decision?.context?.current_album?.title || state.album?.name || "COMP Album"].join(" › ")
    : "Search multiple sources to find the best artwork for your music.";
  if (musicBrainz) renderMusicBrainzWorkspace();
  renderPanes(); renderReview();
  requestAnimationFrame(() => { fitReviewToViewport(); updateBiographyToggle(); });
  restoreScroll(SCROLL_SURFACES);
}
function renderAll() {
  renderLibrary(); renderHero(); renderSources(); renderCandidates(); renderWorkspace();
}

async function loadLibrary(query = "") {
  const request = ++state.listRequest;
  state.libraryLoading = true;
  try {
    if (state.libraryMode === "artists") state.artists = await api("/api/artists?q=" + encodeURIComponent(query));
    else state.albumsIndex = await api("/api/albums?view=" + state.libraryMode + "&q=" + encodeURIComponent(query));
    if (request !== state.listRequest) return;
    state.libraryLoading = false;
    renderLibrary();
    if (state.libraryMode === "artists") hydrateArtistThumbnails(request);
    if (state.libraryMode === "artists" && !state.artist && state.artists.length) await chooseArtist(state.artists[0].id, false);
    else setStatus(state.libraryMode === "fallback" && !state.albumsIndex.length ? "No missing-ID albums in the library" : "Library ready · WebGUI ready");
  } catch (error) { if (request === state.listRequest) { state.libraryLoading = false; renderLibrary(); setStatus(error.message, "error"); } }
}
async function hydrateArtistThumbnails(request) {
  try {
    const thumbnails = await api("/api/artists/thumbnails");
    if (request !== state.listRequest || state.libraryMode !== "artists") return;
    let changed = false;
    for (const thumbnail of thumbnails) {
      const artist = state.artists.find(item => item.id === thumbnail.id);
      if (artist && thumbnail.portrait && artist.portrait !== thumbnail.portrait) {
        artist.portrait = thumbnail.portrait; changed = true;
      }
      if (state.artist?.id === thumbnail.id && thumbnail.portrait && state.artist.portrait !== thumbnail.portrait) {
        state.artist.portrait = thumbnail.portrait; changed = true;
      }
    }
    if (changed) { renderLibrary(); if (typeof renderHero === "function") renderHero(); }
  } catch { /* Local Artist Art remains available when Navidrome is offline. */ }
}
async function chooseArtist(id, select = true) {
  await cancelActiveCompWork();
  const request = ++state.selectionRequest;
  ++state.searchRequest; ++state.artistMetadataRequest;
  clearFocusCandidates(); resetFocusAdjustments();
  state.artist = null; state.artistMetadata = null; state.album = null; state.compAlbums = [];
  state.editorMode = "artist"; state.workspace = "find";
  renderAll();
  setStatus("Loading artist and existing artwork…", "busy");
  try {
    const overview = await api("/api/artist/overview?id=" + encodeURIComponent(id));
    if (request !== state.selectionRequest) return;
    const artist = overview.artist, compAlbums = overview.comp_albums;
    state.artist = artist; state.artistMetadata = null; state.compAlbums = compAlbums; state.compGroups = [];
    const listedArtist = state.artists.find(item => item.id === artist.id);
    if (listedArtist && artist.portrait) listedArtist.portrait = artist.portrait;
    state.album = null; state.editorMode = "artist"; state.workspace = "find";
    if (select) markSelectedArtist(artist);
    state.candidates = state.artist.local_candidates || []; resetCandidateFilters();
    state.radioCurrent = true; state.radioArtist = false; state.radioBanner = false;
    renderAll(); setStatus("Loaded " + state.artist.name + " · " + state.artist.albums.length + " albums"); loadArtistMetadata(artist.id);
  } catch (error) { if (request === state.selectionRequest) setStatus(error.message, "error"); }
}
async function chooseAlbum(id) {
  await cancelActiveCompWork();
  const request = ++state.selectionRequest;
  ++state.searchRequest;
  clearFocusCandidates(); resetFocusAdjustments();
  state.reviewCleared = false;
  state.album = null; state.editorMode = "album"; state.workspace = "find";
  renderSources(); renderCandidates(); renderWorkspace();
  setStatus("Loading album and existing cover…", "busy");
  try {
    const album = await api("/api/album?id=" + encodeURIComponent(id));
    if (request !== state.selectionRequest) return;
    // Featured, soundtrack, Various Artists, and COMP cards belong to the
    // currently displayed artist projection even when the physical album is
    // owned by another catalog artist.  Keep that artist context on click.
    if (!albumInCurrentArtistContext(id) && state.artist?.id !== album.artist_id) {
      const overview = await api("/api/artist/overview?id=" + encodeURIComponent(album.artist_id));
      if (request !== state.selectionRequest) return;
      state.artist = overview.artist; state.artistMetadata = null;
      state.compAlbums = overview.comp_albums || [];
      loadArtistMetadata(overview.artist.id);
    }
    const fallback = !String(album.album_mbid || "").trim();
    const tracks = fallback ? await api("/api/fallback/tracks?id=" + encodeURIComponent(id)) : [];
    if (request !== state.selectionRequest) return;
    state.album = album; state.editorMode = fallback ? "fallback" : "album"; state.workspace = "find";
    state.fallbackTracks = tracks;
    markSelectedAlbum(album);
    state.candidates = fallback ? [] : album.candidates || []; resetCandidateFilters();
    renderAll();
    setStatus(fallback ? `COMP ready · ${tracks.length} tracks · ${tracks.filter(item => !item.completed).length} pending · validating recorded thumbnails`
      : "Loaded " + album.artist_name + " · " + album.name + " · " + album.tracks + " tracks");
    if (fallback) probeFallbackStatuses();
  } catch (error) { if (request === state.selectionRequest) setStatus(error.message, "error"); }
}
async function selectFallbackTrack(key, previewOnly) {
  if (!state.album || state.editorMode !== "fallback") return;
  const albumId = state.album.id, request = ++state.fallbackTrackRequest;
  await cancelMismatchedCompWork({artist_id: "", album_id: albumId, track_key: key});
  if (request !== state.fallbackTrackRequest || state.album?.id !== albumId || state.editorMode !== "fallback") return;
  state.upscalePreview = null; state.renderedPreview.comp = null; state.compThumbnailPreview = null; state.pendingCompWrite = null; state.fx.comp = {};
  setStatus("Loading existing COMP track artwork…", "busy");
  try {
    const track = await api("/api/fallback/track?id=" + encodeURIComponent(albumId) + "&key=" + encodeURIComponent(key));
    if (request !== state.fallbackTrackRequest || state.album?.id !== albumId) return;
    state.fallbackTrack = track; state.fallbackPreviewOnly = !!previewOnly;
    renderLibrary(); renderHero(); renderCandidates(); renderPanes(); renderReview();
    setStatus((previewOnly ? "Existing preview" : "Selected COMP track") + " · " + (track.title || track.file) + (track.image ? " · embedded art found" : " · no embedded art"));
  } catch (error) { if (request === state.fallbackTrackRequest) setStatus(error.message, "error"); }
}
async function loadArtistMetadata(id, refresh = false) {
  const request = ++state.artistMetadataRequest;
  try {
    const metadata = await api("/api/artist/metadata?id=" + encodeURIComponent(id) + (refresh ? "&refresh=true" : ""));
    if (request !== state.artistMetadataRequest || state.artist?.id !== id) return;
    if (metadata.artist?.id === id) {
      state.artist = metadata.artist;
      const listed = (state.artists || []).find(item => item.id === id);
      if (listed && metadata.artist.portrait) listed.portrait = metadata.artist.portrait;
    }
    if (Array.isArray(metadata.comp_albums)) state.compAlbums = metadata.comp_albums;
    state.artistMetadata = metadata; renderLibrary(); renderHero(); renderSources();
  } catch { /* The labeled sample layout remains available when Navidrome is offline. */ }
}
async function searchArtist() {
  if (!state.artist) return;
  const id = state.artist.id;
  const request = ++state.searchRequest;
  state.sourceDiagnostics = [];
  $("searchButton").disabled = true;
  setStatus("Searching configured artist-image sources for " + state.artist.name + "…", "busy");
  try {
    const result = await api("/api/search/artist", {method: "POST", headers: {"Content-Type": "application/json"}, body: JSON.stringify({id})});
    if (request !== state.searchRequest || state.artist?.id !== id || state.editorMode !== "artist") return;
    state.candidates = [...(state.artist.local_candidates || []), ...result.candidates]; state.sourceDiagnostics = result.warnings || []; resetCandidateFilters();
    renderSources(); renderCandidates(); renderPanes(); renderReview();
    setStatus(result.candidates.length + " online candidates found" + (result.warnings.length ? " · " + result.warnings.length + " source warnings" : ""));
  } catch (error) {
    if (request !== state.searchRequest) return;
    state.candidates = state.artist?.local_candidates || []; resetCandidateFilters();
    renderSources(); renderCandidates();
    setStatus("Online artist search failed · local art remains available", "error");
  }
  finally { if (request === state.searchRequest) $("searchButton").disabled = !state.artist; }
}
function enterArtistArt() {
  if (!state.artist) return;
  ++state.selectionRequest;
  ++state.searchRequest;
  $("artistArtEntry").classList.add("flash");
  setTimeout(() => $("artistArtEntry").classList.remove("flash"), 350);
  state.editorMode = "artist";
  state.workspace = "enhance";
  clearFocusCandidates(); resetFocusAdjustments();
  state.radioCurrent = true; state.radioArtist = false; state.radioBanner = false; state.themedPackaging = false;
  renderAll();
  setStatus("Artist Art ready · edit the current package or press LAUNCH to search artist-image sources");
}

function showDialog(title, lines) {
  $("dialogTitle").textContent = title;
  const body = $("dialogBody"); clear(body);
  for (const line of lines) body.append(node("p", "", line));
  $("workspaceDialog").showModal();
}

$("artistSearch").addEventListener("input", event => { if (state.libraryMode === "artists") loadLibrary(event.target.value); });
$("biographyToggle").addEventListener("click", () => {
  state.biographyExpanded = !state.biographyExpanded;
  updateBiographyToggle();
});
window.addEventListener("resize", () => requestAnimationFrame(updateBiographyToggle));
$("albumSearch").addEventListener("input", event => { if (state.libraryMode === "artists") renderLibrary(); else loadLibrary(event.target.value); });
$("candidateSearch").addEventListener("input", renderCandidates);
$("clearCandidateFilter").addEventListener("click", () => { $("candidateSearch").value = ""; state.activeSource = ""; resetCandidateFilters(); renderSources(); renderCandidates(); });
for (const [buttonId, disposition] of [["candidateIncludeSource", "included"], ["candidateDisregardSource", "disregarded"]]) {
  $(buttonId).addEventListener("click", () => { if (!state.activeSource) return; state.candidateFilters.set(candidateFilterKey("source", state.activeSource), disposition); state.activeSource = ""; renderSources(); renderCandidates(); });
}
for (const [buttonId, mode] of [["selectAllMedia", "all"], ["selectFilteredMedia", "filtered"], ["selectNoMedia", "none"]]) {
  $(buttonId).addEventListener("click", () => {
    if (state.mediaFilterLocked) return;
    if (mode === "none") state.mediaSelected.clear();
    else {
      const items = state.libraryMode === "artists" ? (state.artist?.albums || []) : state.album ? [state.album] : [];
      if (!items.length) { setStatus("Choose an artist or album before selecting media", "error"); return; }
      if (mode === "filtered" && !$("artistSearch").value.trim() && !$("albumSearch").value.trim() && state.mediaStatuses.size === MEDIA_STATUSES.length && state.libraryMode !== "fallback") {
        setStatus("Select Filtered needs a search, status filter, or Fallback view", "error"); return;
      }
      state.mediaSelected.clear();
      for (const item of items.filter(mediaMatches)) state.mediaSelected.add(item.id);
      if (state.libraryMode === "artists" && state.mediaSelected.size) state.mediaSelected.add(state.artist.id);
    }
    renderLibrary(); setStatus("Selected " + state.mediaSelected.size + " library items for review");
  });
}
for (const [kind, id] of [["media", "mediaFilterLock"], ["candidate", "candidateFilterLock"]]) {
  $(id).addEventListener("click", () => {
    const key = kind === "media" ? "mediaFilterLocked" : "candidateFilterLocked";
    state[key] = !state[key]; $(id).textContent = state[key] ? "🔒" : "🔓"; $(id).setAttribute("aria-pressed", String(state[key]));
    for (const field of (kind === "media" ? ["artistSearch", "albumSearch", "selectAllMedia", "selectFilteredMedia", "selectNoMedia"] : ["candidateSearch", "clearCandidateFilter", "candidateIncludeSource", "candidateDisregardSource"])) $(field).disabled = state[key];
    renderMediaStatusChips(); renderCandidateTypeChips();
  });
}
for (const [buttonId, mode] of [["artistsTab", "artists"], ["albumsTab", "albums"], ["fallbackTab", "fallback"]]) {
  $(buttonId).addEventListener("click", async () => {
    if (state.libraryMode === mode) return;
    await cancelActiveCompWork();
    ++state.selectionRequest; ++state.searchRequest; ++state.artistMetadataRequest;
    state.libraryLoading = true;
    state.libraryMode = mode; state.artist = null; state.artistMetadata = null; state.album = null; state.compAlbums = []; state.mediaSelected.clear();
    state.editorMode = mode === "fallback" ? "fallback" : "artist"; state.workspace = "find";
    clearFocusCandidates(); resetFocusAdjustments(); renderAll();
    loadLibrary(mode === "artists" ? $("artistSearch").value : $("albumSearch").value);
  });
}
for (const [buttonId, name] of [["findTab", "find"], ["enhanceTab", "enhance"], ["embedTab", "embed"]]) {
  $(buttonId).addEventListener("click", () => { state.workspace = name; renderWorkspace(); if (name === "embed") renderQueue(); setStatus(name === "embed" ? "Persistent embedding queue" : name === "enhance" ? "Artwork enhancements" : "Find & Match Artwork ready"); });
}
$("searchButton").addEventListener("click", () => runAction(launchCandidateWork));
$("artistArtEntry").addEventListener("click", () => {
  if (state.editorMode === "fallback") {
    const key = state.fallbackTrack?.key || state.fallbackTracks[0]?.key;
    if (key) selectFallbackTrack(key, true);
  } else enterArtistArt();
});
$("refreshArtistInfo").addEventListener("click", () => state.artist && runAction(async () => { await loadArtistMetadata(state.artist.id, true); setStatus("Artist information refreshed · " + state.artist.name); }));
$("musicbrainzArtistButton").addEventListener("click", () => {
  if (state.artist?.mbid) window.open("https://musicbrainz.org/artist/" + encodeURIComponent(state.artist.mbid), "_blank", "noopener");
});
$("writeButton").addEventListener("click", () => runAction(writeArtwork));
$("themedPackaging").addEventListener("click", () => { state.themedPackaging = !state.themedPackaging; renderReview(); setStatus(state.themedPackaging ? "Themed Packaging adjustments active" : "Themed Packaging preview off"); });
$("compareButton").addEventListener("click", () => {
  const choices = state.candidates.filter(candidate => state.compare.has(candidate.image));
  showDialog("Candidate comparison", choices.map(candidate => candidate.source + " · " + candidate.name + " · " + candidate.width + " × " + candidate.height));
});
function upscalePreviewStatus(preview, prepared = null, compContext = false) {
  const result = "Upscale preview · " + (preview.reason || (preview.width + " × " + preview.height));
  if (compContext && prepared)
    return result + " · Embedded Thumb Ready · " + prepared.width + " × " + prepared.height + " · no audio written";
  return result + " · original artwork is unchanged until written";
}
$("upscaleButton").addEventListener("click", async () => {
  if (state.editorMode === "artist" && !state.compReportVisible) {
    state.workspace = "enhance"; renderWorkspace();
    await runAction(upscaleArtist);
    return;
  }
  const decision = activeArtworkDecision();
  const compContext = decision?.jobKind === "comp" || state.editorMode === "fallback" || state.editorMode === "artist" && state.compReportVisible;
  if (compContext) invalidateCompThumbnail();
  const source = decision?.jobKind === "comp" && decision.selected ? decision.selected.image
    : state.editorMode === "fallback" ? state.fallbackTrack?.image
    : state.editorMode === "artist" && state.compReportVisible ? state.compTrack?.image
    : state.selectedCover?.image || state.album?.cover;
  if (!source) { setStatus("Choose artwork to preview", "error"); return; }
  const request = state.selectionRequest, albumId = state.album?.id, trackKey = state.fallbackTrack?.key;
  const compTrackKey = state.compTrack?.key;
  const queuedCandidate = decision?.selected?.image || "";
  $("upscaleButton").disabled = true;
  setStatus("Preparing upscale preview…", "busy");
  try {
    const payload = compContext ? componentPayload("comp", source, {upscale: true}) : {image: source};
    const preview = await api("/api/preview/upscale", {method: "POST", headers: {"Content-Type": "application/json"}, body: JSON.stringify(payload)});
    if (request !== state.selectionRequest || albumId !== state.album?.id
      || (state.editorMode === "fallback" && trackKey !== state.fallbackTrack?.key)
      || (state.editorMode === "artist" && state.compReportVisible && compTrackKey !== state.compTrack?.key)
      || queuedCandidate !== (activeArtworkDecision()?.selected?.image || "")) return;
    state.upscalePreview = preview;
    const prepared = compContext ? await prepareCompThumbnail({announce: false}) : null;
    renderReview();
    setStatus(upscalePreviewStatus(preview, prepared, compContext), "ready");
  } catch (error) { if (request === state.selectionRequest) setStatus(error.message, "error"); }
  finally { if (request === state.selectionRequest) renderReview(); }
});
$("embedBatchButton").addEventListener("click", () => runAction(stageAlbum));
$("removeAllStaged").addEventListener("click", () => runAction(() => queueAction("clear", null, null, null, "embed-album")));
$("collapseLibrary").addEventListener("click", () => { const collapsed = document.querySelector(".workspace").classList.toggle("library-collapsed"); $("collapseLibrary").textContent = collapsed ? "+" : "−"; $("collapseLibrary").title = collapsed ? "Expand Media Library" : "Collapse Media Library"; fitReviewToViewport(); });
for (const [buttonId, panelId, otherId] of [["mediaFilterToggle", "mediaFilter", "candidateFilter"], ["candidateFilterToggle", "candidateFilter", "mediaFilter"]]) {
  $(buttonId).addEventListener("click", () => { const open = $(panelId).hidden; $(panelId).hidden = !open; $(otherId).hidden = true; for (const [id, panel] of [["mediaFilterToggle", "mediaFilter"], ["candidateFilterToggle", "candidateFilter"]]) { const active = !$(panel).hidden; $(id).classList.toggle("active", active); $(id).setAttribute("aria-expanded", String(active)); $(id).querySelector("b").textContent = active ? "−" : "+"; } });
}
for (const button of document.querySelectorAll(".group-toggle")) button.addEventListener("click", () => {
  const panel = $(button.dataset.group + "Group"), opening = panel.hidden;
  for (const other of document.querySelectorAll(".group-toggle")) { $(other.dataset.group + "Group").hidden = true; other.querySelector("b").textContent = "+"; }
  panel.hidden = !opening; button.querySelector("b").textContent = opening ? "−" : "+";
});
$("collapseEnhancements").addEventListener("click", () => {
  const collapsed = document.querySelector(".enhance-layout").classList.toggle("collapsed");
  $("collapseEnhancements").textContent = collapsed ? "+" : "−"; $("collapseEnhancements").setAttribute("aria-expanded", String(!collapsed));
  for (const button of document.querySelectorAll(".group-toggle")) { $(button.dataset.group + "Group").hidden = true; button.querySelector("b").textContent = "+"; }
});
$("resetEnhancements").addEventListener("click", () => {
  const target = activeFxTarget(); if (target) state.fx[target] = {};
  if (target === "comp") invalidateCompThumbnail();
  if (target === "artist" || target === "banner") state.fit[target] = {zoom: 1, x: 0, y: 0};
  if (target === "package") { state.fit.artist = {zoom: 1, x: 0, y: 0}; state.fit.banner = {zoom: 1, x: 0, y: 0}; }
  renderEnhancementMenus(); applyPreviewEffects();
  $("resetEnhancements").classList.add("flash"); setTimeout(() => $("resetEnhancements").classList.remove("flash"), 350);
  setStatus("Preview adjustments reset · current files unchanged");
});
$("currentRadio").addEventListener("click", () => { state.radioCurrent = true; state.radioArtist = false; state.radioBanner = false; renderPanes(); renderReview(); });
$("artistRadio").addEventListener("click", () => { state.radioArtist = !state.radioArtist; state.radioCurrent = !state.radioArtist && !state.radioBanner; renderPanes(); renderReview(); });
$("bannerRadio").addEventListener("click", () => { state.radioBanner = !state.radioBanner; state.radioCurrent = !state.radioArtist && !state.radioBanner; renderPanes(); renderReview(); });
$("useArtist").addEventListener("click", () => { state.radioArtist = true; state.radioBanner = false; state.radioCurrent = false; renderPanes(); renderReview(); });
$("useBanner").addEventListener("click", () => { state.radioBanner = true; state.radioArtist = false; state.radioCurrent = false; renderPanes(); renderReview(); });
$("showAllAlbums").addEventListener("click", () => { const expanded = $("albumGrid").classList.toggle("expanded"); $("showAllAlbums").textContent = expanded ? "Show Less" : "Show All"; rememberScroll("albumGrid"); });
$("albumGrid").addEventListener("wheel", event => {
  const grid = $("albumGrid");
  if (grid.classList.contains("expanded") || grid.scrollWidth <= grid.clientWidth) return;
  const delta = Math.abs(event.deltaY) >= Math.abs(event.deltaX) ? event.deltaY : event.deltaX;
  const next = Math.max(0, Math.min(grid.scrollLeft + delta, grid.scrollWidth - grid.clientWidth));
  if (next === grid.scrollLeft) return;
  event.preventDefault();
  grid.scrollLeft = next;
}, {passive: false});
function closeAlbumStatusKey() {
  $("albumStatusKey").hidden = true;
  $("albumStatusHelp").setAttribute("aria-expanded", "false");
}
$("albumStatusHelp").addEventListener("click", () => {
  const key = $("albumStatusKey"), opening = key.hidden; key.hidden = !opening;
  $("albumStatusHelp").setAttribute("aria-expanded", String(opening));
});
$("fallbackHelpToggle").addEventListener("click", () => {
  state.fallbackHelpExpanded = !state.fallbackHelpExpanded;
  renderHero();
});
document.addEventListener("keydown", event => { if (event.key === "Escape" && !$("albumStatusKey").hidden) closeAlbumStatusKey(); });
document.addEventListener("pointerdown", event => {
  if (!$("albumStatusKey").hidden && !event.target.closest("#albumStatusKey, #albumStatusHelp")) closeAlbumStatusKey();
});
$("compReportButton").addEventListener("click", () => {
  if (state.compReportVisible || state.compReportLoading) closeArtistCompReport();
  else openArtistCompReport();
});
$("themeButton").addEventListener("click", () => {
  showDialog("Theme", ["Choose the Windows inspired workspace palette. Your choice is kept in this browser."]);
  const options = node("div", "theme-options");
  for (const [key, label] of [["dark", "Dark"], ["chalk", "Chalk"], ["light", "Light"]]) {
    const button = node("button", "minor-button" + (state.theme === key ? " active" : ""), label); button.type = "button";
    button.addEventListener("click", () => { state.theme = key; document.documentElement.dataset.theme = key; try { localStorage.setItem("splined.browser-preview.theme", key); } catch {} $("workspaceDialog").close(); setStatus(label + " theme selected"); }); options.append(button);
  }
  $("dialogBody").append(options);
});
$("settingsButton").addEventListener("click", () => runAction(openSettings));
$("helpButton").addEventListener("click", () => {
  showDialog("SPLINED WebGUI", ["Use Settings to configure your library, PostgreSQL catalog, artwork policy, and provider credentials. Refresh the library index after adding music.", "LAUNCH searches the selected albums, or the current artist when no albums are selected. Review queued decisions in the browser. Artist Art opens portrait and banner search. WRITE saves prepared artwork; EMBED Batch saves the cover and stages track embedding without changing your current view.", "Live Write enables music and tag changes. Read mode supports searching and previewing. The queue and completion ledger persist in PostgreSQL."]);
  const link=node("a","guide-link","Open WebGUI guide");link.href="/guide";link.target="_blank";link.rel="noopener";$("dialogBody").append(link);
  const icon=node("a","guide-link","Save SPLINED dashboard icon");icon.href="/assets/splined-dashboard-icon.png";icon.download="splined-dashboard-icon.png";$("dialogBody").append(icon);
});
$("closeDialog").addEventListener("click", () => $("workspaceDialog").close());

const workspaceElement = document.querySelector(".workspace");
const layoutStorageKey = "splined.browser-preview.layout";
function clamp(value, minimum, maximum) { return Math.max(minimum, Math.min(value, Math.max(minimum, maximum))); }
function setDockSize(value) {
  workspaceElement.style.setProperty("--dock-height", value + "px");
  workspaceElement.style.setProperty("--candidate-card-width", clamp(Math.round(value * 0.69), 114, 240) + "px");
}
function availablePaneWidth() {
  const style = getComputedStyle(workspaceElement);
  return workspaceElement.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight) - 2 * parseFloat(style.columnGap);
}
function reviewWidthLimits() {
  return [250, availablePaneWidth() - $("libraryBody").parentElement.getBoundingClientRect().width - 380];
}
function fitReviewToViewport() {
  if (window.innerWidth <= 800 || workspaceElement.classList.contains("embed-active")) return;
  const width = $("review").getBoundingClientRect().width;
  const fitted = clamp(width, ...reviewWidthLimits());
  if (Math.abs(fitted - width) > 0.5) workspaceElement.style.setProperty("--review-width", fitted + "px");
}
function saveLayout() {
  try {
    const previous = JSON.parse(localStorage.getItem(layoutStorageKey) || "{}") || {};
    localStorage.setItem(layoutStorageKey, JSON.stringify({
      library: Math.round($("libraryBody").parentElement.getBoundingClientRect().width),
      review: $("review").hidden ? previous.review || 400 : Math.round($("review").getBoundingClientRect().width),
      candidates: $("candidateDock").hidden ? previous.candidates || 220 : Math.round($("candidateDock").getBoundingClientRect().height),
    }));
  } catch { /* The layout still works if browser storage is unavailable. */ }
}
function restoreLayout() {
  try {
    const values = JSON.parse(localStorage.getItem(layoutStorageKey) || "null");
    if (!values || typeof values !== "object") return;
    if (Number.isFinite(values.library)) workspaceElement.style.setProperty("--library-width", clamp(values.library, 200, Math.min(480, availablePaneWidth() - 250 - 380)) + "px");
    if (Number.isFinite(values.review)) workspaceElement.style.setProperty("--review-width", clamp(values.review, ...reviewWidthLimits()) + "px");
    if (Number.isFinite(values.candidates)) setDockSize(clamp(values.candidates, 160, Math.min(620, workspaceElement.clientHeight - 280)));
  } catch { /* Ignore invalid or unavailable saved layout. */ }
}
function installSplitter(id, axis, direction, measure, limit, apply) {
  const handle = $(id);
  let drag = null;
  handle.addEventListener("pointerdown", event => {
    if (event.button !== 0) return;
    drag = {coordinate: axis === "x" ? event.clientX : event.clientY, size: measure()};
    handle.setPointerCapture(event.pointerId);
    handle.classList.add("dragging");
    event.preventDefault();
  });
  handle.addEventListener("pointermove", event => {
    if (!drag) return;
    const coordinate = axis === "x" ? event.clientX : event.clientY;
    apply(clamp(drag.size + direction * (coordinate - drag.coordinate), ...limit()));
  });
  function finish() { if (!drag) return; drag = null; handle.classList.remove("dragging"); saveLayout(); }
  handle.addEventListener("pointerup", finish);
  handle.addEventListener("pointercancel", finish);
  handle.addEventListener("keydown", event => {
    const step = event.shiftKey ? 40 : 15;
    const delta = axis === "x" ? (event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0) : (event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0);
    if (!delta) return;
    apply(clamp(measure() + direction * delta * step, ...limit())); saveLayout(); event.preventDefault();
  });
}
restoreLayout();
installSplitter("librarySplitter", "x", 1,
  () => $("libraryBody").parentElement.getBoundingClientRect().width,
  () => [200, Math.min(480, availablePaneWidth() - $("review").getBoundingClientRect().width - 380)],
  value => workspaceElement.style.setProperty("--library-width", value + "px"));
installSplitter("reviewSplitter", "x", -1,
  () => $("review").getBoundingClientRect().width,
  reviewWidthLimits,
  value => workspaceElement.style.setProperty("--review-width", value + "px"));
installSplitter("candidateSplitter", "y", -1,
  () => $("candidateDock").getBoundingClientRect().height,
  () => [160, Math.min(620, workspaceElement.clientHeight - 280)],
  setDockSize);
window.addEventListener("resize", () => requestAnimationFrame(fitReviewToViewport));
$("candidateGrid").addEventListener("wheel", event => {
  if (Math.abs(event.deltaY) <= Math.abs(event.deltaX)) return;
  $("candidateGrid").scrollLeft += event.deltaY;
  event.preventDefault();
}, {passive: false});

function fitGestureTarget(host, event) {
  if (state.editorMode !== "artist" || state.radioCurrent || !event.target.closest("img")) return null;
  if (host.id === "artistPane") return state.radioArtist ? "artist" : null;
  if (host.id === "bannerPane") return state.radioBanner ? "banner" : null;
  if (state.radioArtist && state.radioBanner) {
    if (!state.themedPackaging) return null;
    return event.target.closest(".portrait-art") ? "artist" : "banner";
  }
  return state.radioBanner ? "banner" : "artist";
}
for (const id of ["selectedFrame", "artistPane", "bannerPane"]) {
  const host = $(id);
  let drag = null;
  host.addEventListener("dragstart", event => event.preventDefault());
  host.addEventListener("wheel", event => {
    const target = fitGestureTarget(host, event);
    if (!target) return;
    const old = state.fit[target].zoom;
    state.fit[target].zoom = clamp(old + (event.deltaY < 0 ? .1 : -.1) * Math.max(1, old / 2), .5, 8);
    renderEnhancementMenus(); applyPreviewEffects(); event.preventDefault();
  }, {passive: false});
  host.addEventListener("pointerdown", event => {
    const target = fitGestureTarget(host, event);
    if (!target || event.button !== 0) return;
    drag = {target, x: event.clientX, y: event.clientY, startX: state.fit[target].x, startY: state.fit[target].y, pointerId: event.pointerId};
    host.setPointerCapture(event.pointerId);
    event.preventDefault();
  });
  host.addEventListener("pointermove", event => {
    if (!drag || drag.pointerId !== event.pointerId) return;
    const box = host.getBoundingClientRect();
    state.fit[drag.target].x = clamp(drag.startX + (event.clientX - drag.x) / box.width * 100, -100, 100);
    state.fit[drag.target].y = clamp(drag.startY + (event.clientY - drag.y) / box.height * 100, -100, 100);
    applyPreviewEffects();
    event.preventDefault();
  });
  for (const end of ["pointerup", "pointercancel", "lostpointercapture"]) host.addEventListener(end, () => { drag = null; });
}
for (const id of SCROLL_SURFACES) $(id).addEventListener("scroll", () => rememberScroll(id), {passive: true});
try { const theme = localStorage.getItem("splined.browser-preview.theme"); if (["dark", "chalk", "light"].includes(theme)) { state.theme = theme; document.documentElement.dataset.theme = theme; } } catch {}
