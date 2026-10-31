# Windows source/binary baseline reconciliation (2026-10-18)

> **Historical reconciliation record.** Local paths, branches, binaries, and
> validation counts below describe the 2026-10-18 investigation only. They are
> not current installation or build instructions.

The Windows source in the **local `dev` working tree** was reconciled to the locally retained SPLINED baseline executable. The later, unaccepted thumbnail-control and direct MusicBrainz-preview-write changes were reversed. This is a source baseline, not a new feature release.

## What changed

The baseline reconstruction was applied to `windows/config.example.toml`, `windows/ui/app.slint`, and the Windows Rust modules `application.rs`, `compilation.rs`, `config.rs`, `final_artwork.rs`, `gui_events.rs`, `main.rs`, `media_database.rs`, and `scan_runtime.rs`. After the test-only escape normalization noted below, nine of these files differ from the pre-reconciliation snapshot; `compilation.rs` has the same bytes as that snapshot. No installed executable or music file was changed.

The complete pre-reconciliation source snapshot is preserved at `E:\apps\ChatGpt\S.P.L.I.N.E.D-source-before-baseline-reconciliation-20261018`. These source and documentation edits are **uncommitted, local changes on `dev`**; they have not been pushed or merged to `main`. The note itself is documentation and does not perform any file operation.

## Validation

- Baseline executable SHA-256: `918CACD0185AA1644D5A503D2EEEE711AB5C49F8D419344335C512246FFFF753`
- Rebuilt executable SHA-256: `057E23B91B2A350EE8BC7F2A6A4AD472DB52AE6C5C68EEE1E62B1ACDAA77D024`
- Both executables are 41,701,376 bytes. Their `.text`, `.data`, `.pdata`, `.rsrc`, and `.reloc` sections are byte-for-byte identical.
- The 377 differing bytes are confined to the PE build timestamp (3 bytes), source-location line records in `.rdata` (349 bytes), repeated build timestamps (9 bytes), and the PDB identifier (16 bytes). No machine-code or resource bytes differ.
- `cargo check --manifest-path windows/Cargo.toml --bin splined --quiet` passed.
- `cargo test --manifest-path windows/Cargo.toml --quiet` passed: 391 core tests and 26 UI tests.
- `cargo fmt --manifest-path windows/Cargo.toml --check` passed.
- `cargo clippy --manifest-path windows/Cargo.toml --all-targets -- -D warnings` passed after normalizing six test-only string escapes from `\02` to the equivalent `\x002`. This does not change the release executable.
- The root-level Windows UI source contract was updated to describe the baseline's `Existing` action, two MusicBrainz page buttons, TV-shaped placeholder image, and Artist Art status text. The full root `cargo test --quiet` suite then passed: 224 tests.
- Python's declared requirements and local `splined-pyratatui-input` extension were installed into a separate test directory outside the repository. `unittest discover` passed: 272 tests, one skipped.
- `git diff --check` reported no whitespace errors.

The raw SHA-256 values cannot match on a fresh Windows build because the PE header and debug records include build-specific values. The section comparison above is the acceptance check for equivalent compiled code and assets. It does not imply that the source text has identical line numbering to the original build.

## Integration boundary

The installed executable and retained baseline executable were not replaced. The project release-output executable was returned to the baseline bytes after preserving the validated rebuild separately. No commit, push, pull request, or merge was performed. Review and integrate the `dev` working-tree changes into `main` through the normal Git workflow only after confirming the intended source and documentation diff; the working tree also contains other pre-existing, uncommitted changes.
