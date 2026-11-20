-- Startup reconciliation is deliberately opt-in and uses the already saved,
-- authorized snapshot/path mapping.  "apply-safe" can never apply conflicts.
ALTER TABLE sqlite_reconciliation_settings
    ADD COLUMN IF NOT EXISTS startup_mode TEXT NOT NULL DEFAULT 'off'
    CHECK (startup_mode IN ('off', 'compare', 'apply-safe'));
