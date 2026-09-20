# Config v5 Reference — Windows GUI v3.0.0 Stable

This page documents the **Windows GUI Config v5 contract** for S:P:L:I:N:E:D v3.0.0 Stable.

> **Important:** the repository currently still contains an older Config v4 example used by the existing root/Python-side code. Do **not** treat `config.example.toml` as the final Windows v3.0.0 Config v5 schema until the finalized Windows source is integrated and the repository example is updated.

Config schema version and application version are separate concepts:

```text
Windows GUI release: 3.0.0 Stable
Windows config schema: 5
```

The Python/Docker implementation has its own release/version state and may temporarily use a different config schema during synchronization.

---

## Core principles

Config v5 follows these rules:

1. **Portable by default.** Application-owned paths remain relative to the SPLINED application root unless explicitly configured otherwise.
2. **Credentials are separate from configuration.** `config.toml` identifies the credential directory; secrets remain in provider credential files.
3. **GUI-only preferences are separate.** Appearance/UI state such as System/Light/Dark belongs in GUI-local UI settings rather than the main operational config.
4. **Global policy remains the default.** Per-source policy is opt-in through Source Override.
5. **Range Type is authoritative.** Advanced numeric source constraints refine the policy rather than replacing the global Range Type model.
6. **History/bypass/timeout state is runtime authority, not decoration.** The GUI derives tree state from the same execution authority.
7. **Backward compatibility should be deliberate.** Old/legacy fields may be accepted for migration, but Config v5 should not emit obsolete provider credential filename references.

---

# File location

Portable Windows installations use an application-owned configuration directory:

```text
SPLINED/
├── splined.exe
└── config/
    └── config.toml
```

The main file is:

```text
config/config.toml
```

The GUI is the preferred editor for Config v5.

When manually editing TOML, validate the file before a production run.

---

# Config version

The file identifies its schema version:

```toml
config_version = 5
```

Do not change this value to match the application release number.

---

# General behavior

Config v5 retains the core operational concepts used throughout SPLINED:

- Read / Write behavior
- logging verbosity
- scan configuration
- media library path
- ignored/excluded directories
- sample/cache behavior
- credential directory
- output formats
- artwork geometry/output policy
- global artwork Resolution Range
- enabled artwork sources
- per-source policies
- history / retention / bypass / timeout behavior

Exact table/key names for the finalized Windows-specific additions should be verified against the v3.0.0 source at repository integration time. This page documents the public behavior that the final schema must preserve.

---

# Credential directory

Config v5 stores the credential **directory**, not provider secrets.

Conceptually:

```toml
[credentials]
credential_dir = "credentials"
```

Provider credentials live beneath that directory.

Typical portable layout:

```text
credentials/
├── musicbrainz.json
├── lastfm.json
├── fanarttv.json
├── discogs.json
└── ...
```

Config v5 should not require normal users to specify provider filenames such as:

```text
credential_file = "lastfm.json"
credential_file = "fanarttv.json"
token_file = "musicbrainz.json"
```

Standard provider filenames should resolve internally beneath the configured credential directory unless an intentionally supported backward-compatible override exists.

Secrets, OAuth tokens, refresh tokens, API keys, and client secrets must never be committed to source control.

---

# MusicBrainz credential/runtime options

MusicBrainz authentication data and MusicBrainz runtime options are logically separate from the main Config v5 source policy.

The MusicBrainz credential document must be updated non-destructively. Existing authentication/token fields must survive option changes.

Current runtime option defaults are:

```json
{
  "options": {
    "retry_max": 4,
    "min_delay": 1.05,
    "recording_timeout": 7
  }
}
```

Changing these options must not remove or invalidate existing MusicBrainz authentication data.

The main config continues to reference the credential directory, not the token value itself.

---

# Artwork Resolution Range

SPLINED classifies artwork using the image **short side**.

Default global scale:

| Range Type | Short side |
| --- | ---: |
| `BelowMinimum` | `< 1200` |
| `LowerRange` | `1200–1799` |
| `Ideal` | `1800` |
| `UpperRange` | `1801–2400` |
| `Ladder` | `2401–3600` |
| `AboveLadder` | `> 3600` |

The default range anchors are:

```text
Minimum = 1200
Ideal   = 1800
Maximum = 2400
Ladder  = 3600
```

A non-square image is classified by the shorter dimension.

Examples:

```text
600 x 900    -> short side 600  -> BelowMinimum
1200 x 1600  -> short side 1200 -> LowerRange
1800 x 1800  -> Ideal
2000 x 2400  -> short side 2000 -> UpperRange
3000 x 4200  -> short side 3000 -> Ladder
4000 x 5000  -> short side 4000 -> AboveLadder
```

---

# Source Enabled vs Source Override

These are separate concepts.

## Source Enabled

Controls whether the provider may be queried.

```text
Enabled = No
```

means the provider is not queried.

```text
Enabled = Yes
```

means the provider is available to the artwork-discovery pipeline.

## Source Override

Controls whether the provider uses a source-specific policy.

```text
Source Override = No
```

means the provider uses the normal global SPLINED artwork policy.

```text
Source Override = Yes
```

means the provider uses its own configured source policy.

Disabling Source Override must not erase the source's saved custom values. Those values should remain available if the override is re-enabled later.

---

# Per-source policy

Each supported artwork source may persist its own applicable policy values.

The Windows GUI supports the following concepts where meaningful for the provider:

- Source Enabled
- Source Override
- Minimum Range Type
- Allow BelowMinimum fallback
- optional advanced minimum short side
- optional advanced maximum short side
- optional minimum width
- optional minimum height
- Primary image only where provider metadata supports it

Not every provider supports every capability. Unsupported controls should be disabled/explained rather than pretending to enforce unavailable metadata.

---

# Minimum Range Type

When a source override is enabled, Minimum Range Type establishes the normal minimum category accepted for that provider.

Example:

```text
Source: Discogs
Source Override: Yes
Minimum Range Type: LowerRange
```

produces the conceptual policy:

| Range | Result |
| --- | --- |
| BelowMinimum | Reject, or fallback if explicitly allowed |
| LowerRange | Accept |
| Ideal | Accept |
| UpperRange | Accept |
| Ladder | Accept |
| AboveLadder | Accept unless another constraint rejects it |

---

# BelowMinimum fallback

`Allow BelowMinimum fallback` is a **per-source** policy.

When disabled:

```text
BelowMinimum -> REJECT
```

When enabled:

```text
BelowMinimum -> FALLBACK
```

Fallback does not mean the image becomes normally preferred. It means an otherwise below-minimum candidate may remain available when no normally acceptable candidate satisfies the effective policy.

This is intentionally not a global blanket rejection of all artwork below 1200 px. Different providers can have different source-specific fallback value.

---

# Advanced dimension constraints

Advanced numeric fields are additional constraints.

Blank fields mean no additional restriction beyond the effective Range Type/global policy.

The normal Range Type threshold remains the primary derived/default authority.

Example:

```text
Minimum Range Type = LowerRange
Derived short-side minimum = 1200
```

If the user explicitly sets:

```text
Minimum short side = 1500
```

then the effective result becomes:

| Range / size | Result |
| --- | --- |
| `< 1200` | Reject/fallback according to policy |
| `1200–1499` | Reject because of explicit minimum short side |
| `1500–1799` | Accept |
| `1800+` | Accept unless another constraint rejects it |

The GUI should distinguish a value derived from Range Type from an explicit user-entered constraint.

---

# Primary image only

`Primary image only` must rely on provider metadata.

It does **not** mean SPLINED visually recognizes that an image is a front cover.

The current Windows policy does not attempt semantic visual classification of:

- front cover
- rear cover
- disc
- booklet
- jewel case
- other photographed/scanned objects

Future SPLINEDAI functionality may provide semantic image classification, but Config v5 source policy does not assume that capability today.

If a provider does not expose meaningful primary-image metadata, the control should be disabled with an explanatory tooltip.

---

# Output formats

Output formats remain a **global output policy** unless the finalized Config v5 source explicitly defines otherwise.

Supported static output types currently include:

```text
JPEG
PNG
WebP
```

Do not silently reinterpret these as per-source output-format settings.

---

# Artwork geometry / square policy

SPLINED preserves aspect ratio while applying the configured output policy.

The Windows configuration can control concepts including:

- square/crop behavior
- square rounding
- whether upscaling below Ideal is permitted
- evaluating candidates by the image SPLINED would actually write

The final v3.0.0 Config v5 key names should be documented from the source once integrated into the repository.

Policy principle:

> Candidate ranking should reflect the final image SPLINED would write, not merely provider-reported dimensions.

---

# Library and scan paths

Config v5 maintains distinct concepts for:

- main media library
- scan target
- cache
- credentials
- logs/history

Portable Windows paths should remain application-relative where practical.

Avoid embedding machine-specific development paths in public examples.

Ignored/excluded directory entries must persist correctly when Settings is reopened.

---

# History, processed state, bypass, and timeout

The GUI tree must derive state from the same runtime authority used by execution.

It must not maintain a second independent persistent interpretation.

Album-level states include:

| State | Color | Meaning |
| --- | --- | --- |
| Default/unprocessed | White | Normally eligible |
| Processed/history | Orange | Not auto-selected; may be manually reprocessed |
| Bypassed | Red | Requires explicit bypass override/prompt |
| Timeout-active | Purple | Temporarily protected by configured scan timeout |

Artist aggregate states include:

| Artist state | Color | Rule |
| --- | --- | --- |
| Unprocessed | White | All eligible albums unprocessed, no bypass |
| Partial | Purple | Mix of processed and unprocessed, no bypass |
| Complete | Green | All eligible albums processed, no bypass |
| Contains bypass | Blue | At least one red/bypassed album; blue takes precedence |

Artist aggregate precedence:

```text
BLUE  -> any bypassed album exists
GREEN -> all eligible albums processed, none bypassed
PURPLE -> mixed processed/unprocessed, none bypassed
WHITE -> all eligible albums unprocessed, none bypassed
```

Album timeout purple and Artist partial purple are separate internal meanings even though they share a visual color.

---

# Artist selection rules

Selecting an Artist normally:

- auto-selects eligible White/unprocessed Albums
- does not auto-select Orange/processed Albums
- does not auto-select timeout-active Purple Albums
- requires explicit bypass confirmation for Red Albums

Manual selection may allow deliberate Orange reprocessing and temporary bypass override according to the existing execution policy.

A temporary bypass override must not silently delete the saved bypass record.

---

# Media Filter

Media Filter is a GUI view/selection filter over the already loaded in-memory library model.

It must not:

- rewrite Config v5 merely because a filter is used
- alter history
- delete bypass state
- change timeout authority
- rescan the filesystem on every keystroke

Supported concepts include:

- live Artist text filtering
- live Album text filtering
- status/color filtering

Filtering a row out of view should not silently mutate its underlying execution/selection/history state.

---

# Appearance / UI settings

System / Light / Dark appearance is a GUI-local preference and should remain separate from Config v5 operational policy.

Other purely presentational settings should follow the same separation unless they materially affect execution.

This prevents cosmetic configuration from changing the operational config schema unnecessarily.

---

# Config v5 compatibility expectations

A Config v5 implementation should:

- preserve unknown/future values where practical during non-destructive edits
- avoid reconstructing credential JSON from a minimal hard-coded schema
- retain saved source override values when override is temporarily disabled
- persist ignored/excluded folders correctly
- separate GUI appearance state from operational config
- distinguish Config schema version from application version
- avoid reintroducing provider credential filenames into the normal main config

---

# Python / Docker compatibility

The Python/Docker runtime is a separate supported implementation.

During the transition to Windows Config v5, the repository may temporarily expose:

```text
Windows GUI -> Config v5
Python/Docker -> older synchronized/compatible schema
```

Do not change a working Python/Docker config merely because the Windows GUI uses Config v5 unless the Python runtime release explicitly adds that migration.

The Docker runtime currently resolves its primary configuration from:

```text
/config/config.toml
```

and provider credentials from:

```text
/credentials
```

See [`../docker/README.md`](../docker/README.md).

---

# Manual editing

TOML is strict syntax.

Before manually editing:

1. Keep a known-good copy.
2. Use a plain UTF-8/ASCII-safe editor.
3. Avoid JSON punctuation in TOML sections.
4. Run the application's config validation/check command after editing where available.

A parse error reported at end-of-file often means an earlier quote, array, or table construct was left incomplete.

---

# Repository integration requirement

When the finalized Windows v3.0.0 Stable source is added to the repository, this page must be reconciled against the actual Config v5 structs/serialization before release publication.

That integration should also:

- add/update the authoritative Config v5 example
- remove stale Config v4 statements from Windows-specific documentation
- keep Python/Docker version/schema differences explicit
- preserve the credential-directory-only normal configuration model

Until that integration is complete, this document is the Windows **behavioral reference**, while exact newly introduced v5 key names remain source-verified at integration time.
