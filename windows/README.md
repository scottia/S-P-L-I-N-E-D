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

The manual all-OS release workflow performs one clean Windows release build,
native smoke, PE/version checks, and portable allowlist auditing. The same built
executable/DLL pair also feeds Store packaging and isolated MSIX QA when its
`microsoft_store` input is enabled (the default for Windows/All). Its
`extended_windows_validation` input is off by default; enable it when a release
run should also execute the full Rust and WinForms GUI regression suite.

Release CI caches Cargo registry and Git dependency downloads but never
`windows/target`. Restoring the compiled target had cost several minutes and
was immediately discarded by the mandatory clean release build. The normalized
dependency key ignores only the release-version field, so ordinary version
bumps reuse downloads while dependency changes still invalidate the cache.

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

## Release workflows

The repository has four supported workflows:

1. `SPLINED (All OS) and GHCR` prepares an exact release commit from `main`,
   builds the selected platforms, and only after successful builds pushes the
   numeric tag and publishes the GitHub/GHCR outputs. A Windows build always
   creates `splined-windows-x86_64.zip` and `windows-update.json`; when selected,
   those same binaries also create the Store MSIX and are submitted with
   generated Store Highlights after GitHub Release/GHCR publication.
2. `SPLINED (Tagged) > MS Store Package Resolution` rebuilds and validates the
   Store MSIX plus Store Highlights from an existing numeric tag, then stops at
   the Actions artifact.
3. `SPLINED > MS Store Publish & Update` rebuilds and validates an existing
   numeric tag and submits it to the live Store product.
4. `SPLINED (Docker) > GHCR Resolution` publishes an existing release to GHCR
   and refreshes that existing GitHub Release body when Docker needs recovery.

All release notes come from `git-cliff 2.14.2`. One context classified by the
root `cliff.toml` renders both the formatted GitHub release body and concise
Store Highlights; unconventional commits fall into `📦 Other Changes`. See
[`docs/release-automation.md`](../docs/release-automation.md).

A selected-platform build failure, including selected Store packaging or QA,
occurs before numeric tag creation, so it cannot leave an orphan release tag.

## Packaging with WinAppCli

The production listing is approved and live at
[Microsoft Store — Install SPLINED](https://apps.microsoft.com/detail/9p8g4gmbbvbs?hl=en-US&gl=US).
Microsoft Store is the managed-install/update Windows channel; the portable ZIP
is the equally supported no-install/direct channel.

During a normal Windows/All release, `SPLINED (All OS) and GHCR` pins upstream
Microsoft WinAppCli and creates two isolated x64 layouts from the same clean
runtime pair used by the Portable ZIP. The production layout keeps the exact Partner Center identity
`Psycotix.SPLINED` / `CN=FE370EF6-D95D-4A6F-9AAB-2654E6DE00FE`, is packed
unsigned, and never runs `create-debug-identity`. CI unpacks the result and
validates its identity, publisher display name `Psycotix`, release version,
desktop family, `.spl` association, `runFullTrust`, runtime pair, and PE machine.

A separate runner-temporary development layout receives debug identity, a
disposable development certificate, signing, install/activation/identity checks,
and clean uninstall. Development identity and signing cannot mutate the Store
layout or artifact. Development private keys remain ephemeral.

`SPLINED (Tagged) > MS Store Package Resolution` retains this same packaging and
QA contract as a package-only recovery path. `SPLINED > MS Store Publish &
Update` applies it when an existing tag needs Store publication or
republication. Neither creates or changes a release tag.

The primary GitHub artifact is `splined-windows-x86_64.zip`, containing exactly:

```text
SPLINED\splined.exe
SPLINED\runtime\splined-core.dll
SPLINED\README-WINDOWS.txt
```

For future Store releases and updates, `SPLINED-x64-store-unsigned.msix` and
`STORE-HIGHLIGHTS.txt` are uploaded in the
`splined-windows-store-submission` GitHub Actions artifact for audit/recovery.
When **Microsoft Store** is enabled, the normal workflow stages the package,
preserves existing listing/package metadata, updates only the en-US ReleaseNotes
field, and submits it automatically for Microsoft processing. The unsigned MSIX
is not an end-user installer and is not published as a normal GitHub Release
download. Microsoft Store processing supplies production signing and
Windows-managed updates. The live Store product
PFN is `Psycotix.SPLINED_8pvn5te36e43t` and Store ID is
`9P8G4GMBBVBS`. The App Installer template is a separate, disabled
direct-distribution design, not the Store update path. See
[`docs/windows-store.md`](../docs/windows-store.md).

## Updates

**Help > Check for Updates** selects its authority with
`ConfigStore.IsPackaged`. Portable checking reads GitHub Releases and
`windows-update.json`; it is notification-only and can open the official release
page for manual replacement. Packaged checking uses
`Windows.Services.Store.StoreContext`; **UPDATE NOW** delegates download and
installation to Windows/Microsoft Store, while **LATER** defers. If Store API
integration is unavailable, the GUI offers the official Store product page and
never Portable ZIP instructions.

There is no custom updater, executable/DLL extraction, hidden helper, shell
lifecycle script, self-reinvocation, process-name killing, or in-app binary
replacement. Users replace `splined.exe`, `runtime\splined-core.dll`, and the
README manually while preserving `data\` and configured external resources.
