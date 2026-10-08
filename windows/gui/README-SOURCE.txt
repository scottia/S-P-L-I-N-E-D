S:P:L:I:N:E:D Windows WinForms source
=======================================

This directory contains the restored mature WinForms frontend. The accepted
1.0.60 application is the visual and interaction reference; later correctness
fixes remain integrated.

BUILD-WINDOWS-GUI.cmd performs the explicit x64 release build. Ordinary Cargo
checks and Rust tests do not emit splined.exe. RUN-GUI-QA.cmd compiles and
runs the deterministic GUI/model regression harness.

The packaged runtime is:

    splined.exe
    runtime\splined-core.dll

NativeCore.cs loads the fixed DLL and uses the stable UTF-8 C ABI implemented in
windows\src\ffi.rs. Rust processing remains authoritative and runs in the same
process. No worker/updater executable or runtime code extraction is used.

Portable Config v5 and UI state are data\config.toml and data\ui.toml beneath
the executable root. Installed MSIX mode uses per-user package LocalState.
Registry ConfigV5/UiV4 values are one-time migration input only. The portable
Registry .spl association is intentional shell integration, not settings
authority.

Generated binaries, package layouts, certificates, local data, credentials,
cache, logs, history, and QA captures do not belong in source control.
