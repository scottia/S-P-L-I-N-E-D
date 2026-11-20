# Workflow 01 build source map

**Verified structure only. Not a behavioral policy or runtime acceptance.**

## Active targets

| Target | Source directory | Primary build/packaging input |
| --- | --- | --- |
| GHCR WebGUI | `build/WEBGui/webgui/` | `build/WEBGui/Dockerfile`; Python module `webgui` |
| Windows Portable + Store | `build/Windows/` | `build/Windows/Cargo.toml`; `build/Windows/build.rs` |

Workflow `.github/workflows/01-full-release.yml` is the release entry point.
It builds the Windows Rust/Slint executable using `build/Windows/Cargo.toml`
and publishes GHCR using `build/WEBGui/Dockerfile`. The WebGUI Python
package, backend, static resources, migrations, and tests reside under
`build/WEBGui/webgui/`. Native Rust code, Slint UI, GUI assets, and packaging
reside under `build/Windows/`.

Runtime SQL/JSON defaults consumed by both targets remain in `schema/`.
The WebGUI Dockerfile copies those shared files and
`docker/config.example.toml` into the container. The Windows compiler
embeds required checked-in defaults. Do not duplicate these files as
separate competing policies inside the two build directories.

Release helpers live in `release/` and deployment references in `docker/`.
The active `build/` has exactly two immediate subdirectories:
`WEBGui` and `Windows`. No Markdown documentation belongs in either.

## Archived material and limitations

The pre-migration source tree and withdrawn documentation are preserved in
[`dev-archive`](https://github.com/scottia/S-P-L-I-N-E-D/tree/dev-archive).
The old root Rust CLI is not an active Workflow 01 target. Archived policies,
audits, PR descriptions, and the historic 38-row comparison may be consulted
as evidence but are not the current application policy.

This file does **not** certify either client, its processing behavior, its
existing JSON defaults, or the correctness of its runtime tests.
The current source is awaiting independent verification. Nothing here
authorizes a production release or a merge to `main`.
