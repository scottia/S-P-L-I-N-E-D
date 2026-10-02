# Retention, bypass, and timeout state

All durable runtime state is stored in `<scan.cache_dir>/splined.db`. SPLINED
does not maintain a separate history directory or JSON history files.

## SQLite authority

- `albums.status` is the current Album state: `unprocessed`, `incomplete`,
  `processed`, `bypassed`, or `timeout`.
- `albums.processed_at` records the most recent completed Write operation.
- `albums.bypassed` records an explicit persistent bypass.
- `albums.timeout_until` records when a timeout Album becomes eligible again.
- `albums.selected_source` records the selected artwork source.
- `cache_entries` stores auxiliary state such as source-selection ranking and
  scan fingerprints.
- `cache_history` is the append-only audit surface for runtime changes.

Python owns shared-library indexing. Python and Windows may both update Album
runtime state when `[scan].sqlite_shared = true`; shared mode uses rollback
journaling and a 30-second busy timeout.

`cover_found`, `cover_path`, and `local_art_json` describe artwork currently in
the Album folder. They do not create Processed authority. Only an explicit
SPLINED completion recorded through runtime history/provenance marks an Album
Processed, so an operator-added or third-party `cover.*` remains Unprocessed.

## Retention policy

`[history].enabled` controls whether completed runtime outcomes influence later
selection. `[history].retention_days = 0` retains them indefinitely; a positive
value makes older completion state eligible for pruning/reprocessing.

Diagnostic log retention is independent. Deleting `_logs/run` never changes
Album state.

## Bypass and timeout

Bypass is explicit and remains active until the operator clears it. A timeout
is temporary and applies only until `timeout_until`. An incomplete manual
compilation remains incomplete so it can resume without being presented as a
successfully completed Album.

## Backup

Preserve:

```text
config/
credentials/
<scan.cache_dir>/splined.db
```

Candidate files, samples, and diagnostic logs are disposable.
