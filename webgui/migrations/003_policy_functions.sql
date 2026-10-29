-- Preserve the established bound policy query's substring semantics.
CREATE OR REPLACE FUNCTION instr(haystack TEXT, needle TEXT) RETURNS INTEGER
LANGUAGE SQL IMMUTABLE STRICT PARALLEL SAFE AS $$ SELECT position(needle in haystack) $$;
