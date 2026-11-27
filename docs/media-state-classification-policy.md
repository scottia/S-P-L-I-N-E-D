# Artist and Album state classification policy

The owner-approved Artist/Album name colors and dot priorities are governed by
[POLICY-REGISTER.md](POLICY-REGISTER.md). This page describes runtime policy
delivery and persistence; it does not establish another policy authority.

SPLINED uses one JSON policy schema for Artist file-list name colors, Album
file-list name colors, Album status dots, database-monitor presentation, and
opposing-application Album-card presentation. WebGUI stores the JSON document
as a revisioned PostgreSQL row; coordinated Windows keeps a validated local
`media-state-classification.json` mirror. Changing a valid policy does not
require recompiling WebGUI or Windows.

## Where the policy is loaded

The repository schema is the built-in default. A standalone Windows SQLite
installation uses that default without writing an external JSON. WebGUI seeds
the singleton `media_state_classification_policy` PostgreSQL row during database
migration, then PostgreSQL is its revisioned policy authority.

Windows portable and Store packages do not contain an installed JSON. Windows
Setup creates files only for an explicitly selected shared-SQLite topology:

- **Shared SQLite only:** Setup Save creates or adopts
  `media-state-classification.json` in the selected Provider Credential
  Directory.
- **Shared SQLite + WebGUI PostgreSQL:** Test first validates the PostgreSQL
  connection, coordination schema, and PostgreSQL policy without writing any
  file. Setup Save then creates `album-coordination.json` and a validated local
  `media-state-classification.json` mirror of that PostgreSQL revision.
- **Standalone SQLite:** neither JSON is created during Setup.

Setup never writes a blank JSON and never overwrites a conflicting existing
credential or policy. Never put credentials, media paths, or database URLs in
the classification policy. `album-coordination.json` contains only the optional
PostgreSQL connection credential; it does not contain classification rules.

The v2 classification removes white Artist/Album names. On first load, the
shipped `splined-media-state-v1` classification is migrated to
`splined-media-state-v2` while retaining its custom palette, database-monitor,
and override-presentation settings. WebGUI stores that migration as a new
PostgreSQL policy revision. Coordinated Windows normalizes the PostgreSQL
revision before activating and mirroring it; shared-SQLite-only Setup rewrites
only the recognized v1 policy, never an unrelated/custom policy identifier.

Coordinated Windows opens from its local validated mirrors. After the window is
ready, a background five-second check reads both the media-state and
[Artist COMP report](comp-report-policy.md) revisions. A newer validated
revision activates only after processing is idle. An unavailable PostgreSQL
server leaves the last valid mirrors active. Classification is never fetched
during rendering or changed during an active COMP batch.

Use **Settings → Advanced ON → Connections → Artist & Album State Policy ·
JSON** to edit it. Windows writes PostgreSQL first when Album Coordination is
configured, then mirrors and activates the returned revision locally. Without
coordination it saves only the local editable policy. WebGUI writes the same
policy transactionally to PostgreSQL. An invalid schema, missing default rule,
or unknown color is rejected without activating it.

## Standalone and coordinated installations

The same JSON works in every supported topology:

- Windows with standalone SQLite classifies local evidence with the built-in
  default and needs no JSON file.
- Windows with shared SQLite and no PostgreSQL can use an editable local JSON.
- WebGUI with only PostgreSQL classifies its local PostgreSQL evidence.
- When both clients use Album Coordination, the coordination tables add
  `opposing_override` evidence and PostgreSQL owns the shared revisioned policy.
  Coordination is optional and does not replace local classification.

The policy synchronizes definitions, priorities, labels, and colors. It does
not copy a database or manufacture evidence that is absent from a standalone
catalog.

Album and COMP coordination records are the cross-client ownership evidence.
Their `writer` decides whether the other application presents a dark-grey
override. Artwork-source provenance remains independent: a track written by
Windows may still retain `itunes`, `local`, `existing`, or another provider as
its `source_kind`. A matching durable artwork digest is sufficient to apply
the opposing override, and neither application rewrites the provider merely
to change a status color.

Windows-authored COMP coordination also restores Windows' in-memory completed
track state after an index reload. Re-reading the same unchanged coordination
revision is a no-op; it does not refresh the file list, move scroll focus, or
re-open the music file. The COMP report's lazy artwork inspection remains the
authority for preferred-size green versus attention-required purple/blue.

## Database connection badge

`database_monitor` maps backend names and connection states to badge labels and
semantic colors. It does not decide database health. Windows and WebGUI perform
one lightweight connection/query validation at launch, after Test or Save, on
manual retry, and at most once every three minutes.

- Green: the configured connection validated.
- Yellow: the connection is available to configure but is missing or invalid.
- Red: the configured connection check failed.
- `Current`: standalone Windows SQLite, where shared monitoring is not enabled.

The compact badge in **Selected Artwork** is clickable only while yellow or
red. Clicking it clears the cached result and performs one immediate re-check.

## Artist file-list name rules

Artist names aggregate Artist Art, Album art, and every Album and Artist-scoped
COMP track associated with that real Artist. The default priority is first-match
wins:

1. `incomplete_or_error` → red
2. `missing_artist_art` (Artist or associated Album art is missing) → yellow
3. `embedding_issues` → purple
4. `all_albums_complete_or_overridden` → green
5. `default` → red

White, dark grey, orange, gold, and blue are forbidden Artist-name colors.
Opposing overrides remain Album evidence: when every Album is complete or
overridden, the Artist is green. A purple Album or COMP track makes the Artist
purple unless red or yellow evidence has higher priority.

## Album name and status rules

Album state is independent of the Artist aggregate. The default Album name
priority is:

1. `opposing_override` → dark grey
2. `incomplete_tracks` → red
3. `missing_art` → yellow
4. `embedding_issues` → purple
5. `processed` → orange
6. `tracks_complete` → green
7. `default` → red

Red means independent **Incomplete/Error** evidence (the current canonical
JSON uses the narrower display label `Incomplete album tracks`). Missing
Artist or Album art is yellow,
not red unless independent incomplete/error evidence is also present. Blue is
auxiliary status for an animated `cover.webp`; it is never an Album name color.
White and blue are forbidden Album-name colors.

Status dots are independent and multiple dots may be visible at once. Their
conditions, labels, and colors are in `album.status_dots`. Their default display
order is dark grey, red, yellow, purple, orange, green, then the auxiliary blue
animated-cover dot.

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

The existing Windows and WebGUI policy loaders validate the owner-approved source ordering. Official Workflows 01–03 and the independent pull-request policy-review gate check canonical source contracts; they do not automatically change saved PostgreSQL policy revisions or Windows credential-directory mirrors.

The order of each `ranked_*` array is behavior: the first matching condition
wins. Album `status_dots` are not mutually exclusive and their array order is
their display order.

WebGUI includes this policy and the active COMP report policy in both **Back Up
Safe Settings JSON** and **Download Shared Settings Contract**. Restore accepts
the wrapped safe-settings format and remains compatible with older config-only
exports.
