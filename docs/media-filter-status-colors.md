# Media Filter and Status Colors

This page documents the Windows GUI Media Filter, live Artist/Album filtering, status-color filters, and the meaning of tree colors in S:P:L:I:N:E:D.

> **Windows target:** v3.0.0 Stable with Config v5.
>
> **Repository note:** exact control labels should be verified against the finalized Windows source when it is integrated. The behavior below is the intended public contract.

---

## Media Filter purpose

Media Filter changes what is shown in the already-loaded library tree. It is a view/selection aid, not a separate scan engine.

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

# Artist and Album text filters

The Media Filter provides live text filtering for:

```text
Artist
Album
```

Text matching is case-insensitive.

Examples:

```text
Artist:  maniacs
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

The Windows GUI supports status-based filtering for the authoritative tree states.

Current status concepts:

| Filter | Color | Meaning |
| --- | --- | --- |
| Unprocessed | White | Album/artist has no retained processed/bypass/timeout authority for normal eligibility |
| Processed | Orange | Album has retained processed history |
| Bypassed | Red | Album has persistent bypass state |
| Partial / Timeout | Purple | Artist is partially processed, or Album is timeout-active depending on row type |
| Artist Complete | Green | All eligible child albums are processed and none bypassed |
| Artist Contains Bypass | Blue | One or more child albums are bypassed |

The same color may have different meaning depending on row type. Purple is the primary example.

---

# Album colors

Album rows use:

| Color | Album meaning |
| --- | --- |
| White | Unprocessed / normally eligible |
| Orange | Processed / history retained |
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

Purple means:

```text
Artist row -> partial aggregate state
Album row  -> timeout-active state
```

These are separate execution meanings.

The GUI should use row context and tooltips/status text to make the distinction clear.

---

# Filtering does not alter selection authority

If a checked album becomes hidden because a text/status filter changes, the application should preserve the underlying state rather than silently rewriting history or eligibility.

The GUI may choose to preserve or clear a transient checkbox selection according to its explicit interaction design, but it must not confuse visibility with persistent execution state.

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

# Select / All / None

The Media Library Selection controls operate on the current authoritative library model and selection rules.

Normal behavior should respect history/bypass/timeout state rather than blindly checking every visible row.

Conceptually:

## Select

Applies the normal eligibility rules:

- White eligible albums can be selected;
- Orange processed albums are skipped by normal automatic selection;
- Purple timeout-active albums remain protected;
- Red bypassed albums require explicit override.

## All

If the GUI exposes an All operation, it should still respect any explicit safety/override rules defined by the application rather than silently deleting bypass/timeout authority.

## None

Clears transient GUI selection only. It should not erase history, bypass, or timeout records.

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

The user is explicitly asking SPLINED to process the album again.

Likewise, a temporary override of a Red album should not silently remove its persistent bypass record.

---

# Scan Bypass controls

The Windows Media Filter area may expose scan-bypass controls such as filtered Read/Write behavior or selection of filtered results.

These controls are operational shortcuts, not replacements for the persistent album bypass state represented by Red.

Do not confuse:

```text
Scan Bypass control
```

with:

```text
Persistent saved album bypass
```

The exact v3.0.0 labels should be documented from the finalized Windows source during integration.

---

# Auto Mode

Auto Mode changes how the current eligible selection proceeds through the existing scan lifecycle.

It must still respect:

- history;
- persistent bypass;
- timeout authority;
- source policy;
- Read/Write mode.

Auto Mode should not make a Red or Purple album eligible merely because automation is enabled.

---

# Tree refresh

Library refresh/reload should recalculate colors from authoritative state.

It should not:

- preserve a stale color after its timeout expires;
- turn a bypassed album White because it was temporarily hidden;
- derive colors solely from previous GUI paint state;
- rebuild unrelated theme state just because the library model changed.

The tree is a presentation of current authority.

---

# Status colors when history is unavailable

If retained history is disabled, expired, missing, or unreadable, SPLINED cannot truthfully show processed/aggregate colors that depend on that history.

The GUI should warn that standard folder presentation is being used or that status coloring is unavailable/reduced.

It should not create a hidden GUI-only history database just to keep Orange/Green/Purple aggregate colors visible.

---

# Filter performance expectations

Artist and Album text filtering should be responsive on large libraries.

Expected implementation behavior:

```text
load library once
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

Filtering should not cause unrelated title bar/theme/log/candidate panels to reconstruct.

---

# Tooltips and status explanations

Tooltips should explain state without requiring users to memorize colors.

Useful examples:

- Orange album: previously processed; manually selectable for reprocessing.
- Red album: persistent bypass; explicit override required.
- Purple album: timeout-active; include remaining/next eligible time where available.
- Purple artist: partial completion across child albums.
- Green artist: all eligible child albums processed.
- Blue artist: contains one or more bypassed albums.

Color must not be the only source of meaning.

---

# Accessibility

Status should be communicated through more than color where practical:

- text/tooltips;
- icons or row context;
- checkbox/selection behavior;
- status descriptions.

This is particularly important for Red/Orange/Green distinctions and the dual use of Purple.

---

# Troubleshooting

## Artist filter returns nothing

Check:

- spelling/substring;
- other active status filters;
- Album filter is not excluding the same rows;
- library model has finished loading.

## Album is Orange but filter says Unprocessed only

This is expected: Orange is processed/history state and is excluded by an Unprocessed-only status filter.

## Artist is Blue but most albums are Orange/White

At least one child album is Red/bypassed. Expand the artist and locate the bypassed album.

## Purple row seems ambiguous

Check row type:

- Artist = partial aggregate;
- Album = timeout-active.

## Status colors disappeared

Check history/retention availability and configured history/log paths.

---

# Relationship to history authority

For the full persistence/selection model, see [History, retention, bypass, and timeout](history-retention-bypass-timeout.md).

Media Filter must consume that authority; it does not replace it.

---

# Related documentation

- [History, retention, bypass, and timeout](history-retention-bypass-timeout.md)
- [Config v5 reference](config-v5-reference.md)
- [Source policies and Range Types](source-policies-range-types.md)
- [Windows GUI walkthrough](windows-gui-guide.md) *(planned)*
