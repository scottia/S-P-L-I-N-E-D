# Select Media and Status Colors

Select Media is the operational library workspace. Text filters, Album Status
filters, selection controls, scan scope, and Launch all operate on one resident
Artist/Album model. Filtering changes visibility only; it does not rewrite
history, bypass, timeout, credentials, or source policy.

The Windows GUI and Python/Docker Ratatui interfaces share the same semantic
colors, although their storage and presentation implementations differ.

## Python/Docker inventory authority

The Python/Docker TUI loads the complete Artist/Album read model from:

```text
<scan.cache_dir>/splined.db
```

The index is populated from Mutagen tags and local artwork inspection. Artist
and Album identity uses MusicBrainz/tag keys; paths are current locations, not
SQL identity indexes.

The visible Artist Picker uses physical ownership, not authority-row count.
Each Album belongs to the first directory beneath the configured library root.
For example, `/music/Christina Aguilera/AGUILERA` appears under `Christina
Aguilera`, while `/music/[Soundtracks]/A Star Is Born Soundtrack` appears once
under `[Soundtracks]`. Nested Various Artists layouts collapse to their
top-level `/music/[Various Artists]` folder.

Warm startup is database-only for Select Media. Opening an Artist, typing a
filter, scrolling, or changing focus does not scan the filesystem or change a
row's status.

External tagger or filesystem changes are incorporated by
the explicit Refresh action. Refresh completes its inventory/tag transaction
before replacing the visible model, so colors do not progressively change
while the user is working.

See [SPLINED media database](splined-media-database.md).

## Ignored and hidden folders

`[library].ignored_subs` is authoritative during the initial database build and
explicit Refresh. Matching directories:

- are not traversed;
- do not appear as Artist or Album rows;
- do not contribute to counts;
- do not enter selection payloads.

POSIX directory basenames beginning with `.` are excluded automatically.
Symlink/reparse-style loops are not followed.

## Artist and Album text filters

Artist and Album filters are case-insensitive and combine with each other and
with Album Status filters.

Examples:

```text
Artist: maniacs
Album:  ruins
```

can match:

```text
10,000 Maniacs
Love Among the Ruins
```

Filtering is performed against the resident model. Keystrokes do not trigger
filesystem, Mutagen, MusicBrainz, provider, image, or AI work.

## Album Status panel order

The top-left `S:P:L:I:N:E:D ALBUM STATUS` panel uses seven consecutive rows:

```text
Unprocessed
Incomplete
Processed
Bypassed
Partial / Timeout
Artist Complete
Artist Contains Bypass
```

The bullet/label uses the semantic status color. The count is right-aligned in
a fixed second column. Focus, hover, and selection emphasis must not replace the
underlying status color.

## Album states

| Color | Album meaning | Normal automatic selection |
| --- | --- | --- |
| White | Unprocessed / no current processed authority | Yes |
| Blue | Manual Comp LIVE WRITE completed some, but not all, tracks | No |
| Orange | Processed/history or recognized canonical local cover | No |
| Red | Persistent bypass | No |
| Purple | Timeout active | No |

Blue Incomplete Albums may be selected explicitly to resume Manual Scan; they
are excluded from normal automatic selection. Orange Albums may be deliberately
selected for reprocessing. Red Albums require intentional saved-bypass removal.
Purple Albums remain protected while timeout is active.

READ mode may evaluate a candidate, but a no-cover Album does not become
durably processed merely because a possible image was found. LIVE WRITE updates
the database after the actual Album result and local `cover.*` state are known.

## Physical Artist-folder aggregate states

| Color | Physical Artist-folder meaning |
| --- | --- |
| White | All eligible Albums unprocessed, no bypass |
| Purple | Mixed processed/unprocessed/timeout state, no bypass |
| Green | All eligible Albums processed or timeout-protected, no bypass |
| Blue | One or more child Albums bypassed |

Precedence:

```text
BLUE   → any bypass exists
GREEN  → every eligible Album is processed or timeout-protected
PURPLE → mixed state without bypass
WHITE  → all eligible Albums are unprocessed
```

Purple is context-sensitive:

```text
Album row  → timeout active
Artist row → partial aggregate
```

Blue is also context-sensitive:

```text
Album row  → incomplete Manual Comp progress
Artist row → contains one or more bypassed child Albums
```

Color should be accompanied by row context and status text where practical.

## Counts

Album Status counts are exact for the resident SQLite model. Artist counts are
physical top-level picker folders, not tagged authority identities:

```text
Unprocessed            = White Albums / White Artists for shared context
Incomplete             = Blue Albums with started, unfinished Manual Comp progress
Processed              = Orange Albums
Bypassed               = Red Albums
Partial / Timeout      = Purple Albums + Purple Artists
Artist Complete        = Green Artists
Artist Contains Bypass = Blue Artists
```

The count is not the current selection count. Selection count appears in Album
Selection as `SELECTED [n]`.

## Initial and direct selection

Album Selection defaults to:

```text
Select [NONE]
```

A direct Album-row click is exclusive: focus moves to that Album, the selected
count stays at one, and the right-side artwork/statistics follow that Album.

Bulk actions are explicit:

- **Select [ALL]** — eligible Albums for the active Artist;
- **Select [FILTERED]** — eligible Albums matching active text filters;
- **Select [NONE]** — clear transient selection.

A row hidden by a filter remains in the underlying model and does not become
processed, bypassed, or timeout-active merely because it is hidden.

## Selecting a physical Artist folder

Selecting an Artist cascades only to eligible children:

```text
White Album  → selectable
Orange Album → skipped by automatic selection; manually reprocessable
Purple Album → protected while timeout active
Red Album    → saved bypass removal required
```

A Blue Artist can therefore be selected without silently including Red child
Albums.

## Album Scanning and Launch

Album Scanning defines scope:

```text
Auto Scan [ALL]
Auto Scan [SELECTED]
```

Launch defines mutation mode:

```text
Launch [READ] Source Results
Launch [LIVE WRITE] Choice Results
```

Auto Scan without an explicit Launch choice prompts inside the Album Scanning
panel rather than silently defaulting to READ.

The launch payload is path-exact and contains only the checked Album set unless
Auto Scan [ALL] is explicitly chosen.

## Refresh behavior

Refresh is the only normal action that reconciles external Artist/Album/tag or
cover changes with the database.

```text
Refresh requested
        ↓
full topology inventory
        ↓
reuse unchanged representative tags
        ↓
Mutagen-read new/changed Albums
        ↓
refresh cover facts and status projection
        ↓
one SQLite transaction
        ↓
replace visible model
```

Rows do not progressively change as individual Artists are scanned.

## Selected Album panels

When focus changes, the right side shows a compact `LOADING ALBUM INFO` banner
until complete data is ready. Indexed Album/tag/statistics values normally make
this transition immediate.

If `cover.*` exists, the upper panel displays the local image and its saved
resolution. If no cover exists, the panel is blank except for:

```text
NO COVER-ART FOUND
USE
S:P:L:I:N:E:D LAUNCH
```

The lower Selected Album Statistics panel remains scrollable.

## Performance contract

Expected warm behavior:

```text
open splined.db
→ load Artist/Album/status rows
→ apply filters in memory
```

Avoid:

```text
open Artist
→ scan filesystem
→ change color
```

and:

```text
each keystroke
→ scan or parse media
```

## Accessibility

Status should be communicated through more than color where practical:

- label and row context;
- status descriptions;
- selection protection behavior;
- tooltips/help text;
- counts and result messages.

## Troubleshooting

### External Album is missing

Run explicit Refresh. Warm startup does not crawl the filesystem for external
changes.

### Tagged Artist appears under the wrong collection

Picker ownership follows the physical Album path, not track-level featured
artists or the SQL authority row's observed path. Verify the Album is physically
beneath the intended first-level library folder, then run Refresh if it was
moved after the index was built.

### Artist changed color after processing

Expected: SPLINED updated one or more child Album statuses and recomputed the
physical parent-folder aggregate.

### Artist changed color merely by opening it

Not expected under the SQLite model. Capture the current runtime debug log and
report the Artist/Album paths involved.

### Everything is White

Verify history/cover state, database path, library-root signature, and that the
first media-index build or explicit Refresh completed successfully.

## Related documentation

- [SPLINED media database](splined-media-database.md)
- [History, retention, bypass, and timeout](history-retention-bypass-timeout.md)
- [Python Ratatui TUI](ratatui-tui.md)
- [Config v5 reference](config-v5-reference.md)
