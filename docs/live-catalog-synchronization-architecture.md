# Live catalog synchronization architecture

**Status:** Draft implementation boundary. This design extends the durable
transport introduced by PR #148 without changing its Album last-writer visual
contract or making PostgreSQL a catalog authority.

## Authority and direction

Windows `splined.db` remains the sole authority for synchronized Artist, Album,
and Track fields. Live catalog synchronization is therefore a one-way catalog
stream:

```text
Windows SQLite transaction
  -> durable SQLite catalog outbox
  -> PostgreSQL event ledger
  -> guarded PostgreSQL catalog apply
  -> durable acknowledgement
```

WebGUI artwork writes use the separate PR #148 coordination path:

```text
verified WebGUI write
  -> Album coordination event
  -> Windows acknowledgement and physical-media verification
  -> bounded Windows catalog update
  -> ordinary authoritative catalog event
```

PostgreSQL never emits Artist, Album, or Track catalog values back into SQLite.
Jobs, Audit, authentication, embedded-art validation, UI state, and coordination
provenance remain PostgreSQL/WebGUI-owned.

## Persistent mode

Both clients expose the same three-value catalog synchronization mode:

| Mode | Behavior |
|---|---|
| `live` | Incremental catalog events are published and applied. Routine reconciliation schedules and startup Apply are disabled. Normal SQLite database backups continue. |
| `manual-snap` | Existing authorized SNAP Compare/Apply remains available and live transport is inactive. This is the compatibility default. |
| `off` | Catalogs operate independently. Neither live events nor routine SNAP reconciliation run automatically. Manual backup remains available. |

Changing to `live` is rejected until the selected Windows source has a verified
baseline checkpoint. Changing modes never deletes SNAPs, reconciliation history,
live event history, or ordinary database backups.

## Transactional change capture

SQLite triggers on the authoritative `artists`, `albums`, and `tracks` tables
append a local monotonic revision to a durable outbox in the same transaction as
the catalog mutation. The trigger records entity table, exact entity key,
operation, event identity, and creation time. It does not rely on `updated_at`.

The Windows publisher opens one consistent SQLite read transaction, coalesces
superseded events for the same entity, and serializes the current complete row
projection. A missing row becomes a tombstone. Each transmitted event includes:

- stable Windows source-instance ID;
- local revision and event ID;
- entity table, key, and operation;
- complete allow-listed Windows projection;
- deterministic payload digest;
- latest Album artwork-coordination revision observed by Windows;
- schema/protocol version.

The outbox row remains until PostgreSQL records an applied or deterministic
no-op acknowledgement for that exact event. Retry after disconnect or restart
reuses the same event identity.

## PostgreSQL ingestion and guarded apply

PostgreSQL stores every accepted event under a unique
`(source_instance_id, local_revision)` key. A per-entity state row records the
last applied revision and digest. Duplicate delivery is a no-op; an older
revision cannot replace a newer entity state.

Apply uses the field lists in `webgui/catalog_authority.py`:

- Windows synchronized fields may be inserted or updated.
- PostgreSQL-exclusive and derived fields are preserved.
- Full Refresh remains unable to replace fields protected by
  `catalog_sync_authority`.
- Deletion is allowed only for an entity already managed by the same Windows
  source; standalone WebGUI rows are never removed by a foreign tombstone.
- Parent/child work is ordered Artist → Album → Track for upserts and the
  reverse for tombstones.

An event whose observed Album-coordination revision predates a newer verified
WebGUI artwork write cannot overwrite artwork-dependent catalog fields. It is
held for Windows acknowledgement and physical verification. The later Windows
event carries the observed coordination revision and releases that causal
barrier. This prevents stale processing data from reversing newer verified
physical work without granting PostgreSQL catalog authority.

## Baseline and integrity

Live activation has two explicit phases:

1. Generate a complete SQLite projection in a consistent read transaction.
2. Compare it with PostgreSQL using the existing identity and field-ownership
   rules. Source-only rows and safe authoritative updates may be staged;
   ambiguous identity conflicts remain owner-review items.

Only a conflict-free or explicitly reviewed baseline records `ready`. Existing
SNAP reconciliation is the recovery/migration tool during this phase and is not
removed.

After activation, Windows periodically publishes per-table row counts and
complete-row digests over the synchronized projection. PostgreSQL independently
recomputes its Windows-owned projection. A mismatch marks the source `drifted`,
stops automatic live Apply, preserves the outbox, and directs the operator to a
manual SNAP comparison. Integrity checks never repair data silently.

## Recovery and idempotency

- Windows offline: SQLite transactions and outbox entries continue normally.
- WebGUI/PostgreSQL offline: publisher retries with the same event identities.
- WebGUI offline but PostgreSQL online: ingestion remains durable; guarded Apply
  resumes with the WebGUI worker.
- Windows restart: source ID, outbox, acknowledgements, and integrity checkpoint
  are read from SQLite.
- PostgreSQL restart: pending ledger rows and entity state survive.
- Repeated events: unique source revision plus payload digest produces a no-op.
- Interrupted batch: each entity has an independent outcome; acknowledgement is
  issued only after commit.
- Recursive loops: PostgreSQL never republishes synchronized catalog rows, and
  Album coordination is excluded from catalog triggers.

## Compatibility risks

1. PR #148 must merge before this stacked implementation can merge cleanly.
2. Legacy SQLite files need trigger/outbox installation without manufacturing
   changes for pre-existing rows; the explicit baseline owns initial population.
3. Artist-key or Album-key identity changes appear as tombstone plus insert and
   must preserve parent ordering.
4. A large first baseline can exceed routine batch sizes; it must be resumable.
5. WebGUI artwork verification differs by audio container. Windows must use its
   existing CoverFront reader before acknowledging physical completion.
6. Full Refresh and live Apply can overlap; short advisory locks and existing
   field ownership are required rather than a long global processing lock.
7. Clock skew is irrelevant to ordering but remains diagnostic metadata; local
   revisions and event identity are authoritative.

## Validation plan

The isolated E: environment remains mandatory:

- media: `E:\apps\ChatGpt\SPLINED-local-test-library\music`;
- Windows: `E:\apps\SPLINED` with isolated SQLite state;
- WebGUI: `E:\apps\SPLINED-WebGUI`;
- PostgreSQL: `127.0.0.1:55432/splined`.

Acceptance must prove:

1. complete baseline of populated Artist, Album, and Track rows;
2. disconnected Windows inserts, updates, and removals replay after reconnect;
3. disconnected WebGUI artwork writes survive and are physically verified by
   Windows before catalog acknowledgement;
4. deterministic replay and duplicate delivery are idempotent;
5. client and PostgreSQL restart preserve pending and applied state;
6. missing-record and digest drift detection stop automatic Apply safely;
7. PostgreSQL-only validation and Audit fields survive live updates;
8. Full Refresh cannot rewrite Windows-managed values or create new live diffs;
9. routine SNAP scheduling is disabled only in live mode while SQLite database
   backup remains active;
10. manual SNAP recovery still operates after live mode is disabled.

No production endpoint, database, media path, deployment, release, or automatic
merge is part of this Draft implementation.
