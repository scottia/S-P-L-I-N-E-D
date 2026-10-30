# SPLINED WebGUI and PostgreSQL

The container starts the SPLINED WebGUI service. All normal operations, configuration, provider credentials, and work review take place in a browser.

The checked-in [Compose stack](compose.yaml) builds the current source and starts PostgreSQL alongside it. Published `2.0.5` images include the Python WebGUI/PostgreSQL migration. Merging source does not replace an existing registry image; the numbered release workflow publishes a new WebGUI image from `webgui/Dockerfile` only when a release is explicitly built.

## Deploy

Use your Docker/NAS stack manager to deploy the repository's Compose definition with these environment values from [.env.example](.env.example):

- `SPLINED_MUSIC_DIRECTORY`: host music directory, mounted at `/music` with write access for Live Write.
- `SPLINED_POSTGRES_PASSWORD`: long random database password.
- `SPLINED_WEB_TOKEN`: a different long random administrator enrollment and recovery token. It is required for network binding, authorizes first setup and password recovery, and is not stored in PostgreSQL.
- `SPLINED_BIND_ADDRESS`: defaults to `127.0.0.1`; use the intended LAN bind address for another computer.

Maintainers can build and start the same stack with `docker compose --env-file docker/.env -f docker/compose.yaml up -d --build`. This is deployment tooling; application use requires only the browser. Docker Desktop or your NAS manager owns start/stop/restart and stack environment values.

Open `http://127.0.0.1:8080` on the server, or the configured server address. The first network visit creates the single administrator with username, password, confirmation, and `SPLINED_WEB_TOKEN`; later logins use username and password. After enrollment, **Forgot password?** validates that same unchanged token, replaces only the WebGUI password hash, and invalidates active browser sessions. Complete **Settings**, save, then **Full Refresh Library Index**. The default is Read mode. Set **mode** to `write` when ready to permit cover, artist-art, and track-tag writes. The V/A / OST Artist Images Directory is an image directory; blank uses Persistent Cache Directory itself, while any other mounted path or folder name may be selected. PostgreSQL connection values are supplied to the application through standard `PGHOST`, `PGPORT`, `PGDATABASE`, `PGUSER`, and `PGPASSWORD` variables; a complete `SPLINED_DATABASE_URL` is also supported. WebGUI does not use the native `sqlite_shared` setting.

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
