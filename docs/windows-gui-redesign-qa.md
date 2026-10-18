# Windows Rust/Slint graphical workspace QA

This record covers the package-defined graphical Artwork Workspace implemented
by the native Rust/Slint application. The numbered implementation references
map to real application states rather than independent mock screens:

1. **Find & Match Artwork** — graphical library, ordered source cards, grouped
   tri-state filters, Selected Artwork, and the horizontal candidate strip.
2. **Edit Artwork** — dedicated canvas plus all eight independently selectable,
   reorderable adjustments and one Activate All action.
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

- `cargo fmt --check`
- `cargo check --manifest-path windows/Cargo.toml --locked --all-targets`
- `cargo clippy --manifest-path windows/Cargo.toml --locked --all-targets -- -D warnings`
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
interface scale solely from the RDP desktop's pixel dimensions. The main,
Custom Upscale, and comparison windows each follow their current monitor's DPI
and work area. Reflow and splitter clamping consume Slint logical dimensions,
including after a window crosses monitors; no global `SLINT_SCALE_FACTOR` is
set. Saved main-window maximize is dispatched after the Slint event loop starts,
then a settled-size reflow follows once Windows finishes per-monitor DPI
placement. The Media Library always opens expanded and LAUNCH restores it if
necessary. Secondary windows never inherit that state and retain independent
native maximize/restore behavior.

## Interactive gate

Run against an existing SPLINED state root without modifying the database:

1. Confirm Artists and Albums populate from `splined.db`. Artist rows contain
   Album names on line two; Album rows contain Artist names on line two; no
   filesystem path or third status line is rendered. Confirm Album names are
   yellow, Artist secondary/status text retains its semantic library color, and
   both respond to the Media Filter's status controls. Activate an Artist and confirm its selectable Album radio rows
   expand directly underneath it while the Artists tab remains active. Confirm
   Fallback replaces the old Folders tab, lists each Album without a
   MusicBrainz Album ID once,
   and renders tracks only after that Album is opened, without a checkbox.
   The October 2026 real-library regression gate loaded 1,014 Artists and 3,869
   Albums. The internal 3,869-folder projection remains covered by diagnostics
   for database compatibility but is no longer a top-level UI tab.
2. Collapse Media Library and confirm the narrow rail retains a clickable
   expand control. Re-expand it and confirm the populated list appears at a
   usable clamped width before continuing.
   Repeat this gate in Custom Upscale and verify its shared sidebar exposes the
   same Artists, Albums, Fallback tracks, selection, and status models.
   The read-only Slint diagnostic must report `media_collapsed=true`,
   `media_expanded=true`, `restored_media_width_at_least_min=true`,
   `media_model_restored=true`, `artist_click_kept_view=true`,
   `artist_inline_album_rows=true`, `fallback_album_enables_launch=true`, and
   `fallback_album_queues_whole_album=true`.
3. Open Media Filter and confirm separate Artist and Album search bars, Select
   Modes (All, Filtered, None), and Library Status controls. Confirm active
   radios are green and deselected radios are hollow red; no checkmark control
   remains anywhere in the application. Resize the sidebar and confirm the
   three Select Mode buttons remain equal width. Confirm the filter stays
   clipped/scrollable without covering library rows and no explicit UI text is
   smaller than 13 pixels.
4. Confirm Navidrome Artist/Album thumbnails populate from the local
   `data/navidrome-thumbnails` cache, a selected Artist retries missing art,
   and a processed Album updates its thumb.
5. Confirm direct album, additive album, inline Artist expansion and child Album
   selection, All, Filtered, and None.
6. Launch one intentionally selected album in READ and review real candidates.
   Confirm the one top button transitions LAUNCH → PROCESSING → SKIP ALBUM at
   the appropriate runtime phases and that clicking PROCESSING requests stop.
7. Exercise source/range filtering, candidate selection, Bypass, MusicBrainz
   authority/results, and return to candidate review. Confirm Bypass is orange,
   becomes red Bypassed only after the durable outcome, and asks before clearing.
8. Resize every divider and expand/collapse every region while maximized. The
   app must remain maximized, the artwork pane must expand beyond the former
   720-pixel cap, and Selected Artwork must remain square.
9. Verify current/local artwork and the existing-cover preview path.
10. Use a controlled album for an intentional LIVE WRITE validation; confirm the
   application remains usable and SQLite reflects the final outcome.
11. Verify Config v5, provider credential test/save, separate `.spl` settings
   and `.sqlite` database restore, `.json` diagnostics, independent schedules,
   and channel-correct update check.
12. Move the main, Custom Upscale, and comparison windows between monitors with
    different Windows scale settings. Confirm each reflows at the destination
    monitor's DPI, and that maximize/restore uses that monitor's work area.

Do not use a fixture-only run as the final functional acceptance. Any LIVE WRITE
test must be explicitly selected so unrelated media is never changed.

For visual acceptance, capture the actual built Slint application at the five
states above and compare composition, hierarchy, density, control placement,
and interaction surfaces with the package references. A black background-only
window capture is not evidence; repeat capture from an unlocked interactive
desktop. Do not claim pixel or visual parity from source inspection alone.
