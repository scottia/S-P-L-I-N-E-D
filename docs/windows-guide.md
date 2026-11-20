# Native Windows application guide

**Current intended scope:** one native Rust/Slint executable for
artwork and music-library management. This guide is reconstructed
from the current Windows source and owner-approved rules; controls
that lack installed-GUI validation must not be called certified.

## Installation and first launch

Use the official Windows portable release or Microsoft Store channel.
The native release source is [`build/Windows/`](../build/Windows/)
and the [Store packaging guide](windows-store.md) describes its
manifest and release boundaries.

First-run structured Setup should select a music library, **persistent
SQLite Database Directory**, temporary run cache, logs and provider
credentials. The normal workspace must not invent a second default
catalog if configured paths or Config v5 are missing/invalid.
`splined.db` is stored under the configured `scan.cache_dir`.

### SQLite choice

- **Standalone:** local Windows `splined.db` and the same embedded
  canonical Artist/Album and COMP policies as every other client.
- **Shared / Network:** one physical SQLite file accessed by multiple
  native clients, with deliberate journal/lock coordination and path
  mapping. It is **not** an independent policy choice.
- **Optional PostgreSQL coordination:** communicates reconciliation
  and artwork-write provenance with the WebGUI, while Windows SQLite
  remains authoritative for synchronized catalog fields.

Before changing shared storage, review
[Windows SQLite media database](splined-media-database.md).
A schema v2 catalog follows the explicit
[v2→v3 preparedness procedure](windows-schema-upgrade-preparedness.md),
with verified backup and one-way compatibility warning.

## Browse and classify music

Use the Media Library to browse Artists, Albums, V/A OST and fallback
compilation work. Internal catalog processing labels do not directly
assign Artist/Album FILE-LIST name colors. The owner-approved
classifications are:

| Display | Highest to lowest precedence |
| --- | --- |
| Artist NAME | **Red → Yellow → Purple → Green** |
| Album NAME | **Dark Grey → Red → Yellow → Purple → Orange → Green** |
| Album status dots | **all applicable**, ordered Dark Grey → Red → Yellow → Purple → Orange → Green → auxiliary Blue |

Names **default to Red** when no category matches. Yellow includes
missing Artist Art or an ordinary static `cover.*`. Purple includes
bad/missing embedded thumbnails relative to the preferred size.
A lone animated `cover.webp` gives the auxiliary Blue dot and
does not satisfy the static cover requirement.

**Unprocessed appears only as a numeric MEDIA FILTER count.** It is
not a name color and not an Album status dot. Artist names never use
White, Dark Grey, Orange, Gold or Blue; Album names never use White
or Blue.

The owner's accepted card frames, multiple dots, ARTIST/FEATURING/
SOUNDTRACK/VARIOUS/COMPILED/OVERRIDE banners and COMP row colors
must remain independent of name-color corrections. A successful
opposing-app Album write displays the approved **Dark Grey override
frame** and greyed card thumbnail. The Selected Artwork preview is
not greyed; a new successful local write updates writer provenance.
Older descriptions of a Red override frame are not the current rule.

## Artwork, COMP and embedding

The standard workflow is to select an Album, inspect candidate
artwork and source dimensions, optionally apply enhancements and
preview upscaling, then explicitly **WRITE Cover** or
**WRITE Artist Art**. Previewing or selecting does not write audio.

Album embedding and curated COMP/fallback embedding are distinct:
a recognized Album can stage its preferred-size track thumbnail,
while fallback/COMP activity uses track-scoped MusicBrainz identity
and **EMBED Comp** for the selected file. COMP rows display:
**Blue** pending, **Purple** embedded-thumb repair/attention and
**Green** completed. Those colors do not reclassify the Album name.

MusicBrainz release selection, source ranking, aliases and eligible
COMP work remain governed by the currently implemented
[`schema/comp-report-policy.json`](../schema/comp-report-policy.json)
with the accepted working baseline and an explicit policy-change gate.
Do not infer that every current candidate/report control passed
native UI acceptance from this documentation alone.

## Settings, backup and updates

The Settings UI is backed by Config v5 and
[`schema/settings-ui.json`](../schema/settings-ui.json). Sources,
enhancement presets and image quality controls must be saved
deliberately. Provider credentials remain separate files described
in [Credentials](credentials-providers.md).

Back up persistent SQLite and settings separately; SQLite snapshots
use a consistent backup mechanism, not a filesystem copy of an
active WAL journal. Artist images, audio and staged cache may need
separate preservation. Store updates use Windows package delivery;
portable updates do not justify replacing `splined.db` or
credential files.

**Review status:** The documented owner classification is approved;
the current Windows native UI, backup/restore, embedding and
cross-client writer display still require focused on-device tests.
See [validation](SCHEMA-VALIDATION.md) for remaining gaps.
