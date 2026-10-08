const SPECIAL_ORDER = new Map([
  ["SOUNDTRACK", 0],
  ["COMPILATION", 1],
  ["VARIOUS ARTISTS", 2],
]);

const text = value => String(value ?? "").trim();
const folded = value => text(value).toLocaleUpperCase();

export function musicBrainzFamily(row) {
  const releaseType = folded(row.release_class ?? row.release_type);
  if (releaseType === "SOUNDTRACK" || releaseType === "COMPILATION") return releaseType;
  if (folded(row.release_artist ?? row.recording_artist) === "VARIOUS ARTISTS") return "VARIOUS ARTISTS";
  return "";
}

function compareText(left, right) {
  return text(left).localeCompare(text(right), undefined, { sensitivity: "base" });
}

function compareDatesNewest(left, right) {
  const a = text(left);
  const b = text(right);
  if (!a && !b) return 0;
  if (!a) return 1;
  if (!b) return -1;
  return b.localeCompare(a);
}

function compareCountries(left, right) {
  const a = folded(left);
  const b = folded(right);
  if (!a && !b) return 0;
  if (!a) return 1;
  if (!b) return -1;
  if (a === "US" && b !== "US") return -1;
  if (b === "US" && a !== "US") return 1;
  return a.localeCompare(b);
}

export function compareMusicBrainzRows(left, right) {
  const leftFamily = musicBrainzFamily(left);
  const rightFamily = musicBrainzFamily(right);
  if (!leftFamily && rightFamily) return -1;
  if (leftFamily && !rightFamily) return 1;
  if (!leftFamily && !rightFamily) {
    const artist = compareText(left.release_artist ?? left.recording_artist, right.release_artist ?? right.recording_artist);
    if (artist) return artist;
  } else {
    const family = SPECIAL_ORDER.get(leftFamily) - SPECIAL_ORDER.get(rightFamily);
    if (family) return family;
  }
  return compareDatesNewest(left.release_date, right.release_date)
    || compareCountries(left.country, right.country)
    || compareText(left.release_title, right.release_title)
    || compareText(left.release_mbid, right.release_mbid);
}

export function filterMusicBrainzRows(rows, artists = [], releaseTypes = []) {
  const artistSet = new Set(artists.map(folded));
  const typeSet = new Set(releaseTypes.map(folded));
  return [...rows]
    .filter(row => !artistSet.size || artistSet.has(folded(row.release_artist ?? row.recording_artist)))
    .filter(row => !typeSet.size || typeSet.has(folded(row.release_class ?? row.release_type)))
    .sort(compareMusicBrainzRows);
}

export function releaseTypesForArtists(rows, artists = []) {
  const artistSet = new Set(artists.map(folded));
  return [...new Set(rows
    .filter(row => !artistSet.size || artistSet.has(folded(row.release_artist ?? row.recording_artist)))
    .map(row => text(row.release_class ?? row.release_type))
    .filter(Boolean))].sort(compareText);
}

export function musicBrainzGroups(rows) {
  const groups = [];
  for (const row of rows) {
    const name = text(row.release_class ?? row.release_type) || "OTHER";
    let group = groups.find(item => folded(item.name) === folded(name));
    if (!group) {
      group = { name, rows: [] };
      groups.push(group);
    }
    group.rows.push(row);
  }
  return groups;
}

export function buildMusicBrainzView(payload, artistFilters = [], typeFilters = []) {
  const authority = {
    authority: true,
    label: "[CURRENT ALBUM]",
    release_type: "",
    recording_mbid: text(payload.authority_recording_mbid),
    artist_mbids: text(payload.authority_artist_mbids),
    release_mbid: text(payload.authority_release_mbid),
    release_title: text(payload.authority_release_title || payload.album),
    release_artist: text(payload.authority_release_artist || payload.album_artist || payload.artist),
  };
  const rows = filterMusicBrainzRows(payload.items ?? [], artistFilters, typeFilters);
  return { authority, groups: musicBrainzGroups(rows), rows };
}

export function createSelectionState() {
  return {
    albumPaths: new Set(),
    bypassOverrides: new Set(),
    explicitTrackPath: "",
    focusedTrackPath: "",
    focusedAlbumPath: "",
    focusedArtist: "",
  };
}

export function shouldLoadLibraryAfterBootstrap(firstRun) {
  return !firstRun;
}

export function canPersistPortableUi(portableState) {
  return Boolean(portableState && !portableState.first_run);
}

export function configuredScanMode(configText) {
  const match = String(configText ?? "").match(/^\s*mode\s*=\s*["'](read|write)["']\s*$/im);
  return match?.[1]?.toLocaleLowerCase() === "write" ? "write" : "read";
}

export function focusTrack(selection, albumPath, trackPath) {
  return { ...selection, focusedAlbumPath: text(albumPath), focusedTrackPath: text(trackPath) };
}

export function checkTrack(selection, albumPath, trackPath, checked) {
  const albums = checked ? new Set([text(albumPath)]) : new Set();
  return {
    ...selection,
    albumPaths: albums,
    bypassOverrides: new Set(),
    focusedAlbumPath: text(albumPath),
    focusedTrackPath: text(trackPath),
    explicitTrackPath: checked ? text(trackPath) : (selection.explicitTrackPath === text(trackPath) ? "" : selection.explicitTrackPath),
  };
}

export function checkAlbum(selection, albumPath, checked) {
  const albums = new Set(selection.albumPaths);
  const bypassOverrides = new Set(selection.bypassOverrides || []);
  if (checked) albums.add(text(albumPath)); else albums.delete(text(albumPath));
  if (!checked) bypassOverrides.delete(text(albumPath));
  return {
    ...selection,
    albumPaths: albums,
    bypassOverrides,
    focusedAlbumPath: text(albumPath),
    explicitTrackPath: "",
  };
}

function normalizedAlbumStatus(album) {
  const status = folded(album?.status).replaceAll(" ", "-");
  if (status.includes("TIMEOUT")) return "timeout";
  if (status.includes("BYPASS")) return "bypassed";
  if (status.includes("INCOMPLETE")) return "incomplete";
  if (status === "PROCESSED" || status === "COMPLETE") return "processed";
  return "unprocessed";
}

export function compilationArtworkPending(album) {
  if (!album?.compilation_track_art_eligible) return false;
  const tracks = album.compilation_tracks || [];
  return normalizedAlbumStatus(album) === "incomplete"
    || tracks.length === 0
    || tracks.some(track => !track.embedded_artwork_recorded);
}

function defaultSelectable(album) {
  const status = normalizedAlbumStatus(album);
  return status === "unprocessed" || status === "incomplete" || compilationArtworkPending(album);
}

function filteredSelectable(album) {
  const status = normalizedAlbumStatus(album);
  return status === "unprocessed" || status === "incomplete" || status === "processed" || compilationArtworkPending(album);
}

function replaceSelection(selection, albums, bypassOverrides = []) {
  return {
    ...selection,
    albumPaths: new Set(albums.map(album => text(album.path))),
    bypassOverrides: new Set(bypassOverrides.map(album => text(album.path))),
    explicitTrackPath: "",
  };
}

export function applyArtistSelection(selection, albums, options = {}) {
  const additive = Boolean(options.additive);
  const includeBypassed = Boolean(options.includeBypassed);
  const candidates = albums.filter(album => {
    const status = normalizedAlbumStatus(album);
    if (status === "timeout") return false;
    if (status === "bypassed") return compilationArtworkPending(album) || includeBypassed;
    return defaultSelectable(album);
  });
  const candidatePaths = new Set(candidates.map(album => text(album.path)));
  const currentlyAllSelected = candidates.length > 0
    && candidates.every(album => selection.albumPaths.has(text(album.path)));
  const paths = additive ? new Set(selection.albumPaths) : new Set();
  const overrides = additive ? new Set(selection.bypassOverrides || []) : new Set();
  if (additive && currentlyAllSelected) {
    for (const path of candidatePaths) {
      paths.delete(path);
      overrides.delete(path);
    }
  } else {
    for (const album of candidates) {
      const path = text(album.path);
      paths.add(path);
      if (normalizedAlbumStatus(album) === "bypassed") overrides.add(path);
    }
  }
  return {
    ...selection,
    albumPaths: paths,
    bypassOverrides: overrides,
    focusedArtist: text(options.artist || albums[0]?.artist || albums[0]?.tagged_artist),
    explicitTrackPath: "",
  };
}

export function applyBulkSelection(selection, albums, mode) {
  if (mode === "none") return replaceSelection(selection, []);
  const predicate = mode === "filtered" ? filteredSelectable : defaultSelectable;
  const selected = albums.filter(album => {
    const status = normalizedAlbumStatus(album);
    if (status === "timeout") return false;
    if (status === "bypassed") return compilationArtworkPending(album);
    return predicate(album);
  });
  return replaceSelection(
    selection,
    selected,
    selected.filter(album => normalizedAlbumStatus(album) === "bypassed"),
  );
}

export function selectAlbum(selection, album, options = {}) {
  const path = text(album?.path);
  const status = normalizedAlbumStatus(album);
  const pending = compilationArtworkPending(album);
  const additive = Boolean(options.additive);
  const alreadySelected = selection.albumPaths.has(path);
  if (additive && alreadySelected) {
    const albums = new Set(selection.albumPaths);
    const overrides = new Set(selection.bypassOverrides || []);
    albums.delete(path);
    overrides.delete(path);
    return { selection: { ...selection, albumPaths: albums, bypassOverrides: overrides, focusedAlbumPath: path, explicitTrackPath: "" }, protection: null };
  }
  if (status === "timeout") return { selection: { ...selection, focusedAlbumPath: path }, protection: "timeout" };
  if (status === "bypassed" && !pending && !options.allowBypass) {
    return { selection: { ...selection, focusedAlbumPath: path }, protection: "bypass" };
  }
  const albums = additive ? new Set(selection.albumPaths) : new Set();
  const overrides = additive ? new Set(selection.bypassOverrides || []) : new Set();
  albums.add(path);
  if (status === "bypassed") overrides.add(path); else overrides.delete(path);
  return {
    selection: {
      ...selection,
      albumPaths: albums,
      bypassOverrides: overrides,
      focusedAlbumPath: path,
      focusedArtist: text(album.artist || album.tagged_artist),
      explicitTrackPath: "",
    },
    protection: null,
  };
}

export function restoreSafeSelection(selection, albums) {
  const byPath = new Map(albums.map(album => [text(album.path), album]));
  const restored = [...selection.albumPaths]
    .map(path => byPath.get(text(path)))
    .filter(Boolean)
    .filter(album => {
      const status = normalizedAlbumStatus(album);
      return status !== "timeout" && (status !== "bypassed" || compilationArtworkPending(album));
    });
  return replaceSelection(
    selection,
    restored,
    restored.filter(album => normalizedAlbumStatus(album) === "bypassed"),
  );
}

export function launchSelection(selection, albums, autoScanScope = "selected") {
  if (autoScanScope !== "all") return selection;
  const paths = new Set(selection.albumPaths);
  const overrides = new Set(selection.bypassOverrides || []);
  for (const album of albums) {
    if (!defaultSelectable(album)) continue;
    const path = text(album.path);
    paths.add(path);
    if (normalizedAlbumStatus(album) === "bypassed" && compilationArtworkPending(album)) {
      overrides.add(path);
    }
  }
  return { ...selection, albumPaths: paths, bypassOverrides: overrides };
}

export function scanRequestFromSelection(selection, options = {}) {
  const albumsByPath = new Map((options.albums || []).map(album => [text(album.path), album]));
  const albumRequests = [...selection.albumPaths].map(path => {
    const album = albumsByPath.get(text(path));
    return {
      path: text(path),
      indexedAlbumPath: text(album?.path || path),
      indexedAlbumKey: text(album?.album_key) || null,
      bypassOverride: selection.bypassOverrides?.has(text(path)) || false,
      compilationTrackPath: selection.focusedAlbumPath === text(path) ? selection.explicitTrackPath || null : null,
    };
  });
  return {
    albums: albumRequests,
    albumPaths: albumRequests.map(album => album.path),
    compilationTrackPath: selection.explicitTrackPath || null,
    mode: options.mode === "write" ? "write" : "read",
    reviewRequired: options.reviewRequired !== false,
    autoIdeal: Boolean(options.autoIdeal),
    fallbackAlbum: null,
    fallbackArtist: null,
  };
}

export function filterCandidates(items, filters = {}) {
  const excludedSources = new Set((filters.sources ?? []).map(folded));
  const excludedTypes = new Set((filters.types ?? []).map(folded));
  const excludedPolicies = new Set((filters.policies ?? []).map(folded));
  const excludedRanges = new Set((filters.ranges ?? []).map(folded));
  return items.filter(item => {
    const type = item.local_origin || (item.acceptable ? "usable" : "rejected");
    return !excludedSources.has(folded(item.source))
      && !excludedTypes.has(folded(type))
      && !excludedPolicies.has(folded(item.policy_status))
      && !excludedRanges.has(folded(item.range_class));
  });
}

export function groupAlbumsByArtist(albums) {
  const groups = new Map();
  for (const album of albums) {
    const artist = text(album.artist || album.tagged_artist) || "Unknown Artist";
    if (!groups.has(artist)) groups.set(artist, []);
    groups.get(artist).push(album);
  }
  return [...groups]
    .sort(([a], [b]) => compareText(a, b))
    .map(([artist, items]) => ({ artist, albums: items.sort((a, b) => compareText(a.title, b.title) || compareText(a.path, b.path)) }));
}

export function albumStatusClass(status) {
  const value = folded(status);
  if (value.includes("TIMEOUT")) return "purple";
  if (value.includes("BYPASS")) return "red";
  if (value.includes("INCOMPLETE")) return "blue";
  if (value === "PROCESSED" || value === "COMPLETE" || value.includes("IDEAL")) return "orange";
  if (value.includes("SELECT")) return "blue";
  return "white";
}

export function artistAggregateStatus(albums) {
  const statuses = albums.map(normalizedAlbumStatus);
  if (statuses.includes("bypassed")) return "contains-bypass";
  if (statuses.includes("incomplete")) return "partial";
  if (statuses.length && statuses.every(status => status === "processed" || status === "timeout")) return "complete";
  if (statuses.some(status => status === "processed" || status === "timeout")) return "partial";
  return "unprocessed";
}

export function artistStatusClass(status) {
  const value = folded(status);
  if (value.includes("BYPASS")) return "blue";
  if (value === "COMPLETE") return "green";
  if (value.includes("PARTIAL")) return "purple";
  return "white";
}

export const statusClass = albumStatusClass;
