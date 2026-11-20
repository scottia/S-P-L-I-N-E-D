-- v2 diagnostics describe retired cache paths and must not be presented as
-- current v3 failures after an in-place PostgreSQL upgrade. Catalog rows are
-- deliberately preserved; a Full Refresh revalidates and republishes them.
DELETE FROM web_audit
WHERE application_version ~ '^[0-9]+\.'
  AND split_part(application_version, '.', 1)::INTEGER < 3;

DELETE FROM web_audit_sessions
WHERE application_version ~ '^[0-9]+\.'
  AND split_part(application_version, '.', 1)::INTEGER < 3;
