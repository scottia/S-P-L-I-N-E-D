"use strict";

// Browser operations use the same prepared pixels and persistent catalog as writes.
async function runAction(action) {
  try { return await action(); } catch (error) { setStatus(error.message, "error"); }
}
const post = (path, value = {}) => api(path, {method: "POST", headers: {"Content-Type": "application/json"}, body: JSON.stringify(value)});
function actionButton(label, action, host = $("dialogBody")) {
  const button = node("button", "operation-button", label); button.type = "button";
  button.addEventListener("click", () => runAction(async () => {
    button.disabled = true;
    try { await action(); } finally { button.disabled = false; }
  })); host.append(button); return button;
}
function field(host, title, value = "", type = "text") {
  const label = node("label", "setting-field"); label.append(node("span", "", title));
  const input = node(type==='mode'?'select':'input');
  if(type==='mode') for(const [key,title] of [['read','Read · preview artwork'],['write','Live Write · save artwork and tags']]) {const option=node('option','',title);option.value=key;input.append(option);}
  else input.type = type;
  input.value = value; label.append(input); host.append(label); return input;
}
function dialog(title, kind = "") {
  $("dialogTitle").textContent = title; clear($("dialogBody"));
  $("workspaceDialog").classList.toggle("settings-dialog", kind === "settings");
  if (!$("workspaceDialog").open) $("workspaceDialog").showModal();
  return $("dialogBody");
}
async function sessionStatus() {
  const status = await api("/api/status"); state.csrf = status.csrf; state.server = status;
  state.compRepairEnabled = !!status.comp_repair; return status;
}
async function initialize() {
  const status = await sessionStatus();
  if (!status.authenticated) {
    showAuthentication(status);
    return;
  }
  document.body.className = "authenticated";
  if (status.setup_required) { await openSettings(); setStatus("Complete library and database setup in Settings"); return; }
  const settings = await api("/api/settings");
  applyGraphicalEnhancement(settings.config.webgui_ui?.enhancement);
  state.enabledSources = (settings.config.sources?.cover_sources || []).filter(key=>!settings.config.sources?.exclude_cover_sources?.includes(key) && settings.config.source_policies?.[key]?.enabled!==false);
  renderAll(); await loadLibrary(); await pollJobs();
  void runAction(checkLibraryChanges);
}
function showAuthentication(status) {
  document.body.className = "auth-required";
  $("adminSetupForm").hidden = !status.admin_setup_required;
  $("loginForm").hidden = status.admin_setup_required;
  $("passwordRecoveryForm").hidden = true;
  $("authTitle").textContent = status.admin_setup_required ? "Create the SPLINED administrator" : "Sign in to SPLINED";
  $("authMessage").textContent = status.admin_setup_required
    ? "Create the first administrator. The deployment's WebGUI token authorizes enrollment and future password recovery but is not stored in PostgreSQL."
    : "Enter the saved administrator username and password.";
  $("authError").textContent = "";
  (status.admin_setup_required ? $("setupUsername") : $("loginUsername")).focus();
}
function showPasswordRecovery() {
  $("adminSetupForm").hidden = true; $("loginForm").hidden = true; $("passwordRecoveryForm").hidden = false;
  $("authTitle").textContent = "Reset the SPLINED administrator password";
  $("authMessage").textContent = "Enter the administrator username, a new password, and the existing deployment WebGUI token. The token is validated from the container environment and is not changed or stored in PostgreSQL.";
  $("authError").textContent = ""; $("recoveryUsername").value ||= $("loginUsername").value; $("recoveryUsername").focus();
}
async function submitAuthentication(path, payload) {
  $("authError").textContent = "";
  try {
    await post(path, payload);
    $("setupPassword").value = ""; $("setupConfirmation").value = ""; $("setupToken").value = ""; $("loginPassword").value = "";
    await initialize();
  } catch (error) {
    $("authError").textContent = error.message;
  }
}
async function submitPasswordRecovery(payload) {
  $("authError").textContent = "";
  try {
    await post("/api/admin/recover", payload);
    $("recoveryPassword").value = ""; $("recoveryConfirmation").value = ""; $("recoveryToken").value = "";
    const status = await sessionStatus(); showAuthentication(status); $("loginUsername").value = payload.username;
    $("authMessage").textContent = "Administrator password reset. Sign in with the new password."; $("loginPassword").focus();
  } catch (error) {
    $("recoveryPassword").value = ""; $("recoveryConfirmation").value = ""; $("recoveryToken").value = "";
    $("authError").textContent = error.message;
  }
}
async function checkLibraryChanges() {
  const changes = await api("/api/library/changes");
  state.libraryChanges = changes;
  const button = $("applyLibraryChanges");
  button.hidden = !changes.count;
  button.textContent = changes.count
    ? `Apply Library Changes (${changes.count})`
    : "Apply Library Changes";
  if (changes.count) {
    setStatus(`Music library changes · ${changes.added} added · ${changes.changed} changed · ${changes.removed} removed`);
  }
}
function componentPayload(kind, imageURL, extra = {}) {
  const target = kind === "cover" ? "album" : kind;
  return {kind, image: imageURL, adjustments: state.fx[target] || {},
    package_adjustments: state.themedPackaging ? state.fx.package : {}, fit: state.fit[target], ...extra};
}
function coverPayload() {
  if (!state.album) throw new Error("Choose an album first");
  return componentPayload("cover", state.selectedCover?.image || state.album.cover,
    {album_id: state.album.id, upscale: !!state.upscalePreview});
}
async function writeArtwork() {
  if (state.editorMode === "artist" && state.compReportVisible) {
    if (state.compTrack) await enqueueComp(state.compTrack.albumId, [state.compTrack.key]);
    return;
  }
  if (state.editorMode === "fallback") {
    if (state.fallbackTrack) await enqueueComp(state.album.id, [state.fallbackTrack.key]);
    return;
  }
  if (state.editorMode === "album") {
    await post("/api/artwork/cover", coverPayload());
    state.album = await api("/api/album?id=" + encodeURIComponent(state.album.id));
    renderHero(); renderReview(); setStatus("Cover written · " + state.album.name); return;
  }
  if (!state.selectedPortrait && !state.selectedBanner) { enterArtistArt(); return; }
  const components = [];
  if (state.radioArtist && (state.selectedPortrait?.image || state.artist.portrait)) components.push(componentPayload("artist", state.selectedPortrait?.image || state.artist.portrait));
  if (state.radioBanner && (state.selectedBanner?.image || state.artist.banner)) components.push(componentPayload("banner", state.selectedBanner?.image || state.artist.banner));
  if (!components.length) throw new Error("Choose the artist image or banner radio before writing");
  const result = await post("/api/artwork/artist", {artist_id: state.artist.id, components});
  state.artist = await api("/api/artist?id=" + encodeURIComponent(state.artist.id));
  renderHero(); renderPanes(); renderReview(); setStatus("Written: " + result.written.join(", "));
}
const previewReview = renderReview;
renderReview = function () {
  previewReview();
  const entry = state.editorMode === "artist" && !state.compReportVisible && !state.selectedPortrait && !state.selectedBanner;
  const writable = state.server?.mode === "write";
  $("writeButton").disabled = entry ? !state.artist : !writable || (state.editorMode === "album" ? !(state.selectedCover?.image || state.album?.cover)
    : state.editorMode === "fallback" ? !state.fallbackTrack : state.compReportVisible ? !state.compTrack : !((state.radioArtist && (state.selectedPortrait || state.artist?.portrait)) || (state.radioBanner && (state.selectedBanner || state.artist?.banner))));
  $("embedBatchButton").disabled = !writable || state.editorMode !== "album" || !(state.selectedCover?.image || state.album?.cover);
  $("musicbrainzButton").disabled = !state.album || state.editorMode !== "album";
  $("bypassButton").disabled = !state.album || state.editorMode !== "album";
  $("writeNotice").textContent = writable ? "Live Write · WRITE saves artwork. EMBED Batch saves the cover and stages track embedding. COMP work asks for track artwork in queued review." : "Read mode · searching and previewing are enabled. Enable Live Write in Settings to save artwork or track tags.";
  if (state.queuedDecision) {
    $("upscaleButton").disabled = true;
    $("writeButton").disabled = true;
    $("embedBatchButton").disabled = true;
    $("writeNotice").textContent = "Queued review · preview a Candidate Artwork card, then press its Choose button.";
  }
};
async function searchAlbum(releaseId = null) {
  if (!state.album) return;
  const id = state.album.id, request = ++state.searchRequest;
  setStatus("Searching configured artwork sources…", "busy");
  const result = await post("/api/search/album", {id, release_id: releaseId});
  if (request !== state.searchRequest || state.album?.id !== id) return;
  state.candidates = [...(state.album.candidates || []), ...result.candidates]; resetCandidateFilters();
  renderSources(); renderCandidates(); renderReview();
  setStatus(result.candidates.length + " online candidates" + (result.warnings.length ? " · " + result.warnings.join("; ") : ""));
};
async function launchArtist() {
  if (!state.artist) return;
  const ids = [...state.mediaSelected].filter(id => state.artist.albums.some(album => album.id === id && album.album_mbid));
  const result = await post("/api/jobs", {kind: "scan", payload: ids.length ? {album_ids: ids} : {artist_id: state.artist.id}});
  await reviewJob(result.id);
}
async function enqueueComp(albumId, keys) {
  const result = await post("/api/jobs", {kind: "comp", payload: {album_id: albumId, track_keys: keys}});
  await reviewJob(result.id);
}
async function launchFallback() {
  if (!state.album) return;
  await enqueueComp(state.album.id, state.fallbackTracks.filter(track => !track.completed).map(track => track.key));
};
async function launchArtistComp() {
  const groups = state.compGroups.map(group => ({id: group.id, keys: group.tracks.filter(track => track.status === "blue").map(track => track.key)})).filter(group => group.keys.length);
  let first;
  for (const group of groups) {
    const result = await post("/api/jobs", {kind: "comp", payload: {album_id: group.id, track_keys: group.keys}}); first ||= result.id;
  }
  if (first) await reviewJob(first); else setStatus("No blue COMP tracks need artwork");
};
async function stageAlbum() {
  const result = await post("/api/jobs", {kind: "embed-album", payload: coverPayload()});
  await pollJobs(); renderHero(); renderReview();
  setStatus(result.already_queued ? "Album is already staged" : "Cover written and album staged · " + state.album.name);
};
async function queueAction(action, id = null, value = null, nonce = null) {
  const result = await post("/api/jobs/action", {action, id, value, nonce}); await pollJobs(); return result;
}
function queuedCandidate(option, index) {
  const dimensions = String(option.resolution || "").match(/(\d+)\D+(\d+)/);
  return {
    ...option,
    name: option.title || option.name || option.source || "Candidate " + (index + 1),
    source: option.source || "Queued review",
    kind: "cover",
    width: Number(option.width) || Number(dimensions?.[1]) || 0,
    height: Number(option.height) || Number(dimensions?.[2]) || 0,
    queueNumber: index + 1,
    queueValue: option.number ?? option.index ?? index + 1,
  };
}
function clearQueuedDecision() {
  const previous = state.queuedDecision?.previousCandidates || [];
  state.queuedDecision = null;
  state.candidates = previous;
  state.compare.clear();
  resetCandidateFilters();
  renderSources(); renderCandidates(); renderReview();
}
async function submitQueuedDecision(value) {
  const decision = state.queuedDecision;
  if (!decision) throw new Error("This queued decision is no longer current");
  await post("/api/jobs/action", {
    action: "decision", id: decision.jobId, value: String(value), nonce: decision.nonce,
  });
  const label = decision.selected?.queueNumber || value;
  clearQueuedDecision();
  setStatus("Queued choice " + label + " submitted · waiting for the next decision", "busy");
  await pollJobs();
}
function renderQueuedDecisionActions(host) {
  const decision = state.queuedDecision;
  if (!decision) return;
  const actions = node("div", "queued-decision-actions");
  actions.append(node("strong", "", [decision.context.artist, decision.context.album].filter(Boolean).join(" — ") || "Queued artwork review"));
  if (decision.kind !== "musicbrainz-results") actionButton("Use suggested", () => submitQueuedDecision("s"), actions);
  if (["normal-picker", "local-comparison"].includes(decision.kind)) actionButton("Keep / skip album", () => submitQueuedDecision("k"), actions);
  if (decision.kind === "fallback-picker" || decision.context.musicbrainz) actionButton("MusicBrainz matches", () => submitQueuedDecision("m"), actions);
  actionButton(decision.kind === "normal-picker" ? "Bypass album" : "Leave unchanged", () => submitQueuedDecision("b"), actions);
  actionButton("Pause work", async () => { await queueAction("pause", decision.jobId); clearQueuedDecision(); }, actions);
  host.append(actions);
}
function presentQueuedCandidates(job, prompt, options) {
  if ($("workspaceDialog").open) $("workspaceDialog").close();
  const previousCandidates = state.queuedDecision?.previousCandidates || state.candidates;
  const context = prompt.context || {};
  state.queuedDecision = {
    jobId: job.id, nonce: prompt.nonce, kind: context.kind || "", context,
    previousCandidates, selected: null,
  };
  state.workspace = "find";
  state.candidates = options.map(queuedCandidate);
  state.activeSource = ""; state.compare.clear(); resetCandidateFilters();
  renderWorkspace(); renderSources(); renderCandidates(); renderReview();
  setStatus("Queued artwork review · choose a Candidate Artwork card", "busy");
}
function renderQueue() {
  const host = $("embedQueue"); clear(host);
  const jobs = state.jobs || [];
  if (!jobs.length) host.append(node("p", "empty-copy", "No queued work yet."));
  for (const job of jobs) {
    const card = node("div", "queue-card state-" + job.state), copy = node("div", "row-copy");
    copy.append(node("strong", "", job.payload.name || job.kind), node("small", "", job.state + (job.progress.message ? " · " + job.progress.message : "") + (job.progress.total ? " · " + (job.progress.completed || 0) + "/" + job.progress.total : "")));
    if (job.error) copy.append(node("p", "operation-error", job.error)); card.append(copy);
    if (job.state === "waiting") actionButton("Review", () => reviewJob(job.id), card);
    if (["failed", "paused"].includes(job.state)) actionButton("Resume", () => queueAction("retry", job.id), card);
    if (["running", "waiting", "queued"].includes(job.state)) actionButton("Pause", () => queueAction("pause", job.id), card);
    else actionButton("Remove", () => queueAction("remove", job.id), card);
    host.append(card);
  }
  const pending = jobs.filter(job => !["done", "failed", "cancelled"].includes(job.state));
  state.embedQueue = pending.filter(job => job.kind === "embed-album").map(job => ({...job.payload, id: job.payload.album_id}));
  $("embedTab").classList.toggle("has-queued-albums", !!pending.length);
  $("removeAllStaged").disabled = !jobs.some(job => !["running", "waiting"].includes(job.state));
  const waiting = jobs.filter(job => job.state === "waiting");
  $("reviewWork").hidden = !waiting.length; $("reviewWork").textContent = "Review queued work (" + waiting.length + ")";
};
let jobsPolling = false, lastPrompt = "";
async function pollJobs() {
  if (jobsPolling || !state.server?.authenticated || state.server?.setup_required) return;
  jobsPolling = true;
  try {
    state.jobs = await api("/api/jobs"); renderQueue();
    if (state.reviewJobId) {
      const job = state.jobs.find(item => item.id === state.reviewJobId);
      const key = job?.id + ":" + job?.state + ":" + job?.progress.prompt?.nonce;
      if (job && key !== lastPrompt && (job.state === "waiting" || $("workspaceDialog").open)) {
        lastPrompt = key; renderJob(job);
      }
    }
  } finally { jobsPolling = false; }
}
async function reviewJob(id) {
  state.reviewJobId = id; lastPrompt = "";
  const job = (state.jobs || []).find(item => item.id === id);
  if (job) renderJob(job); else dialog("Queued work");
  await pollJobs();
}
function renderJob(job) {
  const prompt = job.progress.prompt;
  const context = prompt?.context || {}, kind = context.kind;
  const contextualOptions = prompt ? context.options || [] : [];
  const currentOptions = prompt ? job.progress.current?.candidates?.items || [] : [];
  const options = currentOptions.some(option => option.image) ? currentOptions : contextualOptions;
  if (job.state === "waiting" && prompt && options.some(option => option.image)) {
    presentQueuedCandidates(job, prompt, options);
    return;
  }
  const body = dialog("Queued work · " + job.kind);
  body.append(node("p", "", job.payload.name || job.progress.message || job.state));
  if (job.error) body.append(node("p", "operation-error", job.error));
  if (job.state !== "waiting" || !prompt) {
    body.append(node("p", "", job.state === "done" ? "Finished" : job.state));
    if(job.result) for(const [key,value] of Object.entries(job.result)) if(typeof value!=="object") body.append(node("p","",key.replaceAll("_"," ")+": "+value));
    if (job.state === "done") {
      if (state.compReportVisible && state.artist) runAction(async () => { state.compGroups = await api("/api/artist/comp-report?id=" + encodeURIComponent(state.artist.id)); renderArtistCompReport(); probeArtistCompStatuses(); });
      actionButton("Next queued decision", async () => { const next = state.jobs.find(item => item.state === "waiting"); if (next) await reviewJob(next.id); else { state.reviewJobId = null; $("workspaceDialog").close(); } });
    }
    if (["paused", "failed"].includes(job.state)) actionButton("Resume", () => queueAction("retry", job.id));
    return;
  }
  body.append(node("p", "", [context.artist, context.album].filter(Boolean).join(" — ") || "Choose how to continue"));
  const decide = async value => { await queueAction("decision", job.id, String(value), prompt.nonce); lastPrompt = ""; await pollJobs(); };
  if (kind !== "musicbrainz-results") actionButton("Use suggested", () => decide("s"));
  if (kind === "normal-picker" || kind === "local-comparison") actionButton("Keep / skip this album", () => decide("k"));
  if (kind === "fallback-picker" || context.musicbrainz) actionButton("MusicBrainz matches", () => decide("m"));
  actionButton(kind === "normal-picker" ? "Bypass album" : "Leave unchanged", () => decide("b"));
  if (kind === "fallback-picker") {
    const details = node("details", "settings-section"); details.append(node("summary", "", "Find by MusicBrainz authority")); body.append(details);
    const artist = field(details, "Artist ID", context.authority?.artist_mbids || "");
    const release = field(details, "Release ID", ""); const recording = field(details, "Recording ID", "");
    actionButton("Search IDs", () => decide(JSON.stringify({action: "manual-authority-query", artist_id: artist.value, release_id: release.value, recording_id: recording.value, edited: "all"})), details);
  }
  actionButton("Pause work", () => queueAction("pause", job.id));
}
function settingValue(object, path, fallback = undefined) {
  let value = object;
  for (const key of path.split(".")) {
    if (!value || typeof value !== "object" || !(key in value)) return fallback;
    value = value[key];
  }
  return value;
}
function setSettingValue(object, path, value) {
  const keys = path.split("."); let owner = object;
  for (const key of keys.slice(0, -1)) owner = owner[key] ||= {};
  if (value === undefined) delete owner[keys.at(-1)]; else owner[keys.at(-1)] = value;
}
function settingsInput(host, descriptor, path, draft, bindings, saved) {
  const label = node("label", "setting-field"), title = node("span", "setting-label", descriptor.label);
  if (descriptor.future) { label.classList.add("future-setting"); title.append(node("span", "future-badge", "FUTURE")); }
  const existing = settingValue(draft, path, descriptor.default);
  if (descriptor.type === "statement") {
    label.classList.add("statement-setting");
    label.append(title, node("span", "setting-statement", descriptor.display ?? (Array.isArray(existing) ? existing.join(" · ") : String(existing ?? ""))));
    if (descriptor.help) label.append(node("small", "", descriptor.help));
    host.append(label); return null;
  }
  const shell = node("span", "setting-input-shell");
  const input = descriptor.type === "enum" ? node("select") : node("input");
  if (descriptor.type === "enum") {
    for (const value of descriptor.choices || []) { const option = node("option", "", value); option.value = value; input.append(option); }
    input.value = existing ?? "";
  } else if (descriptor.type === "boolean") {
    input.type = "checkbox"; input.checked = Boolean(existing); label.classList.add("boolean-setting");
  } else {
    input.type = descriptor.type === "integer" || descriptor.type === "optional-integer" ? "number"
      : descriptor.type === "color" ? "color" : path === "database.url" ? "password" : "text";
    input.value = Array.isArray(existing) ? existing.join(", ") : existing ?? "";
    if (descriptor.min !== undefined) input.min = descriptor.min;
    if (descriptor.max !== undefined) input.max = descriptor.max;
    if (descriptor.required) input.required = true;
    if (descriptor.type === "optional-integer") input.placeholder = "Optional";
  }
  if (saved.database_managed && path === "database.url") { input.disabled = true; input.placeholder = "Managed by server environment"; }
  shell.append(input);
  if (descriptor.suffix) shell.append(node("span", "setting-suffix", descriptor.suffix));
  if (descriptor.type === "boolean") label.append(shell, title); else label.append(title, shell);
  if (descriptor.help) label.append(node("small", "", descriptor.help));
  host.append(label); bindings.push({input, descriptor, path});
  return input;
}
function applySettingsInputs(draft, bindings, strict = true) {
  for (const {input, descriptor, path} of bindings) {
    if (input.disabled) continue;
    let value;
    if (descriptor.type !== "boolean" && input.required && !input.value.trim()) {
      if (strict) throw new Error(descriptor.label + " is required");
      continue;
    }
    if (descriptor.type === "boolean") value = input.checked;
    else if (descriptor.type === "list") value = input.value.split(",").map(item => item.trim()).filter(Boolean);
    else if (["integer", "optional-integer"].includes(descriptor.type)) {
      if (!input.value.trim() && descriptor.type === "optional-integer") value = undefined;
      else {
        if (!input.value.trim() || !input.checkValidity()) {
          if (strict) throw new Error(descriptor.label + " is outside its allowed range");
          continue;
        }
        value = Number(input.value);
      }
    } else value = input.value;
    setSettingValue(draft, path, value);
  }
}
function enhancementValues(draft) {
  return ENHANCEMENT_VALUE_FIELDS.map(([, path, fallback]) => Number(settingValue(draft, path, fallback)));
}
function applyEnhancementValues(draft, values) {
  ENHANCEMENT_VALUE_FIELDS.forEach(([, path, fallback], index) => setSettingValue(draft, path, Number(values[index] ?? fallback)));
}
function hsvToHex(hue, saturation, value) {
  hue = ((hue % 360) + 360) % 360; saturation = Math.max(0, Math.min(1, saturation)); value = Math.max(0, Math.min(1, value));
  const chroma = value * saturation, x = chroma * (1 - Math.abs((hue / 60) % 2 - 1)), match = value - chroma;
  const [red, green, blue] = hue < 60 ? [chroma, x, 0] : hue < 120 ? [x, chroma, 0]
    : hue < 180 ? [0, chroma, x] : hue < 240 ? [0, x, chroma] : hue < 300 ? [x, 0, chroma] : [chroma, 0, x];
  return "#" + [red, green, blue].map(channel => Math.round((channel + match) * 255).toString(16).padStart(2, "0")).join("").toUpperCase();
}
function enhancementPreviewFilter(values) {
  const [picture, sharpen, softness, contrast, exposure, brightness, gamma, color, hue, darkness, tint] = values;
  const bright = Math.max(.35, 1 + (exposure + brightness + gamma - darkness) / 120);
  const contrastValue = Math.max(.4, 1 + (contrast + sharpen - softness) / 100);
  const saturation = Math.max(0, 1 + (picture + Math.abs(color) * .08 + Math.abs(tint) * .05) / 60);
  return `brightness(${bright}) contrast(${contrastValue}) saturate(${saturation}) hue-rotate(${hue + color * .25}deg)`;
}
function renderGraphicalEnhancement(host, draft, rerender) {
  const graphical = normalizeGraphicalEnhancement(settingValue(draft, "webgui_ui.enhancement", {}));
  setSettingValue(draft, "webgui_ui.enhancement", graphical);
  const baseline = GRAPHICAL_ENHANCEMENT_DEFAULTS;
  const panel = node("section", "graphical-enhancement");
  const heading = node("div", "graphical-heading");
  heading.append(node("div", "", "Theme Enhancements"), node("small", "", "Choose a swatch, then click or drag on the OLED Chroma Wheel. Save Enhancement keeps the palette and processing defaults."));
  panel.append(heading);
  const layout = node("div", "graphical-layout"), wheelColumn = node("div", "graphical-wheel-column"), paletteColumn = node("div", "graphical-palette-column"), previewColumn = node("div", "graphical-preview-column");
  const wheel = node("div", "theme-wheel"); wheel.tabIndex = 0; wheel.setAttribute("role", "slider"); wheel.setAttribute("aria-label", "OLED Chroma Wheel");
  const marker = node("span", "theme-wheel-marker"); wheel.append(marker);
  const selectedLine = node("div", "graphical-selected-color"), selectedChip = node("span"), selectedText = node("span", "", "COMPILED background  #3478FF"); selectedLine.append(selectedChip, selectedText);
  const hueLabel = node("label", "graphical-range-label"), hueRange = node("input"); hueRange.type = "range"; hueRange.min = "0"; hueRange.max = "359"; hueRange.value = String(graphical.wheel_hue_shift_degrees); hueLabel.append(node("span", "", `Hue Range · ${hueRange.value}°`), hueRange);
  const brightnessLabel = node("label", "graphical-range-label"), brightnessRange = node("input"); brightnessRange.type = "range"; brightnessRange.min = "30"; brightnessRange.max = "170"; brightnessRange.value = String(graphical.wheel_brightness_percent); brightnessLabel.append(node("span", "", `Brightness · ${brightnessRange.value}%`), brightnessRange);
  wheelColumn.append(node("h4", "", "OLED Chroma Wheel"), wheel, selectedLine, hueLabel, brightnessLabel);

  let selected = null, previewBannerKey = "compiled", previewFrameKey = "complete";
  const previewCard = node("div", "album-card windows-card graphical-album-card");
  const previewBanner = node("span", "album-type-banner"), previewBody = node("span", "album-card-body"), previewPicture = node("span", "album-card-picture");
  previewPicture.append(image("/assets/splined-watermark.png", "", "SPLINED album card preview"));
  previewBody.append(previewPicture, node("strong", "", "Album Card"), node("span", "album-card-foot", "2026   ☆☆☆☆☆")); previewCard.append(previewBanner, previewBody);
  previewColumn.append(node("h4", "", "Album card preview"), previewCard, node("small", "", "Live banner and frame preview"));

  function updatePreview() {
    const style = graphical.banner_styles[previewBannerKey], frame = graphical.frame_styles[previewFrameKey];
    previewBanner.textContent = style.name; previewBanner.style.setProperty("--banner-bg", style.background); previewBanner.style.setProperty("--banner-ink", style.letters); previewBanner.style.borderColor = style.frame;
    previewCard.style.setProperty("--frame-color", frame.color);
    if (selected) { selectedChip.style.background = selected.object[selected.key]; selectedText.textContent = `${selected.label}  ${selected.object[selected.key]}`; }
  }
  function setColor(value) {
    if (!selected || !/^#[0-9a-f]{6}$/i.test(value)) return;
    selected.object[selected.key] = value.toUpperCase();
    if (selected.bannerKey && graphical.banner_sync_frame_letters && ["frame", "letters"].includes(selected.key)) {
      selected.object.frame = selected.object.letters = selected.object[selected.key];
      panel.querySelectorAll(`[data-banner-key="${selected.bannerKey}"][data-color-key="frame"], [data-banner-key="${selected.bannerKey}"][data-color-key="letters"]`).forEach(input => { input.value = selected.object[input.dataset.colorKey]; });
    }
    updatePreview();
  }
  function chooseColor(object, key, label, input, bannerKey = "", frameKey = "") {
    selected = {object, key, label, input, bannerKey, frameKey};
    if (bannerKey) previewBannerKey = bannerKey;
    if (frameKey) previewFrameKey = frameKey;
    panel.querySelectorAll("input[type=color]").forEach(control => control.classList.toggle("active", control === input));
    updatePreview();
  }
  function colorControl(object, key, label, bannerKey = "", frameKey = "") {
    const input = node("input", "graphical-color-input"); input.type = "color"; input.value = object[key]; input.dataset.bannerKey = bannerKey; input.dataset.colorKey = key;
    input.addEventListener("focus", () => chooseColor(object, key, label, input, bannerKey, frameKey));
    input.addEventListener("click", () => chooseColor(object, key, label, input, bannerKey, frameKey));
    input.addEventListener("input", () => { chooseColor(object, key, label, input, bannerKey, frameKey); setColor(input.value); });
    return input;
  }
  const controls = node("div", "graphical-controls");
  for (const [text, key] of [["Synchronize Banner Frame & Lettering", "banner_sync_frame_letters"], ["Random Harmony · Selected Banner", "banner_random_harmony"]]) {
    const label = node("label", "graphical-toggle"), input = node("input"); input.type = "checkbox"; input.checked = !!graphical[key]; input.addEventListener("change", () => graphical[key] = input.checked); label.append(input, node("span", "", text)); controls.append(label);
  }
  paletteColumn.append(node("h4", "", "Banners · Rename And Edit Each Color"), controls);
  const header = node("div", "palette-row palette-header"); for (const value of ["Name (10 max)", "Background", "Frame", "Letters", "Baseline"]) header.append(node("span", "", value)); paletteColumn.append(header);
  for (const [key, style] of Object.entries(graphical.banner_styles)) {
    const row = node("div", "palette-row"), name = node("input"); name.type = "text"; name.maxLength = 10; name.value = style.name;
    name.addEventListener("input", () => { style.name = name.value.toUpperCase(); if (previewBannerKey === key) updatePreview(); });
    const background = colorControl(style, "background", `${style.name} background`, key), frame = colorControl(style, "frame", `${style.name} frame`, key), letters = colorControl(style, "letters", `${style.name} letters`, key);
    const reset = node("button", "palette-reset", "○ Reset"); reset.type = "button"; reset.addEventListener("click", () => { Object.assign(style, cloneGraphicalEnhancement(baseline.banner_styles[key])); name.value = style.name; background.value = style.background; frame.value = style.frame; letters.value = style.letters; updatePreview(); });
    row.append(name, background, frame, letters, reset); paletteColumn.append(row);
  }
  paletteColumn.append(node("h4", "frame-heading", "Album Card Frames"));
  for (const [key, style] of Object.entries(graphical.frame_styles)) {
    const row = node("div", "frame-style-row"), color = colorControl(style, "color", style.name, "", key), reset = node("button", "palette-reset", "○ Reset"); reset.type = "button";
    reset.addEventListener("click", () => { Object.assign(style, cloneGraphicalEnhancement(baseline.frame_styles[key])); color.value = style.color; updatePreview(); });
    row.append(node("span", "", style.name), color, reset); paletteColumn.append(row);
  }
  layout.append(wheelColumn, paletteColumn, previewColumn); panel.append(layout);
  chooseColor(graphical.banner_styles.compiled, "background", "COMPILED background", paletteColumn.querySelector('input[data-banner-key="compiled"][data-color-key="background"]'), "compiled");
  function wheelColor(event) {
    if (!selected) return;
    const box = wheel.getBoundingClientRect(), x = (event.clientX - box.left) / box.width * 2 - 1, y = (event.clientY - box.top) / box.height * 2 - 1;
    const saturation = Math.min(1, Math.hypot(x, y)), hue = (Math.atan2(y, x) * 180 / Math.PI + 360 + Number(graphical.wheel_hue_shift_degrees)) % 360;
    const light = Math.min(1, .3 + Number(graphical.wheel_brightness_percent) / 170 * .7), value = hsvToHex(hue, saturation, light);
    setColor(value); selected.input.value = value;
    marker.style.left = `${(x + 1) * 50}%`; marker.style.top = `${(y + 1) * 50}%`;
    if (selected.bannerKey && selected.key === "background" && graphical.banner_random_harmony) {
      selected.object.frame = hsvToHex(hue + 145, .62, .86); selected.object.letters = hsvToHex(hue + 42, .72, 1);
      panel.querySelectorAll(`[data-banner-key="${selected.bannerKey}"]`).forEach(input => input.value = selected.object[input.dataset.colorKey]);
      updatePreview();
    }
  }
  let wheelDragging = false;
  wheel.addEventListener("pointerdown", event => { wheelDragging = true; wheel.setPointerCapture(event.pointerId); wheelColor(event); });
  wheel.addEventListener("pointermove", event => { if (wheelDragging) wheelColor(event); });
  for (const eventName of ["pointerup", "pointercancel", "lostpointercapture"]) wheel.addEventListener(eventName, () => wheelDragging = false);
  hueRange.addEventListener("input", () => { graphical.wheel_hue_shift_degrees = Number(hueRange.value); hueLabel.firstChild.textContent = `Hue Range · ${hueRange.value}°`; wheel.style.setProperty("--wheel-rotation", `${hueRange.value}deg`); });
  brightnessRange.addEventListener("input", () => { graphical.wheel_brightness_percent = Number(brightnessRange.value); brightnessLabel.firstChild.textContent = `Brightness · ${brightnessRange.value}%`; wheel.style.setProperty("--wheel-brightness", String(Math.max(.45, brightnessRange.value / 100))); });

  const defaultsPanel = node("section", "enhancement-defaults-panel"); defaultsPanel.append(node("h4", "", "Enhancement Defaults"));
  const presetRows = node("div", "preset-rows");
  graphical.adjustment_presets.forEach((preset, index) => {
    const row = node("div", "preset-row"), slot = node("span", "", index ? `Custom ${"ABCD"[index - 1]}` : "Default"), name = node("input"); name.value = preset.name; name.maxLength = 32; name.addEventListener("input", () => preset.name = name.value);
    const summary = node("small", "", preset.saved ? preset.values.join(" · ") : "Not saved yet");
    const save = node("button", "operation-button", "Save Current"); save.type = "button"; save.addEventListener("click", () => { preset.values = enhancementValues(draft); preset.tint_color = settingValue(draft, "output.upscale_tint_color", "#FFB36B"); preset.saved = true; summary.textContent = preset.values.join(" · "); });
    const apply = node("button", "operation-button", "Apply"); apply.type = "button"; apply.disabled = !preset.saved; apply.addEventListener("click", () => { applyEnhancementValues(draft, preset.values); setSettingValue(draft, "output.upscale_tint_color", preset.tint_color || "#FFB36B"); rerender(); });
    row.append(slot, name, summary, save, apply); presetRows.append(row);
  });
  defaultsPanel.append(presetRows);
  const suggest = node("button", "operation-button suggested-values", "Use Suggested Slider Values"); suggest.type = "button"; suggest.addEventListener("click", () => { applyEnhancementValues(draft, GRAPHICAL_ENHANCEMENT_DEFAULTS.adjustment_presets[0].values); rerender(); }); defaultsPanel.append(suggest);
  const compare = node("div", "enhancement-compare"), current = node("div", "enhancement-preview current", "CURRENT"), adjusted = node("div", "enhancement-preview adjusted", "ADJUSTED"); compare.append(current, adjusted); defaultsPanel.append(compare);
  const sliders = node("div", "enhancement-default-sliders");
  ENHANCEMENT_VALUE_FIELDS.forEach(([label, path, fallback, minimum, maximum, suffix]) => {
    const row = node("label", "enhancement-default-slider"), top = node("span"), value = node("b"), input = node("input"); input.type = "range"; input.min = minimum; input.max = maximum; input.value = settingValue(draft, path, fallback);
    const update = () => { value.textContent = `${input.value}${suffix}`; setSettingValue(draft, path, Number(input.value)); adjusted.style.filter = enhancementPreviewFilter(enhancementValues(draft)); };
    top.append(node("span", "", label), value); row.append(top, input); input.addEventListener("input", update); update(); sliders.append(row);
  });
  defaultsPanel.append(sliders); panel.append(defaultsPanel); host.append(panel);
}
function settingsHelp(host, summary, detail) {
  const help = node("details", "settings-help"), heading = node("summary", "", "ⓘ  " + summary);
  help.append(heading, node("p", "", detail)); host.append(help); return help;
}
function formatStorageBytes(value) {
  const bytes = Number(value || 0);
  if (!Number.isFinite(bytes) || bytes < 1) return "0 B";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"], index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return `${(bytes / (1024 ** index)).toFixed(index ? 1 : 0)} ${units[index]}`;
}
function renderStorageSettings(host) {
  host.append(node("h4", "settings-group-heading", "SPLINED File Storage"));
  const summary = node("div", "storage-summary"); summary.append(node("p", "settings-subcopy", "Loading persistent WebGUI storage…")); host.append(summary);
  void api("/api/storage").then(value => {
    clear(summary);
    for (const [label, detail, size] of [
      ["PostgreSQL Catalog", value.database, formatStorageBytes(value.database_bytes)],
      ["Persistent Cache Directory", value.cache, "mounted path"],
      ["V/A / OST Artist Images Directory", value.artist_art, "mounted path"],
      ["Provider Credential Directory", value.credentials, "protected path"],
      ["Configuration File", value.configuration, "Config v5"],
    ]) {
      const row = node("div", "storage-row"); row.append(node("strong", "", label), node("span", "storage-path", detail || "Not configured"), node("span", "storage-size", size)); summary.append(row);
    }
    const runtime = node("div", "storage-runtime"); runtime.append(node("strong", "", "Runtime"), node("span", "", `v${value.version} · PostgreSQL · ${(value.mode || "read").toUpperCase()}`)); summary.append(runtime);
  }).catch(error => { clear(summary); summary.append(node("p", "operation-error", error.message)); });
  settingsHelp(host, "What is included in storage usage?", "The exact catalog size comes from PostgreSQL. Persistent Cache, V/A / OST Artist Images, Provider Credentials, and Configuration are mounted paths; SPLINED does not recursively scan those network folders merely to open Settings.");
}
function renderSourceOrderSettings(host, draft, selectedProvider, selectProvider, rerender) {
  settingsHelp(host, "Why can a connected provider say Excluded?", "Connection credentials and artwork-search inclusion are separate. Excluded providers can still be tested under Connections but are not queried for candidate artwork.");
  host.append(node("h4", "settings-group-heading", "Artwork Search Order"), node("p", "settings-subcopy", "Move a provider left or right. Select a card to edit its candidate quality policy below."));
  const configured = Array.from(settingValue(draft, "sources.cover_sources", [])), excluded = new Set(settingValue(draft, "sources.exclude_cover_sources", []));
  const sourceInfo = new Map(SOURCES.filter(([, key]) => key).map(([label, key, icon]) => [key === "fanart" ? "fanarttv" : key, {label, icon}]));
  const ordered = ["local", ...configured.filter(key => key !== "local")];
  if (!ordered.includes("navidrome")) ordered.push("navidrome");
  const strip = node("div", "settings-source-strip");
  for (const key of ordered) {
    const info = sourceInfo.get(key) || {label: key, icon: key}, configuredIndex = configured.indexOf(key);
    const card = node("div", "settings-source-card" + (selectedProvider === key ? " active" : "") + (excluded.has(key) ? " excluded" : ""));
    const choose = node("button", "source-card-main"); choose.type = "button";
    choose.append(info.icon === "⊘" ? node("span", "source-symbol", "⊘") : image("/assets/source-icons/" + info.icon + ".svg", "", ""), node("strong", "", info.label), node("small", "", excluded.has(key) ? "⊘ Excluded" : key === "local" ? "● Always ready" : "● Ready"));
    choose.disabled = key === "local";
    if (key !== "local") choose.addEventListener("click", () => { selectProvider(key); rerender(); });
    const moves = node("div", "source-move-buttons");
    for (const [label, delta] of [["‹", -1], ["›", 1]]) {
      const move = node("button", "", label); move.type = "button"; move.disabled = configuredIndex < 0 || configuredIndex + delta < 0 || configuredIndex + delta >= configured.length;
      move.addEventListener("click", () => { const next = configuredIndex + delta; [configured[configuredIndex], configured[next]] = [configured[next], configured[configuredIndex]]; setSettingValue(draft, "sources.cover_sources", configured); rerender(); }); moves.append(move);
    }
    card.append(choose, moves); strip.append(card);
  }
  host.append(strip);
}
async function renderConnectionSettings(host, initialProvider) {
  settingsHelp(host, "Does Test enable artwork search?", "No. Test validates the saved provider connection. Include the provider separately under Artwork Sources.");
  host.append(node("h4", "settings-group-heading", "Source Provider Credentials"));
  const picker = node("label", "settings-provider-picker"), select = node("select"); picker.append(node("span", "", "Provider"));
  for (const name of ["navidrome", "fanarttv", "lastfm", "discogs", "musicbrainz"]) { const option = node("option", "", name === "fanarttv" ? "Fanart.tv" : name[0].toUpperCase() + name.slice(1)); option.value = name; select.append(option); }
  select.value = initialProvider; picker.append(select); host.append(picker);
  const panel = node("div", "connection-editor"); host.append(panel);
  async function load(provider) {
    clear(panel); panel.append(node("p", "settings-subcopy", "Loading saved connection…"));
    try {
      const result = await api("/api/credentials/" + provider), inputs = {}; clear(panel);
      for (const item of result.fields) {
        const input = field(panel, item.name.replaceAll("_", " "), item.value, item.secret ? "password" : "text");
        input.placeholder = item.secret && item.configured ? "Saved · leave blank to retain" : item.configured ? "Saved" : "Optional"; inputs[item.name] = input;
      }
      const configured = result.fields.some(item => item.configured);
      panel.append(node("p", configured ? "connection-saved" : "settings-subcopy", configured ? "Saved" : "Not configured"));
      const actions = node("div", "connection-actions");
      actionButton("Reload", () => load(provider), actions);
      actionButton("Test", async () => { const value = await post("/api/credentials/" + provider + "/test"); setStatus(value.message, value.ok ? "ready" : "error"); }, actions);
      actionButton("Save Connection", async () => { await post("/api/credentials/" + provider, Object.fromEntries(Object.entries(inputs).map(([key, input]) => [key, input.value]))); await load(provider); setStatus(provider + " credentials saved"); }, actions);
      panel.append(actions);
    } catch (error) { clear(panel); panel.append(node("p", "operation-error", error.message)); }
  }
  select.addEventListener("change", () => load(select.value)); await load(initialProvider);
}
function downloadJsonFile(name, value) {
  const blob = new Blob([JSON.stringify(value, null, 2) + "\n"], {type: "application/json"});
  const link = document.createElement("a"); link.href = URL.createObjectURL(blob); link.download = name; link.click(); URL.revokeObjectURL(link.href);
}
function renderCatalogMaintenance(host) {
  const maintenance = node("section", "catalog-maintenance");
  maintenance.append(node("h4", "", "↕ Catalog Maintenance"), node("p", "settings-subcopy", "Refresh the PostgreSQL catalog or import a deliberate read-only SQLite snapshot."));
  maintenance.append(node("p", "settings-warning", "A full refresh deliberately republishes the complete music catalog. Normal added, changed, and removed files use the faster header update."));
  const refresh = node("button", "operation-button full-action", "Full Refresh Library Index"); refresh.type = "button";
  refresh.addEventListener("click", () => runAction(async () => { $("workspaceDialog").close(); const job = await post("/api/jobs", {kind: "index", payload: {mode: "full"}}); await reviewJob(job.id); }));
  maintenance.append(refresh, node("h5", "", "Import an existing SQLite catalog"), node("p", "settings-subcopy", "Imports a server-side snapshot into an empty PostgreSQL catalog. The source remains unchanged."));
  const path = field(maintenance, "Snapshot path on server"), importButton = node("button", "operation-button full-action", "Import Catalog"); importButton.type = "button";
  importButton.addEventListener("click", () => runAction(async () => { const result = await post("/api/catalog/import", {path: path.value}); setStatus("Imported " + (result.albums || 0) + " albums"); }));
  maintenance.append(importButton); host.append(maintenance);
}
function renderBackupSettings(host, saved, contract, onRestored) {
  const layout = node("div", "backup-layout"), backup = node("section", "backup-column"), restore = node("section", "backup-column");
  backup.append(node("h4", "", "↑ Backup"), node("p", "settings-subcopy", "WebGUI exports its safe Config v5 settings and interface palette. Provider secrets and PostgreSQL data are deliberately excluded."));
  const contents = node("div", "archive-contents"); for (const label of ["Config V5 Settings", "Interface Settings", "Shared UI Contract"]) contents.append(node("span", "", "●  " + label)); backup.append(contents);
  const safe = node("button", "operation-button full-action", "Back Up Safe Settings JSON"); safe.type = "button"; safe.addEventListener("click", () => downloadJsonFile("splined-settings-safe.json", saved.config));
  const shared = node("button", "operation-button full-action", "Download Shared Settings Contract"); shared.type = "button"; shared.addEventListener("click", () => downloadJsonFile("splined-settings-ui.json", contract));
  const backupActions = node("div", "backup-actions"); backupActions.append(safe, shared); backup.append(backupActions);
  settingsHelp(backup, "Will this preserve sliders, panes, and banner colors?", "Banner colors, frame colors, enhancement presets, and other WebGUI settings are included. Browser pane sizes remain device-local. Provider credentials remain in the credential volume and are not downloaded.");
  backup.append(node("p", "settings-warning", "Back up PostgreSQL separately with a consistent pg_dump and retain the config, credentials, cache/staged-artwork, and Artist Art volumes."));

  restore.append(node("h4", "", "↓ Restore"), node("p", "settings-subcopy", "Restore a safe settings JSON exported by this WebGUI. This does not replace PostgreSQL or provider credentials."));
  const restoreActions = node("div", "backup-actions"), file = node("input"); file.type = "file"; file.accept = ".json,application/json"; file.className = "restore-file"; restoreActions.append(file);
  const button = node("button", "operation-button full-action", "Restore Selected Settings File"); button.type = "button"; button.addEventListener("click", () => runAction(async () => {
    if (!file.files?.length) throw new Error("Choose a safe settings JSON file first");
    const value = JSON.parse(await file.files[0].text());
    if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("The selected file is not a SPLINED settings object");
    const restored = await post("/api/settings", {config: value}); applyGraphicalEnhancement(restored.config.webgui_ui?.enhancement); setStatus("Safe WebGUI settings restored"); await onRestored(restored);
  })); restoreActions.append(button); restore.append(restoreActions);
  settingsHelp(restore, "Can I restore a Windows .spl file here?", "No. Windows .spl archives can contain Windows-only SQLite and interface state. Use the WebGUI safe JSON here, and restore PostgreSQL with PostgreSQL tools.");
  renderCatalogMaintenance(restore);
  layout.append(backup, restore); host.append(layout);
}
function renderRuntimeSettings(host, section, draft, bindings, saved, advanced) {
  const layout = node("div", "runtime-settings-layout"), processing = node("section", "runtime-column"), logging = node("section", "runtime-column");
  processing.append(node("h4", "", "Processing")); settingsHelp(processing, "When can SPLINED change files?", "Only Live Write permits explicit WRITE and EMBED actions. Read mode keeps searching and previewing available without changing media files.");
  logging.append(node("h4", "", "Logging"));
  for (const descriptor of section.fields || []) {
    if (descriptor.runtimes && !descriptor.runtimes.includes("webgui") || descriptor.advanced && !advanced) continue;
    if (["scan.sqlite_shared", "scan.windows_client_lock"].includes(descriptor.path)) continue;
    const target = descriptor.path === "scan.log_dir" || descriptor.path === "verbosity" || descriptor.path.startsWith("logging.") ? logging : processing;
    settingsInput(target, descriptor, descriptor.path, draft, bindings, saved);
  }
  const scope = node("div", "runtime-summary"); scope.append(node("span", "", "Scope  SELECTED / QUEUED"), node("span", "", `Mode  ${(settingValue(draft, "mode", "read") || "read").toUpperCase()}`)); processing.append(scope);
  const logHeading = node("div", "diagnostic-heading"); logHeading.append(node("strong", "", "WebGUI Events")); const refresh = node("button", "operation-button", "Refresh"); refresh.type = "button"; logHeading.append(refresh); logging.append(logHeading);
  const events = node("pre", "diagnostic-events", "Loading events…"); logging.append(events);
  async function load() {
    try { const rows = await api("/api/diagnostics"); events.textContent = rows.length ? rows.map(row => `${row.created_at}  ${row.action}  ${row.subject}${row.details && Object.keys(row.details).length ? "  " + JSON.stringify(row.details) : ""}`).join("\n") : "No recorded WebGUI events yet."; }
    catch (error) { events.textContent = error.message; }
  }
  refresh.addEventListener("click", load); void load(); layout.append(processing, logging); host.append(layout);
}
async function openSettings() {
  state.reviewJobId = null;
  let saved, contract;
  [saved, contract] = await Promise.all([api("/api/settings"), api("/api/settings/contract")]);
  const body = dialog("Settings · WebGUI / PostgreSQL", "settings");
  const workspace = node("div", "settings-workspace"), nav = node("nav", "settings-nav"), content = node("section", "settings-content");
  workspace.append(nav, content); body.append(workspace);
  let active = "library", advanced = false, sourceProvider = contract.providers[0], credentialProvider = "navidrome", draft = null, bindings = [];
  const available = contract.sections.filter(section => (!section.runtimes || section.runtimes.includes("webgui")) && section.id !== "catalog");

  function renderNav() {
    clear(nav); nav.append(node("div", "settings-nav-title", "SETTINGS SECTIONS"));
    for (const section of available) {
      const button = node("button", section.id === active ? "active" : "", section.icon + "  " + section.label); button.type = "button";
      button.addEventListener("click", () => renderSection(section.id)); nav.append(button);
    }
    const helpLink = node("a", "settings-help-link", "Settings HELP");
    helpLink.href = "https://github.com/scottia/S-P-L-I-N-E-D/blob/main/docs/config-v5-reference.md"; helpLink.target = "_blank"; helpLink.rel = "noopener"; nav.append(helpLink);
    const advancedButton = node("button", "settings-advanced" + (advanced ? " active" : ""), advanced ? "Advanced ON · All Sections" : "Advanced OFF · All Sections");
    advancedButton.type = "button"; advancedButton.addEventListener("click", () => { applySettingsInputs(draft, bindings, false); advanced = !advanced; renderSection(active, true); }); nav.append(advancedButton);
    if (state.server?.auth_required) {
      const logout = node("button", "", "Sign out"); logout.type = "button";
      logout.addEventListener("click", () => runAction(async () => { await post("/api/logout"); $("workspaceDialog").close(); await initialize(); })); nav.append(logout);
    }
  }
  function addSpecialSection(section, fields) {
    if (section.id === "enhancement") renderGraphicalEnhancement(fields, draft, () => renderSection(active, true));
    if (section.id === "sources") renderSourceOrderSettings(fields, draft, sourceProvider, value => sourceProvider = value, () => renderSection(active, true));
    if (section.id === "connections") {
      void renderConnectionSettings(fields, credentialProvider);
    }
    if (section.id === "backup") {
      renderBackupSettings(fields, saved, contract, async restored => { saved = restored; await sessionStatus(); renderAll(); renderSection("backup"); });
    }
  }
  async function saveCurrentSection(section, message = section.label + " settings saved") {
    applySettingsInputs(draft, bindings);
    saved = await post("/api/settings", {config: draft}); await sessionStatus();
    applyGraphicalEnhancement(saved.config.webgui_ui?.enhancement);
    state.enabledSources = (saved.config.sources?.cover_sources || []).filter(key => !saved.config.sources?.exclude_cover_sources?.includes(key) && saved.config.source_policies?.[key]?.enabled !== false);
    renderAll(); setStatus(message); renderSection(active);
  }
  function renderSection(id, preserveDraft = false) {
    active = id; bindings = [];
    if (!preserveDraft) draft = structuredClone(saved.config);
    draft.scan ||= {}; draft.scan.shared_artist_art_dir ??= ""; draft.scan.temporary_cache_dir ??= draft.scan.cache_dir || ""; delete draft.scan.sqlite_shared; delete draft.scan.windows_client_lock;
    draft.samples ||= {}; draft.aisplined ||= {};
    const section = available.find(item => item.id === id); clear(content); renderNav();
    const header = node("header", "settings-content-header"), copy = node("div");
    copy.append(node("h3", "", section.label), node("p", "", section.description)); header.append(copy);
    if (section.id !== "backup") {
      const save = node("button", "", "Save " + section.label); save.type = "button";
      save.addEventListener("click", () => runAction(() => saveCurrentSection(section))); header.append(save);
    }
    content.append(header); const fields = node("div", `settings-fields settings-fields-${section.id}`); content.append(fields);
    if (["enhancement", "sources"].includes(section.id)) addSpecialSection(section, fields);
    if (section.id === "library") settingsHelp(fields, "Which folders does SPLINED use?", "Music Library is the default scan root. Scan Mode may select a narrower server-visible folder. Persistent Cache, V/A / OST Artist Images, and Provider Credential directories must use persistent server-visible storage.");
    if (section.id === "artwork") settingsHelp(fields, "What is saved, and what is embedded?", "WRITE saves cover.*, artist.*, or banner.* files. EMBED Batch also stages a preference-sized square thumbnail for supported tracks; it does not shrink the saved full-resolution cover.");
    if (section.id === "runtime") renderRuntimeSettings(fields, section, draft, bindings, saved, advanced);
    else {
      let displayedGroup = "";
      const groupFor = path => section.id === "library" ? (["library.music_library", "scan.scan_mode", "scan.scan_library_dir"].includes(path) ? "Library And Application Paths" : path === "database.url" ? "Database" : "Persistent Folders")
        : section.id === "artwork" ? (path.startsWith("output.") ? "Output Files" : path === "scan.shared_artist_art_dir" ? "V/A / OST Artist Images Directory" : "Sample Images")
        : section.id === "enhancement" ? "Resolution Range" : "";
      for (const descriptor of section.fields || []) {
        if (descriptor.runtimes && !descriptor.runtimes.includes("webgui") || descriptor.advanced && !advanced) continue;
        const group = groupFor(descriptor.path);
        if (group && group !== displayedGroup) { fields.append(node("h4", "settings-group-heading", group)); displayedGroup = group; }
        settingsInput(fields, descriptor, descriptor.path, draft, bindings, saved);
      }
      if (section.id === "artwork") {
        const sampleToggle = bindings.find(item => item.path === "samples.sample_write")?.input;
        const sampleDirectory = bindings.find(item => item.path === "samples.sample_dir")?.input;
        if (sampleToggle && sampleDirectory) {
          const updateSamplePath = () => { sampleDirectory.disabled = !sampleToggle.checked; sampleDirectory.required = sampleToggle.checked; sampleDirectory.placeholder = sampleToggle.checked ? "Choose a persistent directory" : "Enable Write Sample Cover Images to set a path"; };
          sampleToggle.addEventListener("change", updateSamplePath); updateSamplePath();
        }
      }
      if (section.id === "library") {
        const scanToggle = bindings.find(item => item.path === "scan.scan_mode")?.input;
        const scanDirectory = bindings.find(item => item.path === "scan.scan_library_dir")?.input;
        if (scanToggle && scanDirectory) {
          const scanField = scanDirectory.closest(".setting-field");
          const updateScanPath = () => { scanField.hidden = !scanToggle.checked; scanDirectory.disabled = !scanToggle.checked; scanDirectory.required = false; scanDirectory.placeholder = "Optional"; };
          scanToggle.addEventListener("change", updateScanPath); updateScanPath();
        }
        renderStorageSettings(fields);
      }
    }
    if (section.repeater) {
      fields.append(node("h4", "settings-group-heading", section.repeater.label));
      const picker = node("label", "settings-provider-picker"); picker.append(node("span", "", "Artwork Provider"));
      const select = node("select");
      for (const name of contract.providers) { const option = node("option", "", name); option.value = name; select.append(option); }
      select.value = sourceProvider; select.addEventListener("change", () => { applySettingsInputs(draft, bindings); sourceProvider = select.value; renderSection(active, true); }); picker.append(select); fields.append(picker);
      for (const descriptor of section.repeater.fields) settingsInput(fields, descriptor, section.repeater.path + "." + sourceProvider + "." + descriptor.path, draft, bindings, saved);
    }
    if (section.id === "sources") {
      settingsHelp(fields, "What does source policy change?", "This policy applies only to the selected provider. Optional dimensions remain unset until you type a value or use the number arrows; a blank field does not impose a limit.");
      const update = node("button", "operation-button full-action", "Update Selected Source Policy"); update.type = "button";
      update.addEventListener("click", () => runAction(() => saveCurrentSection(section, `${sourceProvider} source policy updated`))); fields.append(update);
    }
    if (!["enhancement", "sources", "runtime"].includes(section.id)) addSpecialSection(section, fields);
  }
  renderSection(active);
}
async function credentialsMenu() {
  const body = dialog("Provider Credentials");
  for (const provider of ["navidrome", "fanarttv", "lastfm", "discogs", "musicbrainz"]) actionButton(provider, () => editCredentials(provider), body);
  actionButton("Back to Settings", openSettings);
}
async function editCredentials(provider) {
  const result = await api("/api/credentials/" + provider), body = dialog(provider + " credentials"), inputs = {};
  body.append(node("p", "", "Blank secret fields retain saved values. Test validates the saved credentials."));
  for (const item of result.fields) { const input = field(body, item.name.replaceAll("_", " "), item.value, item.secret ? "password" : "text"); input.placeholder = item.configured ? "Saved" : "Not configured"; inputs[item.name] = input; }
  actionButton("Save Credentials", async () => { await post("/api/credentials/" + provider, Object.fromEntries(Object.entries(inputs).map(([key, input]) => [key, input.value]))); setStatus(provider + " credentials saved"); });
  actionButton("Test Connection", async () => { const value = await post("/api/credentials/" + provider + "/test"); setStatus(value.message, value.ok ? "ready" : "error"); });
  if (provider==='musicbrainz') actionButton('Authorize MusicBrainz',async()=>{
    const flow=await post('/api/credentials/musicbrainz/authorize');
    const area=dialog('Authorize MusicBrainz');
    const link=node('a','guide-link','Open MusicBrainz authorization');link.href=flow.url;link.target='_blank';link.rel='noopener';area.append(link);
    area.append(node('p','','Approve in MusicBrainz, then paste its authorization code below.'));
    const code=field(area,'Authorization code');
    actionButton('Complete authorization',async()=>{await post('/api/credentials/musicbrainz/complete',{code:code.value,state:flow.state});setStatus('MusicBrainz authorization saved');await editCredentials(provider);});
  });
  actionButton("Back", credentialsMenu);
}
async function chooseMusicBrainz() {
  if (!state.album) return;
  const id = state.album.id, body = dialog("MusicBrainz release search");
  const artist = field(body, "Artist", state.album.artist_name), title = field(body, "Album", state.album.name);
  const results = node("div", "job-options"); body.append(results);
  actionButton("Search Releases", async () => {
    clear(results); const matches = await post("/api/musicbrainz", {album_id: id, artist: artist.value, title: title.value});
    if (!matches.length) results.append(node("p", "", "No matching releases"));
    for (const match of matches) {
      const card = node("div", "job-option"); card.append(node("p", "", [match.artist, match.title, match.date, match.country, match.resolution].filter(Boolean).join(" · ")));
      actionButton("Search Artwork", async () => { $("workspaceDialog").close(); await searchAlbum(match.id); }, card); results.append(card);
    }
  });
}
async function upscaleArtist() {
  const components = state.radioBanner ? ["banner"] : ["artist"];
  for (const kind of components) {
    const source = kind === "banner" ? state.selectedBanner?.image || state.artist?.banner : state.selectedPortrait?.image || state.artist?.portrait;
    if (!source) throw new Error("Choose artist artwork before upscaling");
    const result = await post("/api/preview/upscale", componentPayload(kind, source, {package_adjustments:{}}));
    const candidate = {...result, name: "Upscaled " + kind, source: "Prepared", kind};
    if (kind === "banner") state.selectedBanner = candidate; else state.selectedPortrait = candidate;
    state.fx[kind] = {}; state.fit[kind] = {zoom: 1, x: 0, y: 0};
  }
  renderPanes(); renderReview(); setStatus("Upscaled artwork ready to write");
}
$("startStaged").addEventListener("click", () => runAction(() => queueAction("start")));
$("pauseAfterAlbum").addEventListener("click", () => runAction(() => queueAction("pause-after")));
$("pauseAllAlbums").addEventListener("click", () => runAction(() => queueAction("pause")));
$("reviewWork").addEventListener("click", () => runAction(() => reviewJob(state.jobs.find(job => job.state === "waiting").id)));
$("applyLibraryChanges").addEventListener("click", () => runAction(async () => {
  $("applyLibraryChanges").hidden = true;
  const job = await post("/api/jobs", {kind: "index", payload: {mode: "incremental"}});
  await reviewJob(job.id);
}));
$("closeDialog").addEventListener("click", () => { state.reviewJobId = null; });
$("workspaceDialog").addEventListener("cancel", () => { state.reviewJobId = null; });
$("musicbrainzButton").addEventListener("click", () => runAction(chooseMusicBrainz));
$("bypassButton").addEventListener("click", () => runAction(async () => {
  if (!state.album) return;
  const result = await post("/api/bypass", {album_id: state.album.id, enabled: state.album.status !== "bypassed"});
  state.album.status = result.bypassed ? "bypassed" : "unprocessed"; renderHero(); setStatus(result.bypassed ? "Album bypassed" : "Album bypass removed");
}));
$("embedAllAlbums").addEventListener("click", () => runAction(async () => {
  if (!state.artist) return;
  let count = 0;
  for (const album of state.artist.albums.filter(item => item.album_mbid && item.cover)) {
    await post("/api/jobs", {kind: "embed-album", payload: {album_id: album.id, image: album.cover}}); count++;
  }
  await pollJobs(); setStatus(count + " albums staged");
}));
$("uploadArtwork").addEventListener("click", () => {
  const input = node("input"); input.type = "file"; input.accept = "image/jpeg,image/png,image/webp";
  input.addEventListener("change", () => runAction(async () => {
    const file = input.files[0]; if (!file) return;
    if (file.size > 25 * 1024 * 1024) throw new Error("Choose an image smaller than 25 MB");
    const data = await new Promise((resolve, reject) => { const reader = new FileReader(); reader.onload = () => resolve(String(reader.result).split(",")[1]); reader.onerror = reject; reader.readAsDataURL(file); });
    const candidate = await post("/api/artwork/upload", {name: file.name, data});
    candidate.kind = state.editorMode === "artist" ? state.radioBanner ? "banner" : "artist" : "cover";
    state.candidates.unshift(candidate); renderCandidates(); selectCandidate(candidate, candidate.kind === "cover" ? "album" : candidate.kind);
  })); input.click();
});
$("adminSetupForm").addEventListener("submit", event => {
  event.preventDefault();
  submitAuthentication("/api/admin/setup", {
    username: $("setupUsername").value,
    password: $("setupPassword").value,
    confirmation: $("setupConfirmation").value,
    token: $("setupToken").value,
  });
});
$("loginForm").addEventListener("submit", event => {
  event.preventDefault();
  submitAuthentication("/api/login", {
    username: $("loginUsername").value,
    password: $("loginPassword").value,
  });
});
$("forgotPasswordButton").addEventListener("click", showPasswordRecovery);
$("cancelPasswordRecovery").addEventListener("click", () => showAuthentication(state.server));
$("passwordRecoveryForm").addEventListener("submit", event => {
  event.preventDefault();
  submitPasswordRecovery({
    username: $("recoveryUsername").value,
    password: $("recoveryPassword").value,
    confirmation: $("recoveryConfirmation").value,
    token: $("recoveryToken").value,
  });
});
setInterval(() => { if (!document.hidden) runAction(pollJobs); }, 1500);
initialize().catch(error => { document.body.className = "auth-required"; $("authError").textContent = error.message; });

async function repairFallback() {
  if (!state.album || !state.artist) return;
  let repaired=0;
  for (const track of state.fallbackTracks.filter(item=>item.completed)) {
    const status=await api('/api/fallback/track-status?id='+encodeURIComponent(state.album.id)+'&key='+encodeURIComponent(track.key));
    if (status.status==='purple' && status.repairable) {
      await post('/api/artist/comp-repair',{artist_id:state.artist.id,album_id:state.album.id,track_key:track.key});repaired++;
    }
  }
  setStatus(repaired+' COMP thumbnails repaired');
}
const quickPreview=applyPreviewEffects;
let previewTimer,previewGeneration=0;
applyPreviewEffects=function () {
  quickPreview();clearTimeout(previewTimer);const generation=++previewGeneration;
  if (!state.server?.authenticated) return;
  previewTimer=setTimeout(()=>runAction(async()=>{
    const requests=[];
    const add=(selector,kind,url,extra={})=>{
      const element=document.querySelector(selector);if (!element || !url) return;
      requests.push((async()=>{
        const result=await post('/api/preview/render',componentPayload(kind,url,extra));
        if (generation!==previewGeneration || !element.isConnected) return;
        element.src=result.image;element.style.filter='none';element.style.transform='none';
      })());
    };
    if (state.editorMode==='album') {
      const source=state.selectedCover?.image||state.album?.cover;
      add('#selectedFrame .selected-image','cover',source,{upscale:!!state.upscalePreview});
      add('#albumPane img','cover',source,{upscale:!!state.upscalePreview});
    } else if (state.editorMode==='artist' && !state.compReportVisible) {
      const portrait=state.selectedPortrait?.image||state.artist?.portrait,banner=state.selectedBanner?.image||state.artist?.banner;
      add('#artistPane img','artist',portrait);add('#bannerPane img','banner',banner);
      if (!state.radioCurrent) {
        if (state.radioArtist && state.radioBanner) {add('#selectedFrame .portrait-art','artist',portrait);add('#selectedFrame .banner-art','banner',banner);}
        else add('#selectedFrame .selected-image',state.radioBanner?'banner':'artist',state.radioBanner?banner:portrait);
      }
    }
    await Promise.all(requests);
  }),220);
};
