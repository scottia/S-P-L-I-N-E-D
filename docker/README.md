# Docker Installation

Docker is the simplest deployment method for Linux servers, NAS systems, and other container hosts.

## Image

```text
ghcr.io/scottia/splined:latest
```

To pin a release instead of following `latest`, select an available image
tag from the GitHub package or release listing. Concrete version tags are not
used in this evergreen installation example.

## Quick start

```yaml
services:
  splined:
    image: ghcr.io/scottia/splined:latest
    container_name: splined
    security_opt:
      - no-new-privileges:true
    restart: unless-stopped
    volumes:
      - /path/to/music:/music:rw
      - /path/to/splined/config:/config:rw
      - /path/to/splined/credentials:/credentials:rw
      - /path/to/splined/_cache:/_cache:rw
      - /path/to/splined/_logs:/_logs:rw
```

Change only the host paths on the left side of each mount.

No port mapping is required for normal CLI operation.

Before first use, copy
[`docker/config.example.toml`](config.example.toml) to `config.toml` in the
host directory mounted at `/config`.

The Python/Docker runtime is separately supported and currently requires
Config v4. The repository-root `config.example.toml` is the Windows/native
Config v5 example and is not interchangeable with the Docker example.

S:P:L:I:N:E:D creates its runtime cache, sample, log, and history directories as needed. A typical host layout becomes:

```text
/path/to/splined/
├── config/
│   └── config.toml
├── credentials/
├── _cache/
│   └── samples/
└── _logs/
    ├── splined_debug.log
    └── _history/
        ├── chosen-source-history.json
        ├── scan-completed-history.json
        └── bypass-source-history.json
```

Runtime files appear when the associated feature is used.

## Container paths

| Container path | Purpose |
| --- | --- |
| `/music` | Music library |
| `/config/config.toml` | Main configuration |
| `/credentials` | Provider credential JSON files |
| `/_cache` | Disposable candidate/cache data |
| `/_cache/samples` | Selected scan samples |
| `/_logs` | Persistent logs |
| `/_logs/_history` | Persistent completion, source, and bypass history |

For Read mode, `/music` may be mounted read-only:

```yaml
      - /path/to/music:/music:ro
```

Write mode requires `/music` to be writable.

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

Scan one artist or directory:

```bash
docker compose exec splined splined --scan-dir "10,000 Maniacs"
```

Follow container output:

```bash
docker compose logs -f splined
```

The image starts in idle mode when no command is supplied, so normal SPLINED commands can be run with `docker compose exec`.

## Advanced mounts

The Quick Start uses one persistent mount for all logs and history:

```text
/_logs
└── _history
```

You do not need a separate history mount. Advanced deployments may split any documented path into a separate bind mount when different storage, backup, or permission policies are required.

## Related files

- [`config.example.toml`](config.example.toml) — Python/Docker Config v4 template
- [`../config.example.toml`](../config.example.toml) — Windows/native Config v5 template
- [`../python/Dockerfile`](../python/Dockerfile) — container image definition
- [`../python/requirements.txt`](../python/requirements.txt) — Python runtime dependencies
