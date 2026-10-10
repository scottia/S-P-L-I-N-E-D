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
Artist. Artist, Album, Folder Status, and Show Tracks controls filter the view.
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

## Scan activity and candidates

The main workspace shows Album progress, provider activity, decisions,
diagnostics, and run completion. Candidate cards preserve source, image type,
resolution range, policy, ranking, and selected/recommended state. Artwork
Filter selections narrow actual candidate values. Advanced controls accompany
the selected candidate and include upscale defaults, picture, sharpen,
softness, contrast, exposure, brightness, gamma, color temperature, and
existing-cover editing. Compare and full-size preview use the cached candidate
bytes that the Rust backend evaluated.

The single toggle immediately before **Candidate Findings | Upscale Artwork
Editing** collapses only the three finding/source/resolution summaries. Upscale
controls remain visible and switch to two rows of four taller, centered sliders
with one-step arrow controls. The workspace scrolls vertically rather than
clipping sliders when the candidate-results pane is enlarged.

**Upscale Preview** is enabled by default. It keeps Upscale and full-size
preview available, but **Use Selected** saves the chosen source at its source
resolution without applying upscale or edit-profile filtering. Turn it off to
enter explicit **Upscale** save mode. The framed resolution tile beside
**Upscale Show Full** shows the source aspect ratio and projected dimensions.
Settings remains available for passive local/candidate previews and is blocked
only while processing or an active decision is waiting.

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

In **Show Tracks**, clicking a pending fallback compilation's Album row selects
that Album for normal SQLite-ledger resume and enables **LAUNCH**. Clicking a
track remains focus/preview only; checking one or more tracks requests explicit
reopen of exactly that additive queue.

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
installation to Windows/Microsoft Store, and **LATER** defers it. If the Store
API cannot be used safely, the GUI offers the official Store page. A packaged
installation never receives Portable ZIP replacement instructions.

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
