# Windows portable guide

## Architecture and package

The Windows application uses upstream Tauri v2 and contains its frontend and
authoritative Rust backend in one permanent executable:

```text
SPLINED\
  splined.exe
  README-WINDOWS.txt
```

No SPLINED processing child is launched. Commands start Rust operations in the
application process; structured events return progress, decisions, candidates,
MusicBrainz matches, logs, and completion state to the frontend. Cancellation
uses the same in-process boundary.

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

The executable directory is the portable root. Relative paths resolve from it,
not process CWD. Moving or copying the complete directory carries settings.
Absolute and UNC paths remain exact.

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
Config/UI authority. SPLINED registers the portable executable as the backup
opener, and the Tauri Windows package declares the same association. Opening an
existing `.spl` file launches the selective Restore surface with that path
pre-filled.

For the same backup and category selection, two clean portable directories
produce equivalent Config v5 and UI state. Selected categories overwrite their
destinations. Unselected categories stay unchanged. Registry contents,
previous roots, and unrelated folders do not affect restore.

## Updates and trust

Production automatic updates require all of the following:

- signed update metadata;
- a cryptographically signed updater package;
- a stable publisher certificate on the application and update installer;
- a trusted timestamp on Windows signatures;
- explicit user approval.

The standard update installer receives the current portable root and replaces
application code there. Portable `data\` and all configured SQLite, credential,
history, cache, and log paths are outside the update payload. Unsigned developer
builds cannot install public updates.

The release pipeline emits a separate update installer, update archive,
archive signature, and `latest.json`. A schema 2 notification manifest remains
for older builds but intentionally lacks the new trust contract and therefore
cannot authorize installation.

## Diagnostics

Run Activity shows the live event stream. Persistent logs and run reports stay
in Config v5 locations. Reports should include the application version, run
mode, Album path, relevant events, and sanitized provider errors; never include
credential contents.
