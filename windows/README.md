# SPLINED Windows GUI source

This directory contains the finalized native Windows application:

- Application: **S:P:L:I:N:E:D v3.0.0 Stable**
- Configuration schema: **Config v5**
- GUI: C# Windows Forms under `gui/`
- Processing core: Rust under `src/`

The Cargo manifest is the reproducible Windows build entry point. On Windows,
the build script compiles the WinForms shell and the Rust processing core into:

```text
windows/target/release/splined.exe
windows/target/release/splined-core.exe
windows/target/release/splined-watermark.png
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
suite. Generated executables, QA images, local configuration, credentials,
cache, logs, and history are intentionally excluded from version control.

The repository/native release number is independent of this application's
v3.0.0 Stable identity. The next-patch release workflow must not rewrite this
manifest or `gui/ReleaseInfo.cs`.
