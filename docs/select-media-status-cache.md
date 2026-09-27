# Select Media status cache

SPLINED keeps a status-only JSON cache at:

```text
<_cache>/select-media-status.json
```

With the default Docker paths this is:

```text
/_cache/select-media-status.json
```

The cache accelerates the Artist-status readiness pass. It is **not** the
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
in batches. If the run is cancelled or interrupted, completed Artist entries
remain available. The next run begins its loading banner at the retained cache
progress and inventories only uncached or changed Artists.

## Later runs

A complete cache makes Select Media immediately usable. SPLINED validates
cached directory signatures in the normal background status worker:

- unchanged Artists reuse the cached inventory;
- new Artists are inventoried;
- removed Artists are ignored;
- an Artist whose Album/directories changed is inventoried again;
- current completion history, bypass state and timeout policy are still applied
  fresh on every run.

The cache may be deleted safely. SPLINED will rebuild it without changing
library files, history, or selection policy.
