# Windows v4 Build Notes

These notes summarize the Windows v4 interface/source generation. They are not
an application-version declaration: the Windows application currently reports
`3.0.0 Stable`, while all supported runtimes use Config v5.

## Highlights

- Moved Windows Config v5 and interface preferences into internal per-user
  settings.
- Added an in-memory GUI-to-Rust configuration handoff with no runtime TOML.
- Added a unified MusicBrainz matching and artwork-review workflow.
- Added curated-compilation processing with resumable per-track embedded art.
- Added selective, optionally password-protected `.spl` backup and restore.
- Refined layout, menus, themes, panel framing, selection, Launch, and preview.
- Preserved shared SQLite compatibility, durable writes, and deterministic
  source/ranking behavior.

## Portable Windows model

- The release archive contains one replaceable `splined.exe`.
- Extraction creates no `config`, `credentials`, `_cache`, `_logs`,
  `docker_builds`, `config.toml`, `ui.toml`, or `config.location`.
- First-run SQL database, temporary run cache, log, and credential values default beneath
  `%LOCALAPPDATA%\SPLINED`, remain editable, and are created only after save.
- Existing local, mapped, and UNC paths are not migrated automatically.
- The fingerprinted GUI shell is privately cached beneath
  `%LOCALAPPDATA%\SPLINED\runtime` to avoid repeated extraction and Windows
  security scanning; stale older shells are pruned on start.
- Internal Config v5 is passed to the Rust child process in memory and is not
  written into the portable folder or user profile as a handoff file.
- Downloaded and derived candidate images are removed when the run completes;
  `splined.db` remains isolated in the SQL Database Directory.

## MusicBrainz matching

- Added an interactive MusicBrainz Matches workspace for normal Albums and
  curated compilations.
- Grouped results by newest-to-oldest decade and populated official release
  type.
- Displayed Artist, country, date, type, release, inspected resolution, and
  preview/authority links.
- Cached per-release source results for the active Album so returning to a
  green current or blue inspected match does not repeat provider discovery.
- Added session-only Artist, Release, and Recording MBID correction fields with
  UUID and relationship validation.
- Fixed Artist MBID arrays displaying as `System.Collections.ArrayList` and
  gave decade/release category rows a distinct magenta role.
- Used exact MusicBrainz Apple Music/iTunes relationships before ordinary
  Artist/Album iTunes searches when available.

## Curated compilations

Albums with `compilation=1` and no Album/Release MBID use a dedicated per-track
output path:

- local Recording-ID/Artist-ID and indexed-Album evidence is checked first;
- bounded MusicBrainz work occurs only after a local miss;
- the strict automatic authority order is Album, Soundtrack, Compilation;
- the operator can inspect broader Artist/Track results;
- ordinary configured artwork providers and Range policy evaluate each chosen
  release;
- Live Write replaces only the approved track's embedded front artwork;
- no folder-level `cover.*` is created, changed, or removed;
- progress is committed per track and incomplete Albums remain resumable;
- unresolved, timeout, or upstream-error tracks remain unchanged.

## Backup and restore

**File > Backup** can export or import selected sections:

- internal Config v5;
- interface state and panel layout;
- credentials;
- `splined.db`;
- diagnostics.

Sections are independent. Exports include integrity protection and may use a
password. Opening an associated `.spl` file enters the selective restore dialog
and does not restore data without confirmation.

## Interface and design

- Added the multicolor `S:P:L:I:N:E:D` wordmark and spectrum panel framing.
- Enlarged and aligned the wordmark, and applied matching spectrum frames to
  Select Media, Album information, and the shared Artwork pane.
- Reworked Light appearance around a warm cream palette.
- Removed excess menu spacing and the duplicate Select Media Launch control.
- Kept one primary LAUNCH/STOP action beneath Artwork Candidates and Preview.
- Added Balanced, Wider Select Media, Wider Decisions, Stacked, and custom
  draggable layouts.
- Contained Media Library Selection scrolling inside its spectrum frame and
  kept all three work areas independently scrollable in Stacked layout.
- Consolidated Artist/Album filters, selection scope, scan scope, launch mode,
  and Folder Status in Select Media.
- Added live selection and seven-category Folder Status counts, separating
  Incomplete Albums from Artists that contain bypasses.
- Matched Python Auto Scan scope: `[SELECTED]` queues explicit selections;
  `[ALL]` queues every Unprocessed Album plus explicit selections.
- Made Auto Scan opt-in and limited unattended acceptance to a
  policy-qualified `Ideal` candidate. Non-Ideal results return to operator
  review, and ordinary Select modes never silently enable Auto Scan.
- Clarified that `Ideal` is a resolution/geometry/policy classification, not a
  visual guarantee that the provider returned the correct front cover.
- Matched Python bulk selection: active-Artist Unprocessed Albums for ALL,
  clear for NONE, and required text-filter scope for FILTERED.
- Added a persisted **View > Show Artwork** surface beside Album/activity
  information. Selected Albums show SQLite-indexed cover, year, resolution,
  track count, status, path, and file counts without provider or inventory work.
- Kept Album information synchronized with the active processing Album or the
  manually focused Album, cleared stale context, and colored text by status.
- Ordered candidate cards with `[LOCAL]` first, the recommended result second,
  and remaining results by descending resolution. Added candidate-only purple,
  green, and clear glass surfaces with square responsive thumbnails.
- Routed enabled candidate and URL hover into the embedded Artwork surface,
  while retaining the floating preview whenever Show Artwork is disabled.
- Integrated MusicBrainz Matches into the main Scan Activity workspace instead
  of a blocking dialog. Its artwork `[URL]` previews release-group front art in
  the shared Artwork panel regardless of the ordinary hover switch, falls back
  to exact-release art, reports preview dimensions beneath the image, and
  restores Scan Activity plus the selected Album cover when review ends.
- Kept the Artwork surface square as the Activity/Candidate divider moves and
  constrained downward travel when the minimum Album/activity width is reached.
- Replaced checkbox/checkmark chrome with solid-green-on/red-outline-off
  state controls throughout the picker, candidate, Settings, credential, and
  Backup interfaces.

## SQLite and reliability

- SQLite remains the only Album/status authority; an arbitrary `cover.*` does
  not mark an Album Processed.
- Shared databases retain rollback journal, FULL synchronization, and bounded
  busy handling.
- Windows consumes Python-owned shared inventory while maintaining compatible
  runtime-status and compilation-ledger updates.
- Added safe embedded-artwork replacement and resumable compilation ledgers.
- Added Unicode normalization and legacy-decoded SMB directory translation.
- Added compact snapshot, provider, candidate, and post-cover timing records.
- Preserved detailed provider notes in the final Album report instead of
  replacing them with only a count.
- Stopped preparing or touching the review-samples directory when review
  samples are disabled.
- Removed successful-update `.splined-backup-*` files after Windows releases
  the prior executable.
- Configuration failures now exit nonzero; empty core output is no longer
  reported merely as an incompatible snapshot.
- Logs centrally redact credential, token, password, authorization, and private
  values.

## Validation baseline

The Windows v4 source is validated with:

```text
cargo fmt --manifest-path windows/Cargo.toml -- --check
cargo test --manifest-path windows/Cargo.toml --locked
cargo clippy --manifest-path windows/Cargo.toml --all-targets --locked -- -D warnings
cargo build --manifest-path windows/Cargo.toml --locked --release
windows\gui\TEST-WINDOWS-GUI.cmd
```

The shared fixture `fixtures/cross-runtime-album-policy.json` is consumed by
the Rust and Python policy suites to keep missing-Album-ID eligibility,
automatic release types, and folder-versus-embedded output targets aligned.
