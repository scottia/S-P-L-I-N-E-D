# Windows Rust/Slint graphical workspace QA

This record covers the package-defined graphical Artwork Workspace implemented
by the native Rust/Slint application. The numbered implementation references
map to real application states rather than independent mock screens:

1. **Find & Match Artwork** — graphical library, ordered source cards, grouped
   tri-state filters, Selected Artwork, and the horizontal candidate strip.
2. **Edit Artwork** — dedicated canvas plus five prominent, reorderable primary
   adjustments; the other supported adjustments remain promotable.
3. **Upscale / Enhance** — current configured upscale behavior, preview/apply
   distinction, before/preview composition, and Custom Upscale entry.
4. **MusicBrainz Results** — integrated authority card, result history,
   refinement, Apply IDs, Open MB Page, and return workflow.
5. **Custom Upscale** — a separate resizable current/upscaled comparison window
   with primary controls and explicit cancel/apply actions.

The compositions use the existing Rust state and callbacks. Source and
candidate data, ratings, dimensions, provider capabilities, and artwork are
never fabricated to fill a reference screen.

## Targeted implementation gates

- `cargo fmt --manifest-path windows/Cargo.toml --all -- --check`
- `cargo check --locked --manifest-path windows/Cargo.toml --all-targets`
- `cargo clippy --locked --manifest-path windows/Cargo.toml --all-targets -- -D warnings`
- focused Rust tests for workspace-layout migration, invalid collapsed media
  geometry recovery, and returning to every full top-level library model
- `splined.exe --slint-library-diagnostics` against the configured database

Architecture tests additionally fail if an active `.cs` source, managed GUI
compiler path, application DLL bridge, or extra Windows package runtime appears.

The current graphical layout revision is stored in `data/ui.toml`. When an
older temporary-shell geometry is encountered, SPLINED replaces only obsolete
pane dimensions with the numbered-workspace defaults. It preserves appearance,
library view/filter state, Config v5, and normal window dimensions. Windows and
Slint native per-monitor DPI remain authoritative; startup never infers an
interface scale solely from the RDP desktop's pixel dimensions.

## Interactive gate

Run against an existing SPLINED state root without modifying the database:

1. Confirm Artists, Albums, and Folders populate from `splined.db`.
   Switching to a top-level tab clears any Artist drill-down scope, so Folders
   always restores the complete physical-folder model.
   The October 2026 real-library regression gate loaded and visibly rendered
   1,014 Artists, 3,869 Albums, and 3,869 Folders.
2. Collapse Media Library and confirm the narrow rail retains a clickable
   expand control. Re-expand it and confirm the populated list appears at a
   usable clamped width before continuing.
3. Confirm text and each status filter change visibility only.
4. Confirm direct album, additive album, artist cascade, All, Filtered, and None.
5. Launch one intentionally selected album in READ and review real candidates.
6. Exercise source/range filtering, candidate selection, Keep Local, Skip,
   MusicBrainz authority/results, and return to candidate review.
7. Verify STOP releases both active processing and a decision wait.
8. Verify current/local artwork and the existing-cover preview path.
9. Use a controlled album for an intentional LIVE WRITE validation; confirm the
   application remains usable and SQLite reflects the final outcome.
10. Verify Config v5, provider credential test/save, `.spl` backup/restore, and
   channel-correct update check.

Do not use a fixture-only run as the final functional acceptance. Any LIVE WRITE
test must be explicitly selected so unrelated media is never changed.

For visual acceptance, capture the actual built Slint application at the five
states above and compare composition, hierarchy, density, control placement,
and interaction surfaces with the package references. A black background-only
window capture is not evidence; repeat capture from an unlocked interactive
desktop. Do not claim pixel or visual parity from source inspection alone.
