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
  return { albumPaths: new Set(), explicitTrackPath: "", focusedTrackPath: "", focusedAlbumPath: "" };
}

export function shouldLoadLibraryAfterBootstrap(firstRun) {
  return !firstRun;
}

export function canPersistPortableUi(portableState) {
  return Boolean(portableState && !portableState.first_run);
}

export function focusTrack(selection, albumPath, trackPath) {
  return { ...selection, focusedAlbumPath: text(albumPath), focusedTrackPath: text(trackPath) };
}

export function checkTrack(selection, albumPath, trackPath, checked) {
  const albums = new Set(selection.albumPaths);
  if (checked) albums.add(text(albumPath));
  return {
    ...selection,
    albumPaths: albums,
    focusedAlbumPath: text(albumPath),
    focusedTrackPath: text(trackPath),
    explicitTrackPath: checked ? text(trackPath) : (selection.explicitTrackPath === text(trackPath) ? "" : selection.explicitTrackPath),
  };
}

export function checkAlbum(selection, albumPath, checked) {
  const albums = new Set(selection.albumPaths);
  if (checked) albums.add(text(albumPath)); else albums.delete(text(albumPath));
  return {
    ...selection,
    albumPaths: albums,
    focusedAlbumPath: text(albumPath),
    explicitTrackPath: "",
  };
}

export function scanRequestFromSelection(selection, options = {}) {
  return {
    albumPaths: [...selection.albumPaths],
    compilationTrackPath: selection.explicitTrackPath || null,
    mode: options.mode === "write" ? "write" : "read",
    reviewRequired: options.reviewRequired !== false,
    autoIdeal: Boolean(options.autoIdeal),
    fallbackAlbum: null,
    fallbackArtist: null,
    indexedAlbumPath: selection.focusedAlbumPath || null,
    indexedAlbumKey: options.indexedAlbumKey || null,
    bypassOverrides: options.bypassOverrides || [],
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

export function statusClass(status) {
  const value = folded(status);
  if (value.includes("TIMEOUT")) return "purple";
  if (value.includes("BYPASS")) return "orange";
  if (value.includes("COMPLETE") || value.includes("IDEAL")) return "green";
  if (value.includes("INCOMPLETE") || value.includes("MISSING")) return "red";
  if (value.includes("SELECT")) return "blue";
  return "white";
}
