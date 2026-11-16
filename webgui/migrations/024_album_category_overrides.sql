CREATE TABLE album_category_overrides (
    album_key TEXT PRIMARY KEY REFERENCES albums(album_key) ON DELETE CASCADE,
    category TEXT NOT NULL CHECK(category IN (
        'artist',
        'various-artists',
        'soundtracks',
        'fallback',
        'misc-curated',
        'misc-undefined'
    )),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_album_category_overrides_category
    ON album_category_overrides(category);

COMMENT ON TABLE album_category_overrides IS
    'Operator-selected Album category exceptions; these override path grouping without rescanning media.';
