"use strict";

const $ = id => document.getElementById(id);
const state = {
  artists: [], albumsIndex: [], artist: null, album: null,
  libraryMode: "artists", editorMode: "artist", workspace: "find",
  candidates: [], selectedPortrait: null, selectedBanner: null, selectedCover: null,
  activeSource: "", radioCurrent: true, radioArtist: false, radioBanner: false,
  embedQueue: [], listRequest: 0,
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
  const element = node("img", className); element.src = src; element.alt = alt; element.loading = "lazy"; return element;
}
function clear(element) { element.replaceChildren(); }
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
    $("artistCount").textContent = state.artists.length + " shown";
    for (const artist of state.artists) {
      const button = node("button", "library-row" + (state.artist?.id === artist.id ? " active" : "")); button.type = "button";
      button.append(node("span", "status-orbit", state.artist?.id === artist.id ? "●" : "○"));
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
    $("artistCount").textContent = state.albumsIndex.length + " shown";
    for (const album of state.albumsIndex) list.append(albumRow(album, false));
  }
  if (!list.childElementCount) list.append(node("p", "empty-copy", "No items match this filter."));
}
function albumRow(album, child) {
  const button = node("button", "library-row album-row" + (child ? " album-child" : "") + (state.album?.id === album.id ? " active" : "")); button.type = "button";
  button.append(node("span", "status-orbit", state.album?.id === album.id ? "●" : "○"));
  button.append(album.cover ? image(album.cover, "library-thumb", "") : node("span", "library-thumb", "♫"));
  const copy = node("span", "row-copy"); copy.append(node("strong", "", album.name), node("small", "", child ? album.tracks + " tracks" : album.artist_name)); button.append(copy);
  button.addEventListener("click", () => chooseAlbum(album.id)); return button;
}

function renderHero() {
  const artist = state.artist;
  $("artistName").textContent = artist?.name || "Select an artist";
  $("overviewArtist").textContent = artist?.name || "";
  $("artistSummary").textContent = artist ? artist.albums.length + " albums · " + (artist.status || "indexed") : "Browse your music library to begin.";
  $("hero").style.backgroundImage = artist?.banner ? `url("${artist.banner}")` : artist?.portrait ? `url("${artist.portrait}")` : "none";
  const portrait = $("heroPortrait");
  const replacement = artist?.portrait ? image(artist.portrait, "hero-portrait", artist.name + " artist image") : node("div", "hero-portrait placeholder", "♪");
  replacement.id = "heroPortrait"; portrait.replaceWith(replacement);
  const grid = $("albumGrid"); clear(grid);
  const albums = artist?.albums || []; $("albumCount").textContent = albums.length + " shown";
  if (!albums.length) { grid.append(node("p", "empty-copy", "Choose an artist to view albums.")); return; }
  for (const album of albums) {
    const button = node("button", "album-card" + (state.album?.id === album.id ? " active" : "")); button.type = "button";
    button.append(album.cover ? image(album.cover, "", album.name + " cover") : node("span", "album-placeholder", "♫"));
    button.append(node("strong", "", album.name), node("small", "", album.tracks + " tracks"));
    button.addEventListener("click", () => chooseAlbum(album.id)); grid.append(button);
  }
}

function renderSources() {
  const strip = $("sourceStrip"); clear(strip);
  for (const [label, key, icon] of SOURCES) {
    const count = state.candidates.filter(candidate => sourceKey(candidate.source) === key).length;
    const available = !!key && count > 0;
    const button = node("button", "source-card" + (!available ? " unavailable" : "") + (state.activeSource === key && key ? " active" : "")); button.type = "button"; button.disabled = !available;
    button.title = available ? "Show " + label + " candidates" : label + " · no candidates in this preview";
    button.append(icon === "⊘" ? node("span", "source-symbol", "⊘") : image("/assets/source-icons/" + icon + ".svg", "", ""));
    button.append(node("strong", "", label), node("small", "", available ? count + " found" : "0 found"), node("span", "source-state", available ? "● Ready" : "○ —"));
    button.addEventListener("click", () => { state.activeSource = state.activeSource === key ? "" : key; renderSources(); renderCandidates(); }); strip.append(button);
  }
}
function renderCandidates() {
  const grid = $("candidateGrid"); clear(grid);
  const query = $("candidateSearch").value.trim().toLowerCase();
  const visible = state.candidates.filter(candidate => (!state.activeSource || sourceKey(candidate.source) === state.activeSource) && (!query || (candidate.name + " " + candidate.source).toLowerCase().includes(query)));
  $("candidateCount").textContent = state.candidates.length + (state.candidates.length === 1 ? " candidate" : " candidates");
  $("candidateHint").textContent = state.editorMode === "album" ? "Select a cover candidate to preview" : "Blue = artist · pink = banner";
  if (!visible.length) { grid.append(node("p", "empty-copy", state.candidates.length ? "No candidates match the filter." : "Press LAUNCH to search artist art, or select an album to inspect folder art.")); return; }
  for (const candidate of visible) {
    const kind = candidateKind(candidate);
    const active = state.selectedCover === candidate || state.selectedPortrait === candidate || state.selectedBanner === candidate;
    const button = node("button", "candidate-card" + (kind === "banner" ? " banner" : "") + (active ? " active" : "")); button.type = "button";
    button.append(image(candidate.image, "", candidate.name), node("strong", "", candidate.name), node("small", "", candidate.source + " · " + candidate.width + " × " + candidate.height));
    button.append(node("span", "candidate-type"));
    button.addEventListener("click", () => {
      if (state.editorMode === "album") state.selectedCover = candidate;
      else if (kind === "banner") { state.selectedBanner = candidate; state.radioCurrent = false; state.radioBanner = true; }
      else { state.selectedPortrait = candidate; state.radioCurrent = false; state.radioArtist = true; }
      renderCandidates(); renderPanes(); renderReview(); setStatus(candidate.name + " selected for preview");
    }); grid.append(button);
  }
}

function packageFrame(frame, banner, portrait) {
  clear(frame);
  if (!banner && !portrait) { frame.append(node("span", "empty-image", "No current artist package")); return; }
  if (banner) frame.append(image(banner, "package-banner", "Artist banner"));
  if (portrait) frame.append(image(portrait, "package-portrait", "Artist portrait"));
}
function paneImage(frame, src, label, portrait = false) {
  clear(frame); frame.append(src ? image(src, portrait ? "portrait-preview" : "", label) : node("span", "empty-image", label));
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
    if (portrait) packageView.append(image(portrait, "portrait-art", "Artist portrait"));
    if (banner || portrait) frame.append(packageView);
    type = "Artist package"; source = state.radioCurrent ? "Current local art" : "Candidate preview"; quality = state.radioCurrent ? "Current" : "Preview";
  } else {
    const bannerOnly = state.radioBanner && !state.radioArtist;
    candidate = bannerOnly ? state.selectedBanner : state.selectedPortrait;
    url = candidate?.image || (bannerOnly ? state.artist?.banner : state.artist?.portrait);
    type = bannerOnly ? "Artist Banner" : "Artist Image"; source = candidate?.source || (url ? "Local" : "—"); quality = candidate ? "Preview" : "Current";
    if (url) frame.append(image(url, bannerOnly ? "selected-image" : "selected-image portrait", type));
  }
  if (!frame.childElementCount) frame.append(image("/assets/set-your-imagination-tv.jpg", "", "Set your imagination now"));
  $("selectedType").textContent = type; $("selectedSource").textContent = source;
  $("selectedDimensions").textContent = candidate ? candidate.width + " × " + candidate.height : "—";
  $("selectedSize").textContent = candidate ? size(candidate.bytes) : "—";
  $("selectedQuality").textContent = quality;
}
function renderReview() {
  $("writeButton").textContent = state.editorMode === "artist" ? "WRITE Artist Art" : "WRITE Cover";
  $("embedBatchButton").disabled = !(state.editorMode === "album" && state.album && (state.selectedCover?.image || state.album.cover));
  $("reviewChoice").hidden = state.editorMode !== "artist" || !(state.selectedPortrait || state.selectedBanner);
  $("useArtist").classList.toggle("active", !state.radioBanner || state.radioArtist);
  $("useBanner").classList.toggle("active", state.radioBanner);
  selectedImage();
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
    if (!state.artist && state.artists.length) await chooseArtist(state.artists[0].id);
    else setStatus("Library ready · browser layout preview");
  } catch (error) { if (request === state.listRequest) setStatus(error.message, "error"); }
}
async function chooseArtist(id) {
  setStatus("Loading artist and existing artwork…", "busy");
  try {
    state.artist = await api("/api/artist?id=" + encodeURIComponent(id));
    state.album = null; state.editorMode = "artist"; state.workspace = "find";
    state.candidates = state.artist.local_candidates || []; state.selectedPortrait = null; state.selectedBanner = null; state.selectedCover = null; state.activeSource = "";
    state.radioCurrent = true; state.radioArtist = false; state.radioBanner = false;
    renderAll(); setStatus("Loaded " + state.artist.name + " · " + state.artist.albums.length + " albums");
  } catch (error) { setStatus(error.message, "error"); }
}
async function chooseAlbum(id) {
  setStatus("Loading album and existing cover…", "busy");
  try {
    const album = await api("/api/album?id=" + encodeURIComponent(id));
    if (state.artist?.id !== album.artist_id) state.artist = await api("/api/artist?id=" + encodeURIComponent(album.artist_id));
    state.album = album; state.editorMode = "album"; state.workspace = "find";
    state.candidates = album.candidates || []; state.selectedCover = null; state.activeSource = "";
    renderAll(); setStatus("Loaded " + album.artist_name + " · " + album.name + " · " + album.tracks + " tracks");
  } catch (error) { setStatus(error.message, "error"); }
}
async function searchArtist() {
  if (!state.artist) return;
  const id = state.artist.id;
  $("searchButton").disabled = true;
  setStatus("Searching Fanart.tv and Deezer for " + state.artist.name + "…", "busy");
  try {
    const result = await api("/api/search/artist", {method: "POST", headers: {"Content-Type": "application/json"}, body: JSON.stringify({id})});
    if (state.artist?.id !== id) return;
    state.candidates = [...(state.artist.local_candidates || []), ...result.candidates];
    state.selectedPortrait = null; state.selectedBanner = null;
    state.radioCurrent = true; state.radioArtist = false; state.radioBanner = false;
    state.activeSource = ""; renderSources(); renderCandidates(); renderPanes(); renderReview();
    setStatus(result.candidates.length + " online candidates found" + (result.warnings.length ? " · " + result.warnings.length + " source warnings" : ""));
  } catch (error) { setStatus(error.message, "error"); }
  finally { $("searchButton").disabled = !state.artist; }
}
function renderQueue() {
  const queue = $("embedQueue"); clear(queue);
  if (!state.embedQueue.length) queue.append(node("p", "empty-copy", "No albums staged yet."));
  for (const item of state.embedQueue) {
    const card = node("div", "queue-card");
    card.append(item.cover ? image(item.cover, "", item.name + " cover") : node("span", "library-thumb", "♫"));
    const copy = node("span", "row-copy"); copy.append(node("strong", "", item.artist + " — " + item.name), node("small", "", "Preview staged · " + item.tracks + " tracks · no tags changed")); card.append(copy);
    const remove = node("button", "", "Remove"); remove.type = "button"; remove.addEventListener("click", () => { state.embedQueue = state.embedQueue.filter(entry => entry.id !== item.id); renderQueue(); setStatus("Removed album from preview queue"); }); card.append(remove); queue.append(card);
  }
  $("removeAllStaged").disabled = !state.embedQueue.length;
}
function stageAlbum() {
  if (!state.album) return;
  if (!state.embedQueue.some(item => item.id === state.album.id)) state.embedQueue.push({id: state.album.id, name: state.album.name, artist: state.album.artist_name, cover: state.selectedCover?.image || state.album.cover, tracks: state.album.tracks});
  state.workspace = "embed"; renderQueue(); renderWorkspace(); setStatus("Preview staged: " + state.album.name + " · no tracks changed");
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
$("clearCandidateFilter").addEventListener("click", () => { $("candidateSearch").value = ""; state.activeSource = ""; renderSources(); renderCandidates(); });
for (const [buttonId, mode] of [["artistsTab", "artists"], ["albumsTab", "albums"], ["fallbackTab", "fallback"]]) {
  $(buttonId).addEventListener("click", () => { state.libraryMode = mode; loadLibrary(mode === "artists" ? $("artistSearch").value : $("albumSearch").value); renderLibrary(); });
}
for (const [buttonId, name] of [["findTab", "find"], ["enhanceTab", "enhance"], ["embedTab", "embed"]]) {
  $(buttonId).addEventListener("click", () => { state.workspace = name; renderWorkspace(); if (name === "embed") renderQueue(); setStatus(name === "embed" ? "Embedding queue preview · no tags changed" : name === "enhance" ? "Enhancement layout preview · no image processing" : "Find & Match Artwork ready"); });
}
$("searchButton").addEventListener("click", () => state.editorMode === "artist" ? searchArtist() : (renderCandidates(), setStatus("Showing images in the selected album folder")));
$("upscaleButton").addEventListener("click", () => { state.workspace = "enhance"; renderWorkspace(); setStatus("Enhancement layout preview · no upscale processing"); });
$("embedBatchButton").addEventListener("click", stageAlbum);
$("removeAllStaged").addEventListener("click", () => { state.embedQueue = []; renderQueue(); setStatus("Preview queue cleared"); });
$("collapseLibrary").addEventListener("click", () => { const collapsed = document.querySelector(".workspace").classList.toggle("library-collapsed"); $("collapseLibrary").textContent = collapsed ? "+" : "−"; $("collapseLibrary").title = collapsed ? "Expand Media Library" : "Collapse Media Library"; });
for (const [buttonId, panelId, otherId] of [["mediaFilterToggle", "mediaFilter", "candidateFilter"], ["candidateFilterToggle", "candidateFilter", "mediaFilter"]]) {
  $(buttonId).addEventListener("click", () => { const open = $(panelId).hidden; $(panelId).hidden = !open; $(otherId).hidden = true; for (const [id, panel] of [["mediaFilterToggle", "mediaFilter"], ["candidateFilterToggle", "candidateFilter"]]) { const active = !$(panel).hidden; $(id).classList.toggle("active", active); $(id).setAttribute("aria-expanded", String(active)); $(id).querySelector("b").textContent = active ? "−" : "+"; } });
}
for (const button of document.querySelectorAll(".group-toggle")) button.addEventListener("click", () => { const panel = $(button.dataset.group + "Group"); panel.hidden = !panel.hidden; button.querySelector("b").textContent = panel.hidden ? "+" : "−"; });
$("resetEnhancements").addEventListener("click", () => { state.selectedPortrait = null; state.selectedBanner = null; state.selectedCover = null; state.radioCurrent = true; state.radioArtist = false; state.radioBanner = false; renderCandidates(); renderPanes(); renderReview(); setStatus("Preview selection reset · current files unchanged"); });
$("currentRadio").addEventListener("click", () => { state.radioCurrent = true; state.radioArtist = false; state.radioBanner = false; renderPanes(); renderReview(); });
$("artistRadio").addEventListener("click", () => { state.radioArtist = !state.radioArtist; state.radioCurrent = !state.radioArtist && !state.radioBanner; renderPanes(); renderReview(); });
$("bannerRadio").addEventListener("click", () => { state.radioBanner = !state.radioBanner; state.radioCurrent = !state.radioArtist && !state.radioBanner; renderPanes(); renderReview(); });
$("useArtist").addEventListener("click", () => { state.radioArtist = true; state.radioBanner = false; state.radioCurrent = false; renderPanes(); renderReview(); });
$("useBanner").addEventListener("click", () => { state.radioBanner = true; state.radioArtist = false; state.radioCurrent = false; renderPanes(); renderReview(); });
$("showAllAlbums").addEventListener("click", () => { const expanded = $("albumGrid").classList.toggle("expanded"); $("showAllAlbums").textContent = expanded ? "Show Less" : "Show All"; });
$("compReportButton").addEventListener("click", () => {
  const albums = (state.artist?.albums || []).filter(album => album.compilation);
  showDialog("COMP Report · " + (state.artist?.name || "No artist selected"), albums.length ? albums.map(album => album.name + " · " + album.tracks + " tracks · " + album.status) : ["No compilation albums are indexed for this artist."]);
});
$("themeButton").addEventListener("click", () => showDialog("Theme", ["This template uses the Windows dark theme colors and SPLINED branding assets.", "Additional Windows themes can be mapped here after the browser workspace behavior is established."]));
$("settingsButton").addEventListener("click", () => showDialog("Settings", ["Browser settings are represented by this panel. The local template reads the isolated Python index and copied music files.", "The dedicated PostgreSQL catalog and container settings will be connected in the image build. No settings are changed here."]));
$("helpButton").addEventListener("click", () => showDialog("Workspace Help", ["Choose an artist or album in Media Library, then press LAUNCH to search artist art or review folder cover candidates.", "Click candidate images to preview them. The enhancement pane radios choose current, artist, banner, or a combined package.", "EMBED Batch stages the selected album only in this browser preview. WRITE and processing actions remain unavailable."]));
$("closeDialog").addEventListener("click", () => $("workspaceDialog").close());

loadLibrary();
