CREATE TABLE IF NOT EXISTS comp_report_policy (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    policy JSONB NOT NULL,
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by TEXT NOT NULL DEFAULT 'webgui'
);

COMMENT ON TABLE comp_report_policy IS
    'PostgreSQL authority for the shared Windows and WebGUI COMP matching, batching, and coordination policy.';
