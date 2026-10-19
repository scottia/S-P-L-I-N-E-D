# MusicBrainz Fallback Workflow

Current Windows compilation/fallback workflow and its identity rules.

## UI contract

- The primary MusicBrainz identity is one friendly, status-colored identity.
- **Advanced IDs** contains only Recording ID and Artist ID. The Release ID is obtained from the selected MusicBrainz album.
- Keep the existing release-family headings and edition detail. United States editions are primary within a family.
- A fallback album without a Release/Album ID is marked by a muted magenta diagonal **COMP** banner.
- While MusicBrainz is open, Selected Artwork shows the current track's embedded image, a hovered or selected result, or a selected Existing source. Artist Art and compilation-folder `cover.*` must never be substituted there.

## User workflow

1. Select an artist in Artist Overview.
2. Click an album marked with the muted magenta **COMP** banner.
3. If the compilation contains multiple tracks by that artist, select the desired track. A single matching track opens automatically.
4. Confirm the pinned cyan header, for example: `TRACK  10cc — The Things We Do for Love`.
5. SPLINED resolves the identity automatically using `splined.db`, Navidrome, and MusicBrainz validation. A successful identity turns green.
6. SPLINED queries the exact Recording ID and, when necessary, expands through the related Work while retaining only recordings with the validated Artist ID.
7. If the identity is invalid, SPLINED performs an Artist-restricted fuzzy title search. Results are grouped by percentage and colored red, orange, yellow, or green.
8. If automatic recovery still fails, **MB INFO** opens the exact MusicBrainz Artist page, or an artist search when no valid Artist ID exists, and leaves SPLINED waiting on the same track.
9. After researching on MusicBrainz, return to SPLINED, expand **Advanced IDs**, correct the Recording and/or Artist ID, and press **Apply IDs**. SPLINED validates them, preserves rejected values for correction, and reruns the lookup. Accepted IDs turn green and are saved to the track and `splined.db`.
10. Review the separate **Existing** choices at the top of the results and the grouped MusicBrainz releases. Hover to preview, click to lock, then write the preview directly or optionally use **Upscale Preview** first. **EMBED Thumbs** can prepare a preference-sized copy before **EMBED Comp**. SPLINED returns to the expanded COMP album and marks the written track green.

## Authority and recovery rules

1. A missing Release/Album ID is what makes the album eligible for fallback; it is not an error.
2. Validate the tagged Recording ID and Artist ID together.
3. Use exact Recording releases first.
4. When the exact result is too narrow, use its related Work only to discover additional recordings by the same validated Artist ID. A Work match may never introduce another performer.
5. When the Recording ID is invalid or mismatched, validate the Artist ID independently and perform an Artist-ID-restricted fuzzy title search.
6. Preserve invalid values in Advanced IDs so the user can correct them. Never silently replace an ID based only on filename or album text.
7. A selected result supplies the Release ID; Release ID is not an editable authority field.
