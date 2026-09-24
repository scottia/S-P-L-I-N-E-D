# History, Retention, Bypass, and Timeout

This page explains the persistent execution-state model used by S:P:L:I:N:E:D and how the Windows GUI derives album/artist status from the same authority used by the scanner.

> **Windows target:** v3.0.0 Stable with Config v5.

---

## Core principle

History is operational state, not cosmetic GUI state.

The Windows GUI must derive its status colors, selection behavior, bypass protection, and timeout behavior from the same persistent execution authority used by SPLINED itself.

It should not maintain a second independent persistent GUI status database.

Conceptually:

```text
persistent history / bypass / timeout authority
                  ↓
             scan engine
                  ↓
               GUI tree
```

---

# Persistent history vs disposable cache

SPLINED separates persistent state from disposable scan/cache data.

Typical portable layout:

```text
SPLINED/
├── _cache/
│   └── ... disposable candidate/sample data
└── _logs/
    └── _history/
        └── ... persistent history files
```

The current Python/Docker runtime documents history files including:

```text
chosen-source-history.json
scan-completed-history.json
bypass-source-history.json
```

Windows v3.0.0 may serialize additional or evolved state as required by Config v5 behavior, but the separation remains the same:

```text
_cache        -> disposable
_logs/_history -> persistent operational state
```

Deleting cache should not be treated as deleting scan history or bypass decisions.

---

# Processed history

When an album has been successfully processed and history retention is enabled, SPLINED can remember that completion state.

The Windows GUI represents a processed/history album as:

```text
ORANGE
```

An Orange album is not automatically selected again during normal selection, but it can be manually reselected for deliberate reprocessing.

Processed does not necessarily mean:

- the artwork can never change;
- the album is permanently excluded;
- its local artwork is guaranteed to remain unchanged forever.

It means SPLINED has retained a successful prior processing state and normal automatic selection respects that state.

---

# Retention

Retention determines how long applicable history remains authoritative.

When retained history is still valid, SPLINED can avoid repeatedly treating already-processed albums as new work.

When history is disabled, expired, or otherwise unavailable, the GUI cannot truthfully show execution-state colors based on history it no longer has.

In that condition, the GUI should fall back to ordinary folder/tree presentation and should warn that status coloring is unavailable or reduced.

Retention should not silently create a second GUI-only state store merely to keep colors visible.

---

# Bypass

A bypass is a persistent instruction to skip/protect an album from normal processing.

Album-level bypass state is represented as:

```text
RED
```

A Red album is not normally auto-selected.

To process it, the user must deliberately override the bypass through the supported prompt/control.

A temporary override must not silently delete the persistent bypass record.

Conceptually:

```text
saved bypass = ON
       ↓
normal selection skips album
       ↓
user explicitly overrides for this run
       ↓
album may run
       ↓
saved bypass still exists afterward
```

Removing the bypass permanently is a separate intentional action.

---

# Timeout

Timeout protects recently processed or otherwise timeout-governed albums from being immediately reprocessed.

A timeout-active album is represented as:

```text
PURPLE
```

Album-level Purple means **timeout-active**.

The GUI should show the remaining timeout and/or next eligible time where practical.

A timeout-active album should not be auto-selected while the timeout is authoritative.

If timeout is disabled or configured to zero, an album must not remain Purple merely because it once had a timeout timestamp.

---

# Album status colors

| Album color | Meaning | Normal auto-selection |
| --- | --- | --- |
| White | Unprocessed / no authoritative retained state | Yes |
| Orange | Processed/history retained | No |
| Red | Persistent bypass | No |
| Purple | Timeout-active | No |

Manual actions can differ from normal auto-selection where the GUI explicitly allows deliberate reprocessing/override.

---

# Artist aggregate colors

Artist rows summarize the state of eligible child albums.

| Artist color | Meaning |
| --- | --- |
| White | All eligible albums are unprocessed and none are bypassed |
| Purple | Mixed processed/unprocessed state, no bypass |
| Green | All eligible albums are processed, no bypass |
| Blue | One or more child albums are bypassed |

Aggregate precedence:

```text
BLUE   -> any bypassed album exists
GREEN  -> all eligible albums processed, none bypassed
PURPLE -> mixed processed/unprocessed, none bypassed
WHITE  -> all eligible albums unprocessed, none bypassed
```

Blue therefore takes precedence over otherwise complete/partial states when a child bypass exists.

---

# Two meanings of Purple

Purple is intentionally context-sensitive:

```text
Album row  -> timeout-active
Artist row -> partially processed aggregate
```

These are separate internal meanings even though the same visual color is used.

The GUI should use row context/tooltips to avoid ambiguity.

---

# Selection behavior

Selecting an Artist normally follows authoritative child state.

Expected behavior:

- White/unprocessed albums -> auto-selectable;
- Orange/processed albums -> not auto-selected, but manually selectable for reprocessing;
- Purple/timeout-active albums -> protected while timeout is active;
- Red/bypassed albums -> require explicit bypass override.

Selecting a Blue artist should not silently override its Red child albums. The bypass prompt/override path remains explicit.

---

# Library load and refresh

When the library is loaded or refreshed, SPLINED should derive status from:

- album folder/local state where relevant;
- retained completion history;
- bypass history;
- timeout authority.

A reload should not manufacture new status records merely to reproduce the previous color.

The tree is a view of authoritative state.

---

# History expiration

When retained completion state expires according to configured policy, the album becomes eligible according to the remaining authorities.

For example, an expired completion record may result in White/unprocessed behavior unless:

- a bypass is still active;
- a timeout is still active;
- another authoritative rule applies.

Expiration of completion history should not automatically erase a bypass unless the configured data model explicitly ties those records together.

---

# Bypass and timeout precedence

Bypass is a stronger explicit protection state than ordinary processed history.

At the artist aggregate level, any bypass produces Blue.

At the album level, persistent bypass should be shown as Red rather than Orange processed state.

Timeout is also distinct from processed history; a timeout-active album should show Purple while the timeout remains active.

Where multiple records exist, the GUI should display the state that controls current eligibility rather than whichever file was read last.

---

# Manual reprocessing

Manual reprocessing is intentionally different from clearing history.

For an Orange album, manual selection can request another processing pass while retaining the fact that the album was previously processed.

For a Red album, an explicit temporary bypass override can allow a run without deleting the saved bypass.

For a Purple timeout-active album, the configured timeout rules remain authoritative; any manual override must be a deliberate supported action rather than an accidental side effect of tree selection.

---

# History disabled

If persistent history is disabled:

- SPLINED cannot reliably distinguish previously processed albums using history alone;
- Orange/Green/Purple-partial aggregate presentation may be unavailable or reduced;
- the GUI should explain that standard folder colors are being used instead;
- the GUI must not secretly create its own persistent status database to compensate.

Bypass data may still be separately authoritative if bypass persistence remains enabled in the implementation.

---

# Backing up history

If you want to preserve scan state when moving/reinstalling SPLINED, back up:

```text
_logs/_history/
```

along with:

```text
config/
credentials/
```

`_cache/` can normally be regenerated and should not be relied on as the authoritative history store.

---

# Moving an installation

When moving a portable installation:

1. close SPLINED;
2. copy the application plus `config/`, `credentials/`, and `_logs/_history/`;
3. update paths in Config v5 if the library/cache/log locations changed;
4. launch and verify the library root;
5. confirm status colors/history before a Write-mode run.

SPLINED should not automatically discover and import another installation merely because one exists elsewhere.

---

# Troubleshooting

## Everything is White after restart

Check:

- history is enabled;
- retention has not expired;
- `_logs/_history/` exists and is readable;
- the configured history/log location is the expected one;
- the library path did not change in a way that prevents identity matching.

## Album stays Purple forever

Check:

- timeout is enabled;
- configured timeout duration is non-zero;
- system clock/time is correct;
- stored timestamp can be parsed;
- reload recalculates remaining eligibility rather than persisting a cosmetic Purple flag.

## Red album runs without confirmation

This is not expected normal behavior. Verify that a temporary/global bypass override was not intentionally enabled.

## Bypass disappeared after one manual run

A temporary override should not remove the saved bypass. Verify that the action used was an override rather than a permanent bypass removal.

## Artist shows Blue

At least one eligible child album is bypassed. Expand the artist and locate the Red album(s).

---

# Data integrity rules

Persistent state updates should be safe against partial writes where practical.

History/bypass updates should not:

- truncate unrelated records;
- recreate files from incomplete in-memory subsets;
- treat cache deletion as history deletion;
- replace user-authoritative bypass decisions during ordinary scan completion.

In the interactive Ratatui mode, these stores are reconciled again after each
batch before returning from `LAST RUN SUMMARY` to Select Media. The disposable
picker SQLite topology is retained, but it never becomes processed/bypass/
timeout authority. Multiple batches may run in one TUI process; attempted
selection is cleared while history/status and the session's cumulative failure
exit state are retained.

---

# Related documentation

- [Select Media and status colors](media-filter-status-colors.md)
- [Config v5 reference](config-v5-reference.md)
- [Installation and first run](installation-first-run.md)
- [Installation and first run](installation-first-run.md)
