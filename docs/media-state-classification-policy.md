# Artist and Album state classification policy

SPLINED reads `media-state-classification.json` at runtime. The file is the
shared authority for Artist file-list name colors, Album file-list name colors,
Album status dots, and opposing-application Album-card presentation. Changing
a valid policy does not require recompiling WebGUI or Windows.

## Where the policy is loaded

The first available policy wins:

1. `SPLINED_MEDIA_STATE_POLICY`, when explicitly set;
2. `media-state-classification.json` beside the active `config.toml`;
3. the packaged external policy shipped with the application.

Windows portable and Store packages include the external JSON. The WebGUI
image includes it under `/app/schema`; saving through Settings writes the
editable override to the persistent `/config` volume beside `config.toml`.
Never put credentials, media paths, or database URLs in this policy.

Use **Settings → Advanced ON → Connections → Artist & Album State Policy ·
JSON** to edit it. Windows saves and activates the validated policy when
**Save Connections** is pressed. WebGUI uses **Validate & Save Policy**. An
invalid schema, missing default rule, or unknown color is rejected without
activating it.

## Standalone and coordinated installations

The same JSON works in every supported topology:

- Windows with only SQLite classifies its local SQLite evidence.
- WebGUI with only PostgreSQL classifies its local PostgreSQL evidence.
- When both clients use Album Coordination, the coordination tables add
  `opposing_override` evidence. Coordination is optional and does not replace
  local classification.

The policy synchronizes definitions, priorities, labels, and colors. It does
not copy a database or manufacture evidence that is absent from a standalone
catalog.

## Artist file-list name rules

Artist names aggregate Artist Art plus every Album and Artist-scoped COMP track
associated with that real Artist. The default priority is first-match wins:

1. `missing_artist_art` → yellow (`bypassed` UI key)
2. `embedding_issues` → purple
3. `incomplete_or_error` → red
4. `opposing_override` → dark grey
5. `all_albums_complete_or_overridden` → green
6. `default` → white

Orange, gold, and blue are forbidden Artist-name colors. Missing Album cover
art is Album evidence; it does not by itself turn an Artist name yellow.

## Album name and status rules

Album state is independent of the Artist aggregate. The default Album name
priority is:

1. `opposing_override` → dark grey
2. `missing_art` → yellow
3. `embedding_issues` → purple
4. `incomplete_tracks` → red
5. `tracks_complete` → green
6. `processed` → orange
7. `default` → white

Red means **Incomplete album tracks**. Missing Artist or Album art is yellow,
not red. Blue is auxiliary status for an animated `cover.webp`; it is never an
Album name color.

Status dots are independent and multiple dots may be visible at once. Their
conditions, labels, and colors are in `album.status_dots`.

## Opposing-application presentation

`override_presentation` defines the complete Album-card treatment when an
ordinary Artist Album has current `opposing_override` evidence:

- OVERRIDE banner;
- grayscale Album-card thumbnail;
- dark-grey status dot;
- dark-grey frame and Album file-list name.

This is presentation provenance, not destructive image processing. Opening the
Album keeps the real full-color artwork in **Selected Artwork**, and the Album
remains editable. Fallback/Curated Compilation Album cards never receive the
override grayscale/banner/frame treatment and remain editable and previewable.

## Editing safely

Keep `schema_version` at `1`, retain one `default` rule for both Artist and
Album rankings, and reference only names present in `colors`. Light-theme
values belong in `light_colors`. `ui_status_colors` maps the shared state keys
to named colors used by both interfaces.

The order of each `ranked_*` array is behavior: the first matching condition
wins. Album `status_dots` are not mutually exclusive and their array order is
their display order.

WebGUI includes the active policy in both **Back Up Safe Settings JSON** and
**Download Shared Settings Contract**. Restore accepts the wrapped safe-settings
format and remains compatible with older config-only exports.
