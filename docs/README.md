# S:P:L:I:N:E:D Documentation

These versioned repository pages are the canonical public documentation for
[S:P:L:I:N:E:D](../README.md).

## Supported implementations

S:P:L:I:N:E:D has separate supported runtimes:

- **Windows GUI:** v3.0.0 Stable, using Config v5.
- **Windows source:** [`../windows/`](../windows/README.md), containing the finalized GUI and its Config v5 processing core.
- **Repository-root native command line:** Config v5 implementation used by Linux and macOS; its application version follows repository releases.
- **Python/Docker:** Config v5 runtime whose application version follows repository releases and whose interactive workflow uses the Ratatui TUI.

Application release versions and configuration schema versions are independent.
Do not infer one from another.

## Start here

- [Installation and first run](installation-first-run.md)
- [Windows v4 interface and runtime](windows-v4-interface.md)
- [Config v5 reference](config-v5-reference.md)
- [Credentials and provider setup](credentials-providers.md)
- [API/OAuth credential validation](oauth-validation.md)
- [MusicBrainz OAuth](musicbrainz-oauth.md)
- [Source policies and Range Types](source-policies-range-types.md)
- [Select Media and status colors](media-filter-status-colors.md)
- [History, retention, bypass, and timeout](history-retention-bypass-timeout.md)
- [Python Ratatui TUI](ratatui-tui.md)
- [SPLINED media database](splined-media-database.md)
- [Select Media SQLite status authority](select-media-status-cache.md)
- [Docker installation](../docker/README.md)

## Python Ratatui display model

Interactive Python/Docker scans use the Ratatui TUI when stdin/stdout are
interactive terminals.

```text
TUI
    = operational / decision interface

--no-tui
    = advanced verbose / diagnostic interface
```

The TUI uses exactly two first-class themes, `OLED` and `CHALK`, and shares the
same source, history, bypass, timeout, and status semantics as the Python scan
engine.

### Persistent Select Media model

Python/Docker Select Media is populated from the persistent tag-identified
SQLite database:

```text
<scan.cache_dir>/splined.db
```

Default Docker path:

```text
/_cache/splined.db
```

The first interactive launch performs a complete Artist/Album index build. One
representative audio file per Album is read with Mutagen to obtain Album Artist,
Album, MusicBrainz identities, year, compilation, and sort metadata. Durable
per-Album checkpoints make an interrupted first build resumable. The completed
snapshot is validated and published only after every expected row is present.
The index also materializes configured `cover.*` state and the values required
by the right-side Album artwork/statistics panels.

Warm startup opens SQLite read-only, projects current history/bypass/timeout
facts in memory, and loads the complete Artist/Album picker before the first
Select Media frame. A WAL-free database on a NAS/bind mount may be copied to a
temporary local read snapshot for compact sequential hydration; that copy is
deleted after startup and never replaces the persistent database.
There is no Artist-by-Artist background sentinel validation and no color change
merely because the user opens an Artist.

Artist and Album identity is tag-based:

```text
Artist: MusicBrainz Album Artist ID, then normalized ALBUMARTIST fallback
Album:  MusicBrainz Album/Release ID, then normalized tagged fallback
```

Filesystem paths remain mutable locations. They are stored but are not indexed
as Artist or Album identity.

SQL authority identity and visible picker ownership are separate:

```text
/music/Christina Aguilera/AGUILERA
    → Artist Picker: Christina Aguilera

/music/[Soundtracks]/A Star Is Born Soundtrack
    → Artist Picker: [Soundtracks]

/music/[Various Artists]/[Various Artists]/<group>/<album>
    → Artist Picker: [Various Artists]
```

The first physical directory beneath the library root owns the picker row.
`albumartist` / `musicbrainz_albumartistid` remains the normal tagged authority
for Album identity and artwork lookup. Authority-artist counts can therefore
differ from physical Artist Picker folder counts without indicating duplicate
Album rows.

External library changes are reconciled only after the explicit Refresh action.
The stable old picker remains visible while Refresh inventories the library,
reuses unchanged representative tags, reads new/changed tags, and commits the
replacement model in one transaction.

SPLINED's own LIVE WRITE, bypass, timeout, and completion actions update the
affected rows and physical Artist-folder aggregate immediately.

See [SPLINED media database](splined-media-database.md) for the schema,
identity rules, migration behavior, backup requirements, and diagnostic
queries.

### Select Media and processing

The current TUI provides:

- four equal top control panels: S:P:L:I:N:E:D Album Status, Album Selection,
  Album Scanning, and S:P:L:I:N:E:D Launch;
- a 25% Artist Picker, 50% Album Picker, and 25% Album artwork/statistics column
  on wide layouts;
- explicit READ or LIVE WRITE Launch authority;
- single-click exclusive Album focus, with explicit bulk selection controls;
- local `cover.*` preview and yellow saved resolution;
- a no-cover call to action;
- scrollable, materialized Selected Album statistics;
- source-grouped candidate presentation on a shared terminal-cell grid;
- URL-backed hover/focus candidate preview;
- OLED and CHALK native-image cleanup parity;
- a per-Album final run report and guarded return to the same resident Select Media
  session.

Direct mouse/touch interaction is provided through the isolated
`splined-pyratatui-input` crossterm extension. Render-time hit regions drive
selection and the list under the pointer receives wheel/touch scrolling.

## AISPLINE Config v5 boundary

Config v5 keeps the future AISPLINE policy under the canonical `[aisplined]`
section. AISPLINE processing has not begun and the runtime remains disabled by
default:

```toml
[aisplined]
enabled = false
endpoint = ""
minimum_short_side = 600
allow_below_minimum_override = false
```

No AI result, enhancement, activity, or backend capability is inferred merely
because placeholder configuration or layout code exists.

## Configuration examples

- [`../config.example.toml`](../config.example.toml) is the native/Windows Config v5 example.
- [`../docker/config.example.toml`](../docker/config.example.toml) is the Python/Docker Config v5 example with container paths.

Both examples use the central credential-directory architecture. Provider
secrets and normal provider credential filenames do not belong in
`config.toml`.

## Authoritative status colors

| Color | Meaning |
| --- | --- |
| White | Default / unprocessed |
| Orange | Processed; manually reprocessable |
| Red | Bypassed; confirm removal before processing |
| Purple | Partial Artist or timeout-active Album, depending on row type |
| Green | Artist complete |
| Blue | Artist contains at least one bypassed Album |

The database materializes these facts for immediate display, while completion
history, bypass history, timeout policy, and actual local artwork remain the
operational authority used by execution.

## Python command equivalence

Current Python command behavior remains authoritative in:

- `python/splined.py --help`
- `python/splined.py`
- `python/splined_scan.py`

Interactive Python operational scans additionally use the Ratatui TUI.
Redirected/scripted runs retain the plain CLI.

## Security, backups, and safe operation

- Never commit credential JSON, API keys, OAuth tokens, or secrets.
- Use Read mode to evaluate without changing artwork in Album folders.
- Test Write mode against a copy, backup, snapshot, or staging library first.
- Back up `config/`, `credentials/`, and `<scan.cache_dir>/splined.db`.
- For Python/Docker, also back up `<scan.cache_dir>/splined.db`; other candidate
  and sample data under the cache directory remains disposable.

## Project links

- [Repository README](../README.md)
- [Windows portable instructions](../release/README-WINDOWS.txt)
- [Linux portable instructions](../release/README-LINUX.txt)
- [macOS portable instructions](../release/README-MACOS.txt)
- [Latest releases](https://github.com/scottia/S-P-L-I-N-E-D/releases/latest)
