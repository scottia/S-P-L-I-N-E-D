S:P:L:I:N:E:D v3.0.0 Stable - Windows GUI source
==================================================

This directory contains the finalized C# Windows Forms shell. The repository
windows/Cargo.toml is the authoritative build entry point and compiles this GUI
together with its Rust processing core.

From the repository root on Windows run:

    cargo fmt --manifest-path windows/Cargo.toml --all -- --check
    cargo check --manifest-path windows/Cargo.toml --locked
    cargo test --manifest-path windows/Cargo.toml --locked
    cargo clippy --manifest-path windows/Cargo.toml --locked -- -D warnings
    cargo build --manifest-path windows/Cargo.toml --locked --release

Expected distributable output:

    windows\target\release\splined.exe
    windows\target\release\splined-core.exe
    windows\target\release\splined-update.exe

BUILD-WINDOWS-GUI.cmd invokes the authoritative Cargo build.
TEST-WINDOWS-GUI.cmd runs the Config v5, selection, filtering, theme, Help,
About, one-time portable-settings migration, deterministic two-root selective
restore, and reusable launch-lifecycle regression suite.

ReleaseInfo.cs is the single Windows GUI version and URL authority:

    S:P:L:I:N:E:D v3.0.0 Stable
    Config v5
    https://github.com/scottia/S-P-L-I-N-E-D/blob/main/docs/README.md

The Rust worker consumes the same Config v5 policy, credential-directory,
history, bypass, timeout, candidate, and artwork behavior used by the GUI.
Config v5 and UI state live beneath the portable root in data\config.toml and
data\ui.toml; legacy HKCU values are one-time migration input only.
`splined.exe` is the fixed WinForms GUI with embedded image/icon resources;
The release archive places `splined-core.exe` under runtime as the fixed
processing worker. The GUI never
re-invokes itself as `--scan-dir`, and normal runtime never extracts or manages
an executable file. The separately published splined-update.exe is temporary
and runs only after explicit update approval. No local configuration, credentials, cache, logs, history,
generated build executables, QA captures, or archives belong in source control.

The repository/native release number is independent. A repository release such
as 1.0.4 can contain this unchanged Windows v3.0.0 Stable application.
