# Windows guide

## Architecture and package

SPLINED for Windows is a native WinForms frontend over the in-process Rust
core. It does not use Tauri, a web frontend, or a worker executable.

```text
SPLINED\
  splined.exe
  runtime\
    splined-core.dll
  README-WINDOWS.txt
```

The frontend loads `runtime\splined-core.dll` through the existing narrow C
ABI. UTF-8 requests and structured callbacks carry scans, progress, decisions,
candidates, MusicBrainz results, cancellation, errors, and completion. Rust
owns the provider, policy, SQLite, path-mapping, editing, and output authority.
Work executes away from the UI thread.

Portable state is created under `data\` beside the executable. The Store build
uses its writable per-user LocalState. Both channels retain Config v5,
`ui.toml`, the configured credential directory, and `splined.db` as their
authorities.

## Replacement artwork workspace

The Windows artwork experience is a replacement application composition, not
a restyle of the earlier MainForm hierarchy. Its persistent shell contains:

- a branded app bar with global Artist/Album/Folder search and Home, Scan,
  Settings, and Help actions;
- a graphical Media Library sidebar with Artists, Albums, and Folders modes;
- Find & Match Artwork, Edit Artwork, Upscale / Enhance, and Advanced workspace
  modes;
- a dominant Selected Artwork pane on the right;
- a horizontally scrollable Candidate Artwork strip;
- a compact status bar backed by real runtime state.

The layout adapts to live client dimensions and per-monitor DPI. At compact
widths, cards and controls reduce their spacing and nonessential text elides;
the source and candidate strips remain mouse-wheel scrollable rather than
wrapping into a second row. Native light-theme scrollbar chrome is not exposed
inside the dark workspace.

The repository logo and colorized `S.P.L.I.N.E.D` wordmark are loaded from the
same application assets used elsewhere in the product. The shell never invents
provider counts, ratings, models, versions, or metadata: empty and loading
states remain honest until the backend supplies values.

## Graphical Media Library

Artists, Albums, and Folders are projections of the existing SQLite/path
authority. The sidebar is not a second library database.

- **Artists** groups indexed Albums and shows cached remote Artist art when a
  deterministic Navidrome identity is available.
- **Albums** shows the current SPLINED/local cover first and may use cached
  Navidrome album art as enrichment.
- **Folders** remains grounded in canonical SPLINED paths and external path
  mapping.

Remote art loads asynchronously into a bounded cache. An unavailable
Navidrome server never blocks local library navigation. Search changes only the
visible projection; it does not mutate SQLite identity, source priority, or
selection authority.

Album state semantics remain White/Unprocessed, Blue/Incomplete,
Orange/Processed, Red/Bypassed, and Purple/Timeout. Artist Complete and Artist
Contains Bypass retain their established meanings. Compilation-track focus and
explicit track selection remain separate.

READ evaluates without writing. LIVE WRITE may install the confirmed artwork.
Skip Album advances without persisting a bypass; Bypass Album records the
deliberate state. When bypass is already saved, **NO Bypass** confirms and
removes only that saved Album bypass before restoring the current review.

## Dynamic source strip

The source card row is generated from Config v5 `sources.cover_sources` and the
corresponding source policies. It has no private GUI-only provider list or
priority setting.

- cards appear in the exact saved order;
- disabled/excluded cards retain their priority position and are visibly
  disabled;
- mouse-wheel horizontal scrolling keeps the full order inspectable;
- dragging a card writes the new order through the normal Config v5 save path;
- Settings reorder and reload immediately project back into the same row;
- choosing a card focuses that source experience without changing priority.

Local Files is a built-in current-library card. MusicBrainz opens the embedded
authority/results workspace. Navidrome uses the supplemental provider behavior
documented below. All other cards invoke their existing provider paths.

## Find & Match Artwork

Find & Match is the default candidate workflow. The heading reports the active
Album and real candidate count. Below the dynamic source row are two independently
collapsible cards:

- **Filters** exposes tri-state image categories and candidate policy.
- **Artwork Preview & Editing** exposes the selected five primary adjustments.

Each image category cycles through three states:

1. neutral — do not constrain the category;
2. included — show that category;
3. red-X disregarded — exclude that category.

The tri-state UI writes the existing included/excluded interface state and
filters only already-discovered candidates. It does not launch network searches
on every click.

The adjustment authority is the existing set of eight: Picture, Sharpen,
Softness, Contrast, Exposure, Brightness, Gamma, and Color. Exactly five are
selected as primary controls. Arrange/Select changes and reorders that primary
five in `ui.toml`; it does not duplicate Config v5 output values. Each visible
control retains its numeric value, reset, keyboard, mouse, and persisted edit
behavior.

The bottom Candidate Artwork strip contains real candidate images, provider
provenance, dimensions, range/policy state, and selection state. It scrolls
horizontally with the mouse. The shell does not fabricate ratings for providers
that do not supply them.

## Edit Artwork

Edit Artwork keeps the graphical library, candidate strip, and Selected
Artwork pane visible while expanding the five primary controls. Selecting a
candidate updates the preview without changing the candidate's provider or
MusicBrainz authority. Use Selected, Keep Local, Compare, Bypass/NO Bypass, and
provider-specific actions retain their established backend behavior.

## Upscale / Enhance and Custom Upscale

Upscale / Enhance uses the existing SPLINED projection and editing pipeline. It
does not advertise an AI model that the current runtime does not implement. The
workspace shows a real before/after preview beside the active five controls.
Artwork above Ideal is preserved at native resolution, and
`output.upscale_max_percent` remains the maximum scale authority.

Selected Artwork exposes **Open Custom Upscale** only when a previewable
candidate exists. It opens one separate resizable **Upscale Comparison &
Editing** window; repeated activation brings that window forward instead of
adding a panel below the main workspace or opening duplicates.

The editor provides:

- equal `[CURRENT]` and `[UPSCALED]` previews rendered from the cached candidate;
- display-only yellow `RESOLUTION #x#  RATIO #:#` references over the bottom
  edge of each image (never composited into the saved file);
- current source, original/target dimensions, square policy, and maximum scale;
- the same five configured primary adjustments;
- mutually exclusive Upscale Preview and Apply Upscale modes;
- the existing adaptive Apply Upscale Default behavior;
- Apply/Save Existing and red Cancel Upscale actions.

Preview mode cannot save. Apply mode uses the existing Rust/write policy and
never bypasses the configured scale, square, format, or native-resolution
rules.

## Advanced

Advanced combines the live Scan Activity and Decisions stream with the complete
eight-adjustment catalog. It is a diagnostic/advanced-control projection over
the same run and interface state, not a second processing engine.

## Selected Artwork

The right pane remains the visual focus. It displays the selected local or
candidate image, source, actual dimensions, effective range/quality, and fit or
zoom controls. Candidate focus updates this pane; it does not submit a decision.
If a remote preview fails, the candidate remains reviewable with its diagnostic
state and local navigation remains usable.

## MusicBrainz Matches

MusicBrainz is embedded in the replacement shell beneath the same dynamic
source row. It remains release authority; Cover Art Archive supplies image
bytes. `CURRENT ALBUM` stays fixed above ordinary results and is never derived
from the first search row.

Artist and Release Type filters apply to ordinary results. The current release,
recent visits, cached result navigation, Apply IDs, source-result return, and
release-page links retain the accepted behavior. Choosing another release
updates the authority passed to normal source discovery; it does not silently
install artwork. Candidate actions are unavailable while the MusicBrainz
decision boundary owns the active decision.

## Navidrome / OpenSubsonic

Navidrome is supplemental library and current-server artwork enrichment:

```text
MusicBrainz  = metadata/release authority
Navidrome    = library/current-server artwork enrichment
splined.db   = durable media/workflow authority
```

SPLINED uses compatible OpenSubsonic endpoints first for ping/capability
detection, search, Artist/Album identity, and cover retrieval. Narrow
Navidrome-specific behavior is reserved for required identity/path enrichment.
The implementation does not add server administration, playlist management, or
user management.

Candidate matching is deterministic:

1. exact MusicBrainz Release ID when available;
2. otherwise one unique normalized exact Artist + Album match;
3. ambiguous fallback matches return no Navidrome candidate.

Cover art is fetched only after a deterministic match. Candidate references use
an in-memory `splined+navidrome://` session identity, so API keys, passwords, and
authentication challenges cannot enter callback payloads, logs, screenshots,
SQLite, or Config v5. The 25 MiB artwork limit and decoded-image safety checks
still apply.

Credentials are stored only in `credentials\navidrome.json` beneath the
configured credential directory. The contract includes Server URL and exactly
one normal OpenSubsonic authentication mode: dedicated API key (preferred) or
username/password. Create a key in Navidrome under **Settings > Players & API
keys > Generate API key**. API-key mode never sends a username; password mode
uses a fresh salt/token challenge and never sends the clear password. Native
Navidrome Web-API bearer tokens are not required or sent to `/rest/*`. The
Windows credential editor masks secrets and performs a real compatible
capability test. Python/Docker exposes `--navidrome-credentials` for the same
file contract.

This is distinct from Navtagger's post-write notification path. Navtagger uses
its restricted `navidrome_docker_api` endpoint to execute the native
`navidrome scan -t <library>:<target>` command in the configured container.

See [Credentials and provider setup](credentials-providers.md) and
[Config v5 reference](config-v5-reference.md).

## Compilation resume and fallback artwork

Normal incomplete compilation launch consults the SQLite completion ledger and
starts at the first unfinished track whose recorded authority does not already
match. Clicking a track changes focus/preview only; checking a completed track
explicitly reopens that track. A blank explicit-track path remains normal Album
resume.

During an active compilation decision, focusing another track changes only the
embedded-art preview. It does not clear the decision, candidate cards, or SQLite
resume cursor. Successful per-track writes update the ledger before advancing.
Folder-level cover files are not modified by embedded-track fallback work.

## Portable state, backup, and restore

Portable relative paths resolve from the portable root. Store relative paths
resolve from the writable LocalState root. Absolute, mapped-drive, and UNC paths
remain exact. Config v5 controls the music library, cache/database, temporary
artwork cache, credentials, logs, history, source policy, and external path
mapping. SQLite remains `<scan.cache_dir>\splined.db`; the shell creates no
competing database.

`ui.toml` stores presentation-only state including workspace/library mode,
panel expansion, primary adjustment order, tri-state filters, splitters, zoom,
and window placement. Selective `.spl` backup/restore can transfer Config v5,
interface state, credentials, SQLite, and diagnostics through the established
validated format.

## Distribution and updates

Windows remains x64-only. Portable and Store packages use the same executable
and in-process DLL. Portable update checks are notification-only; Store updates
remain delegated to Windows/Microsoft Store. No artwork-workspace feature
changes the updater, MSIX identity, release/tag flow, or packaging authority.

See the [release automation guide](release-automation.md) and
[Microsoft Store channel guide](windows-store.md) for those contracts.

## Diagnostics and screenshot QA

The activity surface shows live events. Persistent run logs remain in Config v5
locations and redact credential material. Unexpected WinForms exceptions record
their bounded exception chain before SPLINED offers to continue.

`windows\gui\TEST-PREVIEW-WINDOWS-GUI.cmd` builds the real replacement controls
with deterministic QA-only data and captures:

```text
actual_01_FIND_MATCH_ARTWORK.png
actual_02_EDIT_ARTWORK.png
actual_03_UPSCALE_ENHANCE.png
actual_04_MUSICBRAINZ_RESULTS.png
actual_05_CUSTOM_UPSCALE.png
actual_06_NAVIDROME_CREDENTIALS.png
actual_07_RESPONSIVE_1280x800.png
```

Fixture names, counts, and generated artwork exist only in the QA executable.
They are not production fallback data. The preview test additionally verifies
that each image was produced and is non-empty; functional GUI self-tests cover
dynamic order/reorder, tri-state filtering, primary adjustments, the separate
upscale window, and Navidrome credential redaction/matching.
