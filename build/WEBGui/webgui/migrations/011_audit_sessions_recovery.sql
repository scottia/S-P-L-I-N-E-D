CREATE TABLE web_audit_sessions (
    session_id TEXT PRIMARY KEY,
    application_version TEXT NOT NULL,
    application_revision TEXT NOT NULL,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE web_audit ADD COLUMN session_id TEXT NOT NULL DEFAULT '';
ALTER TABLE web_audit ADD COLUMN application_version TEXT NOT NULL DEFAULT '';
ALTER TABLE web_audit ADD COLUMN application_revision TEXT NOT NULL DEFAULT '';
ALTER TABLE web_audit ADD COLUMN recovered_from_fallback BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE web_audit ADD COLUMN fallback_id TEXT NOT NULL DEFAULT '';

CREATE INDEX web_audit_session ON web_audit(session_id, id);
CREATE INDEX web_audit_revision ON web_audit(application_revision, id);
CREATE UNIQUE INDEX web_audit_fallback_id
    ON web_audit(fallback_id)
    WHERE fallback_id <> '';
