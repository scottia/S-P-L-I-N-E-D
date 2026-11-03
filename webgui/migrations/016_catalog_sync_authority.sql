-- Per-entity ownership prevents WebGUI indexing from replacing synchronized
-- Windows SQLite authority while leaving standalone WebGUI catalogs unchanged.
CREATE TABLE catalog_sync_authority (
    entity_table TEXT NOT NULL CHECK (entity_table IN ('artists', 'albums', 'tracks')),
    entity_key TEXT NOT NULL,
    authority TEXT NOT NULL CHECK (authority = 'windows'),
    source_fingerprint TEXT NOT NULL,
    source_label TEXT NOT NULL,
    established_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_confirmed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY(entity_table, entity_key)
);

CREATE INDEX catalog_sync_authority_source
    ON catalog_sync_authority(source_fingerprint, entity_table);
