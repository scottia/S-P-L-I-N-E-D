# Select Media and Status Colors

This page documents the Windows GUI **Select Media** control, live Artist/Album filtering, status filters, tree colors, and the required Python Ratatui parity model.

> **Windows target:** v3.0.0 Stable with Config v5.

---

## Select Media purpose

**Select Media** is the single collapsible control beneath Media Library Selection. It changes the visible in-memory tree and provides selection and filtered-scan shortcuts; it is not a separate scan engine.

The filter should operate against the in-memory library model and should not rescan the filesystem on every keystroke.

Filtering must not silently change:

- Config v5;
- persistent history;
- bypass state;
- timeout state;
- provider credentials;
- candidate/source policy.

A row filtered out of view remains part of the underlying library model unless another explicit operation changes it.

---

# Windows-equivalent folder inventory authority

The Python Ratatui path preserves the Windows folder/status model through a
complete persistent picker snapshot suitable for large NAS libraries. The
first cold run inventories lightweight folder topology into
`<cache>/splined-picker.sqlite3`. A valid warm run reads that complete snapshot
and renders Select Media before any music-library filesystem access.

Required behavior:

```text
load COMPLETE SQLite generation
        ↓
apply retained history / bypass / timeout state
        ↓
Select Media becomes usable
        ↓
validate a new staging generation in the background
        ↓
atomically promote coherent changes
```

This inventory is intentionally different from album processing. It does not read audio tags, perform MusicBrainz authority lookup, discover providers, download artwork, decode artwork to determine geometry, rank candidates, or call AISPLINE. Those operations begin only after the user launches the selected albums.

The picker SQLite database is disposable acceleration state, not status
authority. Folder names remain the stable Select Media identity. No pre-Launch
Mutagen enrichment rewrites Artist or Album rows. `Auto Scan [ALL]` and the
explicit Refresh Library Index action force a complete validation/rebuild.
Incomplete generations are never exposed as a complete library.

## Ignored/excluded folders

`[library].ignored_subs` is authoritative during recursive library inventory.

Ignored exact names/patterns:

- are not traversed;
- do not appear as Artist rows;
- do not appear as Album rows;
- do not contribute to library statistics;
- do not participate in status/history reconciliation;
- do not appear in selection payloads.

Wildcard behavior should remain compatible with current SPLINED/Windows matching. Symlink/reparse-style recursive loops are not followed.

POSIX/Linux directory basenames beginning with `.` are always excluded at the
root and during nested traversal. They never become Artist/Album rows, affect
counts or status, enter selection payloads, or persist in the picker index.
This automatic filesystem rule is additional to `[library].ignored_subs` and
does not rewrite Config v5.

---

# Artist and Album text filters

Select Media provides live text filtering for:

```text
Artist
Album
```

Text matching is case-insensitive.

Examples:

```text
Artist: maniacs
```

can match:

```text
10,000 Maniacs
```

and:

```text
Album: ruins
```

can match:

```text
Love Among the Ruins
```

Artist and Album filters combine with each other and with status filters.

---

# Status filters

Current status concepts:

| Filter | Color | Meaning |
| --- | --- | --- |
| Unprocessed | White | Album/artist has no retained processed/bypass/timeout authority for normal eligibility |
| Processed | Orange | Album has retained processed history or Windows-equivalent detected local artwork state |
| Bypassed | Red | Album has persistent bypass state |
| Partial / Timeout | Purple | Artist is partially processed, or Album is timeout-active depending on row type |
| Artist Complete | Green | All eligible child albums are processed and none bypassed |
| Artist Contains Bypass | Blue | One or more child albums are bypassed |

The same color may have different meaning depending on row type. Purple is the primary example.

---

# Album colors

| Color | Album meaning |
| --- | --- |
| White | Unprocessed / normally eligible |
| Orange | Processed / history retained or local artwork reconciled by Windows-equivalent inventory |
| Red | Bypassed |
| Purple | Timeout-active |

Normal selection behavior follows the same authority:

- White -> auto-selectable;
- Orange -> not auto-selected, but may be manually reprocessed;
- Red -> explicit bypass override required;
- Purple -> protected while timeout remains active.

---

# Artist colors

Artist rows summarize child-album state.

| Color | Artist meaning |
| --- | --- |
| White | All eligible albums unprocessed, none bypassed |
| Purple | Mixed processed/unprocessed state, none bypassed |
| Green | All eligible albums processed, none bypassed |
| Blue | At least one child album bypassed |

Artist aggregate precedence:

```text
BLUE   -> any bypass exists
GREEN  -> all eligible albums processed, no bypass
PURPLE -> mixed processed/unprocessed, no bypass
WHITE  -> all eligible albums unprocessed, no bypass
```

Blue therefore has priority when an artist contains one or more Red albums.

---

# Purple is context-sensitive

```text
Artist row -> partial aggregate state
Album row  -> timeout-active state
```

These are separate execution meanings. UI row context/status text should make the distinction clear.

---

# Filtering does not alter selection authority

If a checked album becomes hidden because a text/status filter changes, the application should preserve the underlying state rather than silently rewriting history or eligibility.

In particular:

```text
hidden by filter != bypassed
hidden by filter != processed
hidden by filter != timeout-active
```

---

# Combining filters

Filters combine as constraints over the same loaded tree.

Example:

```text
Artist contains: 10,000
Status: Processed + Unprocessed
```

shows matching rows allowed by those active filters.

Status filters should work together with Artist/Album text filters without triggering a full filesystem reload.

---

# Select Mode

The Select Mode group contains mutually exclusive selection actions:

- **Select [ALL]** selects normally eligible Albums across the complete active
  snapshot (after validation when required);
- **Select [NONE]** clears transient selection;
- **Select [FILTERED]** selects Albums in the complete current in-memory filter
  result.

Selection actions respect history, bypass, and timeout authority and never erase persistent records.

---

# Selecting an Artist

Selecting an Artist normally cascades to eligible child albums.

Expected automatic behavior:

```text
White Album  -> selected
Orange Album -> skipped unless manually reselected
Purple Album -> skipped while timeout-active
Red Album    -> requires explicit bypass override
```

A Blue artist can therefore be selected without automatically overriding its Red child albums.

---

# Manual reprocessing

Orange albums may be deliberately reselected for another processing pass.

Manual selection does not mean the old completion history must be erased first.

Likewise, a temporary override of a Red album should not silently remove its persistent bypass record.

---

# Scan Mode

The Scan Mode group contains mutually exclusive:

- **Filtered Scan [READ]**;
- **Filtered Scan [WRITE]**.

These are operational shortcuts for the current filtered selection, not replacements for persistent album bypass.

Launching a filtered scan intersects the visible filter result with albums that
are already checked. It must not expand the run to other visible eligible
albums. **AUTO LAUNCH** likewise processes only the checked Album set; selecting
an Artist or using **Select [FILTERED]** is what changes that set.

# Auto Mode

**AUTO LAUNCH** starts the existing launch workflow for the eligible selection.

It must still respect history, persistent bypass, timeout authority, source policy, and Read/Write mode.

---

# Tree refresh

Library refresh/reload should recalculate colors from authoritative state.

It should not preserve stale colors, derive colors solely from previous paint state, or rebuild unrelated theme state just because the library model changed.

The tree is a presentation of current authority.

## Count scopes

Album status suffixes derive from every Album in the complete active snapshot:
Unprocessed, Processed, Bypass, and Timeout are complete-library counts after
authoritative history reconciliation. Artist Complete and Artist Contains
Bypass derive from the complete Artist population. Statistics show complete
Artist/Album totals, exact selection, active-Artist detail, cached local-art
format counts, and optional validation progress. There is no normal
`inventory not loaded` state after a complete snapshot exists.

At batch completion, transient Activity becomes a scrolling per-Album final
run report and pauses indefinitely. Enter or Esc returns to the same in-memory
Select Media model; affected history/status is reconciled without a SQLite
reload, root reconciliation, or full-library rescan.

---

# Status colors when history is unavailable

If retained history is disabled, expired, missing, or unreadable, SPLINED cannot truthfully show processed/aggregate colors that depend on that history.

The UI should warn that status coloring is unavailable/reduced rather than create a hidden UI-only history database.

---

# Filter performance expectations

Artist and Album text filtering should be responsive on large libraries.

Expected implementation behavior:

```text
perform one lightweight Windows-equivalent library inventory
-> build in-memory model
-> apply text/status filters in memory
```

Avoid:

```text
each keystroke
-> rescan filesystem
-> rebuild history
-> reload entire application shell
```

Also avoid performing normal album processing before Select Media becomes usable.

Filtering should not cause unrelated title/theme/log/candidate panels to reconstruct.

---

# Python Ratatui parity

The Python Ratatui library-selection workspace should follow the same inventory, status, filter, and selection authority described above rather than invent a separate TUI model.

Required parity direction:

- Windows-equivalent lightweight pre-selection filesystem/history inventory;
- authoritative `[library].ignored_subs` exclusion during traversal;
- no tag/MusicBrainz/provider/candidate/AI work before Launch;
- live Artist and Album filter boxes;
- filtering begins as text is entered;
- filtering operates against the loaded in-memory model;
- the same White/Orange/Red/Purple/Green/Blue meanings and artist precedence;
- artist selection cascades only to eligible child albums;
- filtered READ/WRITE and selection shortcuts use existing SPLINED execution policy;
- status filtering never rewrites history/bypass/timeout state;
- direct mouse/touch interaction for rows, checkboxes, filter focus, source policy, candidate actions, and URL where supported;
- true scrolling for Artist, Album, policy, and candidate regions;
- keyboard remains a complete fallback.

The user's WebSSH iOS terminal is a verified target for touch interaction: the prior SPLINED `--tui` accepted touches that moved result selection and supported touch/gesture scrolling. The current loss of that behavior is therefore a regression in the new binding/application path, not an unverified terminal capability.

The published `pyratatui==0.3.0` wheel does not expose crossterm mouse capture
or `MouseEvent`. Python SPLINED supplies that missing input surface through the
small `splined-pyratatui-input==0.1.0` PyO3 extension while keeping the
published pyratatui renderer unchanged. It uses render-time hit rectangles and
normal crossterm mouse capture/events; it does not parse raw escape sequences
or rewrite SPLINED in Rust.

OLED and CHALK may render these states with different visual intensity, but state meaning and precedence remain unchanged.

The TUI should also expose the practical source-policy configuration needed to reduce unwanted candidate clutter before expensive download/AI review, using Config v5 source-policy authority rather than presentation-only filtering.

---

# Mouse/touch interaction contract

When mouse events are available, direct hit-testing should support:

- Artist rows and checkboxes;
- Album rows and checkboxes;
- Artist/Album filter fields;
- Album Status controls;
- Select ALL/NONE/FILTERED;
- READ/WRITE/Auto scan controls;
- Source Policy Settings and source-policy fields;
- candidate rows;
- `AI ENHANCED` controls when present;
- `[URL]`;
- confirmation dialogs;
- scrollable list regions.

Hit regions should be derived from render-time rectangles/rows rather than inferred later from text content. A direct tap on an explicit checkbox/button should perform that action without requiring a second Enter press.

Mouse wheel or terminal-provided touch scrolling should move the list under interaction and must not toggle selection merely because the list scrolled.

---

# Tooltips and status explanations

Useful explanations include:

- Orange album: previously processed; manually selectable for reprocessing.
- Red album: persistent bypass; explicit override required.
- Purple album: timeout-active; include remaining/next eligible time where available.
- Purple artist: partial completion across child albums.
- Green artist: all eligible child albums processed.
- Blue artist: contains one or more bypassed albums.

Color must not be the only source of meaning.

---

# Accessibility

Status should be communicated through more than color where practical: text/tooltips, icons or row context, checkbox/selection behavior, and status descriptions.

---

# Troubleshooting

## Artist filter returns nothing

Check spelling/substring, other active status filters, Album filtering, and whether the lightweight library inventory has finished loading.

## Album is Orange but filter says Unprocessed only

This is expected: Orange is processed/history state and is excluded by an Unprocessed-only status filter.

## Artist is Blue but most albums are Orange/White

At least one child album is Red/bypassed.

## Purple row seems ambiguous

Check row type:

- Artist = partial aggregate;
- Album = timeout-active.

## Status colors disappeared

Check history/retention availability and configured history/log paths.

---

# Relationship to history authority

For the full persistence/selection model, see [History, retention, bypass, and timeout](history-retention-bypass-timeout.md).

Select Media consumes that authority; it does not replace it.

---

# Related documentation

- [History, retention, bypass, and timeout](history-retention-bypass-timeout.md)
- [Config v5 reference](config-v5-reference.md)
- [Source policies and Range Types](source-policies-range-types.md)
- [Python Ratatui TUI](ratatui-tui.md)
- [Documentation home](README.md)
