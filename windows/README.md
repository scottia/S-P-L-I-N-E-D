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
the build script emits the two permanent executable artifacts plus the
separately published temporary update helper:

```text
windows/target/release/splined.exe
windows/target/release/splined-core.exe
windows/target/release/splined-update.exe
```

`splined.exe` is the C# Windows Forms GUI, including its watermark and icon
resources. `splined-core.exe` is the Rust processing worker used for snapshots,
Album scans, and direct core diagnostics. The build-only
`splined-update.exe` output is published separately for approved updates and is
not a permanent archive component.

Build and validate from the repository root:

```text
cargo fmt --manifest-path windows/Cargo.toml -- --check
cargo check --manifest-path windows/Cargo.toml --locked
cargo test --manifest-path windows/Cargo.toml --locked
cargo clippy --manifest-path windows/Cargo.toml --all-targets --locked -- -D warnings
cargo build --manifest-path windows/Cargo.toml --locked --release
```

`gui/TEST-WINDOWS-GUI.cmd` runs the WinForms Config v5 and lifecycle regression
suite. The release archive places only `splined.exe` at its root and ships the
fixed worker as `runtime\splined-core.exe`. The GUI always invokes that worker;
it never re-invokes itself as a worker.
Normal startup and scanning never extract, generate, rename, replace, or delete
an executable. Generated build/QA outputs, local settings, credentials, cache,
logs, and database files are intentionally excluded from version control.

Windows stores the validated Config v5 document in `data\config.toml` and
interface preferences in `data\ui.toml` beneath the portable root. First run requires the
library, SQL database, temporary run cache, log, and credential paths and creates the selected runtime
directories only after save. Database, run-cache, log, and credential fields initially point
beneath `%LOCALAPPDATA%\SPLINED`, remain editable, and do not change existing
saved or UNC paths. The release archive does not contain `data`, `_cache`,
`_logs`, `credentials`, or `docker_builds`; first save creates the portable
settings files. **File > Backup** exports and restores selected portable
settings, interface state, credential JSON, SQLite, and diagnostics in an
optionally password-protected `.spl` container. Restoring the same selected
categories into independently clean portable folders is path-deterministic:
Config/UI output is equivalent, absolute and UNC resources are unchanged, and
unselected categories retain their pre-restore contents.

The standalone worker uses that same `data\config.toml` default when it is
invoked directly; it does not create or treat `config\config.toml` as a second
Windows settings authority.

The GUI serializes the validated Config v5 record directly into each Rust
child process's private environment. Snapshot and Album runs do not create a
runtime TOML. Configuration-load failures exit nonzero so the GUI can
surface the actual error instead of attempting to parse empty snapshot output.

When both portable settings files are absent, the GUI performs a one-time copy
from the legacy per-path HKCU `ConfigV5`/`UiV4` values, records completion, and
leaves the legacy values intact. The Registry is never ongoing Config v5 or UI
authority; moving the complete folder carries settings and deleting it cannot
resurrect a completed migration.

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

`output.upscale_adaptive_defaults` keeps the bounded analyze-first correction.
The optional `upscale_picture_percent`, `upscale_sharpen_percent`,
`upscale_softness_percent`, `upscale_contrast_percent`,
`upscale_exposure_percent`, `upscale_brightness_percent`,
`upscale_gamma_percent`, and `upscale_color_temperature` values form one saved
advanced profile shared by Settings and the Artwork Filter. Zero leaves that
property unchanged. These values are applied during an eligible enlargement
below Ideal or an explicitly previewed/adjusted manual candidate edit. Manual
editing is available for any cached provider or local candidate, but it does
not change Preferred/Auto eligibility, validation, source policy, or Maximum
Upscale safeguards. Automatic processing never downscales or modifies Ideal
and higher-resolution artwork.

The Artwork Filter opens in the Scan Activity workspace beside Selected Album
Artwork rather than over candidate thumbnails. Its four framed groups keep compact
top-aligned rows instead of stretching their contents across the workspace:
Candidate Findings, Source Selection, and Resolution sit above one full-width
Upscale / Advanced group. Each group and each Advanced control uses the same
multicolor spectrum-framed treatment, headings,
padding, and row spacing as Select Media. Zero-count Image Type, Policy, Wanted,
and Unwanted choices stay visible but disabled, so categories do not disappear
between Albums. Upscale Preview and Upscale Show Full are compact fixed-width
actions beside the 1:1 source-to-edit resolution. Show Full opens the current
upscaled and edited image at actual pixels in a scrollable resizable window.
Its eight profile controls use one contained row of equal-width spectrum-framed
vertical adjustments; each frame centers the name and symbol, live value above
the slider, and one square reset button below it. The filter and Advanced region
fill their workspace without nested scrollbars. Opening the filter temporarily expands
the Activity pane, then restores the previous divider when it closes. Existing local `cover.*` candidates remain
previewable and editable at any resolution without resetting Album status.
Selecting an Album exposes its local cover in Artwork Filter immediately,
without provider discovery. **Save Existing** applies the previewed profile
through the native safe-write pipeline in Write mode. Manual review keeps an
Ideal local cover in candidate results; Auto mode retains fast preflight.
Selecting one candidate immediately focuses that exact image in Selected Album
Artwork for editing; multiple selections retain Compare behavior. Alternate
MusicBrainz-release candidates keep Use Selected enabled while the replacement
decision is handed back to the scanner, so the chosen artwork can be saved.
Candidate hover changes only the Artwork preview and is exit-debounced to avoid
flickering or rebuilding the filter controls. Completing Use Selected or Save
Existing closes temporary MusicBrainz/filter workspaces and restores the final
Album report. The Media Library Selection title uses a hollow `«` control at
panel-title size. It reduces the selector to a narrow spectrum rail whose `≫`
control restores it; **View > Show Media Album Selector** remains available.

The Windows snapshot preserves the compilation marker. Pending compilation
track-art work remains LAUNCH-eligible until `embedded-compilation` completion
is recorded, including when older Album-level processed or bypass history is
present. This eligibility change only enters the existing per-track embedded
art workflow; it does not replace folder-level artwork.

Candidate image bytes are cached in RAM after their first read, with a fixed
256 MB FIFO limit, so thumbnails and repeated previews do not reread the same
Temporary Run Cache file from NAS. The cache is cleared on application exit and
never contains or copies `splined.db`.

Per-track release validation reads MP3 authority directly from the leading
ID3v2 block. It does not seek to ID3v1, Lyrics3, APE, audio properties, or
embedded artwork at the end of every MP3. Other formats use Lofty with audio
properties and cover art disabled. This retains the all-track authority audit
without the repeated end-of-file SMB round trips that delayed large box sets.

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

The GUI discovers the newest official, non-prerelease release containing
`windows-update.json`, `splined-windows-x86_64.zip`, and the separate
`splined-update.exe` asset. Backward-compatible schema 2 metadata binds the release URL and commit
to the sizes and SHA-256 digests of the archive, temporary updater, GUI, and
core. Releases for only another operating system are skipped.

GUI-to-helper arguments use Windows-correct trailing-backslash quoting. The
helper retains a narrowly parsed fallback for the exact ordered handoff from
earlier automatic-update GUI builds, so an installed affected build can update
through a corrected next-patch helper; extra or reordered arguments are not
accepted.

Notification validation intentionally accepts older schema 2 documents that
contain only the established release identity. Automatic-install validation is
separate and requires exact version/commit, archive/updater URLs, all four
sizes, and all four SHA-256 digests. Missing integrity fields never receive
defaults; such a release remains notification/open-page only.

Only explicit user approval enters the executable-update path. The GUI
downloads both release assets into the system temporary directory under the
stable updater filename, verifies them, and launches the updater with a visible
window plus the exact GUI/core PIDs. The helper stages the approved archive,
verifies the GUI/core pair, preserves recovery copies, performs transactional
replacement with rollback, verifies the installed pair, and restarts the GUI.
The restarted GUI verifies its release commit and both installed hashes before
removing the updater and staging directory. The helper is never embedded in or
extracted from `splined.exe`, never uses a randomized executable filename, and
uses no CMD, PowerShell, hidden window, or self-deletion loop. Portable Config v5/UI state,
credentials, cache/SQLite/history, and logs are outside the replacement targets.
