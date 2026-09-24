# Python Ratatui TUI

The supported Python implementation includes an interactive terminal interface
rendered by Ratatui through the `pyratatui` Python bindings. The TUI is a
presentation and input layer around the existing SPLINED engine; provider
discovery, MusicBrainz authority, candidate scoring, Range Types, local-art
policy, output transforms, history, writes, counters, and exit codes remain
owned by the Python engine.

The runtime pins `pyratatui==0.3.0` (Ratatui 0.30.2) for rendering and adds the
isolated `splined-pyratatui-input==0.1.0` ABI3 extension for crossterm input.
The production Docker build compiles that extension as a normal wheel in a
Rust builder stage and copies only the wheel into the final Python image. End
users of that image do not need a Rust toolchain. This does not introduce a
SPLINED Rust application or a Ratatui-core fork.

## Activation and display modes

An operational scan uses the TUI by default when both standard input and
standard output are interactive terminals:

```text
splined --scan-dir
splined --scan-dir "10,000 Maniacs"
```

Use `--tui` to require the interactive interface or `--no-tui` to retain the
plain CLI in a terminal:

```text
splined --tui --scan-dir "10,000 Maniacs"
splined --no-tui --scan-dir "10,000 Maniacs"
```

`--no-tui` is the advanced verbose/diagnostic view. It intentionally exposes
full runtime paths, provider/authentication state, discovery/download details,
URLs, exact comparison values, and linear debug output. The TUI is the
operational/decision view and should be concise without hiding facts needed for
the current decision.

Redirected input/output and other non-TTY execution always use the plain CLI.
An explicit `--tui` request with non-interactive input or output fails clearly
instead of attempting raw terminal mode.

## Themes

Exactly two first-class themes are defined:

- `OLED` (default): true-black canvas, spectral branding, high-chroma semantic
  colors, and solid colored frames across the complete TUI surface.
- `CHALK`: charcoal canvas, chalk-white text, muted mineral colors, and the
  same state semantics with lower visual intensity.

Select CHALK with:

```text
splined --tui-theme CHALK --scan-dir
```

Theme changes presentation only. State names, controls, candidate order,
selection authority, and engine decisions are identical.

OLED and CHALK apply to the whole TUI, not only candidate/result tables.

## Readiness-driven branding

TUI startup has no wall-clock splash timeout. WIDE and NORMAL layouts keep a
large spectral S:P:L:I:N:E:D logo and the folding standard/stylized expansion
phrase above the live inventory card for as long as root Artist/index
initialization is still running. The animation advances at a restrained rate;
the transition occurs only when the initial root Artist model and picker state
are usable. It does not wait for every Artist or Album to be indexed.

The inventory card reports real state such as the library root, picker-index
path, discovered root Artists, cached/indexed Artists, known Albums, current
Artist, and cache recovery. COMPACT and MINIMUM layouts reduce this to safe
one-line branding rather than allowing the logo to overlap controls.

Processing, candidate, activity, and Last Run Summary views retain a distinct
context header: an eight-row large-cell brand on WIDE, a four-row framed brand
on NORMAL, and one line on COMPACT/MINIMUM. S:P:L:I:N:E:D is the normal identity.
A:I:S:P:L:I:N:E:D replaces it only while a real AI activity event or an active
AI enhancement selection exists, then the header returns to S:P:L:I:N:E:D.

## Shared status-color authority

The Python TUI must use the same execution/status semantics as the Windows
Select Media model:

| State | Color |
| --- | --- |
| Unprocessed | White |
| Processed | Orange |
| Bypassed | Red |
| Partial artist / timeout-active album | Purple |
| Artist complete | Green |
| Artist contains bypass | Blue |

See [Select Media and status colors](media-filter-status-colors.md). Theme
palettes may change hue/intensity presentation, but they must not change state
meaning or precedence.

## Library selection workspace

The TUI includes an interactive library-selection workspace modeled on the
Windows Select Media behavior. Wide terminals present the three control
regions across the top, followed by the three library regions:

- Album Status Mode;
- Album Select Mode;
- Scan Mode;
- Artist Picker;
- Album Picker;
- Media Library Statistics;
- live Artist and Album filter boxes;
- source/range configuration access where appropriate.

### Persistent, lazy Select Media inventory

Normal Python Ratatui startup uses a disposable SPLINED-owned SQLite picker
index in the configured cache directory:

```text
<cache>/splined-picker.sqlite3
```

It is an acceleration index only. Completion, bypass, timeout, configuration,
tag, and processing authority remain in their existing stores and engine
paths. Deleting this database is safe.

The normal startup sequence is:

```text
open picker index
        ↓
enumerate immediate Artist folders at the library root
        ↓
reconcile root Artist topology and cached Album rows
        ↓
SELECT MEDIA IS USABLE
        ↓
user opens an Artist
        ↓
inventory only that Artist subtree when not loaded this session
        ↓
cache its Album folders and local-art filenames
```

Ordinary startup does not recursively walk every Artist. A cold cache exposes
folder-derived Artist rows immediately; unindexed Artists use a muted
`inventory not loaded` presentation rather than fabricating White history
state. A warm cache loads known Album topology from SQLite without a routine
full-library validation pass.

Artist and Album identity in Select Media is stable and folder-derived:

```text
Artist display identity = Artist folder name
Album display identity  = Album folder name
```

Tags read after Launch never rename, regroup, or reorder picker rows.

When an Artist is explicitly opened, its allowed pre-Launch work is:

```text
    enumerate that Artist's directories/files
            ↓
    identify album folders
            ↓
    detect local cover filenames/extensions
            ↓
    apply retained history
            ↓
    update the in-memory model and picker index
```

This phase is a filesystem/history inventory only. It may:

- enumerate directories and filenames;
- identify supported audio files by extension;
- identify album directories by the presence of supported audio files;
- detect local artwork by filename/extension without opening or decoding it;
- apply configured ignored/excluded directory rules;
- load retained completion/bypass/timeout history;
- derive album status and Artist aggregate status;
- build lightweight counts/statistics and the in-memory Artist/Album model.

The blocking phase before the first usable Select Media screen must **not**
perform:

- Mutagen/audio-tag parsing or `read_track()` work;
- MusicBrainz authority/release lookup;
- provider discovery;
- remote artwork requests or candidate acquisition;
- image decoding, geometry analysis, ranking, or transformation;
- AISPLINE review/backend work;
- any other album-processing operation that belongs after Launch.

Those expensive operations begin only after the user launches the selected
album(s). There is no representative-track Mutagen enrichment pass before
Launch and no background operation that changes picker identity while the user
is selecting media. Post-Launch processing reads the authoritative tags it
requires through the unchanged engine path.

### Ignored/excluded directory authority

`[library].ignored_subs` is authoritative during the lightweight inventory.
Ignored names/patterns must match practical Windows behavior: do not descend
into excluded directories, do not show them as Artist or Album rows, and do not
include them in statistics, status reconciliation, or selection payloads.
Wildcard entries remain supported where current SPLINED/Windows rules support
them. Symlink/reparse-style recursive loops are not followed.

On POSIX/Linux traversal, every directory basename beginning with `.` is also
excluded automatically at the root and at every nested level. Hidden paths are
not persisted in the picker SQLite database and do not require a Config v5
entry.

Artist and Album text filters operate on the already-loaded in-memory model.
Typing into a filter begins filtering immediately and never triggers a
filesystem rescan. Returning to an Artist already inventoried in the current
session is immediate.

`Auto Scan [ALL]` and the explicit `R` / Refresh Library Index action are the
intentional full-library paths. They index every Artist with genuine Artist and
Album counts before applying the normal authoritative eligibility rules.
Simply opening SPLINED never performs that work.

### Count scopes

Picker numbers deliberately name their scope:

- the Artist header is the number of root Artist rows visible after the active
  Artist filters;
- `Artists` is root total and filtered-visible;
- `Indexed` is cached/indexed Artists over the root total;
- `Validated` is Artists filesystem-validated in this session;
- `Albums` is the currently known/indexed topology, not an implied complete
  library total;
- `Active` is total and visible Albums for the current Artist;
- `Selected` is checked Albums in the known model.

Unprocessed, Processed, Bypass, and Timeout suffixes cover only the active
Artist and therefore sum to that Artist's indexed Album count. Artist Complete
and Artist Contains Bypass cover indexed Artists only; unindexed Artists remain
`inventory not loaded`. `Select [ALL]` explicitly completes full indexing
before selecting the normally eligible library. `Select [FILTERED]` reports
and changes the current Album Picker scope, while `Select [NONE]` reports the
known checked count it will clear.

Selecting an artist cascades only to eligible child albums. History, bypass,
timeout, and manual-reprocessing rules remain authoritative.

The scan launch payload is always path-exact. Filtered READ/WRITE launches use
the intersection of currently visible and already checked Albums; filtering
never auto-adds an unchecked Album. AUTO SELECTED uses all checked Albums even
when a filter hides them, while the explicit AUTO ALL action selects the full
normally eligible library first.

## Mouse, touch, hit-testing, and scrolling

Mouse/touch interaction is a required design target for selection, checkboxes,
scrollbars, candidate rows, URL/provenance actions, dialogs, source-policy
controls, and filter focus.

The user's WebSSH iOS terminal has already demonstrated working touch behavior
with the prior SPLINED `--tui`: touching a different result moved the active
selection and touch/gesture scrolling worked. Therefore touch support is not a
speculative terminal capability for this target environment. Loss of that
behavior in the current `pyratatui==0.3.0` path is an application/binding
regression to restore.

### pyratatui 0.3.0 limitation and the SPLINED input extension

The published `pyratatui` 0.3.0 wheel exposes `Terminal.poll_event()` as a
keyboard-only `PyKeyEvent` API. Its terminal lifecycle does not enable
crossterm mouse capture, and the Python module exports no `MouseEvent`.

SPLINED implements the missing surface in `python/pyratatui_input`, a minimal
PyO3/crossterm extension used alongside the unmodified published pyratatui
renderer. It exposes:

- `EventReader.enable_mouse_capture()` / `disable_mouse_capture()`;
- `MouseEvent` values for button down, button up, drag, move, and wheel motion;
- zero-based row/column coordinates and Ctrl/Alt/Shift modifiers;
- keyboard and resize events through the same event reader;
- an emergency restoration function for partial initialization failures.

The application does not reinterpret raw escape sequences. On normal exit,
Ctrl+C, SIGTERM, engine/TUI exception, and partial initialization failure it
disables mouse capture and restores the pyratatui terminal lifecycle.

Rendering should register structured hit regions rather than infer clicks from
painted text. Required hit targets include:

- Artist and Album rows and checkboxes;
- Artist and Album filter boxes;
- status, select-mode, and scan-mode controls;
- Source Policy Settings, source rows, and source-policy fields;
- candidate rows;
- `AI ENHANCED` controls when present;
- `[URL]`;
- confirmation-dialog buttons;
- scrollable list regions.

Every redraw rebuilds the hit map from the actual responsive `Rect` geometry;
no click is inferred by searching rendered strings. Direct taps on
buttons/checkboxes perform the corresponding action without an extra Enter
press. Tapping a filter gives it visible focus and subsequent text input
appears immediately. Mouse wheel or terminal-provided touch scrolling scrolls
the region under interaction without changing a checkbox merely because it
scrolled.

Keyboard control remains fully supported as a fallback and for normal console
use.

## Processing workspace

The processing workspace uses workflow-specific geometry rather than one
universal left/right dashboard. Solid semantic frames remain part of the visual
identity.

Important sections include:

- selected albums;
- current album;
- authority;
- local artwork;
- suggested/target artwork;
- comparison result;
- preferred source candidate;
- per-source candidate groups;
- optional Enhanced group;
- live activity/status;
- history/log views;
- Last Run Summary.

Source candidates are grouped by source rather than combined into one
large undifferentiated table. Candidate groups form a vertical scroll region;
navigation reveals every row and the frame reports visible row/group ranges.
The TUI does not squeeze a complete search into one physical screen.

Every candidate group in a frame uses one shared Ratatui `Table` column grid.
Preferred, Local, Enhanced, and provider rows therefore start SOURCE,
RESOLUTION, FORMAT, RANGE TYPE, DISTANCE, SQUARE, ACCEPTABLE, APPROVED, and URL
at the same terminal-cell positions. When AISPLINE is enabled, `AI ENHANCED`
and `AI SPLINED` are part of that same grid; when disabled, both columns are
absent rather than blank. Compact layouts deliberately remove lower-priority
columns instead of depending on accidental truncation.

On WIDE terminals, a small true-color half-block artwork preview appears at the
far right of a candidate group. It is generated only from an artwork file that
normal candidate processing already acquired, cached once per Candidate for
the session, and cannot affect evaluation or ranking. Preview failure is a
presentation-only `NO PREVIEW` state. Compact layouts reclaim that space.

## Candidate-table presentation

The primary user-facing candidate table does not need provider IDs. Provider
IDs are debug/internal data and may remain in logs.

The actionable final column is `URL`, with user-facing provenance markers:

```text
[LOCAL]     existing local artwork
[URL]       normal remote/source artwork
[Enhanced]  validated prior AISPLINE result known through history/provenance
```

The full remote URL remains in candidate state but the normal cell displays
only an underlined `[URL]` marker. The marker uses OSC 8 terminal hyperlink
metadata; the terminal client owns opening the URL. SPLINED does not invoke a
browser inside its Docker/SSH host. Pressing `U` or tapping a captured `[URL]`
opens a URL interaction surface containing `[OPEN URL]` and the raw URL. Mouse
capture is temporarily released so clients such as WebSSH can activate the
link, then restored when the modal closes. `Esc` closes the URL surface.

A history-backed Enhanced entry is shown only when its corresponding file still
exists and validates. History alone must not resurrect a missing result.

## Local and Enhanced candidates

Local artwork is a first-class candidate. This is important both for normal
SPLINED comparison and because an already-present local image can be handed to
AISPLINE directly without first selecting, installing, and rediscovering a
remote source.

Previously enhanced artwork is a separate provenance concept. It may appear in
an `Enhanced` candidate group and/or use `[Enhanced]` in the URL/provenance
column.

`Enhanced` provenance is not the same thing as the current `AI ENHANCED`
checkbox described below.

## AISPLINE candidate review and enhancement choice

When `[aisplined].enabled = false`, the TUI shows no AISPLINE columns, no AI
checkboxes, no AI activity widget, and no AI-specific title treatment.

When AISPLINE is enabled, relevant candidates may be reviewed before the source
summary is presented.

This implementation provides the SPLINED-side state/widgets only. Because no
authoritative AISPLINE endpoint protocol or adapter exists yet, enabling the
section reports runtime capability as unavailable and never invents a review,
enhancement, success, or activity event.

The source-summary semantics are:

```text
AI SPLINED
    = completed AISPLINE review result
    = yes / no recommendation

AI ENHANCED
    = user-selectable enhancement action
    = checkbox + required enhancement delta
    = examples: ☐ +300 EH, ☐ +1200 EH, N/A
```

`AI SPLINED = yes` does not mean enhancement has already happened. It means the
candidate review says enhancement may be worthwhile/appropriate under current
policy.

The enhancement delta is the practical change to the normal target. Example:

```text
1500 -> 1800 = +300 EH
600  -> 1800 = +1200 EH
```

At most one candidate per album may be selected for AI enhancement. When the
user checks one `AI ENHANCED` option, every other enhancement checkbox for that
album is disabled/gray until the selected option is unchecked or changed. This
single-selection rule applies across Local, suggested, remote, and other source
groups.

A suggested remote candidate may be handed to AISPLINE directly; it does not
need to be installed into the album directory first.

## AISPLINE floor and experimentation

The default user floor is a short side of 600 px. This is a practical policy
baseline, not an absolute statement that lower-resolution artwork can never be
useful.

Low-resolution images may occasionally be the only available source, and some
may remain surprisingly usable despite their dimensions. Documentation should
therefore distinguish:

```text
default floor
    -> protects normal users from poor/expensive attempts

explicit experimentation
    -> user may deliberately permit an attempt below the floor
```

Below-floor attempts must be explicit and must never silently weaken the
configured policy.

## Upscale policy one-attempt override

If AISPLINE is enabled and the user chooses an enhancement while:

```toml
[output]
upscale_below_ideal = false
```

SPLINED must not silently ignore the conflict. The preferred interactive
behavior is a red warning/confirmation that offers a runtime-only override for
that one album, one candidate, and one enhancement attempt.

The override:

- does not rewrite Config v5;
- does not affect later albums;
- does not permanently enable ordinary SPLINED upscaling;
- is canceled if the user answers No.

Users who do not want repeated prompts can configure their policy explicitly.

## Live activity while processing

Authority lookup, provider discovery, candidate downloads, and AISPLINE work
must publish useful live activity into the TUI so the screen does not appear
idle or frozen.

Example activity types:

```text
MusicBrainz authority lookup
provider start / done / error
candidate reference counts
download start / done / skip
AISPLINE assessment
AISPLINE restoration/upscale
AISPLINE validation
success / rejection reason
```

The activity view is a visibility feature, not a substitute for performance.
The provider/discovery/download path should still be optimized so the current
long loading lag is materially reduced.

## AISPLINE activity widget

The shared Ratatui design includes a reusable live AISPLINE activity widget or
popup. It should report the actual processing phase and final result, for
example:

```text
ASSESSING
RESTORING
UPSCALING
VALIDATING
✓ AISPLINE SUCCESS
```

or:

```text
✕ AISPLINE REJECTED
reason: validation failed
original candidate retained
```

The widget reports engine state; it does not decide policy.

When an AI-specific workflow/context is active, the runtime title may change
from:

```text
S:P:L:I:N:E:D
```

to the canonical companion name:

```text
A:I:S:P:L:I:N:E:D
```

## Source-policy parity and performance

Python should expose the same practical source-policy controls already modeled
by Windows Config v5 where supported:

- Source Enabled;
- Source Override;
- Minimum Range Type;
- adjacent-lower fallback;
- minimum short side;
- maximum short side;
- minimum width;
- minimum height;
- Primary image only when provider metadata supports it;
- source priority;
- global Min / Ideal / Max / Ladder and square round-to.

Filtering/rejection should occur as early as policy permits so irrelevant
low-value candidates do not consume unnecessary downloads, AI review, or TUI
space. Source filtering must not change ranking semantics for candidates that
remain eligible.

The Python path performs independent provider discovery with at most four
workers and image downloads with at most four workers. Results are merged in
configured source/reference order before the unchanged ranking code runs.
Reliable provider geometry/front metadata is filtered before download;
unknown geometry is downloaded and evaluated normally rather than guessed.
Proven duplicate source URLs are fetched once.

## Interactive batch lifecycle

An interactive TUI scan is a reusable session, not a one-batch process. A
successful or failed batch presents `LAST RUN SUMMARY`; Enter or Esc returns
to Select Media, where the retained root topology, loaded Artist subtrees,
filters, focus, activity/history, and picker database can be reused. Attempted
Album checkmarks are cleared, affected Album/Artist status is reconciled from
the existing history authority, and no full-library traversal is performed.

Another batch can then be selected and launched. Session exit occurs only on
explicit `q`, Ctrl+C/SIGTERM, or a fatal unrecoverable error. The process exit
code retains any failed batch seen during that session. The plain `--no-tui`
path remains the original single-run advanced diagnostic flow.

The Source Policy Settings workflow edits an in-memory draft. `Ctrl+S`
explicitly saves/applies it to Config v5 using an atomic TOML update that
preserves comments, unknown keys, and unrelated sections. Navigation never
rewrites configuration.

## Keyboard basics

Keyboard is a complete fallback, not the only intended interaction path.

| Key | Action |
| --- | --- |
| `↑` / `↓` | Move within the focused list/control |
| `←` / `→` | Change context/value; collapse/expand where offered |
| `Space` | Toggle the focused checkbox/selection |
| `Enter` | Open/activate the highlighted item |
| `Tab` / `Shift+Tab` | Move between panes/regions |
| `/` | Focus the current Artist/Album filter where applicable |
| `Esc` | Leave filter focus, close help, or go back where permitted |
| `PgUp` / `PgDn` | Page-scroll the focused list |
| `Home` / `End` | Jump to first/last item in the focused list |
| `?` | Contextual help |
| `s` | Use the engine's suggested candidate |
| `k` | Keep existing local artwork |
| `f` | Edit fallback artist/album and retry |
| `m` | MusicBrainz retry/search/pick |
| `b` | Open the bypass confirmation dialog |
| digits | Highlight an exact candidate; `Enter` activates it |
| `u` | Open the terminal-client hyperlink surface for highlighted `[URL]` |
| `p` | Open Source Policy Settings from Select Media |
| `r` | Explicitly rebuild the complete disposable picker index |
| `Ctrl+S` | Explicitly save/apply the source-policy draft |
| `q` | Close a completed view; disabled during unsafe active work |
| `Ctrl+C` | Stop the TUI and restore the terminal |

Help output must document `--tui`, `--no-tui`, `--tui-theme OLED`, and
`--tui-theme CHALK`, including the fact that interactive TTY scans enter the TUI
automatically while `--no-tui` provides the advanced verbose view.

## Responsive layout and terminal safety

Layouts adapt by width/height. Wide screens may use parallel cards; compact
screens stack cards and reduce lower-priority columns. If the terminal is too
small, show a resize message rather than overlapping panels.

Terminal setup is context-managed. Normal completion, handled engine errors,
`KeyboardInterrupt`, SIGTERM, and unexpected exceptions must restore raw mode,
alternate-screen state, cursor visibility, mouse capture, and normal shell
input.

## A:I:S:P:L:I:N:E:D Config v5 boundary

Config v5 retains AISPLINE policy under the canonical `[aisplined]` section; no
Config v5 schema-version bump is required merely because additional AISPLINE
policy fields are added there.

Baseline direction:

```toml
[aisplined]
enabled = false
endpoint = ""
minimum_short_side = 600
allow_below_minimum_override = false
```

The 600 px value is a default user floor. It is configurable/overridable for
explicit experimentation; it is not an absolute hard lock.

AISPLINE may also use its own JSON/runtime settings for detailed model/backend
preferences where that proves cleaner. Such AISPLINE-specific settings may be
made accessible through the TUI, but a final JSON schema is not locked by this
document.
