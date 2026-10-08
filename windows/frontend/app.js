import {
  albumStatusClass,
  applyArtistSelection,
  applyBulkSelection,
  artistAggregateStatus,
  artistStatusClass,
  buildMusicBrainzView,
  canPersistPortableUi,
  checkTrack,
  compilationArtworkPending,
  configuredScanMode,
  createSelectionState,
  filterCandidates,
  focusTrack,
  groupAlbumsByArtist,
  launchSelection,
  releaseTypesForArtists,
  restoreSafeSelection,
  scanRequestFromSelection,
  selectAlbum,
  shouldLoadLibraryAfterBootstrap,
} from "./app-model.mjs";

const tauri = window.__TAURI__;
const invoke = tauri?.core?.invoke;
const listen = tauri?.event?.listen;
const dialogApi = tauri?.dialog;
const opener = tauri?.opener;
const windowApi = tauri?.window;
const appWindow = windowApi?.getCurrentWindow?.();
const $ = selector => document.querySelector(selector);
const $$ = selector => [...document.querySelectorAll(selector)];
const escapeHtml = value => String(value ?? "").replace(/[&<>'"]/g, character => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", "'": "&#39;", '"': "&quot;" }[character]));

const runtime = {
  portable: null,
  ui: null,
  library: null,
  albums: new Map(),
  selection: createSelectionState(),
  selectionHistory: [],
  selectionCursor: -1,
  running: false,
  waiting: false,
  currentAlbum: null,
  candidates: [],
  selectedCandidate: null,
  musicBrainz: null,
  mbArtistFilters: [],
  mbTypeFilters: [],
  selectedMusicBrainz: null,
  lastVisitedRelease: "",
  candidateFilters: { sources: [], types: [], policies: [], ranges: [] },
  saveTimer: null,
};

async function command(name, args = {}) {
  if (!invoke) throw new Error("The desktop command bridge is unavailable.");
  return invoke(name, args);
}

function notify(title, body, action = null) {
  $("#message-title").textContent = title;
  $("#message-body").textContent = String(body ?? "");
  const button = $("#message-action");
  button.classList.toggle("hidden", !action);
  if (action) {
    button.textContent = action.label;
    button.onclick = action.run;
  }
  $("#message-dialog").showModal();
}

function activity(message, kind = "") {
  const item = document.createElement("li");
  item.textContent = message;
  if (kind) item.className = kind;
  $("#activity-log").append(item);
  item.scrollIntoView({ block: "nearest" });
  $("#status-text").textContent = message;
}

function applyTheme(theme) {
  document.documentElement.dataset.theme = theme || "System";
  $("#theme-select").value = theme || "System";
}

function captureUi() {
  if (!runtime.ui) return null;
  const paneWidth = Math.round($("#media-pane").getBoundingClientRect().width);
  return {
    ...runtime.ui,
    theme: $("#theme-select").value,
    hover_enabled: $("#hover-toggle").checked,
    media_filter_expanded: !$("#media-filters").classList.contains("hidden"),
    candidate_filter_expanded: !$("#candidate-filters").classList.contains("hidden"),
    media_artist_filter: $("#artist-filter").value,
    media_album_filter: $("#album-filter").value,
    show_tracks: $("#show-tracks").checked,
    selected_compilation_track_path: runtime.selection.explicitTrackPath,
    filtered_scan_mode: $("#scan-mode").value,
    auto_scan_enabled: $("#auto-scan").checked,
    auto_scan_scope: $("#auto-scan-scope").value,
    media_show_white: $('[data-status="unprocessed"]').checked,
    media_show_incomplete: $('[data-status="incomplete"]').checked,
    media_show_orange: $('[data-status="processed"]').checked,
    media_show_red: $('[data-status="bypassed"]').checked,
    media_show_purple: $('[data-status="partial-timeout"]').checked,
    media_show_green: $('[data-status="artist-complete"]').checked,
    media_show_blue: $('[data-status="artist-bypass"]').checked,
    selected_album_paths: [...runtime.selection.albumPaths].filter(path => {
      const status = String(runtime.albums.get(path)?.status || "").toLocaleLowerCase();
      return status !== "bypassed" && status !== "timeout";
    }),
    candidate_excluded_sources: runtime.candidateFilters.sources,
    candidate_excluded_types: runtime.candidateFilters.types,
    candidate_excluded_policies: runtime.candidateFilters.policies,
    candidate_excluded_ranges: runtime.candidateFilters.ranges,
    main_width: Math.max(940, window.innerWidth),
    main_height: Math.max(640, window.innerHeight),
    main_splitter_distance: paneWidth,
  };
}

async function captureWindowUi(ui) {
  if (!ui || !appWindow) return ui;
  try {
    const [size, position, maximized] = await Promise.all([
      appWindow.innerSize(), appWindow.outerPosition(), appWindow.isMaximized(),
    ]);
    return {
      ...ui,
      main_width: Math.max(940, size.width),
      main_height: Math.max(640, size.height),
      main_x: position.x,
      main_y: position.y,
      main_maximized: maximized,
    };
  } catch {
    return ui;
  }
}

async function restoreWindowUi(ui) {
  if (!appWindow || !windowApi?.PhysicalSize || !ui) return;
  try {
    if (!ui.main_maximized) {
      await appWindow.setSize(new windowApi.PhysicalSize(ui.main_width, ui.main_height));
      if (ui.main_x >= 0 && ui.main_y >= 0 && windowApi.PhysicalPosition) {
        await appWindow.setPosition(new windowApi.PhysicalPosition(ui.main_x, ui.main_y));
      }
    } else {
      await appWindow.maximize();
    }
  } catch (error) {
    activity(`Saved window placement could not be restored: ${error}`);
  }
}

function scheduleUiSave() {
  if (!canPersistPortableUi(runtime.portable)) return;
  clearTimeout(runtime.saveTimer);
  runtime.saveTimer = setTimeout(async () => {
    const ui = await captureWindowUi(captureUi());
    if (!ui) return;
    runtime.ui = ui;
    try { await command("save_ui", { ui }); } catch (error) { activity(`UI state was not saved: ${error}`, "error"); }
  }, 250);
}

function restoreUi() {
  const ui = runtime.ui;
  applyTheme(ui.theme);
  $("#hover-toggle").checked = ui.hover_enabled;
  $("#artist-filter").value = ui.media_artist_filter;
  $("#album-filter").value = ui.media_album_filter;
  $("#show-tracks").checked = ui.show_tracks;
  $("#scan-mode").value = configuredScanMode(runtime.portable?.config_text);
  $("#auto-scan").checked = ui.auto_scan_enabled;
  $("#auto-scan-scope").value = ui.auto_scan_scope === "all" ? "all" : "selected";
  $('[data-status="unprocessed"]').checked = ui.media_show_white !== false;
  $('[data-status="incomplete"]').checked = ui.media_show_incomplete !== false;
  $('[data-status="processed"]').checked = ui.media_show_orange !== false;
  $('[data-status="bypassed"]').checked = ui.media_show_red !== false;
  $('[data-status="partial-timeout"]').checked = ui.media_show_purple !== false;
  $('[data-status="artist-complete"]').checked = ui.media_show_green !== false;
  $('[data-status="artist-bypass"]').checked = ui.media_show_blue !== false;
  $("#media-filters").classList.toggle("hidden", !ui.media_filter_expanded);
  $("#candidate-filters").classList.toggle("hidden", !ui.candidate_filter_expanded);
  document.documentElement.style.setProperty("--media-width", `${Math.max(250, ui.main_splitter_distance)}px`);
  runtime.selection.albumPaths = new Set(ui.selected_album_paths || []);
  runtime.selection.explicitTrackPath = ui.selected_compilation_track_path || "";
  runtime.candidateFilters = {
    sources: ui.candidate_excluded_sources || [], types: ui.candidate_excluded_types || [],
    policies: ui.candidate_excluded_policies || [], ranges: ui.candidate_excluded_ranges || [],
  };
}

async function loadLibrary(refresh = false) {
  $("#library-summary").textContent = refresh ? "Refreshing media index…" : "Loading media index…";
  try {
    runtime.library = await command("load_library", { refresh });
    runtime.albums = new Map(runtime.library.albums.map(album => [album.path, album]));
    runtime.selection = restoreSafeSelection(runtime.selection, runtime.library.albums);
    $("#library-summary").textContent = `${runtime.library.albums.length} Albums · ${runtime.library.database_path}`;
    renderMediaTree();
  } catch (error) {
    $("#library-summary").textContent = "Media index unavailable";
    activity(String(error), "error");
  }
}

function activeStatusFilters() {
  return new Set($$("[data-status]").filter(item => item.checked).map(item => item.dataset.status));
}

function albumFilterCategory(album) {
  const color = albumStatusClass(album.status);
  return color === "purple" ? "partial-timeout"
    : color === "blue" ? "incomplete"
      : color === "orange" ? "processed"
        : color === "red" ? "bypassed" : "unprocessed";
}

function artistFilterCategory(status) {
  if (status === "contains-bypass") return "artist-bypass";
  if (status === "complete") return "artist-complete";
  if (status === "partial") return "partial-timeout";
  return "unprocessed";
}

function textFilteredAlbums() {
  if (!runtime.library) return [];
  const artistNeedle = $("#artist-filter").value.trim().toLocaleLowerCase();
  const albumNeedle = $("#album-filter").value.trim().toLocaleLowerCase();
  return runtime.library.albums.filter(album =>
    (!artistNeedle || String(album.artist || album.tagged_artist).toLocaleLowerCase().includes(artistNeedle))
    && (!albumNeedle || String(album.title).toLocaleLowerCase().includes(albumNeedle)));
}

function updateSelectionSummary() {
  $("#selection-count").textContent = `SELECTED [${runtime.selection.albumPaths.size}]`;
}

function updateStatusCounts(allGroups) {
  const counts = {
    unprocessed: 0, incomplete: 0, processed: 0, bypassed: 0,
    "partial-timeout": 0, "artist-complete": 0, "artist-bypass": 0,
  };
  for (const album of runtime.library.albums) counts[albumFilterCategory(album)] += 1;
  for (const group of allGroups.values()) {
    const category = artistFilterCategory(artistAggregateStatus(group.albums));
    if (category !== "unprocessed") counts[category] += 1;
  }
  $$('[data-status-count]').forEach(output => { output.value = counts[output.dataset.statusCount] || 0; });
}

function renderMediaTree() {
  if (!runtime.library) return;
  const statuses = activeStatusFilters();
  const showTracks = $("#show-tracks").checked;
  const allGroups = new Map(groupAlbumsByArtist(runtime.library.albums).map(group => [group.artist, group]));
  updateStatusCounts(allGroups);
  const groups = groupAlbumsByArtist(textFilteredAlbums()).map(group => {
    const aggregate = artistAggregateStatus(allGroups.get(group.artist)?.albums || group.albums);
    return { ...group, aggregate, albums: group.albums.filter(album => statuses.has(albumFilterCategory(album))) };
  }).filter(group => group.albums.length && statuses.has(artistFilterCategory(group.aggregate)));
  $("#media-tree").innerHTML = groups.map(group => `
    <details class="tree-artist" open><summary data-artist-row="${escapeHtml(group.artist)}"><span class="status-ring ${artistStatusClass(group.aggregate)}"></span>${escapeHtml(group.artist)} <span class="subtle">${group.albums.length}</span></summary>
      ${group.albums.map(album => {
        const focused = runtime.selection.focusedAlbumPath === album.path ? " focused" : "";
        const selected = runtime.selection.albumPaths.has(album.path) ? " selected" : "";
        const tracks = showTracks && album.compilation_track_art_eligible ? album.compilation_tracks.map(track => `
          <div class="track-row${runtime.selection.focusedTrackPath === track.path ? " focused" : ""}" data-track-row="${escapeHtml(track.path)}" data-album-path="${escapeHtml(album.path)}" title="Click previews; check explicitly reopens this track">
            <input type="checkbox" data-track-check="${escapeHtml(track.path)}" data-album-path="${escapeHtml(album.path)}" ${runtime.selection.explicitTrackPath === track.path ? "checked" : ""}>
            <span class="status-ring ${track.embedded_artwork_recorded ? "green" : "white"}"></span><span class="album-title">${escapeHtml(track.title || track.path.split(/[\\/]/).pop())}</span>
          </div>`).join("") : "";
        return `<div class="album-row${focused}${selected}" data-album-row="${escapeHtml(album.path)}" title="Click selects this Album; Ctrl+Click toggles it additively"><span class="status-ring ${albumStatusClass(album.status)}"></span><span class="album-title">${escapeHtml(album.title || album.path)}${album.compilation ? " · Compilation" : ""}</span></div>${tracks}`;
      }).join("")}
    </details>`).join("") || `<div class="empty-state"><p>No Albums match the current filters.</p></div>`;

  $$('[data-artist-row]').forEach(row => row.addEventListener("click", event => {
    const group = groups.find(item => item.artist === row.dataset.artistRow);
    if (group) selectArtistGroup(group, event.ctrlKey);
  }));
  $$('[data-album-row]').forEach(row => row.addEventListener("click", event => selectAlbumRow(row.dataset.albumRow, event.ctrlKey)));
  $$('[data-track-row]').forEach(row => row.addEventListener("click", event => {
    if (event.target.matches("input")) return;
    runtime.selection = focusTrack(runtime.selection, row.dataset.albumPath, row.dataset.trackRow);
    previewTrack(row.dataset.trackRow); renderMediaTree(); scheduleUiSave();
  }));
  $$('[data-track-check]').forEach(box => box.addEventListener("change", () => {
    runtime.selection = checkTrack(runtime.selection, box.dataset.albumPath, box.dataset.trackCheck, box.checked);
    const album = runtime.albums.get(box.dataset.albumPath);
    if (box.checked && String(album?.status || "").toLocaleLowerCase() === "bypassed") {
      runtime.selection.bypassOverrides.add(box.dataset.albumPath);
    }
    if (box.checked) previewTrack(box.dataset.trackCheck);
    pushSelectionHistory(); renderMediaTree(); scheduleUiSave();
  }));
  updateSelectionSummary();
}

async function selectAlbumRow(path, additive) {
  const album = runtime.albums.get(path);
  if (!album) return;
  let outcome = selectAlbum(runtime.selection, album, { additive });
  if (outcome.protection === "timeout") {
    await focusAlbum(path);
    notify("Album timeout active", "This Album remains protected until its recorded timeout expires.");
    return;
  }
  if (outcome.protection === "bypass") {
    const allowed = window.confirm("This Album is marked bypassed. Override bypass for this run only?\n\nThe saved bypass history will not be deleted.");
    if (!allowed) { await focusAlbum(path); return; }
    outcome = selectAlbum(runtime.selection, album, { additive, allowBypass: true });
  }
  runtime.selection = outcome.selection;
  await focusAlbum(path);
  pushSelectionHistory(); renderMediaTree(); scheduleUiSave();
}

function selectArtistGroup(group, additive) {
  const protectedBypass = group.albums.filter(album => String(album.status).toLocaleLowerCase() === "bypassed" && !compilationArtworkPending(album));
  const includeBypassed = protectedBypass.length > 0 && window.confirm(`This Artist contains ${protectedBypass.length} bypassed Album(s). Include them using a temporary override for this run?\n\nThe saved bypass history will not be deleted.`);
  runtime.selection = applyArtistSelection(runtime.selection, group.albums, { additive, includeBypassed, artist: group.artist });
  const focused = group.albums.find(album => runtime.selection.albumPaths.has(album.path));
  if (focused) focusAlbum(focused.path);
  pushSelectionHistory(); renderMediaTree(); scheduleUiSave();
}

function applySelectMode(mode) {
  if (mode === "none") {
    runtime.selection = applyBulkSelection(runtime.selection, [], "none");
  } else if (mode === "all") {
    if (!runtime.selection.focusedArtist) return notify("Select [ALL]", "Select an Artist first.");
    const albums = runtime.library.albums.filter(album => (album.artist || album.tagged_artist) === runtime.selection.focusedArtist);
    runtime.selection = applyBulkSelection(runtime.selection, albums, "all");
  } else {
    if (!$("#artist-filter").value.trim() && !$("#album-filter").value.trim()) {
      return notify("Select [FILTERED]", "Enter an Artist or Album filter first.");
    }
    runtime.selection = applyBulkSelection(runtime.selection, textFilteredAlbums(), "filtered");
  }
  pushSelectionHistory(); renderMediaTree(); scheduleUiSave();
}

async function focusAlbum(path) {
  const album = runtime.albums.get(path);
  if (!album) return;
  runtime.selection = { ...runtime.selection, focusedAlbumPath: path, focusedTrackPath: "" };
  runtime.currentAlbum = album;
  renderMediaTree(); scheduleUiSave();
  if (album.compilation_track_art_eligible) {
    showArtwork(null, "No embedded artwork", "Selected Track Embedded Artwork");
    $("#artwork-caption").textContent = "Select a track to preview its embedded front artwork. Folder cover files are not used in this workflow.";
  } else if (album.cover_path) {
    try { showArtwork(await command("read_image_data_url", { path: album.cover_path }), `${album.cover_name} · ${album.cover_width} × ${album.cover_height}`, "Selected Album Artwork"); }
    catch { showArtwork(null, "No artwork", "Selected Album Artwork"); }
  } else showArtwork(null, "No artwork", "Selected Album Artwork");
}

async function previewTrack(path) {
  try {
    const data = await command("read_track_embedded_artwork", { path });
    showArtwork(data, data ? `${path.split(/[\\/]/).pop()} · embedded front artwork` : "No embedded artwork", "Selected Track Embedded Artwork");
  } catch (error) { showArtwork(null, "No embedded artwork", "Selected Track Embedded Artwork"); activity(String(error), "error"); }
}

function showArtwork(dataUrl, caption, title = "Selected Album Artwork") {
  $("#artwork-title").textContent = title;
  $("#artwork-preview").innerHTML = dataUrl ? `<img src="${dataUrl}" alt="${escapeHtml(title)}">` : `<span>${escapeHtml(caption)}</span>`;
  $("#artwork-caption").textContent = caption || "";
  const image = $("#artwork-preview img");
  if (image) image.onclick = () => openImageOverlay([image.src]);
}

function pushSelectionHistory() {
  const snapshot = { albums: [...runtime.selection.albumPaths], overrides: [...runtime.selection.bypassOverrides], explicit: runtime.selection.explicitTrackPath };
  const current = runtime.selectionHistory[runtime.selectionCursor];
  if (current && JSON.stringify(current) === JSON.stringify(snapshot)) return;
  runtime.selectionHistory = runtime.selectionHistory.slice(0, runtime.selectionCursor + 1);
  runtime.selectionHistory.push(snapshot); runtime.selectionCursor = runtime.selectionHistory.length - 1;
}

function moveSelectionHistory(delta) {
  const next = runtime.selectionCursor + delta;
  if (next < 0 || next >= runtime.selectionHistory.length) return;
  runtime.selectionCursor = next;
  const snapshot = runtime.selectionHistory[next];
  runtime.selection.albumPaths = new Set(snapshot.albums); runtime.selection.bypassOverrides = new Set(snapshot.overrides || []); runtime.selection.explicitTrackPath = snapshot.explicit;
  renderMediaTree(); scheduleUiSave();
}

async function launchScan() {
  if (runtime.running) { await command("stop_scan"); activity("Stop requested; the in-process workflow will stop at a safe boundary."); return; }
  const autoScan = $("#auto-scan").checked;
  const launchState = launchSelection(runtime.selection, runtime.library?.albums || [], autoScan ? $("#auto-scan-scope").value : "selected");
  const request = scanRequestFromSelection(launchState, {
    albums: runtime.library?.albums || [],
    mode: $("#scan-mode").value,
    reviewRequired: true,
    autoIdeal: autoScan,
  });
  if (!request.albums.length) { notify("Selection required", "Select at least one Album before launching."); return; }
  try {
    runtime.running = true; runtime.waiting = false; setLifecycle("STOP");
    $("#run-progress").value = 0; $("#run-progress").max = request.albums.length;
    activity(`${request.mode === "write" ? "LIVE WRITE" : "READ"} scan started in the application process.`);
    await command("start_scan", { request });
  } catch (error) { runtime.running = false; setLifecycle("LAUNCH"); notify("Scan could not start", error); }
}

function setLifecycle(label) {
  $("#launch-button").textContent = label;
  $("#launch-button").classList.toggle("primary", label === "LAUNCH");
}

function setWaiting(waiting) {
  runtime.waiting = waiting;
  if (runtime.running) setLifecycle(waiting ? "WAITING" : "STOP");
  $("#use-selected").disabled = !waiting || runtime.selectedCandidate == null;
  $("#skip-button").disabled = !waiting;
}

async function submitDecision(decision) {
  try { await command("submit_scan_decision", { decision }); setWaiting(false); }
  catch (error) { notify("Decision was not accepted", error); }
}

function advancedDecisionValues() {
  const value = id => Number($(`#${id}`)?.value || 0);
  return {
    upscale_adaptive_defaults: $("#adaptive-defaults")?.checked ?? true,
    upscale_picture_percent: value("edit-picture"), upscale_sharpen_percent: value("edit-sharpen"),
    upscale_softness_percent: value("edit-softness"), upscale_contrast_percent: value("edit-contrast"),
    upscale_exposure_percent: value("edit-exposure"), upscale_brightness_percent: value("edit-brightness"),
    upscale_gamma_percent: value("edit-gamma"), upscale_color_temperature: value("edit-color"),
    apply_edit_profile: $("#apply-edit-profile")?.checked ?? false,
    edit_existing_cover: $("#edit-existing-cover")?.checked ?? false,
  };
}

function renderAdvancedControls(payload) {
  const controls = [["picture",-50,50],["sharpen",-100,100],["softness",-100,100],["contrast",-100,100],["exposure",-100,100],["brightness",-100,100],["gamma",-100,100],["color",-100,100]];
  const musicBrainzAction = payload.musicbrainz_back_available
    ? { action:"back_musicbrainz", label:"MusicBrainz Matches…" }
    : payload.musicbrainz_retry_available
      ? { action:"retry_musicbrainz", label:"Retry MusicBrainz…" }
      : null;
  $("#decision-title").textContent = "Upscale / Advanced";
  $("#decision-content").innerHTML = `<div class="mb-authority"><label><input id="adaptive-defaults" type="checkbox" checked> Apply default upscale</label></div><div class="candidate-filters">${controls.map(([name,min,max]) => `<label>${name[0].toUpperCase()+name.slice(1)} <input id="edit-${name}" type="range" min="${min}" max="${max}" value="0"><output>0</output></label>`).join("")}</div><div class="candidate-filters"><label><input id="apply-edit-profile" type="checkbox"> Apply editing profile</label><label><input id="edit-existing-cover" type="checkbox"> Edit existing cover</label>${musicBrainzAction ? `<button id="source-navigation">${musicBrainzAction.label}</button>` : ""}</div>`;
  $$('#decision-content input[type="range"]').forEach(slider => slider.oninput = () => slider.nextElementSibling.textContent = slider.value);
  if (musicBrainzAction) $("#source-navigation").onclick = () => submitDecision({ action:musicBrainzAction.action });
}

async function renderCandidates(payload) {
  runtime.candidates = payload.items || []; runtime.selectedCandidate = payload.recommended_index || runtime.candidates[0]?.index || null;
  runtime.currentCandidatePayload = payload;
  const visible = filterCandidates(runtime.candidates, runtime.candidateFilters);
  $("#run-context").textContent = `${payload.compilation_track ? "Compilation track" : "Album"} · ${visible.length} visible candidate${visible.length === 1 ? "" : "s"}${payload.hidden_by_source_policy ? ` · ${payload.hidden_by_source_policy} policy-hidden` : ""}`;
  $("#candidate-grid").innerHTML = visible.length ? visible.map(item => `<article class="candidate-card${item.index === runtime.selectedCandidate ? " selected" : ""}${item.recommended ? " recommended" : ""}" data-candidate="${item.index}"><div class="candidate-image" data-image-path="${escapeHtml(item.cache_path)}"></div><strong>${escapeHtml(item.source)}</strong><div class="candidate-meta">${item.width} × ${item.height} → ${item.projected_width} × ${item.projected_height}<br>${escapeHtml(item.range_class)} · ${escapeHtml(item.policy_status)}</div></article>`).join("") : `<div class="empty-state compact"><p>No candidates match the Artwork Filter.</p></div>`;
  for (const holder of $$("[data-image-path]")) {
    try { const src = await command("read_image_data_url", { path: holder.dataset.imagePath }); holder.innerHTML = `<img src="${src}" alt="Artwork candidate">`; }
    catch { holder.innerHTML = `<div class="empty-state compact">Preview unavailable</div>`; }
  }
  $$('[data-candidate]').forEach(card => {
    card.onclick = () => { runtime.selectedCandidate = Number(card.dataset.candidate); renderCandidates(runtime.currentCandidatePayload); setWaiting(runtime.waiting); };
    if ($("#hover-toggle").checked) card.onmouseenter = () => { const image=card.querySelector("img"); if(image) showArtwork(image.src, card.querySelector(".candidate-meta").textContent, "Candidate Artwork"); };
  });
  $("#compare-button").disabled = visible.length < 1;
  renderCandidateFilters(); renderAdvancedControls(payload); setWaiting(runtime.waiting);
}

function renderCandidateFilters() {
  const categories = [
    ["sources", "Source", [...new Set(runtime.candidates.map(item => item.source).filter(Boolean))]],
    ["types", "Image Type", [...new Set(runtime.candidates.map(item => item.local_origin || (item.acceptable ? "usable" : "rejected"))) ]],
    ["policies", "Policy", [...new Set(runtime.candidates.map(item => item.policy_status).filter(Boolean))]],
    ["ranges", "Resolution", [...new Set(runtime.candidates.map(item => item.range_class).filter(Boolean))]],
  ];
  $("#candidate-filters").innerHTML = categories.map(([key,label,values]) => `<fieldset><legend>${label}</legend>${values.map(value => `<label><input type="checkbox" data-candidate-filter="${key}" value="${escapeHtml(value)}" ${runtime.candidateFilters[key].includes(value) ? "" : "checked"}> ${escapeHtml(value)}</label>`).join("")}</fieldset>`).join("");
  $$('[data-candidate-filter]').forEach(box => box.onchange = () => {
    const key = box.dataset.candidateFilter;
    runtime.candidateFilters[key] = $$(`[data-candidate-filter="${key}"]`).filter(item => !item.checked).map(item => item.value);
    scheduleUiSave(); renderCandidates(runtime.currentCandidatePayload);
  });
  const count = Object.values(runtime.candidateFilters).reduce((sum, values) => sum + values.length, 0);
  $("#candidate-filter-toggle").querySelector("strong").textContent = count ? `${count} active` : "All";
}

function multiFilter(label, values, selected, id) {
  return `<div class="multi-filter"><button data-filter-button="${id}">${label} ▼</button><div class="multi-options hidden" data-filter-options="${id}" tabindex="0">${values.map(value => `<label><input type="checkbox" value="${escapeHtml(value)}" ${selected.includes(value) ? "checked" : ""}> ${escapeHtml(value)}</label>`).join("")}<button data-apply-filter="${id}" class="primary">Apply</button></div></div>`;
}

function renderMusicBrainz() {
  const payload = runtime.musicBrainz;
  if (!payload) return;
  const artistValues = [...new Set((payload.items || []).map(item => item.release_artist || item.recording_artist).filter(Boolean))].sort((a,b)=>a.localeCompare(b));
  const typeValues = releaseTypesForArtists(payload.items || [], runtime.mbArtistFilters);
  runtime.mbTypeFilters = runtime.mbTypeFilters.filter(value => typeValues.includes(value));
  const view = buildMusicBrainzView(payload, runtime.mbArtistFilters, runtime.mbTypeFilters);
  $("#decision-title").textContent = "MusicBrainz Matches";
  $("#decision-content").innerHTML = `
    <p class="subtle">Choose the authoritative release for ${escapeHtml(payload.artist)} — ${escapeHtml(payload.title)}. Current Album stays outside all filtering.</p>
    <div class="mb-authority"><div class="authority-grid"><label>MusicBrainz Artist ID(s)</label><input id="authority-artist" value="${escapeHtml(view.authority.artist_mbids)}"><button id="apply-authority" class="primary">Apply IDs</button><label>MusicBrainz Release ID</label><input id="authority-release" value="${escapeHtml(view.authority.release_mbid)}"><label>MusicBrainz Recording ID</label><input id="authority-recording" value="${escapeHtml(view.authority.recording_mbid)}"></div></div>
    <div class="mb-toolbar">${multiFilter("Filter by Artist",artistValues,runtime.mbArtistFilters,"artist")}${multiFilter("Filter by Release Type",typeValues,runtime.mbTypeFilters,"type")}<span class="subtle">Ctrl selects multiple values · Enter applies</span></div>
    <table class="mb-table"><thead><tr><th style="width:42px">[#]</th><th>Artist</th><th style="width:70px">Country</th><th style="width:90px">Date</th><th style="width:120px">Release Type</th><th>Release</th><th style="width:90px">Resolution</th></tr></thead><tbody>
      <tr class="group-row"><td colspan="7">[CURRENT ALBUM]</td></tr>
      <tr class="authority-row${runtime.selectedMusicBrainz === "authority" ? " selected" : ""}" data-authority-row><td>[*]</td><td>${escapeHtml(view.authority.release_artist)}</td><td></td><td></td><td></td><td>${escapeHtml(view.authority.release_title)}</td><td>—</td></tr>
      ${view.groups.map(group => `<tr class="group-row"><td colspan="7">RELEASE TYPE [${escapeHtml(group.name)}]</td></tr>${group.rows.map(row => {
        const visitClass = row.release_mbid === runtime.lastVisitedRelease || row.current ? " visited-latest" : row.visited ? " visited" : "";
        return `<tr data-release="${escapeHtml(row.release_mbid)}" data-index="${row.index}" class="${runtime.selectedMusicBrainz === row.index ? "selected" : ""}${visitClass}"><td>${row.index}</td><td>${escapeHtml(row.release_artist || row.recording_artist)}</td><td>${escapeHtml(row.country)}</td><td>${escapeHtml(row.release_date)}</td><td>${escapeHtml(row.release_class)}</td><td>${escapeHtml(row.release_title)}</td><td>${escapeHtml(row.resolution || "—")}</td></tr>`;
      }).join("")}`).join("")}
    </tbody></table>
    <div class="dialog-actions"><button id="use-mb-release" ${typeof runtime.selectedMusicBrainz === "number" ? "" : "disabled"}>Use Release</button><button id="open-mb-page" ${runtime.selectedMusicBrainz == null ? "disabled" : ""}>Open MB Page</button><button id="leave-track">Leave Track Unchanged</button></div>`;
  $("#apply-authority").onclick = () => submitDecision({ action:"use_musicbrainz_authority", artist_mbids:$("#authority-artist").value, release_mbid:$("#authority-release").value, recording_mbid:$("#authority-recording").value });
  $("[data-authority-row]").onclick = () => { runtime.selectedMusicBrainz="authority"; renderMusicBrainz(); };
  $$('[data-release]').forEach(row => row.onclick = () => { runtime.selectedMusicBrainz=Number(row.dataset.index); renderMusicBrainz(); });
  $("#use-mb-release").onclick = () => submitDecision({ action:"use_musicbrainz_match", index:runtime.selectedMusicBrainz });
  $("#leave-track").onclick = () => submitDecision({ action:"leave_unchanged" });
  $("#open-mb-page").onclick = async () => {
    const item = typeof runtime.selectedMusicBrainz === "number" ? payload.items.find(row => row.index === runtime.selectedMusicBrainz) : null;
    const release = item?.release_mbid || $("#authority-release").value.trim();
    if (!release) return;
    runtime.lastVisitedRelease = release; if (item) item.visited = true;
    await openUrl(item?.url || `https://musicbrainz.org/release/${encodeURIComponent(release)}`); renderMusicBrainz();
  };
  $$('[data-filter-button]').forEach(button => button.onclick = () => $(`[data-filter-options="${button.dataset.filterButton}"]`).classList.toggle("hidden"));
  $$('[data-filter-options]').forEach(box => {
    box.onkeydown = event => { if (event.key === "Enter") { event.preventDefault(); applyMusicBrainzFilter(box.dataset.filterOptions); } };
    box.querySelectorAll('input').forEach(input => input.onclick = event => {
      if (!event.ctrlKey) box.querySelectorAll('input').forEach(other => { if (other !== input) other.checked=false; });
    });
  });
  $$('[data-apply-filter]').forEach(button => button.onclick = () => applyMusicBrainzFilter(button.dataset.applyFilter));
}

function applyMusicBrainzFilter(id) {
  const selected = $$(`[data-filter-options="${id}"] input:checked`).map(input => input.value);
  if (id === "artist") runtime.mbArtistFilters = selected; else runtime.mbTypeFilters = selected;
  renderMusicBrainz();
}

async function openUrl(url) {
  if (opener?.openUrl) await opener.openUrl(url); else await command("open_url", { url });
}

function handleEvent(payload) {
  const event = payload || {};
  switch (event.event) {
    case "album_started":
      runtime.currentAlbum = runtime.albums.get(event.album_path) || runtime.currentAlbum;
      $("#run-progress").value = event.index - 1; activity(`Album ${event.index}/${event.total}: ${event.album_path}`); break;
    case "album_skipped": activity(`Skipped ${event.album_path}: ${event.reason}`); break;
    case "album_postponed": activity(`Postponed ${event.album_path}`); break;
    case "album_error": activity(`${event.album_path}: ${event.message}`, "error"); break;
    case "musicbrainz_matches":
      runtime.musicBrainz = event; runtime.selectedMusicBrainz = "authority"; runtime.mbArtistFilters=[]; runtime.mbTypeFilters=[]; renderMusicBrainz(); setWaiting(true); break;
    case "musicbrainz_authority_error": activity(event.message, "error"); setWaiting(true); break;
    case "candidates": renderCandidates(event); break;
    case "decision_required": setWaiting(true); activity("Waiting for an artwork decision."); break;
    case "compilation_started": activity(event.message); break;
    case "compilation_track_started":
      runtime.selection = focusTrack(runtime.selection, event.album_path, event.track_path); renderMediaTree();
      if (event.embedded_artwork_path) command("read_image_data_url", { path:event.embedded_artwork_path }).then(data => showArtwork(data, `${event.artist} — ${event.title} · embedded front artwork`, "Selected Track Embedded Artwork")).catch(()=>showArtwork(null,"No embedded artwork","Selected Track Embedded Artwork"));
      else showArtwork(null,"No embedded artwork","Selected Track Embedded Artwork");
      activity(`Compilation track ${event.index}/${event.total}: ${event.artist} — ${event.title}`); break;
    case "compilation_track_skipped": activity(`Resume ledger skipped completed track ${event.index}.`); break;
    case "compilation_track_completed": activity(`Completed ${event.track_path}`, "success"); break;
    case "compilation_track_unresolved": activity(`${event.artist} — ${event.title}: ${event.reason}`, "error"); break;
    case "source_results_cache_hit": activity("Restored cached source results."); break;
    case "album_completed": activity(`Completed ${event.album_path}`, "success"); break;
    case "scan_completed": activity("Scan completed.", "success"); break;
    case "scan_error": runtime.running=false; setLifecycle("LAUNCH"); setWaiting(false); activity(event.message,"error"); notify("Scan ended",event.message); break;
    case "scan_idle": runtime.running=false; setLifecycle("LAUNCH"); setWaiting(false); $("#run-progress").value=$("#run-progress").max; activity("Ready.","success"); loadLibrary(false); break;
    case "update_progress": $("#status-text").textContent = `Downloading signed update${event.total ? ` · ${event.total} bytes` : ""}…`; break;
    case "update_downloaded": activity("Signed update downloaded and verified; standard update transport is starting."); break;
    default: if (event.message) activity(event.message);
  }
}

function openImageOverlay(sources) {
  const overlay = $("#image-overlay");
  overlay.querySelectorAll("img").forEach(image => image.remove());
  sources.filter(Boolean).forEach(source => { const image=document.createElement("img"); image.src=source; image.alt="Full-size artwork preview"; overlay.append(image); });
  overlay.classList.remove("hidden");
}

function backupSelection() {
  return Object.fromEntries($$('[data-backup]').map(box => [box.dataset.backup, box.checked]));
}

async function openCredentials() {
  try {
    const entries = await command("list_credentials");
    $("#credential-select").innerHTML = entries.map(item => `<option value="${escapeHtml(item.name)}">${escapeHtml(item.name)}${item.exists ? " · configured" : ""}</option>`).join("");
    await loadCredential(); $("#credentials-dialog").showModal();
  } catch (error) { notify("Credentials", error); }
}

async function loadCredential() {
  const name = $("#credential-select").value;
  $("#credential-editor").value = await command("read_credential", { name });
  const supported = name === "musicbrainz.json" || name === "lastfm.json";
  $("#authorization-row").classList.toggle("hidden", !supported);
  $("#authorization-code").classList.toggle("hidden", name !== "musicbrainz.json");
}

async function beginCredentialAuthorization() {
  const name = $("#credential-select").value;
  try {
    await command("save_credential", { name, contents:$("#credential-editor").value });
    const url = name === "musicbrainz.json"
      ? await command("begin_musicbrainz_authorization")
      : await command("begin_lastfm_authorization");
    await openUrl(url);
    activity(`${name === "musicbrainz.json" ? "MusicBrainz" : "Last.fm"} authorization opened.`);
  } catch (error) { notify("Authorization could not start", error); }
}

async function completeCredentialAuthorization() {
  const name = $("#credential-select").value;
  try {
    if (name === "musicbrainz.json") await command("complete_musicbrainz_authorization", { code:$("#authorization-code").value });
    else await command("complete_lastfm_authorization");
    await loadCredential(); activity("Provider authorization completed.", "success");
  } catch (error) { notify("Authorization is not complete", error); }
}

async function checkUpdate() {
  try {
    const result = await command("check_for_update");
    if (!result.signed_install_enabled) return notify("SPLINED update check", result.notes);
    if (!result.available) return notify("SPLINED update check", `SPLINED ${result.current_version} is current.`);
    notify("Signed update available", `Installed: ${result.current_version}\nAvailable: ${result.version}\n${result.published_at || ""}\n\n${result.notes || ""}`, { label:"Install Update", run:async()=>{ $("#message-dialog").close(); try{await command("install_update");}catch(error){notify("Update not installed",error);} } });
  } catch (error) { notify("Update check failed", error); }
}

function wireControls() {
  $("#launch-button").onclick = launchScan;
  $("#refresh-library").onclick = () => loadLibrary(true);
  $("#select-all").onclick = () => applySelectMode("all");
  $("#select-filtered").onclick = () => applySelectMode("filtered");
  $("#select-none").onclick = () => applySelectMode("none");
  $("#media-filter-toggle").onclick = () => { $("#media-filters").classList.toggle("hidden"); scheduleUiSave(); };
  $("#candidate-filter-toggle").onclick = () => { $("#candidate-filters").classList.toggle("hidden"); scheduleUiSave(); };
  ["#artist-filter","#album-filter"].forEach(selector => $(selector).oninput = () => { renderMediaTree(); scheduleUiSave(); });
  $$("[data-status]").forEach(box => box.onchange = () => { renderMediaTree(); scheduleUiSave(); });
  $("#show-tracks").onchange = () => { renderMediaTree(); scheduleUiSave(); };
  $("#scan-mode").onchange = scheduleUiSave; $("#auto-scan").onchange=scheduleUiSave; $("#auto-scan-scope").onchange=scheduleUiSave; $("#hover-toggle").onchange=scheduleUiSave;
  $("#theme-select").onchange = () => { applyTheme($("#theme-select").value); scheduleUiSave(); };
  $("#history-back").onclick=()=>moveSelectionHistory(-1); $("#history-forward").onclick=()=>moveSelectionHistory(1);
  $("#use-selected").onclick=()=>submitDecision({action:"use",index:runtime.selectedCandidate,...advancedDecisionValues()});
  $("#skip-button").onclick=()=>submitDecision({action:"bypass"});
  $("#compare-button").onclick=()=>{ const selected=$(`[data-candidate="${runtime.selectedCandidate}"] img`)?.src; const recommended=$(`.candidate-card.recommended img`)?.src; openImageOverlay([...new Set([selected,recommended])]); };
  $("#clear-activity").onclick=()=>$("#activity-log").replaceChildren();
  $("#close-overlay").onclick=()=>$("#image-overlay").classList.add("hidden");
  $("#image-overlay").onclick=event=>{if(event.target===$("#image-overlay"))$("#image-overlay").classList.add("hidden")};
  $$('[data-modal]').forEach(button => button.onclick = () => {
    if (button.dataset.modal === "settings") { $("#config-editor").value=runtime.portable.config_text; $("#settings-dialog").showModal(); }
    else if (button.dataset.modal === "credentials") openCredentials();
    else $(`#${button.dataset.modal}-dialog`).showModal();
  });
  $("#save-config").onclick=async event=>{ event.preventDefault(); try{runtime.portable=await command("save_config",{configText:$("#config-editor").value});$("#settings-dialog").close();activity("Portable Config v5 saved.","success");await loadLibrary(false);}catch(error){notify("Settings were not saved",error);} };
  $("#credential-select").onchange=loadCredential;
  $("#authorize-credential").onclick=beginCredentialAuthorization;
  $("#complete-authorization").onclick=completeCredentialAuthorization;
  $("#save-credential").onclick=async event=>{event.preventDefault();try{await command("save_credential",{name:$("#credential-select").value,contents:$("#credential-editor").value});$("#credentials-dialog").close();activity("Credential saved.","success");}catch(error){notify("Credential was not saved",error);}};
  $("#choose-backup").onclick=async()=>{if(!dialogApi)return;const chosen=await dialogApi.save({filters:[{name:"SPLINED backup",extensions:["spl"]}]});if(chosen)$("#backup-path").value=chosen;};
  $("#export-backup").onclick=async()=>{try{const ui=await captureWindowUi(captureUi());await command("export_backup",{path:$("#backup-path").value,password:$("#backup-password").value,selection:backupSelection(),ui});activity("Selective .spl backup exported.","success");notify("Backup complete","The selected portable state was backed up.");}catch(error){notify("Backup failed",error);}};
  $("#restore-backup").onclick=async()=>{try{runtime.portable=await command("restore_backup",{path:$("#backup-path").value,password:$("#backup-password").value,selection:backupSelection()});runtime.ui=runtime.portable.ui;restoreUi();await loadLibrary(false);$("#backup-dialog").close();activity("Selected .spl categories restored.","success");notify("Restore complete","Selected categories were restored; unselected categories were unchanged.");}catch(error){notify("Restore failed",error);}};
  $("#backup-dialog").addEventListener("close",()=>{if(runtime.portable?.first_run&&!$("#settings-dialog").open){$("#config-editor").value=runtime.portable.config_text;$("#settings-dialog").showModal();}});
  $("#update-button").onclick=checkUpdate;
  window.addEventListener("resize",scheduleUiSave);
  appWindow?.onMoved?.(scheduleUiSave);
  appWindow?.onResized?.(scheduleUiSave);
}

async function start() {
  if (!invoke || !listen) { $("#status-text").textContent="The desktop bridge is unavailable."; return; }
  try {
    runtime.portable = await command("bootstrap"); runtime.ui=runtime.portable.ui;
    restoreUi(); await restoreWindowUi(runtime.ui); wireControls(); pushSelectionHistory();
    await listen("splined-event", event => handleEvent(event.payload));
    if (shouldLoadLibraryAfterBootstrap(runtime.portable.first_run)) await loadLibrary(false);
    $("#app").ariaBusy="false"; activity(runtime.portable.first_run ? "First-run setup is ready." : "Portable settings loaded.","success");
    const associatedBackup = await command("startup_backup_path");
    if (associatedBackup) {
      $("#backup-path").value = associatedBackup;
      $("#backup-dialog").showModal();
    } else if (runtime.portable.first_run) {
      $("#config-editor").value=runtime.portable.config_text; $("#settings-dialog").showModal();
    }
  } catch (error) { notify("SPLINED could not start",error); $("#status-text").textContent=String(error); }
}

start();
