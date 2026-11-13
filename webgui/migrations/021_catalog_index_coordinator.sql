-- Bounded catalog indexing is prepared in durable generation-scoped staging
-- rows.  The published artists/albums/tracks tables remain the reader-facing
-- snapshot and are changed only by the final publication transaction.
CREATE TABLE IF NOT EXISTS catalog_generations (
    generation_id TEXT PRIMARY KEY,
    backend TEXT NOT NULL DEFAULT 'postgresql',
    mode TEXT NOT NULL CHECK (mode IN ('initial', 'incremental', 'full')),
    state TEXT NOT NULL CHECK (
        state IN (
            'building', 'validating', 'publishing', 'published',
            'cancelled', 'failed', 'abandoned'
        )
    ),
    base_generation_id TEXT NOT NULL DEFAULT '',
    library_root TEXT NOT NULL,
    owner_token TEXT NOT NULL,
    lease_expires_at TIMESTAMPTZ NOT NULL,
    discovered_albums BIGINT NOT NULL DEFAULT 0,
    completed_albums BIGINT NOT NULL DEFAULT 0,
    changed_albums BIGINT NOT NULL DEFAULT 0,
    unchanged_albums BIGINT NOT NULL DEFAULT 0,
    removed_albums BIGINT NOT NULL DEFAULT 0,
    error_count BIGINT NOT NULL DEFAULT 0,
    current_path TEXT NOT NULL DEFAULT '',
    phase TEXT NOT NULL DEFAULT 'enumerating',
    timings JSONB NOT NULL DEFAULT '{}'::jsonb,
    error TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    published_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS catalog_generations_one_active
    ON catalog_generations(backend)
    WHERE state IN ('building', 'validating', 'publishing');

CREATE INDEX IF NOT EXISTS catalog_generations_recovery
    ON catalog_generations(state, lease_expires_at, updated_at);

CREATE TABLE IF NOT EXISTS catalog_index_state (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    active_generation_id TEXT NOT NULL DEFAULT '',
    maintenance_generation_id TEXT NOT NULL DEFAULT '',
    catalog_state TEXT NOT NULL DEFAULT 'initializing' CHECK (
        catalog_state IN ('initializing', 'ready', 'maintenance', 'failed')
    ),
    change_summary JSONB NOT NULL DEFAULT
        '{"count":0,"added":0,"changed":0,"removed":0,"examples":[]}'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO catalog_index_state(singleton, catalog_state)
VALUES (TRUE, CASE WHEN EXISTS(SELECT 1 FROM albums) THEN 'ready' ELSE 'initializing' END)
ON CONFLICT(singleton) DO NOTHING;

-- Adopt an already published pre-coordinator catalog without rebuilding it.
-- Its rows remain authoritative while the first coordinated generation is
-- prepared, validated, and atomically published.
INSERT INTO catalog_generations(
    generation_id,backend,mode,state,library_root,owner_token,lease_expires_at,
    discovered_albums,completed_albums,changed_albums,phase,published_at
)
SELECT
    'legacy-pre-coordinator','postgresql','initial','published','legacy',
    'migration-021',now(),count(*),count(*),count(*),'complete',now()
FROM albums
HAVING count(*) > 0
ON CONFLICT(generation_id) DO NOTHING;

UPDATE catalog_index_state
SET active_generation_id='legacy-pre-coordinator',catalog_state='ready',updated_at=now()
WHERE singleton=TRUE AND active_generation_id='' AND EXISTS(SELECT 1 FROM albums);

CREATE TABLE IF NOT EXISTS catalog_generation_albums (
    generation_id TEXT NOT NULL REFERENCES catalog_generations(generation_id)
        ON DELETE CASCADE,
    album_path TEXT NOT NULL,
    scan_token TEXT NOT NULL,
    album_key TEXT NOT NULL DEFAULT '',
    artist_key TEXT NOT NULL DEFAULT '',
    inventory_signature TEXT NOT NULL,
    artwork_signature TEXT NOT NULL DEFAULT '',
    tag_signature TEXT NOT NULL DEFAULT '',
    track_count BIGINT NOT NULL DEFAULT 0,
    total_size BIGINT NOT NULL DEFAULT 0,
    newest_mtime_ns BIGINT NOT NULL DEFAULT 0,
    disposition TEXT NOT NULL CHECK (
        disposition IN ('changed', 'unchanged', 'protected', 'deferred', 'error')
    ),
    artist_payload JSONB,
    album_payload JSONB,
    tracks_payload JSONB,
    review_payload JSONB,
    error TEXT NOT NULL DEFAULT '',
    completed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY(generation_id, album_path)
);

CREATE INDEX IF NOT EXISTS catalog_generation_albums_key
    ON catalog_generation_albums(generation_id, album_key);

CREATE TABLE IF NOT EXISTS catalog_change_manifest (
    album_path TEXT PRIMARY KEY,
    album_key TEXT NOT NULL,
    inventory_signature TEXT NOT NULL,
    artwork_signature TEXT NOT NULL DEFAULT '',
    tag_signature TEXT NOT NULL DEFAULT '',
    track_count BIGINT NOT NULL DEFAULT 0,
    total_size BIGINT NOT NULL DEFAULT 0,
    newest_mtime_ns BIGINT NOT NULL DEFAULT 0,
    last_seen_generation TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS catalog_change_manifest_generation
    ON catalog_change_manifest(last_seen_generation);

CREATE TABLE IF NOT EXISTS catalog_index_events (
    event_id TEXT PRIMARY KEY,
    generation_id TEXT NOT NULL,
    album_key TEXT NOT NULL,
    album_path TEXT NOT NULL,
    operation TEXT NOT NULL CHECK (operation IN ('upsert', 'remove', 'move')),
    inventory_signature TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(generation_id, album_path, operation)
);

CREATE INDEX IF NOT EXISTS catalog_index_events_generation
    ON catalog_index_events(generation_id, created_at);
