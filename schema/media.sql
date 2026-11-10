PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS artists (
    artist_key TEXT PRIMARY KEY COLLATE NOCASE,
    artist_name TEXT NOT NULL,
    artist_sort TEXT,
    musicbrainz_artistid TEXT,
    primary_path TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'unprocessed',
    album_count INTEGER NOT NULL DEFAULT 0,
    unprocessed_count INTEGER NOT NULL DEFAULT 0,
    processed_count INTEGER NOT NULL DEFAULT 0,
    bypassed_count INTEGER NOT NULL DEFAULT 0,
    timeout_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    splined_version TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS artist_art_profiles (
    artist_name TEXT PRIMARY KEY COLLATE NOCASE,
    lookup_name TEXT NOT NULL DEFAULT '',
    musicbrainz_artistid TEXT NOT NULL DEFAULT '',
    biography TEXT NOT NULL DEFAULT '',
    external_url TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL,
    splined_version TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_artist_art_profiles_mbid
    ON artist_art_profiles(musicbrainz_artistid)
    WHERE trim(musicbrainz_artistid) <> '';

CREATE TABLE IF NOT EXISTS albums (
    album_key TEXT PRIMARY KEY COLLATE NOCASE,
    artist_key TEXT NOT NULL,
    album_name TEXT NOT NULL,
    album_sort TEXT,
    musicbrainz_albumid TEXT,
    musicbrainz_releasegroupid TEXT,
    release_year TEXT,
    compilation INTEGER NOT NULL DEFAULT 0,
    path TEXT NOT NULL,
    representative_file TEXT,
    representative_size INTEGER,
    representative_mtime_ns INTEGER,
    tag_signature TEXT NOT NULL,
    track_count INTEGER NOT NULL DEFAULT 0,
    inventory_fingerprint TEXT,
    status TEXT NOT NULL DEFAULT 'unprocessed',
    cover_required INTEGER NOT NULL DEFAULT 1,
    cover_found INTEGER NOT NULL DEFAULT 0,
    cover_path TEXT,
    cover_name TEXT,
    cover_format TEXT,
    cover_width INTEGER,
    cover_height INTEGER,
    artwork_jpeg INTEGER NOT NULL DEFAULT 0,
    artwork_png INTEGER NOT NULL DEFAULT 0,
    artwork_webp INTEGER NOT NULL DEFAULT 0,
    artwork_other INTEGER NOT NULL DEFAULT 0,
    root_files INTEGER NOT NULL DEFAULT 0,
    cover_files INTEGER NOT NULL DEFAULT 0,
    cover_names_json TEXT NOT NULL DEFAULT '[]',
    local_art_json TEXT NOT NULL DEFAULT '[]',
    other_filenames_json TEXT NOT NULL DEFAULT '[]',
    webp_found INTEGER NOT NULL DEFAULT 0,
    webp_size_mb REAL NOT NULL DEFAULT 0,
    webp_resolution TEXT,
    webp_conversion INTEGER NOT NULL DEFAULT 0,
    processed_at TEXT,
    bypassed INTEGER NOT NULL DEFAULT 0,
    timeout_until TEXT,
    selected_source TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    splined_version TEXT NOT NULL,
    FOREIGN KEY (artist_key)
        REFERENCES artists(artist_key)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS tracks (
    track_key TEXT PRIMARY KEY COLLATE NOCASE,
    album_key TEXT NOT NULL,
    path TEXT NOT NULL,
    title TEXT NOT NULL,
    artist_name TEXT NOT NULL,
    musicbrainz_recordingid TEXT,
    musicbrainz_artistid TEXT,
    file_size INTEGER NOT NULL,
    file_mtime_ns INTEGER NOT NULL,
    updated_at TEXT NOT NULL,
    splined_version TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS recording_release_lookups (
    recording_mbid TEXT PRIMARY KEY COLLATE NOCASE,
    artist_mbids_key TEXT NOT NULL,
    fetched_at TEXT NOT NULL,
    splined_version TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS recording_release_candidates (
    recording_mbid TEXT NOT NULL COLLATE NOCASE,
    release_mbid TEXT NOT NULL COLLATE NOCASE,
    release_group_mbid TEXT,
    release_class TEXT NOT NULL CHECK (
        release_class IN ('album', 'soundtrack', 'compilation')
    ),
    class_rank INTEGER NOT NULL,
    candidate_rank INTEGER NOT NULL,
    release_title TEXT NOT NULL,
    release_artist TEXT NOT NULL,
    artist_mbids_key TEXT NOT NULL,
    release_date TEXT,
    PRIMARY KEY(recording_mbid, release_mbid),
    FOREIGN KEY (recording_mbid)
        REFERENCES recording_release_lookups(recording_mbid)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS compilation_track_artwork (
    track_path TEXT PRIMARY KEY COLLATE NOCASE,
    recording_mbid TEXT NOT NULL COLLATE NOCASE,
    artist_mbid TEXT NOT NULL COLLATE NOCASE,
    source_kind TEXT NOT NULL,
    source_locator TEXT NOT NULL,
    release_mbid TEXT,
    artwork_sha256 TEXT NOT NULL,
    outcome TEXT NOT NULL,
    applied_at TEXT NOT NULL,
    splined_version TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS compilation_album_progress (
    album_path TEXT PRIMARY KEY COLLATE NOCASE,
    total_tracks INTEGER NOT NULL,
    completed_tracks INTEGER NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('incomplete', 'complete')),
    updated_at TEXT NOT NULL,
    splined_version TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS album_refresh_review_queue (
    album_key TEXT PRIMARY KEY COLLATE NOCASE,
    requested_at TEXT NOT NULL,
    source TEXT NOT NULL,
    details_json TEXT NOT NULL,
    splined_version TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS cache_entries (
    cache_key TEXT PRIMARY KEY,
    cache_type TEXT NOT NULL,
    album_key TEXT,
    payload_json TEXT NOT NULL,
    splined_version TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS cache_history (
    history_id INTEGER PRIMARY KEY AUTOINCREMENT,
    cache_key TEXT NOT NULL,
    cache_type TEXT NOT NULL,
    album_key TEXT,
    action TEXT NOT NULL,
    payload_json TEXT,
    splined_version TEXT,
    event_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS db_maintenance_state (
    action_name TEXT PRIMARY KEY,
    completed_at TEXT NOT NULL,
    details_json TEXT NOT NULL,
    splined_version TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS picker_inventory (
    inventory_key TEXT PRIMARY KEY,
    signature_json TEXT NOT NULL,
    folders_json TEXT NOT NULL,
    generated_at TEXT NOT NULL,
    splined_version TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS retired_album_paths (
    album_key TEXT PRIMARY KEY COLLATE NOCASE,
    album_path TEXT NOT NULL,
    reason TEXT NOT NULL,
    retired_at TEXT NOT NULL,
    splined_version TEXT NOT NULL
);

-- Catalog Index Coordinator.  Generation rows and album payloads are durable
-- checkpoints; the existing catalog tables remain the last published reader
-- snapshot until one atomic publication transaction completes.
CREATE TABLE IF NOT EXISTS catalog_generations (
    generation_id TEXT PRIMARY KEY,
    backend TEXT NOT NULL DEFAULT 'sqlite',
    mode TEXT NOT NULL CHECK(mode IN ('initial','incremental','full')),
    state TEXT NOT NULL CHECK(state IN (
        'building','validating','publishing','published',
        'cancelled','failed','abandoned'
    )),
    base_generation_id TEXT NOT NULL DEFAULT '',
    library_root TEXT NOT NULL,
    owner_token TEXT NOT NULL,
    lease_expires_at TEXT NOT NULL,
    discovered_albums INTEGER NOT NULL DEFAULT 0,
    completed_albums INTEGER NOT NULL DEFAULT 0,
    changed_albums INTEGER NOT NULL DEFAULT 0,
    unchanged_albums INTEGER NOT NULL DEFAULT 0,
    removed_albums INTEGER NOT NULL DEFAULT 0,
    error_count INTEGER NOT NULL DEFAULT 0,
    current_path TEXT NOT NULL DEFAULT '',
    phase TEXT NOT NULL DEFAULT 'enumerating',
    timings_json TEXT NOT NULL DEFAULT '{}',
    error TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    published_at TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_catalog_generations_one_active
    ON catalog_generations(backend)
    WHERE state IN ('building','validating','publishing');

CREATE INDEX IF NOT EXISTS idx_catalog_generations_recovery
    ON catalog_generations(state,lease_expires_at,updated_at);

CREATE TABLE IF NOT EXISTS catalog_index_state (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    active_generation_id TEXT NOT NULL DEFAULT '',
    maintenance_generation_id TEXT NOT NULL DEFAULT '',
    catalog_state TEXT NOT NULL DEFAULT 'initializing' CHECK(
        catalog_state IN ('initializing','ready','maintenance','failed')
    ),
    change_summary_json TEXT NOT NULL DEFAULT
        '{"count":0,"added":0,"changed":0,"removed":0,"examples":[]}',
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS catalog_generation_albums (
    generation_id TEXT NOT NULL,
    album_path TEXT NOT NULL,
    scan_token TEXT NOT NULL,
    album_key TEXT NOT NULL DEFAULT '',
    artist_key TEXT NOT NULL DEFAULT '',
    inventory_signature TEXT NOT NULL,
    artwork_signature TEXT NOT NULL DEFAULT '',
    tag_signature TEXT NOT NULL DEFAULT '',
    track_count INTEGER NOT NULL DEFAULT 0,
    total_size INTEGER NOT NULL DEFAULT 0,
    newest_mtime_ns INTEGER NOT NULL DEFAULT 0,
    disposition TEXT NOT NULL CHECK(disposition IN (
        'changed','unchanged','protected','deferred','error'
    )),
    artist_payload_json TEXT,
    album_payload_json TEXT,
    tracks_payload_json TEXT,
    review_payload_json TEXT,
    error TEXT NOT NULL DEFAULT '',
    completed_at TEXT NOT NULL,
    PRIMARY KEY(generation_id,album_path),
    FOREIGN KEY(generation_id) REFERENCES catalog_generations(generation_id)
        ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_catalog_generation_albums_key
    ON catalog_generation_albums(generation_id,album_key);

CREATE TABLE IF NOT EXISTS catalog_change_manifest (
    album_path TEXT PRIMARY KEY COLLATE NOCASE,
    album_key TEXT NOT NULL,
    inventory_signature TEXT NOT NULL,
    artwork_signature TEXT NOT NULL DEFAULT '',
    tag_signature TEXT NOT NULL DEFAULT '',
    track_count INTEGER NOT NULL DEFAULT 0,
    total_size INTEGER NOT NULL DEFAULT 0,
    newest_mtime_ns INTEGER NOT NULL DEFAULT 0,
    last_seen_generation TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_catalog_change_manifest_generation
    ON catalog_change_manifest(last_seen_generation);

CREATE TABLE IF NOT EXISTS catalog_index_events (
    event_id TEXT PRIMARY KEY,
    generation_id TEXT NOT NULL,
    album_key TEXT NOT NULL,
    album_path TEXT NOT NULL,
    operation TEXT NOT NULL CHECK(operation IN ('upsert','remove','move')),
    inventory_signature TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    UNIQUE(generation_id,album_path,operation)
);

CREATE INDEX IF NOT EXISTS idx_catalog_index_events_generation
    ON catalog_index_events(generation_id,created_at);

-- Cross-client artwork provenance is deliberately separate from catalog and
-- processing state. The outbox survives PostgreSQL outages; the state table
-- preserves the last acknowledged writer across Windows restarts and SNAPs.
CREATE TABLE IF NOT EXISTS album_artwork_write_state (
    album_identity TEXT PRIMARY KEY,
    album_key TEXT NOT NULL COLLATE NOCASE,
    musicbrainz_albumid TEXT NOT NULL DEFAULT '',
    relative_path TEXT NOT NULL DEFAULT '',
    writer TEXT NOT NULL CHECK(writer IN ('windows', 'webgui')),
    operation TEXT NOT NULL,
    event_id TEXT NOT NULL UNIQUE,
    source_completed_ms INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0,
    completed_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS album_artwork_write_outbox (
    event_id TEXT PRIMARY KEY,
    album_identity TEXT NOT NULL,
    album_key TEXT NOT NULL COLLATE NOCASE,
    musicbrainz_albumid TEXT NOT NULL DEFAULT '',
    relative_path TEXT NOT NULL DEFAULT '',
    writer TEXT NOT NULL CHECK(writer = 'windows'),
    operation TEXT NOT NULL,
    source_completed_ms INTEGER NOT NULL,
    completed_at TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_artists_status
    ON artists(status, artist_name COLLATE NOCASE);

CREATE INDEX IF NOT EXISTS idx_artists_mb_artistid
    ON artists(musicbrainz_artistid)
    WHERE musicbrainz_artistid IS NOT NULL
      AND trim(musicbrainz_artistid) <> '';

CREATE INDEX IF NOT EXISTS idx_albums_artist
    ON albums(artist_key, album_sort COLLATE NOCASE, album_name COLLATE NOCASE);

CREATE INDEX IF NOT EXISTS idx_albums_artist_status
    ON albums(artist_key, status);

CREATE INDEX IF NOT EXISTS idx_albums_status
    ON albums(status);

CREATE INDEX IF NOT EXISTS idx_albums_mb_albumid
    ON albums(musicbrainz_albumid)
    WHERE musicbrainz_albumid IS NOT NULL
      AND trim(musicbrainz_albumid) <> '';

CREATE INDEX IF NOT EXISTS idx_albums_release_group
    ON albums(musicbrainz_releasegroupid)
    WHERE musicbrainz_releasegroupid IS NOT NULL
      AND trim(musicbrainz_releasegroupid) <> '';

CREATE INDEX IF NOT EXISTS idx_tracks_recording_artist
    ON tracks(musicbrainz_recordingid, musicbrainz_artistid)
    WHERE musicbrainz_recordingid IS NOT NULL
      AND trim(musicbrainz_recordingid) <> ''
      AND musicbrainz_artistid IS NOT NULL
      AND trim(musicbrainz_artistid) <> '';

CREATE INDEX IF NOT EXISTS idx_tracks_album
    ON tracks(album_key);

CREATE INDEX IF NOT EXISTS idx_recording_release_candidates_rank
    ON recording_release_candidates(
        recording_mbid,
        artist_mbids_key,
        class_rank,
        candidate_rank
    );

CREATE INDEX IF NOT EXISTS idx_cache_entries_album
    ON cache_entries(album_key);

CREATE INDEX IF NOT EXISTS idx_cache_entries_album_cf
    ON cache_entries(lower(album_key));

CREATE INDEX IF NOT EXISTS idx_cache_entries_type
    ON cache_entries(cache_type);

CREATE INDEX IF NOT EXISTS idx_cache_history_key
    ON cache_history(cache_key, event_at);

CREATE INDEX IF NOT EXISTS idx_retired_album_paths_key
    ON retired_album_paths(album_key, retired_at DESC);

CREATE INDEX IF NOT EXISTS idx_album_artwork_write_state_key
    ON album_artwork_write_state(album_key);

CREATE INDEX IF NOT EXISTS idx_album_artwork_write_outbox_order
    ON album_artwork_write_outbox(source_completed_ms, event_id);
