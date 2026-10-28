# SPLINED browser workspace guide

The browser workspace previews the artwork workflow using an isolated Python SQLite index and copied music files. The index remains read-only. Artist COMP thumbnail repair is the only optional write action; when enabled on the loopback test service, it backs up the original embedded image and records each audio-file change in a separate log.

## Mouse workflow

1. Select an artist or album in Media Library. Previous candidates and the old selected preview clear while the next item loads. Artist Overview shows indexed albums, status, and songs. Select an album card for its cover.
2. Press **LAUNCH** to find artist images from Fanart.tv and Deezer. The center status bar reports search progress. Local images are also available as candidates. Blue candidate controls choose artist images; pink controls choose banners.
3. Select **Artists Art Enhancements** or **Album Art Enhancements**. Pane 1 shows the saved artist package. Pane 2 previews the proposed artist image; pane 3 previews the proposed banner. Pane radios control Selected Artwork. Both proposed radios show the combined package, with **Themed Packaging** at the preview's upper left.
4. Filters, Fitment, and Detailed apply browser-only preview adjustments. Drag a slider, double-click it to reset, or use the small reset beside Enhancements. Fitment starts at the center (1×): move left to shrink or right to enlarge. Wheel over an artist or banner preview to zoom; click and drag the image to reposition it. The round artist frame stays fixed while its pixels move.
5. **EMBED Batch** stages the selected album only in the browser's temporary preview queue. Opening Album Track Embedding does not stage an album.
6. Media Filter starts with every status category enabled. Each bullet and its matching library-row bullet use the same status color; fill or outline shows selection. Toggle categories to narrow the library and use its lock to keep the chosen set. Candidate Filter fills green for categories found in the current results; click a category to cycle green Included → red Disregarded → outlined Neutral. Only Disregarded hides matching candidates. Its lock keeps the current filter through another search.

## Overview data

Album names, counts, status, and album tracks come from the isolated music index. Artist biography, similar artists, and top songs are requested read-only from Navidrome's Subsonic API. Navidrome may return no biography, similar artists, or top songs if its metadata integrations are unavailable. In that case, the interface labels the similar-artist cards as sample layout data; it does not invent a biography. Songs from the local index are shown as **Indexed songs**, since they are not ranked by popularity.

## Limits of this test build

Saved upscaling, saved adjustments, image writing, blue COMP tag embedding, MusicBrainz review, and image searches from the other Windows providers are not yet connected. Artist COMP Report can group every matching missing-ID album for the selected artist, preview green embedded art, and identify purple thumbnails. REPAIR COMP writes only to the copied music when the loopback test service starts with `--allow-comp-repairs`. No PostgreSQL connection is used by this local browser preview. **Settings** shows the active test paths and repair mode.

For the Windows/browser layout mapping and planned PostgreSQL boundary, see [Browser workspace schematic](browser-workspace-schematic.md). The [function audit](windows-web-function-audit.md) lists the Windows rules, event sequences, and remaining WebUI gaps.
