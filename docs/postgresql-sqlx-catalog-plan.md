# SQLite/PostgreSQL-SQLx migration plan

This document defines the development and acceptance contract for replacing
the separate Windows SQLite catalog and WebGUI PostgreSQL catalog with one
PostgreSQL catalog. It is an active development proposal, not supported
release behavior.

## Objective

- Windows accesses the shared catalog through `sqlx-postgres`.
- Windows and WebGUI use one schema, migration history, identity model, and
  Artist/Album/track/COMP authority.
- Local caches and temporary files may remain client-local, but they must not
  become a second catalog authority.
- SQLite catalog reconciliation, replay, count repair, and opposing-index
  synchronization are removed only after PostgreSQL parity is proven.

## Baseline and incremental indexing

The first successful scan creates a complete catalog baseline. Later refreshes
must be incremental:

1. Persist a normalized file manifest with path identity, size, modification
   time, and the fingerprint required to detect an actual metadata change.
2. Classify files as added, changed, moved, unchanged, or removed before tag
   parsing.
3. Read media tags only for added or changed files.
4. Apply bounded batches through PostgreSQL staging tables and transactional
   upserts.
5. Recompute only affected Artist, Album, appearance, artwork, and COMP
   projections.
6. Record checkpoints so interruption resumes without repeating completed
   batches.
7. Use explicit projection versions for code changes. Reproject stored facts
   without reopening media whenever the required raw values already exist.

Database startup applies schema migrations in place. It must not silently reset
or rebuild an established catalog.

## Shared authority contract

- PostgreSQL stores canonical raw track facts and derived catalog projections.
- A database-backed scan lease permits only one catalog mutation run at a time.
- Both clients may request refresh, but concurrent requests join or observe the
  authoritative run rather than launching competing scans.
- Writes use revisioned, idempotent operations and reject stale updates.
- PostgreSQL-exclusive authentication, Audit, and operational fields remain
  isolated from catalog projection updates.
- Reset, backup, and restore identify the exact PostgreSQL target and fence
  active workers before mutation.

## Development gates

### 1. Isolated shadow catalog

Create a disposable PostgreSQL database, role, credentials file, copied media
fixture, cache, and logs. Import the current catalog and compare every projected
Artist, Album, track, appearance, and COMP value while existing applications
remain untouched.

### 2. Windows PostgreSQL repository

Introduce a catalog repository boundary and implement it with the existing
Rust `sqlx` PostgreSQL dependency. Preserve native Rust/Slint behavior while
moving reads, writes, queues, reset, and backup contracts behind that boundary.

### 3. Incremental scanner

Add manifest comparison, bounded parallel tag reads, staged bulk upserts,
affected-projection rebuilding, checkpoints, and progressive catalog
availability. Instrument enumeration, tag I/O, database work, projection, and
UI refresh separately.

### 4. Shared-client validation

Run Windows and WebGUI against the same disposable PostgreSQL catalog. Verify
concurrent refresh requests, artwork writes, Artist Art, Album embedding,
COMP/Fallback processing, cancellation, restart recovery, and exact status
projection.

### 5. SQLite retirement

Stop creating or reading `splined.db` only after the PostgreSQL implementation
passes parity, performance, failure, backup/restore, and reset testing. Remove
obsolete synchronization code in a separately reviewable change.

## Performance acceptance

Measure on the approved test PC and report both cold and warm runs:

- A 70,000-track cold baseline must be at least four times faster than the
  extrapolated current full-refresh path, with a target of 15–20 minutes.
- A no-change refresh must complete within 60 seconds.
- One changed Album must become visible within 10 seconds.
- Completed batches must become usable before the full cold import finishes.
- An interrupted refresh must resume without rereading completed files.
- Measurements must identify filesystem enumeration, tag reads, database
  writes, projection, and rendering independently.

Targets may be tightened after the first representative 70,000-track baseline;
they may not be relaxed without recorded hardware and bottleneck evidence.

## Failure and recovery acceptance

- PostgreSQL unavailability produces a clear non-destructive error and never
  creates a divergent local catalog.
- Repeated operations are idempotent.
- Stale clients cannot replace newer verified writes.
- Backup and restore operate only on the confirmed PostgreSQL target.
- Reset preserves schema, migrations, administrative configuration, and
  credentials while clearing application records as documented.
- Artist, Album, track, appearance, and COMP counts and identities are exactly
  equal in both clients because they query the same authority.

## Scope boundaries

- No production database or original media library is used for development.
- Reserved migration numbers 018 and 019 are not reused.
- The initial planning change does not alter dependencies, migrations, runtime
  settings, application behavior, or release packaging.
- Release and deployment remain separate owner-authorized work after the
  PostgreSQL-only catalog passes its acceptance gates.
