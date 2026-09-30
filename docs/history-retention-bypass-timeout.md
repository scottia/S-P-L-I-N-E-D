# History, Retention, Bypass, and Timeout

History is operational authority, not cosmetic UI state.

Completion, bypass, timeout, and chosen-source history must drive the same
eligibility decisions used by scanning and Select Media. The Python/Docker
`splined.db` database materializes those facts for fast indexed display; it does
not replace or invent execution authority.

Conceptually:

```text
persistent history + actual local artwork + current timeout policy
                            ↓
                     execution authority
                            ↓
                 splined.db Select Media view
                            ↓
                      TUI Album Status
```

## Persistent history and cache data

Typical Python/Docker layout:

```text
SPLINED data/
├── _cache/
│   ├── splined.db              persistent Select Media read model
│   ├── samples/                disposable
│   └── candidate files         disposable
└── _logs/
    └── _history/
        ├── chosen-source-history.json
        ├── scan-completed-history.json
        └── bypass-source-history.json
```

Deleting candidate/sample cache must not delete history or bypass decisions.
Deleting `splined.db` does not delete execution history or music files, but the
next Python/Docker TUI launch must rebuild the full tag-identified media index.

## Processed history

A successfully processed Album with retained completion authority is shown as:

```text
ORANGE
```

Orange Albums are excluded from normal automatic selection but remain manually
selectable for deliberate reprocessing.

Processed does not mean the artwork can never change. It means SPLINED retains
a successful prior result and normal automatic behavior respects that result.
Recognized local `cover.*` can also provide processed presentation without
fabricating a completion timestamp.

Read mode evaluates candidates but does not create durable processed authority
for an Album that still has no installed cover.

## Retention

Retention controls how long applicable history remains authoritative. When a
completion record expires, the Album becomes eligible according to remaining
facts such as local artwork, bypass, or timeout.

The SQLite Select Media database is projected against retention at warm startup.
It does not keep an expired completion state alive merely because an older row
was previously Orange.

## Bypass

Persistent bypass is represented as:

```text
RED
```

A Red Album is not normally auto-selected. Processing requires deliberate
removal of the saved bypass through the supported confirmation. Removing a
bypass is distinct from a temporary command-line override.

At physical Artist-folder level, any bypassed child produces:

```text
BLUE
```

## Timeout

A timeout-active Album is represented as:

```text
PURPLE
```

Timeout prevents immediate automatic reprocessing while the configured window
is authoritative. If timeout is disabled or zero, an Album must not remain
Purple solely because a stale timestamp exists.

Timeout is recalculated when the database is loaded. No filesystem sentinel
crawl is required merely to update clock-based eligibility.

## Album colors

| Album color | Meaning | Normal auto-selection |
| --- | --- | --- |
| White | Unprocessed / no active retained state | Yes |
| Orange | Processed/history or recognized local cover | No |
| Red | Persistent bypass | No |
| Purple | Timeout active | No |

Manual reprocessing can differ where an explicit supported action allows it.

## Physical Artist-folder aggregate colors

| Artist-folder color | Meaning |
| --- | --- |
| White | All eligible Albums unprocessed, no bypass |
| Purple | Mixed processed/unprocessed/timeout state, no bypass |
| Green | All eligible Albums processed/timeout-protected, no bypass |
| Blue | One or more child Albums bypassed |

Precedence:

```text
BLUE   → any bypass exists
GREEN  → all eligible Albums processed or timeout-protected
PURPLE → mixed state, no bypass
WHITE  → all unprocessed, no bypass
```

Purple therefore has two context-dependent meanings:

```text
Album row  → timeout active
Artist row → partial aggregate
```

## Stable TUI presentation

The Python/Docker TUI loads complete physical Artist-folder/Album/status rows
from `splined.db` before the first Select Media frame. Tagged Artist authority
rows may differ in number from visible physical folders. Opening an Artist
folder does not cause a folder scan or status-color transition.

Colors change during a session only after:

- a visible SPLINED processing result;
- adding or removing bypass;
- timeout/history projection on a new launch;
- an explicit media-index Refresh.

External tagger or filesystem changes are reconciled by the
explicit Refresh. The Refresh progress screen completes before the new model is
shown, preventing progressive Artist color changes while the user works.

## Manual reprocessing

Manual reprocessing is not the same as deleting history.

- Orange Album: may be explicitly selected for another pass.
- Red Album: saved bypass must be deliberately removed or explicitly overridden
  by a supported command-line mode.
- Purple Album: timeout remains authoritative unless an explicit supported
  override exists.

## History disabled

When persistent history is disabled or expired, SPLINED cannot truthfully infer
previous processing from history alone. Actual local artwork and separately
persisted bypass facts may still contribute.

The database must not fabricate completion authority merely to retain a color.
It stores the current read model and is reprojected from available authority on
startup.

## Backup

Back up:

```text
config/
credentials/
_logs/_history/
```

For Python/Docker, also back up:

```text
<scan.cache_dir>/splined.db
```

Stop SPLINED before a raw database copy if consistent WAL state is required.

## Moving a Python/Docker installation

1. stop SPLINED;
2. copy config, credentials, history, and `splined.db`;
3. update Config v5 paths and Docker bind mounts;
4. launch and verify the library root;
5. run explicit Refresh if music paths changed;
6. verify Album Status before a LIVE WRITE batch.

Paths are mutable locations, not SQL identity. Albums with stable MusicBrainz or
fallback tag identity can retain the same logical row after Refresh updates the
location.

## Troubleshooting

### Everything is White

Check:

- history is enabled and not expired;
- the expected history directory is mounted/readable;
- `splined.db` was built from the intended library;
- configured `cover.*` naming matches local files;
- an explicit Refresh has been run after external changes.

### Album stays Purple

Check timeout duration, system time, stored completion time, and policy
fingerprint. Restarting reloads and reprojects timeout state from the database
and history.

### Artist shows Blue

At least one child Album in that physical folder is bypassed. Open the folder
and locate the Red row.

### Status changes only after Refresh

That is expected for external library changes. Warm startup and ordinary Artist
selection do not crawl Album folders. SPLINED's own writes and bypass actions
update affected rows immediately.

## Data integrity

History and database updates should be atomic and must not:

- truncate unrelated records;
- treat candidate-cache deletion as history deletion;
- overwrite saved bypass decisions during ordinary completion;
- use absolute paths as Artist or Album identity;
- advertise a partially refreshed picker as complete.

An explicit Refresh of an established media index uses one replacement SQLite
transaction, and the TUI displays the replacement model only after that
transaction succeeds. The initial index build instead uses resumable Album
checkpoints and publishes the active-snapshot marker only after validation.

## Related documentation

- [SPLINED media database](splined-media-database.md)
- [Select Media and status colors](media-filter-status-colors.md)
- [Config v5 reference](config-v5-reference.md)
- [Python Ratatui TUI](ratatui-tui.md)
