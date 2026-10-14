# Windows Rust/Slint architecture checklist

This checklist describes the supported Windows production architecture after
the native migration. Git history preserves the retired implementation; it is
not a current build or runtime option.

## Production architecture

- [x] Cargo builds one native `splined.exe`.
- [x] Slint is the only Windows presentation layer.
- [x] Rust owns configuration, SQLite, library state, scan control, candidates,
      MusicBrainz decisions, artwork editing/upscale, credentials, backup, and
      update services in process.
- [x] There are no tracked production `.cs` files, managed GUI entry point,
      `MainForm`, `NativeCore`, C# compiler invocation, Windows Forms dependency,
      application bridge DLL, or worker executable.
- [x] Portable and Store artifacts use the same x64 Rust executable.

## Configuration and first run

- [x] Config v5 is the sole application-configuration authority.
- [x] Portable authority is
      `<splined.exe directory>\data\config.toml`; UI state is the adjacent
      `data\ui.toml`.
- [x] Packaged authority is
      `%LOCALAPPDATA%\Packages\<package-family>\LocalState\SPLINED\data\config.toml`;
      UI state is the adjacent `ui.toml`.
- [x] Startup discovers these files automatically and does not ask the operator
      to select a config file.
- [x] Relative paths resolve against the active state root; absolute, mapped,
      extended, and UNC paths remain unchanged.
- [x] No Windows Registry value or migration marker is active configuration
      authority.
- [x] Missing, invalid, or operationally incomplete Config v5 opens structured
      Setup and prevents the ordinary workspace from presenting as ready.
- [x] Pre-save first run creates no `data`, database, temporary cache, log,
      credential, or obsolete bootstrap directories.
- [x] Explicit **Save and Continue** validates the Rust Config model, creates
      only the configured application-owned directories, atomically writes
      Config v5, writes UI state separately, and then loads the database.
- [x] Normal Settings uses structured Slint controls backed directly by the
      Rust Config model; raw TOML is not the normal workflow.
- [x] Structured Settings preserves scan, timeout, SQLite sharing, ignored
      directories, source priority/exclusions/policies, artwork ranges, output
      and square/upscale behavior, adjustment defaults, provider options, and
      retention fields supported by Config v5.

## Persistent state and diagnostics

- [x] `splined.db` is derived only from `<scan.cache_dir>\splined.db`.
- [x] Existing Config v5 and database state loads automatically into Artists,
      Albums, and Folders without a secondary GUI inventory.
- [x] Startup diagnostics report active state root, Config path, SQLite path,
      and loaded Album/Artist/Folder counts without secrets.
- [x] Provider credentials are read only from the configured credential
      directory and are never copied into Config v5, SQLite, or logs.
- [x] Navidrome remains a supplemental OpenSubsonic source; a visible connected
      status requires a real live request and `splined.db` remains authority.
- [x] Config/UI/credential/database backup and restore retain separate
      categories and atomic writes.
- [x] Portable `.spl` shell opening imports unprotected Config v5 settings
      before startup; protected archives fall back to the visible restore
      surface with their path preselected.
- [x] Setup exposes the optional shared/network SQLite choice and preserves
      the remaining loaded Config v5 model when correcting a database path or
      ownership mismatch.

## Workflow behavior retained

- [x] Artist, Album, and Folder views project the resident SQLite model.
- [x] Selection, bypass/timeout protection, compilation eligibility, READ/LIVE
      WRITE queues, sequential recovery, and STOP are Rust-owned state.
- [x] Candidate filtering, source order, Keep Local/Apply/Bypass/Skip,
      MusicBrainz authority, editing/upscale, and current-cover editing invoke
      the native Rust services.
- [x] Source order saves to Config v5; no widget-local provider authority exists.
- [x] Portable update notification and native Store update handling retain their
      separate supported channels.
- [x] Native Slint dividers resize the Media Library, central workspace,
      Artwork Preview & Editing, candidate/results, and activity regions.
- [x] Rust owns pane bounds, previous expanded dimensions, normal window size,
      maximized state, and saved-layout clamping; `data\ui.toml` persists these
      presentation preferences independently of Config v5.
- [x] Collapsed panes reclaim their space and restore their previous useful size
      when expanded, subject to the current window's minimum usable dimensions.

The workflow-level comparison with the final mature WinForms behavior is in
[Windows 1.0.88 functional-parity audit](windows-functional-parity-1.0.88.md).
That pass restores track navigation, multi-status and filtered selection,
tri-state candidate filters, comparison/hover/fallback, MusicBrainz history and
edited authority, Custom Upscale, primary-adjustment selection, persisted
collapsible regions, and run statistics without restoring managed code.

## Validation gates

- [x] `cargo fmt --check` for the Windows crate.
- [x] `cargo check --locked --all-targets` for the Windows crate.
- [x] `cargo clippy --locked --all-targets -- -D warnings` for the Windows crate.
- [x] Windows Rust/Slint tests, root Rust/architecture tests, and relevant Python
      parity tests pass.
- [x] Existing Config v5 auto-discovery, relative/absolute/UNC preservation,
      SQLite derivation, and complete library counts are regression tested.
- [x] A clean portable smoke creates no state before Setup Save.
- [x] Optimized x64 build, two-file portable packaging validation, and Store
      package validation pass.
- [x] Tracked production C# file count remains zero.

## Configuration-correction evidence

- Windows Rust/Slint: 313 library tests passed.
- Root Rust: 199 library, 3 public-config, 9 release-automation, and 10
  architecture tests passed (221 total).
- Python parity: 271 tests passed with 1 expected skip.
- The optimized x64 Rust/Slint executable auto-discovered a resident portable
  Config v5, opened its configured `splined.db` read-only, and loaded 3,869
  Albums, 1,014 Artists, and 3,869 distinct Folder/path rows.
- The actual Slint model exposed 1,014 Artist, 3,869 Album, and 3,869 Folder
  rows. Artist activation changed selection/scope, Album activation populated
  Current Album, and Select None returned the selection count to zero.
- A live request using the configured Navidrome credential reported library,
  album-artwork, artist-artwork, and path-enrichment capabilities available;
  no secret entered diagnostic output.
- Clean portable and unpacked Store-payload smoke runs created none of `data`,
  `_cache`, `_logs`, `credentials`, or `docker_builds` before Setup Save.
- The two-file portable allowlist and unsigned Store package were validated.
  Store identity remained `Psycotix.SPLINED`, version `1.0.92.0`, x64,
  `Windows.Desktop`, with the `.spl` association and no DLL host artifact.

## Safety invariants

- Never destructively migrate, reset, or silently replace `splined.db`.
- Never silently substitute defaults for a missing or invalid Windows Config v5.
- Never create configured runtime directories before explicit validated Save.
- Never log secrets or write credentials into Config v5 or SQLite.
- Never restore a retired managed Windows production path.
- Do not create a release or tag as part of configuration correction work.
