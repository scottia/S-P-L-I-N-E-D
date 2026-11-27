# SPLINED for Windows

This is the maintainer guide for building and packaging the native Windows
application. Installation, everyday use, backup, and troubleshooting belong in
the [Windows user guide](windows-guide.md).

The Windows application is one native Rust executable built by Cargo. Rust owns
the application state, media-library model, SQLite access, provider pipeline,
MusicBrainz review, artwork decisions, editing/upscale state, configuration,
credentials, backups, and update checks. Slint is the only GUI layer and renders
projections of that Rust state.

There is no managed GUI host, C ABI application bridge, worker executable, or
runtime application DLL.

## Build

From the repository root:

```powershell
cargo fmt --manifest-path build/Windows/Cargo.toml -- --check
cargo check --locked --manifest-path build/Windows/Cargo.toml --all-targets
cargo test --locked --manifest-path build/Windows/Cargo.toml --lib
cargo build --locked --release --manifest-path build/Windows/Cargo.toml --target x86_64-pc-windows-msvc
```

The release executable is:

```text
build\Windows\target\x86_64-pc-windows-msvc\release\splined.exe
```

### Evaluation and official release workflows

Workflow 20 is the rapid Windows and WebGUI evaluation path for current dev or
an explicit PR. Its artifacts are QA-only, carry release_eligible false, and
are never consumed by an official release.

Workflows 01–03 are the official main release entry points. Workflow 01 builds
both applications, Workflow 02 builds Windows only, and Workflow 03 builds
WebGUI only. Each run pins the dispatch commit on main, chooses the next
available numeric main release tag, validates a no-MSIX candidate Build Report,
and keeps its candidate identity inside the same run.

After protected approval, selected targets use a clean full-source build.
Windows releases produce and validate the portable archive and unsigned
production MSIX from the same executable. WebGUI releases validate Python,
PostgreSQL, Node, and the exact production container. A second Build Report
must pass before tagging. Pinned git-cliff generates release notes, the GitHub
Release is published, and WebGUI targets publish the exact smoke-tested image
to GHCR. Windows targets then retain an offline Microsoft Store draft/review
bundle. Partner Center submission is not performed.

No candidate or build run ID is copied between workflows. Required source-PR
Audio Integrity results remain authoritative, and Workflow 20 artifacts cannot
substitute for an official main build.

### Production-equivalent local PC test build

For routine local iterations, close SPLINED and double-click
`E:\apps\SPLINED\Apply Quick Local Update.cmd`, or run:

```powershell
.\build\Windows\scripts\Install-LocalTestBuild.ps1 -Quick
```

Quick mode reuses unchanged optimized Cargo objects, verifies and replaces the
complete executable, then reopens SPLINED. It does not patch a running process
or produce a release artifact.

Before repository promotion or Microsoft Store preparation, close SPLINED and
double-click `E:\apps\SPLINED\Apply Local Test Build.cmd`, or run:

```powershell
.\build\Windows\scripts\Install-LocalTestBuild.ps1
```

The production-equivalent path clears the target-specific release output, disables Cargo incremental
code generation, and uses the same locked optimized build command as the
GitHub Windows release. It verifies the resulting PE and SHA-256, installs it at
`E:\apps\SPLINED\splined.exe`, and reopens the app. It does not commit, push,
tag, run a GitHub workflow, or touch Microsoft Store state. The clean build is
slower than an incremental compile, but makes the local runtime representative
of the executable produced by the release workflow. The optimization is
required because the real 3,884-album library makes an unoptimized Windows UI
too slow for valid testing. This local executable is not a release candidate.

After the local build is approved, open a reviewed topic Draft PR against
`dev`; the owner separately authorizes each merge and any release. Pull requests do not automatically execute strict Windows Rust/Slint
validation. The retained **Windows Rust/Slint — advisory** compatibility check
runs on Ubuntu and reports **Not run — permanent PR policy exception** without
provisioning Rust or a Windows runner. This applies to Windows, Slint,
shared-schema, dependency, workflow, and promotion changes. Targeted local
Windows compilation and regression testing remains appropriate in the isolated
E: acceptance environment. Official Workflows 01 and 02 audit canonical source policies and the exact main candidate, then perform the native optimized build, identity, portable smoke, MSIX contract and isolated development-package validation from the same executable. The retained Store result is an offline draft; no Partner Center submission occurs.

`build/Windows/build.rs` compiles `build/Windows/ui/app.slint` and embeds build identity.
The executable links directly to the Rust library modules in `build/Windows/src`.

## Runtime layout

A fresh portable archive contains only:

```text
SPLINED\
  splined.exe
  README-WINDOWS.txt
```

Standalone SQLite Setup creates neither shared-policy JSON. Shared SQLite only
creates or adopts `media-state-classification.json` and `comp-report-policy.json` in the selected Provider
Credential Directory. If the user also selects WebGUI PostgreSQL coordination,
Test validates the connection, schema, and PostgreSQL policy without writing;
Setup Save then creates `album-coordination.json` and the validated local policy
mirror. Conflicting existing files block Setup and are never overwritten. The
executable carries the repository policy as an unsaved first-run default only;
Windows packages do not install a second copy beside `splined.exe`.

Portable startup discovers `config.toml` and `ui.toml` beside the executable,
or uses an installation-scoped Windows bootstrap to locate an explicitly
selected Configuration Path. Config v5 records that selected path as
`configuration_path`; the bootstrap stores only the path and is not another
settings authority. A v3.0.10 or earlier `configuration-location.toml` is
migrated once and removed. A packaged MSIX launch discovers the TOMLs beneath
the current package family's `LocalState\SPLINED` directory. Legacy portable
`data\config.toml` remains discoverable. A clean first launch creates only the
disposable `splined-setup.tmp` draft beside the executable, and no configuration,
database, cache, log, credential, or obsolete bootstrap directories. Missing,
invalid, or operationally incomplete Config v5
opens structured Setup. Only explicit validated **Save & Open Settings** creates
the configured application-owned directories and writes Config v5 atomically.
The executable and read-only package assets remain separate from writable
state.

Config v5 remains authoritative. Structured Slint Settings maps directly onto
the Rust Config model; it does not use raw TOML or a separate settings
authority. The Setup draft is deleted after both TOMLs and Configuration Path
bootstrap are verified. The active `config.toml`, `ui.toml`, provider credential JSON files,
and `<scan.cache_dir>\splined.db` keep their established responsibilities.
Credentials are not written into Config v5, SQLite, logs, or backups unless the
operator explicitly selects credentials for a protected `.spl` export.

Cross-client Album artwork coordination is optional. In **Settings →
Connections**, save and test **Album Coordination** against the same PostgreSQL
service used by WebGUI, then restart Windows once to start the worker. The
server, username, and password stay in the protected credential directory;
they are not written to Config v5, SQLite, logs, SNAPs, or diagnostics.
`SPLINED_COORDINATION_DATABASE_URL` remains an environment override for
managed launches and automated testing. With neither source configured,
Windows retains normal standalone behavior. Verified Windows writes are still
queued durably in SQLite if a configured service is temporarily unavailable.
Use TLS and a narrowly permitted PostgreSQL account for non-loopback service
access.

## Application modules

- `src/application.rs`: library views, filters, selection, candidate,
  MusicBrainz, source order, and adjustment state.
- `src/scan_controller.rs`: sequential in-process run orchestration, STOP, and
  decision flow.
- `src/media_database.rs`: authoritative `splined.db` projection and updates.
- `src/override_coordination.rs`: optional PostgreSQL last-writer exchange plus
  the durable SQLite acknowledgement/outbox boundary.
- `src/scan_runtime.rs`: real provider/artwork processing.
- `src/settings.rs` and `src/credential_store.rs`: Config v5 and provider
  credential services.
- `src/backup.rs`: selective `.spl` backup/restore, normal restorable SQLite
  online backups, secret-safe diagnostic summaries, and validated authoritative
  Windows SQLite reconciliation snapshots.
- `src/update.rs`: native Store query or pinned portable release notification.
- `ui/app.slint`: functional validation surface only; it does not own business
  decisions.

## Packaging

The portable ZIP and Store MSIX are produced from the same x64 Cargo-built
`splined.exe`. `build/Windows/package/Build-StorePackage.ps1` verifies Partner Center
identity, Store version, x64 PE architecture, file association, full-trust
declaration, and absence of extra DLL host artifacts. The development package
script installs an isolated signed MSIX, activates it through the registered
application identity, verifies LocalState initialization, and removes it.

The release workflow can ask the executable to validate itself without opening
the GUI:

```powershell
splined.exe --native-smoke
splined.exe --write-build-info build-info.json
```

## Validation

The historic root-crate architecture tests are outside the current source layout.
The official Windows release portable allowlist and native smoke checks remain active. Reestablish
current-layout architecture coverage before treating past test claims as current.
Controller and library unit tests cover real state transitions without placing
business authority in Slint callbacks.

The PR compatibility report is informational and never modifies source. It
records the tested revision and permanent policy exception without claiming
that unexecuted diagnostics passed. Release and recovery workflows keep their
existing build, package, smoke, and strict-validation responsibilities.

