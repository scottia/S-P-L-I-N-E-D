-- Recovery and source-authority hardening for live catalog synchronization.
--
-- One explicitly selected Windows source may feed the automatic consumer.
-- Baseline generation/change revisions close the mode-transition gap, while
-- attempt metadata preserves durable failure diagnostics.
ALTER TABLE catalog_sync_settings
    ADD COLUMN active_source_instance_id TEXT NOT NULL DEFAULT '';

ALTER TABLE catalog_sync_sources
    ADD COLUMN baseline_generation BIGINT NOT NULL DEFAULT 1;

ALTER TABLE catalog_sync_sources
    ADD COLUMN baseline_change_revision BIGINT NOT NULL DEFAULT 0;

ALTER TABLE catalog_sync_events
    ADD COLUMN attempt_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE catalog_sync_events
    ADD COLUMN last_attempt_at TIMESTAMPTZ;

CREATE INDEX catalog_sync_active_source
    ON catalog_sync_sources(source_instance_id, baseline_state, source_root, target_root);

COMMENT ON COLUMN catalog_sync_settings.active_source_instance_id IS
    'The sole owner-authorized Windows SQLite source eligible for automatic live Apply.';

COMMENT ON COLUMN catalog_sync_sources.baseline_generation IS
    'Windows mode-transition generation captured by the authorized immutable SNAP.';

COMMENT ON COLUMN catalog_sync_sources.baseline_change_revision IS
    'All-catalog mutation counter captured by the authorized immutable SNAP.';
