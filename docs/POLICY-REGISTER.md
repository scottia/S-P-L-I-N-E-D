# SPLINED Policy Register — owner-approved classification

**Scope:** Artist and Album file-list NAME colors, Album status-dot order, and
related evidence. **Approval basis:** owner's written instructions of
November 19, 2026. This is not an approval of existing runtime behavior,
database records, or other application policy.

The old 38-row Windows/WebGUI comparison is **discarded as policy**; do not
reimport it. Only explicit owner requirements belong here. This register is
the normative behavior specification; checked-in JSON is an implementation
artifact to be validated against it, not the source of approval.

## Media Filter Artist-name policy

| Policy ID | Required behavior |
| --- | --- |
| `ART-COLOR-001` | An Artist file-list name is classified **independently of its Albums' own displayed name colors**. Aggregate applicable Artist Art, associated Album Art, Album embedding, and COMP-track embedding evidence. |
| `ART-COLOR-002` | Rank Artist names: **red** (Incomplete/Error) > **yellow** (missing applicable Artist or Album Art) > **purple** (Album/track/COMP embedding issues) > **green** (all eligible Album covers and tracks complete or overridden, with no higher-priority deficiency). No matching category defaults to **red**. |
| `ART-COLOR-003` | Artist names are **never white, dark grey, orange, gold, or blue**. Orange processed-Album, gold bypassed-Album, and dark-grey opposing-override indications do not become Artist-name colors. |
| `ART-COLOR-004` | A yellow Artist condition includes missing applicable `artist.*`/`banner.*` or associated Album `cover.*`; `cover.webp` is excluded as proof that required ordinary Album cover art is present. |
| `ART-COLOR-005` | Purple includes applicable Album, embedded Track, and COMP-track embedding issues. Red outranks yellow and purple; yellow outranks purple; purple outranks green. |
| `ART-COLOR-006` | An Unprocessed total/count in **MEDIA FILTER** is separate from the Artist NAME color. Counts, labels, default theme text, and other neutral UI presentations may not force a white Artist name. |

## Album-name and status-dot policy

| Policy ID | Required behavior |
| --- | --- |
| `ALB-COLOR-001` | An Album file-list name is classified **independently** from the parent Artist name and independently from the multiple status dots displayed on its Album card. Applies to the relevant Artist Album, V/A OST, and Fallback/Curated Compilations views, subject to their actual evidence. |
| `ALB-COLOR-002` | Rank the single Album name color: **dark grey** (opposing-app override) > **red** (Incomplete/Error) > **yellow** (missing Artist/Album Art) > **purple** (embedded-art issues) > **orange** (Processed Album) > **green** (complete without an applicable higher-priority condition). No matching category defaults to **red**. |
| `ALB-COLOR-003` | Album names are **never white or blue**. Blue indicates the presence of animated `cover.webp` only as an auxiliary Album status dot. Its presence does not independently satisfy ordinary `cover.*` requirements. |
| `ALB-COLOR-004` | Purple embedding issues include missing, below-preferred-size, or above-preferred-size embedded artwork, using the configured preferred thumbnail size. The check must use actual track evidence rather than a color shortcut. |
| `ALB-COLOR-005` | Show **all applicable Album status dots** simultaneously, ordered: **dark grey, red, yellow, purple, orange, green, auxiliary blue**. A single Album NAME priority winner must not suppress other applicable status dots. |
| `ALB-COLOR-006` | Missing Artist Art or ordinary Album Art is yellow evidence; incomplete/error is red evidence. Green requires verified completion and absence of higher-priority deficiency. The governing effective color rule, not hard-coded CSS, determines the name. |

## Non-negotiable enforcement

- **No Artist or Album name may display white**, including Unprocessed,
  empty/default classification, selected/highlighted, or light-theme states.
- Default/no evidence for either scope: **red**. Applying a count key as a name
  color is prohibited.
- The ranked Artist and Album name rules and the ordered status-dot list must
  be validated as behavior, not merely accepted because JSON parses.
- Verify WebGUI and Windows **independently** against the same owner policy.
  Never decide which is right by copying the other.
- Runtime PostgreSQL policy revisions and Windows local mirrors may differ
  from the checked-in JSON seed. Validate all effective values and do not
  overwrite runtime state or media files as part of a documentation audit.

## Boundaries requiring explicit follow-up

The statements above preserve the owner's wording but do not invent missing
implementation detail. These points require evidence or owner clarification
before any broadened code change:

1. **Art completeness:** whether both portrait and banner are required or
   whether either qualifies as Artist Art in each supported category; how
   missing `cover.*` is evaluated on Fallback/Curated COMP cards.
2. **Green versus opposing override:** dark grey outranks green for an
   overridden Album name, while an Artist can be green when all associated
   Albums are complete or overridden. The owner's Album-green wording
   mentions dark grey; do not make a single overridden Album green contrary
   to the stated name ranking.
3. **Gold/bypass:** gold is excluded from Artist names but is not currently a
   named palette value in `schema/media-state-classification.json`. Its
   Album-dot or legend role is not specified by the present Album-dot list.
4. **Override frame:** this message defines dark-grey override NAME/DOT,
   not a frame color. Previously approved historical material specifies a
   red frame; the current JSON seed uses dark grey. Resolve the separately
   scoped frame rule without inferring it from the name color.
5. **Red and embedding completion:** the checked-in JSON calls red
   `Incomplete album tracks`, whereas the approved meaning here includes
   `Incomplete/Error`; determine the exact evidence mapping and correct
   both validators/tests and both UI presentations as needed.

Other SPLINED policies (COMP search, database schema, Settings, providers,
identity, synchronization, queues, etc.) remain **unapproved in this
register** until independently reviewed. Do not expand them from historical
assessments or current source behavior.
