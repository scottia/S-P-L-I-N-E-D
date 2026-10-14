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
incomplete, the ordinary workspace stays unavailable and structured **Setup**
opens with the actual error. Setup collects the music library, SQLite/database,
temporary cache, log, and credential locations. Only **Save and Continue**
validates them, creates the configured application-owned directories, writes
Config v5 atomically, and loads the library. `ui.toml` is separate UI state.

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
model counts, and non-writing selection/navigation checks. The Navidrome report
uses the configured credential file for a live capability request but never
records its secret.

## Media Library

The graphical left sidebar is projected from the resident Rust database model.
It provides three views over the same album authority:

- **Artists** shows aggregate artist status. Activating an artist scopes the
  album set and cascades selection to eligible albums.
- **Albums** provides direct and additive album selection.
- **Folders** displays physical album paths while retaining the same album keys
  and selection set. Choosing any top-level Artists, Albums, or Folders tab
  clears an earlier Artist drill-down scope before rebuilding that view; a
  Folder list therefore cannot remain restricted to the last Artist click.

The main window is resizable and maximizable. Artwork is clipped to its preview
card so a native-resolution cover cannot cover the library or intercept its
controls. The Media Library heading reports the current visible-row count, and
an empty compilation-track section yields its space to the scrollable library.

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
size, and restore returns to that normal geometry. Collapsing Media Library,
Artwork Preview & Editing, or Artwork Filter reclaims its space while retaining
the previous expanded width or height for the next expansion.

Pane geometry, expanded dimensions, the active artwork mode, normal window
size, maximized state, and the **System / Light / Dark** appearance choice are
presentation preferences in `data\ui.toml`. They are not Config v5 fields.
Older `ui.toml` files remain valid and acquire safe defaults for fields they do
not contain. A numbered-workspace layout revision retires obsolete temporary-
shell pane geometry without changing Config v5 or unrelated UI preferences.
For Retina-sized RDP sessions that incorrectly report 96 DPI, SPLINED applies a
bounded startup scale fallback; ordinary native Windows DPI remains in control.

### Artwork workspace modes

- **Find & Match Artwork** combines the Config v5 source strip, tri-state
  filters, adjustment preview, Selected Artwork, and the candidate tray.
- **Edit Artwork** provides candidate source selection, a current-image canvas,
  five primary adjustment controls, and the existing preview/apply actions.
- **Upscale / Enhance** presents the real configured upscaler and opens the
  dedicated Custom Upscale before/after window; no model names or capabilities
  are invented by the interface.
- **Advanced** keeps source-priority controls, launch/stop actions, and live
  Scan Activity and Decisions accessible without displacing the main artwork
  workflow.

MusicBrainz Results temporarily occupies the central mode workspace while an
authority decision is pending. The current album stays distinct from ordinary
results, release pages can be opened directly, and refine/back/apply actions
continue to use the Rust run controller.

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

**Show Tracks / Embedded Art** projects cached compilation tracks for direct
track-level reopen while retaining the parent album's queue and completion
authority.

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

The bottom candidate strip scrolls horizontally. Its cards contain real source
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

Available decisions include:

- Apply Selected
- Keep Local
- Bypass
- NO Bypass (clear persistent bypass authority)
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

**Upscale Preview** preserves source resolution and disables the output edit
profile for the candidate decision. **Apply Upscale** enables the configured
profile and existing maximum-upscale enforcement. The real final-artwork code
continues to preserve native artwork above Ideal resolution, apply square/crop
policy, and enforce `upscale_max_percent`.

**Custom Upscale** opens a separate resizable Original/Preview comparison and
editing workspace with library and current-album context. Resolution and ratio
labels are display overlays only. All eight controls provide increment,
decrement, and reset; exactly five may be promoted and reordered as primary
controls. Adaptive defaults remain Config v5 backed.

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

## Backup and restore

The maintenance page exports or restores selected Config v5 settings,
credentials, SQLite, interface preferences, and diagnostics in the established
`.spl` envelope. Protected backups use PBKDF2-HMAC-SHA1, AES-256-CBC, and an
HMAC-SHA256 envelope compatible with existing Windows backups. Restore validates
paths and content and uses atomic replacement. A successful restore reloads
Config v5, SQLite, and the workspace immediately.

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
