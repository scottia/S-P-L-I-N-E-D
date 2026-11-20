-- SQLite integers were 64-bit. Preserve file sizes and nanosecond timestamps.
ALTER TABLE albums ALTER COLUMN representative_size TYPE BIGINT;
ALTER TABLE albums ALTER COLUMN representative_mtime_ns TYPE BIGINT;
ALTER TABLE tracks ALTER COLUMN file_size TYPE BIGINT;
ALTER TABLE tracks ALTER COLUMN file_mtime_ns TYPE BIGINT;
