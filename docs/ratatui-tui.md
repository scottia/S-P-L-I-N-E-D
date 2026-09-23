# Python Ratatui TUI

The supported Python implementation includes an interactive terminal interface
rendered by Ratatui through the `pyratatui` Python bindings. The TUI is a
presentation and input layer around the existing SPLINED engine; provider
discovery, MusicBrainz authority, candidate scoring, Range Types, local-art
policy, output transforms, history, writes, counters, and exit codes remain
owned by the Python engine.

## Activation

An operational scan uses the TUI by default only when both standard input and
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

Redirected input/output and other non-TTY execution always use the plain CLI.
An explicit `--tui` request with non-interactive input or output fails clearly
instead of attempting raw terminal mode. If automatic TUI initialization fails
before scan work starts, SPLINED reports the problem and safely falls back to
plain output. An explicit `--tui` initialization failure is reported as an
error.

Help, version, configuration, credential, OAuth, release-discovery, and scan
preview commands remain plain CLI commands. The Docker entry point's bare
operational invocation follows the same TTY rule.

## Themes

Exactly two first-class themes are available:

- `OLED` (default): true black canvas, spectral branding, and vivid semantic
  colors derived from the existing Python OLED palette.
- `CHALK`: charcoal canvas, chalk-white text, sage success, ochre
  warning/fallback, brick red failure, dusty violet history, slate cyan
  navigation, and muted mauve special accents.

Select a theme for a run with:

```text
splined --tui-theme CHALK --scan-dir
```

Theme changes presentation only. State names, controls, candidate order, and
engine decisions are identical.

## Workflows and responsive layout

The TUI has workflow-specific presentations for scan progress, current-album
processing, local/target comparison, candidate selection, manual fallback,
MusicBrainz release selection, session history, logs/diagnostics, help, and the
final summary. `Tab` and `Shift+Tab` switch among the active workflow, history,
and logs.

Wide terminals show parallel cards and the complete candidate table. Normal
and compact terminals reduce columns and stack cards. Below 48 columns or 12
rows, the UI displays a resize message instead of overlapping panels. No Nerd
Font is required for functional meaning.

Launches show the brief S:P:L:I:N:E:D spectral title and a single folding
expansion row, then collapse to a compact persistent header. Provider and image
work runs on the engine worker while the terminal event/render loop remains
responsive.

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

The command letters are the same values consumed by the plain pickers. TUI raw
mode and plain `input()` are never active at the same time.

## Runtime dependency and terminal safety

The Python requirements pin `pyratatui==0.3.0`, which targets Ratatui 0.30.2.
Published ABI3 wheels cover glibc Linux x86_64, musl Linux x86_64/AArch64,
macOS x86_64/Apple Silicon, and Windows x86_64, so a Rust toolchain is not
required on those targets. The Debian-based SPLINED Docker image is currently
published from an x86_64 runner; it installs the wheel and includes the
`python/tui/` package.

Terminal setup is context-managed. Normal completion, handled engine errors,
`KeyboardInterrupt`, SIGTERM, and unexpected exceptions pass through terminal
restoration so raw mode, the alternate screen, and cursor state are not left
active.

## A:I:S:P:L:I:N:E:D boundary

Config v5 uses the documentary companion boundary:

```toml
[aisplined]
enabled = false
endpoint = ""
```

Legacy `[splineai]` is accepted by the Python loader only when canonical
`[aisplined]` is absent. If both are present, their `enabled` and `endpoint`
values must match; conflicting tables fail validation. No endpoint calls, AI
provider behavior, or image remediation are implemented by this TUI work.
