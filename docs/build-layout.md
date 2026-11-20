# Release build source boundaries

> Source-location inventory only. Application behavior and policy must be
> verified independently; this file does not approve conflicting legacy rules.

Workflow 01 (`.github/workflows/01-full-release.yml`) is the supported
production release entry point for Windows, WebGUI, and the All selection.

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
  Dockerfile as Workflow 01.

The `build/` directory contains exactly two application-specific directories,
`WEBGui` and `Windows`. Policy documents and operator guides stay in
`docs/`, outside `build/`. Runtime-served `guide.html` is an application
asset, not the authoritative policy register.

## Workflow 01 reference chain

Workflow 01 stamps `build/Windows/Cargo.toml`, its lockfile and Windows
package manifests, plus `build/WEBGui/webgui/backend/splined.py`. Windows
releases compile with `--manifest-path build/Windows/Cargo.toml`; WebGUI
validation invokes the relocated Python tests, Node syntax checks, and a
Docker build using `build/WEBGui/Dockerfile`. GHCR publication builds that
same Dockerfile from the release tag, with an explicit OCI revision label.

Docs and release source packages are distinct from the GHCR runtime image.
The image is controlled by `.dockerignore` and the Dockerfile COPY inputs.

## Archive boundary

The `dev-archive` branch preserves the complete pre-migration `/dev`
snapshot, including the root-native CLI, old architecture tests, benchmarks,
old file paths, and historic references. No archive-only content is an
implicit authority for new Codex corrections. Do not change `/main` until
this refactor passes the relevant runtime verification.

This source map does **not** certify documentation policy correctness.
Conflicting behavior contracts remain subject to owner approval.
