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

Status: **IN PROGRESS — local acceptance passed; checkpoint CI pending**.

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

Remaining gate before marking Requirement 2 passed: the checkpoint commit and
its GitHub Actions run must be green.
