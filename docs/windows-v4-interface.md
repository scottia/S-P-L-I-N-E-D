# Windows v4 Guide

This guide describes the native Windows application: the Windows Forms shell,
the embedded Rust processing core, Config v5, and the SQLite media database.
The Windows application currently identifies itself as **S:P:L:I:N:E:D
v3.0.0 Stable**. “Windows v4” names the interface/source generation; it is not
the application version or the Config v5 schema version.

## Portable application model

The Windows release is distributed as one `splined.exe`. The application can
be extracted to any writable final folder and that folder can later be moved or
replaced without moving the media library or selected persistent data.

| Data | Authority and location | Lifecycle |
| --- | --- | --- |
| Program | `splined.exe` in the selected portable folder | Replaced by an update |
| Config v5 | Current user's internal Windows application settings | Persists until reset or restored |
| Interface state | Current user's internal Windows application settings | Persists theme, layout, filters, and selection |
| SQL database | User-selected database directory; first-run default is `%LOCALAPPDATA%\SPLINED\cache` | Persistent; contains `splined.db` |
| Temporary run cache | User-selected disposable directory; first-run default is `%LOCALAPPDATA%\SPLINED\run-cache` | Candidate and derived images are removed after the run |
| Logs | User-selected log directory; first-run default is `%LOCALAPPDATA%\SPLINED\logs` | Diagnostic and disposable |
| Credentials | User-selected credential directory; first-run default is `%LOCALAPPDATA%\SPLINED\credentials` | Persistent and sensitive |
| GUI-to-core configuration | Private child-process environment | Memory-only; disappears with the child process |
| Embedded GUI shell | `%LOCALAPPDATA%\SPLINED\runtime` | Fingerprinted private cache reused by the same build; stale older shells are removed on start |

`%PROGRAMDATA%` is not used for per-user defaults because it is shared by all
users on the computer. `%LOCALAPPDATA%` is the correct Windows per-user data
location. The database, temporary-cache, log, and credential fields remain editable, including to
UNC or mapped-drive locations. Existing saved paths are never moved
automatically.

The portable folder does not require or create `config.toml`, `ui.toml`,
`config.location`, `config`, `credentials`, `_cache`, `_logs`, `docker_builds`,
or a runtime configuration file. Python, Docker, Linux, and macOS still use a
file-backed Config v5 document.

## First run

1. Extract the complete Windows archive into its final folder.
2. Run `splined.exe`.
3. Choose the required music-library, SQL database, temporary run cache, log,
   and credential directories.
4. Review Read/Write, artwork, range, source, history, and shared-SQLite
   settings under **Advanced**.
5. Select **Save and Continue**.
6. Configure enabled provider credentials through **File > Credentials...**.
7. Begin with **Filtered Scan [READ]** and a small Album selection.

The first-run fields are mandatory. SPLINED validates them before saving and
creates the selected database, temporary-cache, log, and credential directories only after
**Save and Continue**. Merely extracting or opening Setup does not create those
directories.

Settings are later available from **File > Settings...**. **Validate Saved
Settings** checks the internal Config v5 record without exporting it. The GUI
passes a validated in-memory copy to each Rust snapshot or Album process; the
display label `Windows internal settings` is never treated as a filename.

## UNC, NAS, and mapped paths

The Windows library, database, run-cache, log, and credential fields accept normal absolute
paths, mapped drives, and UNC paths. A UNC path is preferred when a drive letter
may vary between devices or sessions.

For a database shared with Python/Docker:

- point both runtimes at the same physical `splined.db`;
- enable `sqlite_shared` in both runtimes;
- let Python/Docker own library inventory and ignored-folder policy;
- use **File > Refresh Library Index** in Windows after the Python index is
  rebuilt;
- keep rollback journal mode, FULL synchronization, and the configured busy
  timeout—do not weaken durability to hide network latency.

Windows maps the canonical Linux library root stored in the shared database to
the configured Windows library root for file I/O. Unicode normalization and
legacy-decoded SMB names are resolved against physical directory entries; the
user should not rename an Album merely because its indexed and visible Windows
forms differ.

See [SPLINED media database](splined-media-database.md) for ownership,
journaling, schema, and path-mapping details.

## Main window

The compact top bar contains the multicolor `S:P:L:I:N:E:D` wordmark and four
menus:

- **File** — Settings, Credentials, Backup import/export, Refresh Library
  Index, and Exit;
- **View** — System/Dark/Light appearance, panel-layout presets, and the
  persisted **Show Artwork** switch;
- **Status** — configured providers and credential state;
- **Help** — documentation, update check, and About.

Major panels use the spectrum border colors. Light appearance uses a warm
cream canvas so artwork, state colors, and the watermark retain contrast.
**View > Panel Layout** offers Balanced, Wider Select Media, Wider Decisions,
and Stacked presets. Panel dividers remain draggable; a custom layout persists
for the current user. Media Library Selection uses a scroll canvas contained
inside its spectrum frame, while Scan Activity and Decisions and Artwork
Candidates and Preview retain independent scroll canvases. This prevents the
native scrollbar from painting across the Media Selection frame in Stacked
layout and keeps controls reachable when a panel is reduced.

Windows uses the same red/green state language throughout the picker, candidate
cards, Settings, credentials, and Backup surfaces. A round indicator has a red
outline when off and becomes solid green when on; no checkbox/checkmark glyph
is used. Folder-tree selection uses the same round red/green states.

### Media Library Selection

Select Media contains:

- Artist and Album text filters;
- Select `[ALL]`, `[NONE]`, and `[FILTERED]` scope;
- Auto Scan `[ALL]` or `[SELECTED]`;
- exactly one launch mode: Filtered Scan `[READ]` or `[LIVE WRITE]`;
- Folder Status filters and counts;
- the Artist/Album tree.

The only control that starts or stops processing is the primary **LAUNCH**
button beneath Artwork Candidates and Preview. There is no second Launch button
inside Select Media.

Auto Scan is opt-in. With neither Auto Scan choice active, selected Albums form
an operator-reviewed queue. **Auto Scan `[SELECTED]`** processes only explicitly
selected Albums, while **Auto Scan `[ALL]`** processes every Unprocessed Album
plus any Album selected explicitly. Read versus Live Write remains an
independent, required choice; changing scan scope never changes mutation mode.

The unattended path accepts only a candidate classified `Ideal` and accepted by
its active source policy. If no such candidate exists, Windows pauses for
operator review. `Ideal` verifies configured resolution, geometry, and source
policy—not the picture's semantic accuracy. It cannot detect merchandise,
inserts, disc cases, or unrelated photographs returned by a provider.

Select `[ALL]` only changes the checked Album set. It never enables Auto Scan.

Ctrl+Click supports independent Album selections. Selecting multiple Artists
selects their normally eligible Albums while leaving orange Processed Albums
manual. Bulk selection matches Python:

- **Select `[ALL]`** replaces the current selection with only the active
  Artist's Unprocessed Albums;
- **Select `[NONE]`** clears selection and temporary bypass overrides;
- **Select `[FILTERED]`** requires Artist or Album filter text, replaces the
  current selection, and includes text-matching Unprocessed and Processed
  Albums. Folder Status visibility does not redefine that text-filter scope.

### Album status

SQLite—not the presence of an arbitrary `cover.*` file—owns Album status.

| Album color | Meaning |
| --- | --- |
| White | Unprocessed |
| Orange | Processed by SPLINED; manually reprocessable |
| Red | Bypassed; removal must be confirmed before processing |
| Purple | Timeout active |
| Blue | Started but unfinished compilation track-art work |

Artist colors aggregate the Album states: green means complete, purple means
partial, and blue means the Artist contains a bypass. See [Select Media and
status colors](media-filter-status-colors.md) for the full state model.

## Read and Live Write

**Filtered Scan [READ]** runs authority, provider, image, ranking, and preview
logic without installing folder artwork, replacing embedded artwork, or
committing completion state.

**Filtered Scan [LIVE WRITE]** can alter Album files. For an ordinary Album it
installs the approved folder artwork according to Config v5. For an eligible
curated compilation it replaces only the approved track's embedded front image.

The Launch queue is a snapshot of selected Albums. A completed or
operator-stopped Album is consumed from that queue; a failed Album and Albums
not yet started remain selected. After the batch, LAUNCH becomes available for
another independent selection.

## Artwork candidates and preview

**Scan Activity and Decisions** is divided into Album information on the left
and an Artwork surface on the right. Selecting any Album—including an orange
Processed Album—shows the SQLite-indexed Artist, Album, year, track count,
status, path, root-file count, current `cover.*` filename, and recorded
resolution. The image is loaded from the indexed local cover path; selecting an
Album does not rerun providers or rescan the Album directory.

The Album information follows the Album currently being processed, then returns
to the Album focused manually in the tree. It clears when neither context has
an Album and uses that Album's status color. Both Album information and Artwork
use the same spectrum frame as the other major work surfaces.

The horizontal divider above Artwork Candidates also controls the Artwork
surface geometry. Moving it up reduces the Artwork column with the available
height; moving it down expands the column only while a square Artwork panel can
coexist with the minimum Album/activity width. The divider is constrained at
that point instead of stretching the Artwork panel.

**View > Show Artwork** hides or restores the right-side Artwork surface and is
saved as Windows interface state. When shown, **Enable Hover** routes candidate
image and candidate-URL hover into that surface. Leaving the candidate restores
the selected Album cover. Clicking a candidate URL also places its cached image
in the Artwork surface before the authority URL opens. When Show Artwork is
off, Enable Hover retains the floating resizable preview behavior.

MusicBrainz review is an intentional exception to the ordinary hover switch.
While MusicBrainz Matches is active, it replaces the left Scan Activity surface
and temporarily shows the existing Artwork surface at right even if **Show
Artwork** was off. Hovering or selecting an artwork `[URL]` previews the
release-group front image there without requiring **Enable Hover**. Leaving the
URL restores the selected Album cover; leaving MusicBrainz Matches restores
Scan Activity and the user's saved Show Artwork preference.

The processing panel reports local evidence, authority resolution, provider
timings, candidate policy, strict-content evidence, and errors. Artwork
Candidates and Preview displays every manually reviewable strict candidate.
An unverified strict candidate is marked **Manual only**: its image and URL stay
available, but it cannot become Recommended or be accepted by Auto Scan.

- **Use Selected** approves one active candidate.
- **Compare** compares multiple active candidates.
- **Skip Album** leaves the current Album unchanged and applies the selected
  bypass behavior.
- **Enable Hover** toggles automatic candidate preview and persists as
  interface state.
- Provider and candidate URLs open the corresponding artwork or authority page
  for inspection before approval.

Candidate order remains deterministic. Local evidence is displayed first,
the recommended result follows, and remaining results are ordered by descending
pixel resolution with their stable candidate index as the final tie-breaker.
Only candidate cards use the subtle glass treatment: local is light purple,
recommended is green, and other results are clear. Thumbnails remain square
and grow or shrink with the candidate pane. The shared Artwork preview,
MusicBrainz Matches, and Compare surfaces do not inherit candidate glass.
Concurrent provider work may reduce
waiting time, but configured source order, source policy, Range Type, geometry,
distance, and tie-breaking still determine the displayed ranking.

## MusicBrainz Matches

Every Album candidate review can open **MusicBrainz Matches...**, including an
Album that already has a valid Album/Release MBID. Matches are integrated into
the main decision workspace rather than opened as a separate modal window, so
the shared Artwork panel remains available throughout authority review.
Matches are grouped by newest-to-oldest decade and by populated official
release type: Album, Single, EP, Soundtrack, Compilation, and any additional
type returned by MusicBrainz.

A fixed **CURRENT ALBUM · Artist • Album** row remains immediately above the
scrollable result list. It identifies the Album being refined even when a
representative track or a curated-compilation track supplied the Recording
search terms.

Rows show:

- Artist;
- country;
- release date;
- release type;
- release title;
- known inspected artwork resolution;
- an artwork-preview `[URL]`, plus **Open MB Page** for the exact MusicBrainz
  release page.

The artwork `[URL]` uses the Cover Art Archive release-group front endpoint,
falling back to the exact-release front endpoint. Hover and row selection load
that image asynchronously into the existing Artwork surface and record its
actual dimensions beneath the image. The match-list Resolution column remains
the exact-Release-MBID evidence from inspected configured source results, not
the release-group preview size. Preview bytes are bounded and cached for the
active Windows session; previewing does not rerun provider discovery or alter
source ranking.

Selecting a row runs the ordinary configured artwork providers for that
release. **Back to MB Matches** returns to the same cached list. The current
release is green and previously inspected releases are blue. Re-selecting an
inspected row restores its source candidates, resolution evidence, diagnostics,
and deterministic ordering without repeating provider discovery or image
downloads. **Return to Source Results** restores the original Album results.

The Matches workspace also has session-only Artist, Release, and Recording MBID
fields. **Apply IDs** requires canonical UUIDs and lets the Rust core validate
available Recording/Artist and Recording/Release relationships before
rebuilding the list. These fields choose search authority for the current
session; SPLINED does not write edited MBIDs into the media tags.

For iTunes, exact official Apple Music/iTunes relationships from MusicBrainz
are tried before ordinary Artist/Album searches. Current no-slug Album URLs and
older slugged `/id...` URLs are both recognized.

## Curated compilations

An Album enters compilation track-art handling only when its representative
track has no Album/Release MBID and the Album is tagged `compilation=1`. The
yellow warning remains explicit. This path never creates, changes, or removes
folder-level `cover.*` artwork.

For each track SPLINED:

1. checks the exact Recording-ID/Artist-ID SQLite cache;
2. lazily checks indexed Albums belonging to the matching Artist identity;
3. uses bounded MusicBrainz work only after a local miss;
4. applies the automatic order Album, Soundtrack, then Compilation;
5. lets the operator inspect broader Artist/Track MusicBrainz results;
6. runs the ordinary configured artwork providers and policies;
7. replaces only the approved embedded front image in Live Write;
8. commits the track ledger and Album progress immediately.

Tracks without authoritative evidence remain unchanged and appear unresolved.
Timeouts and upstream errors likewise leave the existing embedded image alone.
Incomplete Albums remain blue and resumable; already completed track decisions
are skipped on the next run.

## Settings and credentials

Windows displays Config v5 in native controls; it does not require the user to
edit TOML. Provider credentials remain separate JSON files in the configured
credential directory. Config v5 stores only that directory—not tokens,
passwords, authorization headers, client secrets, or provider filenames.

Each provider in **Sources & Matching** exposes **Strict Override**. When on,
Windows compares the decoded complete image with exact-release CAA Front art,
a validated local cover, or independent front-art consensus. Strict Override
uses the global range and supersedes Source Override while retaining its saved
custom values. Candidates without strict evidence remain visible and
previewable for explicit operator selection. Amazon defaults to strict mode.

Credential writes are atomic and Windows applies a current-user/System ACL
where the selected filesystem supports it. A NAS may not support Windows ACLs;
SPLINED reports that limitation without exposing the credential value.

MusicBrainz timing and retry behavior comes from the saved credential options,
including `min_delay`, `recording_timeout`, and `retry_max`. No fallback value
is silently hard-coded when a configured value exists.

See [Credentials and provider setup](credentials-providers.md) and
[MusicBrainz OAuth](musicbrainz-oauth.md).

## Backup and restore

**File > Backup** provides separate Import and Export commands. A `.spl` backup
may include any combination of:

- internal Config v5 settings;
- interface state and panel layout;
- credential JSON files;
- `splined.db`;
- diagnostic metadata.

The sections are independent; a backup is not forced to include secrets or the
database. Exports include integrity protection. Optional password protection
uses PBKDF2-derived encryption and authentication.

Opening a registered `.spl` file launches SPLINED directly into the selective
restore dialog. No section is restored until the operator selects it and
confirms. A backup created from an active shared database must still represent
a consistent database copy; stop writers or use the database owner's backup
procedure first.

## Diagnostics

Debug output is written to:

```text
<configured log directory>\run\splined-<level>-<timestamp>-p<pid>.log
```

Windows keeps one current GUI session: prior `splined-*.log` files in that
`run` directory are removed at the next start. Logs are diagnostic only and do
not own Album status, history, bypass, timeout, or compilation progress.

Compact records include snapshot timing, per-provider elapsed time,
candidate/reference counts, post-cover persistence timing, and error class.
Credentials, tokens, passwords, authorization headers, and private values are
redacted and must never be logged.

## Updates

**Help > Check for Update...** uses the channel compiled into the application:

- Stable accepts only an official non-prerelease release with the matching
  Windows manifest and setup asset.
- Dev accepts only the rolling `windows-dev` prerelease and exact dev manifest.

The updater verifies channel, commit, byte count, SHA-256, and asset URL before
replacement. It refuses installation during an active Album run, preserves a
rollback copy during replacement, restarts the verified executable, and leaves
internal settings plus the configured database, temporary cache, logs, and credentials
unchanged. The rollback executable is removed after the old process unlocks it;
the restarted application also prunes a stale `.splined-backup-*` as a safety
net. It replaces the whole executable; it is not a source-code patcher.

Ordinary pushes do not publish the rolling dev build. The corresponding GitHub
workflow is manually dispatched after the desired commit is ready.

## Troubleshooting

### `Windows internal settings` reported as a missing file

Current builds pass internal Config v5 in memory. They do not send the display
label through `--config-path`. Update the Windows executable if an older build
logs `Unable to read SPLINED configuration Windows internal settings`.

### `SPLINED returned an incompatible SQLite media snapshot`

Inspect the immediately preceding `media_snapshot.exit` record. Current builds
return a nonzero core exit and the actual configuration/database error instead
of treating empty output as a successful snapshot.

### `database is locked` or `attempt to write a readonly database`

Confirm that the configured SQL Database Directory and `splined.db` are writable by
the current Windows user and that every runtime opening the same physical
database has `sqlite_shared=true`. Do not place a shared database in WAL mode.
Close obsolete test executables before retrying.

### No Albums appear in Select Media

Validate Settings, confirm the cache points to the intended `splined.db`, and
check the snapshot log. For shared mode, rebuild inventory through Python and
then use **File > Refresh Library Index** in Windows. Windows does not apply a
second ignored-folder policy to the Python-owned shared inventory.

### An accented or legacy-decoded Album directory is not found

Use the real Windows library root in Settings and do not rewrite the SQLite
canonical root. Current builds compare normalized and legacy-decoded directory
forms before reporting the physical path missing. Attach the current run log if
translation still fails.

### A provider returns zero candidates or an upstream error

Provider diagnostics distinguish success, zero results, timeout, and error.
One provider failure does not change another provider's ranking or make the
Album Processed. Verify the candidate URL independently and retain the run log
without adding credential files.

## Invariants

- SQLite is Album/status authority; an arbitrary existing `cover.*` does not
  become Processed authority.
- Shared SQLite retains rollback journal, FULL synchronization, and bounded
  busy handling.
- Source order, source policy, ranking, and deterministic tie-breaking do not
  change because of interface navigation or concurrent provider discovery.
- Read mode does not mutate artwork or durable completion state.
- A curated compilation never silently falls through to folder artwork.
- Config v5 and interface state are internal on Windows; credentials remain
  separate and secrets never enter logs.
- `fixtures/cross-runtime-album-policy.json` is consumed by Rust and Python
  policy tests to keep eligibility and output targets aligned.
