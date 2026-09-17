# Docker Installation

Docker is the simplest deployment method for Linux servers, NAS systems, and other container hosts.

## Image

```text
ghcr.io/scottia/splined:latest
```

To pin a specific release instead of following `latest`:

```text
ghcr.io/scottia/splined:v1.0.2
```

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

Before first use, place `config.toml` in the host directory mounted at `/config`. The repository [`config.example.toml`](../config.example.toml) is the reference configuration template.

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

- [`../config.example.toml`](../config.example.toml) — configuration template
- [`../python/Dockerfile`](../python/Dockerfile) — container image definition
- [`../python/requirements.txt`](../python/requirements.txt) — Python runtime dependencies
