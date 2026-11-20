# Windows SQLite schema upgrade preparedness

Windows database schema changes are explicit startup maintenance, not a
library refresh. The first 2.0.36 Catalog Index Coordinator build advances the
native catalog from schema v2 to v3 through an additive, one-way migration.

## What the operator sees

When the configured `splined.db` is schema v2, Windows opens a dedicated
**Prepare This Database Upgrade** window before it constructs the normal
workspace. The window reports:

- the source and target schema versions;
- the exact SQLite path and its size;
- the current Artist, Album, Track, and history counts;
- the directory where the downgrade backup will be written;
- the one-way downgrade warning; and
- the explicit boundary that PostgreSQL is not part of this operation.

Choose **Back Up & Upgrade** to begin. The status changes to **Database
Conversion In Progress** and confirms that conversion has begun. The window
cannot be closed while the conversion thread is active. Choose **Exit Safely**
before starting if storage is unavailable or the maintenance should be
postponed.

The normal SPLINED workspace opens only after the upgraded database passes all
validation and the operator chooses **Open SPLINED**.

## Safety and preservation contract

The upgrade sequence is:

1. Open only the configured Windows SQLite catalog and verify that its schema
   is the supported v2 source.
2. Create a standalone SQLite backup with the online backup API under
   `schema-upgrade-backups` beside `splined.db`.
3. Verify the backup with `PRAGMA quick_check`, its v2 `user_version`, and the
   source Artist/Album/Track/history counts.
4. Acquire an exclusive SQLite transaction. Shared configurations retain the
   configured Windows `.client-lock` for the lifetime of the operation.
5. Apply only the additive v3 tables and indexes, initialize Catalog Index
   Coordinator state, and mark the existing catalog as the
   `legacy-pre-coordinator` published generation.
6. Confirm that all pre-existing Artist, Album, Track, and history counts are
   unchanged before committing.
7. Commit the v3 `user_version`, run `PRAGMA quick_check`, validate the
   coordinator publication marker, and report the verified backup path.

Any failure before commit rolls the schema transaction back. A failed backup
is removed rather than presented as recoverable. A verified backup is retained
if a later validation fails.

The migration does **not** reset SQLite, re-index music, update the saved v2
inventory signature, or access PostgreSQL. The legacy inventory signature is
accepted until the next explicit index refresh writes a current baseline.

## Downgrade and recovery

The migration is one-way. A v2-only executable rejects a v3 database rather
than attempting to downgrade it. To return to the older application, exit all
SPLINED clients and restore the timestamped v2 SQLite snapshot shown in the
completion screen. Do not copy the v2 `user_version` onto a v3 database.

An unsupported schema other than v2 is never converted automatically. Windows
leaves it unchanged and uses the existing Setup/error path for diagnosis.

