# SPLINED WebGUI

The supported server interface runs in a browser. Python handles providers, image processing, library and tag writes; PostgreSQL stores the catalog, completion ledger, and persistent work queue. No Python terminal interface or native terminal-input extension is installed. Windows remains a separate native Rust application with its own SQLite store.

## First run

Deploy the [WebGUI and PostgreSQL stack](../docker/README.md), then open `http://127.0.0.1:8080` on the server or its configured LAN address. Sign in with the deployment's WebGUI access token. In **Settings**, select the music library path as seen by the server, cache and artist-art paths, output policy, and provider credentials. Save Settings, then choose **Refresh Library Index**. Start in `read` mode; change **mode** to `write` to enable artwork and tag writes.

Every Config v5 table is editable in Settings. Array fields use JSON lists, for example `["deezer","itunes"]`. Source policy switches and order are saved with the configuration. **Provider Credentials** edits the five supported credential files and tests saved access. Blank secret fields retain their existing values; saved secrets are never sent back to the browser. Container deployments normally supply PostgreSQL credentials through environment variables, so the connection field is managed by the server.

## Artwork workflow

1. Select an artist or album. **LAUNCH** on an artist queues review of its selected eligible albums; in an album view it searches that album's configured sources. **Artist Art** opens portrait and banner search. **MusicBrainz** searches alternate releases and lets you choose which release's artwork to inspect.
2. Choose a candidate or **Upload Art**. In Enhancements, use Filters, Fitment, and Detailed to prepare the image. Wheel to zoom and drag to position artist artwork. Proposed component radios choose what is previewed and written. Both radios allow writing the portrait and banner together; Themed Packaging applies shared adjustments.
3. **WRITE Cover** saves the prepared cover. **WRITE Artist Art** saves the chosen components. Read mode disables these music writes. **Upscale Preview** prepares a twofold Lanczos resize, not an AI reconstruction; writing remains explicit.
4. **EMBED Batch** saves the prepared album cover and stages a verified snapshot for track embedding. It keeps the current workspace open. The **Album Track Embedding** button gains muted purple text, outline, and background while unfinished work exists.
5. Open Album Track Embedding and choose **Start All Staged**. Progress and failures are persisted. **Pause After Album** pauses waiting albums while the current operation finishes; **Pause ALL Albums** requests a pause between atomic track writes. **Resume** retries paused or failed work. A server restart pauses interrupted work until explicitly resumed. Staged artwork survives browser and server restarts.

An album without a MusicBrainz Album ID uses the COMP workflow. Folder cover artwork is never automatically substituted for individual COMP track artwork. **COMP Report** groups the focused artist's tracks across missing-ID albums. Green means recorded artwork has a valid thumbnail; purple means recorded artwork needs repair; blue needs an artwork decision. **LAUNCH COMP** queues blue tracks; **EMBED Comp** queues the selected track for review. Use **Review queued work** to choose candidates and MusicBrainz releases with browser buttons. Authority fields can requery Artist, Release, and Recording IDs. **REPAIR COMP** repairs readable recorded thumbnails and preserves original embedded images in the cache.

Normal and COMP embedding write exact square JPEG thumbnails at `output.embedded_thumbnail_size` (default 1200). Audio container metadata and non-front artwork are preserved. Failed writes are reported; queued jobs do not report success merely because they were staged.

## Browsing and display

The workspace fills the browser width, including ultrawide monitors such as 3440×1440. Resize its panels with the pane splitters; the main workspace stays aligned with the full-width header.

Artists, Albums, and Fallback remember separate scroll positions during the browser session. Returning to the same artist or album restores the overview position; expanded album grids retain expansion and scroll. Biographies collapse to five lines, with same-size themed more/less text at the end of the final visible line. Dark, Chalk, and Light persist in the browser. Browser extensions that force colors can override the selected theme.

Media Filter retains the existing selected-item counter behavior. Status toggles narrow the list; this change does not introduce album-total status counters. Candidate Filter controls result visibility. Locked filters remain in place across selections.

Navidrome supplies biography, similar artists, and top songs when available. Indexed songs are identified separately from ranked metadata. Artwork counts refer to actual embedded track images, not folder sidecars.

## Data, recovery, and deployment

PostgreSQL is authoritative for WebGUI catalog and jobs. There is no silent SQLite fallback. Settings can explicitly import a read-only SQLite snapshot into an empty PostgreSQL catalog; paths must match the server's library. The source is left unchanged. Windows and WebGUI do not open the same live database; simultaneous writers to the same music library should be avoided.

Back up PostgreSQL using a consistent database dump, plus the config, credential, cache/staged-artwork, and any external artist-art volumes. Track changes require a music-library backup for full recovery. Do not treat a SQLite snapshot or the image cache as a PostgreSQL backup.

Run one WebGUI application process per catalog. Loopback operation may omit an access token; network binding requires one. Use HTTPS through a reverse proxy for remote access. The service rejects cross-origin writes and requires a session request token for mutations.
