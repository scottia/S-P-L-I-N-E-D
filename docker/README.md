# SPLINED WebGUI and PostgreSQL

The container starts the Python WebGUI server. All normal operations, configuration, provider credentials, and work review take place in a browser. The retired Python terminal interface and its native input extension are not included.

The checked-in [Compose stack](compose.yaml) builds the current source and starts PostgreSQL 17 alongside it. Published `2.0.4` images predate this migration; merging source does not replace an existing registry image. The numbered release workflow builds the new WebGUI image from `webgui/Dockerfile` when a future release is explicitly published.

## Deploy

Use your Docker/NAS stack manager to deploy the repository's Compose definition with these environment values from [.env.example](.env.example):

- `SPLINED_MUSIC_DIRECTORY`: host music directory, mounted at `/music` with write access for Live Write.
- `SPLINED_POSTGRES_PASSWORD`: long random database password.
- `SPLINED_WEB_TOKEN`: a different long random browser access token.
- `SPLINED_BIND_ADDRESS`: defaults to `127.0.0.1`; use the intended LAN bind address for another computer.

Maintainers can build and start the same stack with `docker compose --env-file docker/.env -f docker/compose.yaml up -d --build`. This is deployment tooling; application use requires only the browser. Docker Desktop or your NAS manager owns start/stop/restart and stack environment values.

Open `http://127.0.0.1:8080` on the server, or the configured server address. Sign in, complete **Settings**, save, then **Refresh Library Index**. The default is Read mode. Set **mode** to `write` when ready to permit cover, artist-art, and track-tag writes. PostgreSQL connection values are supplied to the application through standard `PGHOST`, `PGPORT`, `PGDATABASE`, `PGUSER`, and `PGPASSWORD` variables; a complete `SPLINED_DATABASE_URL` is also supported.

## Persistent storage

| Mount / service | Purpose |
| --- | --- |
| PostgreSQL `catalog` volume | Authoritative catalog, COMP ledger, audit records, queued work |
| `/config` | Saved Config v5 settings |
| `/credentials` | Provider credentials, separate from Config |
| `/_cache` | Candidate files, original embedded-image backups, staged embedding snapshots, shared artist artwork by default |
| `/_logs` | Operational logs |
| `/music` | Music files and album covers |

The WebGUI runs as UID/GID 1000. Bind-mounted music and custom directories must grant that account the required access. Keep cache and staged artwork persistent: queued jobs verify their staged image hash before writing. Container upgrades retain these volumes. Do not run multiple WebGUI workers against the same catalog, or concurrent Windows and WebGUI writers against the same audio files.

PostgreSQL is not exposed on a host port by this Compose stack. For network browser use, keep the access token and place HTTPS in front of the service. Use an existing reverse proxy for remote deployments.

## Migration and backup

There is no automatic conversion of a live Windows database. **Settings → Import an existing SQLite catalog** copies a named server-side read-only snapshot into an empty PostgreSQL catalog. Match music paths to the server mount before import, or refresh a new index instead. The source snapshot is not modified. Windows retains SQLite and operates independently.

Use a consistent PostgreSQL dump for database backup. Also preserve config, credentials, staged artwork/cache, external artist-art directories, and music backups. The retired Python `.spl` terminal commands are not installed in this image. Restoring a catalog alone does not restore changed music tags.

## Maintainer validation

Install `webgui/requirements-test.txt`, set `SPLINED_TEST_DATABASE_URL` to a disposable PostgreSQL database, and run `python -m unittest discover -s webgui/tests -v` with the repository and `webgui/backend` on `PYTHONPATH`. Integration tests create unique schemas, generate silent FLAC files, verify real image/tag writes, and drop only their own schemas. GitHub validation also builds and boots the Linux container; native Windows checks remain separate.

See the [WebGUI user guide](../docs/browser-workspace-guide.md) and [source Dockerfile](../webgui/Dockerfile).
