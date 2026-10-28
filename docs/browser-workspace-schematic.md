# Browser workspace schematic

This is the comparison baseline for the SPLINED browser workspace and Windows GUI. Review it whenever either interface changes. The Windows implementation is in `windows/ui/app.slint`; the browser template is in `python/web_preview/index.html`, `app.css`, and `app.js`.

## Screen arrangement

```text
┌────────── Header: SPLINED mark + slogan | centered status | Theme Settings Help ──────────────────────────┐
│ Media Library          │ Find & Match | Art Enhancements | Album Track Embedding │ Selected Artwork             │
│ Artists Albums Fallback├───────────────────────────────────────────────────────────┤ artwork or package preview   │
│ Media/Candidate Filter │ Candidate Findings: source cards                         │ Upscale Preview              │
│ artist/album rows      │ Artist Overview: banner, round thumb, albums             │ WRITE Artist Art / Cover     │
│                       │ OR: Enhancements + image/banner panes                    │ EMBED Batch                  │
│                       │ OR: embedding queue (uses full center/right width)       │ source/type/dimensions/size  │
│                       ├───────────────────────────────────────────────────────────┴──────────────────────────────┤
│                       │ Candidate Artwork: LAUNCH, count, horizontal image cards                                │
└───────────────────────┴──────────────────────────────────────────────────────────────────────────────────────────┘
```

At narrower widths, the browser stacks these regions while keeping the same reading order and mouse actions. On desktop, the left library is persistent, Selected Artwork stays on the right, and candidates occupy the bottom. Album Track Embedding uses the middle and right width and hides the candidate dock, as in Windows.

## Region mapping

| Windows source | Browser element | Behavior to keep aligned |
| --- | --- | --- |
| `BrandMark`, `AppTheme`, and header actions near `app.slint:172`, `app.slint:22`, and `app.slint:1830` | `.app-header`, `.brand`, `.header-actions`, CSS tokens | Use the real SPLINED icon and slogan assets; preserve centered status and right aligned Theme, Settings, Help. |
| `MediaSelector` near `app.slint:561` | `.library`, `#artistList`, filter buttons | Artists, Albums, Fallback; expandable artist albums; filter controls and collapse affordance. |
| Workspace buttons near `app.slint:1927` | `#findTab`, `#enhanceTab`, `#embedTab` | Art Enhancements label changes with artist/album context. |
| `Candidate Findings` near `app.slint:2095` | `#sourceStrip` | Source icon cards, counts, and statuses. Only sources with returned candidates are selectable in the template. |
| `Artist Overview & Album Selection` near `app.slint:2114` | `#hero`, `#artistArtEntry`, `#albumGrid`, `.overview-actions` | Wide banner behind artist detail, round artist image with a small lower-left Artist Art entry radio, album cards and overview actions along the lower edge. |
| Artist/album enhancement workspace near `app.slint:2520` | `#enhanceView`, `.enhancements`, `#paneGrid` | Left vertical controls; current package, proposed artist, proposed banner; one album cover pane in album context. |
| `Album Track Embedding` near `app.slint:2634` | `#embedView`, `#embedQueue` | Queue starts empty and receives an album only after EMBED Batch is pressed. |
| `Selected Artwork` near `app.slint:2693` | `#review`, `#selectedFrame`, `#themedPackaging`, `.review-actions` | Current or proposed preview, source metadata, Themed Packaging for the combined artist package, Upscale Preview before WRITE, then EMBED Batch. |
| `Candidate Artwork` near `app.slint:2852` | `#candidateDock`, `#candidateGrid` | LAUNCH and a horizontally scrolling candidate strip. |

The browser assets under `python/web_preview/assets/` are copies of `windows/gui/splined-app-icon.png`, `windows/gui/splined-watermark.png`, `windows/gui/source-icons/*.svg`, `windows/gui/set-your-imagination-tv.jpg`, and `assets/branding/splined-slogan.svg`. If Windows branding changes, refresh those copies and compare the header and source cards.

## Mouse workflow and state

1. Click an artist or album in Media Library. The artist overview, selected art, album cards, and local candidate strip update together. A new artist clears the prior selection.
2. Press LAUNCH for an artist to fetch Fanart.tv and Deezer candidates. From a selected album, click the small radio at the lower-left of the artist thumbnail to open Artists Art Enhancements and start the same search. The orange action reads **Artist Art** while the artist search entry is available, then **WRITE Artist Art** once a proposed artist component is selected. A candidate click selects its artist image or banner independently. A source card with results filters the candidate strip; click again to clear that source filter.
3. Press Upscale Preview or the Art Enhancements tab to inspect the enhancement layout. Artist mode shows current package, proposed artist image, and proposed banner. The three pane radios choose current, artist, banner, or both. Both proposed radios show the combined package in Selected Artwork and enable the Themed Packaging preview control. Album mode shows its cover pane.
4. The vertical Filters, Fitment, and Detailed groups expand by mouse. Their controls are a layout reference in this template. Image processing, fitment writes, and WRITE Artist Art/Cover require the backend implementation and are disabled here. The header Theme, Settings, and Help actions open local template dialogs.
5. With an album selected, press EMBED Batch to place it in the **browser's in-memory preview queue** and open Album Track Embedding. Queue removal is mouse driven. Start/Pause actions are disabled until a real embedding service is connected. No track tag changes occur in this template. Merely opening Album Track Embedding never stages an album.
6. Drag the divider beside Media Library or Selected Artwork to resize either side pane. Drag the handle above Candidate Artwork upward to enlarge the candidate strip and its image cards. The candidate strip scrolls horizontally with the mouse wheel. Divider positions persist in browser storage and can also be changed with arrow keys when a divider has focus.

## Data and write boundaries

- The local template currently reads the isolated Python SQLite index and copied music files through `python/web_preview.py`. It uses neither the live Windows database nor the new PostgreSQL container yet.
- Candidate searches may request remote art. The template holds image bytes in memory for preview; it does not write artwork, thumbnails, tags, or a persistent candidate cache.
- The proposed PostgreSQL catalog should index the same mounted `/music` library, with artist, album, track, embedded-art status, and source references. Music files and approved `artist.*`, `banner.*`, and `cover.*` remain on mounted storage. PostgreSQL stores identity and indexing metadata, not duplicate image blobs.
- Before enabling WRITE or embedding in a deployed build, add authenticated mutating endpoints, recheck the live file and image identity at write time, and record the affected artist/album/track paths in separate Python logs. The Windows app must continue to own its existing database and write behavior independently.

## Container and domain path

The planned container listener is `0.0.0.0:8080`; the Compose service can publish `${SPLINED_PORT:-8080}:8080`. The dedicated `splined-postgresql` service stays separate on the same Compose network. Nginx Proxy Manager can forward `pixelized.ai` to `splined:8080` with TLS and an access policy. The current local test listener is `127.0.0.1:8080` and does not make the app publicly accessible.

In the supplied Compose environment, correct `DB_PASS: ${SPLLINED_POSTGRESQL_PASSWORD}` to `DB_PASS: ${SPLINED_POSTGRESQL_PASSWORD}` before container testing.

## Comparison checklist

- Header logo, slogan, status placement, and theme colors match Windows.
- Left library, top workspace buttons, source cards, overview, selected preview, and candidate strip retain their relative positions.
- Artist and album contexts change the enhancement tab and WRITE label together.
- Candidate and pane selections update the same preview without changing the current saved package.
- The artist shortcut starts its candidate search from a selected album. Resizing Candidate Artwork increases image visibility while leaving the library and selected preview in place.
- EMBED Batch is the only staging entry point; opening the queue alone leaves it empty.
- A Windows UI change updates this schematic, browser markup, and any copied assets in the same review.
