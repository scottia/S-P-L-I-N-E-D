CREATE SEQUENCE album_artwork_write_revision_seq;

CREATE TABLE album_artwork_write_coordination (
    album_identity TEXT PRIMARY KEY,
    album_key TEXT NOT NULL,
    musicbrainz_albumid TEXT NOT NULL DEFAULT '',
    relative_path TEXT NOT NULL DEFAULT '',
    writer TEXT NOT NULL CHECK(writer IN ('windows', 'webgui')),
    operation TEXT NOT NULL,
    event_id TEXT NOT NULL UNIQUE,
    source_completed_ms BIGINT NOT NULL,
    revision BIGINT NOT NULL DEFAULT nextval('album_artwork_write_revision_seq'),
    completed_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_album_artwork_write_album_key
    ON album_artwork_write_coordination(album_key);

CREATE INDEX idx_album_artwork_write_revision
    ON album_artwork_write_coordination(revision);

COMMENT ON TABLE album_artwork_write_coordination IS
    'Durable last-successful Album artwork writer; presentation provenance only.';
