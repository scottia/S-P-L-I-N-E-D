# Native Windows SQLite media catalog

**Scope:** The native Rust/Slint client's persistent `splined.db`,
including optional multi-client shared-file use. Windows SQLite is the
owner of synchronized catalog fields; WebGUI operates its separate
PostgreSQL catalog and persistent work queue. The optional PostgreSQL
coordination pathway does **not** change shared-field authority.
See [Catalog field authority](catalog-authority.md).

## Persistent locations and authority

Windows uses the configured `scan.cache_dir` for
`<scan.cache_dir>/splined.db`. The SQL Database Directory is
persistent; the Temporary Run Cache stores expendable candidate and
preview data independently. Do not delete `splined.db` when cleaning
temporary caches, and do not treat PostgreSQL as a SQLite file backup.

Config v5 records the library root and database path. The Rust backend
owns SQLite schema, indexing, identity, processing/bypass/timeout
history and snapshot construction; the Slint UI consumes a projection,
not a second authoritative JSON database. A malformed or unsupported
schema must fail with a diagnostic rather than silently creating an
alternative catalog.

## Standalone versus shared SQLite

| Mode | Storage/concurrency |
| --- | --- |
| Standalone | Local SQLite with WAL and its configured busy-timeout; no shared-file coordination is implied |
| Shared / Network | Several native Windows clients access **one physical `splined.db`**. Configure `scan.sqlite_shared = true` in each client; use rollback journaling and verified cross-host filesystem locking |
| Optional PostgreSQL coordination | Shares successful artwork-writer provenance and reconciliation metadata with WebGUI; does **not** make PostgreSQL the primary Windows catalog or change Artist/Album policy |

Shared mode is a storage decision, **never an alternate policy**.
Both modes use the checked-in canonical
[`schema/media-state-classification.json`](../schema/media-state-classification.json)
and
[`schema/comp-report-policy.json`](../schema/comp-report-policy.json)
contracts. The Windows executable can embed the same approved policy;
existing credential-directory mirrors must not silently introduce new
rankings or dot order.

For shared use, verify one physical database, aligned source schema
and canonical library path mapping, correct ignored folders and
`cover.*` rules, and working filesystem locks before any concurrent
writer is enabled. Optional `scan.windows_client_lock` adds a
Windows-side client-file lock where supported; a stale sidecar filename
alone does not imply an active OS lock.

## Identity, indexing and startup

Artist and Album identities come from their approved MusicBrainz
authorities when present or normalized tag-derived fallbacks.
Database paths retain the canonical library namespace; each client
maps them to its configured local filesystem root for reads/writes.
Never duplicate an Album because of an unrelated path spelling or
different client-specific mount point.

Warm startup should reuse the published catalog and stored
representative-track evidence without rereading unchanged audio
metadata. Explicit **Apply Library Changes** processes detected
folder/inventory deltas; a deliberate **Refresh DB** performs
full index maintenance. Changed COMP/Fallback tracks can require
more detailed track-level evidence than a normal Album. Completion,
history and checkpoint updates must remain durable across bounded
index transactions, interruption and restart.

The v3 Catalog Index Coordinator establishes generations,
checkpoints and publication state. A v2 database requires the
explicit [v2→v3 preparedness flow](windows-schema-upgrade-preparedness.md),
not an implicit startup refresh or schema downgrade.

## Media and artwork state

Album catalog rows retain internal processing labels, cover and
track evidence, bypass and timeout state. **Internal `unprocessed`
is not an Artist/Album NAME color or Album status dot**: it belongs
to the numeric MEDIA FILTER count. Visible Artist/Album names,
multi-status Album dots and override frames use the independent
ranked [owner-approved policy](POLICY-REGISTER.md).

Verified Album artwork writes and individual COMP-track embeddings
publish last-successful-writer provenance through coordination
outboxes when enabled. The other client may show the approved
Dark Grey Album override frame or COMP-track notice without losing
the ability to search, preview, or perform a new local write. A
failed or preview-only operation must not claim writer authority.

## Backup, maintenance and recovery

Use consistent SQLite online backups for the catalog and a separate
secret-safe settings/credential backup. A PostgreSQL dump is a
different operation; also preserve any music files that might be
modified by artwork embedding. A database reset, migration or
uncontrolled shared-file rewrite requires explicit operator action.

The current documentation restore performed **no database access or
schema migration**. Full runtime acceptance still requires startup,
shared filesystem, v2→v3 preservation, index checkpoint, interruption,
and Windows/PostgreSQL synchronization testing.
