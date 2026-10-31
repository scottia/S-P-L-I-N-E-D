# Windows SQLite media database

This is the authoritative reference for the native Windows SQLite catalog,
including optional shared-file operation. It is not the WebGUI database guide.
WebGUI requires PostgreSQL for its separate catalog and persistent queue; see
the [WebGUI data guide](browser-workspace-guide.md#data-recovery-and-deployment)
and [Docker deployment guide](../docker/README.md). Windows does not connect to
the live PostgreSQL store.

The native Windows implementation stores its persistent Select Media model in:

```text
<scan.cache_dir>/splined.db
```

For a new portable Windows configuration the default separation is:

```text
_db/splined.db       persistent SQLite
_cache/...           disposable previews and candidates
```

This database is persistent. Candidate downloads and optional samples belong
to the independently configurable temporary run cache; `splined.db` is never
part of normal run-cache cleanup.

The `artist_art_profiles` table stores a manually confirmed MusicBrainz Artist
match plus its saved biography and external profile URL. Artist portraits and
banners remain ordinary image files: normal Artists use their Artist folder,
while Soundtracks and Various Artists use the configured V/A / OST Artist
Images Directory. There is no separate `artist-art.db`.

Windows derives the file from the browsable **SQL Database Directory** field;
its **Temporary Run Cache** is independent and may remain local.
SQLite access, schema validation, refresh, and status updates are owned by the
authoritative Rust backend. The Windows frontend receives a compact snapshot
through the in-process command boundary. It does not link another SQLite
implementation or maintain a separate JSON-derived Select Media authority.

On startup Windows discovers Config v5 from the portable or packaged state
root, resolves relative paths against that root, and derives the database only
as `<scan.cache_dir>\splined.db`. It does not probe legacy default folders or
fall back to a second database. Startup diagnostics report the active state,
Config, and SQLite paths plus loaded Album, Artist, and Folder counts without
including credentials. If Config v5 is missing, invalid, or operationally
incomplete, structured Setup opens before any database or runtime directory is
created.

## Shared Windows database

Two installations use the same physical database by selecting cache
directories that resolve to the same file. No database server or synchronization
layer is involved. Enable shared-file behavior in every process that opens it:

```toml
[scan]
sqlite_shared = true
```

TOML keys are section-scoped: `sqlite_shared` must appear after the `[scan]`
header and before the next table header. Placing the same text elsewhere does
not configure `scan.sqlite_shared`. On Windows, enable
**Shared/Network SQLite** in Setup or Advanced Processing Settings; the GUI
stores the same Config v5 field internally.

The first Windows index publication establishes the shared database's canonical
path namespace, ignored-folder policy, and cover filename. Later Windows clients
can explicitly refresh its Artist, Album, and complete track inventory. Each
client maps saved canonical paths onto its own `library.music_library` and
uses the published ignored-folder and cover-name policy instead of silently
substituting its local settings. The Windows runtime configurations resolve
`scan.cache_dir` to the same physical `splined.db` and enable `sqlite_shared`.

### Before enabling shared mode

Shared SQLite is an advanced choice, not required for a Windows-only library.
Each client must point to the **same physical database**, map the same music
files beneath its own library root, and use a share whose cross-host file locks
have been verified. Do not copy independent databases over one another or
refresh from two clients at once. Back up `splined.db` and the settings archive
before changing the shared path or publisher. A 30-second busy timeout can
wait for a transient SQLite lock; it cannot repair a disconnected share or
replace cross-host coordination.

The extra `.client-lock` is **recommended when every client can acquire it**.
It is not a password or a stale-file marker: the OS lock is held only by a
running process and is released when that process closes. If a client cannot
acquire the extra lock on the chosen share, leave it disabled and avoid
overlapping writers. SQLite's own locks still apply either way. Setup and
Advanced Settings link to this guide before shared mode is saved.

Local/default mode retains WAL and `synchronous=NORMAL`. Shared Windows mode
uses the rollback `DELETE` journal, `synchronous=FULL`, foreign keys, and a
30-second busy timeout for UNC/network storage.

Windows clients can also hold an OS-managed lock on the
`splined.db.client-lock` sidecar by enabling `scan.windows_client_lock` in
Setup or Advanced Processing Settings. This extra lock is off by default. An
abandoned sidecar file does not itself hold a lock; the OS lock exists only
while a client is connected.
SQLite's rollback journal and 30-second busy timeout still apply. The optional
sidecar depends on the shared filesystem honoring cross-host file locks; do not
assume that a timeout substitutes for verified coordination.

The first Windows publisher's configured `ignored_subs` patterns control which folders are excluded.
A leading period alone is not an exclusion: `.38 Special` and
`...Baby One More Time` are valid music-folder names. Explicitly list service
folders such as `.nomifo` or `.animatedartworkdownloader` when they should be
ignored. One representative track supplies an Album's identity tags. The same
explicit refresh also reconciles complete `tracks` rows. It reads full per-track
tags only for new or changed COMP/Fallback files and reuses unchanged metadata.

An existing database establishes its canonical path namespace in
`picker_inventory.signature_json`. The first Windows client that publishes a
new shared database records its configured library root as that namespace.
Later Windows clients treat their configured
`library.music_library` as the local filesystem root, strips the saved canonical
root from database paths, preserves the relative Artist/Album/file location,
and joins that location to the Windows root for I/O. Writes are translated back
before SQL is updated.

Windows normal startup opens an established, usable index read-only. It does
not reapply the schema or request a journal-mode transition. A background
startup check enumerates Album folders and sorted track filenames, checks one
representative file per normal Album, and compares full path/size/mtime
fingerprints only for COMP/Fallback Albums. It does not open tags or change the
database. When Album folders were added, changed, or removed, the GUI reports
each count and offers **Apply Library Changes**. That action updates only the
Album delta in one transaction; untouched Artist, Album, and track rows remain
in place. It reads one representative track for a changed Album identity and
reads per-track tags only for changed COMP/Fallback files. It neither inspects
embedded art nor modifies music files. **Settings → Refresh DB** remains the
deliberate complete index rebuild. A new local or shared index can be published
entirely by Windows; Python is not an index authority.

Raw UNC cache paths are supported and are preferred when mapped drive letters
are not stable across devices or sessions. On Windows, SPLINED internally
adapts a configured path such as `\\server\share\path\to\_cache`. It dynamically
reuses any Windows connection to the same remote prefix, regardless of its
current drive letter; this preserves provider-specific behavior such as an NFS
mount. If none exists, it creates a temporary, non-persistent connection only
while SQLite is open, then removes it. The internally saved Config v5 path
remains UNC-based and never depends on a particular drive letter.

Windows extended-length paths are classified before network adaptation.
`\\\\?\\X:\\...` and `\\\\.\\X:\\...` remain local paths and never invoke the
UNC adapter; `\\\\?\\UNC\\server\\share\\...` is normalized to the equivalent
ordinary UNC path before connection discovery. This distinction is covered by
regression tests because Windows may return extended local paths after
canonicalization even when Config v5 contains a normal drive path.

The centralized path mapper is the only media-path conversion boundary.
Windows maps
the path-bearing Artist/Album snapshot, representative, cover/local-art, and
retired-path values that it reads or writes. Existing canonical track and
compilation-progress/artwork rows are left untouched by workflows that do not
consume them; any Windows workflow that later consumes those rows must pass
their paths through the same mapper. A mount-root difference, independent
ignored-directory rules, and different output filenames are compatible because
the published policy controls the shared inventory. The database schema version must still agree;
a genuine schema conflict fails clearly without quarantining or rebuilding the
shared database.

When the Windows GUI opens existing artwork for a normal Album, it may resolve
an equivalent physical directory spelling (including Unicode normalization or
a recoverable UTF-8/Windows-1252 filename mismatch). It also recognizes the
private-use filename representation exposed by some network shares for a
trailing period. That lookup is an I/O-only compatibility step: the canonical
Album, cover, and local-art paths in SQLite remain unchanged. Eligible fallback
compilations never use this lookup for the current-track preview; their embedded
track art remains the only authority.

## Design

The schema uses persistent logical identities, resumable checkpoints, and
explicit maintenance/audit surfaces:

- logical text keys instead of path identities;
- `cache_entries` and `cache_history` audit surfaces;
- `db_maintenance_state`;
- `picker_inventory`;
- a review queue for ambiguous logical identities;
- `retired_album_paths` for moved or removed locations;
- `ANALYZE` and `PRAGMA optimize` after an index refresh.

SPLINED adds first-class `artists` and `albums` tables containing the values
required by Select Media, Album Status, local cover preview, and selected
Album statistics.

## Identity and paths

Filesystem paths are stored as mutable locations. They are **not** Artist or
Album identity and are not SQL index keys.

Artist identity is resolved in this order:

```text
MusicBrainz Album Artist ID
        ↓ unavailable
normalized ALBUMARTIST tag
```

Album identity is resolved in this order:

```text
MusicBrainz Album / Release ID
        ↓ unavailable
hash of tagged Artist identity + ALBUM + release-group ID + year + compilation
```

Representative tags are read by the native Rust tag reader. Any standards-compliant tag writer
may supply those values; SPLINED does not require or query another
application's database.

The path columns remain necessary to open and process the current Album folder.
If a MusicBrainz-identified Album moves to another directory, an explicit
refresh updates the path on the same logical Album row and records its prior
location in `retired_album_paths`.

The schema intentionally has no index on:

- `artists.primary_path`;
- `albums.path`;
- `albums.representative_file`;
- `albums.cover_path`;
- `retired_album_paths.album_path`.

The primary lookup indexes are Artist key, Album key, MusicBrainz IDs, Artist
membership, and Album Status.

## Authority identities and physical picker folders

The `artists` table stores tagged Album-Artist authority identities. It is not a
one-row-per-directory table. One authority can legitimately occur in several
physical folders, and several authority identities can occur beneath one
collection folder.

`artists.primary_path` is a current observed location for an authority row; it
is not the Artist Picker grouping key and must not be used to count visible
picker folders. Picker ownership is derived independently from each
`albums.path` by taking the first directory beneath the configured library
root:

```text
Album path                                      Artist Picker folder
/music/Christina Aguilera/AGUILERA              /music/Christina Aguilera
/music/[Soundtracks]/A Star Is Born Soundtrack  /music/[Soundtracks]
/music/[Various Artists]/[Various Artists]/...  /music/[Various Artists]
```

This keeps a normal Artist folder visible even when the same tagged authority
also appears on a soundtrack. It also prevents OST and Various Artists Albums
from being duplicated beneath each credited performer. Album Artist and
MusicBrainz Album Artist ID remain identity/artwork-search authority; physical
folder ownership is a presentation and processing boundary.

## First build

When no compatible database exists, Windows **Refresh DB** performs a one-time
index publication:

```text
inventory Artist / Album folders
        ↓
choose one representative audio file per Album
        ↓
read one representative track for Album identity
        ↓
read COMP/Fallback track identities and record all path/size/mtime manifests
        ↓
inspect configured cover.* and materialized statistics
        ↓
publish Artist, Album, and complete track rows in one transaction
        ↓
load Select Media from SQLite
```

The initial publication reads each track once to establish Windows authority.
Incremental reconciliation leaves unchanged rows untouched and reads full tags
only for changed COMP/Fallback files. A Settings **Refresh DB** rebuild still
reuses unchanged track metadata by path, size, and modification time while
republishing the complete model. Normal Artist overview remains Album-oriented
and validates only one representative track; full embedded-art inspection
remains lazy. A partially built model is never published because either
transaction commits only after every affected row is ready. Neither operation
alters music-library files.

## Warm startup

A normal warm launch is read-only and database-only for Select Media:

```text
open splined.db
        ↓
optionally stage one temporary local read snapshot
        ↓
project current history / bypass / timeout facts in memory
        ↓
load compact authority-Artist and Album rows
        ↓
derive physical Artist Picker folders from Album paths
        ↓
render final Album Status and picker colors
```

There is no Artist-by-Artist background filesystem validation, no structural
sentinel pass, and no color change merely because the user opens an Artist.
Artist and Album rows are already resident before the first Select Media frame.
Full right-side Album statistics are read lazily by indexed `album_key` when an
Album is focused instead of loading every statistics column before first paint.

Windows follows the same rule. Normal GUI loading invokes the Rust snapshot
command and reads indexed rows only. **Refresh DB** is the explicit filesystem
reconciliation boundary for both an independent Windows database and an
established shared database.

When `splined.db` is on a different device from the container temporary
directory and has no active WAL, the default `auto` policy copies it
sequentially to a temporary immutable SQLite snapshot for warm hydration. The
persistent database remains untouched. `SPLINED_SQLITE_LOCAL_SNAPSHOT` accepts
`auto`, `always`, or `never`; unsafe/unavailable snapshot conditions fall back
to direct read-only access.

The startup title distinguishes these paths:

```text
BUILDING ALBUM STATUS INDEX  = first build or explicit Refresh
LOADING ALBUM STATUS         = warm read of an existing usable database
```

The only normal color changes during a session are the result of visible
SPLINED actions, such as processing an Album or adding/removing a bypass.

## Explicit Refresh

**Refresh DB** is the Windows external-library reconciliation boundary. Windows
can publish a new local/shared index or refresh an existing one. In a shared
configuration, use one refresh publisher at a time; normal startup reads the
published index rather than rebuilding it.

An explicit refresh:

1. inventories the complete Artist/Album topology;
2. reuses stored tags when the same representative file has unchanged size and
   modification time;
3. reads native tag evidence for new or changed Album representatives and
   new or changed COMP/Fallback track files;
4. identifies rows by tagged Artist/Album keys rather than paths;
5. refreshes `cover.*`, resolution, format, file counts, and WebP information
   for identified Albums; missing-ID Fallback Albums exclude `cover.*` from
   those projections;
6. reconciles the complete `tracks` inventory and removes stale track rows;
7. commits the replacement model in one SQLite transaction;
8. swaps the resident GUI model only after the transaction completes.

The previous stable picker remains visible while Refresh runs. Rows and colors
do not progressively change as individual Artists are reached.

External changes made by taggers, file managers, or another application become
visible after this explicit Refresh. SPLINED's own LIVE WRITE and bypass/status
actions update the affected database rows immediately.

## Fallback track-art caches

Missing-ID Album artwork recovery uses the following persistent surfaces without
opening every track during normal overview loading:

- `tracks` stores the complete Windows-published track inventory and any exact
  local Recording-ID/Artist-ID relationships;
- `recording_release_lookups` and `recording_release_candidates` cache positive,
  bounded MusicBrainz Recording-ID results;
- `compilation_track_artwork` records approved per-track artwork outcomes, the
  unchanged local tag identity, and a content digest, but never stores
  credential values or image bytes. Operator-edited/current-result IDs remain
  session authority and the chosen Release ID is stored separately;
- `compilation_album_progress` stores only Album path, total/completed counts,
  `incomplete`/`complete`, update time, and SPLINED version;
- a `cache_entries` row of type `compilation_resume` stores the canonical path
  of the track active during an interruptible normal Album run. It contains no
  image bytes or MusicBrainz/tag authority.

The Fallback branch first queries the exact `tracks` inventory. Folder-level
`cover.*` is never a candidate or count; release
artwork comes from the current track's embedded image and remote providers.
The Windows warm snapshot projects cached fallback track paths from `tracks`
and joins the track-artwork ledger only to mark completed embedded-art writes in
**● Embedded Art**. This is SQLite-only: it does not walk the music share or
reopen audio tags. For a legacy database not yet refreshed by Windows, the
selected Album can still be hydrated non-recursively until **Refresh DB**
publishes its complete track rows. The compilation runtime reads the
same progress and track-artwork tables when an eligible compilation starts.
Normal discovery continues to
inspect one representative track per Album.
Resume accepts a prior completion when the current local Recording and Artist
IDs still match the ledger row. For compatibility with older rows that stored
the selected session authority, an identity mismatch is accepted and normalized
only when the current embedded front image still matches the ledger's SHA-256;
a replaced or changed file therefore remains unfinished. Separately, the
interruption cursor starts the next normal run at the track that was active when
Stop/error ended the previous run. The cursor advances only as the runtime
reaches each next track and is cleared after a normal pass reaches the end;
completed-track skipping remains authoritative throughout.

A targeted Windows edit passes explicitly checked compilation
tracks to the runtime. Highlighting or clicking a track is preview focus only and never sets
the target environment value, so an ordinary Album launch still reads the
ledger and interruption cursor. Checking the Album clears any explicit track
target. A checked track bypasses the resume skip for that track only, including
when it is already complete, extracts its current embedded front image into the
portable `data\preview-cache`, and sends it through the same review/edit/write
path. This preview-only cache is independent of the configured provider run
cache. Other tracks in the Album are not processed and folder cover files
remain untouched. The
fallback GUI preview uses that embedded cache image only. Selecting the fallback
Album itself shows **No embedded artwork** and does not create a folder-cover
candidate; it never substitutes the Album folder's `cover.*`.

LIVE WRITE creates or refreshes the progress row when compilation work starts, so
an operator exit before the first approval is represented as `0/N incomplete`.
Progress-only events also update the retained in-memory Album status; the next
`library_update` therefore cannot overwrite the blue incomplete state.
Operator MBID edits and text-discovery selections remain session-only and are
not stored in these tables as tag authority. MusicBrainz Artist/Track search
results are cached only in the active run so source-candidate Escape can
return to the same list without another request. A deliberate `M` search
refreshes that per-run list while preserving the configured MusicBrainz delay,
timeout, and attempt budget.

Exact Recording-to-release lookups use the requested/tagged Recording MBID as
the persistent cache parent. If MusicBrainz redirects a merged Recording to a
different canonical MBID, SPLINED uses the canonical result for the active run
but does not insert mismatched child rows beneath the old parent key. This
avoids a foreign-key failure without changing tag authority or rewriting the
database identity.

## Album Status and physical folder aggregates

The database materializes Album facts including:

- `status`;
- `cover_required` and `cover_found`;
- canonical cover path/name/format/resolution;
- completion, bypass, timeout, and selected-source fields;
- local artwork and selected Album statistics.

Album status values are:

```text
unprocessed
incomplete
processed
bypassed
timeout
```

Visible Artist Picker status is aggregated from Album rows sharing the same
physical top-level folder:

```text
unprocessed
partial
complete
contains-bypass
```

Completion history, bypass history, timeout policy, and actual local artwork
remain operational authority. SQLite is the persistent, indexed Select Media
read model for those facts; it is not a replacement provider/ranking engine.
The authority-oriented `artists.status` value may cover Albums located in more
than one physical folder; the Windows GUI therefore recomputes its displayed
folder-level aggregate from the projected Album rows.

`incomplete` is specific to a started but unfinished compilation Album. It does not
claim folder-level `cover.*` authority. The runtime updates only the progress
status after each embedded-track write; it does not rescan or materialize
folder cover statistics for that event.

## Selected Album artwork and statistics

The right-side Album panels read their initial data from SQLite, including:

- tagged Album, Artist, year, and track count;
- current `cover.*` path and resolution;
- JPEG/PNG/WebP/other counts;
- root, cover, and other filenames;
- WebP size, resolution, and conversion state.

This removes the ordinary delay previously caused by reparsing tags and image
statistics every time focus moved between already-indexed Albums. Warm startup
loads full statistics only for an already-selected Album; later focus changes
read the indexed row on demand. When LIVE WRITE changes an Album, SPLINED
persists the already-validated final path, format, and dimensions with its
authority/folder status. It updates only that Album and its owning Artist
aggregate; it does not re-enumerate the Album directory or recompute every
Artist after the final artwork write. The rollback journal, `FULL`
synchronization, audit row, and commit-before-`album_completed` ordering remain
unchanged.

## Runtime state

The former JSON status and history surfaces are retired. SPLINED neither reads
nor writes them. Completion, source selection, bypass, timeout, and incomplete
compilation progress are committed to SQLite. The Windows Rust application also
reads the published SQLite inventory instead of maintaining a second GUI
filesystem inventory.

The durable runtime fields and audit surfaces are:

- `albums.status`: `unprocessed`, `incomplete`, `processed`, `bypassed`,
  or `timeout`;
- `albums.processed_at`: most recent completed Write operation;
- `albums.bypassed`: explicit persistent bypass;
- `albums.timeout_until`: time when a timed-out Album becomes eligible;
- `albums.selected_source`: source selected for the installed artwork;
- `cache_entries`: auxiliary state such as source-ranking counts and scan
  fingerprints;
- `cache_history`: append-only runtime audit records.

An explicit bypass remains until cleared. A timeout is temporary and applies
only until `timeout_until`. An unfinished manual compilation stays
`incomplete` so it can resume without appearing successfully processed.

`[history].enabled` controls whether completed outcomes affect later
selection. `[history].retention_days = 0` retains them indefinitely; a positive
value permits older completion state to be pruned or reprocessed. Diagnostic
log retention is independent: deleting logs never changes Album state.

`cover_found`, `cover_path`, and `local_art_json` describe artwork present
in the Album folder. They do not create Processed authority. Only an explicit
SPLINED completion recorded through runtime provenance marks an Album
Processed, so operator-added or third-party `cover.*` remains Unprocessed.

## Backup and deletion

Use Windows Backup / Restore for native SQLite snapshots. Database exports use
SQLite's online backup API so committed WAL transactions are included, and
restores validate the snapshot before replacement. WebGUI backups instead
require a consistent PostgreSQL dump and its persistent volumes; terminal
Python `.spl` commands are retired.

The WAL/SHM sidecars may exist while SPLINED is running. Stop SPLINED before
copying the database if a consistent filesystem-level backup is required.

On Windows, **File > Backup > Export Backup...** can include portable settings,
interface state, credentials, `splined.db`, and diagnostics independently. Stop
other writers before exporting an actively shared database.

Candidate files, review samples, temporary run-cache images, and diagnostic
logs are disposable and do not need to be included in a state backup.

Deleting `splined.db` is safe for music files, but also removes the durable
index and runtime state. The next Windows **Refresh DB** performs the complete
one-time native index build again.

## Inspection

Useful read-only checks on a copied Windows database:

```powershell
sqlite3 N:\path\to\_db\splined.db '.tables'
sqlite3 N:\path\to\_db\splined.db 'PRAGMA user_version;'
sqlite3 N:\path\to\_db\splined.db `
  'SELECT status, COUNT(*) FROM albums GROUP BY status ORDER BY status;'
sqlite3 N:\path\to\_db\splined.db `
  'SELECT status, COUNT(*) FROM artists GROUP BY status ORDER BY status;'
```

SPLINED does not require the `sqlite3` CLI; these commands are optional
diagnostics when it is installed.

In debug logs, keep the count domains distinct:

```text
authority_artists = tagged identities in artists
picker_folders / artist_folders = unique physical top-level folders represented by albums
albums            = indexed physical Album paths
```

A difference between `authority_artists` and the physical-folder count is
expected. Warm-load records use `picker_folders`; index validation records use
`artist_folders` for the same physical count.
Useful lifecycle records include `splined.db.index_check`,
`splined.db.warm_load.*`, `splined.db.promotion_done`, and
`splined.db.session_ready`.
