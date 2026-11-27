# S:P:L:I:N:E:D — owner-approved policy register

**Authority:** The repository owner's instructions, reconfirmed November 20,
2026. The rules below have **already been given and approved**. No repeat
owner approval is required to state or document them. The register is
normative; current code, test assertions, saved database copies and old
PR documents are not alternate policy authorities.

**Scope:** Artist/Album FILE-LIST **name colors**, ordered multiple Album
status dots, approved visual indicators, policy consistency across
deployment modes, shared Windows/WebGUI lifecycle behavior, safeguards for
changing shared policy JSON, and the **artwork-only media-write invariant**
(both runtimes, including COMP).
For implementation verification, see
[SCHEMA-VALIDATION.md](SCHEMA-VALIDATION.md).

## Approved Artist identity presentation and saved Artist Art

These rules govern the visible Artist projection without changing catalog
identity or MusicBrainz data.

| ID | Approved requirement |
| --- | --- |
| `ART-001` | In the Artist listing, duplicate normalized Artist names collapse to one visible Artist presentation. Different, duplicate, missing, or conflicting MusicBrainz Artist IDs on associated Albums do not create additional visible Artist rows. |
| `ART-002` | Artist display grouping is not an Artist-MBID rewrite rule. Preserve distinct Album associations, per-Album identities and legitimate credits; display each distinct Album card once and never coalesce unrelated Albums. |
| `ART-003` | Discover saved `artist.*` and `banner.*` through the established bounded inventory/refresh path across every owned folder and managed package in the grouped presentation. A canonical row's missing Art, release MBID or Artist MBID cannot hide valid physical Artist Art stored under another grouped row. |
| `ART-004` | **Shared Artist identity and credit resolution.** Windows and WebGUI shall apply one ordered Artist catalog methodology while retaining native storage: **(1)** capture original local Album Artist, Track Artist multi-values, ordered MusicBrainz Artist IDs, Album identifiers, path and category evidence with provenance; **(2)** establish the durable Artist identity and exactly one physical Album owner from Album Artist MBID, otherwise normalized Album Artist name, using Track Artist and then physical folder only when Album Artist is absent; **(3)** apply an explicit Album-category override before configured folder classification; **(4)** resolve native multi-value credits first, then exact ordered MBID cardinality and positional associations, then exact owner-anchored evidence, and only then configured collaboration separators or strictly bounded filename inference; a nonzero mismatched MBID count cannot manufacture positions; **(5)** establish separate durable-owner, Album-appearance and positioned track-credit associations without reparenting; **(6)** apply the shared NFKC, Unicode-casefolded, alphanumeric catalog presentation normalization; **(7)** group equal normalized identities and resolve same-MBID/different-name aliases only from nonambiguous local owner or positionally paired credit evidence, never provider-only, absent or conflicting IDs; **(8)** select one deterministic local display label in this precedence: positionally paired local credit, MBID-bearing local Album Artist, greatest unique owned Artist-category Album evidence, stable durable-key tie-break, then stable normalized/exact-label tie-break; **(9)** establish membership only from an Artist-category owner or explicit eligible credit, while V/A, OST, fallback and curated-only evidence may enrich an already eligible presentation; **(10)** apply secondary-provider metadata or mapped appearances only after local eligibility and never as independent membership, alias or ownership authority; and **(11)** use the same complete presentation/alias vocabulary and Unicode search token for typed Artist search, and count unique eligible presentations and unique association edges from one inventory generation. Preserve every underlying key, MBID, conflict and Album/track edge. Catalog presentation identity, search token and the protected COMP match identity are separate namespaces; ART-004 does not amend the independent COMP matcher. Numerical equality alone is not acceptance evidence. |
| `UI-001` | Windows and WebGUI Header, Embedding Center, status bar and equivalent progress surfaces publish the newest locally committed state promptly. Remote coordination polling must not block unrelated rendering; opposing-client reconciliation remains asynchronous. |
| `SAFE-001` | Multiple MBIDs or duplicate visible rows are diagnostic evidence, not permission to rewrite identities or merge database rows. Preserve legitimate unrelated Artists and Album associations and report ambiguous mappings. |

`ART-001` groups the presentation key only. `ART-002` and `SAFE-001` preserve
the independent underlying PostgreSQL/SQLite rows and Album keys. This allows
one Beyoncé-style visible Artist entry to retain all distinct Albums and saved
Art without turning name equality into a destructive catalog operation.
`ART-004` is one cross-client method, not permission for client-specific
matching rules behind a common outcome statement. Both native implementations
must consume common conformance inputs and produce the same presentation
identities, deterministic labels, membership decisions and unique owner,
Album-appearance and positioned track-credit associations. Its catalog
presentation and search identities remain distinct from, and do not amend,
the independent COMP matching policy.

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

**Retained WebP source rule:** An existing `cover.webp` is immutable source
artwork, never a replacement or cleanup target. Its original bytes and animation
remain intact; an explicit WRITE Cover may derive a separate static `cover.jpg`
still, and audio embeds use independently prepared static thumbnails. An attempt
to overwrite an existing WebP with different content must fail before any
publication. This file-artwork rule is independent of the ID3/audio integrity gate.

| ID | Mandatory product rule |
| --- | --- |
| `AUDIO-INTEGRITY-001` | The only permitted audio-file mutation is approved front artwork; all text, custom, MB extended and other non-artwork tags are immutable. |
| `AUDIO-INTEGRITY-002` | Preserve the exact incoming ID3v2.x version (including v2.4), field/frame data, separate multivalues, non-front pictures, and every MusicBrainz tag. ID3 support does **not** permit any version conversion, reserialization changes, slash joining or normalization. |
| `AUDIO-INTEGRITY-003` | Write only to a staged copy and compare its full non-artwork metadata and audio stream against the original **before** replacement. Any mismatch, unreadable tag, or unsupported verification must fail closed without replacing the original. |
| `AUDIO-INTEGRITY-004` | Enforce the same restriction in Windows Rust/Lofty and WebGUI Python/Mutagen across Album thumbnail queues, COMP/Fallback, and all supported audio containers; a selected COMP track's artwork may change, never its metadata. |
| `AUDIO-INTEGRITY-005` | Explicitly prohibit **all** MusicBrainz Recording/Artist/Release and extended-tag writes in COMP. Alternate covers, fuzzy matches, Artist/Recording lookup and manual ID edits are search/session/database information only. Preserve the separate COMP policy's percentage bands, completion authority, pinned revisions, reporting and coordination flags. |
| `AUDIO-INTEGRITY-006` | **Fail-closed PR validation remains mandatory**: affected WebGUI runtime changes require its real-writer audio-integrity tests, Windows runtime changes require its tests, and shared artwork policy/schema or validation-gate changes require both. Synchronized numeric release-metadata-only changes retain their accepted WebGUI writer plus release-contract validation without recompiling unchanged Windows writer code. A selected required test that fails, is skipped or is missing is not a pass. **Workflows 01–03 are the owner-designated official `main` release workflows**; each validates and remembers its candidate internally, retains a valid Build Report, builds only its named target set, and allocates the next available numeric `main` release tag without consuming Workflow 20 `/dev` evaluation artifacts. Mandatory negative controls reject source-tag corruption while preserving the original file. |

These rules authorize **no additional tag editing**. They do not rewrite either
protected JSON contract. COMP `publish_verified_artwork_write=true`,
`publish_unresolved_identity_write=true` and
`completion_requires_authoritative_ids=true` remain unchanged: valid artwork
can be published while identity is unresolved, without a successful completion
record or any audio MBID write. The media-state JSON continues to govern name
and dot classification; a failed staged write cannot produce Green/completed
status or verified-write coordination.

## Shared Windows/WebGUI lifecycle contract — owner-directed review (November 23, 2026)

The **common lifecycle source contract** is
[`schema/lifecycle-policy.json`](../schema/lifecycle-policy.json). It defines
equal **behavioral acceptance outcomes** for equivalent supported operations
in Windows Rust/Slint/SQLite and WebGUI Python/browser/PostgreSQL, without
requiring identical UI code, token/queue primitives, or database engines.
See [Shared lifecycle policy](lifecycle-policy.md) for runtime source mapping,
native differences, acceptance and deployment limitations.

| ID | Shared acceptance requirement | Evidence required |
| --- | --- | --- |
| `LIFE-001` | **Current operation ownership.** Use client-native generations, tokens or equivalent ownership to prevent superseded search, selection, preview, report, and queued decisions from committing stale UI state. | The newest valid selection or operation wins even when asynchronous responses complete out of order. A stale result cannot overwrite the current Artist, Album, artwork, report, status, or queue decision. Safe navigation remains available; only conflicting actions are blocked. |
| `LIFE-002` | **Usable catalog before secondary work.** Publish already indexed/published Artist and Album presentation promptly; defer unrelated thumbnail decoding, remote metadata/provider results, coordination and full candidate inspection. | The initial usable list and selected overview do not await unrelated secondary work. Equivalent direct and related-Artist navigation converge on the correct current identity. Startup and selection responsiveness are compared against accepted measured baselines, without inventing fixed latency requirements. |
| `LIFE-003` | **Bounded background work.** Bound parallel discovery, thumbnail, polling, probe, search and coordination work; coalesce identical requests and supersede or cancel obsolete work using each client's mechanisms. | No overlapping duplicate requests or unbounded polling loops. Background activity becomes idle or backs off and cannot starve interactive work. Retries do not manufacture new processing or ownership events. |
| `LIFE-004` | **Durable queue and decision integrity.** Represent eligible discovery, selection, staged, running, review, verification and terminal states according to each client's authoritative queue. Never treat a stale or invalid decision as valid. | Queued or staged work has a visible next state, explicit decision, or terminal outcome; it is not indefinitely stranded behind a hidden gate. Cancelling or changing context reconciles pending UI against authoritative durable work. A stale or duplicate action cannot skip extra Albums, bypass nonce/token validation where required, or publish an unverified result. |
| `LIFE-005` | **Phase-accurate progress and completion.** Progress belongs to its active operation and reflects observed work; staging/ready, embedding, verification, successful completion, cancellation and failure remain distinct. | A late provider, index or coordination message cannot overwrite newer operation status. Only verified media/artwork outcomes appear completed or GREEN; staging alone never claims embedded completion. Where available, report current item, processed/total counts and final outcome without blocking the client. |
| `LIFE-006` | **Evidence-preserving visual classification.** Preserve the owner-approved independent Artist NAME, Album NAME, Album CARD dot and COMP-track contracts while switching views or incrementally hydrating details. | Selection, focus or transient hydration cannot discard verified indexed evidence and manufacture an unmatched RED. Genuine incomplete/error evidence and the Red no-match default remain valid; higher-priority Yellow/Purple evidence remains authoritative. All independently applicable Album status dots and approved frame/banner semantics are retained. |
| `LIFE-007` | **Deterministic search, COMP and thumbnail authority.** Keep Artist search typed to Artists, Album search typed to Albums, and selected COMP/Album decisions pinned through discovery, artwork preparation and verified write, respecting the canonical COMP policy and configured preferred embedded dimensions. | No stale lookup reopens a completed discovery phase or replaces an accepted selected result. Applicable COMP or Album writes derive their embedded square image at the current configured preferred side (1200 px is the default, not a fixed policy). No non-artwork media metadata mutation; preserve immutable source WebP and the artwork-only verification gate. |
| `LIFE-008` | **Database ownership and validation provenance.** Maintain Windows SQLite authority for designated synchronized Artist/Album/Track fields and PostgreSQL ownership of WebGUI-only jobs, validation, audit and coordination; lifecycle progress or a refresh is not a transfer of authority. | No client silently rewrites the other client's protected catalog ownership fields to clear a notice or appear complete. Revalidation/completion notices derive only from the actual successfully validated work; cancelled, failed and unseen records are not claimed as verified. This JSON is a behavioral contract only: it is not database synchronization transport, schema migration or a new data owner. |
| `LIFE-009` | **Shared acceptance without identical implementations.** Equivalent supported Windows and WebGUI workflows must meet the same outcome and failure-boundary acceptance criteria while retaining native UI, job/queue, database and cancellation implementations. | Focused tests cover applicable normal, delayed, stale, double-click, cancellation, restart/recovery and terminal cases. Measure and document genuine runtime parity or deviations independently; a green unit test alone does not prove installed parity. Reuse accepted evidence; do not rerun unrelated real-writer suites for a documentation-only change. |

The existing `WEB-LIFE-001..009` clauses below remain **owner-approved
WebGUI-specific refinements**. The new `LIFE-001..009` clauses apply to
both clients where an equivalent operation exists, rather than copying
WebGUI's browser/nonce implementation into Windows. Shared lifecycle behavior
does **not** move synchronized catalog ownership: Windows SQLite remains
authoritative for designated Artist/Album/Track fields; PostgreSQL retains
its own jobs, audit, revalidation evidence and coordination roles.

**Activation status:** The accepted shared JSON is locally loaded and
fail-closed validated by native Rust and Python consumers before client
database/catalog initialization. The independent canonical-policy review gate
protects it in addition to the existing media-state/COMP files. Focused tests
verify identity, client scope, ordered rule IDs, WebGUI refinement mapping and
add/change/delete/rename rejection. This activates the source contract; it is
not a claim of completed behavioral or installed parity, automatic policy
mirroring, or SQLite/PostgreSQL synchronization. See
[lifecycle-policy.md](lifecycle-policy.md) for evidence and remaining gaps.
## Approved WebGUI lifecycle and interaction contract (November 23, 2026)

**Authority and scope:** The owner approved `WEB-LIFE-001..009` as permanent WebGUI-specific acceptance refinements. The proposed shared `LIFE-001..009` above specify equivalent outcomes on both supported clients. In this context, “GUI-only” refers to the `build/WEBGui/` browser UI, Python backend, processing workers and PostgreSQL-owned operations; it does **not** automatically authorize Windows source changes or edits to Windows-owned synchronized fields.

These rules **supplement**, but do not alter, `ART-*`, `ART-COLOR-*`, `ALB-COLOR-*`, `VISUAL-*`, `STATE-POLICY-*`, `UI-001`, `AUDIO-INTEGRITY-*`, or the protected JSON contracts. An implementation proposal in a PR does not become owner-approved policy merely because code or tests suggest it.

| ID | Approved lifecycle requirement | Acceptance / regression criteria |
| --- | --- | --- |
| `WEB-LIFE-001` | **Operation generation authority.** Each search, selection, preview, report, and processing decision retains its own current generation/token; late or superseded responses cannot change current state. | Rapid clicks, reversed network response order and changing Artists/Albums leave the latest valid Artist, Album, queue decision, preview and status in control; only unsafe conflicting actions are disabled. |
| `WEB-LIFE-002` | **Immediate usable presentation.** Publish indexed Artist lists and selected Artist/Album projections before unrelated thumbnail, provider, COMP, metadata, or coordination hydration. Direct and Similar Artist selection share the same bounded projection. | The usable list/overview renders without awaiting secondary operations; measured performance must not regress from an accepted baseline. No speculative hard latency guarantee is created. |
| `WEB-LIFE-003` | **Bounded asynchronous requests.** Coalesce identical calls; debounce applicable queries; cancel/supersede stale operations; limit concurrent requests and background retries, probes, thumbnails, and coordination polling. | No uncontrolled request fan-out, overlapping identical polls or permanent one-second loops; low-priority work becomes idle and cannot starve interactive work. |
| `WEB-LIFE-004` | **Authoritative queue and decision lifecycle.** Album/COMP discovery, selection, staging, queued/running work, verification and terminal states must follow the applicable durable worker/nonce authority. | No hidden decision gate or stalled staged Album; a stale SKIP/review prompt reconciles to current durable state without reusing an invalid nonce, bypassing protection, or skipping more than one Album. |
| `WEB-LIFE-005` | **Accurate status ownership and completion.** Only the current operation may own progress/status; staging/ready is distinct from successfully embedded and verified completion. | Late index or coordination responses cannot restore old busy text; each phase transitions to correct active/ready/verified/cancelled/failed state with available Album/track counts and current path. Green completion is never inferred from staging alone. |
| `WEB-LIFE-006` | **Canonical Artist/Album presentation.** Preserve ART-COLOR, ALB-COLOR, VISUAL and STATE-POLICY precedence. Selection, focus and partial hydration must not discard valid indexed evidence or manufacture an unsupported classification. | Classification follows current verified evidence across navigation and rendering. Transient loading or incomplete presentation is not an independent error; the established Red default applies only when no ranked condition qualifies. Staged Album indicators preserve all independently applicable dots. |
| `WEB-LIFE-007` | **Deterministic search and COMP stages.** Artist Search returns Artist results; Album Search returns Album results. MusicBrainz/COMP discovery, result choice, artwork preparation and writing are separate stages; respect pinned COMP policy and existing limits. | Current Artist/Album identity stays pinned; stale discovery cannot restart after accepted selection; COMP writes prepare and verify the configured preferred square thumbnail size (1200px is a default, not a fixed requirement), preserving all non-artwork metadata. |
| `WEB-LIFE-008` | **Cross-client database authority.** WebGUI indexing, revalidation, coordination and background operations must preserve authoritative synchronized fields and distinguish verified local evidence from ownership changes. | No WebGUI-only lifecycle operation silently rewrites Windows-owned fields or claims validation without evidence; any unresolved cross-client authority conflict requires separate owner review before implementation. |
| `WEB-LIFE-009` | **Focused lifecycle acceptance.** Every affected change must prove the relevant normal, slow, superseded, cancelled, restarted, recovered and terminal transition paths. | Attach focused tests/evidence to the behavior PR, including stale responses, double-clicks, selection color stability, queue nonce recovery, bounded polling and UI status. Reuse accepted evidence and avoid unrelated build-only reruns; a product PR may not automatically amend owner policy. |

**Policy governance:** Codex may audit and implement owner-approved application corrections in the designated behavior PR, but **must not edit this register, `AGENTS.md`, `POLICY-INDEX.md`, other normative documentation, or protected policy JSON as an automatic side effect of product work**. A separate, explicit owner instruction and review scope are required for policy/documentation revisions. Policy conflicts are reported, not silently repaired by changing the contract.

## Protected behavioral policy JSON

**Independent gate:** The two previously protected behavioral JSONs below
remain subject to their strict fail-closed review gate. The accepted
`schema/lifecycle-policy.json` is now protected by the same independent gate,
with add/change/delete/rename negative tests and no reduction to the existing
protections. Protection is a review boundary, never a SQLite/PostgreSQL
replication mechanism or automatic authorization to revise policy.

| ID | Approved safeguard |
| --- | --- |
| `GUARD-001` | Protect **both** `schema/media-state-classification.json` and `schema/comp-report-policy.json` in the independent canonical-policy PR gate. A diff, deletion or rename must trigger a **failure and owner-policy review**. |
| `GUARD-002` | A failed check directs Codex to `AGENTS.md`, this register and [POLICY-INDEX.md](POLICY-INDEX.md). It **does not automatically revert** files or allow a test pass to stand in for approval. |
| `GUARD-003` | Owner authorization must be explicit, narrow and auditable. GitHub required checks and rulesets remain independent; owner consent does **not** forge a green status or implicitly bypass a technical restriction. |
| `GUARD-004` | Protect `schema/lifecycle-policy.json` independently after its accepted seed. Add, change, delete and rename operations fail closed, while native Rust and Python readers validate the exact checked-in identity, runtime scope, stable `LIFE-001..009` order and WebGUI refinement mapping without creating database synchronization. |

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
