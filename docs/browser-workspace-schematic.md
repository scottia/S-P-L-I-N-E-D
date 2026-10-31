# Browser workspace schematic

This is a maintainer map of the WebGUI layout and its corresponding source
regions. It helps reviewers notice deliberate visual alignment with Windows,
but it is not a parity promise or user operating guide. The Windows
implementation is in `windows/ui/app.slint`; the browser interface is in
`webgui/static/index.html`, `app.css`, and `app.js`.

Use the current source and tests to establish behavior. The dated
[function and event-sequence audit](windows-web-function-audit.md) is retained
only as migration evidence.

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
| `Candidate Findings` near `app.slint:2095` | `#sourceStrip` | Source icon cards, counts, and truthful Ready/Results/Pending states. Clicking a source filters the candidate strip. |
| `Artist Overview & Album Selection` near `app.slint:2114` | `#hero`, `#artistArtEntry`, `#albumGrid`, `.overview-actions` | Wide banner behind artist detail, round artist image with a small lower-left Artist Art entry radio, album cards and overview actions along the lower edge. |
| Artist/album enhancement workspace near `app.slint:2520` | `#enhanceView`, `.enhancements`, `#paneGrid` | Left vertical controls; current package, proposed artist, proposed banner; one album cover pane in album context. |
| `Album Track Embedding` near `app.slint:2634` | `#embedView`, `#embedQueue` | Queue starts empty and receives an album only after EMBED Batch is pressed. |
| `Selected Artwork` near `app.slint:2693` | `#review`, `#selectedFrame`, `#themedPackaging`, `.review-actions` | Current or proposed preview, source metadata, Themed Packaging for the combined artist package, Upscale Preview before WRITE, then EMBED Batch. |
| `Candidate Artwork` near `app.slint:2852` | `#candidateDock`, `#candidateGrid` | LAUNCH and a horizontally scrolling candidate strip. |

The browser assets under `webgui/static/assets/` are copies of `windows/gui/splined-app-icon.png`, `windows/gui/splined-watermark.png`, `windows/gui/source-icons/*.svg`, `windows/gui/set-your-imagination-tv.jpg`, and `assets/branding/splined-slogan.svg`. If Windows branding changes, refresh those copies and compare the header and source cards.

## Mouse workflow and state

1. Click an artist or album in Media Library. The prior candidate strip and selected preview clear immediately while the next selection loads. The artist overview, selected art, album cards, and local candidates then update together. A late search response from the old focus is ignored.
2. Press LAUNCH for an artist to fetch candidates from the configured artist-image-capable sources: Local Files, Fanart.tv, Navidrome, Deezer, Last.fm, and Discogs. From a selected album, click the small radio at the lower-left of the artist thumbnail to open Artists Art Enhancements without starting a search. The orange action reads **Artist Art** while the artist-art entry is available, then **WRITE Artist Art** for the current or proposed package. A candidate click selects its artist image or banner independently. A source card with results filters the candidate strip; click again to clear that source filter.
3. Press Upscale Preview or the Art Enhancements tab to inspect the enhancement layout. Artist mode shows current package, proposed artist image, and proposed banner. The three pane radios choose current, artist, banner, or both. Both proposed radios show the combined package in Selected Artwork and enable the Themed Packaging preview control. Album mode shows its cover pane.
4. The vertical Filters, Fitment, and Detailed groups expand by mouse. Their sliders and fitment gestures change the in-memory browser preview. In live write mode the Python backend processes the preview and performs explicit WRITE Artist Art/Cover operations; read mode keeps music writes disabled. Theme selects Dark, Chalk, or Light. Settings reviews the server paths; Help opens the [WebUI guide](browser-workspace-guide.md).
5. With an album selected, EMBED Batch saves the prepared cover and stages a verified image snapshot in PostgreSQL without changing workspaces. The embedding tab gains a muted purple cue. Start, pause, retry, and removal operate on persistent jobs. Opening the tab never stages another album.
6. Drag the divider beside Media Library or Selected Artwork to resize either side pane. Drag the handle above Candidate Artwork upward to enlarge the candidate strip and its image cards. The candidate strip scrolls horizontally with the mouse wheel. Divider positions persist in browser storage and can also be changed with arrow keys when a divider has focus.

## Data and write boundaries

`webgui/server.py` serves the approved static interface and operation endpoints. Python handles providers, image processing and real artwork/tag writes. PostgreSQL owns catalog, completion ledger, audit records and persistent jobs. Read mode blocks music writes. Settings and provider credentials are editable in the browser; MusicBrainz authorization uses a server-side PKCE verifier.

Windows remains native Rust with SQLite. There is no shared live
PostgreSQL/Windows database. The explicit SQLite import copies recognized rows
from a read-only snapshot before destination Album rows exist; see the
[deployment guide](../docker/README.md) for its validation limits and the
[WebGUI guide](browser-workspace-guide.md) for operation.

## Comparison checklist

- Header logo, slogan, status placement, and theme colors match Windows.
- Left library, top workspace buttons, source cards, overview, selected preview, and candidate strip retain their relative positions.
- Artist and album contexts change the enhancement tab and WRITE label together.
- Candidate and pane selections update the same preview without changing the current saved package.
- The artist shortcut starts its candidate search from a selected album. Resizing Candidate Artwork increases image visibility while leaving the library and selected preview in place.
- EMBED Batch is the only staging entry point; opening the queue alone leaves it empty.
- A Windows UI change updates this schematic, browser markup, and any copied assets in the same review.

## Validation

Validate behavior through the current WebGUI tests, generated disposable media,
and the relevant Windows tests. The historical
[function audit](windows-web-function-audit.md) records what was checked during
the migration but is not proof of the current implementation.
