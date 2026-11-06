# Cross-client Album override implementation

This implementation follows the visual contract merged through PR #147. It
adds a presentation-only last-successful-writer exchange between
the Windows SQLite application and the WebGUI PostgreSQL application.

## Contract

- Only a verified Album artwork WRITE or EMBED creates an event.
- The writer's own card retains its normal full-color presentation.
- The opposing client desaturates only the Album thumbnail and uses a red card
  frame. A verified opposing write suppresses an otherwise stale unprocessed
  red Missing dot; every other independent dot, label, status calculation and
  control remains unchanged.
- The opposing card replaces its normal category banner with the configurable
  **OVERRIDE** banner. Its reset baseline is a white frame (`#FFFFFF`),
  light-grey background (`#D3D3D3`) and red lettering (`#FF344F`). Both clients
  expose it below ARTIST in Enhancement settings and restore the Album's normal
  category banner when local ownership returns.
- The notice remains across selection, refresh and restart until the local
  client completes its own verified WRITE or EMBED.
- Preview, search, failed work, cancellation, Compare/Dry Run, Full Refresh and
  reconciliation do not create or clear events.

Both clients present Album cards in the same stable order: owned Artist Albums,
Featured appearances, Soundtracks, Various Artists, then COMP fallbacks. Each
category is newest-to-oldest. Processing state and override state never move a
card into another sort group.

## Storage and recovery

PostgreSQL migration 017 creates one current row per canonical Album identity.
Each event records the Album key, optional MusicBrainz Album ID, library-relative
path, writer, operation, unique event ID, source completion time and monotonic
PostgreSQL revision. An Album display name alone is never identity evidence.

Windows keeps the current acknowledgement and an unsent-event outbox in
`splined.db`. WebGUI keeps its transient-failure outbox in the persistent cache.
Both clients retry idempotently. A delayed older event is acknowledged but does
not replace the newer writer. Coordination does not transfer processing/catalog
authority or participate in reconciliation classification or Apply. When a
newer opposing-client write is observed, the receiving client resolves the
affected Album's physical artwork. Windows persistently updates only that
Album's artwork path/inventory in SQLite; completion, bypass, selected source,
representative tracks, and unrelated Albums remain unchanged. Both UIs
invalidate stale thumbnails and recover readable artwork after restart.

Windows enables the optional connection with the protected **Album
Coordination** credential under Settings → Connections. The credential survives
a normal application restart while keeping its password outside Config v5 and
SQLite. `SPLINED_COORDINATION_DATABASE_URL` remains a process-environment
override for managed launches and tests. WebGUI uses its existing PostgreSQL
connection. LISTEN/NOTIFY is not required: both clients poll the durable row at
a short interval, so service and client restarts recover from stored state.

## Isolated PC acceptance environment

The implementation is exercised only with the established local test stack:

| Component | Isolated location |
|---|---|
| Media | `E:\apps\ChatGpt\SPLINED-local-test-library\music` |
| Windows installation | `E:\apps\SPLINED` |
| Windows state | `E:\apps\SPLINED\test-state` |
| WebGUI runtime | `E:\apps\SPLINED-WebGUI` |
| WebGUI config | `E:\apps\ChatGpt\SPLINED-local-test-library\state\config\config.toml` |
| PostgreSQL | `127.0.0.1:55432/splined` |

The local PostgreSQL catalog is intentionally smaller than production. Before
feature testing, its baseline column catalog is compared read-only with
production; migration 017 is the only intentional local schema addition.
Production media and databases are never write targets for acceptance.

## Schema alignment review

The production and local servers both report PostgreSQL 18.6. Catalog metadata
was read from `pg_catalog`, rather than permission-filtered
`information_schema`, with production kept read-only.

- The production baseline and the pre-feature local baseline match exactly:
  24 tables, 3 sequences, 244 columns, 247 constraints and 55 indexes.
- Migration 017 is the only local extension: one table, one sequence, 11
  columns, 14 constraints and 4 indexes.
- The local `public` schema and isolated
  `splined_cross_client_20261104` acceptance schema have identical normalized
  table, column, constraint and index definitions.
- Catalog row counts intentionally differ. The acceptance catalog contains 52
  Artists, 210 Albums and 2,896 Tracks from the isolated E: library.

## Validation checkpoint

- WebGUI: 305 tests passed, including real isolated PostgreSQL integration and
  the rendered Brave workflow.
- Windows: 471 library tests and 102 executable tests passed; formatting,
  locked dependency checking and strict Clippy passed.
- JavaScript syntax, Python compilation and `git diff --check` passed.
- Docker is not installed on the PC test host. The selected GitHub WebGUI lane
  remains the formal container build and smoke gate.
- The optimized quick local build was installed at
  `E:\apps\SPLINED\SPLINED.exe`; SHA-256
  `00615AED7C687F694E323B81EC4C1C30597CCAF3D562A29CDB1F9429325CDFB0`.

## PC integration evidence

The backed-up `A Great Big World / Particles` fixture was used for the physical
write check.

1. A real WebGUI **WRITE Cover** completed against the isolated E: Album and
   created a durable `webgui / write-cover` coordination row.
2. The running Windows test executable pulled the same event into
   `album_artwork_write_state` with no pending outbox entry.
3. A newer Windows protocol event transferred the durable row to
   `windows / write-cover`. The rendered WebGUI showed `Overridden 1`, a
   grayscale Album thumbnail, red frame, colored OVERRIDE banner, unchanged
   status dot, full-color Selected Artwork, and the `Windows override` notice.
4. PostgreSQL and Windows SQLite retained the same Album key, event ID and
   revision after restarting both applications. Windows outbox count remained
   zero.
5. A real WebGUI Full Refresh reindexed all 210 isolated Albums and left the
   coordination row unchanged.

## PR #152: Cross-format artwork correction and owner acceptance

The owner tested the final PR #152 source head
`c02b56a6b661fc0471028bc721bcd132cdb9c85c` with the isolated E: library.
The installed Windows test executable SHA-256 was
`4E8FD593E87F7DE241DE73EE07A0BA2EA4C7D0F5983E3EB7AADD65F7BD456D5B`.

- WebGUI JPG-to-PNG and Windows PNG-to-JPG writes (and the reverse directions)
  correctly replaced the prior canonical static cover and removed competing
  static duplicates, with no blank Album thumbnails.
- Static cover replacement preserved existing `cover.webp` source artwork;
  an explicitly selected WebP write created its static JPEG still. The owner
  checked the physical source and still files.
- After the selected artwork was staged, embedding produced an actual
  **1200 × 1200** thumbnail in audio-track metadata; this was checked against
  the files, not only the displayed preview.
- Album OVERRIDE writer/revision updates were observed immediately across
  PostgreSQL and Windows SQLite; the visual state and artwork recovered after
  restarts.
- COMP thumbnails and existing track-scoped authority behaved correctly.
- EMBED Batch preserved the established staging lifecycle: purple indicator,
  cleared candidates/Selected Artwork, the Artist/Album workspace remaining
  open, and deferred track embedding until **Start All Staged**.

These are operator-reported installed-client acceptance observations, distinct
from unit tests. Native automated Windows pixel capture remains unavailable in
the current automation desktop (`IGraphicsCaptureItemInterop` cannot capture
the monitor and Slint exposes title-bar accessibility only); no automated
screenshot assertion is claimed. The owner personally verified the GUI and
physical files in the isolated test environment. PR #150's proposed catalog
synchronization and PostgreSQL migrations 018/019 are not part of PR #152.
