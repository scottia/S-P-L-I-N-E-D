# WebGUI Docker — source and deployment reference

**Documentation review in progress.** This is a deployment-source map,
not a verified guide to application behavior, database reconciliation,
credentials policy, or operator workflows.

The published GHCR image is built by Workflow 01 using
`build/WEBGui/Dockerfile` from the repository root. The container copies
`build/WEBGui/webgui/`, shared `schema/` runtime contracts, and
`docker/config.example.toml`; it starts `python -m webgui`.

The repository's local development stack is `docker/compose.yaml`, using
`build.context: ..` and `build/WEBGui/Dockerfile`. Its declared inputs are
`SPLINED_POSTGRES_PASSWORD`, `SPLINED_WEB_TOKEN`, and
`SPLINED_MUSIC_DIRECTORY`, with optional `SPLINED_BIND_ADDRESS`.
Never commit secret values.

For deployed releases, use a reviewed and pinned tag from
[GHCR](https://github.com/scottia/S-P-L-I-N-E-D/pkgs/container/splined)
and confirm compatible runtime storage and PostgreSQL backups before upgrades.

For the current source and deployment boundary, see
[documentation status](../docs/README.md) and
[Workflow 01 build layout](../docs/build-layout.md).
Detailed installation, migration, recovery, and troubleshooting procedures
must be verified before publication as operator instructions.
