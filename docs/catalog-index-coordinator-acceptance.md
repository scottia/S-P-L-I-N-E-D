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
