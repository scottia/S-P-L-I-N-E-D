# PR #129 Candidate Artwork evidence

These screenshots were captured on MEDIAPC from the installed Brave executable
at the normal 1600 × 1000 application viewport. The browser drove a temporary
local FastAPI server, an isolated PostgreSQL schema, generated audio fixtures,
and deterministic MusicBrainz/artwork-provider responses through the production
WebGUI processing routines.

- [Diagnostics collapsed](candidate-area-brave-normal-collapsed.png)
- [Diagnostics expanded](candidate-area-brave-normal-expanded.png)

Both states retain the complete first row of candidate thumbnails, artwork
sources, native resolutions, and quality/range labels. Expanding diagnostics
grows the candidate dock instead of covering or clipping the cards.

No production media, production PostgreSQL data, or live provider response is
represented by these fixtures.
