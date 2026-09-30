# Python Ratatui TUI

The supported Python/Docker implementation includes an interactive terminal
interface rendered by Ratatui through the `pyratatui` Python bindings. The TUI
is the operational and decision layer around the existing SPLINED engine;
MusicBrainz authority, provider discovery, candidate ranking, output safety,
history, and writes remain owned by the engine.

The runtime pins `pyratatui==0.3.0` and packages a small
`splined-pyratatui-input` PyO3/crossterm extension for mouse/touch events,
terminal-image protocol detection, and native-image cleanup. The final Docker
image does not contain a Rust toolchain.

## Activation

An operational scan uses the TUI by default when stdin and stdout are
interactive terminals:

```text
splined --scan-dir
splined --scan-dir "10,000 Maniacs"
```

Use `--tui` to require it or `--no-tui` to keep the plain diagnostic stream:

```text
splined --tui --scan-dir
splined --no-tui --scan-dir
```

Redirected input/output and `docker compose exec -T` use the plain CLI.
Explicit `--tui` with non-interactive input or output fails clearly.

## Themes

Exactly two first-class themes are supported:

- `OLED` — true-black canvas with high-chroma semantic colors;
- `CHALK` — charcoal panels with muted mineral colors.

```text
splined --tui-theme CHALK --scan-dir
```

Theme selection changes presentation only. State meaning, candidate order,
selection authority, and engine behavior remain identical. Native artwork
cleanup repaints using the active theme background, so stale image strips do
not become black blocks in CHALK.

## Persistent Select Media startup

Python/Docker Select Media is loaded from:

```text
<scan.cache_dir>/splined.db
```

Default Docker path:

```text
/_cache/splined.db
```

### First launch

When the database is missing or incompatible, startup performs a one-time media
index build:

```text
Artist / Album folder inventory
        ↓
Mutagen read of one representative audio file per Album
        ↓
tag-identified Artist and Album rows
        ↓
cover.* and Selected Album statistics
        ↓
resumable per-Album SQLite checkpoints
        ↓
validated marker-last publication and WAL consolidation
        ↓
Select Media
```

The startup screen reports folder discovery and representative-tag progress.
Its title is `BUILDING ALBUM STATUS INDEX`. The application does not modify
music-library files during this build. If interrupted, the next matching build
reuses completed checkpoints rather than discarding all representative-tag
work.

### Warm launch

A warm launch is database-only for the Select Media read model:

```text
open splined.db
        ↓
optionally stage a temporary local read snapshot
        ↓
project current completion / bypass / timeout facts in memory
        ↓
load compact authority and Album rows
        ↓
derive physical Artist Picker folders from Album paths
        ↓
paint stable Album Status and pickers
```

The warm title is `LOADING ALBUM STATUS`; it is not presented as another first
database build. Warm hydration does not rewrite SQL. On a NAS/bind mount, a
WAL-free database may be copied to temporary container-local storage for the
read and deleted immediately afterward.

There is no background Artist-by-Artist structural validation and no color
change merely because the user opens an Artist. Opening an Artist is an
in-memory database view change, not a filesystem scan.

The retired `select-media-status.json` file is renamed `.legacy` after the first
usable database build and is no longer read or updated.

See [SPLINED media database](splined-media-database.md).

## Tag identity and Artist Picker ownership

The picker is populated from Mutagen tag data rather than absolute path
identity.

Artist identity:

```text
MusicBrainz Album Artist ID
or normalized ALBUMARTIST fallback
```

Album identity:

```text
MusicBrainz Album / Release ID
or tagged Artist + ALBUM + release-group ID + year + compilation fallback
```

Paths remain current locations used to process the Album folder. They are not
SQL identity indexes. SPLINED reads standards-compliant media tags directly and
does not require another application's database.

The Artist Picker is not a list of authority rows. Every Album is assigned to
the first physical directory below the configured library root:

```text
/music/Christina Aguilera/AGUILERA              → Christina Aguilera
/music/[Soundtracks]/A Star Is Born Soundtrack  → [Soundtracks]
/music/[Various Artists]/[Various Artists]/...  → [Various Artists]
```

Thus Album Artist/MusicBrainz identity continues to drive artwork authority,
while the filesystem layout controls where the Album appears and which folder
selection processes it.

## Select Media geometry

On wide layouts the top row uses four equal panels:

```text
25% S:P:L:I:N:E:D Album Status
25% Album Selection
25% Album Scanning
25% S:P:L:I:N:E:D Launch
```

The picker row uses:

```text
25% Artist Picker
50% Album Picker
25% Selected Album Artwork / Statistics
```

The embedded processing view keeps the same column boundaries so launching a
batch does not shift the workspace horizontally.

Compact and minimum breakpoints stack or reduce lower-priority surfaces rather
than allowing a table to cross its frame.

## Album Status

Album Status presents indexed Album facts and physical Artist-folder
aggregates:

| Album state | Color |
| --- | --- |
| Unprocessed | White |
| Processed | Orange |
| Bypassed | Red |
| Timeout active | Purple |

| Artist state | Color |
| --- | --- |
| Unprocessed | White |
| Partial | Purple |
| Complete | Green |
| Contains bypass | Blue |

Counts are right-aligned in a fixed second column. Artist and Album filtering
uses the same shared status semantics as processing.

A row does not change color because it was opened, scrolled, filtered, or
reached by a background worker. During a session, colors change only after a
visible SPLINED action or an explicit media-index Refresh.

## Explicit Refresh

`R` is the external-library reconciliation boundary.

Refresh inventories the complete Artist/Album topology, reuses representative
tags when the file path/size/mtime is unchanged, reads Mutagen tags for new or
changed Albums, refreshes local cover facts, and commits the replacement model
in one transaction.

The existing stable picker remains visible while Refresh runs. The TUI swaps to
the refreshed model only after the transaction succeeds; Artist colors do not
progressively mutate during the scan.

Use Refresh after external changes made by a tagger, a file
manager, or another media application. SPLINED's own LIVE WRITE and bypass
operations update affected database rows immediately.

## Album Selection

Album Selection defaults to:

```text
Select [NONE]
```

A direct Album-row click is an exclusive single selection and artwork/info
focus. Clicking another Album keeps the selected count at one and moves the
focus to that Album. `Ctrl+Click` toggles an Album while preserving all other
selections, allowing an arbitrary multi-Album set.

A direct Artist-row click replaces the current selection with that Artist's
White Unprocessed and Blue Incomplete Albums. `Ctrl+Click` toggles another
Artist additively, auto-selecting the same non-orange Albums for every selected
Artist. Orange Processed Albums are not auto-selected; Red Bypassed and Purple
Timeout Albums retain their protected behavior. Picker rows use status color
and selection emphasis without checkbox or checkmark glyphs.

Bulk selection is explicit:

- `Select [ALL]` — eligible Albums for the active Artist;
- `Select [FILTERED]` — Albums matching the active Artist/Album text filters;
- `Select [NONE]` — clear selection.

Red bypassed and Purple timeout-active Albums remain protected according to the
shared policy.

## Album Scanning and Launch

Album Scanning chooses scope only:

```text
Auto Scan [ALL]
Auto Scan [SELECTED]
```

For a selected Album whose representative track has no Album/Release ID and
has `compilation=1`, the panel also shows an orange, conditional row:

```text
Manual Scan [VA/OST Compilations]
```

This row is absent for normal Albums. Selecting it defines Manual Comp scope
but performs no MusicBrainz or artwork work until Launch is selected.

It does not silently choose READ. A Launch mode is mandatory:

```text
Launch [READ] Source Results
Launch [LIVE WRITE] Choice Results
```

Attempting Auto Scan without a Launch selection places the prompt inside the
Album Scanning panel.

READ can query and evaluate candidates but does not install `cover.*` or create
durable processed state for an Album that still has no cover. LIVE WRITE uses
the same decision pipeline and may write the selected artwork.

Manual Comp READ previews per-track embedded-artwork replacements. Manual Comp
LIVE WRITE replaces only an approved track's embedded front image. It never
creates, changes, or removes folder-level `cover.*`, and unresolved tracks keep
their existing artwork. Each successful track is committed immediately. If the
operator leaves before every track is complete, the Album name becomes blue
`Incomplete`; selecting Manual Scan later resumes after the verified completed
tracks rather than querying them again. This includes 0/N when the operator
leaves before the first approval. Escape leaves the current Manual Album for
the Album Run Report; Enter or Escape there returns to retained Select Media.

## Selected Album artwork and statistics

When focus moves to another Album, a compact green-framed/orange-text
`LOADING ALBUM INFO` indicator remains until the complete right-side artwork
and statistics model is ready.

Most normal Album changes are served immediately from SQLite.

### Existing cover

When canonical `cover.*` exists, the upper panel reads that local file directly
from disk and renders it through the terminal-native image protocol. The saved
resolution appears in yellow below the square preview.

The lower fixed-height panel is scrollable and includes:

```text
Path
Album
Artist
Year
Tracks
Artwork format counts
Root files
Cover* files and names
Other filenames
WebP found / size / resolution / conversion
```

### No cover

When no canonical cover exists, the old native overlay is cleared and the
upper panel shows:

```text
NO COVER-ART FOUND
USE
S:P:L:I:N:E:D LAUNCH
```

Album information remains in the lower scrollable statistics panel.

Native local artwork is not cleared and redrawn for unrelated TUI refreshes.
The overlay is replaced only when content, geometry, or workflow visibility
changes, preventing flicker while retaining correct cleanup on Album changes,
no-cover transitions, resize, and processing launch.

## Candidate decision screen

During Manual Comp review, `CURRENT ALBUM` divides into the curated Album
identity on the left and a yellow-framed, cyan-titled
`FALLBACK ARTIST / ALBUM INFO` panel on the right. Its three rows show the
friendly MusicBrainz Artist, recovered release, and matched recording names,
each followed by the exact Artist/Release/Recording ID. The magenta IDs carry
OSC-8 links to their corresponding MusicBrainz pages so the operator can
validate fallback authority before approval. Normal Album-ID scans do not show
this fallback panel.

Each row begins with a clickable `[E]` box. The editor accepts a canonical
MusicBrainz UUID; Enter updates the displayed session authority and immediately
re-queries MusicBrainz and artwork sources. Artist or Recording edits discard
the previously derived Release before lookup. A Release edit requests that
exact release and validates that it is Official and contains the selected
Recording and Artist. Unlike automatic discovery, an exact operator-selected
release may be any MusicBrainz release type, including a Single or EP.
`M` opens a fresh MusicBrainz Recording search using the local Artist and Track
title. These edits and search selections never write MBID tags; only approved
embedded artwork is written.

When local IDs are absent, ID lookup fails, or `M` is pressed, the lower
Candidate Decision area becomes `MUSICBRAINZ MATCHES`. Numbered rows are sorted
and labeled by recording Artist, release decade, and release type (Album,
Soundtrack, or Compilation), followed by the matched Track and release title.
The Artwork panel intentionally remains empty until a row is selected. Enter
on a keyboard choice—or Enter after clicking a row—uses that release as
session authority and runs the normal source candidate search. Escape from the
source candidates restores the same in-memory result list, without repeating
the MusicBrainz request, so another release can be tested. Escape from the
MusicBrainz list leaves the Manual Album and returns to Select Media.

If an edited ID is valid UUID syntax but MusicBrainz rejects the authority or
returns no artwork candidate, the candidate screen remains open with the
failure reason and all three `[E]` controls. Enter cannot select an empty row;
the operator can correct another ID, press `M` for Artist/Track discovery, leave that track unchanged
with `B`, or leave the Manual Album with Escape.

Candidates are grouped by source while sharing one terminal-cell grid. Depending
on responsive width, the grid may include:

```text
#
SOURCE
RESOLUTION
FORMAT
RANGE TYPE
DISTANCE
SQUARE
ACCEPTABLE
APPROVED
URL
```

When future AISPLINE presentation is enabled, placeholder AI columns use the
same grid. AISPLINE processing itself is not implemented and no AI result is
invented.

The preferred `(S)` candidate is determined by projected final policy and
quality. Local/Embedded and provider candidates participate in the same scale,
distance, shape, and transform comparison. Selecting Embedded artwork by `(S)`
or candidate number materializes the configured canonical cover output rather
than treating embedded bytes as an existing sidecar file.

The Artwork pane follows candidate focus. URL-backed candidates preview the
exact remote resource on hover, arrow movement, or numeric selection. This
preview is memory-only and does not install the image. The chosen candidate is
processed and written only when required by the final action.

Remote URLs and fallback MusicBrainz IDs use OSC-8 hyperlink metadata. SPLINED
does not launch a browser inside the Docker/SSH host.

Repeated identical provider diagnostics are collapsed in the final report with
a count rather than printed once per rejected reference.

## Mouse, touch, and scrolling

The crossterm extension supplies:

- mouse button down/up/drag/move;
- wheel and terminal-provided touch scrolling;
- keyboard and resize events;
- mouse-capture lifecycle and emergency terminal restoration;
- terminal image protocol detection.

Each redraw rebuilds structured hit regions from the current Ratatui `Rect`
geometry. Clicks are not inferred from painted strings. The region beneath the
pointer receives scrolling.

Keyboard operation remains fully supported.
Mouse events are hit-tested actions; they are not converted into Enter
keystrokes. A direct Album click performs its documented exclusive focus/select
action, `Ctrl+Click` preserves and toggles multi-selection, and a control click
invokes that control. The native crossterm bridge carries modifier state with
the mouse event; no additional widget framework is required.

## Processing and final report

READ/LIVE WRITE processing keeps a compact Select Media context above the
active Album, authority, provider, download, and candidate activity surfaces.
The right-side embedded panel remains Album-specific rather than reverting to a
generic media-library summary.

After a batch, the per-Album final report remains until Enter or Esc returns to
the same resident Select Media session. `q` exits. A cumulative failure exit
state is retained across multiple batches in the same TUI process.

The Enter/Esc used to leave the report is consumed by the report. A brief
activation guard on return prevents that same physical Enter press from also
opening the currently focused Artist or activating another Select Media
control.

## Debug diagnostics

Debug records are single-line and bounded to 2,048 characters. High-cardinality
state is summarized: selection changes log counts, launches include at most a
three-path sample plus an omitted count, and library snapshots report counts
rather than dumping every selected path or Artist state.

For retained-session and startup diagnosis, the most useful records are:

```text
splined.db.index_check
splined.db.warm_load.*
picker.library.reuse_session
picker.batch.continue_to_library
library_return_guard.armed
key.suppressed reason='batch-return-activation-guard'
```

## AISPLINE boundary

The `[aisplined]` Config v5 section is a reserved companion-product boundary.
The current release does not define an endpoint protocol or perform AI review
or enhancement. With `enabled = false`, no AI work occurs.

Responsive placeholder code is not an implicit AISPLINE product requirement.
The eventual companion UI contract should be established when AISPLINE
development begins.

## Safe terminal cleanup

On normal exit, Ctrl+C, SIGTERM, worker/TUI exception, and partial
initialization failure, SPLINED disables mouse capture and restores the
terminal. Native image areas are cleared with the active OLED or CHALK panel
background before Ratatui repaints the underlying surface.
