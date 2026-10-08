# SPLINED Windows desktop

The Windows frontend is the mature WinForms application accepted in 1.0.60,
with later correctness fixes retained. `splined.exe` loads the fixed
`runtime\splined-core.dll` and calls authoritative Rust processing in-process
through a stable UTF-8 C ABI. Structured callbacks carry progress, decisions,
candidates, MusicBrainz results, compilation events, cancellation, errors, and
completion without redirected process streams. No SPLINED worker or updater
process exists.

## Build and test

Windows is x64-only. Install the `x86_64-pc-windows-msvc` Rust target, then run:

```powershell
cargo fmt --manifest-path windows/Cargo.toml -- --check
cargo check --locked --manifest-path windows/Cargo.toml --all-targets --target x86_64-pc-windows-msvc
cargo test --locked --manifest-path windows/Cargo.toml --lib --target x86_64-pc-windows-msvc
cargo clippy --locked --manifest-path windows/Cargo.toml --all-targets --target x86_64-pc-windows-msvc -- -D warnings
windows\gui\RUN-GUI-QA.cmd
windows\gui\BUILD-WINDOWS-GUI.cmd
```

Cargo checks and tests do not compile the WinForms executable. The explicit
build script opts into one GUI build and produces the pair under
`windows\target\x86_64-pc-windows-msvc\release`:

```text
splined.exe
splined_core.dll
```

Packaging renames only the DLL filename to `splined-core.dll`. Both binaries
embed the same release version/commit identity, and startup rejects a mismatched
pair. Release CI deletes the release output before building and verifies both PE
Machine fields are AMD64 (`0x8664`).

## Native boundary

`gui/NativeCore.cs` loads the absolute fixed DLL path and owns the managed
callback lifetime. `src/ffi.rs` exposes initialization, build identity, media
snapshot, embedded-artwork preview, existing-cover editing, scan start,
decision submission, cancellation, active-state query, and Rust-buffer release.
Every exported operation contains Rust panics and returns structured errors.
Scan work and callbacks execute away from the WinForms UI thread.

The GUI provides both the executable root and the writable state root during
initialization. This prevents the DLL's `runtime` directory or process CWD from
becoming path authority.

## Portable and packaged state

Portable mode uses:

```text
SPLINED\
  splined.exe
  runtime\
    splined-core.dll
  data\                  # created after first save or migration
    config.toml
    ui.toml
```

The executable directory is the portable root. In installed MSIX mode Config v5
and UI state use the package's per-user LocalState directory because installed
package files are read-only. Absolute, mapped-drive, and UNC paths remain
unchanged. SQLite, credentials, history, logs, and caches retain their Config v5
locations.

Legacy ConfigV5/UiV4 Registry values are one-time migration input only. Portable
files or package LocalState are runtime authority afterward. Portable `.spl`
association is intentional HKCU shell integration; packaged association comes
from the MSIX manifest. Opening a backup pre-fills selective Restore and never
automatically restores all categories.

## Packaging with WinAppCli

The release workflow pins upstream Microsoft WinAppCli and uses it to generate
MSIX assets, generate a non-installed loose-layout debug identity, package x64
payloads, create disposable development certificates, sign a CI-only package,
install/activate/inspect it with real package identity, and uninstall it. Loose
identity registration is available for development machines with Developer Mode
enabled; release CI does not depend on that machine-wide setting. Development
private keys remain ephemeral.

The primary GitHub artifact is `splined-windows-x86_64.zip`, containing exactly:

```text
SPLINED\splined.exe
SPLINED\runtime\splined-core.dll
SPLINED\README-WINDOWS.txt
```

`SPLINED-x64.msix` is the unsigned Store-ready output. Public installation
requires a future external publisher signature or Microsoft Store signing; the
development certificate is not public trust. The App Installer template stays
disabled until that installed channel has a stable publisher and HTTPS endpoint.

## Updates

Portable checking is notification-only and opens the official release page.
There is no custom updater, executable/DLL extraction, hidden helper, shell
lifecycle script, self-reinvocation, process-name killing, or in-app binary
replacement. Users replace `splined.exe`, `runtime\splined-core.dll`, and the
README manually while preserving `data\` and configured external resources.
