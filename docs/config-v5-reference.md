# Config v5 and WebGUI Settings Reference

This is the permanent field and behavior reference opened by **Settings HELP**
in the SPLINED WebGUI. Browser Settings is the normal editor; Config v5 is its
saved, validated configuration contract. The container-ready example is
[`docker/config.example.toml`](../docker/config.example.toml). The native
Windows application shares the same policy schema where noted but keeps its
own SQLite catalog and interface state.

Application release and configuration schema versions are separate:

```text
Application release: follows the repository release
All runtimes:        Config v5
```

## Location and path rules

Config v5 is the common runtime contract, but its storage is runtime-specific.
WebGUI reads the file selected by `SPLINED_CONFIG`, normally
`/config/config.toml` in a container. All WebGUI paths are server-visible paths;
they are not paths on the browser computer. Relative values resolve from the
directory containing the active Config v5 file. Windows stores the same
validated document at `<portable root>/data/config.toml` and interface state at
`<portable root>/data/ui.toml`. There is no Registry authority for either file.

Packaged Windows stores the same files beneath
`%LOCALAPPDATA%\Packages\<package-family>\LocalState\SPLINED\data`. No Registry
value or migration marker is an active configuration authority. If Config v5
is missing or cannot be loaded, structured Setup opens; SPLINED does not
substitute `Config::default()` and present an initialized workspace.

On first Windows launch the library, SQL database, temporary run cache, log,
and credential directories are mandatory choices. SPLINED creates only the
selected database, run-cache, log, and credential directories after explicit,
validated **Save & Open Settings**. The ZIP and an ordinary pre-save launch do not
precreate `data`, `_cache`, `_logs`, `credentials`, or `docker_builds`. Existing
saved absolute, mapped-drive, and UNC paths are preserved unchanged.

Relative Windows runtime paths are resolved from the active portable or
packaged state root. External, NAS, mapped-drive, and UNC paths remain absolute.
WebGUI container paths are normally absolute paths supplied by persistent bind
mounts.

## WebGUI Settings sections

- **Library & Files**: Music Library, optional Scan Mode target, persistent and
  temporary caches, V/A / OST Artist Images, provider credentials, ignored
  folders, and the PostgreSQL/runtime storage summary.
- **Artwork & Embedding**: fixed filename stems and formats, preservation,
  square artwork, embedded thumbnail preference, and optional sample images.
- **Enhancement**: theme palette, adjustment presets, resolution range, and
  bounded enlargement policy.
- **Artwork Sources**: provider order, exclusions, and selected-provider policy.
- **Connections**: provider credentials and connection tests.
- **Backup & Recovery**: secret-free settings/reference export and catalog
  maintenance controls.
- **Processing & Logs**: processing mode, completion history, Log Directory,
  diagnostic level, retention, and bounded WebGUI events.

## General settings

| Key | Default | Meaning |
| --- | --- | --- |
| `config_version` | `5` | Operational schema version |
| `mode` | `"read"` | `read` evaluates; `write` may install artwork |
| `verbosity` | `"info"` | Runtime-log threshold: `none`, `debug`, `info`, `warning`, or `error`; `none` takes effect after restart, without removing PostgreSQL WebGUI events or native operational status |

## `[library]`

| Key | Default | Meaning |
| --- | --- | --- |
| `music_library` | empty | Main media-library root |
| `ignored_subs` | `[]` | Exact names or supported wildcard patterns excluded from scans and index refresh |

## `[scan]`

| Key | Default | Meaning |
| --- | --- | --- |
| `scan_library_dir` | empty | Optional server-visible target shown when Scan Mode is enabled; blank uses `library.music_library` |
| `scan_mode` | `true` | Reveals and enables the optional Scan Folder setting; when off, Music Library is the scan root |
| `scan_mode_timeout` | `24` | Hours before completed Albums are eligible again; `0` disables timeout |
| `cache_dir` | WebGUI `"/_cache"`; Windows `"_db"` | **Persistent Cache Directory**. WebGUI stores staged work and original-artwork backups here; native Windows also retains its historical SQLite-directory compatibility |
| `temporary_cache_dir` | WebGUI `"/_cache/_tmp"`; Windows `"_cache"` | **Temporary Cache Directory** for disposable previews, downloaded candidates, uploads, and COMP work; an older Config v5 file that omits it falls back to `cache_dir` |
| `shared_artist_art_dir` | empty | **V/A / OST Artist Images Directory** for shared Soundtrack or Various Artists portrait/banner files; blank uses Persistent Cache Directory itself, while any other persistent server-visible directory or preferred folder name may be selected |
| `sqlite_shared` | `false` | Opt in when one physical `splined.db` is opened through multiple OS/filesystem views; native-client shared-file behavior; WebGUI uses PostgreSQL instead |
| `windows_client_lock` | `false` | Windows-only optional `.client-lock` coordination for shared SQLite. Has no effect unless `sqlite_shared` is true; older Config v5 files default to off |
| `log_dir` | `"/_logs"` in WebGUI | **Log Directory**, edited under **Processing & Logs → Logging** |

Windows first-run Setup leaves required path choices unset and creates nothing
until the user explicitly saves valid choices. WebGUI deployments normally map
`/_cache`, `/_logs`, `/credentials`, and `/config` to persistent host storage.

### Database and temporary run cache

WebGUI stores its catalog, completion ledger, and queued work in PostgreSQL.
`SPLINED_DATABASE_URL` or standard PostgreSQL connection environment variables
supply the connection; browser Settings can save a database URL for a local
server. Deployment environment values take precedence. Do not place database
passwords in public example configuration.

The WebGUI cache holds downloaded candidates, prepared images, original embedded
image backups, and persistent `staged-artwork`. Keep that cache volume when
upgrading: queued work verifies its staged file before writing. WebGUI does not
open a live SQLite catalog or share Windows' active database. An explicit import
can copy recognized rows from a named read-only SQLite snapshot before
destination Album rows exist. It does not rewrite stored paths or replace a
PostgreSQL backup.

Windows uses `<scan.cache_dir>/splined.db` and a separate Temporary Run Cache.
Its optional `sqlite_shared` and `.client-lock` settings apply to intentional
native shared-file deployments. They do not configure WebGUI's PostgreSQL store.
See [media database](splined-media-database.md) and the
[WebGUI guide](browser-workspace-guide.md).

## `[output]`

| Key | Default | Meaning |
| --- | --- | --- |
| `file_name` | `"cover"` | Compatibility value; WebGUI Settings displays the required `cover`, `artist`, and `banner` filename stems as fixed information |
| `file_formats` | JPEG, PNG, WebP | Supported static formats shown as fixed information in WebGUI Settings |
| `preserve_file` | `true` | Preserve existing `cover.*`, `artist.*`, and `banner.*`; for Albums, an Ideal existing cover may finish at local preflight |
| `square` | `true` | Enable square output policy |
| `square_mode` | `"crop"` | `crop` or `off` |
| `square_round_to` | `16` | Round the squared side down to this multiple; `0` disables rounding |
| `embedded_thumbnail_size` | `1200` | Windows **Embedded Thumbnail Preference** and WebGUI album/COMP embedding size, a square side in pixels from 300 through 4000; the saved folder `cover.*` retains its full-resolution output. |
| `upscale_below_ideal` | `true` | Permit ordinary SPLINED enlargement of validated sources up to Ideal; accepted sources at or above Ideal retain native resolution and are never downscaled by this option |
| `upscale_max_percent` | `200` | Hard maximum enlargement relative to the original short side, valid from `100` through `800`; `200` permits 2×. If Ideal requires more, SPLINED stops at this ceiling, while artwork already above Ideal remains at native resolution. |
| `upscale_adaptive_defaults` | `true` | Analyze exposure/contrast and apply only SPLINED's bounded correction when the source demonstrably needs it |
| `upscale_picture_percent` | `2` | Color-intensity/saturation adjustment, `-20`–`20` percent |
| `upscale_sharpen_percent` | `4` | Explicit artwork-profile sharpening, `0`–`20` |
| `upscale_softness_percent` | `0` | Explicit edge-softening adjustment, `0`–`20` percent |
| `upscale_contrast_percent` | `10` | Explicit artwork-profile contrast override, `-20`–`20` percent |
| `upscale_exposure_percent` | `-5` | Explicit artwork-profile exposure override, `-20`–`20` percent |
| `upscale_brightness_percent` | `-1` | Explicit artwork-profile brightness override, `-20`–`20` percent |
| `upscale_gamma_percent` | `-6` | Gamma correction, `-20`–`20` percent |
| `upscale_color_temperature` | `0` | Explicit artwork-profile color correction, `-100` Cool through `100` Warm |
| `upscale_hue_degrees` | `0` | Hue rotation, `-180` through `180` degrees |
| `upscale_darkness_percent` | `0` | Darkness adjustment, `0`–`100` percent |
| `upscale_tint_percent` | `0` | Signed tint strength, `-100`–`100` percent |
| `upscale_tint_color` | `"#FFB36B"` | Tint color in `#RRGGBB` form |
| `evaluate_final_image` | `true` | Rank the image SPLINED would actually write |

Upscaling preserves aspect ratio. By default it does not sharpen. Before enlargement,
SPLINED measures luminance, shadow/highlight clipping, and contrast. It applies
no correction to balanced artwork; a demonstrably flat or under/overexposed
source may receive at most plus or minus 5 percent brightness and plus 8
percent contrast. Explicit Advanced values are added when an eligible
below-Ideal source is enlarged or when the operator explicitly previews/edits
an existing local cover at native resolution. Merely keeping the local cover
does not rewrite it. Setting Apply adaptive defaults false disables the
analyze-first correction but retains explicit values. The Windows Upscale
Preview reports the effective corrections. When `square = true` and
`square_mode = "crop"`, a square Rust projection is previewed from the same
centered square source crop before resize; the Slint preview therefore does
not stretch a near-square source merely to show the projected dimensions.

Enlargement capability is not Preferred/Auto authority. An upscaled candidate
must still pass the configured range and source policy, strict image-content
validation when active, and the automatic-quality gate. Rejected quality stays
visible for manual review. Untouched same-format files remain byte-identical;
processed JPEG and PNG output retains an embedded ICC profile when present.
Decoded WebP and cross-format conversion retain visible color values but may
normalize format-specific profile metadata.

WebP source artwork is preserved by local-art policy. When better static artwork
replaces a matching JPEG/PNG cover, SPLINED avoids accumulating numbered copies
according to active replacement rules.

## `[webgui_ui]`

| Key | Default | Meaning |
| --- | --- | --- |
| `animate_webp_preview` | `false` | Off displays the first still frame of an animated `cover.webp`; on plays the animation in Selected Artwork. An unedited WebP write preserves the original animation and `.webp` extension. |

This is WebGUI presentation state. It does not enable animated artwork inside
audio tags; embedded Album and COMP thumbnails remain static images at
`output.embedded_thumbnail_size`.

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
Discogs, Amazon Store, and Navidrome.
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

`navidrome` provides supplemental library/current-server artwork through
OpenSubsonic-compatible APIs. It is not MusicBrainz authority and does not
replace `splined.db`. It requires `credentials/navidrome.json`; deterministic
matching uses exact Release MBID first and otherwise one unique normalized
Artist + Album identity. Its downloaded-artwork reference is an in-memory,
secret-free session URI rather than an authenticated server URL.

The credential selects exactly one OpenSubsonic authentication method:
`api_key` (preferred; create it in Navidrome **Settings > Players & API keys**)
or `password` (username plus a fresh salted token challenge per request).
Native Navidrome Web-API bearer tokens are not used for `/rest/*` access.

## `[source_policies.<provider>]`

| Key | Default | Meaning |
| --- | --- | --- |
| `enabled` | `true` (`false` for Amazon) | Whether the source may be used |
| `strict_override` | `false` (`true` for Amazon) | Requires decoded front-art evidence for Preferred/Auto and supersedes `source_override` |
| `source_override` | `false` | Activates saved provider-specific policy |
| `minimum_range_type` | `"LowerRange"` | Minimum normal artwork range |
| `allow_below_minimum_fallback` | `false` | Allows the adjacent lower range as fallback |
| `minimum_short_side` | absent | Optional explicit short-side minimum |
| `maximum_short_side` | absent | Optional explicit short-side maximum |
| `minimum_width` | absent | Optional explicit width minimum |
| `minimum_height` | absent | Optional explicit height minimum |
| `primary_image_only` | `true` | Uses primary/front metadata where available |

When Strict Override is on, a provider uses the global range and decoded image
comparison against exact-release CAA Front, validated local art, or trusted
multi-source consensus. An unverified result remains visible/manual but cannot
be Preferred or Auto. When Strict Override is off, Source Override controls
whether saved provider-specific range values or the global range are used.
MusicBrainz ASIN data narrows Amazon discovery to the exact linked product when
available; image-content evidence still decides strict eligibility.

The Windows Sources editor reads and writes `strict_override` directly. Saved
Source Override dimensions remain intact while strict mode is active. Older
Windows settings without this key migrate Amazon to `true`; other providers
remain `false` unless explicitly enabled.

See [Source policies and Range Types](source-policies-range-types.md).

## `[samples]`

```toml
[samples]
sample_write = false
sample_dir = ""
```

Sample writing defaults off. When enabled, review samples are retained in
`sample_dir`, grouped by Artist and Album. An empty `sample_dir` keeps the
portable default beneath the configured temporary cache. Samples remain
available for later review without launching another provider search.

## `[credentials]`

```toml
[credentials]
credential_dir = "credentials"
```

This is the only normal credential setting. Standard provider JSON filenames
are resolved internally. API keys, OAuth tokens, shared secrets, and provider
filenames do not belong in `config.toml`.

WebGUI Settings labels this path **Provider Credential Directory**. It must be
persistent and server-visible; blank secret fields preserve existing values
and saved secrets are never returned to the browser.

`credentials` is the file-backed schema default. Windows first-run Setup leaves
the required credential location unset; the user may select a local or UNC
directory before saving. Relative values resolve from the portable or packaged
state root.

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
| `logging.retention_days` | `14` | Retention for ordinary diagnostic files; Windows keeps only its current `<configured log directory>\run` session |
| `logging.database_audit` | `false` | Optional bounded database activity diagnostics under `<scan.log_dir>/db-audit`; records database opens, transaction events, and write attempts without SQL values |
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

Windows also records Rust application and runtime errors in the current-run
activity surface and configured diagnostics. The same central redaction is
applied to those diagnostic records.

WebGUI runtime and debug records are single-line and limited to 2,048
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
are never included. Unexpected WebGUI service exceptions are written as one
bounded record. Native runtime failures retain their own bounded diagnostic
records. SIGTERM is recorded explicitly.

Each runtime database supplies processed, timeout, chosen-source, and bypass authority. The
native Windows `splined.db` materializes those facts for
Select Media and stores completion, bypass, timeout, and source-selection state
in that same database.

WebGUI backups use consistent PostgreSQL dumps plus its config, credential,
cache/staged-artwork, external Artist Art, and music volumes. Retired terminal
backup commands are not part of the WebGUI. A SQLite snapshot is not a
PostgreSQL backup. See the [server deployment guide](../docker/README.md).
On Windows use **Settings > Backup / Restore**. Settings, interface state, and
credential JSON are stored in `.spl`; SQLite uses a consistent standalone
`.sqlite` snapshot; and secret-free diagnostic metadata uses a readable `.json`.
Each file type has an independent schedule. The `.spl` export may be password
protected. On Windows, opening an associated
unprotected `.spl` automatically imports only its Config v5 settings before
startup. Password-protected archives and settings-import failures open the
guided restore surface with the archive preselected; remaining categories are
restored only when explicitly selected.

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

## Windows portable interface settings

Windows interface state is stored in `data/ui.toml` beside the portable Config
v5 record. It contains window dimensions and non-authoritative interface
preferences such as the library view, status filter, and bypass visibility. It
does not change provider policy, credentials, the media database, or
artwork-writing rules. Config v5 remains the only application-configuration
authority.

## Validation

Windows structured Settings projects controls into the Rust Config model,
validates the complete model, atomically replaces `data/config.toml`, and
reloads application state. Config v5 remains the sole configuration authority;
Slint does not keep a parallel settings document. Native and WebGUI/container
implementations also validate Config v5; use the container example for
container-specific paths.

The Windows portable release contains one `splined.exe`. A user-approved
production update uses the signed standard update package. Application updates
never target `data/config.toml`, `data/ui.toml`, credentials, SQLite/history,
caches, or logs.
