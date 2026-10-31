# SPLINED for Windows

This is the maintainer guide for building and packaging the native Windows
application. Installation, everyday use, backup, and troubleshooting belong in
the [Windows user guide](../docs/windows-guide.md).

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
cargo fmt --manifest-path windows/Cargo.toml -- --check
cargo check --locked --manifest-path windows/Cargo.toml --all-targets
cargo test --locked --manifest-path windows/Cargo.toml --lib
cargo build --locked --release --manifest-path windows/Cargo.toml --target x86_64-pc-windows-msvc
```

The release executable is:

```text
windows\target\x86_64-pc-windows-msvc\release\splined.exe
```

### Production-equivalent local PC test build

For routine local iterations, close SPLINED and double-click
`E:\apps\SPLINED\Apply Quick Local Update.cmd`, or run:

```powershell
.\windows\scripts\Install-LocalTestBuild.ps1 -Quick
```

Quick mode reuses unchanged optimized Cargo objects, verifies and replaces the
complete executable, then reopens SPLINED. It does not patch a running process
or produce a release artifact.

Before repository promotion or Microsoft Store preparation, close SPLINED and
double-click `E:\apps\SPLINED\Apply Local Test Build.cmd`, or run:

```powershell
.\windows\scripts\Install-LocalTestBuild.ps1
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

After the local build is approved, promote the tested source through `dev` and
`main`. Relevant Rust, Slint, Cargo, and native build-input changes receive a
**Windows Rust/Slint — advisory** PR report with formatting, `cargo check`, and
Clippy outcomes. The report does not use `-D warnings`, run Windows tests, or
gate approval; documentation and display-message-only changes report Not run.
Workflow 01 retains strict optional Extended Windows validation and performs a
fresh optimized build from versioned `main` source, using that same release
executable for portable and Store packaging.

`windows/build.rs` compiles `windows/ui/app.slint` and embeds build identity.
The executable links directly to the Rust library modules in `windows/src`.

## Runtime layout

A fresh portable archive contains only:

```text
SPLINED\
  splined.exe
  README-WINDOWS.txt
```

Portable startup discovers `data\config.toml` and `data\ui.toml` beside the
executable. A packaged MSIX launch discovers them beneath the current package
family's `LocalState\SPLINED\data` directory. Discovery is read-only: a clean
launch creates no `data`, database, cache, log, credential, or obsolete
bootstrap directories. Missing, invalid, or operationally incomplete Config v5
opens structured Setup. Only explicit validated **Save & Open Settings** creates
the configured application-owned directories and writes Config v5 atomically.
The executable and read-only package assets remain separate from writable
state.

Config v5 remains authoritative. Structured Slint Settings maps directly onto
the Rust Config model; it does not use raw TOML or a separate settings
authority. `data\config.toml`, `data\ui.toml`, provider credential JSON files,
and `<scan.cache_dir>\splined.db` keep their established responsibilities.
Credentials are not written into Config v5, SQLite, logs, or backups unless the
operator explicitly selects credentials for a protected `.spl` export.

## Application modules

- `src/application.rs`: library views, filters, selection, candidate,
  MusicBrainz, source order, and adjustment state.
- `src/scan_controller.rs`: sequential in-process run orchestration, STOP, and
  decision flow.
- `src/media_database.rs`: authoritative `splined.db` projection and updates.
- `src/scan_runtime.rs`: real provider/artwork processing.
- `src/settings.rs` and `src/credential_store.rs`: Config v5 and provider
  credential services.
- `src/backup.rs`: selective `.spl` backup/restore.
- `src/update.rs`: native Store query or pinned portable release notification.
- `ui/app.slint`: functional validation surface only; it does not own business
  decisions.

## Packaging

The portable ZIP and Store MSIX are produced from the same x64 Cargo-built
`splined.exe`. `windows/package/Build-StorePackage.ps1` verifies Partner Center
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

The repository architecture tests verify the single-executable Rust/Slint
contract, absence of C# sources and compiler paths, exact portable allowlist,
Store package inputs, state-root separation, and update-channel selection.
Controller and library unit tests cover real state transitions without placing
business authority in Slint callbacks.

PR advisory diagnostics are informational and never modify source. Their
summary reports the tested revision, commands, exit results, and logs as
Passed, Warnings, Failed, or Not run. Release and recovery workflows keep their
existing build, package, smoke, and strict-validation responsibilities.
