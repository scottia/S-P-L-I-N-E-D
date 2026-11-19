CREATE SEQUENCE IF NOT EXISTS comp_track_write_revision_seq;

CREATE TABLE IF NOT EXISTS comp_track_write_coordination (
    track_identity TEXT PRIMARY KEY,
    album_identity TEXT NOT NULL,
    album_key TEXT NOT NULL,
    track_key TEXT NOT NULL DEFAULT '',
    relative_track_path TEXT NOT NULL,
    writer TEXT NOT NULL CHECK (writer IN ('windows', 'webgui')),
    operation TEXT NOT NULL,
    event_id TEXT NOT NULL UNIQUE,
    artwork_sha256 TEXT NOT NULL,
    source_completed_ms BIGINT NOT NULL,
    revision BIGINT NOT NULL DEFAULT nextval('comp_track_write_revision_seq'),
    completed_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS comp_track_write_coordination_album_idx
    ON comp_track_write_coordination(album_identity);
CREATE INDEX IF NOT EXISTS comp_track_write_coordination_revision_idx
    ON comp_track_write_coordination(revision);
