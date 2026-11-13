# Catalog Index Coordinator Acceptance Record

PR #170 is validated one requirement at a time. A requirement is marked passed
only after focused tests, real SQLite/PostgreSQL integration, affected full
suites, and the PR checkpoint CI are green. Windows and WebGUI retain separate
implementations and independent database leases/generations.

## Gate 0 — established regression baseline

Status: **PASSED** at `a2f3d4f`.

- Restored immediate service-level change detection while keeping browser
  filesystem inspection off the request thread.
- Preserved Windows-owned Album protection, embedded-artwork read-failure
  handling, deferred Album discovery, and unchanged row preservation.
- WebGUI: 369 tests passed at the Gate 0 checkpoint.

## Cross-client regression gate — identity history and projected Artists

Status: **PASSED** at `9461c1f` (Album coordination projection) and `d6608f5`
(projected Artist isolation).

This gate is mandatory for subsequent PR #170 checkpoints because coordinator
publication and incremental indexing can change a client's current Album key
without changing the physical Album or its already durable artwork-write
history.

### Coordination identity-history contract

- Coordination history is provenance, not a duplicate Album catalog. Multiple
  historical keys may refer to one stable library-relative path after an MBID
  correction or independently timed client refresh.
- Each active catalog must still contain only one current row for that physical
  Album. Historical coordination rows are retained and replayed idempotently.
- Windows and WebGUI both project the newest event matching either the current
  exact key or normalized relative path onto the current catalog row. Ordering
  is `(source_completed_ms, event_id)`; display names never participate.
- The source client remains normal and the opposing client renders OVERRIDE.
  Projection changes presentation only and cannot create, merge, delete or
  re-key an Album.
- Artist projection remains narrower than Album coordination. A track-credit
  Artist receives only Albums containing that credit and cannot alias into an
  Album owner's complete catalog through a colliding MBID.

### Permanent fixtures and automated evidence

- `American Pie 2`: SQLite and PostgreSQL retain one current Album each at the
  same `[Soundtracks]/American Pie 2` path while intentionally carrying
  different MBIDs. The older Windows event and newer WebGUI event remain in
  both coordination stores. Windows projects the newer WebGUI write and shows
  OVERRIDE; WebGUI remains the local writer.
- `A$AP Rocky`: both clients show exactly one featured Album, Alicia Keys'
  `Here`; no other Alicia Keys Album is associated with the projected Artist.
- Windows focused regression:
  `catalog_projection_prefers_newer_same_path_event_after_album_key_changes`.
- WebGUI real-PostgreSQL regression:
  `test_album_write_coordination_projects_newer_relative_path_identity`.
- WebGUI Artist-projection regression:
  `test_projected_featured_artist_does_not_merge_into_album_owner`.

### Installed retest — 2026-11-11

- Windows 2.0.36 test build from the PR #174 integration head rendered
  `American Pie 2` with the OVERRIDE banner/red frame and counted four opposing
  overrides in the 3 Doors Down overview. Selecting `A$AP Rocky` displayed only
  `Here` (one Album).
- WebGUI at `http://127.0.0.1:8080` kept `American Pie 2` as the local writer
  and loaded `A$AP Rocky · 1 albums`; the only card and file-list child was the
  featured `Here` release.
- Read-only database inspection found one active Album row per catalog, no
  duplicate physical path, and the ordered older-Windows/newer-WebGUI identity
  history in both coordination stores. No reset or reindex was performed.
- The three focused tests above passed against the installed source and the
  established local PostgreSQL service. The Windows full suite immediately
  preceding this retest passed 527 core plus 112 application tests.

Future test builds that change indexing, publication, identity normalization,
reconciliation or coordination must repeat this gate and record the installed
revision and active database targets. It cannot be waived by a green general CI
run.

## Requirement 1 — streaming inventory and bounded batches

Status: **PASSED** at `0be0654`.

Acceptance contract:

- Yield the first Album before the remaining library is enumerated.
- Retain only the current Album and a configured 1–64 Album batch.
- Commit every batch to durable generation checkpoints and release the batch
  payload before continuing.
- Keep the last published catalog authoritative while staging.
- Recover an expired worker from committed checkpoints without publishing a
  partial catalog.
- An unchanged incremental pass must not reopen audio tags.

Implementation:

- Windows streams directories through a callback, commits bounded SQLite/WAL
  checkpoint batches, clears each batch after commit, and publishes only after
  generation validation.
- WebGUI streams the filesystem iterator, commits bounded PostgreSQL staging
  rows, clears its batch-local identity map after commit, and publishes only
  after generation validation.
- `scan.index_batch_size` is an advanced setting constrained to 1–64. The
  current default is 16 for both clients; the implementations remain separate
  and may use different defaults after larger benchmarks justify that change.
- SPLINED-owned verified media writes advance only their affected manifest row,
  preventing the change detector from reporting the application’s own write.

### Automated evidence

- Windows full Rust suite: **515 passed**.
- Shared Rust suite: **201 passed**.
- WebGUI full suite with real disposable PostgreSQL schemas: **374 passed**.
- WebGUI 1,000-Album streaming fixture: complete, unique, first-yield lazy, and
  under the 8 MiB Python allocation ceiling.
- Windows 130-Album SQLite fixture: first durable checkpoint at Album 16,
  16 staged rows and 0 live rows before the next batch, then 130 exact Albums
  and Tracks after publication.
- PostgreSQL 8-Album fixture at batch 3: durable stage counts 3/6/8, live Album
  counts 1/1/1 until publication, batch objects replaced after every commit,
  and 8 exact Albums/9 Tracks after publication.
- Simulated worker loss after the first committed batch passed independently
  for SQLite and PostgreSQL. Both retained the old live catalog, resumed the
  same generation after lease expiry, reused completed checkpoints, and then
  atomically activated the recovered generation.
- Unchanged incremental regression tests record zero Album tag reads and zero
  Track tag reads in both clients.

### Established test-library benchmark

Fixture: `E:\apps\ChatGpt\SPLINED-local-test-library\music` — 216 Albums,
3,496 Tracks. PostgreSQL used disposable schemas on `127.0.0.1:55432`; SQLite
used disposable caches below the established test-library state directory.
Media was read-only. Every run was interrupted after its first durable batch,
verified zero live Albums, expired the abandoned lease, resumed, published,
and then ran an unchanged incremental pass.

Windows SQLite release build (warm filesystem cache):

| Batch | Staged before resume | Total interruption + resume | Resumed checkpoint time | Peak RSS growth | Unchanged incremental | Tag reads on unchanged |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 1 | 4.748 s | 0.142 s / 215 commits | 10.88 MiB | 1.025 s | 0 / 0 |
| 16 | 16 | 4.796 s | 0.046 s / 13 commits | 10.22 MiB | 0.941 s | 0 / 0 |
| 64 | 64 | 4.916 s | 0.031 s / 3 commits | 10.52 MiB | 0.953 s | 0 / 0 |

The first cold-cache batch-1 run took 28.825 seconds; it is retained as the
cold filesystem baseline and is not compared directly with the warm runs.

WebGUI PostgreSQL:

| Batch | Staged before resume | Total interruption + resume | Resumed checkpoint time | Peak RSS growth | Unchanged incremental | Tag reads on unchanged |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 1 | 155.768 s | 17.593 s / 215 commits | 144.88 MiB | 35.705 s | 0 / 0 |
| 16 | 16 | 140.636 s | 1.235 s / 13 commits | 145.74 MiB | 19.546 s | 0 / 0 |
| 64 | 64 | 143.030 s | 0.370 s / 3 commits | 149.08 MiB | 18.659 s | 0 / 0 |

The recorded filesystem/metadata phase for the resumed portion was 82.494,
68.515, and 51.960 seconds for PostgreSQL batches 1, 16, and 64 respectively;
the corresponding Windows values were 4.348, 4.205, and 3.543 seconds. Because
the first committed batch is reused after interruption, these phase values
exclude 1, 16, or 64 completed Albums respectively and are recovery evidence,
not standalone cross-batch throughput comparisons.

Batch 16 is the current measured compromise: it removes most PostgreSQL commit
overhead without the larger recovery window and higher observed memory of batch
64. The 100,000-Track synthetic benchmark remains a final architecture gate and
is not replaced by this established-library checkpoint.

Reproducible harnesses:

- `windows/examples/catalog_index_benchmark.rs`
- `debug/catalog-index-postgres-benchmark.py`

Checkpoint CI: [Validate supported runtimes run 42927892519](https://github.com/scottia/S-P-L-I-N-E-D/actions/runs/42927892519)
passed WebGUI, native Rust, validation selection, and the Windows advisory job.
Installed Windows/WebGUI progress and cancellation behavior belongs to
Requirement 10 and is not claimed here.

## Requirement 2 — persistent index generations

Status: **PASSED** at `e13301b`.

Acceptance contract:

- Persist building progress and completed Album checkpoints outside process
  memory.
- Persist published, cancelled, and failed generation outcomes.
- Preserve the last published catalog when maintenance is cancelled or fails.
- Recover an expired building generation with a new owner and reuse valid
  completed checkpoints after the database/runtime is reopened.
- Reject terminal-state changes from a stale generation owner.

Implementation and defect corrections:

- Windows records its terminal generation transition and maintenance-marker
  restoration in one short SQLite `IMMEDIATE` transaction. Previously, an old
  owner could fail the generation-row update but still clear the recovered
  worker's maintenance marker in a second unguarded write.
- WebGUI records terminal `phase` and clears `current_path` with the terminal
  PostgreSQL state. Previously, a cancelled or failed generation retained the
  misleading `enumerating` phase even though its durable state was terminal.
- SQLite and PostgreSQL continue to use independent generation IDs, ownership
  tokens, leases, and recovery state. No shared cross-database lock or
  authoritative generation is claimed.

### Local acceptance evidence

- Windows Catalog Index Coordinator focused tests: **7 passed**, including
  durable cancelled/failed state after closing and reopening a file-backed
  SQLite database, stale-owner rejection, and published-catalog preservation.
- Windows crash/recovery integration: first committed batch remained staged
  with no live catalog, the SQLite connection was reopened, the same generation
  resumed, and its final state was `published`.
- WebGUI real-PostgreSQL focused tests: durable published/cancelled/failed
  history, stale-owner isolation, and crash/restart recovery passed in
  disposable schemas on `127.0.0.1:55432`.
- The WebGUI recovery test closes and recreates `Database`, `Runtime`, and
  `Services`; previously completed Album paths are asserted not to enter tag
  inspection after restart.
- Windows full Rust suite: **517 passed**.
- Shared Rust suite: **201 passed**.
- WebGUI full suite: **375 passed** in 379.179 seconds, including the rendered
  Edge browser workflow.

Checkpoint CI: [Validate supported runtimes run 42931286109](https://github.com/scottia/S-P-L-I-N-E-D/actions/runs/42931286109)
passed WebGUI, native Rust, validation selection, and the Windows advisory job.

## Requirement 3 — precomputed change manifest

Status: **PASSED** at `95025eb`.

Acceptance contract:

- Read saved change status without walking the library, opening audio tags, or
  inspecting artwork.
- Run physical change detection only as an explicit background probe.
- Detect Album additions, removals, moves, track/tag metadata changes, and
  local `cover.*` changes from bounded metadata manifests.
- Keep a real incremental index request distinct from an active probe job.
- Preserve Windows-owned PostgreSQL Album boundaries and independent
  SQLite/PostgreSQL manifests; no shared lock or generation is claimed.
- Reuse the published catalog on an unchanged startup without reopening audio
  tags.

Implementation and defect corrections:

- Windows stores track inventory plus local artwork size/mtime evidence in its
  SQLite generation manifest. The startup probe runs off the Slint event loop,
  publishes a cached summary for UI reads, and recognizes path moves without
  converting them into separate additions and removals.
- WebGUI `GET /api/library/changes` now reads only the saved PostgreSQL summary.
  An explicit `POST /api/library/check-changes` queues the bounded metadata
  probe, and browser polling reads its durable job state without scanning the
  NAS on an API request thread.
- An `index-probe` job can be deduplicated against other probes but cannot
  absorb or replace a real `index` job.
- Windows previously omitted local artwork metadata from its coordinator
  signature, so replacing `cover.*` could remain classified as unchanged.
  Artwork metadata now participates in both checkpoint validity and move
  matching.

### Acceptance evidence

- Windows focused SQLite tests detect track/tag changes, artwork changes,
  moves, additions, and removals; a cached status read remains unchanged until
  the explicit probe runs.
- WebGUI real-PostgreSQL tests detect the same five change classes in
  disposable schemas on `127.0.0.1:55432`.
- WebGUI API tests prove GET is cached, POST is explicit, probe status is
  durable, and an incremental index queued during a probe receives a distinct
  job ID.
- Unchanged incremental tests record zero Album and Track tag reads in both
  clients.
- Windows full suites: **518 core tests** and **112 native UI tests** passed.
- Shared Rust suite: **201 passed**.
- WebGUI full suite: **377 passed** in 375.011 seconds, including the rendered
  Edge workflow and real disposable PostgreSQL schemas.

Checkpoint CI: [Validate supported runtimes run 42935945677](https://github.com/scottia/S-P-L-I-N-E-D/actions/runs/42935945677)
passed WebGUI, native Rust, validation selection, and the Windows advisory job.

## Requirement 4 — incremental-first indexing

Status: **PASSED** at `278cdde`.

Acceptance contract:

- A first explicit index creates the baseline; subsequent startup status reads
  use saved state and do not silently run a Full Refresh.
- One explicit incremental generation reconciles the current filesystem after
  any combination of added, changed, moved, and removed Albums.
- Unchanged Album/Track rows retain their timestamps and processing history.
- Moved Albums retain their exact identity, including deterministic `:copy:`
  identities for duplicate releases, plus processing history and authority.
- Full Refresh remains an explicit audit/rebuild operation.
- Windows SQLite and WebGUI PostgreSQL satisfy this contract independently;
  no shared generation or cross-database index lock is claimed.

Implementation and defect corrections:

- Windows now recognizes a staged Album with the same logical key as a move,
  rather than counting and auditing it as a removal. Publication records a
  durable `move` event and reports separate moved, removed, and unchanged
  counts.
- Windows duplicate identity allocation now consults both the current bounded
  in-memory batch and already committed SQLite staging rows. Previously, two
  duplicate releases encountered before the batch checkpoint could receive the
  same base key and fail atomic publication.
- Windows move matching reuses the missing manifest identity whose artwork,
  Track count, total size, and newest timestamp match the new path. Exact
  `:copy:` history survives a move instead of being reassigned by scan order.
- WebGUI no longer reserves a missing old path as a live duplicate. It aliases
  the matching manifest identity to the new path, preserves its PostgreSQL
  history, and emits a durable move event during atomic publication.

### Acceptance evidence

- A single incremental generation was exercised with one addition, one tag and
  artwork change, one move, and one removal accumulated between launches in
  both clients. Both published the current four-Album result and reported
  `added=1`, `changed=1`, `moved=1`, and `removed=1` at detection.
- Separate duplicate-release move tests prove that a `:copy:` Album key and its
  processed timestamp/source survive the path change in file-backed SQLite and
  real disposable PostgreSQL schemas.
- Unchanged Album and Track timestamp sentinels remain byte-for-byte unchanged;
  processed state, timestamps, and selected source remain intact for changed
  and moved Albums.
- Cached startup reads perform no filesystem detection. The established
  216-Album/3,496-Track benchmark records zero Album and Track tag reads on an
  unchanged incremental pass for batch sizes 1, 16, and 64 in both clients.
- Windows full suites: **520 core tests**, **112 native UI tests**, and **201
  shared tests** passed.
- WebGUI full suite: **379 passed** in 433.356 seconds against a real disposable
  PostgreSQL database and the installed Edge browser.
- `git diff --check` and `cargo fmt --all -- --check` passed.

Checkpoint CI: [Validate supported runtimes run 42941326160](https://github.com/scottia/S-P-L-I-N-E-D/actions/runs/42941326160)
passed WebGUI, native Rust, validation selection, and the Windows advisory job.

### Windows SQLite schema v3 advancement

Requirements 1–4 advance the native catalog from schema v2 to v3 so durable
generation, checkpoint, manifest, and publication state can coexist with the
established Album/Track catalog.

- A Windows build opening schema v2 applies the additive coordinator tables,
  records the existing catalog as the `legacy-pre-coordinator` published
  generation, preserves all catalog and processing rows, and then advances
  `PRAGMA user_version` to 3. PostgreSQL is not involved in this migration.
- The saved inventory signature may still carry schema v2 immediately after
  the database migration. Schema v3 now accepts that legacy signature during
  normal read-only startup; it does not force Setup, reset the database, or
  initiate a reindex. A later explicit index publication writes the current
  signature.
- Runtime history/source payloads share `splined.db`. Their compatibility layer
  now reads and writes both the v2 bootstrap database and migrated v3 catalog
  without changing a v3 `user_version`.
- Schema v3 is a one-way catalog migration. A Microsoft Store upgrade preserves
  the existing database, but manually downgrading to a v2-only executable
  requires restoring a pre-upgrade SQLite snapshot.
- Regression coverage loads a schema-v3 database with a legacy v2 inventory
  signature through the normal read-only snapshot path, proves the Album row
  timestamp is unchanged, and separately proves runtime payload round trips
  without downgrading `user_version`.
- Installed-PC validation used the existing
  `E:\apps\SPLINED\test-state\sqlite\splined.db`: the corrected 2.0.36 quick
  build loaded schema v3 and all 216 Albums in 24 ms, entered the workspace,
  and performed no reset or reindex. The installed executable SHA-256 was
  `9CF838D5BF80A44F2F689F1B465D194F9E3784030ADC8516D60DFF3670A1979C`.

## Requirement 5 — adaptive batching and instant startup

Status: **PASSED at `021a4b1`**. The final suite, runtime startup checks, and
adaptive/fixed benchmark gates passed; GitHub CI is recorded on the PR after
the branch push.

Acceptance contract:

- Windows SQLite and WebGUI PostgreSQL make independent batch decisions from
  their own published catalog statistics and measured checkpoint behavior.
- The valid range remains 1–64 Albums. With no trustworthy history the first
  batch is 16; `scan.index_batch_adaptive=false` preserves an explicit fixed
  `scan.index_batch_size` override.
- Adaptation occurs only after a durable checkpoint. It considers published
  Artist/Album/Track counts, observed tracks and bytes per Album, processing
  average, transaction latency, payload-memory pressure, and (where the
  scheduler permits it) competing interactive work.
- A small catalog does not recalibrate without measurable value. Growth begins
  only after 512 published or observed Albums, requires two stable low-cost
  checkpoints, and moves one bounded step at a time. Memory, transaction, or
  interactive pressure may reduce the next batch immediately.
- Normal startup constructs no batching controller, walks no filesystem, and
  opens no tags. It displays the last published generation while indexing is
  explicitly requested in the background.

Implementation:

- The native controller records SQLite workload, payload bytes, checkpoint and
  processing time, decisions, and final size in the generation maintenance
  details. Existing Windows scheduling rejects a refresh while artwork scan or
  embedding work is active, so live interactive pressure is false under that
  contract; the independent policy input and regression coverage remain ready
  if that scheduler later permits overlap.
- The WebGUI controller records the equivalent PostgreSQL measurements and
  checks queued/running non-index jobs at each checkpoint. It can reduce the
  next batch while operator work is waiting without blocking STOP/cancellation.
- Historical measurements seed the next comparable generation only when the
  published Album count remains within 0.5–2.0 of the prior catalog. Large
  catalog changes therefore fall back to 16 rather than reusing stale tuning.
- WebGUI unchanged indexing previously opened a new PostgreSQL connection for
  each Album authority lookup. One generation-scoped autocommit lookup
  connection now removes that connection churn without holding a read
  transaction, changing publication ownership, or loading the catalog into
  memory.

### Measured small-catalog behavior

Fixture: the established read-only 216-Album/3,496-Track test library. Every
run was interrupted after its first durable 16-Album checkpoint, verified zero
live Albums, recovered, published, and ran an unchanged incremental generation.

| Client | Mode | Total interruption + recovery | Peak RSS growth | Checkpoints | Unchanged incremental |
| --- | --- | ---: | ---: | ---: | ---: |
| Windows SQLite | fixed 16 | 5.060 s | 10.60 MiB | 13 | 1.186 s |
| Windows SQLite | adaptive | 5.053 s | 10.70 MiB | 13 | 1.130 s |
| WebGUI PostgreSQL | fixed 16 | 76.118 s | 146.94 MiB | 13 | 2.439 s |
| WebGUI PostgreSQL | adaptive | 74.725 s | 145.75 MiB | 13 | 2.461 s |

No adaptive decisions were made on the small catalog, by design. The WebGUI
unchanged pass improved from the prior 19.546-second batch-16 measurement to
2.439 seconds after removing per-Album connection setup. It still records zero
Album and Track tag reads. Full-pass times are reported with filesystem-cache
context and are not presented as a direct cold/warm percentage comparison.

### Measured 100,000-Track uneven behavior

Disposable local fixture: 1,000 Albums / 100,000 zero-byte `.mp3` files,
distributed as 900 Albums × 20 Tracks, 90 × 200, and 10 × 6,400. This deliberately
tests small, large, and sharply uneven per-Album workloads. Publication and
recovery used disposable SQLite caches and PostgreSQL schemas; the approved
test catalog and media library were not modified.

| Client | Mode | Total interruption + recovery | Peak RSS growth | Checkpoint time / commits | Unchanged incremental |
| --- | --- | ---: | ---: | ---: | ---: |
| Windows SQLite | fixed 16 | 64.401 s | 84.73 MiB | 0.981 s / 62 | 32.917 s |
| Windows SQLite | adaptive | 63.825 s | 84.44 MiB | 0.946 s / 43 | 32.034 s |
| WebGUI PostgreSQL | fixed 16 | 351.671 s | 118.21 MiB | 7.830 s / 62 | 14.623 s |
| WebGUI PostgreSQL | adaptive | 320.969 s | 102.05 MiB | 5.386 s / 43 | 10.832 s |

Windows gained 0.9% total and 2.7% unchanged time while cutting commits by 31%.
WebGUI gained 8.7% total, 13.7% peak memory, 31% fewer commits, and 25.9%
unchanged time. Both retained zero tag reads for the unchanged generation.
Payload pressure caused the adaptive full runs to shrink before the remaining
6,400-Track Albums; recorded peak staged payload fell from 27.51 MB to 17.20 MB
in WebGUI and from 17.22 MB to 8.61 MB in Windows.

The benchmark fixture is on the isolated local `E:` test volume, not the
owner's production NAS. `filesystem_metadata_seconds` therefore records real
local file-open/metadata time (Windows 55.02 s adaptive; WebGUI 90.54 s), not a
claimed NAS latency result. Production media was deliberately not accessed.

Reproducible result JSON is retained outside the repository under
`E:\apps\ChatGpt\SPLINED-local-test-library\state\pr170-req5-bench` and
`...\pr170-req5-100k`.

### Startup and runtime evidence

- The standalone native startup probe loads `media_snapshot` from the
  published SQLite catalog without constructing an index coordinator. The
  established 216-Album/3,496-Track catalog loaded in **37.6 ms**. The installed
  2.0.36 test build independently loaded the same catalog in **24 ms**.
- The deliberately pathological 100,000-Track fixture, in which every
  zero-byte file projects into the Fallback view, loaded its already-published
  1,000-Album catalog in 3.763 seconds warm (4.448 seconds first run). It made no
  inventory, calibration, refresh, or tag-read request. This is a worst-case UI
  projection measurement rather than an ordinary startup target.
- Restarting the existing WebGUI launcher, including stopping the old process,
  starting Python, and polling HTTP readiness, took 4.938 seconds. The live
  status endpoint returned PostgreSQL ready at 2.0.36. Direct read-only state
  inspection then reported `catalog_state=ready`, no maintenance generation,
  zero active jobs, and the published 216 Albums/3,496 Tracks. Startup did not
  enqueue indexing or wait for calibration.

### Final Requirement 5 regression evidence

- Windows: 526 core tests and 112 UI tests passed; all four adaptive-policy
  unit tests and the focused bounded-checkpoint/recovery tests passed.
- WebGUI: 382 tests passed in the full suite. The one environment error was the
  absence of Playwright's default Chrome binary; the exact browser acceptance
  test passed separately against the installed Microsoft Edge executable.
  All four PostgreSQL adaptive-policy tests and the focused recovery tests
  passed.
- Shared/config/release/architecture checks passed (202 core, 5 config, 10
  release, and 14 architecture tests), together with `cargo fmt --check`,
  JavaScript syntax checks, and `git diff --check`.

## Requirements 6–11 final corrections and evidence — 2026-11-11

The assessment checkpoint was `b8952fe881a5375e3c91b7453202a02143478177`.
Its supported-runtime CI was green before corrections began: [run
42973237398](https://github.com/scottia/S-P-L-I-N-E-D/actions/runs/42973237398).
The focused implementation is `ad4fe29e1b4076466ef47665e10cfc68e54f1dcd`;
`8fa8804` adds the final browser cancellation-status regression found during
installed validation. Requirements 1–6 and 11, SQLite schema v3, instant
published-catalog startup, the separate database-authority model, and PR
#174's upgrade-preparedness boundary remain unchanged.

### Requirement 6 — generation-safe cleanup

Status: **PASSED**.

- Both clients write `last_seen_generation` to the durable Album manifest and
  stage the complete Album/Track payload under a generation and owner token.
- Unseen Albums, their Tracks, orphaned Artists, and retired manifest rows are
  removed only inside the validated final publication transaction. Staging,
  failure, cancellation, or worker loss cannot delete from the reader-facing
  catalog.
- Publication validates discovered/staged coverage, rejects error checkpoints
  and duplicate Album identities, checks the current owner/lease, and changes
  the active-generation marker in the same transaction as catalog mutation.
- Current-head focused validation passed: Windows coordinator **7/7**,
  `sqlite_refresh_recovers_committed_batch_without_partial_publication`, and
  `incremental_generation_reconciles_multiple_changes_and_preserves_history`;
  WebGUI/PostgreSQL terminal-state and durable-batch recovery tests **2/2**.
  These tests retain the previous published catalog during interruption and
  publish removals only after successful recovery/validation.

### Requirement 7 — single active indexer

Status: **PASSED** at `ad4fe29`.

- The pre-existing SQLite `IMMEDIATE` generation claim, partial unique active
  row, owner lease, durable checkpoints and stale-owner guards remain the
  publication authority.
- The Windows `RefreshFlight` single-flight now lets a duplicate refresh caller
  wait for and receive the active leader's exact completion result. It neither
  starts a second scan nor claims another generation. Reset remains exclusive
  and cannot attach to refresh work.
- `duplicate_refresh_claims_attach_to_the_active_generation_result` proves one
  execution, two identical caller results and release of the flight for a later
  generation. Existing coordinator lease/recovery tests remain green.
- WebGUI continues returning its existing active durable index job for duplicate
  enqueue requests; advisory locking and the unique active-generation row still
  reject a genuinely competing worker.

### Requirement 8 — resource isolation

Status: **PASSED** at `ad4fe29`.

- Windows records and tests an explicit per-generation budget of one metadata
  reader, one SQLite writer, at most 64 staged Albums and zero remote-provider
  requests. The bounded-memory measurements in Requirement 5 remain the
  resource baseline: 84.44 MiB peak growth and 8.61 MB peak staged payload on
  the 100,000-Track adaptive run.
- WebGUI now uses a lazy `psycopg_pool.ConnectionPool`: minimum zero, default
  maximum eight, five-second checkout timeout, and a validated configurable
  range of 1–32. Checkout normalizes transaction/autocommit state; return and
  shutdown release sessions, and the worker releases its session advisory lock
  before returning the connection.
- WebGUI records one index worker, one metadata reader, at most 64 staged
  Albums, the configured PostgreSQL pool ceiling and zero provider requests in
  generation diagnostics. `test_catalog_refresh_performs_no_remote_provider_requests`
  and the equivalent Windows diagnostics assertion close the negative remote
  request gate.
- A real PostgreSQL pool stress run with `max_size=2` held an indexing session
  while completing 50 interactive `SELECT`s: lazy pool construction was 0.013
  ms, mean 1.874 ms, p95 0.481 ms and max 76.340 ms (the cold first checkout),
  with no more than two connections. During installed indexing, 20 live
  `/api/status` requests averaged 9.411 ms (p95 11.260 ms; max 20.580 ms).
- Requirement 5's adaptive 100,000-Track WebGUI run remains bounded at 102.05
  MiB peak growth and 17.20 MB peak staged payload. Startup still opens no
  PostgreSQL connection merely to construct the pool and never waits for index
  calibration or inventory.

### Requirement 9 — thumbnail backpressure

Status: **PASSED** at `ad4fe29`.

- Windows marks catalog refresh activity independently from artwork work. New
  remote thumbnail requests are deduplicated into a deferred set while indexing
  is active and replay exactly once after completion, cancellation or failure.
- Native Navidrome files now live below the configured persistent
  `cache_dir/navidrome-thumbnails`, not `temporary_cache_dir`. Existing files are
  reused across restart; the normal targeted invalidation rules are unchanged.
  Download fan-out remains four Artist and six Album requests.
- `native_thumbnail_hydration_defers_during_index_and_uses_persistent_cache`
  proves deferral, deduplication/release, the persistent root and both limits.
  The full native suite retains the cache-hit and targeted publication tests.
- WebGUI retains database-first placeholder publication, three download slots,
  8 MiB response limits, atomic persistent cache files, retry backoff and
  per-entry invalidation. `test_active_index_defers_remote_thumbnail_then_reuses_persistent_cache`
  proves zero directory/download calls during maintenance, one resumed download,
  same-process cache reuse and the three-slot ceiling.

### Requirement 10 — nonblocking progress and status

Status: **PASSED**. Automated coverage is backed by installed Windows and
WebGUI cancellation/relaunch evidence.

Implemented and automated:

- Windows now calls `media_snapshot_with_progress`, publishes phase,
  percentage, completed/total Albums, changed/removed/error counts and current
  path into application state, and keeps STOP INDEX in the fixed header and
  Settings while work is active. Cancellation uses its own atomic token, is
  checked through scan, validation and publication, preserves the published
  catalog and returns the UI to a reusable state.
- `windows_index_progress_cancellation_preserves_published_catalog`,
  `refresh_cancellation_is_separate_and_visible_to_the_index_callback` and
  `stop_requests_native_cancellation_and_enters_stopping_phase` pass alongside
  the complete native suite.
- WebGUI emits discovery-aware totals and percentage through the durable job and
  generation status. Its fixed header STOP INDEX remains available after
  Settings closes; STOP cancels both job and queued work, while status polling
  remains independent of the processing mutex. Commit `8fa8804` also reports
  `PostgreSQL full refresh cancelled · published catalog preserved` instead of
  retaining the last progress line; its browser regression passes.
- Installed WebGUI validation showed live progress at `256/320 Albums`, then
  `768/832 Albums`, with an enabled fixed STOP INDEX. STOP entered STOPPING,
  canceled the generation, removed the control and retained the immediately
  usable 164-Artist published catalog. After removing the disposable hard-link
  fixture, a normal publication restored `164 Artists · 210 Albums · 6
  Fallback` in two seconds. No original media was modified.

Installed Windows validation — 2026-11-12:

- The installed 2.0.36 client performed a deliberate full refresh against
  `E:\apps\ChatGpt\SPLINED-local-test-library\music`. The fixed header and
  Library & Files pane both displayed STOP INDEX while the generation was
  active.
- Live status exposed phase, percentage, discovery-aware Album totals, changed
  count, error count and current path. Recorded samples included `staging · 37%
  · 80/~216 Albums · 80 changed · 0 errors` at `Aerosmith\Permanent Vacation`
  and `staging · 93% · 216/~232 Albums · 0 errors` at
  `a-ha\Memorial Beach`.
- STOP INDEX immediately entered `Stopping the active library index at its next
  checkpoint...`; after the checkpoint the progress controls disappeared and
  the same control returned to enabled Refresh DB without an application
  restart.
- Immediate relaunch reached `staging · 51% · 112/~216 Albums · 112 changed ·
  0 errors` at `Alan Jackson\Precious Memories`, advanced through the remaining
  Albums, completed, and returned to enabled Refresh DB. The published catalog
  remained available throughout. No database reset, artwork write or media-file
  modification was used for this gate.

### Mandatory validation/publication interruption recovery

Status: **PASSED** at `ad4fe29`.

- SQLite test failpoints interrupt once after generation validation and once
  immediately before the final publication commit.
  `sqlite_validation_and_publication_interruptions_preserve_atomic_catalog`
  proves that the old published generation remains reader-visible, staged work
  is recoverable, stale owners cannot publish, and restart publishes exactly one
  complete catalog without duplicate rows.
- PostgreSQL provides the same two checkpoint failpoints.
  `test_postgres_validation_and_publication_interruptions_preserve_atomic_catalog`
  runs against a disposable database and proves rollback of the interrupted
  transaction, preservation of the active catalog, safe lease recovery,
  stale-worker rejection and one atomic final publication.
- These tests supplement rather than replace the existing mid-batch durable
  checkpoint, cancellation and owner-loss recovery coverage.

### Requirement 11 — Album-level coordination

Status: **PASSED**.

- Album write events are emitted only after verified write transactions and
  use durable PostgreSQL/SQLite state plus retryable outboxes. A failed Album
  write emits no ownership claim; outbox replay is idempotent.
- Coordination is Album/track scoped and does not invoke Full Refresh or
  full-catalog reconciliation after each write. COMP writes remain track
  scoped and do not manufacture Album-write events.
- Exact-key projection remains authoritative, with the documented normalized
  relative-path fallback only for historical Album-key changes. Newest event
  ordering, writer/override presentation, Windows-owned catalog fields, and
  COMP authority remain unchanged.
- Current-head focused validation passed **6/6** PostgreSQL coordination tests
  (durable ordering/projection, historical-path projection, outbox recovery,
  and failed-write isolation) plus the Windows historical-path projection
  regression. Installed `American Pie 2` and `A$AP Rocky` evidence remains the
  permanent cross-client regression gate above.

### Supplemental source, logging and theme parity corrections

- WebGUI Artist Art discovery now follows the native provider contract rather
  than a second, smaller set of limits: Fanart keeps two backgrounds and up to
  ten portraits, Deezer keeps five exact Artist identities, Last.fm keeps up to
  eight non-placeholder images, Discogs inspects five exact Artist identities
  and retains up to four Artist images from each, and iTunes keeps three exact
  Artist profiles. The browser-only global 16-candidate cutoff was removed;
  each provider remains independently bounded. A live Accept query against the
  established test credentials returned the same 14 remote results permitted
  by Windows (Fanart 5, Discogs 4, iTunes 3 and Deezer 2, zero provider
  warnings). Adding the two local files and one Navidrome portrait restores the
  expected 17-card result. Album search retains the already shared provider
  request bounds and Candidate Filter policy evidence; Artist Art is never
  mixed into Album candidates or vice versa.
- Settings source cards now share the Candidate Findings contract: Samples and
  Local Files remain visible, one large search glyph owns separate Artist and
  Cover rows, source names remain readable, and Deezer uses the same generic
  card layout. Album-only providers remain visible but their Artist Art option
  is disabled and labelled `Disqualified` in both clients. Persistence also
  clamps invalid imported or historical Artist Art flags instead of trusting
  the disabled control alone. The two independent controls are now named
  `Include in Album Art Search` and `Include in Artist Art Search`; the former
  is no longer ambiguous with the latter.
- WebGUI Processing & Logs gained the shared Status Bar controls, and both
  clients now expose them without requiring Advanced mode. Audit action
  filtering is case-insensitive across the complete readable event, not only
  the action column; events are selectable and severity-coloured. The obsolete
  Reconciliation Conflicts Settings surface was removed. Windows gained the
  corresponding Database Audit toggle and filterable, copyable Windows Events
  view; SQLite audit records statement class and table only and never writes
  SQL values. Each Logging heading now has a `+` control that expands Logging
  over the Processing column and a `−` control that restores the two-column
  layout.
- `Update Selected Source Policy` remains because it transfers the selected
  provider editor into the unsaved Settings draft; the section-level Save then
  validates and persists the complete source order and provider set. Both
  clients now place this control directly above its explanatory help.
- Native artwork surfaces no longer inherit theme opacity. Windows Chalk uses
  the WebGUI Chalk palette and Light uses readable grey library text for
  unprocessed rows. The former Windows `System` choice is now `Default` (saved
  `System` preferences migrate to it); Default uses the cyan, blue, violet,
  gold, green, yellow, orange and red SPLINED-logo family for controls and
  frames without pink or magenta UI accents, and WebGUI exposes the same
  Default choice. Windows Dark now uses the exact WebGUI Dark charcoal surface,
  line, selected-state and semantic palette instead of the former blue theme.
  Library list text in both clients is theme-owned while semantic state colours
  remain authoritative.
- The legacy `artist_art_targets_for_albums` name is intentionally retained as
  a test-only wrapper (`#[cfg(test)]`). Production uses the projection-aware
  `artist_art_targets_for_albums_with_projection` path introduced with credited
  Artist indexing. The wrapper keeps regression coverage for artist-scoped
  Soundtrack/Various Artist filenames and COMP fallback exclusion without
  compiling unused compatibility code into the release executable. History for
  future diagnosis: commit `0a22077` introduced the original helper on
  2026-10-27; commit `b68a841` added the projection-aware production path on
  2026-11-09 and left the original name as a compatibility seam for those
  focused tests. It is not a second Artist Art implementation or runtime path.

### Artist workspace, local-art and responsive-layout correction evidence

- The reported Accept Artist Art count difference was traced with verbose logs
  rather than normalized by another result cap. Both installed clients now
  resolve the same provider-authorized inventory: Fanart 5, Discogs 4, iTunes
  3 and Deezer 2, plus two local files and one Navidrome portrait (**17**
  candidates total). The earlier 14-card browser capture was stale UI state;
  the current browser accessibility tree, provider response and Candidate
  Filter inventory all agree. No Artist candidates are added to Album search.
- WebGUI About now uses the native About information hierarchy while reporting
  the WebGUI/Python/PostgreSQL runtime truth. Help opens the repository browser
  workspace guide in a new tab and no longer opens a redundant definition
  dialog.
- Windows pane minima and typography now have an explicit compact layout for
  1280 logical pixels and below. The compact layout keeps a 420-pixel center
  workspace with temporary 220-pixel library and 230-pixel artwork minima;
  remembered desktop widths remain unchanged. The installed executable was
  resized to 1024 by 768 and its log reported a responsive 1008 by 729 Slint
  content surface without startup or repaint failure.
- WebGUI Similar Artists now use authenticated lazy portrait URLs backed by a
  bounded downloader and persistent cache. All four live Accept Similar Artist
  portraits resolved through the test service. Artist detail is tried first
  for genres; when Navidrome supplies none, the exact selected-Artist
  appearance projection supplies the library genres. Accept consequently
  resolves its eleven expected genres without associating another Artist.
- A library-collapsed CSS selector was unintentionally broad enough to hide
  every nested panel heading. It is now scoped to the library heading only, so
  `Selected Artwork`, its left status marker, right state badge and Windows-
  parity actions remain visible at compact widths.
- Artist Art enhancement rerenders now restart from the explicitly selected
  portrait/banner source before applying the current adjustment stack. This
  removes cumulative transforms and stale-preview jitter. Neutral banner
  framing uses contain geometry, preserving the complete source at the largest
  available target fit; an explicit operator zoom remains the only path that
  may crop. Async generation guards still reject late preview results.
- Existing local `artist.*` and `banner.*` discovery was re-audited in both
  clients. WebGUI resolves local Artist Art before provider cache and its saved
  package independently resolves portrait and banner. Windows already lazily
  hydrates the same local paths. Native library thumbnails already use a round
  frame; this behavior was preserved rather than duplicated with another
  thumbnail implementation.
- Final installed comparison exposed test-configuration drift rather than a
  discovery defect: WebGUI used
  `SPLINED-local-test-library/state/artist-art`, while the native test client
  still used its older `SPLINED/test-state/artist_art` directory. Both roots
  contained different files for Rob Wasserman's same identity hash. The native
  test configuration now points to the established shared WebGUI directory;
  after restart and Artist reselection Windows loaded the same saved black-and-
  white portrait and banner visible in WebGUI. This is a test-installation
  correction only and does not change production defaults or database state.
- WebGUI now owns the typed Artist query as workspace state. Selecting an Artist
  no longer clears or loses the active filter during rerender. The Rob
  Wasserman regression was checked against the live PostgreSQL projection:
  search remains bound to Rob's exact name and MBID, with `Gold` as a featured
  appearance owned by Aaron Neville; no Aaron Neville Artist Art result was
  returned. Windows retains the already-correct selected-identity sequence.
- Windows biography justification now leaves a sparse line natural when the
  following long word would otherwise create visually extreme spacing. Dense
  non-final lines remain fully justified and the final line remains natural.
  Both clients use bright white non-Light copy, collapse at five complete
  lines, and reserve inline room for an unobscured `more` control.

### Computer-use installed-client validation

The Windows computer-use runtime was reinitialized after the earlier capture
failure and successfully inspected both installed applications on 2026-11-11:

- The installed native executable rendered a 1009 by 761 compact workspace.
  The capture retained the Media Library tabs and filters, Candidate Findings
  source cards, center overview, Selected Artwork heading with `Current`, and
  the Candidate Artwork dock at the same time. A second foreground capture
  showed the live 3 Doors Down overview, portrait, biography, Album status and
  candidate cards rather than Setup or a launcher window. This is installed-
  client evidence for the compact responsive layout; it does not replace the
  separate owner-visible Requirement 10 progress/STOP/cancel/relaunch gate.
- A clean 1920 by 1032 native capture verified that the rebuilt biography is
  bright white, fully justified, exactly five collapsed lines and retains the
  inline `more` action. The signed-in browser independently rendered the same
  five-line contract with the control visible.
- After the owner signed in, the installed WebGUI was captured in the live
  3 Doors Down workspace. Similar Artists rendered as round portraits for
  Creed, Nickelback, Staind, Shinedown, Daughtry, Hinder and Seether. The
  Artist & Featured Artist Top Genres row rendered its returned genres, while
  Selected Artwork retained its heading, marker, `Current` status and actions.
- The live About dialog matched the native information-card structure and
  reported version `2.0.36` at head `b626a2cfe7c1`, GNU GPL v3.0,
  Python/FastAPI/PostgreSQL, project and architecture details, update state and
  the external Microsoft Store, source and release-note links. Help opened the
  repository `docs/browser-workspace-guide.md` in a separate browser tab.
- The live Artist Art Enhancements workspace rendered Current Package, Artist
  Image and Artist Banner together; the full-width banner remained contained
  rather than being cropped to the preview viewport. A live Accept capture
  exposed **17 candidates** from Local Files 2, Navidrome 1, iTunes 3,
  Fanart.tv 5, Discogs 4 and Deezer 2.
- The Rob Wasserman package was then exercised through the actual browser
  fitment and write path. Explicit zoom changed only the working preview,
  Reset Fitment restored the complete source at `1.00x`, and Upscale Preview
  retained that fit. The owner then changed zoom, repositioned the banner and
  completed WRITE Artist Art: WebGUI reloaded the round `384 x 384` portrait
  and contained `4096 x 1755` banner from the two identity-scoped files written
  at the same time, and the Artist Overview matched the approved composition.
  The saved package did not revert to the earlier preview and did not write
  into the Aaron Neville Album folder.
- Entering `Rob` in Artist Search and selecting Rob Wasserman preserved the
  filter value and projected only Rob Wasserman and the Gold Album. The local
  portrait and banner candidates both displayed Rob Wasserman; Aaron Neville
  did not appear in the selected Artist Art identity. The initial refusal to
  launch a completed Gold Album was a defect, not an accepted limitation:
  the Artist-wide launch paths in both clients conflated default unfinished
  selection with explicit operator reprocessing. Explicit All/Filtered
  selection now queues every selected identified Album regardless of green,
  bypassed or timeout status; automatic initial selection still prioritizes
  unfinished work. Live WebGUI validation opened the completed `Gold` Album's
  provider results, kept STOP PROCESS available and restored LAUNCH after the
  operator stopped the review. Live Windows validation showed
  `Processing Aaron Neville — Gold`, changed LAUNCH to STOP PROCESS, and
  restored LAUNCH after cancellation. No artwork was written by either
  reprocessing validation.
- The continuously displayed WebGUI `Checking the music library for changes…`
  state was also reproduced. PostgreSQL showed that its probe had completed in
  about 1.3 seconds, but the browser had cancelled its only completion poll
  while backgrounded. Completion polling now survives background visibility,
  has a single bounded timer and reports failures instead of preserving stale
  busy state. A live reload returned to the normal green workspace status;
  the focused browser regression covers the backgrounded completion path.

### Artist Art regression follow-up after biography styling

- The owner reported that Rob Wasserman Artist Art search, fitment and overwrite
  had worked before the later bright-white biography treatment. Commit history
  confirms that the bright-white/five-line change modified only biography CSS
  and truncation. It did not change Artist Art provider identity, preview state
  or storage. The nearby regressions had three independent causes: stale
  PostgreSQL multi-credit projection, late asynchronous preview responses, and
  WebGUI Artist Art writes inheriting numbered-file preservation.
- The ID3v2.3 tag reader exposed a slash-packed TPE1 value even when the file
  contained ordered per-Artist MusicBrainz IDs. The corrected parser uses that
  ordered MBID cardinality to separate the credit while preserving literal
  names such as `AC/DC`. A one-time managed-credit publication marker advances
  so Full Refresh republishes existing rows. The installed test refresh
  published all 216 Albums and restored **164 Artists**, including exactly one
  `Rob Wasserman` projection with one `Gold` appearance.
- A projected Artist can own a valid track-credit MBID without owning an Album
  directory. Its shared Artist Art is therefore written to the stable
  name-scoped package. Thumbnail discovery previously tried only the MBID stem
  and could show a cached Navidrome portrait while the same overview already
  found the name-scoped banner and local candidates. Discovery now prefers an
  existing MBID package but also recognizes the canonical name-scoped package
  before provider cache. The live Rob endpoint consequently resolves both
  `artist-name-4491d596cbaf4d98cccb4e2f4f3a192b.png` and
  `banner-name-4491d596cbaf4d98cccb4e2f4f3a192b.png`.
- The post-refresh live provider request returned five online candidates
  (Deezer 1, Discogs 2, iTunes 1, Navidrome 1) with no warnings; together with
  the two verified local files the workspace contract is **7 candidates** and
  contains no Aaron Neville Artist Art result. Generation/signature guards and
  request cancellation prevent a late render from repainting an earlier crop.
  WebGUI and Windows writes replace the same canonical shared package, remove
  numbered siblings, and make the last verified client save authoritative.
- The Windows computer-use helper later failed to capture the active monitor
  (`IGraphicsCaptureItemInterop.CreateForMonitor`, `0x80070057`) after one
  documented recovery attempt. API, catalog, provider and file evidence above
  is complete; a new current-head installed visual capture remains pending and
  is not represented as passed by this follow-up.

Codex did not initiate an artwork or destructive database write during this
computer-use validation. The owner explicitly performed the Rob Wasserman
WRITE Artist Art acceptance step described above against the established test
fixture; its resulting files and post-write UI state were inspected read-only.

Focused validation: **57/57 WebGUI Artist Art/identity tests**, **403/403 full
WebGUI tests (78 environment-dependent skips)**, **535/535 Windows core tests**,
**122/122 Windows application tests**, **100/100 browser behavior tests**, Python
compilation, JavaScript syntax, Slint/Rust compilation, and `git diff --check`
passed.

Installed validation: WebGUI 2.0.36 was restarted against the established test
PostgreSQL service and reports `database_ready=true`; its live-served assets
contain Default, the standard-access Status Bar, and Logging expand/compress.
The updated native 2.0.36 executable was installed and started from
`E:\apps\SPLINED\splined.exe`. The final installed revision and executable hash
are recorded in **Final regression evidence and installed revisions** below.
The successful computer-use session supplied current native and browser visual
captures plus accessibility-state inspection. Earlier installed evidence for
the source-card, artwork-opacity, audit and two-stage source-policy corrections
remains valid; current-head source contracts and complete automated suites pass.

### Local WebP, status, theme and Artist Overview follow-up

- Album-folder inspection now starts with every local `cover.*` source visible,
  including animated WebP below the configured quality range. Candidate Filter
  remains available immediately afterward to include or disregard the source by
  policy. This is a display/discovery correction only: the configured preferred
  embedded-thumbnail size remains **1200 x 1200** and is not weakened.
- Valid local WebP is decoded before the prepared-preview size guard is applied.
  The AC/DC fixture's `cover.webp` is a verified 1080 x 1080, 480-frame animated
  source larger than 25 MB; it now remains selectable and is described as
  `Animated WebP` with its frame count. The independent browser-upload limit is
  unchanged. The Selected Artwork embedded-art ribbon uses the accepted blue
  status treatment.
- Completed ready/save/write notices now dismiss after a bounded interval while
  busy and error states remain visible. Small, medium and large status modes grow
  from the header center and the header actions remain right-aligned.
- Light theme uses a transparent logo surface and the same native grey text
  values in both clients (`#18252d` primary and `#52616a` secondary). Chalk is
  a ghosted graphite/plum treatment rather than the earlier flat navy surface:
  both clients use the same deterministic smoky chalk SVG, muted pastel
  outlines and low-opacity panel shadows without neon. The texture and shadows
  are limited to application chrome; neither WebGUI nor native artwork pixels
  are filtered, recolored or dimmed.
- Artist Overview owns its vertical scroll. The five footer actions (`Show All`,
  `Refresh Artist Info`, `MusicBrainz Artist`, `COMP Report`, and
  `EMBED All Albums`) are a normal final content row with `position: relative`;
  they are not fixed, sticky, or a nested scroll surface. Live computer-use
  validation scrolled AC/DC from the portrait through the Album grid to the
  footer at `scrollTop == maxScroll`. Switching to Artists Art Enhancements and
  returning to Find & Match Artwork cleared the saved overview scroll before
  render, returned to `scrollTop == 0`, preserved AC/DC, and did not compress or
  lock the Artist artwork. The browser regression prevents the earlier
  post-render scroll-memory race from returning the footer to the bottom.

Follow-up validation: **401/401 WebGUI tests passed (78 environment-dependent
skips)** and **97/97 browser behavior tests passed**. JavaScript syntax and the
live-served asset check passed. Final native compilation and repository hygiene
results are recorded below with the final follow-up commit.

Live computer-use theme validation selected Chalk and confirmed the served
graphite/plum palette (`#17131b`, `#25202a`, `#302a35`), the shared ghost texture
above the surface wash and `filter: none` on Selected Artwork. Selecting Light
repainted the full workspace and reported the exact Windows primary/secondary
grey values above, with the active controls and body using the primary grey.
The Windows Slint source accepts the same shared texture plus native Rectangle
drop shadows and passed `cargo check`. Its native installation evidence is
recorded with the Windows UI scale and biography follow-up below.

### Windows UI scale and biography typography follow-up

- Windows retains native PerMonitorV2 awareness and Slint logical-pixel window
  geometry. The existing persisted `display_scale_percent` preference remains
  backward-compatible in `ui.toml`, but now drives a proportional application
  UI scale for typography, icons, hit targets, controls, rows, cards,
  thumbnails, spacing, padding and pane constraints. It does not set
  `SLINT_SCALE_FACTOR`, override Windows DPI, stretch the rendered window or
  resize the native window bounds. Responsive narrow-window reflow remains an
  independent multiplier.
- Biography layout no longer estimates text width from character counts or
  uses Slint `HorizontalLayout` with `space-between`. Windows now composes the
  paragraph with DirectWrite's measured Segoe UI glyph advances and justified
  paragraph alignment. An embedded English (US) Knuth-Liang dictionary inserts
  discretionary soft-hyphen opportunities only in the transient layout copy;
  the stored biography is unchanged. DirectWrite leaves the final paragraph
  line natural rather than justified.
- Collapsed biographies expose five composed text lines and expanded
  biographies expose at most ten. The complete Biography paragraph is the one
  click surface; there is no `(…more)` / `(less…)` text, separate control row,
  control object, or fragile suffix-only hit target. Removing the suffix also
  gives measured Biography text the full available paragraph width.
- Native Biography copy uses the same bright white treatment as WebGUI in
  non-Light themes; Light keeps the accepted dark-grey text for contrast. This
  paragraph, UI-scale and expansion-control correction is Windows-only.
- Focused contracts verify proportional 80/90/100/110/125 percent choices,
  native window bounds, responsive paragraph reflow, bounded justified gaps,
  source-text preservation and the single clickable paragraph without a
  visible expansion suffix. The full Windows workspace passed **535 core + 122
  application tests** at this source revision. The test PC's active monitor
  reports 96 DPI (100 percent); installed
  80/100/125 percent application-scale validation is recorded below after the
  local native installation. Live Windows-DPI 125/150 percent evidence remains
  distinct from deterministic geometry/reflow coverage and is not claimed
  unless an appropriate test monitor/session is available.

### Opposing override Artist aggregation follow-up

- An opposing-client verified Album write continues to render that Album card
  dark grey/overridden. The expanded Artist/Album file-list row uses the same
  dark-grey treatment rather than the red error color. Artist State now exposes
  six actionable Album rows in both clients: **Incomplete, Processed, Bypassed,
  Embedding Review, Overridden, Albums Complete**. **Unprocessed** is deliberately
  not an actionable state row; its blue count appears in the `Artist State (#) >
  Unprocessed` heading until an Album acquires a state. The old Artist Bypass row
  was removed and all actual bypass cases use the single yellow Bypassed category.
- The heading count and six Artist State rows are mutually exclusive and
  cumulative for the currently selected Artist. The full-width white
  `Artists Album Count` row supplies the green authoritative total. Album and
  Fallback workspaces report zero Artist State totals. An opposing write
  contributes only to dark-grey Overridden; it
  does not inflate Bypassed or Albums Complete. The overview equation is now
  `shown = completed + in process + overridden + validating`, preventing the
  prior 17-Album summary from adding to 19. The Artist's aggregate list color
  may remain green when all work is satisfied by local completions and/or
  opposing overrides, while each Album retains its true green or grey state.
- Album cards restore the orange Processed dot, use a yellow frame for bypassed
  Albums, and add a dark-grey opposing-override dot/frame. The Album Status Key
  documents the combined yellow `Artist/Album art missing/bypassed` meaning,
  the dark-grey `Opposing album override`, and the yellow `BYPASSED albums with
  custom art` frame. The blue WebP dot is documented separately under
  `MISC STATUS` as `Animated — cover.webp present: review the still cover`; it
  remains supplemental artwork information and does not alter Artist State.
- The installed AC/DC regression demonstrated the upgrade failure and its fix.
  SQLite contained all 17 Albums, but an older `ui.toml` recorded
  `status_filter = "all"` alongside a serialized member list created before the
  Overridden state existed. Windows restored that stale list literally, hiding
  the two opposing overrides and producing 16 file-list/15 overview Albums.
  Durable `all` intent now takes precedence over serialized members, while a
  genuine custom list automatically retains hidden Unprocessed. Live Windows
  validation now shows AC/DC `(17 Albums)`, `17 shown`, `Artists Album Count 17`,
  and the exact partition `7 Unprocessed + 7 Embedding Review + 2 Overridden +
  1 Albums Complete = 17`.
- Windows applies the projection from the newest WebGUI coordination writer;
  WebGUI applies the equivalent projection from the newest Windows writer.
  Neither client rewrites the Album's stored catalog status, coordination
  history, database schema or opposing-card presentation.
- Focused regressions cover a verified opposing Album, separate Bypassed and
  Overridden counts, exclusive Completed/in-Process totals, the seven-row
  category contract, legacy `all` preference migration, grey Album rows/dots,
  and retained purple Embedding Review behavior.
  Installed-client evidence and final full-suite totals are recorded below at
  the final validation commit.

#### ABBA five-Album authority regression

- The established test catalogs both contain exactly five ABBA Albums. SQLite
  retains five durable artwork-write events whose latest writer is `webgui`, so
  Windows correctly presents all five Album cards as dark-grey opposing
  overrides and treats the Artist aggregate as satisfied/complete (green).
- PostgreSQL records a present cover and verified full embedded-track count for
  all five Albums. `Arrival`, `Gold (Greatest Hits) 40th Anniversary`, and `The
  Visitors` were already stored as `processed`; `Super Trouper` and `Waterloo`
  still carried a stale `unprocessed` label even though their materialized
  cover and embedded-track evidence was complete. Trusting that stale label
  made WebGUI's ABBA Artist name purple.
- Album projection now derives effective completion from current cover plus
  verified full embedded-track evidence while preserving explicit bypass,
  timeout, partial, and error states. Windows likewise no longer treats a real
  timeout as complete. Consequently, opposing overrides satisfy the Artist but
  remain counted only as Overridden; verified local Albums are Albums Complete;
  only genuine embedding-review evidence produces the purple Artist state.
- Focused Windows aggregate tests, the WebGUI catalog-projection suite, and the
  100-test browser-state suite cover this regression without rewriting stored
  catalog history or coordination ownership. Installed WebGUI validation then
  confirmed the corrected five-Album ABBA projection after restart and sign-in.

### Final regression evidence and installed revisions

- Windows: **535 core + 122 application tests passed**; the full Rust workspace,
  focused duplicate-flight, resource, thumbnail, progress/cancellation,
  forced-interruption and identity-history tests passed.
- WebGUI: **403 passed, 78 skipped** in the full suite. Browser behavior is
  **100/100 passed** after the Artist workspace and status-lifecycle corrections. The focused real
  PostgreSQL pool, no-provider, thumbnail, progress/cancellation, forced
  interruption, `American Pie 2` and `A$AP Rocky` regressions passed.
- Formatting, Python compilation, JavaScript syntax and `git diff --check`
  passed. The assessment-head supported-runtime CI was green before work began;
  final-head CI is recorded on PR #170 after the documentation commit.
- Supported-runtime Action #259 failed only in the native Ubuntu job; its
  Windows advisory and complete WebGUI jobs passed. The three native failures
  were stale source-contract assertions for accepted proportional UI scaling
  and one test-only contiguous `SLINT_SCALE_FACTOR` literal. The contracts now
  follow the current scaled Slint implementation without weakening the runtime
  prohibition. Local reruns pass 202 Rust library tests, 5 public-config tests,
  10 release-automation tests, 14 Windows architecture tests, 403 WebGUI tests
  with 78 skips, and 100 browser behavior tests. Final-head CI remains the
  completion gate. Corrective head `9fae5a1` passed all supported-runtime jobs
  in [Action #260](https://github.com/scottia/S-P-L-I-N-E-D/actions/runs/43357100481):
  validation selection, native Ubuntu, Windows advisory, and WebGUI.
- Installed Windows 2.0.36 executable:
  `E:\apps\SPLINED\splined.exe`, SHA-256
  `4A7FC81BF27F66BC5B57B7079DD3D09E4532249B0DC6E02D40C4397323B1DDD5`
  from functional commit `7f3ee3a66d90b0bc9f9e842ee8dfc8fab63e963d`.
  The installed WebGUI runs current PR source at 2.0.36 against the established
  PostgreSQL service. Credentialed startup was restored after localhost-only UI
  validation (`auth_required=true`, `database_ready=true`).
- The isolated WebGUI test service intentionally has no automation-only token,
  user ID, or password stored in the repository. Browser session identifiers
  are opaque cookies whose hashes and bounded session data are stored in
  PostgreSQL; a service restart therefore preserves authentication without
  embedding a reusable credential in source or test configuration. Password
  recovery revokes every durable browser session.

### PR #174 preparedness handoff

- PR #170 adds no schema beyond SQLite v3 and does not alter the provisional
  v2→v3 migration policy or any original v2 backup.
- Packaging must include `psycopg_pool` (provided by
  `psycopg[binary,pool]==3.3.8`). The pool is lazy, bounded and closed at
  runtime shutdown; active PostgreSQL session isolation remains an explicit
  PR #174 upgrade concern.
- Native thumbnail persistence now uses `cache_dir/navidrome-thumbnails`.
  This is reusable cache state, not a catalog or a reason to reset/migrate v2.
- Cancellation and both forced-interruption gates prove that upgrade-adjacent
  restarts preserve the previous published catalog and recover staged v3 work.
- Owner notification/consent, Ignore/Bypass behavior, Microsoft Store v2
  upgrade presentation, pre-upgrade backup/recovery and active PostgreSQL
  connection isolation remain deferred to PR #174 exactly as approved. No PR
  #174 UI or migration-policy implementation was changed here.

### Final 12-item acceptance checklist

1. Requirement 1 — streaming inventory and bounded batches: **PASSED**.
2. Requirement 2 — persistent index generations: **PASSED**.
3. Requirement 3 — precomputed change manifest: **PASSED**.
4. Requirement 4 — incremental-first indexing: **PASSED**.
5. Requirement 5 — adaptive batching and instant startup: **PASSED**.
6. Requirement 6 — generation-safe cleanup: **PASSED**.
7. Requirement 7 — single active indexer/duplicate attachment: **PASSED**.
8. Requirement 8 — measured resource isolation: **PASSED**.
9. Requirement 9 — thumbnail backpressure and persistent cache: **PASSED**.
10. Requirement 10 — nonblocking progress, STOP, cancellation and relaunch:
    **PASSED**.
11. Requirement 11 — Album-level coordination: **PASSED**.
12. Mandatory validation/publication interruption recovery: **PASSED**.

All implementation, focused regression, full local-suite, installed-client and
final-head supported-runtime CI gates are complete. PR #170 and PR #174 remain
Draft and unmerged pending owner acceptance.

## v3.0.1 post-release correction — recovery commit accounting

Recorded 2026-11-13 after a 3,885-Album NAS catalog exposed multi-hour Windows
recovery and repeated WebGUI restarts.

PR #170's Requirement 1 intended bounded recovery writes, but its accepted
implementation called `touch_reusable_checkpoint` / `touch_completed` once per
recovered Album. Each call opened and committed its own SQLite or PostgreSQL
transaction. The benchmark's `checkpoint_commits` value counted only normal
staging batches, so the published “13 commits” result omitted the recovered
Album commits and could not enforce the acceptance contract.

The corrected regression gate requires all of the following:

- Load committed checkpoint identity once per resumed generation; do not issue
  one checkpoint lookup per Album.
- Attach recovered checkpoints in the configured adaptive 1–64 Album batch.
- Observe the recovery batch call/transaction in tests; final row counts alone
  are insufficient.
- Count recovery transactions in generation timings.
- Reuse filesystem size/mtime evidence gathered during enumeration instead of
  repeating NAS metadata calls during fingerprint, manifest and Track work.
- Preserve deterministic Track order while Windows and WebGUI each perform at
  a configurable one-to-thirty-two bounded concurrent tag-reader budget
  (default sixteen); both retain one database writer.

The v3 PostgreSQL upgrade now also reports the pinned application and schema
versions, lists automatically applied migrations, removes pre-v3 diagnostic
events that refer to retired cache paths, and recommends a Full Refresh when
earlier-major catalog rows require revalidation. It never relabels catalog rows
without reading the media again.

### Corrected N-share benchmark

The established 216-Album / 3,496-Track read-only library was indexed with the
disposable SQLite database on
`N:\downloads\0_backups\splined\windows\test`. The first run includes cold
filesystem cache; the next two are the comparable warm-cache policy check.

| Policy | Interruption + recovery | Recovery commits | Unchanged incremental | Peak RSS growth |
| --- | ---: | ---: | ---: | ---: |
| Adaptive, cold | 17.048 s | 25 | 1.167 s | 10.58 MiB |
| Fixed 64, warm | 4.833 s | 4 | 1.032 s | 10.75 MiB |
| Adaptive, warm | 4.517 s | 14 | 1.076 s | 10.16 MiB |

The warm adaptive run is slightly faster than fixed 64 with equivalent memory,
so the default remains adaptive with an initial batch of 16 and a bounded 1–64
range. The correction does not trade a larger recovery window for throughput.

### Two-core 100,000-Track regression

The original Requirement 5 uneven fixture (1,000 Albums / 100,000 Tracks) was
rerun on the corrected code with each client pinned to two logical CPUs at
BelowNormal priority. A 16 GiB process ceiling was recorded; neither client
approached it. PostgreSQL server/container memory is separate from the measured
WebGUI client process and is not claimed by this ceiling.

| Client / database | Interruption + recovery | Accurate commits | Unchanged incremental | Peak client RSS growth |
| --- | ---: | ---: | ---: | ---: |
| Windows / disposable local SQLite | 48.520 s | 44 | 26.502 s | 85.17 MiB |
| Windows / disposable N: test SQLite | 50.989 s | 45 | 24.733 s | 84.66 MiB |
| WebGUI / disposable PostgreSQL schema | 201.862 s | 44 | 8.756 s | 118.56 MiB |

Every run staged 16 Albums before simulated worker loss, exposed zero live
Albums before recovery, published 1,000 Albums / 100,000 Tracks, and recorded
zero Album or Track tag reads on the unchanged pass. The one-commit variation
is an adaptive boundary decision. Unlike the PR #170 results, these commit
totals include the recovered-checkpoint transaction.

Result JSON is retained outside the repository under
`E:\apps\ChatGpt\SPLINED-local-test-library\state\pr170-regression-20261113-2core`.

### Production read-only WebGUI projection reference

The production read-only role was loaded only from
`production-splined_postgresql-readonly.env`; no service startup, migration,
audit, or write was attempted. At 1,141 stored Artists / 3,885 Albums / 63,483
Tracks, the three two-core Artist projection passes originally took 37.090,
37.292, and 36.969 seconds. Profiling assigned about 34.05 seconds to resolving
3,885 already-authoritative catalog paths against the NAS for an empty
coordination result.

Catalog coordination projection now derives that relative identity lexically
and retains physical path resolution for write-time event validation. The same
read-only passes take 2.827, 2.722, and 2.767 seconds and return the same 5,240
stored/projected Artist identities. The count query remains 0.005 seconds. Pool
state after the run was maximum 8, size 1, available 1, waiting 0.

### Live NAS metadata-reader taper and eager baseline

The earlier small-fixture result established that bounded parallel tag reads
were safe, but it was not representative production sizing evidence. A live
PostgreSQL development catalog was therefore exercised against the real NAS
library with the SPLINED container limited to two CPUs and 16 GiB. Every taper
sample processed the same first 352 Albums, retained one database writer and
reported zero errors.

| Local tag readers | 352-Album duration | Change from 8 readers |
| ---: | ---: | ---: |
| 8 | 62.0 s | baseline |
| 16 | 50.0 s | 19.4% faster |
| 24 | 51.0 s | 17.7% faster |
| 32 | 52.0 s | 16.1% faster |

The curve tapers after sixteen. The default is therefore sixteen for every
library size, configurable from 1 through 32. It is a ceiling: an Album with
fewer tracks naturally occupies fewer readers. Reader count is not adaptive;
the independent durable database checkpoint batch remains adaptive.

With sixteen readers, the eager baseline published 3,825 Albums / 5,241
Artists in 2,413 seconds (40m13s) with zero errors while the authenticated GUI
remained active. The first 352 Albums had warm stored evidence from the taper;
the remaining Albums required cold NAS tag reads. The run excluded the
Billboard and `Compilations (Top 40)` test directories but deliberately kept
`[Various Artists]`. Container memory reached a stable 2.6/16 GiB plateau,
CPU was 13.7% at the observed tail, and the container had zero restarts.

The follow-up gate reuses representative-track evidence instead of opening
that file again in the bounded track pass. It also places Albums at or above a
configurable track threshold (default 100, zero disables) after ordinary
Albums and emits intra-Album track progress/lease heartbeats. Track-credit
authority and its eager publication semantics remain unchanged pending a
separate architecture review.
