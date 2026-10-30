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
  state.enabledSources = (settings.config.sources?.cover_sources || []).filter(key=>!settings.config.sources?.exclude_cover_sources?.includes(key) && settings.config.source_policies?.[key]?.enabled!==false);
  renderAll(); await loadLibrary(); await pollJobs();
  void runAction(checkLibraryChanges);
}
function showAuthentication(status) {
  document.body.className = "auth-required";
  $("adminSetupForm").hidden = !status.admin_setup_required;
  $("loginForm").hidden = status.admin_setup_required;
  $("authTitle").textContent = status.admin_setup_required ? "Create the SPLINED administrator" : "Sign in to SPLINED";
  $("authMessage").textContent = status.admin_setup_required
    ? "Create the first administrator. The deployment's WebGUI token authorizes this one-time enrollment and is not stored in PostgreSQL."
    : "Enter the saved administrator username and password.";
  $("authError").textContent = "";
  (status.admin_setup_required ? $("setupUsername") : $("loginUsername")).focus();
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
  const label = node("label", "setting-field"); label.append(node("span", "", descriptor.label));
  const shell = node("span", "setting-input-shell");
  const input = descriptor.type === "enum" ? node("select") : node("input");
  const existing = settingValue(draft, path, descriptor.default);
  if (descriptor.type === "enum") {
    for (const value of descriptor.choices || []) { const option = node("option", "", value); option.value = value; input.append(option); }
    input.value = existing ?? "";
  } else if (descriptor.type === "boolean") {
    input.type = "checkbox"; input.checked = Boolean(existing);
  } else {
    input.type = descriptor.type === "integer" || descriptor.type === "optional-integer" ? "number"
      : path === "database.url" ? "password" : "text";
    input.value = Array.isArray(existing) ? existing.join(", ") : existing ?? "";
    if (descriptor.min !== undefined) input.min = descriptor.min;
    if (descriptor.max !== undefined) input.max = descriptor.max;
    if (descriptor.required) input.required = true;
  }
  if (saved.database_managed && path === "database.url") { input.disabled = true; input.placeholder = "Managed by server environment"; }
  shell.append(input);
  if (descriptor.suffix) shell.append(node("span", "setting-suffix", descriptor.suffix));
  label.append(shell);
  if (descriptor.help) label.append(node("small", "", descriptor.help));
  host.append(label); bindings.push({input, descriptor, path});
  return input;
}
function applySettingsInputs(draft, bindings) {
  for (const {input, descriptor, path} of bindings) {
    if (input.disabled) continue;
    let value;
    if (descriptor.type === "boolean") value = input.checked;
    else if (descriptor.type === "list") value = input.value.split(",").map(item => item.trim()).filter(Boolean);
    else if (["integer", "optional-integer"].includes(descriptor.type)) {
      if (!input.value.trim() && descriptor.type === "optional-integer") value = undefined;
      else {
        if (!input.value.trim() || !input.checkValidity()) throw new Error(descriptor.label + " is outside its allowed range");
        value = Number(input.value);
      }
    } else value = input.value;
    setSettingValue(draft, path, value);
  }
}
async function openSettings() {
  state.reviewJobId = null;
  let saved, contract;
  [saved, contract] = await Promise.all([api("/api/settings"), api("/api/settings/contract")]);
  const body = dialog("Settings · WebGUI / PostgreSQL", "settings");
  const workspace = node("div", "settings-workspace"), nav = node("nav", "settings-nav"), content = node("section", "settings-content");
  workspace.append(nav, content); body.append(workspace);
  let active = "library", advanced = false, provider = contract.providers[0], draft = null, bindings = [];
  const available = contract.sections.filter(section => !section.runtimes || section.runtimes.includes("webgui"));

  function renderNav() {
    clear(nav); nav.append(node("div", "settings-nav-title", "SETTINGS SECTIONS"));
    for (const section of available) {
      const button = node("button", section.id === active ? "active" : "", section.icon + "  " + section.label); button.type = "button";
      button.addEventListener("click", () => renderSection(section.id)); nav.append(button);
    }
    const advancedButton = node("button", "settings-advanced" + (advanced ? " active" : ""), advanced ? "Advanced ON" : "Advanced");
    advancedButton.type = "button"; advancedButton.addEventListener("click", () => { applySettingsInputs(draft, bindings); advanced = !advanced; renderSection(active, true); }); nav.append(advancedButton);
    if (state.server?.auth_required) {
      const logout = node("button", "", "Sign out"); logout.type = "button";
      logout.addEventListener("click", () => runAction(async () => { await post("/api/logout"); $("workspaceDialog").close(); await initialize(); })); nav.append(logout);
    }
  }
  function addSpecialSection(section, fields) {
    if (section.id === "connections") {
      fields.append(node("h4", "settings-group-heading", "Provider Credentials"));
      const actions = node("div", "settings-actions");
      for (const name of ["navidrome", "fanarttv", "lastfm", "discogs", "musicbrainz"]) actionButton(name, () => editCredentials(name), actions);
      fields.append(actions, node("p", "", "Connection tests do not automatically include a provider in artwork search. Configure inclusion under Artwork Sources."));
    }
    if (section.id === "catalog") {
      fields.append(node("p", "settings-warning", "A full refresh deliberately republishes the complete music catalog. Normal added, changed, and removed files use the faster header update."));
      const actions = node("div", "settings-actions");
      actionButton("Full Refresh Library Index", async () => { $("workspaceDialog").close(); const job = await post("/api/jobs", {kind: "index", payload: {mode: "full"}}); await reviewJob(job.id); }, actions);
      fields.append(actions, node("h4", "settings-group-heading", "Import an existing SQLite catalog"), node("p", "", "Imports a server-side snapshot into an empty PostgreSQL catalog. The source remains unchanged."));
      const path = field(fields, "Snapshot path on server");
      actionButton("Import Catalog", async () => { const result = await post("/api/catalog/import", {path: path.value}); setStatus("Imported " + (result.albums || 0) + " albums"); }, fields);
    }
    if (section.id === "backup") {
      fields.append(node("p", "settings-warning", "Back up PostgreSQL with a consistent pg_dump, then back up config, credentials, cache/staged artwork, and Artist Art volumes. The browser never packages secrets into an export."));
      const actions = node("div", "settings-actions");
      actionButton("Download Safe Settings JSON", async () => {
        const blob = new Blob([JSON.stringify(saved.config, null, 2) + "\n"], {type: "application/json"});
        const link = document.createElement("a"); link.href = URL.createObjectURL(blob); link.download = "splined-settings-safe.json"; link.click(); URL.revokeObjectURL(link.href);
      }, actions);
      actionButton("Download Shared Settings Contract", async () => {
        const blob = new Blob([JSON.stringify(contract, null, 2) + "\n"], {type: "application/json"});
        const link = document.createElement("a"); link.href = URL.createObjectURL(blob); link.download = "splined-settings-ui.json"; link.click(); URL.revokeObjectURL(link.href);
      }, actions);
      fields.append(actions, node("p", "", "The shared contract defines cross-runtime labels, defaults, ranges, and runtime ownership. It is not a credential or database backup."));
    }
  }
  function renderSection(id, preserveDraft = false) {
    active = id; bindings = [];
    if (!preserveDraft) draft = structuredClone(saved.config);
    draft.scan ||= {}; draft.scan.shared_artist_art_dir ??= ""; delete draft.scan.sqlite_shared; delete draft.scan.windows_client_lock;
    const section = available.find(item => item.id === id); clear(content); renderNav();
    const header = node("header", "settings-content-header"), copy = node("div");
    copy.append(node("h3", "", section.label), node("p", "", section.description)); header.append(copy);
    if (!["catalog", "backup"].includes(section.id)) {
      const save = node("button", "", "Save " + section.label); save.type = "button";
      save.addEventListener("click", () => runAction(async () => {
        applySettingsInputs(draft, bindings);
        saved = await post("/api/settings", {config: draft}); await sessionStatus();
        state.enabledSources = (saved.config.sources?.cover_sources || []).filter(key => !saved.config.sources?.exclude_cover_sources?.includes(key) && saved.config.source_policies?.[key]?.enabled !== false);
        renderAll(); setStatus(section.label + " settings saved"); renderSection(active);
      })); header.append(save);
    }
    content.append(header); const fields = node("div", "settings-fields"); content.append(fields);
    for (const descriptor of section.fields || []) {
      if (descriptor.runtimes && !descriptor.runtimes.includes("webgui") || descriptor.advanced && !advanced) continue;
      settingsInput(fields, descriptor, descriptor.path, draft, bindings, saved);
    }
    if (section.repeater) {
      fields.append(node("h4", "settings-group-heading", section.repeater.label));
      const picker = node("label", "settings-provider-picker"); picker.append(node("span", "", "Artwork Provider"));
      const select = node("select");
      for (const name of contract.providers) { const option = node("option", "", name); option.value = name; select.append(option); }
      select.value = provider; select.addEventListener("change", () => { applySettingsInputs(draft, bindings); provider = select.value; renderSection(active, true); }); picker.append(select); fields.append(picker);
      for (const descriptor of section.repeater.fields) settingsInput(fields, descriptor, section.repeater.path + "." + provider + "." + descriptor.path, draft, bindings, saved);
    }
    addSpecialSection(section, fields);
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
