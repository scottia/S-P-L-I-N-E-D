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
suite. At runtime the embedded GUI is materialized only beneath the current
user's Local Application Data runtime directory; it does not require the
user-selected artwork cache. No GUI, watermark, icon, or core sidecar is part
of the release archive. Generated executables, QA images, local settings,
credentials, cache, logs, and history are intentionally excluded from version
control.

Windows v4 stores the validated Config v5 document and interface preferences
in the current user's internal application settings. First run requires the
library, cache, log, and credential paths and creates the selected runtime
directories only after save. The distribution does not create `config.toml`,
`ui.toml`, `config.location`, `_cache`, `_logs`, `config`, `credentials`, or
`docker_builds`. **File > Backup** exports and restores selected internal
settings, interface state, credential JSON, SQLite, and diagnostics in an
optionally password-protected `.spl` container.

The Windows GUI clears prior `splined-*.log` files from `_logs/run` during the
next startup and creates one diagnostic file for the new application session.
SQLite remains the authority for Album state and is not affected by log cleanup.

The repository/native release number is independent of this application's
v3.0.0 Stable identity. The next-patch release workflow must not rewrite this
manifest or `gui/ReleaseInfo.cs`.

## Windows updates

The updater is compiled into the Windows application source. Stable builds
discover the newest official, non-prerelease release containing both
`windows-update.json` and `setup-splined.exe`. The official release workflow
publishes those paired assets whenever Windows is selected. A release for only
another operating system is skipped during Windows update discovery.

Stable and dev channels are isolated. Stable builds accept only versioned
official-release assets and dev builds accept only the fixed rolling dev
assets. Both require the manifest channel, full commit, byte count, SHA-256,
and executable URL to agree before installation.

### Rolling dev updates

`.github/workflows/windows-dev-update.yml` is manually dispatched after the
desired `dev` commit is ready. Ordinary pushes do not rebuild or publish the
executable. The workflow restores its Rust build cache, performs one optimized
build, and publishes a commit-aware `setup-splined.exe` and manifest to the
rolling `windows-dev` prerelease. The manifest contains the exact commit, byte
count, and SHA-256 digest. Dev-channel builds check that manifest after startup
and through **Help > Check for Update...**. The public rolling prerelease is a
temporary compatibility endpoint for installed dev builds, not the stable
distribution path.

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
