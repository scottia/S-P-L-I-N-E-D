# SPLINED WebGUI and PostgreSQL

SPLINED WebGUI is the Python/browser implementation. PostgreSQL is required
for its catalog, administrator account, completion ledger, audit events, and
persistent work queue. It does not fall back to SQLite or share Windows' live
SQLite database.

Choose one deployment path:

- **Basic — source-built Compose:** use the repository's checked-in
  [`compose.yaml`](compose.yaml). It builds the current checkout and starts
  PostgreSQL beside WebGUI.
- **Advanced — published image:** use the verified, versioned GHCR image in
  your own Compose stack, with either the included PostgreSQL service shown
  below or a separately managed PostgreSQL server.

For browser operation after deployment, use the
[WebGUI guide](../docs/browser-workspace-guide.md).

## Before you start

Every supported container deployment needs:

- Docker with Compose support, or an equivalent NAS/container manager;
- one WebGUI container and one reachable PostgreSQL database;
- persistent storage for PostgreSQL, Config v5, credentials, cache/staged
  artwork, and logs;
- a host music directory mounted at `/music`;
- long, different values for the PostgreSQL password and
  `SPLINED_WEB_TOKEN`.

The image runs as UID/GID 1000. That account needs read access to the music
mount in Read mode and write access before Write mode can change covers or
tags. It also needs write access to `/config`, `/credentials`, `/_cache`,
`/_logs`, and any separate Artist Art directory.

## Basic: build the checked-in Compose stack

The repository's Compose definition is the simplest source deployment. It:

1. starts `postgres:17` with the named `catalog` volume;
2. waits for `pg_isready` to report a healthy database;
3. builds `webgui/Dockerfile` as `splined-webgui:local`;
4. starts WebGUI with `PGHOST=postgres` on Compose's private network; and
5. publishes only WebGUI port 8080. PostgreSQL has no host port.

Copy [`.env.example`](.env.example) to `docker/.env` and replace every
example value:

```dotenv
SPLINED_MUSIC_DIRECTORY=/srv/music
SPLINED_POSTGRES_PASSWORD=replace-with-a-long-random-password
SPLINED_WEB_TOKEN=replace-with-a-different-long-random-token
SPLINED_BIND_ADDRESS=127.0.0.1
```

| Variable | Purpose |
| --- | --- |
| `SPLINED_MUSIC_DIRECTORY` | Host music directory mounted at `/music` |
| `SPLINED_POSTGRES_PASSWORD` | Password supplied to both PostgreSQL and WebGUI |
| `SPLINED_WEB_TOKEN` | First-administrator enrollment and password-recovery token |
| `SPLINED_BIND_ADDRESS` | Host address for port 8080; `127.0.0.1` allows direct access only from the server |

From the repository root:

```sh
docker compose --env-file docker/.env -f docker/compose.yaml up -d --build
```

Confirm both services are running and PostgreSQL is healthy:

```sh
docker compose --env-file docker/.env -f docker/compose.yaml ps
docker compose --env-file docker/.env -f docker/compose.yaml logs --tail=100 webgui postgres
```

Open `http://127.0.0.1:8080` on the server, or use the configured server
address. Create the single administrator, complete Settings with paths as seen
*inside the container*, and run **Full Refresh Library Index**. Begin in Read
mode.

## Advanced: run the published GHCR image

The release workflow publishes `ghcr.io/scottia/splined` only after a
numbered release succeeds. A source commit or merge does not publish an image.
The example below pins the public image verified during this review:

```text
ghcr.io/scottia/splined:v2.0.13
Linux/amd64
Release revision: b1dda55968c431c6db15559ea3452ab679e9ce81
```

Check the [release](https://github.com/scottia/S-P-L-I-N-E-D/releases/tag/2.0.13)
and [container package](https://github.com/scottia/S-P-L-I-N-E-D/pkgs/container/splined)
before deploying. Keep the versioned tag pinned until you deliberately select
and validate a later release; do not turn this example into an unreviewed
`latest` deployment.

### Complete GHCR Compose example

Save this as `compose.yaml` in a new deployment directory:

```yaml
services:
  postgres:
    image: postgres:17
    restart: unless-stopped
    environment:
      POSTGRES_DB: splined
      POSTGRES_USER: splined
      POSTGRES_PASSWORD: ${SPLINED_POSTGRES_PASSWORD:?Set the PostgreSQL password}
    volumes:
      - catalog:/var/lib/postgresql/data
    networks:
      - splined
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U splined -d splined"]
      interval: 10s
      timeout: 5s
      retries: 10

  webgui:
    image: ghcr.io/scottia/splined:v2.0.13
    restart: unless-stopped
    depends_on:
      postgres:
        condition: service_healthy
    environment:
      PGHOST: postgres
      PGPORT: "5432"
      PGDATABASE: splined
      PGUSER: splined
      PGPASSWORD: ${SPLINED_POSTGRES_PASSWORD:?Set the PostgreSQL password}
      SPLINED_WEB_TOKEN: ${SPLINED_WEB_TOKEN:?Set a WebGUI access token}
    ports:
      - "${SPLINED_BIND_ADDRESS:-127.0.0.1}:8080:8080"
    volumes:
      - config:/config
      - cache:/_cache
      - logs:/_logs
      - credentials:/credentials
      - ${SPLINED_MUSIC_DIRECTORY:?Set the host music directory}:/music
    networks:
      - splined

networks:
  splined:

volumes:
  catalog:
  config:
  cache:
  logs:
  credentials:
```

Create a `.env` file beside it using the same four-variable example from the
Basic section, then start the stack:

```sh
docker compose --env-file .env up -d
```

Both services join only the private `splined` network. The service name
`postgres` becomes the database hostname. `depends_on` delays WebGUI until
PostgreSQL passes its health check; it does not replace monitoring or backups.
The image's own health check probes WebGUI at `/api/status`.

### Use an external PostgreSQL server

The application accepts either `SPLINED_DATABASE_URL` or the standard
`PGHOST`, `PGPORT`, `PGDATABASE`, `PGUSER`, and `PGPASSWORD`
variables. The stock repository Compose file always supplies its bundled
database values, so external PostgreSQL is an Advanced custom deployment.

The external database must be reachable before WebGUI starts. Its user needs
permission to create and migrate SPLINED's tables, indexes, and related
objects in the selected database. Use the PostgreSQL server's supported TLS,
availability, and backup controls.

A minimal WebGUI-only Compose file is:

```yaml
services:
  webgui:
    image: ghcr.io/scottia/splined:v2.0.13
    restart: unless-stopped
    environment:
      SPLINED_DATABASE_URL: ${SPLINED_DATABASE_URL:?Set the PostgreSQL URL}
      SPLINED_WEB_TOKEN: ${SPLINED_WEB_TOKEN:?Set a WebGUI access token}
    ports:
      - "${SPLINED_BIND_ADDRESS:-127.0.0.1}:8080:8080"
    volumes:
      - config:/config
      - cache:/_cache
      - logs:/_logs
      - credentials:/credentials
      - ${SPLINED_MUSIC_DIRECTORY:?Set the host music directory}:/music

volumes:
  config:
  cache:
  logs:
  credentials:
```

Its `.env` needs:

```dotenv
SPLINED_DATABASE_URL=postgresql://splined:replace-url-encoded-password@db.example:5432/splined?sslmode=require
SPLINED_WEB_TOKEN=replace-with-a-long-random-token
SPLINED_MUSIC_DIRECTORY=/srv/music
SPLINED_BIND_ADDRESS=127.0.0.1
```

Do not add a second PostgreSQL container when using this form. Do not expose
the stock PostgreSQL container merely to make it “external”; choose one
database authority and back it up consistently.

### Verify and update the stack

After either Advanced deployment starts:

```sh
docker compose --env-file .env ps
docker compose --env-file .env logs --tail=100 webgui
```

For the bundled-database example, also inspect `postgres` and confirm it is
healthy. Open the configured WebGUI address only after the service is healthy.
If startup reports a database/configuration problem, correct the database
connection or mounted paths; do not add a SQLite fallback.

Before changing the pinned image tag, back up PostgreSQL and the persistent
mounts. Then edit the versioned tag deliberately and run:

```sh
docker compose --env-file .env pull
docker compose --env-file .env up -d
```

WebGUI applies its PostgreSQL migrations when it connects. Review the selected
release before allowing that connection. Do not use `docker compose down -v`
for an ordinary update because `-v` removes named volumes.

## Persistent data and permissions

| Mount / volume | Purpose | Backup requirement |
| --- | --- | --- |
| PostgreSQL `catalog` volume or external database | Catalog, account hash, completion ledger, events, and queued work | Consistent PostgreSQL backup |
| `/config` | Saved Config v5 | Preserve |
| `/credentials` | Provider credentials, separate from Config v5 | Preserve securely |
| `/_cache` | Candidates, original embedded-image backups, staged work, and shared Artist Art by default | Preserve while work is queued; recommended for recovery |
| `/_logs` | Operational logs | Optional for recovery; useful for support |
| `/music` | Music files and album/artist artwork | Independent music-library backup |

Keep the named volumes when replacing a container. Queued jobs verify their
staged image hash before writing, so `/_cache` is not disposable while work
is pending. If Artist Art uses another directory, mount and back up that path
too.

Run one WebGUI application process per catalog. Do not run concurrent Windows
and WebGUI writers against the same audio files.

## Network and account safety

The deployment token is not stored in PostgreSQL. Later sign-ins use the saved
administrator username and password. **Forgot password?** validates the
unchanged deployment token, replaces only the password hash, and invalidates
active browser sessions.

For network access, keep the token configured and terminate HTTPS in a trusted
reverse proxy. WebGUI refuses a non-loopback bind without a token. Both Compose
examples require a token even when bound to loopback.

PostgreSQL should remain private unless an external-database design explicitly
requires network access. Restrict that access at the database and network
layers; a WebGUI token is not a PostgreSQL credential.

## Migration, backup, and recovery

There is no automatic conversion or live synchronization with Windows.
**Settings → Import an existing SQLite catalog** accepts a server-accessible
snapshot path; it is not a browser upload. The current endpoint opens the
source read-only and refuses a destination with Album rows, but it does not
validate a source schema version, prove every destination table is empty,
rewrite music paths, or preflight their compatibility with `/music`. Use only
a disposable, path-compatible snapshot, or build a fresh WebGUI index.

Back up PostgreSQL with a consistent dump or the database platform's supported
snapshot process. Also preserve Config v5, credentials, cache/staged artwork,
external Artist Art, and the music library. A SQLite snapshot, settings JSON,
or image cache is not a PostgreSQL backup, and restoring catalog data does not
restore changed music tags.

## Maintainer validation

Install `webgui/requirements-test.txt`, point `SPLINED_TEST_DATABASE_URL` at
a disposable PostgreSQL database, and run:

```sh
python -m unittest discover -s webgui/tests -v
```

The integration suite creates and removes its own schemas and uses generated
media fixtures. Release validation also checks browser JavaScript, builds the
container, and boots it against PostgreSQL. Windows validation is separate.

See the [Config v5 reference](../docs/config-v5-reference.md),
[WebGUI guide](../docs/browser-workspace-guide.md), and
[release automation guide](../docs/release-automation.md).
