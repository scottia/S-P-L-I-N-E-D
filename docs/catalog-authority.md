# Catalog field authority — Windows SQLite and WebGUI

**Review basis:** current `build/WEBGui/webgui/catalog_authority.py`,
PostgreSQL migrations 016, 017 and 020, and the owner's approved
[policy register](POLICY-REGISTER.md). This document describes field
ownership and synchronization boundaries; it does **not** certify
installed SQLite/PostgreSQL synchronization or authorize catalog writes.

## Ownership and field boundaries

**Windows `splined.db` is authoritative for shared, synchronized
Artist/Album/Track catalog fields.** PostgreSQL serves the browser's
separate catalog, operational queues, audit, and WebGUI-owned presentation.
Neither a WebGUI Full Refresh nor PostgreSQL coordination can silently
make PostgreSQL authoritative over Windows-managed fields.

| Authority | Fields / behaviors |
| --- | --- |
| Native Windows SQLite | Artist, Album and Track identity keys; canonical media paths; artist/album names and sort names; MusicBrainz IDs; synchronized inventory and processing/bypass/timeout evidence; Windows catalog timestamps |
| WebGUI PostgreSQL | Browser jobs, authentication, session data, audit, staging and cache metadata, WebGUI-owned embedded-art validation and display materializations |
| Derived UI | File-list Artist/Album name colors, all applicable Album status dots, card frames, COMP-track display, and aggregated counters from owner-approved rules |

The **exact synchronized and excluded columns** are defined in
[`catalog_authority.py`](../build/WEBGui/webgui/catalog_authority.py),
not by a duplicated prose list. The ownership register resides in the
PostgreSQL `catalog_sync_authority` table introduced by
[migration 016](../build/WEBGui/webgui/migrations/016_catalog_sync_authority.sql).

## Reconciliation and indexing lifecycle

- Exact, unambiguous identity associations can establish managed
  ownership even when corresponding data values are already equal.
- Other relationships require an explicitly safe, successful Apply;
  an ambiguous, conflicting or failed association is not silently owned.
- Managed rows retain Windows-owned catalog fields through WebGUI
  indexing and Full Refresh. WebGUI may update its owned and derived
  columns without overwriting Windows-synchronized identity or state.
- Existing unmanaged WebGUI rows and newly discovered physical paths
  require their proper, separately authorized indexing rules. Do not
  manufacture Windows-owned Albums from WebGUI-only discovery.
- Artist grouping for an owned Album differs from a track-scoped COMP
  fallback. Unidentified COMP progress stays on the relevant track
  report and does not arbitrarily change an Artist aggregate.

## Last successful artwork writer

[migration 017](../build/WEBGui/webgui/migrations/017_album_artwork_write_coordination.sql)
records Album artwork-writer provenance;
[migration 020](../build/WEBGui/webgui/migrations/020_comp_track_write_coordination.sql)
handles individual COMP-track writes. These are **coordination and
presentation signals**, not catalog or policy authorities.

The opposing application's **Album thumbnail and approved Dark Grey
override frame** indicate a verified last write by the other client.
The owner's accepted screenshot governs exact framing and banner
appearance; an older description of a Red frame is not authority.
Preview/edit artwork remains in color, and unrelated status dots,
banners and counters retain their independent meanings. A successful
subsequent local write transfers the presentation notice. Failed,
cancelled, or preview-only operations do not claim a new writer.

## Validation still required

Test actual Windows SNAP association, Apply Safe/Conflicts, Full Refresh,
first/second-client writer order and reconnect behavior against disposable
PostgreSQL and SQLite databases. These are **implementation checks**,
not requests to reapprove the name/color/status requirements.

See the [SQLite catalog guide](splined-media-database.md) and
[implementation validation ledger](SCHEMA-VALIDATION.md).
