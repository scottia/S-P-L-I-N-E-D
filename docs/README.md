# S:P:L:I:N:E:D Documentation

These versioned repository pages are the canonical public documentation for
[S:P:L:I:N:E:D](../README.md).

## Supported implementations

S:P:L:I:N:E:D has separate supported runtimes:

- **Windows GUI:** v3.0.0 Stable, using Config v5.
- **Windows source:** [`../windows/`](../windows/README.md), containing the finalized GUI and its Config v5 processing core.
- **Repository-root native command line:** Config v5 implementation used by Linux and macOS; its application version follows repository releases.
- **Python/Docker:** Config v5 runtime whose application version follows repository releases.

Application release versions and configuration schema versions are independent. Do not infer one from another.

## Start here

- [Installation and first run](installation-first-run.md)
- [Config v5 reference](config-v5-reference.md)
- [Credentials and provider setup](credentials-providers.md)
- [API/OAuth credential validation](oauth-validation.md)
- [MusicBrainz OAuth](musicbrainz-oauth.md)
- [Source policies and Range Types](source-policies-range-types.md)
- [Select Media and status colors](media-filter-status-colors.md)
- [History, retention, bypass, and timeout](history-retention-bypass-timeout.md)
- [Python Ratatui TUI](ratatui-tui.md)
- [Docker installation](../docker/README.md)

## Python Ratatui display model

Interactive Python/Docker scans use the Ratatui TUI when stdin/stdout are interactive terminals.

```text
TUI
    = operational / decision interface

--no-tui
    = advanced verbose / diagnostic interface
```

The TUI uses exactly two first-class themes, `OLED` and `CHALK`, and shares the same authoritative source, history, bypass, timeout, and status-color semantics as the Windows implementation.

Select Media opens a disposable SQLite picker index from the configured cache, enumerates only immediate root Artist folders, and becomes usable without a recursive full-library traversal. Opening an Artist lazily inventories only that subtree and persists its folder-derived Album topology. Picker identities stay stable for the session; there is no pre-Launch Mutagen/tag-enrichment pass. `Auto Scan [ALL]` and the explicit Refresh Library Index action are the intentional full-inventory paths. Tag parsing, MusicBrainz authority, providers, candidate downloads, image analysis, ranking, and AISPLINE work begin only after Launch.

Artist/Album filtering operates entirely against the currently indexed in-memory model rather than rescanning the filesystem on each keystroke. The picker database is acceleration state, not history or processing authority, and may be deleted safely.

The TUI includes source-grouped candidate presentation on one shared Ratatui column grid, WIDE-mode terminal-cell artwork previews, live authority/provider/download activity, and URL/provenance markers instead of provider IDs. `[URL]` uses terminal-client OSC 8 hyperlink handling; SPLINED does not launch a browser inside its Docker/SSH host. Direct mouse/touch interaction remains provided through the isolated `splined-pyratatui-input` crossterm extension. Render-time hit regions drive taps and the list under the pointer receives wheel/touch scrolling.

The user's WebSSH iOS terminal is a verified touch target: the prior SPLINED `--tui` supported touch-driven result selection and scrolling. Restoring this behavior in the current Ratatui path is therefore an implementation/binding parity requirement, not a speculative terminal feature.

See [Python Ratatui TUI](ratatui-tui.md) and [Select Media and status colors](media-filter-status-colors.md).

## AISPLINE Config v5 boundary

Config v5 keeps AISPLINE policy under the canonical `[aisplined]` section; no Config v5 schema-version bump is required merely because AISPLINE-specific policy fields are added there.

Baseline direction:

```toml
[aisplined]
enabled = false
endpoint = ""
minimum_short_side = 600
allow_below_minimum_override = false
```

The 600 px value is a default user floor, not an absolute hard lock. Explicit below-floor experimentation may be permitted by policy. When AISPLINE is disabled, no AI review, columns, controls, activity widget, or backend work is shown/performed.

See [Config v5 reference](config-v5-reference.md) and [Source policies and Range Types](source-policies-range-types.md).

## Configuration examples

- [`../config.example.toml`](../config.example.toml) is the native/Windows Config v5 example.
- [`../docker/config.example.toml`](../docker/config.example.toml) is the Python/Docker Config v5 example with container paths.

Both examples use the central credential-directory architecture. Provider secrets and normal provider credential filenames do not belong in `config.toml`.

## Authoritative status colors

| Color | Meaning |
| --- | --- |
| White | Default / unprocessed |
| Orange | Processed; manually reprocessable |
| Red | Bypassed; explicit override required |
| Purple | Partial artist or timeout-active album, depending on row type |
| Green | Artist complete |
| Blue | Artist contains at least one bypassed album |

The tree/TUI derives these states from the same album folder, local-art, history, bypass, and timeout authority used by execution. It does not maintain a separate persistent UI status database.

## Python command equivalence

Current Python command behavior remains authoritative in:

- `python/splined.py --help`
- `python/splined.py`
- `python/splined_scan.py`

Interactive Python operational scans can additionally use the Ratatui TUI. Redirected/scripted runs retain the plain CLI.

## Security and safe operation

- Never commit credential JSON, API keys, OAuth tokens, or secrets.
- Use Read mode to evaluate without changing artwork in album folders.
- Test Write mode against a copy, backup, snapshot, or staging library first.
- Back up `config/`, `credentials/`, and the configured log/history location. `_cache/` is disposable.

## Project links

- [Repository README](../README.md)
- [Windows portable instructions](../release/README-WINDOWS.txt)
- [Linux portable instructions](../release/README-LINUX.txt)
- [macOS portable instructions](../release/README-MACOS.txt)
- [Latest releases](https://github.com/scottia/S-P-L-I-N-E-D/releases/latest)
