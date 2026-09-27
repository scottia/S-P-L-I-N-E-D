# Select Media status cache

SPLINED keeps a status-only JSON cache at:

```text
<_history>/select-media-status.json
```

With the default Docker paths this is:

```text
/_logs/_history/select-media-status.json
```

The cache accelerates the Artist-status readiness pass and deliberately lives
outside the transient candidate cache so normal run-cache cleanup cannot erase it. It is **not** the
retired SQLite picker cache and is not selection or execution authority.

## Stored data

The JSON contains only the lightweight filesystem inventory needed to derive
Artist and Album status:

- Artist paths;
- discovered Album paths;
- supported audio-file paths;
- local cover-art paths;
- relevant inventory fingerprints;
- directory modification signatures;
- partial/full completion progress.

It does not store tags, MusicBrainz/provider results, candidate rankings,
AISPLINE output, approvals, or final-write decisions.

## First run and interrupted runs

A first run inventories Artist folders normally and writes the JSON atomically
in batches. The centered **BUILDING ALBUM STATUS INDEX** banner identifies this
as a one-time setup pass. If the run is cancelled or interrupted, completed
Artist entries remain available for the next run.

## Later runs

Once the JSON exists, SPLINED shows the same-size **LOADING ALBUM STATUS**
banner while validating the saved inventory against the current library.
The progress bar represents this run's real validation progress; cached coverage
is reported separately and does not falsely force the bar to 100% before
validation finishes. Select Media becomes interactive when that validation pass
finishes:

- unchanged Artists reuse the cached inventory;
- new Artists are inventoried;
- removed Artists are ignored;
- an Artist whose Album/directories changed is inventoried again;
- current completion history, bypass state and timeout policy are still applied
  fresh on every run.

The file may be deleted safely. SPLINED will rebuild it without changing
library files, completion/bypass history, or selection policy.
