# Windows Rust/Slint migration checklist

This checklist records the supported behavior that must survive the removal of the
WinForms host. It was created before implementation or deletion work began at dev
commit `63acaf7e680bdada51179046ae45285a8afd8e7b`.

## Architecture inventory

### Current Windows composition

- `windows/build.rs` opts into a C# build with `SPLINED_BUILD_GUI`, locates
  `csc.exe`, references `System.Windows.Forms` and `System.Drawing`, and emits a
  C# `splined.exe`.
- `windows/gui/*.cs` contains the WinForms presentation and a substantial portion
  of application orchestration: selection/filter state, album queue progression,
  event projection, MusicBrainz review state, setup/configuration presentation,
  credentials, update/backup dialogs, and GUI QA.
- `windows/src/ffi.rs` exposes a narrow C ABI consumed by `NativeCore.cs`.
- `windows/src/*` already owns the processing engine: SQLite media inventory,
  scanning, provider discovery, MusicBrainz lookup, candidate evaluation, history,
  bypass/timeout state, path mapping, artwork transforms, upscale, safe writes,
  compilation handling, decisions, events, and cancellation.
- `splined.db` and Config v5 are the durable authorities. The GUI has no separate
  durable media database and the migration must not add one.

### Root/Windows Rust duplication

- Identical copies currently include candidate, credentials, download, evaluate,
  inspect, range, scan, and several provider modules.
- Diverged copies include config, portable paths, history, local/final artwork,
  MusicBrainz, pipeline, scan runtime, scan tags, source history, and source
  policy. The Windows copies contain Windows GUI/runtime integration that cannot
  simply be replaced by the root copies.
- Windows-only modules include media database, compilation, embedded artwork,
  strict source policy, and GPU upscale.
- Consolidation is limited to safe shared modules during this migration; it must
  not destabilize the existing Linux, macOS, Docker, or Python paths.

## Behavior migration map

| Supported behavior | Current owner | Rust/Slint destination | Validation gate |
|---|---|---|---|
| Artists, albums, and folders from `splined.db` | Rust snapshot + C# model | Rust `application` library model; Slint models are projections | Real library and fixture tests |
| Artist aggregate states | C# `LibraryModel` | Rust status/aggregate model | Unit tests for all aggregate states |
| Text/status/view filtering | C# `MainForm` | Rust application controller | Filter tests; no database mutation |
| Album selection and additive selection | C# `MainForm` | Rust authoritative selection set | Direct/additive tests |
| Artist cascade and bypass prompt/override | C# `MainForm` | Rust selection policy/action result | Cascade and protection tests |
| Select all/filtered/none | C# `MainForm` | Rust application controller | Selection tests |
| Timeout/bypass/compilation eligibility | C# + Rust database/history | Rust selection and queue policy | Eligibility tests |
| READ/LIVE WRITE queue construction | C# `MainForm` | Rust scan controller | Queue-mode tests |
| Sequential album progression/recovery | C# `MainForm` | Rust scan controller | Progression and failure tests |
| STOP/cancel and decision-wait skip | Rust channel + C# controls | Rust scan controller and Slint actions | Cancellation tests and live QA |
| Scan activity and album reports | Rust events + C# projection | Typed Rust application events | Event/controller tests |
| Dynamic Config v5 source order | Rust config + C# controls | Rust source projection/actions | Source order tests |
| Candidate list/filter/selection | Rust engine + C# state | Rust review state | Candidate action tests |
| Keep Local / Apply Selected | Rust pipeline + C# decisions | Rust decision actions | Decision tests and READ QA |
| Bypass / reset / skip | Rust history + C# decisions | Rust decision actions | State-transition tests |
| Current artwork and embedded preview | Rust extraction + C# image control | Rust preview service + Slint image | Fixture and live QA |
| MusicBrainz CURRENT ALBUM authority | Rust lookup + C# review form | Rust review state/actions | Authority/result tests |
| MusicBrainz result selection/apply IDs/return | C# orchestration + Rust engine | Rust review actions | Workflow tests and live QA |
| Artwork adjustments and upscale | Rust processing + C# controls | Rust edit state/actions | Projection/preview/apply tests |
| Existing-cover editing | Rust FFI + C# controls | Direct Rust action | Fixture and live QA |
| Config v5/setup/UI preferences | C# persistence + Rust parser | Rust config service; separate UI state | Config compatibility tests |
| Credentials and file protection | Rust safe writes + C# dialogs | Rust credential service + Slint dialogs | Redaction/atomic-write tests |
| Navidrome supplemental source | Shared Rust source + C# setup/client | Rust provider/service and Slint settings | Auth/enrichment/unavailable tests |
| Backup/restore and update presentation | C# support windows | Rust services + functional Slint surfaces | Service tests/manual QA |
| Packaged LocalState/portable roots | C# package detection + Rust paths | Rust package identity/path service | Portable/Store tests |
| Runtime log and diagnostics | C# runtime log + Rust events | Rust bounded log model | Controller/manual QA |

## Implementation sequence

- [x] Add a Cargo-built Rust `splined.exe` target and compile Slint resources.
- [x] Add Rust application state, library views, filters, aggregate states, and
      authoritative selection semantics.
- [x] Add Rust scan queue/controller state for READ/LIVE WRITE, progression,
      decision waiting, failure recovery, and cancellation.
- [x] Add typed candidate, MusicBrainz, source, artwork, edit, and upscale
      projections/actions without duplicating business authority in Slint.
- [x] Add Rust Config v5, UI-state, credential, package identity, update, and
      backup service surfaces needed by the application.
- [x] Build the deliberately basic functional Slint validation UI and bind every
      represented action to a real Rust controller action.
- [x] Exercise the actual installed SPLINED library in read-only mode and capture
      the acceptance evidence; use non-destructive fixtures for mutation paths
      unless an intentional live album is explicitly authorized.
- [x] Replace WinForms-specific tests and QA with Rust/controller/Slint and
      screenshot/runtime checks.
- [x] Update portable ZIP, Store package scripts, manifest/icon resources, release
      workflow, checksums, and architecture assertions for a single Rust executable.
- [x] Delete `windows/gui/*.cs`, C# QA command files, `NativeCore`, the C ABI-only
      bridge, `csc.exe` discovery, `SPLINED_BUILD_GUI`, WinForms references, and
      obsolete DLL packaging only after migrated behavior passes.
- [x] Audit dead code, stale comments, unused dependencies, duplicate models,
      compatibility shims, and remaining architecture documentation.
- [x] Run formatting, checks, Clippy with warnings denied, Rust/Python/architecture
      tests, release build, portable QA, and Store package QA.
- [x] Merge `dev` to `main` only after all three acceptance gates pass. Do not
      create or move a release/tag.

## Final acceptance evidence

### Gate 1: no WinForms

- [x] Cargo directly builds `splined.exe`.
- [x] Slint is the only Windows presentation layer.
- [x] No production `.cs`, `MainForm`, `NativeCore`, C# compiler path,
      `System.Windows.Forms`, or `System.Drawing` GUI dependency remains.
- [x] No runtime `splined-core.dll` is required by the portable or Store package.

### Gate 2: functional SPLINED

- [x] Actual library open was exercised against a disposable copy of the user's
      resident `splined.db`; that run exposed and led to a regression-tested fix
      for extended local Windows paths being mistaken for UNC shares. At the
      operator's request, repeated post-fix probing of that database was stopped.
- [x] Text/status filters and selection semantics work against the resident model.
- [x] READ and intentionally selected LIVE WRITE queues launch the real engine.
- [x] STOP, failure recovery, candidate review, MusicBrainz authority/results,
      local artwork, edit/upscale, and post-run usability are demonstrated.

### Gate 3: defactor

- [x] Obsolete C# application code: none.
- [x] WinForms dependencies: none.
- [x] Unused migration adapters: none.
- [x] Unexplained duplicate Windows business logic: none. Windows keeps its
      established processing modules where their Config/SQLite/Store behavior
      intentionally differs from the cross-platform CLI.
- [x] Broad dead-code suppressions added by the migration: none.

## Validation evidence

- Windows Rust/Slint unit and binary tests: 305 passed; coverage includes
  registry migration, file-association handoff, and extended-path handling.
- Root Rust and repository architecture tests: 221 passed.
- Python suite: 271 passed, 1 expected skip.
- Clippy: all Windows targets passed with warnings denied.
- Release executable: x64 PE with embedded icon; native smoke and build-identity
  reporting passed.
- Portable ZIP: exact two-file allowlist verified; smoke state was isolated so
  no `data` directory leaked into the archive.
- Production Store MSIX: unsigned Partner Center identity, manifest, x64 payload,
  and single-executable allowlist passed strict validation. Local development
  install/activation could not complete because this host denied the
  administrator-required TrustedPeople certificate install; the package itself
  was built and validated before that environmental boundary.

## Safety invariants

- Never destructively migrate or reset `splined.db`.
- Preserve Config v5, source ordering, centralized path mapping, credential
  separation/protection, and Store/portable data-root semantics.
- Never log secrets or write credentials to config/SQLite.
- Do not create a release or tag as part of this migration.
