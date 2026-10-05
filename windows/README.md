# SPLINED Windows GUI source

This directory contains the finalized native Windows application:

- Application: **S:P:L:I:N:E:D**, following the repository release
- Configuration schema: **Config v5**
- GUI: C# Windows Forms under `gui/`
- Processing core: Rust under `src/`

User-facing Windows behavior is documented in the
[Windows guide](../docs/windows-guide.md). This file covers source,
build, packaging, and update implementation details.

The Cargo manifest is the reproducible Windows build entry point. On Windows,
the build script embeds the WinForms shell, processing core, watermark, and
application icon into one distributable executable:

```text
windows/target/release/splined.exe
```

Build and validate from the repository root:

```text
cargo fmt --manifest-path windows/Cargo.toml -- --check
cargo check --manifest-path windows/Cargo.toml --locked
cargo test --manifest-path windows/Cargo.toml --locked
cargo clippy --manifest-path windows/Cargo.toml --all-targets --locked -- -D warnings
cargo build --manifest-path windows/Cargo.toml --locked --release
```

`gui/TEST-WINDOWS-GUI.cmd` runs the WinForms Config v5 and lifecycle regression
suite. At runtime the embedded GUI is materialized only beneath the current
user's `%LOCALAPPDATA%\SPLINED\runtime` directory; it does not require the
user-selected artwork cache. The shell filename is derived from its content
digest, the identical build is reused, and stale older shells are removed on
startup. No GUI, watermark, icon, or core sidecar is distributed beside
`splined.exe`. Generated executables, QA images, local settings, credentials,
cache, logs, and database files are intentionally excluded from version
control.

Windows stores the validated Config v5 document and interface preferences
in the current user's internal application settings. First run requires the
library, SQL database, temporary run cache, log, and credential paths and creates the selected runtime
directories only after save. Database, run-cache, log, and credential fields initially point
beneath `%LOCALAPPDATA%\SPLINED`, remain editable, and do not change existing
saved or UNC paths. The distribution does not create `config.toml`,
`ui.toml`, `config.location`, `_cache`, `_logs`, `config`, `credentials`, or
`docker_builds`. **File > Backup** exports and restores selected internal
settings, interface state, credential JSON, SQLite, and diagnostics in an
optionally password-protected `.spl` container.

The GUI serializes the validated Config v5 record directly into each Rust
child process's private environment. Snapshot and Album runs do not create a
runtime TOML and must never treat the display label `Windows internal settings`
as a filesystem path. Configuration-load failures exit nonzero so the GUI can
surface the actual error instead of attempting to parse empty snapshot output.

The Windows GUI clears prior `splined-*.log` files from
`<configured log directory>\run` during the next startup and creates one
diagnostic file for the new application session. SQLite remains the authority
for Album state and is not affected by log cleanup.

The historical Config v5 `scan.cache_dir` key is the SQL Database Directory.
`scan.temporary_cache_dir` owns downloaded and derived images; those disposable
files are removed after the Album run while `splined.db` remains untouched.

## GPU enlargement

When `output.upscale_below_ideal = true`, Windows first attempts conventional
Lanczos-3 enlargement on a high-performance hardware Vulkan adapter. Source
and output pixels remain in memory; this path does not write an intermediate
image into Temporary Run Cache and does not change candidate dimensions or
ranking before selection. If a compatible hardware adapter is unavailable,
initialization fails, the image exceeds the adapter's storage-buffer limit, or
GPU execution fails, SPLINED immediately uses its existing in-memory CPU
Lanczos-3 implementation. The final diagnostic reports
`upscale_backend=gpu-lanczos3`, `cpu-lanczos3`, or `none`.

`output.upscale_max_percent` limits enlargement relative to the original short
side. The default `200` permits at most 2× enlargement to Ideal. A source beyond
that limit stays visible for manual review but cannot become Preferred or Auto
eligible through enlargement.

Ideal is an enlargement target, not a maximum output size. SPLINED preserves an
accepted source above Ideal at its native resolution; it does not reduce a
validated 3000×3000 selection to an 1800×1800 file.

This backend is ordinary deterministic resampling, not AI super-resolution.
It provides the replaceable GPU boundary that future AISPLINED validation and
enhancement models can reuse without changing Config v5 behavior.

The `album_completed` GUI event includes selected source, source/final
dimensions, resize/conversion flags, and the backend used. Windows applies core
events before finalizing batch statistics so READ-mode upscales remain visible
in the Album Run Report and cannot be overwritten by a premature `Incomplete`
fallback.

The Windows media snapshot projects year, track count, cover path/name/format,
cover dimensions, and root/cover file counts from existing SQLite Album rows.
The GUI uses that projection for the selected-Album information and Artwork
surface; selection must not trigger a provider call or a second filesystem
inventory pass. Candidate hover uses the already-downloaded run-cache image.

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
