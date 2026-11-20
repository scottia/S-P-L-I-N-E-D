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

### Workflow 04 optimized Windows benchmark (testing only)

[Workflow 04](../.github/workflows/04-optimized-build-test.yml) is a separate manual test workflow, not a modification of the clean release procedure above. Choose **Windows** or **All**; optionally enable **Extended Windows validation**, local **Store QA** simulation and **Cold and optimized** comparison. The default cached build restores compiler dependency artifacts and cleans **only the `splined-windows` package**, then rebuilds the complete current Rust/Slint executable with `--locked --release` and `CARGO_INCREMENTAL=0`. Its build identity must match the exact checked-out SHA and the source version, and its x64 PE/native smoke checks must pass. An optional truly cold compile uses a separate temporary target and does not poison the warm cache.

The first run normally populates the compiled dependency cache; measure at least two equivalent runs to compare warm cache hits to the existing Workflow 01 historical cold baseline (roughly 8–15 minutes of Rust compilation in November 2026). Cache restore/upload can offset savings; review measured end-to-end duration before adopting any change to Workflow 01. QA executables and optional unsigned Store packages remain Actions artifacts only. **No version stamping, release, tag, Store submission, GHCR publication or installed local state is changed.**

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
`main`; the owner separately authorizes each merge and any release. Pull requests do not automatically execute strict Windows Rust/Slint
validation. The retained **Windows Rust/Slint — advisory** compatibility check
runs on Ubuntu and reports **Not run — permanent PR policy exception** without
provisioning Rust or a Windows runner. This applies to Windows, Slint,
shared-schema, dependency, workflow, and promotion changes. Targeted local
Windows compilation and regression testing remains appropriate in the isolated
E: acceptance environment. Workflow 01 retains strict optional Extended
Windows validation and always performs a fresh optimized production build from
versioned `main` source, using that same executable for portable and Store
packaging. Workflow 01 checks the accepted canonical source policies, Config v5 and in-memory SQLite DDL **before** version stamping. Owner policy requires Extended Windows validation before a Microsoft
Store submission; submitting a Store update without that setting is blocked by the release preflight.

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
Workflow 01's portable allowlist and native smoke checks remain active. Reestablish
current-layout architecture coverage before treating past test claims as current.
Controller and library unit tests cover real state transitions without placing
business authority in Slint callbacks.

The PR compatibility report is informational and never modifies source. It
records the tested revision and permanent policy exception without claiming
that unexecuted diagnostics passed. Release and recovery workflows keep their
existing build, package, smoke, and strict-validation responsibilities.

