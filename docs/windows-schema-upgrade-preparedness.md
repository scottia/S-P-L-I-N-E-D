# Windows SQLite schema upgrade preparedness

**Reviewed contract:** The v2 → v3 Catalog Index Coordinator upgrade is
dedicated Windows database maintenance, **not** a media refresh or a
PostgreSQL migration. Its previously established operator safeguards
are retained here for source and executable confirmation.

## Operator flow

If the configured `splined.db` is schema v2, Windows enters a
**Prepare This Database Upgrade** maintenance window before creating the
normal workspace. The operator must see the source/target schema
versions, actual database path and size, Artist/Album/Track/history
counts, backup destination, one-way compatibility warning, and the
explicit PostgreSQL exclusion.

**Back Up & Upgrade** begins the operation; while converting, the
window must prevent a second start or unsafe close. **Exit Safely**
is available before conversion starts. **Open SPLINED** is only
allowed after database and backup validation succeeds.

## Preservation and transaction requirements

1. Identify the configured SQLite catalog and confirm supported source
   `user_version = 2`; do not open a different guessed default path.
2. Create an independently restorable SQLite **v2 backup** in
   `schema-upgrade-backups`, using SQLite's online backup mechanism.
3. Verify that snapshot with `PRAGMA quick_check`, schema version and
   original Artist, Album, Track and retained history counts.
4. Acquire the required exclusive SQLite migration transaction and,
   for shared clients, respect configured Windows client-file coordination.
5. Add the v3 coordinator's tables/indexes; preserve existing identities,
   completion/history and the former published inventory as the
   `legacy-pre-coordinator` generation.
6. Confirm all pre-existing catalog/history counts before committing.
   After commit verify v3 `user_version`, `quick_check`, and the
   coordinator's published-generation marker.

A failure before commit must roll back the schema transaction.
Never advertise an unverified backup as usable. A verified v2
backup remains available for manual recovery. The upgrade does
**not** reset the catalog, update audio tags, consume media files,
rewrite saved library inventory signatures, or touch PostgreSQL.

## Downgrade boundary

This is a **one-way migration**. A v2-only executable cannot safely use
the migrated v3 database. To return to a v2 build, close all clients
and restore the verified v2 backup as a database replacement;
**never** change only `PRAGMA user_version` or promise an automatic
downgrade.

Other unsupported schema versions remain unchanged and require
diagnosis, not a guessed conversion. A shared file must not be
migrated while another client writes it.

## Validation status

Source schema and the planned operator contract were previously
documented, but this restored guide does **not** represent a new
Windows native compile or an exercised migration on the current
`dev` head. Before production use, verify a disposable v2 database,
retained history counts, interrupted migration, backup restore,
shared-client isolation and startup screen behavior.

See [Windows SQLite media database](splined-media-database.md)
and the [policy register](POLICY-REGISTER.md).
