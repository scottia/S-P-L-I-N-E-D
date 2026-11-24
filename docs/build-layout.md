# Release build source boundaries

> Source-location inventory only. Application behavior and policy must be
> verified independently; this file does not approve conflicting legacy rules.

Workflows 21–24 are the supported production release chain. Workflow 21
freezes the exact candidate, Workflows 22 and 23 independently build Windows
and WebGUI, and Workflow 24 publishes their validated artifacts.

## Active source roots

- `build/Windows/`: native Rust/Slint crate (`Cargo.toml`, `src/`,
  `ui/app.slint`, `gui/` resources, and `package/` Store tooling).
  A Windows build produces one native `splined.exe`.
- `build/WEBGui/Dockerfile`: container build definition.
- `build/WEBGui/webgui/`: Python runtime package, backend, browser assets,
  migrations, and WebGUI regression tests; the image copies this package to
  `/app/webgui` and runs `python -m webgui`.
- `schema/`: shared JSON/SQL runtime contracts, not duplicated between
  target folders. WebGUI includes these in GHCR at `/app/schema`; Windows
  embeds referenced defaults at compile time.
- `build/WEBGui/docker/config.example.toml`: default WebGUI Config v5 example included
  in the container at `/app/build/WEBGui/docker/config.example.toml`.
- `build/WEBGui/docker/compose.yaml`: source deployment configuration using the same
  Dockerfile as Workflow 23.

The `build/` directory contains exactly two application-specific directories,
`WEBGui` and `Windows`. Policy documents and operator guides stay in
`docs/`, outside `build/`. Runtime-served `guide.html` is an application
asset, not the authoritative policy register.

## Workflows 21–24 reference chain

Before candidate approval, Workflow 21 checks the accepted media-state, COMP
and lifecycle policies, Config v5 UI metadata and SQLite schema contracts.
Required pull-request validation separately checks protected JSON diffs in the
**Canonical policy JSON — review gate**. These enforce the existing approved
register, not a new policy or a deployed-database acceptance.

When an unused synchronized version is unavailable, Workflow 21 may create an
immutable temporary candidate that stamps `build/Windows/Cargo.toml`, its
lockfile and Windows package manifests, plus
`build/WEBGui/webgui/backend/splined.py`. It never writes that stamp to a
protected branch. Workflow 22 compiles Windows with
`--manifest-path build/Windows/Cargo.toml`; Workflow 23 runs the relocated
Python tests, Node syntax checks, and the Docker build using
`build/WEBGui/Dockerfile`. Workflow 24 publishes that exact smoke-tested image
with its candidate revision label instead of rebuilding it.

Docs and release source packages are distinct from the GHCR runtime image.
The image is controlled by `.dockerignore` and the Dockerfile COPY inputs.

## Current-source boundary

Retired root-native CLI sources, architecture tests, benchmarks, and old
file paths are outside the supported `/main` source layout. Historical
material is not an authority for new corrections; use current source and
the owner-approved policy register, with applicable PR validation.

This source map does **not** certify documentation policy correctness.
Conflicting behavior contracts remain subject to owner approval.
