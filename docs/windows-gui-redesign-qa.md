# Windows Rust/Slint functional UI QA

This record covers the functional Slint validation surface introduced by the
Windows native migration. It is not the later graphical canvas redesign.

## Automated gates

- `cargo fmt --manifest-path windows/Cargo.toml -- --check`
- `cargo check --locked --manifest-path windows/Cargo.toml --all-targets`
- `cargo clippy --locked --manifest-path windows/Cargo.toml --all-targets -- -D warnings`
- `cargo test --locked --manifest-path windows/Cargo.toml --lib`
- root architecture and release-automation tests
- x64 release build
- `splined.exe --native-smoke`
- `splined.exe --write-build-info <file>`
- portable allowlist audit
- Store MSIX unpack/identity/PE audit
- installed development-package LocalState smoke

Architecture tests additionally fail if an active `.cs` source, managed GUI
compiler path, application DLL bridge, or extra Windows package runtime appears.

## Interactive gate

Run against an existing SPLINED state root without modifying the database:

1. Confirm Artists, Albums, and Folders populate from `splined.db`.
2. Confirm text and each status filter change visibility only.
3. Confirm direct album, additive album, artist cascade, All, Filtered, and None.
4. Launch one intentionally selected album in READ and review real candidates.
5. Exercise source/range filtering, candidate selection, Keep Local, Skip,
   MusicBrainz authority/results, and return to candidate review.
6. Verify STOP releases both active processing and a decision wait.
7. Verify current/local artwork and the existing-cover preview path.
8. Use a controlled album for an intentional LIVE WRITE validation; confirm the
   application remains usable and SQLite reflects the final outcome.
9. Verify Config v5, provider credential test/save, `.spl` backup/restore, and
   channel-correct update check.

Do not use a fixture-only run as the final functional acceptance. Any LIVE WRITE
test must be explicitly selected so unrelated media is never changed.
