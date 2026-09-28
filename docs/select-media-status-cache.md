# Select Media status cache

SPLINED keeps the persistent Select Media status snapshot at:

```text
<_history>/select-media-status.json
```

Default Docker path:

```text
/_logs/_history/select-media-status.json
```

The current schema is **version 2**. The file is a persistent UI/status
acceleration model, not completion/bypass/timeout execution authority and not
the retired SQLite picker cache.

## JSON-first startup

On a warm launch SPLINED reads the JSON first and seeds the Artist Picker from
saved Artist/Album status immediately. The picker does not wait for a complete
filesystem crawl before it can paint.

```text
START
  ↓
read select-media-status.json
  ↓
seed cached Artist/Album status
  ↓
render Select Media
  ↓
background filesystem reconciliation
       ├─ unchanged → retain cached status
       ├─ changed   → authoritative reclassification
       ├─ new       → discover/reclassify
       └─ correction → update picker + JSON live
```

An unchanged Artist uses its structural sentinel and persisted Album statuses
directly for aggregate Artist status. Missing/invalid status or a changed
sentinel falls back to the authoritative inventory/classification path.

The reconciliation pass is deliberately **non-blocking**. It must not restore a
modal startup gate that makes the JSON-first picker wait for all Artists.

## Stored data

Version 2 stores lightweight Select Media state:

- Artist paths and Album inventory;
- supported audio/local-art paths needed by inventory;
- inventory key and structural sentinels;
- live Album folder status: `unprocessed`, `processed`, `bypassed`, or
  `timeout`;
- timestamps/progress needed for safe incremental persistence.

It does **not** store MusicBrainz/provider authority, candidate ranking,
AISPLINE output, approval, or final-write decisions.

## Live persistence

Status transitions emitted by the resident Select Media session are written
atomically to the JSON during the run. They are not deferred until process
exit. A relaunch can therefore paint the status SPLINED itself most recently
committed while the background reconciliation checks for external filesystem
changes.

Completion history, bypass authority and timeout policy remain authoritative.
The JSON is an acceleration/snapshot layer; it must not manufacture eligibility.

## Structural validation

Validation first checks compact Artist-root/first-level structural sentinels.
An unchanged Artist reuses cached Album inventory/status. Changed/new Artists
fall back to deeper inventory and update only the affected cache entry.

Instrumentation may report:

```text
Album status cache: 1,031 Artists checked · 1,026 unchanged · 5 changed · 5 rescanned · validation: 2.4s
```

The reconciliation can continue after Select Media is already usable.

## First build and recovery

If no usable version-2 snapshot exists, SPLINED builds one incrementally and
writes it atomically. Interrupted completed entries may be reused on the next
run. The file can be deleted safely; SPLINED rebuilds it without deleting
completion/bypass history or changing library files.

The persistent location under `_logs/_history` is intentional so ordinary
candidate-cache cleanup does not remove Select Media status acceleration.
