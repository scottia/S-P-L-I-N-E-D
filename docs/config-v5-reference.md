# Config v5 Reference

This page documents the common Config v5 contract. The complete, secret-free
native/Windows example is [`config.example.toml`](../config.example.toml), and
the Docker example uses the same schema with container paths.

Application release and configuration schema versions are separate:

```text
Windows GUI release: 3.0.0 Stable
Repository release:  independent
All runtimes:        Config v5
```

## Location and path rules

The portable Windows default is:

```text
SPLINED/
├── splined.exe
├── config/
│   ├── config.toml
│   └── ui.toml
├── credentials/
├── _cache/
└── _logs/
    └── _history/
```

Relative runtime paths are resolved from the SPLINED application directory.
External, NAS, and UNC paths remain absolute. An optional `config.location`
file stores the configured config path without moving credential data into the
main config.

## General settings

| Key | Default | Meaning |
| --- | --- | --- |
| `config_version` | `5` | Operational schema version |
| `mode` | `"read"` | `read` evaluates; `write` may install artwork |
| `verbosity` | `"info"` | Runtime logging verbosity |

## `[library]`

| Key | Default | Meaning |
| --- | --- | --- |
| `music_library` | empty | Main media-library root |
| `ignored_subs` | `[]` | Exact names or supported wildcard patterns excluded from scans |

## `[scan]`

| Key | Default | Meaning |
| --- | --- | --- |
| `scan_library_dir` | empty | Configured scan target |
| `scan_mode` | `true` | Enables configured scan-directory behavior |
| `library_scan` | `false` | Enables full-library scanning |
| `scan_mode_timeout` | `24` | Hours before completed albums are eligible again; `0` disables timeout |
| `cache_dir` | `"_cache"` | Disposable candidate/sample cache |
| `log_dir` | `"_logs"` | Diagnostic log location |
| `history_dir` | `"_logs/_history"` | Completion, chosen-source, bypass, and timeout authority |

## `[output]`

| Key | Default | Meaning |
| --- | --- | --- |
| `file_name` | `"cover"` | Output filename stem, without path or extension |
| `file_formats` | JPEG, PNG, WebP | Enabled formats in preference order |
| `preserve_file` | `true` | Preserve existing artwork according to current replacement policy |
| `square` | `true` | Enable square output policy |
| `square_mode` | `"crop"` | `crop` or `off` |
| `square_round_to` | `16` | Round the squared side down to this multiple; `0` disables rounding |
| `upscale_below_ideal` | `false` | Permit ordinary SPLINED enlargement below Ideal |
| `evaluate_final_image` | `true` | Rank the image SPLINED would actually write |

WebP source artwork is preserved by the local-artwork policy. When better
static artwork replaces a matching JPEG/PNG cover, SPLINED avoids accumulating
numbered copies and removes the matching obsolete static cover according to the
active replacement rules.

When AISPLINE is enabled, `upscale_below_ideal = false` remains meaningful. If
an interactive user explicitly chooses an AISPLINE enhancement that requires
upscaling, the preferred behavior is a red warning offering a runtime-only
override for that one album/candidate/attempt. The override does not rewrite
Config v5 and does not enable ordinary SPLINED upscaling for later work.

## `[range]`

Artwork is classified by its short side.

| Key | Default |
| --- | ---: |
| `min` | 1200 |
| `ideal` | 1800 |
| `max` | 2400 |
| `ladder` | 3600 |

| Range Type | Default short side |
| --- | ---: |
| `BelowMinimum` | below 1200 |
| `LowerRange` | 1200–1799 |
| `Ideal` | 1800 |
| `UpperRange` | 1801–2400 |
| `Ladder` | 2401–3600 |
| `AboveLadder` | above 3600 |

The required ordering is `min < ideal <= max < ladder`.

## `[sources]`

`cover_sources` stores artwork-source priority. Supported artwork sources are
Deezer, iTunes, Fanart.tv, Last.fm, Cover Art Archive, and Discogs.
`exclude_cover_sources` disables listed artwork sources without changing their
saved priority.

MusicBrainz is metadata authority and is not added to `cover_sources`.

## `[source_policies.<provider>]`

Config v5 supports source policies for every artwork provider and MusicBrainz.

| Key | Default | Meaning |
| --- | --- | --- |
| `enabled` | derived/enabled | Whether the source may be used |
| `source_override` | `false` | Activates saved provider-specific policy |
| `minimum_range_type` | `"LowerRange"` | Minimum normal artwork range |
| `allow_below_minimum_fallback` | `false` | Allows only the adjacent lower range as fallback |
| `minimum_short_side` | absent | Optional explicit short-side minimum |
| `maximum_short_side` | absent | Optional explicit short-side maximum |
| `minimum_width` | absent | Optional explicit width minimum |
| `minimum_height` | absent | Optional explicit height minimum |
| `primary_image_only` | `true` | Uses provider primary/front metadata where available |

Artwork-specific fields are not written for MusicBrainz. Its policy contains
only `enabled` and `source_override`.

When Source Override is off, the source uses the global range. Saved custom
policy values remain available and are not erased. When fallback is on, only
the single range immediately below the configured minimum becomes a fallback;
it does not become a normally accepted range.

Python/TUI should expose the same practical controls where supported so source
policy can eliminate irrelevant candidates before unnecessary downloads, AI
review, or candidate-screen clutter. This is parity with existing Config v5
policy, not a second selection engine.

See [Source policies and Range Types](source-policies-range-types.md).

## `[samples]`

`sample_write = true` writes review samples beneath the configured cache.

## `[credentials]`

`credential_dir = "credentials"` is the only normal credential setting in
Config v5. Standard provider JSON names are resolved internally beneath that
directory.

Authentication values, API keys, OAuth tokens, shared secrets, and provider
filenames are not written to `config.toml`.

### MusicBrainz runtime options

MusicBrainz authentication and runtime options share the credential document
but are updated independently. Defaults used when no options object exists:

```json
{
  "options": {
    "retry_max": 4,
    "min_delay": 1.05,
    "recording_timeout": 7
  }
}
```

SPLINED reads the existing JSON, merges changed options, preserves
authentication and unknown fields, and atomically replaces the file. Changing
source policy does not rewrite the credential.

See [MusicBrainz OAuth](musicbrainz-oauth.md).

## `[logging]` and `[history]`

| Key | Default | Meaning |
| --- | --- | --- |
| `logging.retention_days` | `14` | Diagnostic-log retention |
| `history.enabled` | `true` | Enables persistent status authority |
| `history.retention_days` | `0` | History retention; `0` means forever |

History supplies processed, timeout, chosen-source, and bypass state. Shortening
or disabling it can remove the authority needed for status colors. `_cache/`
is disposable and is not the history authority.

AISPLINE-enhanced provenance may also be recorded in history so a validated
existing enhanced file can be recognized later. A history record alone must not
resurrect a file that no longer exists or validates.

## `[aisplined]`

Config v5 uses `[aisplined]` as the canonical integration and policy section for
the separate A:I:S:P:L:I:N:E:D companion product. Adding AISPLINE policy under
this section does **not** require a Config v5 schema-version bump.

Baseline direction:

```toml
[aisplined]
enabled = false
endpoint = ""
minimum_short_side = 600
allow_below_minimum_override = false
```

Meaning:

| Key | Baseline | Meaning |
| --- | --- | --- |
| `enabled` | `false` | Whether SPLINED exposes/uses AISPLINE integration |
| `endpoint` | empty | Where SPLINED can reach the AISPLINE runtime/interface when configured |
| `minimum_short_side` | `600` | Default user floor for normal automatic AISPLINE review/enhancement |
| `allow_below_minimum_override` | `false` | Whether deliberate below-floor experimentation may be offered/allowed |

The 600 px floor is a practical default, not an absolute quality claim. Users
may deliberately experiment below it when policy allows because a low-resolution
image can still be the only available source or remain usable despite its size.
Below-floor attempts must be explicit; they must never silently weaken the
configured floor.

When `enabled = false`:

- no AISPLINE review occurs;
- no AI columns/checkboxes appear in the TUI;
- no AI activity widget appears;
- no AISPLINE endpoint/model/backend work occurs.

When enabled, AISPLINE review may occur on relevant local or remote candidates
before the candidate summary is presented. The user-facing source-summary model
is:

```text
AI SPLINED
    yes/no result of completed AISPLINE review

AI ENHANCED
    optional user action shown as checkbox + required delta
    examples: ☐ +300 EH, ☐ +1200 EH, N/A
```

Only one candidate per album may be selected for enhancement at a time. Selecting
one enhancement option disables/grays the other enhancement options for that
album until the selection is changed.

A local image or an uninstalled suggested remote candidate may be handed directly
to AISPLINE. It does not need to be installed into the album directory first.

Detailed model/backend settings may later live in AISPLINE-owned JSON/runtime
configuration if that proves cleaner. Such settings may be surfaced in the TUI,
but this reference does not lock a JSON schema.

Legacy `[splineai]` remains a compatibility concern for existing Python Config
v5 installations; `[aisplined]` is the canonical public name. Conflicting
canonical/legacy values must not be merged silently.

The Python Ratatui Source Policy Settings view persists only the existing
`[sources]`, `[source_policies]`, `[range]`, and relevant `[output]` fields after
an explicit Save/Apply action. The writer is atomic and preserves unknown keys,
comments, table order, and all unrelated Config v5 content. It never changes
`config_version = 5`.

See [Python Ratatui TUI](ratatui-tui.md) and the AISPLINE development policy on
the AI development branch.

## GUI-only `ui.toml`

`config/ui.toml` is deliberately separate from operational Config v5. It stores
presentation and transient GUI state, including:

- System/Light/Dark theme;
- status/confirmation and hover preferences;
- Select Media expansion, filters, and transient selected paths;
- filtered Read/Write mode;
- main, Settings, Compare, and Preview sizes/positions;
- splitter distances and Settings tab positions.

Editing `ui.toml` does not change source policy, credentials, history, or
artwork-writing rules.

## Validation

The Windows Settings **Validate Saved Config** action and **Save and Continue**
use Config v5 validation before execution. Root native and Python/Docker also
validate Config v5; use the Docker example for its container-specific paths.
