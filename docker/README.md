# Docker Installation

Docker is the recommended installation method for Linux servers, NAS systems, and container-based deployments.

The published image uses the validated Python runtime associated with the current S:P:L:I:N:E:D release. Media, configuration, credentials, logs, and history stay outside the image; disposable scan cache is mounted separately.

## Image

```text
scottia/splined:latest
```

Use `latest` to follow the current stable image. For reproducible deployments, pin the image to the tag shown on the [latest GitHub release](https://github.com/scottia/S-P-L-I-N-E-D/releases/latest).

## Related files

- [`../python/Dockerfile`](../python/Dockerfile) — container image definition
- [`../python/requirements.txt`](../python/requirements.txt) — Python runtime dependencies
- [`../config.example.toml`](../config.example.toml) — configuration template
- [`../release/README-LINUX.txt`](../release/README-LINUX.txt) — native Linux portable installation

## Minimal Compose

```yaml
services:
  splined:
    image: scottia/splined:latest
    container_name: splined
    restart: unless-stopped
    environment:
      SPLINED_CONFIG: /config/config.toml
    volumes:
      - /path/to/music:/music:rw
      - /path/to/splined/config:/config:rw
      - /path/to/splined/credentials:/credentials:rw
      - /path/to/splined/_cache:/_cache:rw
      - /path/to/splined/_logs:/_logs:rw
      - /path/to/splined/_logs/_history:/logs/_history:rw
```

This example intentionally shows only the mounts S:P:L:I:N:E:D needs for a straightforward deployment.

Create the host directories before starting the container so ownership and permissions are explicit rather than relying on Docker to create missing bind-source directories.

```text
/path/to/splined/
├── config/
├── credentials/
├── _cache/
│   └── samples/
└── _logs/
    └── _history/
```

Runtime files such as `splined_debug.log` and the JSON history files are created as needed by S:P:L:I:N:E:D.

## Mount semantics

| Host path | Container path | Purpose | Typical access |
| --- | --- | --- | --- |
| `/path/to/music` | `/music` | Music library | `ro` for Read mode, `rw` for Write mode |
| `/path/to/splined/config` | `/config` | `config.toml` | `rw` |
| `/path/to/splined/credentials` | `/credentials` | Provider credential JSON files | `rw` |
| `/path/to/splined/_cache` | `/_cache` | Disposable candidate/cache data and samples | `rw` |
| `/path/to/splined/_logs` | `/_logs` | Persistent log files | `rw` |
| `/path/to/splined/_logs/_history` | `/logs/_history` | Persistent completion/source/bypass history | `rw` |

The host history directory is intentionally under `_logs/_history`. The current container runtime reads that persistent history through `/logs/_history`.

If S:P:L:I:N:E:D will only run in Read mode, mounting the library read-only provides an additional safeguard:

```yaml
      - /path/to/music:/music:ro
```

Write mode requires `/music` to be writable.

## Configuration

The image defaults to:

```text
SPLINED_CONFIG=/config/config.toml
```

Place `config.toml` in the host directory mounted at `/config`. Paths inside the Docker configuration refer to container paths, not host paths.

Typical Docker path values include:

```toml
[scan]
cache_dir = "/_cache"

[library]
music_library = "/music"

[credentials]
credential_dir = "/credentials"
```

See [`../config.example.toml`](../config.example.toml) for the broader configuration template.

## Basic usage

Verify the running container:

```bash
docker compose exec splined splined -V
```

Show help and resolved configuration paths:

```bash
docker compose exec splined splined --help
```

Run an operational scan:

```bash
docker compose exec splined splined --scan-dir
```

Follow container output:

```bash
docker compose logs -f splined
```

The image starts in idle mode when no command is supplied, allowing normal CLI commands through `docker compose exec`.
