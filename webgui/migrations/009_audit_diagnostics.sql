ALTER TABLE web_audit ADD COLUMN severity TEXT NOT NULL DEFAULT 'info';
ALTER TABLE web_audit ADD COLUMN action_id TEXT NOT NULL DEFAULT '';
ALTER TABLE web_audit ADD COLUMN trace_id TEXT NOT NULL DEFAULT '';
ALTER TABLE web_audit ADD COLUMN job_id TEXT NOT NULL DEFAULT '';
ALTER TABLE web_audit ADD COLUMN scope JSONB NOT NULL DEFAULT '{}'::jsonb;
ALTER TABLE web_audit ADD COLUMN stage TEXT NOT NULL DEFAULT '';
ALTER TABLE web_audit ADD COLUMN request_status TEXT NOT NULL DEFAULT '';
ALTER TABLE web_audit ADD COLUMN elapsed_ms DOUBLE PRECISION;
ALTER TABLE web_audit ADD COLUMN state_from TEXT NOT NULL DEFAULT '';
ALTER TABLE web_audit ADD COLUMN state_to TEXT NOT NULL DEFAULT '';
ALTER TABLE web_audit ADD COLUMN error TEXT NOT NULL DEFAULT '';

CREATE INDEX web_audit_created_at ON web_audit(created_at, id);
CREATE INDEX web_audit_severity ON web_audit(severity, id);
CREATE INDEX web_audit_action ON web_audit(action, id);
CREATE INDEX web_audit_trace ON web_audit(trace_id, id) WHERE trace_id <> '';
CREATE INDEX web_audit_job ON web_audit(job_id, id) WHERE job_id <> '';
