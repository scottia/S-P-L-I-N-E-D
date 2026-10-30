CREATE TABLE IF NOT EXISTS artist_album_appearances (
    artist_key TEXT NOT NULL REFERENCES artists(artist_key) ON DELETE CASCADE,
    album_key TEXT NOT NULL REFERENCES albums(album_key) ON DELETE CASCADE,
    navidrome_album_id TEXT NOT NULL DEFAULT '',
    featured INTEGER NOT NULL DEFAULT 0,
    song_paths_json TEXT NOT NULL DEFAULT '[]',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (artist_key, album_key)
);

CREATE INDEX IF NOT EXISTS idx_artist_album_appearances_album
    ON artist_album_appearances(album_key);
