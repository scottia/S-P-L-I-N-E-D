# Windows guide

## Architecture and package

The Windows application restores the accepted 1.0.60 WinForms product
experience while retaining later correctness fixes. Its fixed runtime is:

```text
SPLINED\
  splined.exe
  runtime\
    splined-core.dll
  README-WINDOWS.txt
```

`splined.exe` is the WinForms frontend. It loads the normally shipped DLL from
the fixed `runtime` location and calls the authoritative Rust implementation
through a narrow C ABI. UTF-8 requests and structured callbacks carry scans,
progress, decisions, candidates, MusicBrainz matches, cancellation, errors, and
completion. Rust owns returned buffers and exposes their matching free
function; panics are contained at the boundary. Work runs away from the UI
thread, so the established LAUNCH / Skip Album / STOP interface remains
responsive.

There is one normal SPLINED process. The DLL is never embedded, extracted,
renamed, copied into an executable cache, or launched. Windows releases are
x64-only and both PE images are audited for AMD64 (`0x8664`).

The fresh archive does not contain `data\`. First save or one-time migration
creates `data\config.toml` and `data\ui.toml`.

## Media Library Selection

The left pane reads the authoritative SQLite media index and groups Albums by
Artist. Artist, Album, Folder Status, and **● Embedded Art** controls filter the
view. **● Embedded Art** is indigo and retains the established compilation-track
view behavior. Its checkbox, indigo bullet, and label use the same aligned
Folder Status row structure as the other status choices.
Album colors are White/Unprocessed, Blue/Incomplete, Orange/Processed,
Red/Bypassed, and Purple/Timeout. Green and Blue Artist markers retain the
Artist Complete and Artist Contains Bypass meanings.

A plain Album click replaces the current Album selection; Ctrl+Click toggles
it additively. Selecting an Artist cascades to its eligible Albums. Select
`[ALL]` applies to the active Artist, Select `[FILTERED]` applies to Albums
matching the active text filters, and Select `[NONE]` clears the transient
selection. Processed Albums require direct or filtered selection for deliberate
reprocessing. Bypassed Albums require confirmation for a temporary run-only
override, while timeout-active Albums remain protected.

Track focus and explicit targeting remain separate:

- clicking a track changes preview/focus only;
- checking one or more tracks intentionally queues those exact tracks;
- selecting or launching the Album clears the explicit track target and starts
  normal Album resume.

Track checkboxes are additive within the focused fallback compilation. A plain
checkbox click or Ctrl+Click on the track text can add/remove tracks. After a
targeted track completes, SPLINED keeps the compilation Album expanded and in
focus, turns the completed track indicator green, and leaves any other checked
tracks queued.

Selection history controls live at the right edge of the Media Selection filter
bar and remain visible as the pane is resized.

READ performs a fully evaluated dry run. LIVE WRITE may install selected
artwork. Auto Scan `[SELECTED]` uses only the explicit queue. Auto Scan `[ALL]`
adds every unprocessed/incomplete Album without bypassing protected states.
Auto Scan accepts only policy-eligible automatic decisions; otherwise the
reusable LAUNCH / Skip Album / STOP lifecycle pauses for input. While a
decision is waiting, the orange **Skip Album** header action always asks for
confirmation and advances without recording a bypass. The separate orange
**Bypass Album** candidate action records the deliberate bypass outcome.
When the current Album already has saved bypass authority, that action becomes
**NO Bypass**. It asks for confirmation, removes only the saved SQLite bypass,
keeps the current source results open, and then returns to **Bypass Album**.

Every queued Album carries its own indexed Album path and SQLite key into the
in-process Rust scan context. A multi-Album batch therefore records each result
against that Album's own database identity rather than the currently focused
row.

Before invoking Rust, Windows may resolve an equivalent physical network-folder
spelling for Unicode, recoverable encoding, or a private-use trailing-period
representation. This changes only the path used for file I/O; the queued
canonical Album path/key and SQLite status authority remain intact.

## Scan activity and candidates

The main workspace shows Album progress, provider activity, decisions,
diagnostics, and run completion. Candidate cards preserve source, image type,
resolution range, policy, ranking, and selected/recommended state. Artwork
Filter selections narrow actual candidate values. Advanced controls accompany
the selected candidate and include upscale defaults, picture, sharpen,
softness, contrast, exposure, brightness, gamma, color temperature, and
existing-cover editing. Compare and full-size preview use the cached candidate
bytes that the Rust backend evaluated.

The single unframed arrow immediately before **Candidate Findings | Upscale
Artwork Editing** expands or collapses one unified workspace. Its unframed outer
host contains four thin spectrum-framed panels: **Candidate Findings**, **Source
Selection**, **Resolution**, and **Upscale / Advanced**. The first three retain
the height of the tallest summary, and the fourth frame encloses the shared
Upscale action buttons and all eight sliders. Each editing control remains an
unframed template within that shared fourth panel: a centered title, exactly 20
segmented spectrum bars, compact one-step up/down glyphs, and a centered reset
glyph. The current signed value is painted as a bold white number in the middle
of each spectrum bar without angle brackets. Mouse, wheel, keyboard, reset, and
persisted-value behavior remain unchanged.

SPLINED measures the live editor width in DPI-normalized pixels instead of
guessing a physical device from its resolution. Under 640 pixels uses Compact,
640 through 960 uses Standard, and over 960 uses Spacious. Every density keeps
all eight controls in one centered row. Module width, gaps, and segment height
adapt to the available editor panel; Spacious caps control width so Retina and
other high-resolution Remote Desktop sessions gain breathing room instead of
oversized controls. The row is recalculated after startup, maximize/restore,
display or RDP resolution changes, per-monitor DPI changes, and splitter
movement. Resize events update existing bounds without rebuilding the controls,
preventing preview-selection flicker. Expanding the workspace restores the three
summaries and the same attached slider row together. The slider row does not wrap
or require a scrollbar.

Individual slider modules remain unframed inside the shared fourth
Upscale/Advanced frame. Preview actions use SPLINED's shared action font, glyph,
and framed-button treatment.

**Upscale Preview** is enabled by default. It keeps Upscale and full-size
preview available, but **Use Selected** saves the chosen source at its source
resolution without applying upscale or edit-profile filtering. Turn it off to
enter explicit **Upscale** save mode. The former framed resolution tile beside
**Upscale Show Full** has been removed. Its source aspect ratio and projected
dimensions now appear as unframed yellow text at the bottom of **Source
Selection**, using otherwise unused summary space and leaving the Upscale action
row uncluttered. The text stays on one line when space permits and wraps into
short complete lines at compact widths rather than truncating a dimension.
Both preview buttons remain available across candidate-state changes; when no
editable result is selected they give selection guidance instead of becoming
disabled. A refreshed result set without a formal recommendation focuses its
first editable result without submitting it, so Upscale / Advanced never loses
its preview target merely because MusicBrainz returned a different release.
Re-focusing the active Album in the media tree preserves the current candidate
or MusicBrainz decision workspace; it does not restart or close the run. When
Config v5 enables square crop output and the Rust projection is
square, the in-memory Upscale preview uses the same centered square source crop
before resizing, so the preview does not stretch a near-square source.
Settings remains available for passive local/candidate previews and is blocked
only while processing or an active decision is waiting.

The vertical divider immediately left of the Artwork preview is draggable.
It and the horizontal divider below the preview move as one constrained corner:
dragging either direction derives the other dimension so the outer Artwork pane
remains square. The vertical divider terminates on the horizontal divider, and
the upper editing workspace and Artwork pane end on that same line without a
dead strip below either panel. If the square preview would leave too little room
for the eight-control editor row, both preview dimensions shrink together until
the Compact row fits.
This coupling is isolated from the separate divider beside **Select Media**.
The preview width is stored with the other portable interface splitter state in
`data/ui.toml`.
Saved divider widths are clamped and deferred until WinForms has a valid layout
extent, including first startup at the minimum supported window size.

In **Settings > Sources & Matching**, source controls and their effective-policy
preview share one scroll surface. The policy preview has a bounded content
height; neither column expands into a large empty independently framed region.

## MusicBrainz Matches

`[CURRENT ALBUM]` is a separate yellow authority category fixed above every
ordinary result. Its one `[*]` row represents the Artist, Release, and Recording
IDs shown in the authority fields. Its Release Type cell is blank. It is never
derived from the first result and never participates in filtering or sorting.

Apply IDs submits the edited authority to the Rust backend. Only the authority
row and its release-page target change through that action; ordinary result
identity, URL, index, and visited state remain independent.

Artist and Release Type filters operate on ordinary row values. Ctrl supports
multi-select and Enter applies. Release Type choices narrow with the Artist
filter, visible results satisfy both filters, and empty Release Type groups are
omitted.

Ordinary results sort deterministically:

1. named Artists A–Z;
2. SOUNDTRACK;
3. COMPILATION;
4. remaining Various Artists;
5. within each family, newest Date first with unknown last;
6. US Country first, then alphabetical, then unknown;
7. Release title A–Z, then Release ID.

A Various Artists SOUNDTRACK or COMPILATION belongs to its Release Type family.
The most recently visited Release is green; earlier visited Releases are orange.
Navigation back from source results retains this state.

Selecting an ordinary result keeps that result's Cover Art Archive preview in
the Artwork pane when the pointer leaves its URL. Returning to source results
restores the cached candidate decision. While MusicBrainz owns the pending
decision, source-candidate actions are unavailable, which prevents an artwork
choice from being sent to the wrong Rust decision boundary. The in-process
boundary also rejects a mismatched decision explicitly instead of waiting
indefinitely.

## Compilation resume and fallback artwork

Normal incomplete compilation launch consults the SQLite completion ledger and
starts at the first unfinished track whose recorded authority does not already
match. A track merely clicked for preview never becomes an explicit target.
Checking a completed track intentionally reopens that track.

The native scan boundary treats a missing, blank, or whitespace-only explicit
track path as normal Album resume. Only a real checked-track path enables
targeted edit, so an empty UI selection cannot bypass the completion ledger or
prevent the fallback compilation workflow from loading.

During an active compilation decision, clicking another track changes only the
embedded-art preview. It does not clear the current candidate cards, change the
processing target, or interrupt the SQLite resume cursor. Successful per-track
writes turn the corresponding track indicator green immediately. Embedded
preview cache writes are serialized and reuse identical cached bytes so rapid
focus changes cannot race the in-process scan.

When a checked track is intentionally reopened, its current embedded image is
shown as a local comparison candidate. That preview is not release authority
and does not bypass MusicBrainz Matches or provider discovery; the operator can
still choose another release and select higher-resolution artwork.

In **● Embedded Art**, clicking a pending fallback compilation's Album row
selects that Album for normal SQLite-ledger resume and enables **LAUNCH**.
Clicking a track remains focus/preview only; checking one or more tracks
requests explicit reopen of exactly that additive queue.

The compilation-track-started event moves focus to the actual current track but
does not set an explicit target. Fallback compilation preview reads only that
track's embedded front artwork. If none exists, the preview says **No embedded
artwork**. Folder-level cover files are not displayed or modified by this
workflow. Successful per-track writes are recorded durably before advancing.

## Portable Config v5 and interface state

In portable mode the executable directory is the portable root. Relative paths
resolve from it, not process CWD. Moving or copying the complete directory
carries settings. Absolute and UNC paths remain exact.

In installed MSIX mode the package installation directory is read-only. Config
v5 and UI state therefore live under the package's stable per-user LocalState
directory, and relative configured paths resolve from that writable state root.
Existing absolute, mapped-drive, and UNC resources remain exact. Selective
`.spl` backup/restore is the supported transfer path between portable and
installed copies.

Config v5 retains authority for the music library, database directory,
temporary cache, credentials, logs, history, and external path mapping. The
SQLite file remains `<scan.cache_dir>\splined.db`; the frontend does not create
another database.

When both portable files are absent, a legacy ConfigV5/UiV4 pair may be read
once. The migration writes the portable files and records completion without
deleting legacy values. Thereafter only portable files are runtime authority.

## Credentials

Credentials edits only the fixed provider JSON files under the configured
credential directory. MusicBrainz and Last.fm authorization starts and
completes through in-process Rust commands. Credential paths may be relative to
the portable root or absolute/UNC. Updates do not target credential files.

## Backup and restore

Selective `.spl` export supports Config v5, interface state, credentials,
SQLite, and diagnostics. Optional password protection remains compatible with
the established format. Restore validates its envelope and contents, then uses
recoverable file replacement.

The Windows shell association for `.spl` is intentionally separate from
Config/UI authority. Portable SPLINED registers its current executable as the
backup opener. Installed SPLINED declares the association in its MSIX manifest.
Opening an existing `.spl` file launches the selective Restore surface with
that path pre-filled.

For the same backup and category selection, two clean portable directories
produce equivalent Config v5 and UI state. Selected categories overwrite their
destinations. Unselected categories stay unchanged. Registry contents,
previous roots, and unrelated folders do not affect restore.

## Distribution, updates, and trust

The normal Windows release produces synchronized Portable and optional Store
outputs from one build:

```text
dev -> main -> SPLINED (All OS) and GHCR
    -> one x64 Windows build
    -> Portable ZIP
    -> optional unsigned Store MSIX from the same binaries
    -> successful selected-platform/package validation
    -> numeric tag -> git-cliff notes -> GitHub Release/GHCR
    -> when Store=true, automatic Store package/listing submission

existing numeric tag -> SPLINED (Tagged) > MS Store Package Resolution
                     -> validated recovery MSIX + Store Highlights only

existing numeric tag -> SPLINED > MS Store Publish & Update
                     -> validated MSIX + Store submission/update

existing numeric tag -> SPLINED (Docker) > GHCR Resolution
                     -> exact Docker image + existing Release body refresh
```

The recovery workflows do not create or alter release tags. The Store package
resolution workflow stops at an Actions artifact; the Store publish workflow
updates the existing live product; the Docker workflow never touches Store or
creates another GitHub Release.

The primary GitHub artifact is `splined-windows-x86_64.zip`. Its allowlist is
exactly `splined.exe`, `runtime\splined-core.dll`, and `README-WINDOWS.txt`
under the `SPLINED` directory. A fresh archive contains no `data\`.

Portable update checking is notification-only. It can show release information
and open the official release page, but it cannot download or replace binaries.
Availability is based only on numeric semantic release versions: the advertised
version must be newer than the installed version. A commit difference at the
same version is not an update, and commit IDs are absent from normal update UI.
Manual current checks report **No SPLINED update is available.**; startup checks
are silent when current. The user closes SPLINED and replaces only the three
program/documentation files; portable `data\` and every configured external
resource remain outside that operation.

The installed Microsoft Store channel is an x64 MSIX created with upstream
Microsoft WinAppCli. Its exact production identity is `Psycotix.SPLINED`, its
publisher is `CN=FE370EF6-D95D-4A6F-9AAB-2654E6DE00FE`, and its publisher
display name is `Psycotix`. The package family name is
`Psycotix.SPLINED_8pvn5te36e43t`, and the Store ID is `9P8G4GMBBVBS`.
Certification is approved and the listing is live. The managed-install/update
Windows option is
[Microsoft Store — Install SPLINED](https://apps.microsoft.com/detail/9p8g4gmbbvbs?hl=en-US&gl=US).

**Help > Check for Updates** uses `ConfigStore.IsPackaged` to select exactly one
authority. Portable installations query GitHub Releases and
`windows-update.json` and compare semantic versions. Store installations query
`Windows.Services.Store.StoreContext`; **UPDATE NOW** delegates download and
installation to Windows/Microsoft Store, and **LATER** defers it. Store checks
await the projected WinRT operation rather than reflecting members from a raw
COM object. If the Store API cannot be used safely, the GUI offers the official
Store page. A packaged installation never receives Portable ZIP replacement
instructions.

`SPLINED (All OS) and GHCR` can build the production Store layout during the
same Windows job as the Portable ZIP. Both outputs use the exact same clean
AMD64 executable and DLL. A selected Store packaging or QA failure fails that
Windows job and therefore prevents numeric tag creation. The Store MSIX is an
Actions-only Partner Center artifact: it is not attached to the GitHub Release
and is omitted from public release checksums. With **Microsoft Store** enabled,
the validated package and generated en-US Store Highlights are submitted
automatically after GitHub Release and GHCR publication. With it disabled, no
Store package, QA, authentication, Store-submission artifact, or API submission
step runs. The
production layout never
receives a debug identity or development signature. A separate temporary layout
receives debug identity and a disposable certificate for install, activation,
native-core, and `.spl` association QA, then is removed. Development
certificates and private keys are never committed or published.

`SPLINED (Tagged) > MS Store Package Resolution` repeats the Store build and
validation when an already published tag needs recovery or repackaging, without
publishing it. `SPLINED > MS Store Publish & Update` performs the same exact-tag
build and QA before automated publication to the existing Store product. Both
render Store Highlights from the same authoritative git-cliff 2.14.2 context
used for GitHub release notes. Neither creates a version, tag, GitHub Release,
or GHCR image. See the [release automation guide](release-automation.md) and
[Microsoft Store channel guide](windows-store.md) for the live installation,
production identity, and future-update artifact contract.

Store-installed updates use Windows/Microsoft Store package deployment, not
SPLINED binary self-replacement. The checked-in App Installer template is kept
only for a possible separately signed direct-distribution channel and remains
disabled while its HTTPS placeholders are unresolved. The portable ZIP remains
an equally supported no-install/direct channel with portable-root state.

Release CI caches Cargo dependency downloads only. Compiled `windows/target`
artifacts are deliberately excluded because each release performs a clean
version/commit-bound build and audits the resulting executable and DLL. The
cache key normalizes only the release package's version, allowing unchanged
dependencies to reuse the same small cache without restoring or resaving stale
multi-gigabyte build directories.

## Diagnostics

Run Activity shows the live event stream. Persistent logs and run reports stay
in Config v5 locations. Reports should include the application version, run
mode, Album path, relevant events, and sanitized provider errors; never include
credential contents.

Unexpected WinForms interface exceptions are written to the current run log
with their exception type, message, stack, and bounded inner-exception chain
before SPLINED offers to continue. The dialog identifies the diagnostic file so
a continued session does not discard the evidence. Transient splitter extents
during startup, DPI changes, Remote Desktop display changes, and live resizing
are rejected safely and restored on the next layout pass.
