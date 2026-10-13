# SPLINED for Windows

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

`windows/build.rs` compiles `windows/ui/app.slint` and embeds build identity.
The executable links directly to the Rust library modules in `windows/src`.

## Runtime layout

A fresh portable archive contains only:

```text
SPLINED\
  splined.exe
  README-WINDOWS.txt
```

On first portable launch SPLINED creates its application-owned directories next
to the executable: `data`, `_cache`, `_logs`, `credentials`, and
`docker_builds`. A packaged MSIX launch instead stores mutable state under the
current package family's `LocalState\SPLINED` directory. The executable and
read-only package assets remain separate from writable state.

Config v5 remains authoritative. `data\config.toml`, `data\ui.toml`, provider
credential JSON files, and `splined.db` keep their established responsibilities.
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
