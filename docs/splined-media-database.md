# SPLINED media database

The Python/Docker Ratatui and Windows implementations store their persistent
Select Media read model in:

```text
<scan.cache_dir>/splined.db
```

With the standard Docker configuration:

```text
/_cache/splined.db
```

If the host bind mount is:

```yaml
- /mnt/splined/_cache:/_cache:rw
```

then the host file is:

```text
/mnt/splined/_cache/splined.db
```

This database is persistent. Candidate downloads and optional samples belong
to the independently configurable temporary run cache; `splined.db` is never
part of normal run-cache cleanup.

Windows derives the file from the browsable **SQL Database Directory** field;
its **Temporary Run Cache** is independent and may remain local.
SQLite access, schema validation, refresh, and status updates are owned by the
Rust core; the embedded C# GUI receives a compact JSON snapshot from that core.
It does not link another SQLite library or maintain a separate JSON-derived
Select Media authority.

## Shared Windows/Linux database

Two installations use the same physical database by selecting cache
directories that resolve to the same file. No database server or synchronization
layer is involved. Enable shared-file behavior in every process that opens it:

```toml
[scan]
sqlite_shared = true
```

TOML keys are section-scoped: `sqlite_shared` must appear after the `[scan]`
header and before the next table header. Placing the same text elsewhere in the
Python/Docker file does not configure `scan.sqlite_shared`. On Windows, enable
**Shared SQLite database (network / multi-OS)** in Settings; the GUI stores the
same Config v5 field internally.

The Python/Docker installation is the inventory authority for a shared
database. Its `library.ignored_subs`, Album discovery, and representative-track
policy determine which rows are published. Windows consumes that inventory and
maps the saved canonical paths onto its own `library.music_library`; Windows
does not compare or apply its local `ignored_subs` or output filename while
loading the shared inventory. The two runtime configurations therefore remain
independent apart from intentionally resolving `scan.cache_dir` to the same physical
`splined.db` and enabling `sqlite_shared`.

Local/default mode retains WAL and `synchronous=NORMAL`. Shared mode uses the
rollback `DELETE` journal, `synchronous=FULL`, foreign keys, and a 30-second
busy timeout. This prevents Linux from repeatedly forcing WAL while Windows
opens the same file through a UNC/network share.

An existing database establishes its canonical path namespace in
`picker_inventory.signature_json`. For a cross-mounted shared database, build
the initial index from the Python/Docker view so that its canonical namespace
(normally `/music`) is recorded before Windows opens it. A brand-new independent
Windows database naturally records the configured Windows library root.
Windows treats its configured
`library.music_library` as the local filesystem root, strips the saved canonical
root from database paths, preserves the relative Artist/Album/file location,
and joins that location to the Windows root for I/O. Writes are translated back
before SQL is updated. A Python-created `/music/...` value therefore remains
`/music/...` in SQLite while Windows opens the corresponding UNC path.

Windows normal startup opens an established, usable index read-only. It does
not reapply the schema or request a journal-mode transition. In shared mode,
**Refresh Library Index** reloads the inventory already published by Python; it
does not rebuild that inventory with Windows scan settings. Reindex through
Python after external library/tag changes, then reload in Windows. Windows
LIVE WRITE and status actions still update their applicable shared SQL rows.

Raw UNC cache paths are supported and are preferred when mapped drive letters
are not stable across devices or sessions. On Windows, SPLINED internally
adapts a configured path such as `\\server\share\path\to\_cache`. It dynamically
reuses any Windows connection to the same remote prefix, regardless of its
current drive letter; this preserves provider-specific behavior such as an NFS
mount. If none exists, it creates a temporary, non-persistent connection only
while SQLite is open, then removes it. The internally saved Config v5 path
remains UNC-based and never depends on a particular drive letter.

The centralized path mapper is the only media-path conversion boundary.
Windows maps
the path-bearing Artist/Album snapshot, representative, cover/local-art, and
retired-path values that it reads or writes. Existing canonical track and
compilation-progress/artwork rows are left untouched by workflows that do not
consume them; any Windows workflow that later consumes those rows must pass
their paths through the same mapper. A mount-root difference, independent
ignored-directory rules, and different output filenames are compatible because
Python owns the shared inventory. The database schema version must still agree;
a genuine schema conflict fails clearly without quarantining or rebuilding the
shared database.

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

Representative tags are read with Mutagen. Any standards-compliant tag writer
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

When no compatible database exists, the TUI performs a one-time index build:

```text
inventory Artist / Album folders
        ↓
choose one representative audio file per Album
        ↓
read Album identity tags with Mutagen
        ↓
inspect configured cover.* and materialized statistics
        ↓
save resumable per-Album SQLite checkpoints
        ↓
validate and promote the completed active snapshot in batches
        ↓
publish picker_inventory last, then consolidate the WAL
        ↓
load Select Media from SQLite
```

SPLINED does not parse every track merely to populate the picker. Full
track-level validation remains part of the actual READ or LIVE WRITE processing
pipeline.

This is a performance invariant: normal build and Refresh remain Album-oriented
and retain one representative audio path per Album. They must not become a
whole-library per-track tag-reading pass.

If the first build is interrupted, matching completed checkpoints are reused on
the next launch. A partially promoted snapshot is never advertised as ready:
the `picker_inventory` marker is written only after all Artist/Album rows have
been promoted and validated. An explicit Refresh of an already usable snapshot
continues to replace that snapshot atomically.

A corrupt or incompatible database is renamed with a timestamp and rebuilt.
The build does not alter music-library files.

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
command and reads indexed rows only. For an independent Windows database,
**Refresh Library Index** is the explicit filesystem reconciliation boundary.
For a shared database it reloads Python's published inventory instead.

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

`R` / Refresh is the external-library reconciliation boundary for Python and
for an independent Windows database. With `sqlite_shared = true`, Python owns
the rebuild and Windows Refresh reloads that published result.

An explicit refresh:

1. inventories the complete Artist/Album topology;
2. reuses stored tags when the same representative file has unchanged size and
   modification time;
3. reads Mutagen tags for new or changed Albums;
4. identifies rows by tagged Artist/Album keys rather than paths;
5. refreshes `cover.*`, resolution, format, file counts, and WebP information;
6. commits the replacement model in one SQLite transaction;
7. swaps the resident TUI model only after the transaction completes.

The previous stable picker remains visible while Refresh runs. Rows and colors
do not progressively change as individual Artists are reached.

External changes made by taggers, file managers, or another application become
visible after this explicit Refresh. SPLINED's own LIVE WRITE and bypass/status
actions update the affected database rows immediately.

## Compilation track-art caches

Compilation artwork recovery adds four persistent surfaces without changing
normal index cost:

- `tracks` stores exact local Recording-ID/Artist-ID relationships discovered
  lazily during the compilation branch;
- `recording_release_lookups` and `recording_release_candidates` cache positive,
  bounded MusicBrainz Recording-ID results;
- `compilation_track_artwork` records approved per-track artwork outcomes and a
  content digest, but never stores credential values or image bytes;
- `compilation_album_progress` stores only Album path, total/completed counts,
  `incomplete`/`complete`, update time, and SPLINED version.

The compilation branch first queries the exact `tracks` cache. On a miss it uses the
already-indexed Album Artist ID to restrict local inspection to that Artist's
Albums, stopping at the first exact Recording-ID/Artist-ID match. Only a local
miss can proceed to MusicBrainz. Stale track rows cannot become artwork
authority: file size and modification time are checked before reuse.
The Windows warm snapshot projects cached compilation track paths from `tracks`
and joins the track-artwork ledger only to mark completed embedded-art writes in
Show Tracks. This is SQLite-only: it does not walk the music share or reopen
audio tags. For an older compilation whose full filename list was not cached,
Windows fills the list only when that Album is expanded by enumerating that one
folder non-recursively; no tag data is read. The compilation runtime reads the
same progress and track-artwork tables when an eligible compilation starts.
Normal discovery continues to
inspect one representative track per Album.
Resume accepts a prior completion only when the current local Recording and
Artist IDs still match the ledger row.

A targeted Windows edit passes one selected compilation track to the runtime.
That explicit target bypasses the resume skip for that track only, extracts its
current embedded front image into the run cache, and sends it through the same
review/edit/write path. Other tracks in the Album are not processed and folder
cover files remain untouched.

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
than one physical folder; the TUI therefore recomputes its displayed
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
compilation progress are committed to SQLite. The Windows GUI also reads the
published SQLite inventory instead of performing a separate C# filesystem
inventory.

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

For Python/Docker installations, back up:

```text
/config
/credentials
/_cache/splined.db
```

The WAL/SHM sidecars may exist while SPLINED is running. Stop SPLINED before
copying the database if a consistent filesystem-level backup is required.

On Windows, **File > Backup > Export Backup...** can include internal settings,
interface state, credentials, `splined.db`, and diagnostics independently. Stop
other writers before exporting an actively shared database.

Candidate files, review samples, temporary run-cache images, and diagnostic
logs are disposable and do not need to be included in a state backup.

Deleting `splined.db` is safe for music files, but also removes the durable
index and runtime state. The next Python interactive launch performs the
complete one-time Mutagen/index build again.

## Inspection

Useful read-only checks inside the container:

```bash
sqlite3 /_cache/splined.db '.tables'
sqlite3 /_cache/splined.db 'PRAGMA user_version;'
sqlite3 /_cache/splined.db \
  'SELECT status, COUNT(*) FROM albums GROUP BY status ORDER BY status;'
sqlite3 /_cache/splined.db \
  'SELECT status, COUNT(*) FROM artists GROUP BY status ORDER BY status;'
```

The runtime image does not require the `sqlite3` CLI; these commands are for
hosts or diagnostic containers where it is installed.

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
