-- Preserve per-table compare/apply accounting for downloadable Audit diagnosis.
ALTER TABLE sqlite_reconciliation_runs
    ADD COLUMN table_counts JSONB NOT NULL DEFAULT '{}'::jsonb;
