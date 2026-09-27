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
- compact Artist-root and first-level structural modification sentinels;
- deep directory signatures retained for reinventory/debugging;
- live Album folder status (`unprocessed`, `processed`, `bypassed`, or `timeout`);
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
validation finishes.

Later-run validation uses a two-tier filesystem path. SPLINED first checks the
Artist root plus its immediate structural/category directories. An unchanged
sentinel set reuses that Artist's cached Album inventory without statting every
cached Album directory. A changed/new Artist falls back to the authoritative
deep inventory and rewrites only that Artist's cache entry.

Album status transitions emitted by the resident Select Media session are
written atomically to the JSON immediately. They are not deferred until process
exit. This keeps the persisted status snapshot synchronized with processing,
bypass and timeout-visible state before a relaunch. Completion history, bypass
authority and timeout policy are still applied fresh by the engine; the JSON
does not become execution authority.

At the end of validation SPLINED reports instrumentation in the form:

```text
Album status cache: 1,031 Artists checked · 1,026 unchanged · 5 changed · 5 rescanned · validation: 2.4s
```

New Artists are inventoried, removed Artists are ignored, and changed structural
sentinels trigger reinventory.

The file may be deleted safely. SPLINED will rebuild it without changing
library files, completion/bypass history, or selection policy.
