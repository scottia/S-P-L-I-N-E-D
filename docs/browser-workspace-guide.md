# SPLINED browser workspace guide

The browser workspace is a local, read-only preview of the artwork workflow. It currently reads an isolated Python SQLite index and copied music files. It does not write artwork, tags, or database rows.

## Mouse workflow

1. Select an artist or album in Media Library. Previous candidates and the old selected preview clear while the next item loads. Artist Overview shows indexed albums, status, and songs. Select an album card for its cover.
2. Press **LAUNCH** to find artist images from Fanart.tv and Deezer. The center status bar reports search progress. Local images are also available as candidates. Blue candidate controls choose artist images; pink controls choose banners.
3. Select **Artists Art Enhancements** or **Album Art Enhancements**. Pane 1 shows the saved artist package. Pane 2 previews the proposed artist image; pane 3 previews the proposed banner. Pane radios control Selected Artwork. Both proposed radios show the combined package, with **Themed Packaging** at the preview's upper left.
4. Filters, Fitment, and Detailed apply browser-only preview adjustments. Drag a slider, double-click it to reset, or use the small reset beside Enhancements. Wheel over an artist or banner preview to zoom; drag it to reposition.
5. **EMBED Batch** stages the selected album only in the browser's temporary preview queue. Opening Album Track Embedding does not stage an album.

## Overview data

Album names, counts, status, and album tracks come from the isolated music index. Artist biography, similar artists, and top songs are requested read-only from Navidrome's Subsonic API. Navidrome may return no biography, similar artists, or top songs if its metadata integrations are unavailable. In that case, the interface labels the similar-artist cards as sample layout data; it does not invent a biography. Songs from the local index are shown as **Indexed songs**, since they are not ranked by popularity.

## Limits of this test build

Actual upscaling, saved adjustments, image writing, audio tag embedding, MusicBrainz review, and image searches from the other Windows providers are not yet connected. No PostgreSQL connection is used by this local browser preview. The **Settings** dialog shows its current read-only index and artwork paths.

For the Windows/browser layout mapping and planned PostgreSQL boundary, see [Browser workspace schematic](browser-workspace-schematic.md).
