# SPLINED Windows desktop

The Windows desktop is an upstream Tauri v2 application. `splined.exe` contains
the frontend and the authoritative Rust processing backend. Scans, provider
requests, decisions, MusicBrainz matching, artwork processing, cancellation,
and completion reporting cross an in-process command/event boundary. No SPLINED
processing child is launched.

## Build and test

```powershell
cd windows
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
node --test frontend/tests/app-model.test.mjs
cargo tauri build --no-bundle
```

Unsigned developer builds deliberately disable public automatic installation.
A production build must provide the stable updater public key at compile time,
the updater private signing key to the build tool, and a timestamped publisher
certificate to the Windows bundle signer.

## Portable state

The executable directory is always the portable root; process CWD is ignored.
The durable Windows-owned files are:

```text
SPLINED\
  splined.exe
  data\
    config.toml
    ui.toml
```

`data\` is created only after first-run save or one-time migration. Relative
Config v5 paths resolve from the portable root. Absolute and UNC paths remain
unchanged. SQLite, credentials, history, logs, and caches retain the locations
selected in Config v5.

When no portable state exists, a legacy ConfigV5/UiV4 pair may be read once and
written into the portable files. The legacy values are not deleted and are not
used as runtime authority afterward. The `.spl` shell association is separate.

## Backup and restore

The `.spl` format preserves selective Config v5, interface, credential,
database, and diagnostic categories. Password-protected backups remain
compatible with the prior format. Restore validates content before atomic
replacement. Selected categories overwrite their destinations; unselected
categories are untouched. Resolution is deterministic across different
portable roots.

## Signed updates

The application uses the official updater plugin and its signed metadata and
package contract. The release pipeline also applies and verifies a timestamped
publisher signature on `splined.exe` and the Windows update installer. The
installer receives the current portable root and replaces the application in
that directory while leaving `data\` and configured external resources alone.

The portable ZIP contains exactly `splined.exe` and `README-WINDOWS.txt`.
The update installer, its updater signature, and update metadata are separate
release assets.
