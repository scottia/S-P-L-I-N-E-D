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

Status: **INCOMPLETE — installed Windows visual gate only**.

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

Outstanding proof:

- The exact 2.0.36 Windows binary is installed and running, and automated
  controller/Slint contract coverage passes, but the current RDP session exposes
  only the native window chrome: monitor capture fails and the Slint content is
  absent from the accessibility tree. Therefore a trustworthy installed-client
  visual proof of live percentage/current path plus STOP/cancel/relaunch could
  not be recorded. Per the acceptance rule, R10 remains incomplete rather than
  substituting source assertions or unit tests for that live gate.

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

Codex did not initiate an artwork or destructive database write during this
computer-use validation. The owner explicitly performed the Rob Wasserman
WRITE Artist Art acceptance step described above against the established test
fixture; its resulting files and post-write UI state were inspected read-only.

Focused validation: **41/41 WebGUI source/audit/render tests**, **395/395 full
WebGUI tests (78 environment-dependent skips)**, **533/533 Windows core tests**,
**115/115 Windows application tests**, **92/92 browser behavior tests**, Python
compilation, JavaScript syntax, Slint/Rust compilation, and `git diff --check`
passed.

Installed validation: WebGUI 2.0.36 was restarted against the established test
PostgreSQL service and reports `database_ready=true`; its live-served assets
contain Default, the standard-access Status Bar, and Logging expand/compress.
The updated native 2.0.36 executable was installed and started from
`E:\apps\SPLINED\splined.exe` with SHA-256
`32AC151044F0425DBB7DAA1C8649FD5CEA06218B28C1EF54FF5BF7C12ABFA954`
from functional commit `53c874a`.
The successful computer-use session supplied current native and browser visual
captures plus accessibility-state inspection. Earlier installed evidence for
the source-card, artwork-opacity, audit and two-stage source-policy corrections
remains valid; current-head source contracts and complete automated suites pass.

### Final regression evidence and installed revisions

- Windows: **533 core + 115 application tests passed**; the full Rust workspace,
  focused duplicate-flight, resource, thumbnail, progress/cancellation,
  forced-interruption and identity-history tests passed.
- WebGUI: **395 passed, 78 skipped** in the full suite. Browser behavior is
  **92/92 passed** after the Artist workspace and status-lifecycle corrections. The focused real
  PostgreSQL pool, no-provider, thumbnail, progress/cancellation, forced
  interruption, `American Pie 2` and `A$AP Rocky` regressions passed.
- Formatting, Python compilation, JavaScript syntax and `git diff --check`
  passed. The assessment-head supported-runtime CI was green before work began;
  final-head CI is recorded on PR #170 after the documentation commit.
- Installed Windows 2.0.36 executable:
  `E:\apps\SPLINED\splined.exe`, SHA-256
  `32AC151044F0425DBB7DAA1C8649FD5CEA06218B28C1EF54FF5BF7C12ABFA954`
  from functional commit `53c874a`.
  The installed WebGUI runs current PR source at 2.0.36 against the established
  PostgreSQL service. Credentialed startup was restored after localhost-only UI
  validation (`auth_required=true`, `database_ready=true`).

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

### Review result and remaining work

1. Record one owner-visible installed Windows run showing live phase/percentage,
   Album counts/current path, responsive fixed STOP INDEX, successful cancel and
   immediate relaunch. No implementation defect is presently reproduced; this
   is the sole unproven R10 acceptance gate.

Requirements 7–9 and the mandatory interruption-recovery gate are complete.
Requirement 10 remains explicitly incomplete for the installed Windows visual
evidence above. Requirements 1–6 and 11 remain accepted. PR #170 and PR #174
remain Draft and unmerged.
