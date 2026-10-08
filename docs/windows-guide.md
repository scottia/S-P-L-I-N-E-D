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
thread, so the established LAUNCH / WAITING / STOP interface remains
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
- checking a track intentionally targets that exact track;
- selecting or launching the Album clears the explicit track target and starts
  normal Album resume.

Selection history controls live at the right edge of the Media Selection filter
bar and remain visible as the pane is resized.

READ performs a fully evaluated dry run. LIVE WRITE may install selected
artwork. Auto Scan `[SELECTED]` uses only the explicit queue. Auto Scan `[ALL]`
adds every unprocessed/incomplete Album without bypassing protected states.
Auto Scan accepts only policy-eligible automatic decisions; otherwise the
reusable LAUNCH / WAITING / STOP lifecycle pauses for input.

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

The primary GitHub artifact is `splined-windows-x86_64.zip`. Its allowlist is
exactly `splined.exe`, `runtime\splined-core.dll`, and `README-WINDOWS.txt`
under the `SPLINED` directory. A fresh archive contains no `data\`.

Portable update checking is notification-only. It can show release information
and open the official release page, but it cannot download or replace binaries.
The user closes SPLINED and replaces only the three program/documentation files;
portable `data\` and every configured external resource remain outside that
operation.

The optional installed artifact is an x64 MSIX created with upstream Microsoft
WinAppCli. WinAppCli generates package assets, validates the manifest, supplies
debug/loose-layout identity, creates disposable development certificates, and
supports signing and Store-ready unsigned output. CI generates the loose-layout
identity without registering it, so release validation does not depend on a
machine-wide Developer Mode setting. Developers may register that identity with
WinAppCli on machines where Developer Mode is enabled. CI development-signs a
temporary package, installs and activates it through its package identity,
verifies in-process native-core startup and the `.spl` association, and
uninstalls it. Development certificates and private keys are never committed or
published.

The checked-in `SPLINED.appinstaller.template` prepares Windows App Installer
OnLaunch updates for a future public installed channel. It contains deliberate
publisher and HTTPS placeholders and is not published or enabled until a stable
public publisher/signing chain exists. The portable ZIP never depends on MSIX,
Store publication, or public code-signing infrastructure.

## Diagnostics

Run Activity shows the live event stream. Persistent logs and run reports stay
in Config v5 locations. Reports should include the application version, run
mode, Album path, relevant events, and sanitized provider errors; never include
credential contents.
