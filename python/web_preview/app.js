"use strict";

const $ = id => document.getElementById(id);
const state = {
  artists: [], albumsIndex: [], artist: null, artistMetadata: null, album: null,
  libraryMode: "artists", editorMode: "artist", workspace: "find",
  candidates: [], selectedPortrait: null, selectedBanner: null, selectedCover: null,
  activeSource: "", radioCurrent: true, radioArtist: false, radioBanner: false, themedPackaging: false,
  mediaStatuses: new Set(["unprocessed", "incomplete", "processed", "bypassed", "partial", "artistComplete", "artistBypass"]),
  mediaSelected: new Set(), candidateFilters: new Map(), mediaFilterLocked: false, candidateFilterLocked: false,
  fx: {artist: {}, banner: {}, album: {}, current: {}, package: {}}, fit: {artist: {zoom: 1, x: 0, y: 0}, banner: {zoom: 1, x: 0, y: 0}},
  theme: "dark", compare: new Set(),
  embedQueue: [], listRequest: 0, selectionRequest: 0, searchRequest: 0,
};
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
  const element = node("img", className); element.src = src; element.alt = alt; element.loading = "lazy"; element.draggable = false; return element;
}
function clear(element) { element.replaceChildren(); }
function clearFocusCandidates() {
  state.candidates = [];
  state.selectedPortrait = null; state.selectedBanner = null; state.selectedCover = null;
  state.compare.clear();
  if (!state.candidateFilterLocked) { state.activeSource = ""; state.candidateFilters.clear(); $("candidateSearch").value = ""; }
}
function resetFocusAdjustments() {
  state.fx = {artist: {}, banner: {}, album: {}, current: {}, package: {}};
  state.fit = {artist: {zoom: 1, x: 0, y: 0}, banner: {zoom: 1, x: 0, y: 0}};
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
  return source.replace(/[^a-z]/g, "");
}
function candidateKind(candidate) {
  return String(candidate.kind || "").toLowerCase().includes("banner") ? "banner" : state.editorMode === "album" ? "cover" : "artist";
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
function markSelectedArtist(artist) {
  state.mediaSelected.clear();
  state.mediaSelected.add(artist.id);
  for (const album of artist.albums) {
    const status = String(album.status || "").toLowerCase();
    if (album.album_mbid && status !== "bypassed" && status !== "timeout") state.mediaSelected.add(album.id);
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
  if (state.editorMode === "album") return "album";
  if (state.radioCurrent) return "current";
  if (state.radioArtist && state.radioBanner) return state.themedPackaging ? "package" : null;
  return state.radioBanner ? "banner" : "artist";
}
function currentCandidate() {
  return state.editorMode === "album" ? state.selectedCover : (state.radioBanner && !state.radioArtist ? state.selectedBanner : state.selectedPortrait);
}

function renderLibrary() {
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
      const thumb = state.artist?.id === artist.id && state.artist.portrait ? image(state.artist.portrait, "library-thumb", "") : node("span", "library-thumb", artist.name.slice(0, 1).toUpperCase());
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
    for (const album of state.albumsIndex.filter(mediaMatches)) list.append(albumRow(album, false));
  }
  if (!list.childElementCount) list.append(node("p", "empty-copy", "No items match this filter."));
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

const ALBUM_BANNERS = {
  compiled: ["COMPILED", "#3478ff", "#d9ad43"],
  featuring: ["FEATURING", "#b8875d", "#008b8b"],
  soundtrack: ["SOUNDTRACK", "#4b0082", "#b49b00"],
  various: ["VARIOUS", "#654321", "#32dda0"],
  artist: ["ARTIST", "#006b3c", "#d9ad43"],
};
function albumBannerKind(album, artist) {
  if (!album.album_mbid) return "compiled";
  if (album.soundtrack) return "soundtrack";
  if (/^\[?various artists\]?$/i.test((artist?.name || "").trim())) return "various";
  return "artist";
}
function albumPresentationGroup(album, artist) {
  const kind = albumBannerKind(album, artist);
  if (kind === "compiled") return 5;
  if (kind === "soundtrack") return 2;
  if (kind === "various") return 3;
  return mediaStatusKey(album) === "unprocessed" ? 0 : 1;
}
function albumCardState(album, artist) {
  const status = mediaStatusKey(album);
  const missingCover = !album.cover_found || !album.cover;
  const missingArtist = !artist?.portrait;
  const unfinished = album.tracks > 0 && album.embedded < album.tracks;
  const dots = [];
  if (missingArtist) dots.push(["#f4cc4d", "Artist artwork missing"]);
  if (missingCover || status === "incomplete" || status === "unprocessed") dots.push(["#ff5268", missingCover ? "Album artwork missing" : "Album not complete"]);
  if (status === "bypassed" || status === "artistBypass") dots.push(["#f4cc4d", "Bypassed"]);
  if (status === "processed" && unfinished) dots.push(["#efa54e", "Processed; track artwork review pending"]);
  if (status === "artistComplete" && !unfinished && !missingArtist && !missingCover) dots.push(["#32dda0", "Artwork complete"]);
  if (album.webp_found) dots.push(["#3478ff", "cover.webp present"]);
  // The isolated index does not store Windows' live embedded thumbnail check.
  const frame = status === "artistComplete" && !unfinished && !missingArtist && !missingCover ? "#32dda0"
    : status === "processed" ? "#efa54e" : status === "partial" ? "#7c4dff" : "#ff5268";
  return {dots, frame};
}
function renderAlbumStatusKey(albums, artist) {
  const host = $("albumStatusKey"); clear(host);
  host.append(node("h3", "", "Album Status Key"));
  const columns = node("div", "album-key-columns");
  const sections = [
    ["Status dots", [["Red", "Missing art or unfinished album", "#ff5268"], ["Yellow", "Artist art missing or bypassed", "#f4cc4d"], ["Orange", "Processed; review still pending", "#efa54e"], ["Green", "Artist and album art complete", "#32dda0"], ["Purple", "Embedded thumbnail needs attention (Windows validation)", "#7c4dff"], ["Blue", "cover.webp present", "#3478ff"]]],
    ["Album card frames", [["Green", "Complete", "#32dda0"], ["Orange", "Processed", "#efa54e"], ["Purple", "Partial or error", "#7c4dff"], ["Red", "Missing or incomplete", "#ff5268"]]],
    ["Banners", Object.entries(ALBUM_BANNERS).map(([key, [label, , ink]]) => [label, key === "compiled" ? "No release ID" : key === "artist" ? "Artist release" : key === "various" ? "Various Artists release" : key === "soundtrack" ? "Soundtrack release" : "Featured artist", ink])],
  ];
  for (const [title, rows] of sections) {
    const column = node("div", "album-key-column"); column.append(node("h4", "", title));
    for (const [label, detail, color] of rows) {
      const row = node("div", "album-key-row");
      const swatch = node("span", "album-key-swatch", "●"); swatch.style.color = color;
      const count = title === "Banners" ? albums.filter(album => albumBannerKind(album, artist) === label.toLowerCase()).length
        : title === "Album card frames" ? albums.filter(album => albumCardState(album, artist).frame.toLowerCase() === color).length
        : albums.filter(album => albumCardState(album, artist).dots.some(([dot]) => dot.toLowerCase() === color)).length;
      row.append(swatch, node("strong", "", label), node("small", "", detail), node("b", "", String(count))); column.append(row);
    }
    columns.append(column);
  }
  host.append(columns, node("p", "album-key-note", "Counts use this test index. Embedded thumbnail size validation and Featured credit classification are not indexed here."));
}

function renderHero() {
  const artist = state.artist;
  const metadata = state.artistMetadata;
  $("artistName").textContent = artist?.name || "Select an artist";
  $("overviewArtist").textContent = artist?.name || "";
  $("artistSummary").textContent = artist ? artist.albums.length + " albums · " + (artist.status || "indexed") : "Browse your music library to begin.";
  $("artistBiography").textContent = !artist ? "" : metadata?.biography ? metadata.biography : "No Navidrome biography available for " + artist.name + ".";
  $("similarSource").textContent = metadata?.similar?.length ? "· Navidrome" : "· sample layout";
  const similar = $("similarArtists"); clear(similar);
  const names = !artist ? [] : metadata?.similar?.length ? metadata.similar : state.artists.filter(item => item.id !== artist.id).slice(0, 6).map(item => item.name);
  for (const name of names) {
    const card = node("span", "similar-card");
    card.append(node("span", "similar-avatar", name.slice(0, 1).toUpperCase()), node("span", "similar-name", name), node("span", "similar-status", "●"));
    similar.append(card);
  }
  const facts = $("albumOverviewFacts"); clear(facts);
  if (state.album) {
    const indicator = node("span", "status-indicator", "● " + (state.album.status || "unprocessed"));
    applyStatusPalette(indicator, mediaStatusKey(state.album));
    facts.append(node("strong", "", state.album.name), indicator, node("span", "", state.album.tracks + " tracks · " + state.album.embedded + " embedded"));
  }
  const songs = state.album ? state.album.track_titles || [] : metadata?.top_songs?.length ? metadata.top_songs : artist?.top_tracks || [];
  $("topSongs").textContent = state.album && !songs.length ? "Album track titles unavailable in this test index" : songs.length ? (state.album ? "Album tracks: " : metadata?.top_songs?.length ? "Navidrome top songs: " : "Indexed songs: ") + songs.slice(0, 10).map((song, index) => (index + 1) + ". " + song).join("  ·  ") : "";
  $("hero").style.backgroundImage = artist?.banner ? `url("${artist.banner}")` : artist?.portrait ? `url("${artist.portrait}")` : "none";
  const portrait = $("heroPortrait");
  const replacement = artist?.portrait ? image(artist.portrait, "hero-portrait", artist.name + " artist image") : node("div", "hero-portrait placeholder", "♪");
  replacement.id = "heroPortrait"; portrait.replaceWith(replacement);
  $("artistArtEntry").disabled = !artist;
  $("artistArtEntry").classList.toggle("active", !!artist && state.editorMode === "artist");
  const grid = $("albumGrid"); clear(grid);
  const albums = [...(artist?.albums || [])].sort((a, b) =>
    albumPresentationGroup(a, artist) - albumPresentationGroup(b, artist)
    || (parseInt(b.year, 10) || 0) - (parseInt(a.year, 10) || 0)
    || a.name.localeCompare(b.name));
  $("albumCount").textContent = albums.length + " shown";
  renderAlbumStatusKey(albums, artist);
  if (!albums.length) { grid.append(node("p", "empty-copy", "Choose an artist to view albums.")); return; }
  for (const album of albums) {
    const button = node("button", "album-card windows-card"); button.type = "button";
    const cardState = albumCardState(album, artist); button.style.setProperty("--frame-color", cardState.frame);
    button.setAttribute("aria-label", album.name + ", " + (album.year || "year unknown") + ", " + (album.status || "unprocessed"));
    const kind = albumBannerKind(album, artist), [banner, background, ink] = ALBUM_BANNERS[kind];
    const bannerNode = node("span", "album-type-banner", banner); bannerNode.style.setProperty("--banner-bg", background); bannerNode.style.setProperty("--banner-ink", ink);
    const body = node("span", "album-card-body");
    const picture = node("span", "album-card-picture"); picture.append(album.cover ? image(album.cover, "", album.name + " cover") : node("span", "album-placeholder", "♫"));
    const dots = node("span", "album-status-dots"); for (const [color, tip] of cardState.dots) { const dot = node("span", "", "●"); dot.style.color = color; dot.title = tip; dots.append(dot); }
    picture.append(dots);
    if (state.embedQueue.some(item => item.id === album.id)) { const staged = node("span", "album-staged-indicator", "●"); staged.title = "Staged for embedding"; picture.append(staged); }
    body.append(picture, node("strong", "", album.name), node("span", "album-card-foot", (album.year || "—") + "   ☆☆☆☆☆"));
    button.append(bannerNode, body);
    button.addEventListener("click", () => chooseAlbum(album.id)); grid.append(button);
  }
}

function renderSources() {
  const strip = $("sourceStrip"); clear(strip);
  for (const [label, key, icon] of SOURCES) {
    const count = state.candidates.filter(candidate => sourceKey(candidate.source) === key).length;
    const navidromeMetadata = key === "navidrome" && state.artistMetadata?.source === "Navidrome";
    const searchable = ["local", "fanart", "deezer"].includes(key) || navidromeMetadata;
    const button = node("button", "source-card" + (state.activeSource === key && key ? " active" : "")); button.type = "button";
    button.title = count ? "Show " + label + " candidates" : label + (searchable ? " · ready for artist search" : " · artist search connection pending");
    button.append(icon === "⊘" ? node("span", "source-symbol", "⊘") : image("/assets/source-icons/" + icon + ".svg", "", ""));
    button.append(node("strong", "", label), node("small", "", count + " found"), node("span", "source-state" + (searchable ? "" : " pending"), count ? "● Results" : navidromeMetadata ? "● Metadata" : searchable ? "● Ready" : "○ Pending"));
    button.addEventListener("click", () => { state.activeSource = state.activeSource === key ? "" : key; renderSources(); renderCandidates(); }); strip.append(button);
  }
}
function renderCandidates() {
  const grid = $("candidateGrid"); clear(grid);
  const query = $("candidateSearch").value.trim().toLowerCase();
  const visible = state.candidates.filter(candidate => candidateVisible(candidate) && (!query || (candidate.name + " " + candidate.source).toLowerCase().includes(query)));
  $("candidateCount").textContent = state.candidates.length + (state.candidates.length === 1 ? " candidate" : " candidates");
  $("compareButton").disabled = state.compare.size < 2;
  $("candidateHint").textContent = state.editorMode === "album" ? "Select a cover candidate to preview" : "Blue: artist · pink: banner · wheel to browse";
  renderCandidateTypeChips();
  if (!visible.length) { grid.append(node("p", "empty-copy", state.candidates.length ? "No candidates match the filter." : "Press LAUNCH to search artist art, or select an album to inspect folder art.")); return; }
  for (const candidate of visible) {
    const kind = candidateKind(candidate);
    const active = state.selectedCover === candidate || state.selectedPortrait === candidate || state.selectedBanner === candidate;
    const card = node("div", "candidate-card" + (kind === "banner" ? " banner" : "") + (active ? " active" : ""));
    const picture = node("div", "candidate-picture");
    const main = node("button", "candidate-image-button"); main.type = "button"; main.setAttribute("aria-label", "Preview " + candidate.name);
    main.append(image(candidate.image, "", candidate.name), node("span", "candidate-source-label", candidate.source));
    main.addEventListener("click", () => selectCandidate(candidate, kind === "banner" ? "banner" : "artist")); picture.append(main);
    if (state.editorMode === "artist") {
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
    const compare = node("button", "candidate-compare" + (state.compare.has(candidate.image) ? " active" : ""), state.compare.has(candidate.image) ? "Compare ●" : "Compare ○"); compare.type = "button";
    compare.addEventListener("click", () => { state.compare.has(candidate.image) ? state.compare.delete(candidate.image) : state.compare.add(candidate.image); renderCandidates(); });
    line.append(compare); card.append(line, node("small", "", candidate.width + " × " + candidate.height)); grid.append(card);
  }
}
function selectCandidate(candidate, target) {
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
function renderPanes() {
  const isAlbum = state.editorMode === "album";
  $("paneGrid").classList.toggle("album-layout", isAlbum);
  $("albumEnhancePane").hidden = !isAlbum;
  $("enhanceTab").textContent = isAlbum ? "Album Art Enhancements" : "Artists Art Enhancements";
  if (isAlbum) paneImage($("albumPane"), state.selectedCover?.image || state.album?.cover, "Choose a cover candidate");
  else {
    packageFrame($("currentPackage"), state.artist?.banner, state.artist?.portrait);
    paneImage($("artistPane"), state.selectedPortrait?.image || state.artist?.portrait, "Choose a blue artist candidate", true);
    paneImage($("bannerPane"), state.selectedBanner?.image || state.artist?.banner, "Choose a pink banner candidate");
    $("currentRadio").classList.toggle("active", state.radioCurrent);
    $("artistRadio").classList.toggle("active", state.radioArtist);
    $("bannerRadio").classList.toggle("active", state.radioBanner);
  }
}
function selectedImage() {
  const frame = $("selectedFrame"); clear(frame);
  let candidate = null, url = null, type = "Current artwork", source = "Local", quality = "Current";
  if (state.editorMode === "album") {
    candidate = state.selectedCover; url = candidate?.image || state.album?.cover;
    type = "Front Cover"; source = candidate?.source || (url ? "Album folder" : "—"); quality = candidate ? "Preview" : "Current";
    if (url) frame.append(image(url, "selected-image", state.album?.name + " cover"));
  } else if ((state.workspace === "enhance" && state.radioCurrent) || (!state.radioCurrent && state.radioArtist && state.radioBanner)) {
    const banner = state.radioCurrent ? state.artist?.banner : state.selectedBanner?.image || state.artist?.banner;
    const portrait = state.radioCurrent ? state.artist?.portrait : state.selectedPortrait?.image || state.artist?.portrait;
    const packageView = node("div", "selected-package");
    if (banner) packageView.append(image(banner, "banner-art", "Artist banner"));
    if (portrait) { const mask = node("span", "portrait-viewport"); mask.append(image(portrait, "portrait-art", "Artist portrait")); packageView.append(mask); }
    if (banner || portrait) frame.append(packageView);
    type = "Artist package"; source = state.radioCurrent ? "Current local art" : "Candidate preview"; quality = state.radioCurrent ? "Current" : "Preview";
  } else {
    const bannerOnly = state.radioBanner && !state.radioArtist;
    candidate = bannerOnly ? state.selectedBanner : state.selectedPortrait;
    url = candidate?.image || (bannerOnly ? state.artist?.banner : state.artist?.portrait);
    type = bannerOnly ? "Artist Banner" : "Artist Image"; source = candidate?.source || (url ? "Local" : "—"); quality = candidate ? "Preview" : "Current";
    if (url && bannerOnly) frame.append(image(url, "selected-image", type));
    else if (url) { const mask = node("span", "portrait-viewport"); mask.append(image(url, "selected-image portrait", type)); frame.append(mask); }
  }
  if (!frame.childElementCount) frame.append(image("/assets/set-your-imagination-tv.jpg", "", "Set your imagination now"));
  $("selectedType").textContent = type; $("selectedSource").textContent = source;
  const existing = state.editorMode === "album" ? state.album?.cover_meta : state.radioBanner && !state.radioArtist ? state.artist?.banner_meta : state.artist?.portrait_meta;
  const metadata = candidate || existing;
  $("selectedDimensions").textContent = metadata ? metadata.width + " × " + metadata.height : "—";
  $("selectedSize").textContent = metadata ? size(metadata.bytes) : "—";
  $("selectedQuality").textContent = quality;
  const extra = $("reviewExtra"); clear(extra);
  const ribbon = $("embeddedRibbon");
  if (state.editorMode === "album" && state.album) {
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
  const artistEntry = state.editorMode === "artist" && !state.selectedPortrait && !state.selectedBanner;
  const packageReady = state.editorMode === "artist" && state.workspace === "enhance" && state.radioArtist && state.radioBanner;
  if (!packageReady) state.themedPackaging = false;
  $("themedPackaging").disabled = !packageReady;
  $("themedPackaging").hidden = !packageReady;
  $("themedPackaging").classList.toggle("active", state.themedPackaging);
  $("themedPackaging").setAttribute("aria-pressed", String(state.themedPackaging));
  $("writeButton").textContent = artistEntry ? "Artist Art" : state.editorMode === "artist" ? "WRITE Artist Art" : "WRITE Cover";
  $("writeButton").disabled = !artistEntry || !state.artist;
  $("embedBatchButton").disabled = !(state.editorMode === "album" && state.album && (state.selectedCover?.image || state.album.cover));
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
  if (state.editorMode === "album") styleImage(frame.querySelector(".selected-image"), "album");
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
  if (state.editorMode !== "artist") { fitment.append(node("p", "menu-note", "Artist and banner fitment is available in Artist Art.")); return; }
  const targets = target === "package" ? ["artist", "banner"] : target === "artist" || target === "banner" ? [target] : [];
  if (!targets.length) { fitment.append(node("p", "menu-note", "Select Artist Image or Artist Banner to adjust fitment.")); return; }
  fitment.append(node("p", "menu-note", "Wheel over the preview to zoom; drag the image to position."));
  for (const component of targets) sliderRow(fitment, component === "artist" ? "Artist" : "Banner", component, "fit", -100, 100, 0);
  const reset = node("button", "minor-button", "Reset fitment"); reset.type = "button"; reset.addEventListener("click", () => { for (const component of targets) state.fit[component] = {zoom: 1, x: 0, y: 0}; renderEnhancementMenus(); applyPreviewEffects(); }); fitment.append(reset);
}
function renderWorkspace() {
  for (const name of ["find", "enhance", "embed"]) {
    $(name + "Tab").classList.toggle("active", state.workspace === name);
    $(name + "View").hidden = state.workspace !== name;
  }
  $("candidateDock").hidden = state.workspace === "embed";
  $("review").hidden = state.workspace === "embed";
  document.querySelector(".workspace").classList.toggle("embed-active", state.workspace === "embed");
  renderPanes(); renderReview();
}
function renderAll() { renderLibrary(); renderHero(); renderSources(); renderCandidates(); renderWorkspace(); $("searchButton").disabled = !state.artist && !state.album; }

async function loadLibrary(query = "") {
  const request = ++state.listRequest;
  try {
    if (state.libraryMode === "artists") state.artists = await api("/api/artists?q=" + encodeURIComponent(query));
    else state.albumsIndex = await api("/api/albums?view=" + state.libraryMode + "&q=" + encodeURIComponent(query));
    if (request !== state.listRequest) return;
    renderLibrary();
    if (state.libraryMode === "artists" && !state.artist && state.artists.length) await chooseArtist(state.artists[0].id, false);
    else setStatus(state.libraryMode === "fallback" && !state.albumsIndex.length ? "No missing-ID albums in this test index" : "Library ready · browser layout preview");
  } catch (error) { if (request === state.listRequest) setStatus(error.message, "error"); }
}
async function chooseArtist(id, select = true) {
  const request = ++state.selectionRequest;
  ++state.searchRequest;
  clearFocusCandidates(); resetFocusAdjustments();
  state.artist = null; state.artistMetadata = null; state.album = null;
  state.editorMode = "artist"; state.workspace = "find";
  renderAll();
  setStatus("Loading artist and existing artwork…", "busy");
  try {
    const artist = await api("/api/artist?id=" + encodeURIComponent(id));
    if (request !== state.selectionRequest) return;
    state.artist = artist; state.artistMetadata = null;
    state.album = null; state.editorMode = "artist"; state.workspace = "find";
    if (select) markSelectedArtist(artist);
    state.candidates = state.artist.local_candidates || []; resetCandidateFilters();
    state.radioCurrent = true; state.radioArtist = false; state.radioBanner = false;
    renderAll(); setStatus("Loaded " + state.artist.name + " · " + state.artist.albums.length + " albums"); loadArtistMetadata(id);
  } catch (error) { if (request === state.selectionRequest) setStatus(error.message, "error"); }
}
async function chooseAlbum(id) {
  const request = ++state.selectionRequest;
  ++state.searchRequest;
  clearFocusCandidates(); resetFocusAdjustments();
  state.album = null; state.editorMode = "album"; state.workspace = "find";
  renderSources(); renderCandidates(); renderWorkspace();
  setStatus("Loading album and existing cover…", "busy");
  try {
    const album = await api("/api/album?id=" + encodeURIComponent(id));
    if (request !== state.selectionRequest) return;
    if (state.artist?.id !== album.artist_id) {
      const artist = await api("/api/artist?id=" + encodeURIComponent(album.artist_id));
      if (request !== state.selectionRequest) return;
      state.artist = artist; state.artistMetadata = null; loadArtistMetadata(album.artist_id);
    }
    state.album = album; state.editorMode = "album"; state.workspace = "find";
    markSelectedAlbum(album);
    state.candidates = album.candidates || []; resetCandidateFilters();
    renderAll(); setStatus("Loaded " + album.artist_name + " · " + album.name + " · " + album.tracks + " tracks");
  } catch (error) { if (request === state.selectionRequest) setStatus(error.message, "error"); }
}
async function loadArtistMetadata(id) {
  try {
    const metadata = await api("/api/artist/metadata?id=" + encodeURIComponent(id));
    if (state.artist?.id !== id) return;
    state.artistMetadata = metadata; renderHero(); renderSources();
  } catch { /* The labeled sample layout remains available when Navidrome is offline. */ }
}
async function searchArtist() {
  if (!state.artist) return;
  const id = state.artist.id;
  const request = ++state.searchRequest;
  $("searchButton").disabled = true;
  setStatus("Searching Fanart.tv and Deezer for " + state.artist.name + "…", "busy");
  try {
    const result = await api("/api/search/artist", {method: "POST", headers: {"Content-Type": "application/json"}, body: JSON.stringify({id})});
    if (request !== state.searchRequest || state.artist?.id !== id || state.editorMode !== "artist") return;
    state.candidates = [...(state.artist.local_candidates || []), ...result.candidates]; resetCandidateFilters();
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
  searchArtist();
}
function renderQueue() {
  const queue = $("embedQueue"); clear(queue);
  if (!state.embedQueue.length) queue.append(node("p", "empty-copy", "No albums staged yet."));
  for (const item of state.embedQueue) {
    const card = node("div", "queue-card");
    card.append(item.cover ? image(item.cover, "", item.name + " cover") : node("span", "library-thumb", "♫"));
    const copy = node("span", "row-copy"); copy.append(node("strong", "", item.artist + " — " + item.name), node("small", "", "Preview staged · " + item.tracks + " tracks · no tags changed")); card.append(copy);
    const remove = node("button", "", "Remove"); remove.type = "button"; remove.addEventListener("click", () => { state.embedQueue = state.embedQueue.filter(entry => entry.id !== item.id); renderQueue(); renderHero(); setStatus("Removed album from preview queue"); }); card.append(remove); queue.append(card);
  }
  $("removeAllStaged").disabled = !state.embedQueue.length;
}
function stageAlbum() {
  if (!state.album) return;
  if (!state.embedQueue.some(item => item.id === state.album.id)) state.embedQueue.push({id: state.album.id, name: state.album.name, artist: state.album.artist_name, cover: state.selectedCover?.image || state.album.cover, tracks: state.album.tracks});
  state.workspace = "embed"; renderQueue(); renderHero(); renderWorkspace(); setStatus("Preview staged: " + state.album.name + " · no tracks changed");
}

function showDialog(title, lines) {
  $("dialogTitle").textContent = title;
  const body = $("dialogBody"); clear(body);
  for (const line of lines) body.append(node("p", "", line));
  $("workspaceDialog").showModal();
}

$("artistSearch").addEventListener("input", event => { if (state.libraryMode === "artists") loadLibrary(event.target.value); });
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
    renderLibrary(); setStatus("Selected " + state.mediaSelected.size + " library items for review · scan queue not connected");
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
  $(buttonId).addEventListener("click", () => {
    if (state.libraryMode === mode) return;
    ++state.selectionRequest; ++state.searchRequest;
    state.libraryMode = mode; state.artist = null; state.artistMetadata = null; state.album = null; state.mediaSelected.clear();
    state.editorMode = "artist"; state.workspace = "find";
    clearFocusCandidates(); resetFocusAdjustments(); renderAll();
    loadLibrary(mode === "artists" ? $("artistSearch").value : $("albumSearch").value);
  });
}
for (const [buttonId, name] of [["findTab", "find"], ["enhanceTab", "enhance"], ["embedTab", "embed"]]) {
  $(buttonId).addEventListener("click", () => { state.workspace = name; renderWorkspace(); if (name === "embed") renderQueue(); setStatus(name === "embed" ? "Embedding queue preview · no tags changed" : name === "enhance" ? "Enhancement layout preview · no image processing" : "Find & Match Artwork ready"); });
}
$("searchButton").addEventListener("click", () => state.editorMode === "artist" ? searchArtist() : (renderCandidates(), setStatus("Showing images in the selected album folder")));
$("artistArtEntry").addEventListener("click", enterArtistArt);
$("writeButton").addEventListener("click", enterArtistArt);
$("themedPackaging").addEventListener("click", () => { state.themedPackaging = !state.themedPackaging; renderReview(); setStatus(state.themedPackaging ? "Themed Packaging preview active · processing not connected" : "Themed Packaging preview off"); });
$("compareButton").addEventListener("click", () => {
  const choices = state.candidates.filter(candidate => state.compare.has(candidate.image));
  showDialog("Candidate comparison", choices.map(candidate => candidate.source + " · " + candidate.name + " · " + candidate.width + " × " + candidate.height));
});
$("upscaleButton").addEventListener("click", () => { state.workspace = "enhance"; renderWorkspace(); setStatus("Enhancement layout preview · no upscale processing"); });
$("embedBatchButton").addEventListener("click", stageAlbum);
$("removeAllStaged").addEventListener("click", () => { state.embedQueue = []; renderQueue(); renderHero(); setStatus("Preview queue cleared"); });
$("collapseLibrary").addEventListener("click", () => { const collapsed = document.querySelector(".workspace").classList.toggle("library-collapsed"); $("collapseLibrary").textContent = collapsed ? "+" : "−"; $("collapseLibrary").title = collapsed ? "Expand Media Library" : "Collapse Media Library"; });
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
$("showAllAlbums").addEventListener("click", () => { const expanded = $("albumGrid").classList.toggle("expanded"); $("showAllAlbums").textContent = expanded ? "Show Less" : "Show All"; });
$("albumStatusHelp").addEventListener("click", () => {
  const key = $("albumStatusKey"), opening = key.hidden; key.hidden = !opening;
  $("albumStatusHelp").setAttribute("aria-expanded", String(opening));
});
$("compReportButton").addEventListener("click", () => {
  const albums = (state.artist?.albums || []).filter(album => album.compilation);
  showDialog("COMP Report · " + (state.artist?.name || "No artist selected"), albums.length ? albums.map(album => album.name + " · " + album.tracks + " tracks · " + album.status) : ["No compilation albums are indexed for this artist."]);
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
$("settingsButton").addEventListener("click", async () => {
  try {
    const status = await api("/api/status");
    showDialog("Settings · browser review", ["Mode: " + status.mode, "Index: " + status.database, "Music: " + status.music, "Artist images: " + status.artist_art, "Artist search: Fanart.tv and Deezer; other Windows providers are pending browser integration.", "All artwork edits in this preview stay in memory. PostgreSQL and writes are not connected."]);
  } catch (error) { showDialog("Settings", [error.message]); }
});
$("helpButton").addEventListener("click", () => {
  showDialog("About SPLINED WebUI", ["Use the mouse to choose an artist, album, candidate image, or banner. Drag pane dividers to resize; wheel over Candidate Artwork to browse horizontally.", "Filters, Fitment, and Detailed adjust only the in-memory browser preview. WRITE, Bypass, MusicBrainz and tag embedding require the service backend.", "Current test index: isolated Python SQLite. PostgreSQL is planned for the separate web container."]);
  const link = node("a", "guide-link", "Open WebUI guide"); link.href = "/guide"; link.target = "_blank"; link.rel = "noopener noreferrer"; $("dialogBody").append(link);
  $("dialogBody").append(node("p", "", "The same guide is staged for docs/browser-workspace-guide.md on dev after this parity review."));
});
$("closeDialog").addEventListener("click", () => $("workspaceDialog").close());

const workspaceElement = document.querySelector(".workspace");
const layoutStorageKey = "splined.browser-preview.layout";
function clamp(value, minimum, maximum) { return Math.max(minimum, Math.min(value, Math.max(minimum, maximum))); }
function setDockSize(value) {
  workspaceElement.style.setProperty("--dock-height", value + "px");
  workspaceElement.style.setProperty("--candidate-card-width", clamp(Math.round(value * 0.69), 114, 240) + "px");
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
    if (Number.isFinite(values.library)) workspaceElement.style.setProperty("--library-width", clamp(values.library, 200, 480) + "px");
    if (Number.isFinite(values.review)) workspaceElement.style.setProperty("--review-width", clamp(values.review, 250, 700) + "px");
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
  () => [200, Math.min(480, workspaceElement.clientWidth - $("review").getBoundingClientRect().width - 380)],
  value => workspaceElement.style.setProperty("--library-width", value + "px"));
installSplitter("reviewSplitter", "x", -1,
  () => $("review").getBoundingClientRect().width,
  () => [250, Math.min(700, workspaceElement.clientWidth - $("libraryBody").parentElement.getBoundingClientRect().width - 380)],
  value => workspaceElement.style.setProperty("--review-width", value + "px"));
installSplitter("candidateSplitter", "y", -1,
  () => $("candidateDock").getBoundingClientRect().height,
  () => [160, Math.min(620, workspaceElement.clientHeight - 280)],
  setDockSize);
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
try { const theme = localStorage.getItem("splined.browser-preview.theme"); if (["dark", "chalk", "light"].includes(theme)) { state.theme = theme; document.documentElement.dataset.theme = theme; } } catch {}

loadLibrary();
