# Docker Installation

Docker is the simplest deployment method for Linux servers, NAS systems, and
other container hosts.

## Image

```text
ghcr.io/scottia/splined:latest
```

To pin a release instead of following `latest`, select an available image tag
from the GitHub package or release listing.

## Quick start

```yaml
services:
  splined:
    image: ghcr.io/scottia/splined:latest
    container_name: splined
    security_opt:
      - no-new-privileges:true
    cap_drop:
      - ALL
    restart: unless-stopped
    volumes:
      - /path/to/music:/music:rw
      - /path/to/splined/config:/config:rw
      - /path/to/splined/credentials:/credentials:rw
      - /path/to/splined/_cache:/_cache:rw
      - /path/to/splined/_logs:/_logs:rw
```

Change only the host paths on the left side of each mount. No port mapping is
required for normal CLI operation.

The image does not force a fixed non-root UID/GID because its documented bind
mounts commonly belong to different host or NAS accounts. Operators who want a
non-root process can add a Compose `user: "UID:GID"` value matching the host
mount ownership. SPLINED never changes host-mount ownership or applies broad
permission changes.

Before first use, copy
[`docker/config.example.toml`](config.example.toml) to `config.toml` in the
host directory mounted at `/config`.

## Persistent layout

The Python/Docker runtime uses Config v5 and creates runtime files as features
are used. A typical host layout is:

```text
/path/to/splined/
├── config/
│   └── config.toml
├── credentials/
├── _cache/
│   ├── splined.db
│   └── samples/
└── _logs/
    ├── run/
    └── _history/
        ├── chosen-source-history.json
        ├── scan-completed-history.json
        └── bypass-source-history.json
```

`_cache` now contains two classes of data:

```text
splined.db
    persistent tag-identified Select Media database

everything else
    disposable candidate, sample, and transient run data
```

Normal cache cleanup preserves `splined.db`, its SQLite WAL/SHM sidecars, and
quarantined recovery copies.

For the deployment path used during development:

```yaml
- /mnt/psy_data/downloads/0_backups/splined/_cache:/_cache:rw
```

the database is stored on the host as:

```text
/mnt/psy_data/downloads/0_backups/splined/_cache/splined.db
```

## Container paths

| Container path | Purpose |
| --- | --- |
| `/music` | Music library |
| `/config/config.toml` | Main configuration |
| `/credentials` | Provider credential JSON files |
| `/_cache/splined.db` | Persistent authority-Artist/Album index and physical-folder picker model |
| `/_cache` | Database plus disposable candidate/cache data |
| `/_cache/samples` | Selected scan samples |
| `/_logs` | Persistent runtime logs |
| `/_logs/_history` | Persistent completion, source, bypass, and timeout history |

For Read mode, `/music` may be mounted read-only:

```yaml
      - /path/to/music:/music:ro
```

Write mode requires `/music` to be writable.

## First launch and warm startup

The first interactive TUI launch builds `/_cache/splined.db` by inventorying
Artist/Album folders, reading one representative file per Album with Mutagen,
and materializing local `cover.*` information. Per-Album checkpoints make the
first build resumable; the visible picker is published only after the completed
snapshot validates.

Subsequent launches load Select Media read-only from SQLite. With the default
`auto` policy, a WAL-free database on a NAS/bind mount is copied sequentially
to temporary container-local storage, opened as an immutable snapshot, and
deleted after hydration. If that is unsafe or unavailable, SPLINED reads the
persistent database directly. No warm path performs background
Artist-by-Artist Album Status validation. External library changes become
visible after the explicit Refresh action in Select Media.

The optional environment variable accepts `auto`, `always`, or `never`:

```yaml
    environment:
      SPLINED_SQLITE_LOCAL_SNAPSHOT: auto
```

`auto` is the default. This setting controls only the temporary warm-read copy;
it does not relocate or replace `/_cache/splined.db`.

The visible Artist Picker is grouped by the first physical directory below
`/music`. Tagged Album Artist/MusicBrainz identity remains artwork/search
authority, so authority-row totals can differ from picker-folder totals.

An old `/_logs/_history/select-media-status.json` is renamed
`select-media-status.json.legacy` after the database is ready.

See [SPLINED media database](../docs/splined-media-database.md).

## Basic usage

Verify the running container:

```bash
docker compose exec splined splined -V
```

Show help and resolved paths:

```bash
docker compose exec splined splined --help
```

Scan the configured library:

```bash
docker compose exec splined splined --scan-dir
```

When `docker compose exec` provides interactive stdin and stdout, operational
scans open the Python [Ratatui TUI](../docs/ratatui-tui.md). Use `--no-tui` for
the plain terminal stream. Automation and `docker compose exec -T` remain plain
automatically.

Select the alternate CHALK theme with:

```bash
docker compose exec splined splined --tui-theme CHALK --scan-dir
```

Scan one Artist or directory:

```bash
docker compose exec splined splined --scan-dir "10,000 Maniacs"
```

Follow container output:

```bash
docker compose logs -f splined
```

The image starts in idle mode when no command is supplied, so normal SPLINED
commands can be run with `docker compose exec`.

## TUI input extension

The image build uses published `pyratatui==0.3.0` for rendering and builds the
small `splined-pyratatui-input` ABI3 wheel in a separate Rust builder stage.
The final runtime image contains the wheel, not the Rust toolchain. This
extension enables crossterm mouse/touch capture, direct hit-tested controls,
wheel/touch scrolling, terminal image protocol detection, and image cleanup.

## Backups

For Python/Docker, back up:

```text
/config
/credentials
/_logs/_history
/_cache/splined.db
```

Stop SPLINED before copying the SQLite database when a consistent raw
filesystem backup is required. The `-wal` and `-shm` sidecars may exist while
SPLINED is running.

Deleting `splined.db` does not delete music or operational history, but it
forces the complete one-time Mutagen/index build on the next TUI launch.

## Advanced mounts

The Quick Start uses one persistent mount for all logs and history:

```text
/_logs
└── _history
```

A separate history mount is not required. Advanced deployments may split any
documented path into its own bind mount when different storage, backup, or
permission policies are required.

## Related files

- [`config.example.toml`](config.example.toml) — Python/Docker Config v5 template
- [`../config.example.toml`](../config.example.toml) — native/Windows Config v5 template
- [`../python/Dockerfile`](../python/Dockerfile) — container image definition
- [`../python/requirements.txt`](../python/requirements.txt) — Python runtime dependencies
- [`../docs/ratatui-tui.md`](../docs/ratatui-tui.md) — TUI behavior and controls
- [`../docs/splined-media-database.md`](../docs/splined-media-database.md) — database schema and lifecycle
