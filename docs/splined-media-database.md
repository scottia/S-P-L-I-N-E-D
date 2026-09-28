# SPLINED media database

The Python/Docker Ratatui implementation stores its persistent Select Media
read model in:

```text
<scan.cache_dir>/splined.db
```

With the standard Docker configuration:

```text
/_cache/splined.db
```

If the host bind mount is:

```yaml
- /mnt/psy_data/downloads/0_backups/splined/_cache:/_cache:rw
```

then the host file is:

```text
/mnt/psy_data/downloads/0_backups/splined/_cache/splined.db
```

This database is persistent. Candidate downloads and samples beside it remain
disposable, but `splined.db` must not be removed by normal run-cache cleanup.

## Design origin

The schema follows the attached Navtagger database conventions where they fit
SPLINED:

- logical text keys instead of path identities;
- `cache_entries` and `cache_history` audit surfaces;
- `db_maintenance_state`;
- `picker_inventory`;
- a review queue for ambiguous logical identities;
- `retired_album_paths` for moved or removed locations;
- `ANALYZE` and `PRAGMA optimize` after an index refresh.

SPLINED adds first-class `artists` and `albums` tables containing the values
required by Select Media, Folder Status, local cover preview, and selected
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

Representative tags are read with Mutagen. Picard, Beets, Navtagger, or any
other standards-compliant tag writer may supply those values; SPLINED does not
require or query a Navidrome or Beets database.

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
membership, and Folder Status.

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
insert one transaction into splined.db
        ↓
load Select Media from SQLite
```

SPLINED does not parse every track merely to populate the picker. Full
track-level validation remains part of the actual READ or LIVE WRITE processing
pipeline.

A corrupt or incompatible database is renamed with a timestamp and rebuilt.
The build does not alter music-library files.

## Warm startup

A normal warm launch is database-only for Select Media:

```text
open splined.db
        ↓
project current history / bypass / timeout facts
        ↓
load all Artist and Album rows
        ↓
render final Folder Status and picker colors
```

There is no Artist-by-Artist background filesystem validation, no structural
sentinel pass, and no color change merely because the user opens an Artist.
Artist and Album rows are already resident before the first Select Media frame.

The only normal color changes during a session are the result of visible
SPLINED actions, such as processing an Album or adding/removing a bypass.

## Explicit Refresh

`R` / Refresh is the external-library reconciliation boundary.

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

External changes made by Beets, Picard, Navtagger, file managers, or another
application become visible after this explicit Refresh. SPLINED's own LIVE
WRITE and bypass/status actions update the affected database rows immediately.

## Folder Status

The database materializes Album facts including:

- `status`;
- `cover_required` and `cover_found`;
- canonical cover path/name/format/resolution;
- completion, bypass, timeout, and selected-source fields;
- local artwork and selected Album statistics.

Album status values are:

```text
unprocessed
processed
bypassed
timeout
```

Artist status is aggregated from its Album rows:

```text
unprocessed
partial
complete
contains-bypass
```

Completion history, bypass history, timeout policy, and actual local artwork
remain operational authority. SQLite is the persistent, indexed Select Media
read model for those facts; it is not a replacement provider/ranking engine.

## Selected Album artwork and statistics

The right-side Album panels read their initial data from SQLite, including:

- tagged Album, Artist, year, and track count;
- current `cover.*` path and resolution;
- JPEG/PNG/WebP/other counts;
- root, cover, and other filenames;
- WebP size, resolution, and conversion state.

This removes the ordinary delay previously caused by reparsing tags and image
statistics every time focus moved between already-indexed Albums. When LIVE
WRITE changes an Album, SPLINED refreshes that one Album's materialized values
and its parent Artist aggregate.

## Legacy JSON migration

After a usable database is built, the retired v2 file:

```text
/_logs/_history/select-media-status.json
```

is renamed to:

```text
/_logs/_history/select-media-status.json.legacy
```

It is no longer loaded or updated. Completion, source, bypass, and timeout
history JSON files remain authoritative and are not migrated into the picker
database.

## Backup and deletion

For Python/Docker installations, back up:

```text
/config
/credentials
/_logs/_history
/_cache/splined.db
```

The WAL/SHM sidecars may exist while SPLINED is running. Stop SPLINED before
copying the database if a consistent filesystem-level backup is required.

Deleting `splined.db` is safe for music files and persistent history, but the
next interactive launch performs the complete one-time Mutagen/index build
again.

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
