CREATE TABLE IF NOT EXISTS media_state_classification_policy (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    policy JSONB NOT NULL,
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by TEXT NOT NULL DEFAULT 'webgui'
);

COMMENT ON TABLE media_state_classification_policy IS
    'PostgreSQL authority for the shared Artist, Album, override, and database-monitor presentation policy.';
