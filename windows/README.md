# SPLINED Windows GUI source

This directory contains the finalized native Windows application:

- Application: **S:P:L:I:N:E:D v3.0.0 Stable**
- Configuration schema: **Config v5**
- GUI: C# Windows Forms under `gui/`
- Processing core: Rust under `src/`

The Cargo manifest is the reproducible Windows build entry point. On Windows,
the build script embeds the WinForms shell, processing core, watermark, and
application icon into one distributable executable:

```text
windows/target/release/splined.exe
```

Build and validate from the repository root:

```text
cargo fmt --manifest-path windows/Cargo.toml --all -- --check
cargo check --manifest-path windows/Cargo.toml --locked
cargo test --manifest-path windows/Cargo.toml --locked
cargo clippy --manifest-path windows/Cargo.toml --locked -- -D warnings
cargo build --manifest-path windows/Cargo.toml --locked --release
```

`gui/TEST-WINDOWS-GUI.cmd` runs the WinForms Config v5 and lifecycle regression
suite. At runtime the embedded GUI is materialized only in the disposable
`_cache/runtime/` directory; no GUI, watermark, icon, or core sidecar is part
of the release archive. Generated executables, QA images, local configuration,
credentials, cache, logs, and history are intentionally excluded from version
control.

The repository/native release number is independent of this application's
v3.0.0 Stable identity. The next-patch release workflow must not rewrite this
manifest or `gui/ReleaseInfo.cs`.

## Rolling dev updates

Code-bearing pushes to `dev` run `.github/workflows/windows-dev-update.yml`.
Documentation-only and updater-workflow-only pushes do not rebuild the
executable. The workflow restores its Rust build cache, performs one optimized
build, and publishes a commit-aware `setup-splined.exe` and manifest to the
rolling `windows-dev` prerelease. The manifest contains the exact commit, byte
count, and SHA-256 digest. Dev-channel builds check that manifest after startup
and through **Help > Check for Update...**. Stable builds do not consume the dev
channel.

The rolling updater deliberately does not repeat the full test, clippy, and GUI
QA matrix before its distribution build. Those checks remain developer/CI
validation; the dev updater's job is a fast, deterministic full-executable
replacement for an active test environment.

The GUI accepts only the fixed HTTPS repository release asset, validates the
manifest and executable before launch, and refuses installation during an
active Album run. The setup process waits briefly for the prior Windows process
to release `splined.exe`, preserves a rollback copy during replacement, starts
the verified executable, and leaves Config v5, credentials, cache/SQLite, and
logs untouched.
