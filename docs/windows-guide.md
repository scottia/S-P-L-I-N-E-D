# Windows application guide

SPLINED for Windows is one native Rust/Slint application. This guide covers
installation, everyday artwork work, Settings, backup, updates, and support.
It does not apply to the separate browser/PostgreSQL WebGUI.

Maintainer build and packaging instructions belong in the
[Windows source/build guide](../windows/README.md). Exact Config v5 fields live
in the [configuration reference](config-v5-reference.md). Dated audits are
retained as historical evidence, not current operating instructions.

## Start here

| I want to… | Go to |
| --- | --- |
| Install SPLINED or complete first setup | [Install and complete setup](#install-and-complete-setup) |
| Find an Artist, Album, or fallback track | [Browse and select music](#browse-and-select-music) |
| Review or save a cover | [Review and save artwork](#review-and-save-artwork) |
| Save an Artist portrait or banner | [Artist Art](#artist-art) |
| Embed thumbnails in tracks | [Embed track artwork](#embed-track-artwork) |
| Repair a compilation track | [COMP and fallback albums](#comp-and-fallback-albums) |
| Configure providers or behavior | [Settings and providers](#settings-and-providers) |
| Back up, restore, or update | [Backup, restore, and updates](#backup-restore-and-updates) |
| Diagnose a problem | [Diagnostics and troubleshooting](#diagnostics-and-troubleshooting) |

Start with a backed-up library and **READ** mode. Read mode can create normal
application state such as logs, caches, and samples when configured, but it
does not install artwork in the music library. **LIVE WRITE** permits explicit
cover, Artist Art, and audio-tag changes.

## Install and complete setup

Windows has two supported distribution channels:

| Channel | Install/update behavior | Writable application state |
| --- | --- | --- |
| [Microsoft Store](https://apps.microsoft.com/detail/9p8g4gmbbvbs?hl=en-US&gl=US) | Windows installs, updates, and removes the package | Package-family `LocalState\SPLINED` |
| [Portable ZIP](https://github.com/scottia/S-P-L-I-N-E-D/releases) | Extract to a writable folder; updates are manual | `data` beside `splined.exe` |

The Store and Portable channels use the same x64 Rust/Slint application. The
Store installation directory is read-only; Portable keeps its state with the
extracted application.

### First setup

1. Start `splined.exe`. Missing or invalid Config v5 opens **Welcome to
   S.P.L.I.N.E.D.** instead of an initialized workspace.
2. Choose the Music Library.
3. Choose a persistent database directory for `splined.db`. New local
   configurations use `_db` by default.
4. Choose a separate Temporary Run Cache. New configurations use `_cache`.
5. Choose log, credential, sample, and V/A / OST Artist Images directories.
6. Leave **Shared/Network SQLite** off unless multiple native clients will open
   the same physical database and you have reviewed the
   [shared-file requirements](splined-media-database.md#before-enabling-shared-mode).
7. Select **Save & Open Settings**. SPLINED validates the paths, writes Config
   v5 atomically, loads the library, and leaves any error visible for correction.
8. Review Settings, configure credentials, refresh the library when needed,
   and begin with a small READ run.

A clean launch does not precreate Config, database, cache, log, credential, or
backup directories. They are created only by the corresponding saved setup or
operation.

### Where Windows keeps state

Portable mode discovers:

```text
<portable folder>\data\config.toml       Config v5
<portable folder>\data\ui.toml           interface preferences
<credential directory>\*.json             provider credentials
<database directory>\splined.db            media and workflow database
<temporary cache directory>\...            disposable candidates and previews
```

Store mode keeps `config.toml` and `ui.toml` below:

```text
%LOCALAPPDATA%\Packages\<package-family>\LocalState\SPLINED\data
```

Configured absolute, mapped-drive, and UNC paths remain unchanged. Relative
paths resolve from the active Portable or Store state root. SPLINED does not
use the Windows Registry as configuration authority.

### Move or restore existing settings

Setup can load an unprotected or password-protected `.spl` Settings Archive
into its fields for review before anything is saved. Correct paths after moving
a Portable folder, then select **Save & Open Settings**.

Double-clicking an unprotected associated `.spl` archive imports its Config
v5 settings before startup. A protected archive, or one that cannot be imported
directly, opens Backup / Restore with the file selected so its password and
categories can be reviewed. An archive using shared SQLite may offer to enable
the optional Windows `.client-lock`; choose it only when every client can
acquire that lock.

## Browse and select music

The left Media Library is a projection of the resident SQLite catalog:

| View | What it shows |
| --- | --- |
| **Artists** | Artists and their Albums; activating an Artist opens its overview and appearances |
| **Albums** | Direct Album browsing and selection |
| **Fallback** | Albums without a MusicBrainz Album ID, with their indexed tracks |

The Fallback rule is the missing Album ID—not a Various Artists or Compilation
tag. An indexed folder cover can identify a fallback Album card, but it is
never treated as embedded artwork for one of its tracks.

### Select work

- Click an Album for an exclusive selection.
- Use the Artist view to select that Artist's eligible regular, featured,
  soundtrack, and Various Artists appearances.
- Use **All**, **Filtered**, and **None** for deliberate bulk selection.
- Bypassed Albums need an explicit one-run override.
- Active timeouts remain protected.
- Fully green Albums are skipped by an Artist-wide launch unless you select an
  Album explicitly.

Text and status filters change visibility only. They do not alter database
identity, history, bypass, timeout, provider policy, or queued work. Locked
Media and Candidate Filter choices persist in interface state; unlocked choices
retain their normal fresh-run behavior.

### Refresh the catalog

The Home notice can apply added, changed, and removed library entries
incrementally. **Settings → Library & Files → Refresh DB** performs a deliberate
complete SQLite index rebuild. Neither action writes music artwork or inspects
every embedded image. Close other native clients before refreshing a shared
SQLite database.

Warm startup and ordinary browsing use the resident database rather than
rescanning the filesystem on every click. Use an explicit refresh after an
external tagger or filesystem operation changes the library.

### Read the visual status

Open the circled **?** beside Albums for the complete in-app key. The practical
summary is:

- Album dots show missing identity/artwork, processing, bypass, Artist Art, or
  embedded-thumbnail conditions.
- Card frames distinguish validation, completion, and confirmed embedded-art
  problems.
- **ARTIST**, **FEATURING**, **SOUNDTRACK**, **VARIOUS**, and **COMPILED**
  banners describe the Album's relationship to the selected Artist; they are
  not completion states.
- COMP track rows use blue for artwork still needed, purple for thumbnail
  repair/validation trouble, and green for complete.

More than one condition can be visible at once. For example, a completed COMP
Album can retain a missing-Album-ID indicator because that missing ID is what
makes it fallback work.

### Arrange the workspace

The main and comparison windows are resizable and follow Windows per-monitor
DPI. Drag the pane dividers to resize Media Library, the center workspace,
Selected Artwork, and Candidate Artwork. Collapsing a pane preserves its last
expanded size.

Window geometry, pane sizes, the active workspace, filter locks, theme, and
other presentation choices live in `data\ui.toml`, separately from Config v5.
Choose **System**, **Dark**, **Light**, or **Chalk** from the Theme menu.
Artist Overview can also show Similar Artists, combined Artist/Featured genres,
biography, and Navidrome Top Songs when that supplemental data is available.

The center workspace keeps its tasks separate:

- **Find & Match Artwork** — provider results, filters, MusicBrainz, and
  Selected Artwork;
- **Album Art Enhancements** — Album fitment and adjustment preview;
- **Artists Art Enhancements** — portrait, banner, and themed package work;
- **Album Track Embedding** — explicitly staged jobs only; and
- **Advanced** — source priority, run controls, activity, and decisions.

## Review and save artwork

### Regular Album cover

1. Select an Album and choose **LAUNCH**.
2. Review the source cards and Candidate Artwork. Source, type, policy, and
   range filters operate on the already discovered results; changing a filter
   does not contact providers again.
3. Select a candidate. Use **Compare** for a separate comparison window.
4. If identity is uncertain, use MusicBrainz Results before committing artwork.
5. Optionally open Album Art Enhancements or select **Upscale Preview**.
6. In READ mode, inspect the decision without writing. In LIVE WRITE, use the
   context-appropriate action:
   - **WRITE Cover** saves the prepared full-resolution Album cover.
   - **EMBED Batch** saves/stages the prepared cover and thumbnail for the
     embedding queue.

**EMBED Thumbs** prepares and previews the square track image but does not write
it. Selected Artwork shows Before/After dimensions, track count, and projected
size; **Varying Size** means current tracks do not all use the same dimensions.

Available decisions include **Apply Selected**, **Keep Local**, **Bypass**,
clear a saved Bypass, MusicBrainz review, and **SKIP ALBUM**. A controlled stop
or skip does not mark later work complete.

Candidate cards show source provenance, native and projected dimensions, image
type, range/policy result, recommendation, and selection state. Provider order
comes from Config v5; Local Files retains its fixed review role.

### MusicBrainz identity

**CURRENT ALBUM** is the authority row. It shows the current Recording, Artist,
and Release IDs separately from candidate releases. Results are grouped by
best match, same Artist, and other releases, with match percentages.

Hover a release to preview its Cover Art Archive image; click to lock that
preview. **Advanced IDs** accepts normalized Recording and Artist UUIDs, while
the selected result supplies the Release ID. **Apply IDs** and **Use Selected
Album** are deliberate authority actions; either returns to the same artwork
workflow. Matching local artwork can appear as an **Existing** choice and
remains available for optional upscaling or **Embed Existing**.

### Artist Art

1. Select an Artist and choose **Artist Art**.
2. Review portrait and banner candidates from the configured sources.
3. Select the portrait, banner, or both. The combined choice enables themed
   package adjustments.
4. Adjust independent zoom/position and enhancement settings if needed.
5. Choose **WRITE Artist Art** in LIVE WRITE mode.

Normal Artists use `artist.*` and `banner.*` in their Artist directory.
Artists represented only inside shared Soundtrack or Various Artists folders
use distinct files under **V/A / OST Artist Images Directory**, keyed by stable
Artist identity when available. A blank or disposable location is unsafe for
those managed images; keep it persistent and backed up.

A saved banner takes precedence over the Navidrome/background fallback.
Changing the portrait does not replace a dedicated banner. The Artist match
editor can save confirmed MusicBrainz identity, biography, and profile URL in
`splined.db`; it does not rewrite music tags.

### Editing and enlargement

Album and Artist enhancement workspaces provide fitment plus Picture, Sharpen,
Softness, Contrast, Exposure, Brightness, Gamma, temperature/color, Hue,
Darkness, and Tint controls. Saved defaults and named Custom A–D presets are
separate from the current preview.

**Upscale Preview** uses SPLINED's configured conventional enlargement and
final-artwork rules. It does not write a file. Regenerate the preview after
changing sliders. Artwork already above Ideal resolution is preserved rather
than reduced by the ordinary upscale option; output still follows square/crop
and maximum-enlargement policy.

An Album containing `cover.webp` has a separate WebP indicator. **Play
Animated cover.webp Preview** affects only the display. **WRITE Cover** can
create the required still `cover.jpg` while preserving `cover.webp`; audio
tags receive a static thumbnail, never an animation.

## Embed track artwork

The full-quality Album cover and the square image embedded in audio tracks have
separate purposes. **Embedded Thumbnail Preference** controls the track image
size (1200 pixels by default; 300–4000 supported). It does not shrink the saved
full-resolution `cover.*`.

### Regular Albums

1. Select the desired cover or Upscale Preview.
2. Choose **EMBED Batch**. This saves/stages the full-resolution cover and a
   verified thumbnail snapshot while leaving the current workspace open.
3. Open **Album Track Embedding**.
4. Choose **Start All Staged**.
5. Review failures and retry or remove only the affected job.

Opening the embedding tab does not stage the selected Album. **EMBED All
Albums** stages the focused Artist's eligible regular and featured Albums with
saved covers; COMP work remains track-scoped.

Queue colors are purple for staged, orange for running, red for failed, green
for completed, and blue when current tracks already have the preferred
dimensions. Blue describes dimensions, not image identity; starting the job
still embeds the chosen cover.

### COMP and fallback Albums

Each track in an Album without a MusicBrainz Album ID is reviewed independently.
The folder's `cover.*` is display identity only and is never substituted for
that track's embedded artwork.

- Select a track to review its own candidates and MusicBrainz result.
- **LAUNCH COMP** processes pending blue tracks in sequence.
- **EMBED Comp** writes only the selected track's prepared thumbnail.
- **REPAIR COMP** resizes readable, completed thumbnails that do not match the
  configured preference. It does not search for new artwork or change the
  folder cover.
- **COMP Report** groups an Artist's fallback tracks by Album. Green is
  complete, purple needs repair/attention, and blue still needs artwork.

Artist-wide **LAUNCH COMP** repairs eligible purple tracks first, then processes
remaining non-green tracks; completed green tracks are skipped. Every bulk
repair or write still requires its confirmation. Track progress remains in
SQLite so a later session can resume.

### Pause or stop work

Albums run sequentially. The main processing control requests a controlled stop
of the active operation, including a pending decision. Depending on state it
appears as **PROCESSING**, **STOP**, or **SKIP ALBUM**. A failed Album remains
recoverable; later queued Albums are not reported complete merely because they
were staged.

## Settings and providers

Settings uses task-based sections:

| Section | Owns |
| --- | --- |
| **Library & Files** | Library, SQLite, cache, log, credential, shared Artist Art, ignored folders, and Refresh DB |
| **Artwork & Embedding** | Output preservation, file formats, square policy, embedded size, WebP preview, and samples |
| **Enhancement** | Defaults, presets, palette, banner/frame styles, and resolution ranges |
| **Artwork Sources** | Provider enablement, exclusion, policy, and search priority |
| **Connections** | Provider credential files and connection tests |
| **Backup / Restore** | Settings archives, SQLite snapshots, diagnostics, and restore |
| **Processing & Logs** | Read/Write default, history, timeouts, logging, retention, and status bar |

Each section saves its own draft into one validated Config v5 document. There
is no separate Slint configuration authority and raw TOML is not the normal
workflow. **Advanced** reveals specialist controls without changing their
defaults merely by showing them.

Enhancement Settings includes global defaults, named Custom A–D presets,
resolution ranges, banner/frame palettes, the OLED chroma wheel, hue and
brightness ranges, synchronized frame/lettering, and Random Harmony. These are
preview/configuration tools; selecting or adjusting them does not write artwork.
The Help menu opens current help and an About card with version, license,
project, release, Store, and update links.

The Advanced **Reset / Validate / Save** action requires a second red
confirmation and typed `RESET`. It backs up Config v5, restores operational
defaults including READ mode, and preserves music paths, ignored folders,
database/cache/log/credential paths, shared-SQLite choice, database, music
files, and provider credentials. Interface layout/theme and backup schedules
are separate and are not reset.

### Provider setup

Credential JSON remains separate from Config v5 and SQLite. Windows supports
Navidrome/OpenSubsonic, Fanart.tv, Last.fm, Discogs, and MusicBrainz. Use
[Credentials and Provider Setup](credentials-providers.md) for file fields,
authorization, protection, and backup.

A provider participates in artwork search only when it is in Source priority,
not excluded, and enabled by its source policy. A successful connection test
does not enable it automatically. **Include in artwork search** adds an enabled
provider to the ordered list and removes its exclusion. Drag source cards to
change priority; save the section to apply the order.

Navidrome is supplemental: `splined.db` remains Windows catalog authority. A
connection is reported successful only after a real OpenSubsonic `ping.view`
response. Thumbnails are cached under the Temporary Run Cache and do not write
to the music library.

## Backup, restore, and updates

### Back up

Windows keeps three backup types separate:

| Type | File | Contents |
| --- | --- | --- |
| Settings Archive | `.spl` | Selected Config v5, credentials, and interface categories; optional password |
| SQLite Database | `.sqlite` | Consistent online snapshot of `splined.db`; can optionally include managed V/A / OST Artist images |
| Diagnostic Summary | `.json` | Secret-free support information; not restorable |

Each type can have its own Off, Daily, Weekly, or Monthly schedule and maximum
file count. Scheduled work runs only while SPLINED is open and idle; it does
not install a Windows background task. A limit of zero keeps all backups.
Retention removes only older timestamped SPLINED files of the selected type,
not unrelated files in the directory.

For complete protection:

1. Save pending Settings changes.
2. Back up both **Config V5 Settings** and **Interface Settings** when you want
   presets, theme, pane sizes, filter locks, banner/frame styles, and baselines.
3. Create a separate SQLite snapshot.
4. Select **Include V/A / OST Artist images** or separately back up that
   directory.
5. Back up the music library itself. SPLINED state backups cannot restore
   externally changed covers or audio tags.

### Restore

Restore detects `.spl` and `.sqlite`, validates the selected file, and
changes only the selected categories. Settings and credentials use atomic
replacement; SQLite restore uses its online backup API and reloads the Media
Library.

After restoring Interface Settings, close and reopen SPLINED before making
more interface changes so the restored window state wins over the running
window's autosave. Older `.spl` archives containing a database remain
restorable through **SQLite Database Backup**.

### Update

- **Microsoft Store:** **Check for Updates** uses the native Store API.
  Installation remains owned by Microsoft Store Library.
- **Portable:** SPLINED compares the installed semantic version with the
  official GitHub release notification. It can open the release page but never
  downloads or replaces its executable.

For a Portable update, close SPLINED and replace only `splined.exe` and
`README-WINDOWS.txt` from the new archive. Preserve `data`, SQLite,
credentials, cache, logs, backup history, and all configured external paths.

## Diagnostics and troubleshooting

### Create a read-only support report

Release builds provide opt-in diagnostics. Because the Windows GUI does not
attach a terminal, supply an output filename:

```powershell
splined.exe --library-diagnostics library-diagnostics.txt
splined.exe --slint-library-diagnostics slint-library-diagnostics.txt
splined.exe --navidrome-diagnostics navidrome-diagnostics.txt
```

Library and Slint reports check active paths, catalog/model counts, and
non-writing navigation. Navidrome diagnostics performs a live capability
request using the configured credential but does not record its secret.

Logs and diagnostic summaries apply central secret redaction. Verbose and
Debugger file logging begins after restart when enabled. Diagnostic timings can
separate provider, SQLite, cache, local file, and network-share delays without
making those logs application authority.

### Common problems

| Symptom | Check |
| --- | --- |
| Setup reopens | Read the visible validation error; correct the selected path or archive, then use **Save & Open Settings** again |
| No library rows | Verify Music Library and database paths, then apply the incremental update or deliberate **Refresh DB** |
| Album missing after an external move/tag edit | Run an explicit library update; warm browsing does not crawl the filesystem |
| LAUNCH is disabled | Select an eligible Album; resolve timeout protection or explicitly override a bypass |
| No candidate artwork | Check Activity, source enablement/order, credential status, policy filters, and cache path |
| MusicBrainz result is wrong | Review CURRENT ALBUM, refine Artist/Album identity, and validate IDs before applying |
| Navidrome is unavailable | Test the credential; success requires `ping.view` |
| Artist portrait/banner is missing after a move | Verify V/A / OST Artist Images Directory and the Artist-specific files |
| EMBED queue is empty | Stage work with **EMBED Batch**, **EMBED Album**, or **EMBED All Albums**; opening the tab does not stage |
| COMP track stays blue | Select the track or use **LAUNCH COMP**; a folder cover is not track artwork |
| COMP track is purple | Preview it and use **REPAIR COMP** when the saved image is readable but off-preference |
| Store state appears empty | Inspect the package family's `LocalState\SPLINED`, not the read-only installation directory |
| Portable state appears empty | Confirm you launched the intended extracted `splined.exe` beside its `data` folder |
| Shared SQLite is locked/unavailable | Stop other clients, verify the same physical database and share locking, and follow the [shared-database guide](splined-media-database.md) |

For status-color detail, see
[Windows Select Media and Status Colors](media-filter-status-colors.md). For
provider failures, see [API/OAuth validation](oauth-validation.md).
