ALTER TABLE albums
    ADD COLUMN embedded_validation_status TEXT NOT NULL DEFAULT 'unknown';

ALTER TABLE albums
    ADD COLUMN embedded_validated_at TIMESTAMPTZ;

UPDATE albums
SET embedded_validation_status = 'verified',
    embedded_validated_at = now()
WHERE embedded_tracks > 0;

ALTER TABLE albums
    ADD CONSTRAINT albums_embedded_validation_status_check
    CHECK (embedded_validation_status IN ('unknown', 'verified', 'retry'));
