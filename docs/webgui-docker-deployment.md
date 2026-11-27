# WebGUI Docker deployment

This is the deployment-source reference for the two-target `/main` layout;
it does not independently certify runtime migration or shared policy parity.

## Files

- [Compose](../build/WEBGui/docker/compose.yaml)
- [Environment template](../build/WEBGui/docker/.env.example)
- [Config v5 example](../build/WEBGui/docker/config.example.toml)
- [WebGUI Dockerfile](../build/WEBGui/Dockerfile)
- [Shared SQL and behavioral contracts](../schema)
- [Official release build and publication map](build-layout.md)

The Compose file under `build/WEBGui/docker/` resolves its Docker
**build context `../../..` to the repository root**, because the image
requires both `build/WEBGui/webgui/` and the canonical `schema/`.
The root `.dockerignore` allows only the required build inputs.

For a local, deliberate installation, copy `.env.example` to an untracked
`.env`, populate secrets and the host music path, then run from the
**repository root**:

```sh
docker compose --env-file build/WEBGui/docker/.env -f build/WEBGui/docker/compose.yaml up --build -d
```

The example starts WebGUI and PostgreSQL. Persistent volumes hold the
catalog, configuration, cache, logs, and credentials. The host music
directory is provided by `SPLINED_MUSIC_DIRECTORY`; WebGUI binds
to `127.0.0.1:8080` by default unless deliberately configured.

The Dockerfile copies the shared schema contracts without introducing
a Docker-specific media-state/COMP policy. Its example configuration
is installed at `/app/build/WEBGui/docker/config.example.toml`,
matching the runtime's source-relative lookup. Windows uses the same
canonical behavioral contracts, but its SQLite catalog is independent.

For deployment, use an owner-reviewed versioned GHCR image and preserve
database/configuration backups. Never commit `.env`, passwords, API
tokens, credentials, or actual machine-specific music paths.

See the [policy register](POLICY-REGISTER.md) and
[validation report](SCHEMA-VALIDATION.md) for approved requirements
versus the remaining cross-client checks.
