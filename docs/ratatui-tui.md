# Python Ratatui TUI

The supported Python implementation includes an interactive terminal interface
rendered by Ratatui through the `pyratatui` Python bindings. The TUI is a
presentation and input layer around the existing SPLINED engine; provider
discovery, MusicBrainz authority, candidate scoring, Range Types, local-art
policy, output transforms, history, writes, counters, and exit codes remain
owned by the Python engine.

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

The TUI design includes an interactive library-selection workspace modeled on
the Windows Select Media behavior. It may present:

- Album Status Mode;
- Album Select Mode;
- Scan Mode;
- Artist Picker;
- Album Picker;
- Media Library Statistics;
- live Artist and Album filter boxes;
- source/range configuration access where appropriate.

Artist and Album text filters operate on the already-loaded in-memory library
model. Typing into a filter must begin filtering immediately and must not rescan
the filesystem on each keystroke.

Selecting an artist cascades only to eligible child albums. History, bypass,
timeout, and manual-reprocessing rules remain authoritative.

Mouse interaction is a required design target for selection, checkboxes,
scrollbars, candidate rows, and filter focus. If the current `pyratatui` release
does not expose the required mouse event surface, extend the Python binding
layer rather than rewriting SPLINED or Ratatui core. Keyboard control remains
fully supported.

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
- final summary.

Source candidates should be grouped by source rather than combined into one
large undifferentiated table.

## Candidate-table presentation

The primary user-facing candidate table does not need provider IDs. Provider
IDs are debug/internal data and may remain in logs.

The actionable final column is `URL`, with user-facing provenance markers:

```text
[LOCAL]     existing local artwork
[URL]       normal remote/source artwork
[Enhanced]  validated prior AISPLINE result known through history/provenance
```

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

## Keyboard basics

| Key | Action |
| --- | --- |
| `↑` / `↓` | Navigate candidates or releases |
| `←` / `→` | Change context where offered |
| `Tab` / `Shift+Tab` | Switch active workflow, history, and logs |
| `Enter` | Activate the highlighted exact selection |
| `Esc` | Close help or go back where the engine permits |
| `?` | Contextual help |
| `s` | Use the engine's suggested candidate |
| `k` | Keep existing local artwork |
| `f` | Edit fallback artist/album and retry |
| `m` | MusicBrainz retry/search/pick |
| `b` | Open the bypass confirmation dialog |
| digits | Highlight an exact candidate; `Enter` activates it |
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
alternate-screen state, cursor visibility, and normal shell input.

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
