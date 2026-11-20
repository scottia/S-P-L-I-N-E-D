# S:P:L:I:N:E:D — owner-approved policy register

**Authority:** The repository owner's instructions, confirmed November 19,
2026. The rules below have **already been given and approved**. No repeat
owner approval is required to state or document them. The register is
normative; current code, test assertions, saved database copies and old
PR documents are not alternate policy authorities.

**Scope:** Artist/Album FILE-LIST **name colors**, ordered multiple Album
status dots, approved visual indicators, policy consistency across
deployment modes, and safeguards for changing shared policy JSON.
For implementation verification, see
[SCHEMA-VALIDATION.md](SCHEMA-VALIDATION.md).

## Approved Artist FILE-LIST name classification

The Artist name is an aggregate of applicable Artist Art, Album cover,
track-embedding and COMP-track evidence. It is evaluated separately from
the individual Album name colors and MEDIA FILTER counts.

| ID | Approved requirement |
| --- | --- |
| `ART-COLOR-001` | Artist names use an independent ranked aggregation of their applicable Artist/Album Art and Album/Track/COMP evidence—not a copied Album color, filter count or theme foreground color. |
| `ART-COLOR-002` | **Red (Incomplete/Error) > Yellow (missing Artist or Album Art) > Purple (embedding issues) > Green (all applicable covers/tracks complete or overridden without a higher-priority issue).** The first applicable ranked condition wins. If none matches, default to **Red**. |
| `ART-COLOR-003` | Artist NAME colors are **never White, Dark Grey, Orange, Gold or Blue**. The Orange processed-Album, Gold bypassed-Album, and Dark-Grey override indicators are not Artist-name classifications. |
| `ART-COLOR-004` | Yellow includes missing applicable `artist.*` / `banner.*` or an associated ordinary Album `cover.*`; `cover.webp` alone does **not** fulfill the ordinary static cover requirement. |
| `ART-COLOR-005` | Purple includes embedding issues in applicable Albums, tracks and COMP tracks; Red outranks Yellow/Purple/Green, Yellow outranks Purple/Green, Purple outranks Green. |
| `ART-COLOR-006` | `Unprocessed` exists as a MEDIA FILTER **numeric count only**. A catalog row's internal processing label is not the Artist NAME category and never assigns White or independently forces Red. |

**Artist name rank:** **Red → Yellow → Purple → Green**. Excluded:
White, Dark Grey, Orange, Gold and Blue.

## Approved Album FILE-LIST name classification and dots

Album FILE-LIST names are independent of the Artist aggregate and of the
multiple dots shown on Album cards. The applicable Album, V/A OST and
Fallback/Curated Compilation views use the same ranking.

| ID | Approved requirement |
| --- | --- |
| `ALB-COLOR-001` | Classify the Album NAME separately from the Artist NAME and separately from its multiple status dots. |
| `ALB-COLOR-002` | **Dark Grey (opposing-app override) > Red (Incomplete/Error) > Yellow (missing Artist/Album Art) > Purple (embedding issues) > Orange (processed Album) > Green (complete)**. A matching higher-priority rule wins; no match defaults to **Red**. |
| `ALB-COLOR-003` | Album NAME colors are **never White or Blue**. Blue is solely the auxiliary animated-`cover.webp` status dot; that file alone does not satisfy an ordinary static cover requirement. |
| `ALB-COLOR-004` | Purple embedding issues include missing, below-preferred-size or above-preferred-size embedded thumbnails, judged against the configured preferred embedded-art size using real track evidence. |
| `ALB-COLOR-005` | Display **all applicable Album status dots at once** in the fixed priority order: Dark Grey, Red, Yellow, Purple, Orange, Green, then auxiliary Blue. A selected Album NAME color must not suppress another applicable dot. |
| `ALB-COLOR-006` | Missing applicable Artist Art or static Album cover is Yellow evidence; an independent Incomplete/Error condition is Red evidence. Green requires verified completion with no higher-priority deficiency. The rendered NAME uses its classified color, not hard-coded CSS. |

**Album name rank:** **Dark Grey → Red → Yellow → Purple → Orange → Green**.
**Status dots:** **Dark Grey → Red → Yellow → Purple → Orange → Green → Blue (animated WebP only)**.

**Default Red** is the unmatched/uncategorized name color as well as an
explicit Incomplete/Error result. It must **not** be manufactured from
the internal catalog word `unprocessed`: where Yellow evidence exists
and there is no independent Red condition, Yellow is the higher
applicable classification.

## Approved visuals that must not change during name-color corrections

| ID | Accepted appearance |
| --- | --- |
| `VISUAL-001` | The owner's supplied Album Status Key screenshot is the accepted reference for **Album status dots and Album card frames** (including Cyan, Green, Orange/Purple, Green/Purple, Yellow and Dark Grey), independent of the file-list names. Preserve the existing opposing-app **Dark Grey override frame**. |
| `VISUAL-002` | Preserve the currently accepted **COMPILED, FEATURING, SOUNDTRACK, VARIOUS, ARTIST and OVERRIDE banners** and their styling. |
| `VISUAL-003` | Preserve COMP Track ROW colors: **Blue = pending artwork; Green = artwork complete; Purple = embedded thumbnail needing attention**. These are not Artist/Album name-ranking colors. |
| `VISUAL-004` | `cover.webp` creates an auxiliary **Blue Animated** Album indicator; it does not independently classify the Album NAME Blue. |

A correction to Artist/Album name classification is **not** authorization
to alter frames, dots, banners, COMP rows, or their counted states.

## One canonical policy across every installation

| ID | Approved requirement |
| --- | --- |
| `STATE-POLICY-001` | Standalone Windows SQLite, Shared / Network SQLite and WebGUI/PostgreSQL coordination implement the **same Artist/Album color and dot policy**. Shared mode determines storage/coordination only, never the policy. |
| `STATE-POLICY-002` | `schema/media-state-classification.json` is the canonical checked-in contract. Windows may embed this **same** JSON for offline operation; it is **not** a different standalone policy. |
| `STATE-POLICY-003` | Existing credential-directory files, PostgreSQL persisted revisions and Windows mirrors are delivery/persistence mechanisms; they must not silently substitute different name rankings, forbidden colors or dot ordering. Flag revision or behavior drift for deliberate reconciliation. |
| `STATE-POLICY-004` | `Unprocessed` is a MEDIA FILTER count, **not** a name-color or dot category. The fallback for an unmatched Artist/Album NAME remains **Red**. |

## Protected behavioral policy JSON

| ID | Approved safeguard |
| --- | --- |
| `GUARD-001` | Protect **both** `schema/media-state-classification.json` and `schema/comp-report-policy.json` in the independent canonical-policy PR gate. A diff, deletion or rename must trigger a **failure and owner-policy review**. |
| `GUARD-002` | A failed check directs Codex to `AGENTS.md`, this register and [POLICY-INDEX.md](POLICY-INDEX.md). It **does not automatically revert** files or allow a test pass to stand in for approval. |
| `GUARD-003` | Owner authorization must be explicit, narrow and auditable. GitHub required checks and rulesets remain independent; owner consent does **not** forge a green status or implicitly bypass a technical restriction. |

## Codex governance — requested, tracked in PR #217

The owner directed mandatory **topic branch → Draft PR** rather than
direct commits to the long-lived branches. The owner may authorize a
specific exception **only after Codex asks**, identifying the current PR
and exact action. The approval is valid **for that PR and action only**
and expires on PR closure/merger/cancellation.

This process is proposed in [draft PR #217](https://github.com/scottia/S-P-L-I-N-E-D/pull/217)
and becomes active in `AGENTS.md` only after approved merge; it is **not**
a blanket exception for the current draft PR or to the JSON review gate.

## Other contracts and validation boundaries

The owner has accepted `schema/comp-report-policy.json` as a **working
COMP baseline**, not ordered speculative rewrites to its thresholds.
It is checked in, structurally inspected and protected, but its detailed
runtime equivalence across Windows/WebGUI remains a test requirement.
`schema/media.sql` and `schema/settings-ui.json` were structurally
validated and indexed; database migration execution and end-to-end UI
parity remain unverified. These are **validation tasks, not requests to
approve the color rules again**.

**Implementation outstanding:** The approved rules still require
verification of actual on-screen name classification, Album/Artist
evidence, preferred embedded size, and effective PostgreSQL/Windows
policy revisions. Treat all unresolved technical gaps as implementation
work—not as revocation of these owner approvals. Do not silently modify
live data or `main`.
