# S:P:L:I:N:E:D — owner-approved policy register

**Authority:** The repository owner's instructions, reconfirmed November 20,
2026. The rules below have **already been given and approved**. No repeat
owner approval is required to state or document them. The register is
normative; current code, test assertions, saved database copies and old
PR documents are not alternate policy authorities.

**Scope:** Artist/Album FILE-LIST **name colors**, ordered multiple Album
status dots, approved visual indicators, policy consistency across
deployment modes, safeguards for changing shared policy JSON, and the
**artwork-only media-write invariant** (both runtimes, including COMP).
For implementation verification, see
[SCHEMA-VALIDATION.md](SCHEMA-VALIDATION.md).

## Owner-confirmed exact classification priorities (November 20, 2026)

**These requirements apply simultaneously to Windows and WebGUI, and to
standalone/shared SQLite or optional PostgreSQL coordination.** The
Artist FILE-LIST NAME, Album FILE-LIST NAME, multiple Album CARD
status dots, and MEDIA FILTER counts are **four distinct surfaces**.
A selected name color must never suppress another applicable Album dot.

| Surface | Priority from highest to lowest | Forbidden NAME colors | No matching category |
| --- | --- | --- | --- |
| **Artist FILE-LIST NAME** | **Red → Yellow → Purple → Green** | **White, Dark Grey, Orange, Gold, Blue** | **Red** |
| **Album FILE-LIST NAME** | **Dark Grey → Red → Yellow → Purple → Orange → Green** | **White, Blue** | **Red** |
| **Album CARD status dots** | **Dark Grey → Red → Yellow → Purple → Orange → Green → auxiliary Blue** | Not a single name selection; show **every applicable dot** in this order | None unless applicable evidence exists |
| **MEDIA FILTER** | Numeric state counts including Unprocessed, Processed, Bypassed and Override where applicable | **Not NAME classifications** | Calculate from the relevant inventory |

The owner's exact precedence means:

- **Red** denotes an independent Incomplete/Error condition and
  outranks other eligible Artist/Album name colors. When **no**
  category qualifies, the name still defaults to **Red**, not White,
  neutral, a missing value or an inferred new category. The internal
  processing label `unprocessed` **alone is not independent Red
  error evidence**.
- **Yellow** denotes missing applicable `artist.*` or
  `banner.*` Artist Art, or missing ordinary Album static
  `cover.*` **excluding `cover.webp`**; it outranks Purple and
  Green in Artists, and Purple/Orange/Green in Albums.
- **Purple** denotes applicable Album/Track/COMP-track embedding
  issues. For Album embedded thumbnails, missing, below-preferred-size
  and above-preferred-size results all count as issues relative to
  the configured preferred thumbnail size. Purple outranks Orange
  and Green for an Album name and Green for an Artist name.
- **Orange** is the Album processed-name state, below Purple and
  above Green. It is **never an Artist NAME color**. **Gold** bypass
  and **Dark Grey** opposing override remain excluded from Artist
  NAME color even if displayed elsewhere in MEDIA FILTER or frames.
- **Dark Grey** opposing-app override is the **highest-priority
  Album NAME color**. Completed/overridden Album evidence may
  independently contribute to an Artist aggregate's Green result;
  it does not make an overridden Album NAME Green while Dark Grey
  evidence applies.
- **Green** requires verified relevant covers/tracks complete or
  appropriately overridden and no higher-ranked deficiency. An
  Artist aggregates applicable Albums/tracks/COMP evidence rather
  than copying the name color of one Album.
- **Blue** is an **auxiliary Album status dot** when animated
  `cover.webp` exists, never a NAME color and never a replacement
  for ordinary static cover art. All applicable Album dots remain
  colored and visible in their prescribed order.

The owner confirmed that no existing runtime implementation, prior
PR, historical `main` note or filter-count calculation may silently
override these requirements. Their policy approval is **complete**;
remaining tests assess implementation conformance, not policy consent.

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

## Artwork-only audio integrity — permanent owner-approved contract

S:P:L:I:N:E:D is an artwork application, **never an audio metadata tagger**.
Its sole authorized *audio-file* mutation is embedding an approved front-cover
picture. Eligible Albums use the preferred embedded thumbnail size in Settings;
Curated Compilation COMP writes remain single-track events under the existing
separate `schema/comp-report-policy.json`. Folder/Artist artwork files are
managed separately and do not authorize any audio-tag edits.

| ID | Mandatory product rule |
| --- | --- |
| `AUDIO-INTEGRITY-001` | The only permitted audio-file mutation is approved front artwork; all text, custom, MB extended and other non-artwork tags are immutable. |
| `AUDIO-INTEGRITY-002` | Preserve the exact incoming ID3v2.x version (including v2.4), field/frame data, separate multivalues, non-front pictures, and every MusicBrainz tag. ID3 support does **not** permit any version conversion, reserialization changes, slash joining or normalization. |
| `AUDIO-INTEGRITY-003` | Write only to a staged copy and compare its full non-artwork metadata and audio stream against the original **before** replacement. Any mismatch, unreadable tag, or unsupported verification must fail closed without replacing the original. |
| `AUDIO-INTEGRITY-004` | Enforce the same restriction in Windows Rust/Lofty and WebGUI Python/Mutagen across Album thumbnail queues, COMP/Fallback, and all supported audio containers; a selected COMP track's artwork may change, never its metadata. |
| `AUDIO-INTEGRITY-005` | Explicitly prohibit **all** MusicBrainz Recording/Artist/Release and extended-tag writes in COMP. Alternate covers, fuzzy matches, Artist/Recording lookup and manual ID edits are search/session/database information only. Preserve the separate COMP policy's percentage bands, completion authority, pinned revisions, reporting and coordination flags. |
| `AUDIO-INTEGRITY-006` | Fail PR and Workflow 01/04 release/validation when audio-integrity tests fail, are skipped or missing; fast/strict Windows modes may not bypass this requirement. Mandatory negative controls must reject source-tag corruption and preserve the original file. |

These rules authorize **no additional tag editing**. They do not rewrite either
protected JSON contract. COMP `publish_verified_artwork_write=true`,
`publish_unresolved_identity_write=true` and
`completion_requires_authoritative_ids=true` remain unchanged: valid artwork
can be published while identity is unresolved, without a successful completion
record or any audio MBID write. The media-state JSON continues to govern name
and dot classification; a failed staged write cannot produce Green/completed
status or verified-write coordination.

## Protected behavioral policy JSON

| ID | Approved safeguard |
| --- | --- |
| `GUARD-001` | Protect **both** `schema/media-state-classification.json` and `schema/comp-report-policy.json` in the independent canonical-policy PR gate. A diff, deletion or rename must trigger a **failure and owner-policy review**. |
| `GUARD-002` | A failed check directs Codex to `AGENTS.md`, this register and [POLICY-INDEX.md](POLICY-INDEX.md). It **does not automatically revert** files or allow a test pass to stand in for approval. |
| `GUARD-003` | Owner authorization must be explicit, narrow and auditable. GitHub required checks and rulesets remain independent; owner consent does **not** forge a green status or implicitly bypass a technical restriction. |

## Codex governance — active in AGENTS.md

The owner directed mandatory **topic branch → Draft PR** rather than
direct commits to the long-lived branches. The owner may authorize a
specific exception **only after Codex asks**, identifying the current PR
and exact action. The approval is valid **for that PR and action only**
and expires on PR closure/merger/cancellation.

This process was installed on `dev` by
[PR #217](https://github.com/scottia/S-P-L-I-N-E-D/pull/217).
It is **not** a blanket exception for any PR or the protected JSON gate.

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
