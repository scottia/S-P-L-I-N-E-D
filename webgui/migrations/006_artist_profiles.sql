CREATE TABLE IF NOT EXISTS artist_profiles (
    artist_key TEXT PRIMARY KEY REFERENCES artists(artist_key) ON DELETE CASCADE,
    lookup_name TEXT NOT NULL DEFAULT '',
    musicbrainz_artistid TEXT NOT NULL DEFAULT '',
    biography TEXT NOT NULL DEFAULT '',
    external_url TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL DEFAULT '',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_artist_profiles_mbid
    ON artist_profiles(musicbrainz_artistid)
    WHERE trim(musicbrainz_artistid) <> '';
