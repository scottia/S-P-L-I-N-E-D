# Catalog field authority

Windows `splined.db` is the sole authority for catalog values synchronized to
PostgreSQL. Reconciliation records proven Artist, Album, and Track associations
in `catalog_sync_authority`. This ownership survives later WebGUI Full Refresh
and incremental indexing.

## Field ownership

| Owner | Fields |
|---|---|
| Windows | Artist/Album/Track identity keys; paths; names and sort names; MusicBrainz IDs; release metadata; file inventory; cover inventory; processing, bypass and timeout state; created/updated/last-seen timestamps; source SPLINED version |
| PostgreSQL/WebGUI | Album embedded-art validation count, validation status and validation timestamp; jobs; Audit; authentication; UI/session state |
| Derived presentation | Artist status and Album counters; Album status derived from authoritative processing/COMP evidence |

The executable field lists live in `webgui/catalog_authority.py`. A managed
indexing UPSERT may update only the PostgreSQL-exclusive and derived sets.
Artist status and counters use owned Albums plus identified compilations. An
unidentified COMP fallback is per-track presentation for credited Artists, so
its folder-wide progress remains on the COMP card/report and is excluded from
their Artist aggregate.

## Association lifecycle

- An exact, unambiguous SQLite/PostgreSQL key association is marked during
  Compare even when the synchronized values are already equal.
- A non-key association or source-only entity is marked only after its safe
  reconciliation Apply succeeds.
- Ambiguous, blocked, conflicting, or failed entities never acquire ownership.
- Full Refresh anchors a managed Album at its authoritative Artist and Album
  keys, preserves synchronized fields, and never deletes a managed Album or
  Track merely because physical inventory changed.
- Once Windows ownership is configured, an entirely new physical Album is
  deferred until a Windows SNAP introduces its synchronized identity. Existing
  unmanaged WebGUI rows continue to use normal indexing behavior.

## Intentional exceptions

Full Refresh may still inspect physical media, recalculate derived presentation,
and update WebGUI-only embedded-art validation. Explicit owner-initiated WebGUI
artwork writes remain available; they are not indexing operations, and the next
authorized Windows reconciliation remains authoritative for their shared
catalog projection. Reconciliation never copies physical media bytes.

Standalone WebGUI installations with no `catalog_sync_authority` rows retain
the existing insert, update, and stale-row cleanup behavior.
