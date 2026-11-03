-- Migration 013 originally promoted historical embedded_tracks counters to
-- verified without reading the mapped media.  Invalidate that optimistic
-- evidence once while preserving the projected counter for retry diagnostics.
-- Artist Overview will lazily replace it with physical readback evidence.
UPDATE albums
SET embedded_validation_status = 'unknown',
    embedded_validated_at = NULL
WHERE embedded_validation_status = 'verified';
