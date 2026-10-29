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
function dialog(title) {
  $("dialogTitle").textContent = title; clear($("dialogBody"));
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
    const body = dialog("Sign in to SPLINED");
    const token = field(body, "WebGUI access token", "", "password");
    actionButton("Sign in", async () => { await post("/api/login", {token: token.value}); $("workspaceDialog").close(); await initialize(); });
    return;
  }
  if (status.setup_required) { await openSettings(); setStatus("Complete library and database setup in Settings"); return; }
  const settings = await api("/api/settings");
  state.enabledSources = (settings.config.sources?.cover_sources || []).filter(key=>!settings.config.sources?.exclude_cover_sources?.includes(key) && settings.config.source_policies?.[key]?.enabled!==false);
  renderAll(); await loadLibrary(); await pollJobs();
  void runAction(checkLibraryChanges);
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
    if (state.reviewJobId && $("workspaceDialog").open) {
      const job = state.jobs.find(item => item.id === state.reviewJobId);
      const key = job?.id + ":" + job?.state + ":" + job?.progress.prompt?.nonce;
      if (job && key !== lastPrompt) { lastPrompt = key; renderJob(job); }
    }
  } finally { jobsPolling = false; }
}
async function reviewJob(id) {
  state.reviewJobId = id; lastPrompt = ""; dialog("Queued work");
  const job = (state.jobs || []).find(item => item.id === id);
  if (job) renderJob(job); await pollJobs();
}
function renderJob(job) {
  const body = dialog("Queued work · " + job.kind);
  body.append(node("p", "", job.payload.name || job.progress.message || job.state));
  if (job.error) body.append(node("p", "operation-error", job.error));
  const prompt = job.progress.prompt;
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
  const context = prompt.context || {}, kind = context.kind;
  body.append(node("p", "", [context.artist, context.album].filter(Boolean).join(" — ") || "Choose how to continue"));
  const decide = async value => { await queueAction("decision", job.id, String(value), prompt.nonce); lastPrompt = ""; await pollJobs(); };
  const options = context.options || job.progress.current?.candidates?.items || [];
  const grid = node("div", "job-options"); body.append(grid);
  for (const [index, option] of options.entries()) {
    const card = node("div", "job-option");
    if (option.image) card.append(image(option.image, "", "Artwork candidate"));
    card.append(node("p", "", [option.artist, option.title || option.name || option.source, option.resolution || (option.width ? option.width + " × " + option.height : ""), option.date, option.range_type].filter(Boolean).join(" · ")));
    actionButton("Choose " + (index + 1), () => decide(option.number || option.index || index + 1), card); grid.append(card);
  }
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
async function openSettings() {
  state.reviewJobId = null;
  const saved = await api("/api/settings"), cfg = saved.config, body = dialog("Settings · Python / PostgreSQL");
  cfg.scan ||= {};
  cfg.scan.shared_artist_art_dir ??= "";
  // This switch controls shared SQLite files in native clients. WebGUI owns
  // a PostgreSQL catalog, so never carry the native-only mode into a save.
  cfg.scan.sqlite_shared = false;
  body.append(node("p", "", "Set paths on the server. Live Write permits artwork and track-tag writes. Secrets already saved are retained when left blank."));
  const inputs = [];
  function section(parent, object, path = []) {
    for (const [name, value] of Object.entries(object)) {
      const keys = [...path, name];
      if (keys.join(".") === "scan.sqlite_shared") continue;
      if (value && typeof value === "object" && !Array.isArray(value)) {
        const details = node("details", "settings-section"); details.open = ["library", "database", "output"].includes(name);
        details.append(node("summary", "", name.replaceAll("_", " "))); parent.append(details); section(details, value, keys);
      } else {
        const type = name === "mode" && !path.length ? "mode" : typeof value === "boolean" ? "checkbox" : typeof value === "number" ? "number" : name === "url" && path[0] === "database" ? "password" : "text";
        const settingName = keys.join(".") === "scan.shared_artist_art_dir"
          ? "V/A / OST Artist Images Directory"
          : keys.join(".") === "scan.cache_dir" ? "Cache Directory"
          : name === "mode" ? "Mode: read or write (Live Write)" : name.replaceAll("_", " ");
        const input = field(parent, settingName, Array.isArray(value) ? JSON.stringify(value) : value, type);
        if (keys.join(".") === "scan.shared_artist_art_dir") {
          input.placeholder = "Blank uses <Cache Directory>/artist-art";
          input.title = "Persistent portraits and banners for V/A, OST, and other shared artist folders";
        }
        if (type === "checkbox") input.checked = value;
        if (saved.database_managed && path[0] === "database") { input.disabled = true; input.placeholder = "Managed by server environment"; }
        inputs.push({input, keys, original: value});
      }
    }
  }
  section(body, cfg);
  actionButton("Save Settings", async () => {
    for (const {input, keys, original} of inputs) {
      let owner = cfg; for (const key of keys.slice(0, -1)) owner = owner[key];
      owner[keys.at(-1)] = typeof original === "boolean" ? input.checked : typeof original === "number" ? Number(input.value) : Array.isArray(original) ? JSON.parse(input.value) : input.value;
    }
    await post("/api/settings", {config: cfg}); await sessionStatus();
    state.enabledSources = (cfg.sources?.cover_sources || []).filter(key=>!cfg.sources?.exclude_cover_sources?.includes(key) && cfg.source_policies?.[key]?.enabled!==false); renderAll(); setStatus("Settings saved");
  });
  actionButton("Full Refresh Library Index", async () => { const job = await post("/api/jobs", {kind: "index", payload: {mode: "full"}}); await reviewJob(job.id); });
  actionButton("Provider Credentials", credentialsMenu);
  const migration = node("details", "settings-section"); migration.append(node("summary", "", "Import an existing SQLite catalog")); body.append(migration);
  migration.append(node("p", "", "Imports a server-side snapshot into an empty PostgreSQL catalog. Paths must match the configured server library. The source remains unchanged."));
  const path = field(migration, "Snapshot path on server");
  actionButton("Import Catalog", async () => { const result = await post("/api/catalog/import", {path: path.value}); setStatus("Imported " + (result.albums || 0) + " albums"); }, migration);
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
setInterval(() => { if (!document.hidden) runAction(pollJobs); }, 1500);
runAction(initialize);

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
