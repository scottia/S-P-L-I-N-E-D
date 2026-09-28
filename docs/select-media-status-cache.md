# Retired Select Media JSON status cache

The version-2 file:

```text
/_logs/_history/select-media-status.json
```

is retired in the Python/Docker Ratatui implementation.

It previously stored path-based Album inventory, structural directory
sentinels, and live status values. Warm launches still had to perform
Artist-by-Artist filesystem reconciliation, which caused long load times and
made Artist colors change while the user was already working in Select Media.

SPLINED now uses the tag-identified SQLite read model documented in
[SPLINED media database](splined-media-database.md):

```text
/_cache/splined.db
```

After the first usable database build, an existing JSON file is renamed to:

```text
/_logs/_history/select-media-status.json.legacy
```

It is no longer read or updated. The legacy file may be removed after the new
SQLite index has been verified.

This retirement does not affect the separate completion, chosen-source,
bypass, or timeout history files under `/_logs/_history`.
