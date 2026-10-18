# Windows application guide

SPLINED for Windows is a native Rust application with a Slint presentation
layer. One Cargo-built `splined.exe` contains the application, workflow, and UI;
there is no managed frontend, application DLL bridge, or helper process.

The Slint interface uses the native graphical artwork workspace: a branded app
bar, graphical Media Library, switchable Find & Match, Edit, Upscale/Enhance,
and Advanced modes, dominant Selected Artwork preview, and horizontally
scrollable candidate/source strips. The presentation remains a projection of
the same Rust-owned state restored by the
[1.0.88 functional-parity audit](windows-functional-parity-1.0.88.md).

## First run and state

Portable mode uses the directory containing `splined.exe` as its writable state
root. MSIX mode obtains the current package family through Windows and uses:

```text
%LOCALAPPDATA%\Packages\<package-family>\LocalState\SPLINED
```

Configuration authority is discovered automatically; the normal startup path
does not ask for a configuration filename. Portable files are:

```text
<splined.exe directory>\data\config.toml       Config v5
<splined.exe directory>\data\ui.toml           interface preferences
<configured credential directory>\*.json       provider credentials
<scan.cache_dir>\splined.db                     media/workflow database
```

The packaged equivalents are
`%LOCALAPPDATA%\Packages\<package-family>\LocalState\SPLINED\data\config.toml`
and `data\ui.toml` beneath that same root. Existing absolute and UNC paths are
preserved; relative Config v5 paths resolve against the selected state root.

SPLINED does not generate a default Config v5 or create database, temporary
cache, log, credential, or obsolete bootstrap directories merely because the
executable launched. If Config v5 is absent, invalid, or operationally
incomplete, the ordinary workspace stays unavailable and a short **Welcome to
S.P.L.I.N.E.D.** Setup opens with the actual error. A new user browses to an
existing Media Library; the SQLite/database, temporary cache, log, credential,
sample, and backup locations are prefilled from the portable executable root
and remain individually browsable. Only **Save & Open Settings** validates the
paths, creates the configured application-owned directories, writes Config v5
atomically, loads the library, and opens the complete Settings screen for
review. `ui.toml` is separate UI state.

With an existing Config v5, startup opens `<scan.cache_dir>\splined.db`
automatically and reports the active state root, Config path, SQLite path, and
loaded Album/Artist/Folder counts without logging credentials.

Release builds also provide opt-in, read-only support diagnostics. Because the
Windows GUI subsystem does not attach stdout, pass an output filename:

```powershell
splined.exe --library-diagnostics library-diagnostics.txt
splined.exe --slint-library-diagnostics slint-library-diagnostics.txt
splined.exe --navidrome-diagnostics navidrome-diagnostics.txt
```

The library reports include the active paths, complete resident counts, Slint
model counts, and non-writing selection/navigation checks. The Slint report also
collapses and re-expands Media Library, verifies that its width returns at or
above the Rust minimum without losing the current model, and confirms that a
Custom Upscale window receives the complete resident Artist model. The
Navidrome report uses the configured credential file for a live capability
request but never records its secret.

## Media Library

The graphical left sidebar is projected from the resident Rust database model.
It provides three views over the same album authority:

- **Artists** shows aggregate artist status. Activating an artist expands its
  Albums directly below the Artist without leaving the Artist view. Each child
  Album has an explicit selected/deselected radio and a yellow Album name;
  Library Status still controls filtering and selection eligibility.
- **Albums** provides direct and additive album selection.
- **Fallback** displays one row per Album whose MusicBrainz Album ID is empty.
  MusicBrainz-identified Various Artists and other compilations stay out of this
  view. Activating a fallback Album expands only its cached compilation tracks,
  when present; tracks from every Album are never flattened into one repeated
  list. The Album radio selects the full fallback run independently of the
  expand/collapse control.

Media Filter is clipped and internally scrollable, so its Artist/Album searches,
three equal-width scalable Select Mode buttons, and Library Status radios cannot
bleed into the library list. Application text has a 14-pixel explicit minimum,
with 20–22-pixel radio glyphs and a 16-pixel native default.

The main window is resizable and maximizable. Artwork is clipped to its preview
card so a native-resolution cover cannot cover the library or intercept its
controls. The Media Library heading reports the current visible-row count, and
an empty compilation-track section yields its space to the scrollable library.
The library list retains a usable minimum height ahead of secondary status
content. When the sidebar is collapsed, its narrow rail always keeps a visible
expand control; expansion is requested through Rust, restores a clamped width
at or above the media-pane minimum, and immediately renders the resident model.

### Resizable workspace

The Artwork Workspace has native Slint drag dividers between the Media Library,
central mode workspace, and Selected Artwork preview. Horizontal drag areas
resize both side panes continuously while the center receives or releases the
remaining width. A vertical drag area adjusts the candidate-artwork tray. Each
divider has a visible hover state and a wider hit area than its rendered line.

Rust validates every requested size before Slint renders it. Media, center,
artwork, candidate-result, and activity regions retain minimum usable sizes;
saved values are clamped when the window, display resolution, or DPI changes.
Maximize uses the available space without replacing the last normal window
size, and restore returns to that normal geometry. A saved maximized main window
is applied once the Slint event loop is running so Windows performs a native
work-area maximize. Candidate Compare and Custom Upscale do not copy the main
window's maximized state; each opens as an independently resizable native window
and may be maximized or restored on its own monitor. Collapsing Media Library,
Artwork Preview & Editing, or Artwork Filter reclaims its space while retaining
the previous expanded width or height for the next expansion.

Pane geometry, expanded dimensions, the active artwork mode, normal window
size, maximized state, and the **System / Light / Dark** appearance choice are
presentation preferences in `data\ui.toml`. They are not Config v5 fields.
Older `ui.toml` files remain valid and acquire safe defaults for fields they do
not contain. A numbered-workspace layout revision retires obsolete temporary-
shell pane geometry without changing Config v5 or unrelated UI preferences.
Windows and Slint native per-monitor DPI are authoritative. SPLINED does not
infer an interface scale from desktop pixel dimensions, so a Retina-sized RDP
desktop cannot receive a second artificial 1.25x, 1.5x, or 2x scale. The main,
Custom Upscale, and candidate-comparison windows are independent native
PerMonitorV2 windows: each follows the DPI and Windows work area of the monitor
it currently occupies. Crossing monitors causes Slint to reflow from that
window's logical dimensions; pane minimums and saved splitter geometry are
never treated as physical pixels.

### Artwork workspace modes

- **Find & Match Artwork** combines the Config v5 source strip, tri-state
  filters, adjustment preview, Selected Artwork, and the candidate tray.
- **Edit Artwork** provides candidate source selection, a current-image canvas,
  all eight independently selectable adjustment controls, an **Activate All 8**
  action, and the existing preview/apply actions.
- **Upscale / Enhance** presents the real configured upscaler and opens the
  dedicated Custom Upscale before/after window; no model names or capabilities
  are invented by the interface.
- **Advanced** keeps source-priority controls, launch/stop actions, and live
  Scan Activity and Decisions accessible without displacing the main artwork
  workflow.

MusicBrainz Results temporarily occupies the central mode workspace while an
authority decision is pending. Results are grouped by release family and Album
Artist. Selecting a different release asynchronously loads its Cover Art
Archive front image into Artwork Preview. The single bottom **Open MB Page**
action is enabled only for a different selected release; every opened release
retains its purple **VISITED** history while selection remains cyan and current
authority remains yellow. The collapsible, yellow-outlined ID editor exposes
Recording, Artist, and Release MBIDs in labeled rows with one Apply action.

Text and status filters change only visibility. Artist and Album filters can
be combined with general search and multiple independently enabled status
categories. They never rewrite history, bypass, timeout, processed state,
provider policy, or database identity. Available states are Unprocessed,
Partial / Error, Artist Bypass, Artist Complete, Processed, Incomplete, and
Bypassed.

**All**, **Filtered**, and **None** operate on the Rust-owned selection set.
ALL respects an active artist scope; FILTERED requires an intentional filter
scope and selects reviewable results rather than hidden albums.
Bypassed albums require the explicit one-run override; active timeouts remain
protected. Incomplete compilation work remains eligible according to the
authoritative database snapshot.

**Fallback** projects Albums with no MusicBrainz Album ID first, regardless of
Compilation tags. It then shows the selected Album's cached tracks, filling any
missing rows with a read-only enumeration of that one folder, for direct
track-level reopen while retaining the parent Album's queue and completion authority.
Completed track-art ledger entries are green; tracks without a completed entry
are blue. Folder-level `cover.*` files are not used or counted.

## Running work

Choose an auto-scan scope and launch mode:

- **ALL** queues every currently eligible album.
- **SELECTED** queues only explicitly selected eligible albums.
- **READ** performs review without installing artwork or mutating album files.
- **LIVE WRITE** permits the selected artwork operation to update album files.

Launch is disabled when no valid queue exists. During a run the control state is
owned by `ScanController`, and **STOP** requests cancellation of the active
Rust operation, including an operator-decision wait. Albums are processed
sequentially; a failed album remains recoverable and later queued albums are not
silently marked complete.

## Candidate artwork and sources

The horizontally scrollable source-card row is derived from the Config v5
`cover_sources` order. Include,
exclude, and priority actions save back to the same authoritative source list;
there is no UI-only provider order. Local artwork retains its fixed review role.
Provider status and candidate counts are projections of runtime events.

The Candidate Artwork strip scrolls horizontally. Its cards contain real source
provenance, native/projected dimensions,
image type, range class, policy status, cache path, recommendation, comparison,
and selection state. Source, type, policy, and range controls filter the
resident result set. Each control cycles among include, neutral, and disregard
and the filter state persists in `ui.toml`. Filtering never repeats provider
discovery.

Optional hover preview does not replace the selected decision. **Compare**
opens a separate resizable horizontal comparison window. When no acceptable
candidate survives, the fallback reason and editable artist/album refinement
use the existing Rust retry decision.

The single run button is at the start of the workspace-mode row. It reads
**LAUNCH** while idle, **PROCESSING** while work is active, and **SKIP ALBUM**
while the runtime is waiting for an album decision. Clicking PROCESSING
requests the same controlled stop used by the Rust scan controller. Candidate
controls sit directly below the Candidate Artwork heading.

Available decisions include:

- Apply Selected
- Keep Local
- Bypass (orange while available)
- Bypassed (red only after SQLite records the bypass); clicking it asks for
  confirmation before the persistent bypass is cleared
- MusicBrainz retry/authority review
- Skip Album

In READ mode those decisions remain review-only. In LIVE WRITE mode the same
Rust pipeline is permitted to install validated output.

## MusicBrainz authority

**CURRENT ALBUM** is rendered separately from ordinary search results and is
the authority row. It carries the current recording, artist, and release IDs.
Applying IDs submits an authority decision to the Rust runtime. Ordinary result
selection retains its release-row identity and visited state; **Use Release**
is a separate action. Artist and release-type filters use the mature release
ordering without decade groups. Retry and Back to MB retain the active result
history. Editable recording, artist, and release identifiers are normalized
and UUID-validated before **Apply IDs**. After an authority choice the existing
artwork workflow resumes in the same run.

## Artwork editing and upscale

The editing area exposes Picture, Sharpen, Softness, Contrast, Exposure,
Brightness, Gamma, and Color/temperature. Values are application state, not
widget-local state. Saving defaults writes the established Config v5 output
fields atomically.

**Upscale Preview** renders the adjusted image without changing album files and
then changes in place to **Apply Upscale**. Applying follows the same selected
candidate path as **Apply to Selected**, so the user does not return to Find &
Match Artwork or press a second save button. Any slider change returns the
action to **Upscale Preview**. The real final-artwork code continues to preserve
native artwork above Ideal resolution, apply square/crop policy, and enforce
`upscale_max_percent`.

The resizable Original/Upscale comparison remains in the main Upscale / Enhance
workspace. All eight controls provide increment, decrement, and reset, update
the Upscale and Selected Artwork previews live, and never alter the Original
preview. Adaptive defaults remain Config v5 backed.

Existing-cover editing requires LIVE WRITE and a current local cover. It uses
the same validated final-artwork pipeline and safe replacement behavior.

## Settings and credentials

Normal Settings is a structured Slint surface for paths, scan behavior, SQLite
sharing, source order and exclusions, source policies, ranges, output and
square/upscale behavior, adjustment defaults, history/log retention, and
supported provider options. Controls update the Rust Config model, validate the
complete document, atomically write Config v5, and reload application state.
Application configuration is never duplicated in a Slint-specific authority,
and raw TOML is not the normal Settings workflow.

Provider credentials remain separate JSON files. The credential surface
supports Navidrome/OpenSubsonic, Fanart.tv, Last.fm, Discogs, and MusicBrainz.
On Windows, credential writes request user-specific filesystem protection and
report when the storage location cannot apply it.

Navidrome remains a supplemental source; `splined.db` is authoritative. A live
connection test calls the OpenSubsonic `ping.view` API. Password mode uses the
standard salted token contract, and API-key mode sends `apiKey` without sending
username/password. A successful status is shown only after a real server
response.

The Media Library requests the Artist and Album thumbnails exposed by
Navidrome/OpenSubsonic. Downloaded 192-pixel thumbnails are kept under the
portable state root at `data/navidrome-thumbnails`; SPLINED does not write into
the music library to render the list. Missing thumbnails refresh in the
background at startup and the selected Artist is retried immediately. Album
rows temporarily project a newly processed local cover until Navidrome's
read-only thumbnail projection catches up.

## Backup and restore

The maintenance page has exclusive **Backup** and **Restore** modes with separate
remembered, browsable paths. Windows backups are deliberately independent:

- settings, credentials, and interface preferences use a timestamped `.spl`;
- the authoritative library database uses a consistent SQLite online snapshot
  written directly as timestamped `.sqlite`;
- the secret-free diagnostic summary is a readable timestamped `.json` and is
  reference material rather than a restorable settings payload.

Each file type has its own exclusive Off, Daily, Weekly, and Monthly schedule,
so the database can be protected more frequently than infrequently changing
settings. Due jobs run only while SPLINED is open and idle and do not register an
operating-system background task. Manual `.spl` settings archives may be
password protected with the established PBKDF2-HMAC-SHA1, AES-256-CBC, and
HMAC-SHA256 envelope. Restore auto-detects `.spl` and `.sqlite`, validates the
selected file, uses atomic replacement, and immediately reloads the applicable
configuration or Media Library. Older `.spl` archives with an embedded database
remain restorable through the explicit legacy SQLite selection.

Opening an associated unprotected `.spl` file, or launching
`splined.exe --restore <file>`, automatically imports the archive's Config v5
settings before startup and then loads the configured database. Other backup
categories are not changed. If the archive is password protected or its Config
v5 cannot be imported, SPLINED opens the maintenance/restore surface with the
archive preselected so the operator can enter the password and review the
categories. Portable startup maintains the current user-scoped `.spl` shell
association; packaged installs use the manifest-owned association.

## Updates

Packaged installations query the native Microsoft Store API. Portable builds
read only the pinned official stable GitHub notification manifest and compare
semantic versions. Portable update handling is notification-only; SPLINED does
not download a replacement executable, invoke a cleanup shell, or overwrite its
running binary.

## Troubleshooting

- **No library rows:** verify `library.music_library`, the database path, and
  path mappings in Config v5, then rebuild the index.
- **Launch disabled:** select an eligible album; bypassed albums need a one-run
  override and timeout-protected albums cannot launch.
- **No candidate image:** inspect Activity and provider status, then verify the
  selected source credential and cache path.
- **Navidrome says unavailable:** use Test in Provider Credentials. A status is
  not considered connected until `ping.view` succeeds.
- **Packaged state appears empty:** verify the Store package family LocalState
  directory rather than the package installation directory.

Diagnostics redact secrets and retain a bounded application activity log.
