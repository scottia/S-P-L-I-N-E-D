# SPLINED WebGUI and PostgreSQL

SPLINED WebGUI is the Python/browser implementation. PostgreSQL is required
for its catalog, completion ledger, audit events, and persistent work queue.
It does not fall back to SQLite or share Windows' live SQLite database.

This guide owns container deployment. For browser operation after deployment,
use the [WebGUI guide](../docs/browser-workspace-guide.md).

## Supported topology

The checked-in [Compose stack](compose.yaml) is the straightforward deployment:

| Service | Source | Role |
| --- | --- | --- |
| `webgui` | Built locally from `webgui/Dockerfile` | Serves the browser application on port 8080 |
| `postgres` | `postgres:17` | Stores the authoritative WebGUI database |

Compose waits for the PostgreSQL health check before starting WebGUI. The
services communicate over Compose's internal network using `PGHOST=postgres`.
PostgreSQL has no host port in the stock definition.

The WebGUI application also supports an external PostgreSQL server through
`SPLINED_DATABASE_URL` or the standard `PGHOST`, `PGPORT`, `PGDATABASE`,
`PGUSER`, and `PGPASSWORD` variables. The stock Compose file always supplies
its bundled `postgres` service values, so an external server requires deliberate
custom orchestration or a reviewed Compose override. It is not a database-free
mode. WebGUI applies its PostgreSQL migrations when it connects.

## Deploy the source-built Compose stack

Prerequisites are Docker with Compose support, a persistent host music
directory, and permission for container UID/GID 1000 to access every mounted
path it must read or write.

1. Copy [`docker/.env.example`](.env.example) to `docker/.env`.
2. Set all four values:

   | Variable | Required behavior |
   | --- | --- |
   | `SPLINED_MUSIC_DIRECTORY` | Host music directory mounted at `/music`; read access is enough for Read mode, while Write mode needs write access |
   | `SPLINED_POSTGRES_PASSWORD` | Long random password used by both PostgreSQL and WebGUI |
   | `SPLINED_WEB_TOKEN` | Different long random token used for first administrator enrollment and password recovery |
   | `SPLINED_BIND_ADDRESS` | Host address for port 8080; `127.0.0.1` restricts direct access to the server itself |

3. From the repository root, build and start the stack:

   ```sh
   docker compose --env-file docker/.env -f docker/compose.yaml up -d --build
   ```

4. Open `http://127.0.0.1:8080` on the server, or use the configured server
   address. Create the single administrator with a username, password,
   confirmation, and `SPLINED_WEB_TOKEN`.
5. Complete Settings with paths as seen *inside the container*, save, and run
   **Full Refresh Library Index**. Begin in Read mode.

The deployment token is not stored in PostgreSQL. Later sign-ins use the saved
username and password. **Forgot password?** validates the unchanged deployment
token, replaces only the password hash, and invalidates active browser sessions.

For network access, keep the token configured and terminate HTTPS in a trusted
reverse proxy. The application refuses a non-loopback bind without a token;
the stock Compose file requires a token even when bound to loopback.

## Published image and release boundary

The release workflow publishes `ghcr.io/scottia/splined` from
`webgui/Dockerfile` after a numbered release succeeds. Versioned images use a
`vX.Y.Z` tag and the workflow also updates `latest`. Verify the desired tag on
the [GitHub Releases page](https://github.com/scottia/S-P-L-I-N-E-D/releases)
and [container package page](https://github.com/scottia/S-P-L-I-N-E-D/pkgs/container/splined)
before deployment; a source commit or merge alone does not publish an image.

The checked-in Compose file uses `build:` and the local image name
`splined-webgui:local`; it does **not** pull the GHCR image. Running the
published image therefore requires equivalent environment variables, mounts,
networking, and a PostgreSQL service in an external stack definition. Adding a
checked-in prebuilt-image Compose variant would change deployment behavior and
needs separate review.

## Persistence and paths

| Mount / volume | Purpose |
| --- | --- |
| PostgreSQL `catalog` volume | Catalog, completion ledger, audit records, administrator hash, and queued work |
| `/config` | Saved Config v5 settings |
| `/credentials` | Provider credentials, separate from Config v5 |
| `/_cache` | Candidates, original embedded-image backups, staged work, and shared Artist Art by default |
| `/_logs` | Operational logs |
| `/music` | Music files and album covers |

Keep the named volumes and any external Artist Art directory across container
replacement. Queued jobs verify the staged image hash before writing, so the
cache is not disposable while work is pending. Run one WebGUI application
process per catalog, and do not run concurrent Windows and WebGUI writers
against the same audio files.

## Migration, backup, and recovery

There is no automatic conversion or live synchronization with Windows.
**Settings → Import an existing SQLite catalog** accepts a server-accessible
snapshot path and copies recognized rows from a read-only SQLite source into an
initial PostgreSQL catalog. It is not a browser file upload. The current
endpoint refuses a destination with Album rows, but it does not validate a
source schema version, prove that every destination table is empty, rewrite
music paths, or preflight their compatibility with the `/music` mount. Use
only a disposable, path-compatible snapshot or build a fresh WebGUI index. The
source snapshot remains unchanged.

Back up PostgreSQL with a consistent database dump. Also preserve Config v5,
provider credentials, cache/staged artwork, external Artist Art, and the music
library. A SQLite snapshot, settings JSON, or image cache is not a PostgreSQL
backup, and restoring the catalog does not restore changed music tags.

## Maintainer validation

Install `webgui/requirements-test.txt`, point `SPLINED_TEST_DATABASE_URL` at a
disposable PostgreSQL database, and run:

```sh
python -m unittest discover -s webgui/tests -v
```

The integration suite creates and removes its own schemas and uses generated
media fixtures. Release validation also checks browser JavaScript, builds the
container, and boots it against PostgreSQL. Windows validation is separate.

See the [Config v5 reference](../docs/config-v5-reference.md),
[WebGUI guide](../docs/browser-workspace-guide.md), and
[release automation guide](../docs/release-automation.md).
