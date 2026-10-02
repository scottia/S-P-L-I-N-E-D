# Config v5 Reference

This page documents the common Config v5 contract. The complete secret-free
native/Windows example is [`config.example.toml`](../config.example.toml). The
Docker example uses the same schema with container paths.

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
│   └── splined.db
└── _logs/
    └── run/
```

Relative runtime paths are resolved from the SPLINED application directory.
External, NAS, and UNC paths remain absolute. Docker paths are normally absolute
container paths supplied by bind mounts.

## General settings

| Key | Default | Meaning |
| --- | --- | --- |
| `config_version` | `5` | Operational schema version |
| `mode` | `"read"` | `read` evaluates; `write` may install artwork |
| `verbosity` | `"info"` | Runtime-log threshold: `debug`, `info`, `warning`, or `error` |

## `[library]`

| Key | Default | Meaning |
| --- | --- | --- |
| `music_library` | empty | Main media-library root |
| `ignored_subs` | `[]` | Exact names or supported wildcard patterns excluded from scans and index refresh |

## `[scan]`

| Key | Default | Meaning |
| --- | --- | --- |
| `scan_library_dir` | empty | Configured scan target |
| `scan_mode` | `true` | Enables configured scan-directory behavior |
| `library_scan` | `false` | Enables full-library scanning |
| `scan_mode_timeout` | `24` | Hours before completed Albums are eligible again; `0` disables timeout |
| `cache_dir` | `"_cache"` | Candidate/sample cache and persistent `<cache_dir>/splined.db` |
| `sqlite_shared` | `false` | Opt in when one physical `splined.db` is opened through multiple OS/filesystem views; Python owns shared inventory, while rollback journaling and a 30-second busy timeout protect shared writes |
| `log_dir` | `"_logs"` | Diagnostic log location |

### SQLite `cache_dir`

Python/Docker and Windows store the persistent Select Media read model at:

```text
<scan.cache_dir>/splined.db
```

Normal run-cache cleanup preserves that file and its SQLite sidecars. Candidate
downloads, samples, and other cache content remain disposable.

The database location follows `cache_dir`; there is no separate database-path
setting. With the Docker example:

```toml
[scan]
cache_dir = "/_cache"
```

SPLINED uses:

```text
/_cache/splined.db
```

Windows Setup retains its browsable **Cache directory** field and derives the
same filename. A portable Windows `_cache` creates an independent database; a
shared UNC cache can expose the same physical database as another installation.
Raw UNC paths are supported directly in TOML. SPLINED creates an internal,
provider-aware Windows drive adapter for SQLite because Windows network locking
can differ between raw UNC and drive-letter paths. An existing connection to
the same UNC prefix is found dynamically (including NFS, even if its letter
changes); otherwise a temporary connection is used. The configured path remains
UNC and never names a drive letter.

For an intentional shared file, set `sqlite_shared = true` in every process
that can open it. Local databases default to WAL for warm-start performance;
shared mode uses rollback journaling plus `busy_timeout=30000`. Python/Docker
owns shared Album inventory and exclusions. Windows `ignored_subs` and output
filename remain local configuration and are not compatibility requirements;
Windows Refresh reloads the Python-published inventory instead of rebuilding it.

See [SPLINED media database](splined-media-database.md).

## `[output]`

| Key | Default | Meaning |
| --- | --- | --- |
| `file_name` | `"cover"` | Output filename stem, without path or extension |
| `file_formats` | JPEG, PNG, WebP | Enabled formats in preference order |
| `preserve_file` | `true` | `true` permits an Ideal existing cover to finish at local preflight; `false` continues provider discovery and permits canonical replacement |
| `square` | `true` | Enable square output policy |
| `square_mode` | `"crop"` | `crop` or `off` |
| `square_round_to` | `16` | Round the squared side down to this multiple; `0` disables rounding |
| `upscale_below_ideal` | `false` | Permit ordinary SPLINED enlargement below Ideal |
| `evaluate_final_image` | `true` | Rank the image SPLINED would actually write |

WebP source artwork is preserved by local-art policy. When better static artwork
replaces a matching JPEG/PNG cover, SPLINED avoids accumulating numbered copies
according to active replacement rules.

Embedded JPEG artwork selected as preferred source is materialized using the
configured filename stem and canonical JPEG output policy.

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

Required ordering:

```text
min < ideal <= max < ladder
```

## `[sources]`

`cover_sources` stores artwork-source priority. Supported sources are Deezer,
iTunes, Fanart.tv, Last.fm, MusicBrainz / Cover Art Archive, Cover Art Archive,
Discogs, and Amazon Store.
`exclude_cover_sources` disables listed sources without changing saved order.

The `musicbrainz` priority uses MusicBrainz as release authority and Cover Art
Archive as the image host. It prefers the release-group representative;
`coverartarchive` remains the independent exact-release source. Both expose
direct image URLs to Candidate Decision.

`amazon` performs a credential-free, best-effort Amazon Store search and accepts
only primary `https://m.media-amazon.com/images/I/` URLs. It removes Amazon's
between-dots image transform (for example `._AC_UY218_`) before download so the
candidate URL points to the original image. Amazon is disabled by default;
blocking or markup changes appear as provider diagnostics.

## `[source_policies.<provider>]`

| Key | Default | Meaning |
| --- | --- | --- |
| `enabled` | `true` (`false` for Amazon) | Whether the source may be used |
| `source_override` | `false` | Activates saved provider-specific policy |
| `minimum_range_type` | `"LowerRange"` | Minimum normal artwork range |
| `allow_below_minimum_fallback` | `false` | Allows the adjacent lower range as fallback |
| `minimum_short_side` | absent | Optional explicit short-side minimum |
| `maximum_short_side` | absent | Optional explicit short-side maximum |
| `minimum_width` | absent | Optional explicit width minimum |
| `minimum_height` | absent | Optional explicit height minimum |
| `primary_image_only` | `true` | Uses primary/front metadata where available |

MusicBrainz and Amazon use the same Minimum Range Type, adjacent fallback,
dimension, and primary-image controls as other artwork sources. When Source
Override is off, a provider uses the global range while retaining saved custom
values.

See [Source policies and Range Types](source-policies-range-types.md).

## `[samples]`

```toml
[samples]
sample_write = true
```

When enabled, review samples are written beneath the configured cache directory.
Samples are disposable even though `splined.db` in the same parent directory is
persistent.

## `[credentials]`

```toml
[credentials]
credential_dir = "credentials"
```

This is the only normal credential setting. Standard provider JSON filenames
are resolved internally. API keys, OAuth tokens, shared secrets, and provider
filenames do not belong in `config.toml`.

### MusicBrainz runtime options

MusicBrainz authentication and runtime options share the credential document
but are updated independently. The normal runtime defaults are:

```json
{
  "options": {
    "retry_max": 4,
    "min_delay": 1.05,
    "recording_timeout": 7
  }
}
```

SPLINED merges changed options, preserves authentication and unknown fields,
and atomically replaces the credential file.

Compilation track-art recovery requires the exact `retry_max`,
`min_delay`, and `recording_timeout` keys in the credential JSON. It does not
hard-code or infer missing values. In that workflow `retry_max` is the maximum
total attempt count, `min_delay` applies between requests, and
`recording_timeout` applies to each Recording-ID request.

The native Windows runtime additionally uses `recording_timeout` as the bound
for MusicBrainz/CAA release-group and exact-release artwork discovery. This
affects waiting time only; it does not alter artwork source order, policy, or
ranking.

See [MusicBrainz OAuth](musicbrainz-oauth.md).

## `[logging]` and `[history]`

| Key | Default | Meaning |
| --- | --- | --- |
| `logging.retention_days` | `14` | Retention for ordinary diagnostic files; Windows keeps only its current `_logs/run` session |
| `history.enabled` | `true` | Enables persistent completion/status authority |
| `history.retention_days` | `0` | History retention; `0` means forever |

Every initialized invocation creates one current-run diagnostic file under:

```text
<scan.log_dir>/run/
```

The next invocation clears that dedicated run directory before creating the new
file. The filename contains verbosity, timestamp, uniqueness value, and process
ID.

Credentials, tokens, authorization headers, client secrets, and private
credential values must never be logged. SPLINED applies central redaction to UI
and persistent runtime-log messages as a final safety boundary.

Python/Docker runtime and debug records are single-line and limited to 2,048
characters. Embedded newlines are escaped and an oversized record ends with a
truncation marker. Repeated/high-cardinality picker state is summarized as
counts; a launch records at most three sample paths plus the number omitted.
This keeps a debug run readable without losing the event sequence needed to
diagnose startup, selection, report return, or launch behavior.

Windows debug runs also record compact timing summaries. `provider_timing`
reports each enabled source independently as `success`, `zero`, or `error`,
with discovery/download milliseconds and reference/candidate/error counts.
`post_cover_timing` separates pre-persistence work, database open/validation,
the Album update, the affected Artist aggregate, audit insert, durable commit,
and total `Final`-to-`album_completed` time. `media_snapshot.timing` records
SQLite picker-load milliseconds and the returned Album count. Provider timing
errors are bounded and URL query values are redacted; credentials and tokens
are never included. Unexpected Python exceptions are written as one bounded
record, while fatal native interpreter/input-extension failures write a
one-time thread stack so an abrupt TUI exit is not indistinguishable from an
external process termination. SIGTERM is recorded explicitly.

SQLite supplies processed, timeout, chosen-source, and bypass authority. The
shared Python/Docker and Windows `splined.db` materializes those facts for
Select Media and stores completion, bypass, timeout, and source-selection state
in that same database.

Back up:

```text
config/
credentials/
<scan.cache_dir>/splined.db   # Python/Docker and Windows
```

## `[aisplined]`

`[aisplined]` is the reserved canonical boundary for the separate companion
product:

```toml
[aisplined]
enabled = false
endpoint = ""
minimum_short_side = 600
allow_below_minimum_override = false
```

| Key | Baseline | Meaning |
| --- | --- | --- |
| `enabled` | `false` | Whether a future AISPLINE integration is enabled |
| `endpoint` | empty | Future runtime/interface endpoint |
| `minimum_short_side` | `600` | Default user floor for normal enhancement consideration |
| `allow_below_minimum_override` | `false` | Whether explicit below-floor experiments may be allowed |

AISPLINE processing has not begun. When disabled, no AI review, enhancement,
activity, or backend work occurs. Placeholder configuration or columns do not
constitute a finalized companion-product requirement.

Legacy `[splineai]` remains a compatibility concern for existing installations;
`[aisplined]` is the canonical public name.

## GUI-only `ui.toml`

`ui.toml` is separate from operational Config v5 and is stored beside the active
`config.toml`. This also applies when Windows uses a relocated or UNC-hosted
configuration. It stores presentation and transient GUI state such as theme,
window placement, filters, splitter positions, and current UI selections.
Editing it does not change source policy, credentials, the media database, or
artwork-writing rules.

## Validation

Windows Settings **Validate Saved Config** and **Save and Continue** validate
Config v5 before execution. Native and Python/Docker implementations also
validate Config v5; use the Docker example for container-specific paths.
