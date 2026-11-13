# PR #180–182 v3.0.1 acceptance and promotion record

This record captures the catalog, setup, recovery, and WebGUI stabilization
accepted on `dev` through PRs #180, #181, and #182 before promotion to
`main`. It is an engineering acceptance record, not a release announcement or
authorization to create or move a release tag.

## Accepted catalog and setup behavior

- Windows/SQLite and WebGUI/PostgreSQL retain separate catalog authorities,
  independent leases, and one deterministic database writer per index.
- Metadata reads default to 16 workers, remain configurable from 1 through 32,
  and feed adaptive checkpoints of 1–64 whole Albums.
- The representative Track is parsed once and its evidence is reused while the
  bounded readers process only the remaining Tracks.
- Ordinary Albums are processed before Albums at or above the configurable
  large-Album threshold. The default is 100 Tracks and `0` disables deferral.
- Large Albums publish continuous intra-Album progress and lease heartbeats so
  exceptional compilations do not appear stalled.
- Eager Track inventory and existing Track-credit projections are unchanged.
- Interrupted PostgreSQL work resumes the same job and generation, retains
  completed checkpoints, and reports cumulative generation time separately
  from the current process segment.
- Hashed WebGUI sessions remain valid across an orderly application restart.
- Library-change probes run on WebGUI load/reload, immediately after changed
  ignored-folder settings are saved, and hourly while the GUI remains open.
  Probes are read-only and deduplicate against active probes and indexes.
- PostgreSQL reset confirmation reports the effective libpq environment target
  when `PGHOST`, `PGPORT`, and `PGDATABASE` provide the connection.
- Schema startup reports automatic upgrades and earlier-major catalog drift;
  pre-v3 diagnostic records are pruned without relabeling catalog rows.
- Windows Setup retains the Setup and Shared DB guide links, shared/client-lock
  validation, and live first-index progress.

## Accepted WebGUI stability behavior

- Concurrent metadata requests for one selected Artist share one provider
  traversal, and Artist appearance persistence is conflict-safe.
- Signed library-image handles are derived from the configured WebGUI access
  token and remain valid across a container restart.
- An Artist Art write refreshes only the affected Artist row instead of
  rebuilding the complete Artist list.
- Lazy remote Artist thumbnails use a three-request browser limit while
  selected, hero, and review artwork remains eager.
- `/api/health` is a lightweight database- and session-independent liveness
  check. Authenticated database readiness remains on `/api/status`, preventing
  temporary PostgreSQL pool pressure from requesting a container restart.
- Artist COMP remains available after an Album edit and returns to clean Artist
  COMP context when reopened.
- COMP launches and artwork writes are single-flight. The active COMP Track is
  disabled, write controls show `EMBEDDING…` or `WRITING…`, and delayed repeat
  input cannot enqueue duplicate work.

## Live catalog and performance evidence

The reader taper used the same 352-Album NAS prefix with the application
limited to 2 CPUs and 16 GiB:

| Readers | Duration |
| ---: | ---: |
| 8 | 62 s |
| 16 | 50 s |
| 24 | 51 s |
| 32 | 52 s |

Sixteen is the observed taper point.

The fresh PostgreSQL run included Billboard, retained `[Various Artists]`, and
excluded only `Compilations (Top 40)`:

- forced container recreation after 1,231 observed Albums;
- browser authentication and responsiveness retained;
- same job and generation resumed at the next observed Album;
- 3,891 Albums, 62,410 Tracks, and 1,141 catalog Artists published;
- 78 Albums deferred at the 100-Track threshold;
- zero errors in 51m 49s cumulative wall time;
- 2,645 PostgreSQL checkpoint commits consumed 14.500 seconds.

Removing the Top 40 exclusion produced a 57.227-second read-only probe that
reported exactly four added Albums and no changed, moved, or removed Albums.
The subsequent incremental generation reused all 3,891 published Albums, added
four Albums and 1,235 Tracks, and published 3,895 Albums, 63,645 Tracks, and
1,142 catalog Artists in 88.960 seconds with zero errors. The 994-Track
`Jaar Radio Top 2000` Album retained continuous progress and completed normally.

The PR #180 two-core 100,000-Track fixture also completed with bounded memory:

| Runtime | Total | Unchanged | Checkpoint commits | Client peak growth |
| --- | ---: | ---: | ---: | ---: |
| Windows local SQLite | 48.520 s | 26.502 s | 44 | 85.17 MiB |
| Windows SQLite on `N:` | 50.989 s | 24.733 s | 45 | 84.66 MiB |
| WebGUI PostgreSQL | 201.862 s | 8.756 s | 44 | 118.56 MiB |

The production read-only Artist projection retained 5,240 identical results
while improving from approximately 37.1 seconds to 2.77 seconds. A fresh
Shared + client-lock SQLite installation on the established `N:` test path
published 216 Albums and 3,496 Tracks with schema v3, rollback journal mode,
and zero errors.

## Final WebGUI live regression

The isolated PostgreSQL dev stack completed *NSYNC and credit-scoped 070 Shake
Artist Art writes, repeated Album searches, a COMP report, and a verified COMP
Track embed without a restart, sign-out, blank artwork, or database error. A
controlled container restart preserved the authenticated session and existing
artwork handles.

The final single-flight run selected two different *NSYNC COMP Tracks. Each
selection created one unique job and received one write decision. Both jobs
completed with `resolved=1`, `installed=1`, and zero failures. No delayed
duplicate click or follow-on job appeared after either completion. The run
recorded one diagnostic session and no restart or authentication loss.

Unavailable-artwork responses for explicitly inspected MusicBrainz releases
were retained as provider diagnostics. They did not produce a COMP, database,
session, or container failure.

## Automated evidence

- PR #180: 541 Windows core tests, 122 Windows desktop tests, and 407 WebGUI
  tests passed; 80 environment-dependent tests were skipped.
- PR #181: 543 Windows tests, 415 local Python tests, 103 browser regressions,
  strict changed-code Clippy, and 12 workflow-selection tests passed.
- PR #182 final head: 420 backend tests and 107 browser regressions passed;
  the selected GitHub runtime workflow passed before live acceptance.
- PR #182's first workflow retained one historical red WebGUI run because its
  PostgreSQL restart test still expected pre-fix image handles to expire. The
  next test-only commit updated that assertion to the intended restart-stable
  contract, and all four subsequent PR workflow runs passed.

## Promotion boundaries

- No database-authority, eager Track-credit, or mobile-interface redesign is
  included.
- Windows remains SQLite-backed; WebGUI remains PostgreSQL-backed.
- Existing cancellation, resumable checkpoints, published-catalog authority,
  and cross-client coordination are preserved.
- PRs #180, #181, and #182 were accepted on `dev`. Promotion to `main` carries
  this exact accepted scope; release packaging, Microsoft Partner Center
  submission, and release-tag movement remain separate authorized workflows.
