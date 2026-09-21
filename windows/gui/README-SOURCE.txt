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

Expected runtime output:

    windows\target\release\splined.exe
    windows\target\release\splined-core.exe
    windows\target\release\splined-watermark.png

BUILD-WINDOWS-GUI.cmd remains available for direct WinForms-only development.
TEST-WINDOWS-GUI.cmd runs the Config v5, selection, filtering, theme, Help,
About, and reusable launch-lifecycle regression suite.

ReleaseInfo.cs is the single Windows GUI version and URL authority:

    S:P:L:I:N:E:D v3.0.0 Stable
    Config v5
    https://github.com/scottia/S-P-L-I-N-E-D/blob/main/docs/README.md

The Rust core consumes the same Config v5 policy, credential-directory,
history, bypass, timeout, candidate, and artwork behavior used by the GUI. The
GUI launches splined-core.exe with redirected streams and exchanges structured
progress and decision events with that process.

Keep splined-watermark.png beside splined.exe. The executable icon is embedded
from app.ico at build time. No local configuration, credentials, cache, logs,
history, generated executables, QA captures, or archives belong in source
control.

The repository/native release number is independent. A repository release such
as 1.0.4 can contain this unchanged Windows v3.0.0 Stable application.
