# SPLINED WebGUI

This is the user guide for the Python/browser implementation. WebGUI handles
providers, image processing, library files, and tag writes; PostgreSQL stores
its catalog, completion ledger, audit events, administrator account, and
persistent work queue. Windows is a separate native Rust/Slint application with
its own SQLite store. The two runtimes share concepts, not a live database or
guaranteed interface parity.

Use the [Docker deployment guide](../docker/README.md) for containers,
networking, volumes, PostgreSQL, and backup. This page starts after the services
are running.

## First run

1. Open `http://127.0.0.1:8080` on the server or its configured address.
2. Create the single WebGUI administrator with a username, password,
   confirmation, and the deployment's `SPLINED_WEB_TOKEN`.
3. In **Settings**, choose server-visible paths for the Music Library,
   Persistent Cache, V/A / OST Artist Images, logs, and credentials.
4. Review output and provider settings, save, and choose **Full Refresh Library
   Index** for the first publication.
5. Begin in `read` mode. Change **mode** to `write` only after the reported
   decisions and backup plan are satisfactory.

The deployment token authorizes first enrollment and password recovery but is
never stored in PostgreSQL. Later visits use the saved username and password.
After an account exists, **Forgot password?** validates the unchanged token,
replaces only the password hash, and invalidates active browser sessions. It
does not change PostgreSQL credentials, catalog rows, or artwork records.

All WebGUI paths are paths on the server or inside the container, not on the
computer running the browser. A blank V/A / OST Artist Images Directory uses
Persistent Cache Directory itself. Any other value must resolve to a persistent
server-visible image directory.

### Index maintenance

On later launches WebGUI compares indexed Album folders and filenames without
opening every normal track. The header reports separate added, changed, and
removed counts before applying that delta. Changed COMP/Fallback Albums rebuild
their complete track identity inventory during the explicit update. Use
**Settings → Full Refresh Library Index** only for a deliberate full rebuild.

### Settings map

**Settings HELP** opens the authoritative
[Config v5 reference](config-v5-reference.md). **Advanced ON/OFF · All
Sections** is one workspace-wide state.

- **Library & Files** owns library, cache, log, credential, Artist Art, and
  optional Scan Folder paths. A blank Scan Folder uses Music Library.
- **Artwork & Embedding** shows the fixed filename stems and supported formats,
  preservation rules, embedded size, and optional sample output.
- **Enhancement** owns palette, presets, fitment, resolution ranges, and bounded
  conventional enlargement. WebGUI-only presentation choices are saved under
  `webgui_ui` in Config v5.
- **Artwork Sources** owns provider order and source-policy controls.
- **Connections** edits and tests the supported credential files. Saved secrets
  are masked and blank secret fields retain their existing values.
- **Backup & Recovery** owns settings export/restore and deliberate catalog
  maintenance.
- **Processing & Logs** owns mode, history, diagnostic level, and retention.

Container deployments normally supply PostgreSQL credentials through the
environment, making the Advanced connection field server-managed. Diagnostic
level `none` suppresses runtime/access logs after restart without deleting the
bounded PostgreSQL event history.

## Artwork workflow

1. Select an artist or album. **LAUNCH** on an artist queues review of its selected eligible albums; in an album view it searches that album's configured sources. **Artist Art** opens portrait and banner search. **MusicBrainz** searches alternate releases and lets you choose which release's artwork to inspect.
2. Choose a candidate or **Upload Art**. In Enhancements, use Filters, Fitment, and Detailed to prepare the image. Wheel to zoom and drag to position artist artwork. Proposed component radios choose what is previewed and written. Both radios allow writing the portrait and banner together; Themed Packaging applies shared adjustments.

Saved Artist Art is reloaded immediately after **WRITE Artist Art**. Browser image handles include the saved file version, so the Artist Overview portrait and banner cannot reuse stale pixels after an overwrite. An existing current package can be filtered, fitted, upscaled, and written directly without starting another search. **LAUNCH** explicitly searches the configured artist-image-capable sources: Local Files, Fanart.tv, Navidrome, Deezer, Last.fm, and Discogs. Album-only sources are not advertised as artist-image searches.

Artist Overview album cards are grouped as Artist releases, Featured appearances, Soundtracks, Various Artists releases, and COMP fallback albums. Each group is ordered from newest to oldest. COMP cards use the fallback folder's `cover.*` for display only; track artwork is still reviewed and embedded individually through COMP Report.

Artist identities are canonicalized before the overview is rendered. Name variants such as `*NSYNC` and `NSYNC`, or a legacy blank-identity row beside the same MusicBrainz identity, contribute to one Artist entry and one complete appearance list. The first selection hydrates and persists the Navidrome/OpenSubsonic appearance projection before loading its COMP summary, so the initial render cannot show only the primary albums. The persisted projection remains available if Navidrome is temporarily unavailable and is invalidated by an explicit catalog refresh.

Album status dots and frames follow the same meanings as Windows. PostgreSQL cover and embedded-track counts drive the current state, and a completed embedding job refreshes the visible cards without requiring the artist to be reselected. A grey **EMBED Batch** button means the album is already staged; it remains clickable and the queue prevents a duplicate job.
3. **WRITE Cover** saves the prepared cover. **WRITE Artist Art** saves the chosen components. Read mode disables these music writes. **Upscale Preview** uses conventional Lanczos enlargement toward Ideal, capped by **Max Upscale Percent**; it is not an AI reconstruction. **Upscale Below Ideal** is on by default; turn it off to preserve actual source resolution. Writing remains explicit.
4. **EMBED Batch** saves the prepared album cover and stages a verified snapshot for track embedding. It keeps the current workspace open. The **Album Track Embedding** button gains muted purple text, outline, and background while unfinished work exists.
5. Open Album Track Embedding and choose **Start All Staged Albums**. This is an album-only queue: only indexed Artist Albums with a MusicBrainz Album ID and supported tracks are eligible. Both **EMBED Batch** and **EMBED All Albums** enforce that rule in the browser and again on the server. COMP Fallback Albums and tracks remain in their track-scoped COMP Report and cannot become Album jobs.

Each queue card shows the staged cover, Artist and Album, completed/total tracks, preferred embedded size, original cover dimensions, state, and errors. **Pause After Album** lets the current Album finish and gates the next Album; **Resume Queue** clears that gate. **Pause ALL Albums** gates new Albums and pauses a running Album after its current track; **Resume All Albums** requeues paused Album work. **Retry** revalidates the staged Album before queuing it. **Remove All** is rejected while an Album is running or waiting.

PostgreSQL keeps queue order, progress, and state. The persistent cache keeps separate hashed snapshots of the full staged cover and the exact preferred-size thumbnail. Queue reads issue a new authorized image handle for the current server process instead of persisting a signed preview URL; stored snapshot names and Album paths are never accepted as unrestricted image requests. Therefore a browser refresh, new authenticated session, server restart, container recreation with the same volumes, and incremental or full index refresh retain staged work and its preview. Interrupted running work returns as paused. If an Album was removed, relocated, changed identity, or became COMP during a refresh, the card reports the invalidation and the job cannot start or retry until it is removed and the current Album is staged. Active Album work is retrieved independently of the mixed-job history window, so unrelated COMP, scan, and completed work cannot hide it. Completion is recorded only after every target track's artwork bytes and dimensions are verified. Catalog indexing and scan maintenance remain in status and logs; they are never presented as Album Track Embedding work.

An album without a MusicBrainz Album ID uses the COMP workflow. Its Artist Overview and Fallback album cards display the indexed folder `cover.*` when one exists, just like an album identity card. That display cover is never substituted for an individual COMP track's embedded artwork and is not an artwork candidate for that track. Artist selection reads only the indexed COMP card summary; it does not rescan fallback albums or build a full report. The Artist Overview retains that card inventory while you inspect other Albums. **COMP Report** deliberately loads the focused Artist's matching tracks from every COMP album in that Artist's indexed list, then validates recorded thumbnails without allowing stale requests to replace a newer Artist/report selection. Opening an Album from the separate **Fallback** library tab shows every indexed track in that one compilation. Track inventory is the only operational difference: both views use the same MusicBrainz search and percentage results, existing-album matches, candidate review, enhancements, upscale, repair, and one-track embedding workflow. Green means recorded artwork has a valid thumbnail; purple means a successfully read recorded thumbnail needs repair; blue needs an artwork decision. A temporary validation failure stays separate and is retried when the report is reopened rather than being mislabeled purple. Both **LAUNCH COMP** controls start the applicable review of blue tracks. Each track first shows its ranked MusicBrainz release results; artwork providers are queried only after one release is chosen, and repeated provider diagnostics are collapsed. MusicBrainz results show Recording/Artist authority, Artist and Release Type filters, MB links, Apply IDs, return-to-candidates, and Keep Current actions. Validated Apply IDs writes only Recording and Artist identity; the fallback Album remains without a Release ID. Candidate selection, enhancements, and Upscale Preview remain attached to the active track. **EMBED Track** writes only that track and prepares an exact square JPEG at `output.embedded_thumbnail_size` after the full-resolution edit/upscale pass. Completion refreshes the same COMP Report. **CLOSE REPORT** returns to the Artist Overview. **REPAIR COMP** repairs readable recorded thumbnails and preserves original embedded images in the cache. Read mode disables COMP launch and write actions.

**Play Animated cover.webp Preview** is off by default. Off shows the first still frame in the browser while retaining the original source bytes. On plays an animated WebP in Selected Artwork. Writing an unedited WebP preserves its animation and `.webp` extension; staged Album artwork also retains the `.webp` extension. Audio embedding remains a static square thumbnail at the configured preference size.

Normal and COMP embedding write exact square JPEG thumbnails at `output.embedded_thumbnail_size` (default 1200). Audio container metadata and non-front artwork are preserved. Failed writes are reported; queued jobs do not report success merely because they were staged.

## Browsing and display

The workspace fills the browser width, including ultrawide monitors such as 3440×1440. **Full Screen** in the header uses the browser's page-fullscreen API; browsers without that API can use **Add to Home Screen** instead. Resize panels with the pane splitters; the main workspace stays aligned with the full-width header. On desktop, overview buttons follow the bottom of the resized pane while its content scrolls above them. Artist, album, and fallback views retain their separate scroll positions when you return.

Artists, Albums, and Fallback remember separate scroll positions during the browser session. Returning to the same artist or album restores the overview position; expanded album grids retain expansion and scroll. Biographies collapse to five lines, with same-size themed more/less text at the end of the final visible line. Dark, Chalk, and Light persist in the browser. Browser extensions that force colors can override the selected theme.

Media Filter retains the existing selected-item counter behavior. Status toggles narrow the list; this change does not introduce album-total status counters. Candidate Filter controls result visibility. Locked filters remain in place across selections.

Navidrome supplies biography, similar artists, and top songs when available. If Navidrome has no matching Artist or biography, WebGUI confirms MusicBrainz identity and obtains biography text from Last.fm; that fallback profile is retained in PostgreSQL. Indexed songs are identified separately from ranked metadata. Artwork counts refer to actual embedded track images, not folder sidecars.

## Data, recovery, and deployment

PostgreSQL is authoritative for WebGUI catalog and jobs. There is no silent
SQLite fallback, and the native `scan.sqlite_shared` switch is not a WebGUI
setting. Windows and WebGUI do not open the same live database; avoid
simultaneous writers to the same music library.

**Catalog Maintenance → Import an existing SQLite catalog** is an explicit
one-time initialization operation. The path names a snapshot readable by the
server; it is not a client-side file picker or upload. The current importer
opens the source read-only and refuses a destination whose Album table is
populated. It copies recognized tables but does not validate a source schema
version, prove that every destination table is empty, rewrite stored music
paths, or preflight their compatibility with the server library. Use only a
disposable, path-compatible snapshot, or run a fresh WebGUI index. The source
is unchanged.

Back up PostgreSQL using a consistent database dump, plus the config, credential, cache/staged-artwork, and any external artist-art volumes. Track changes require a music-library backup for full recovery. Do not treat a SQLite snapshot or the image cache as a PostgreSQL backup.

**Settings → Backup & Recovery** can download or restore a secret-free current-settings JSON and download the shared settings presentation contract. Catalog Maintenance is grouped directly below Restore so full refresh and one-time SQLite import remain deliberate recovery-adjacent actions. Safe JSON includes the WebGUI palette and enhancement presets but excludes provider secrets and PostgreSQL contents. Sample cover writing is disabled by default and requires an explicit persistent directory when enabled. These files help compare deployments and prevent UI/default drift; neither replaces PostgreSQL, credential, cache, Artist Art, or music-library backups. **Processing & Logs** shows a bounded view of PostgreSQL WebGUI audit events; it never returns credential values.

Run one WebGUI application process per catalog. Loopback operation may omit an access token; network binding requires one. Use HTTPS through a reverse proxy for remote access. The service rejects cross-origin writes and requires a session request token for mutations.

The browser tab uses `/favicon.ico`. Help includes **Save SPLINED dashboard icon**, which downloads the separate 1024×1024 PNG at `/assets/splined-dashboard-icon.png` for dashboards and launchers.
