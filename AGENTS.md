# S:P:L:I:N:E:D — mandatory Codex operating contract

**STOP: READ THIS FILE IN FULL BEFORE ANY REPOSITORY WORK.** These instructions
apply to Codex on `dev`, `main`, any topic branch, and any promotion PR.
They are mandatory agent instructions; actual prevention of direct pushes or
merges additionally requires GitHub-enforced rulesets. Never describe a
written rule as technically enforced unless its GitHub protection is verified.

## 1. Owner authority and non-negotiable preflight

1. Before any repository action, read `AGENTS.md` and the complete relevant
   owner-approved `docs/POLICY-REGISTER.md`, plus `docs/POLICY-INDEX.md`,
   `docs/SCHEMA-VALIDATION.md` and affected source/schema/workflow files.
   For lifecycle work, also read `docs/lifecycle-policy.md` and the current
   checked-in `schema/lifecycle-policy.json` alongside the register.
   This reading is the mandatory first action, not optional post-hoc review.
2. Identify the repository, base/head SHA, local uncommitted work, related
   existing PRs, owner-requested scope and stable policy IDs. Never destroy
   or overwrite uncommitted edits. Reuse an existing matching Draft PR.
3. Distinguish **approved policy** from **observed implementation**, old
   documents, tests, installed databases and guesses. Source and tests are
   evidence, not independent approval. Never choose one runtime's behavior
   over the owner's requirement or silently import stale `main` policy to
   supersede an approved `dev` correction.
4. Before a Draft PR exists, permitted activity is **read-only preflight**
   and the minimum GitHub setup needed to create that Draft PR. No
   application/source/documentation corrections, migrations, tests that
   mutate state, data repair, releases, or other substantive actions may
   precede the PR. If required information or access is missing, STOP and
   report it; do not work around the gate.

## 2. Draft PR must exist BEFORE implementation

1. Create a short-lived topic branch from the explicitly approved base,
   normally `dev`. GitHub requires a different branch/commit to open a PR:
   only a **minimal, nonbehavioral PR-intent/bootstrap commit** is permitted
   before opening the Draft PR. Remove the temporary intent file before
   requesting final review. Do not use bootstrap to change behavior.
2. **Open the Draft PR immediately**, before editing product code, policies,
   configuration, workflows, or permanent documentation. Record PR number,
   head/base branches and exact SHA. If a relevant Draft PR already exists,
   update that PR instead of making a duplicate.
3. All subsequent edits and test results belong to that PR's topic branch.
   NEVER push directly to `dev`, `main` or `dev-macos`, force-update an
   owner's branch, create undocumented commits, or bypass review.
4. PR description must disclose owner-authorized scope, policy IDs, all
   changed files, database/schema/policy impact, actual test results and
   gaps, remaining risks, and the specific next owner decision. Record
   material approvals in the PR; keep changes restricted to the agreed scope.
5. An owner-requested `dev` → `main` promotion may explicitly use
   `dev` as the PR head (for example, Draft PR #221). This **does not**
   authorize a direct branch update, an automatic conflict resolution,
   an implicit policy revert, a PR merge or a release.

## 3. Owner-approved media-state policy — both Windows and WebGUI

The **single** behavioral contract is
`schema/media-state-classification.json`; its owner-approved meaning and
full evidence requirements are in `docs/POLICY-REGISTER.md`. The same
rules govern standalone SQLite, shared/network SQLite, and WebGUI with
PostgreSQL coordination. No mode-specific color policy is permitted.

| Display surface | Highest → lowest priority | No matching category |
| --- | --- | --- |
| Artist FILE-LIST NAME | **Red > Yellow > Purple > Green** | **Red** |
| Album FILE-LIST NAME | **Dark Grey > Red > Yellow > Purple > Orange > Green** | **Red** |
| Album CARD status dots | **Dark Grey > Red > Yellow > Purple > Orange > Green > Blue** | Show every applicable dot, in order |

- **NEVER render an Artist or Album NAME White**, blank/neutral, or copied
  from the theme. Both names must receive their **independent** policy
  classification; an unmatched name is Red, not an invented category.
- Artist NAME may **never** be White, Dark Grey, Orange, Gold or Blue;
  Album NAME may **never** be White or Blue. Processed/Bypassed/Override
  filters and COMP track colors do **not** confer those name colors.
- **Yellow:** missing applicable `artist.*` or `banner.*` Artist Art,
  or ordinary static Album `cover.*`, **excluding `cover.webp`**.
- **Purple:** Album/track/COMP track embedding issues. For Album names,
  evaluate missing, below-preferred-size and above-preferred-size
  thumbnails against the configured preferred embedded size.
- **Green:** verified applicable artwork/tracks complete or overridden,
  without a higher-ranked deficiency. An Album opposing override
  selects the **higher-ranked Dark Grey NAME**, even if a Green
  completion status dot is independently applicable. Artists aggregate
  Album/Track/COMP evidence; never copy one Album's NAME color as the
  Artist name.
- `Unprocessed` is a **MEDIA FILTER numeric count only**. Do not turn
  the raw internal `unprocessed` label into a NAME category/status dot
  or independent Red-error evidence. Count it from its intended
  inventory, separately from ranked name-color classification.
- An animated `cover.webp` adds the auxiliary **Blue Album dot only**:
  it cannot satisfy the static-cover requirement or turn a NAME Blue.
- All applicable Album status dots are displayed simultaneously in
  canonical priority order, even when the Album NAME selects only
  one color. Preserve the owner's accepted card frames, banners and
  Blue/Green/Purple COMP-track row colors independently.

These rankings and separations have **already been approved**. Never
request new approval merely to restate or document them; investigate
implementation drift independently against the exact policy.

## 3a. Owner-approved Artist presentation and physical Art

- **ART-001 — One visible Artist presentation:** duplicate normalized Artist
  names collapse to one visible Artist-list row. Different, duplicate, missing,
  or conflicting MusicBrainz Artist IDs on associated Albums do not create
  additional visible Artist rows.
- **ART-002 — Preserve Album identity:** presentation grouping is not a data
  merge or MBID rewrite. Preserve every distinct underlying Album association,
  credit and identifier, and display each distinct Album card once.
- **ART-003 — Resolve physical Artist Art across aliases:** inspect the bounded
  owned Artist folders and managed Artist-Art packages for every row in the
  presentation group. The canonical row's MBID or path cannot hide valid
  `artist.*` or `banner.*` saved under another grouped row.
- **ART-004 — Shared Artist identity and credit resolution:** both clients use
  one ordered catalog method while retaining native storage: (1) capture
  original local metadata evidence; (2) establish durable Artist identity and
  physical Album ownership; (3) resolve category overrides before folder
  classification; (4) resolve explicit multi-value credits, ordered MBID
  cardinality and positional associations before collaboration separators;
  (5) establish distinct ownership, Album-appearance and track-credit edges;
  (6) apply the shared catalog presentation normalization; (7) group normalized
  identities and resolve only locally evidenced, unambiguous aliases; (8) apply
  identical deterministic display-label precedence; (9) determine Artist
  membership eligibility; (10) apply secondary-provider enrichment without
  membership authority; and (11) apply equivalent typed search semantics and
  unique presentation/association counting. Preserve all durable keys, MBIDs
  and edges. Catalog presentation, search-token and COMP-match normalization
  remain separate namespaces; the independent COMP matcher is unchanged.
- **UI-001 — Publish local progress immediately:** Header, Embedding Center,
  status bar and equivalent progress surfaces must update from newly committed
  local state without waiting for remote coordination polling. Reconcile an
  opposing client asynchronously and never invent completion.

These are display and discovery rules only. Never merge or delete catalog rows,
rewrite MusicBrainz IDs, coalesce unrelated Albums, or scan an unbounded library
to make the presentation appear correct.

## 3b. Universal artwork-only media-write authority

The owner-approved `AUDIO-INTEGRITY-001..006` requirements in
`docs/POLICY-REGISTER.md` prohibit **every** non-artwork audio-tag mutation.
SPLINED is an artwork app, not a tagger. An embedded front artwork operation
may update the intended picture only; preserve incoming ID3v2.x version,
multi-value boundaries, unknown/custom frames, every MusicBrainz extended
field, non-front artwork and audio bytes. **Never convert an ID3 version**.

The independent `schema/comp-report-policy.json` retains fuzzy Artist/Recording
discovery, result percentage ranking, single-track COMP artwork selection,
batch orchestration, identity requirements and coordination rules. It grants
**no permission to write MusicBrainz IDs to audio files**. Never turn a
selected alternate cover or manually corrected identity into audio metadata.

All Windows and WebGUI media writers must stage and verify before publication.
If preservation is not provable, reject the write with the original untouched.
Maintain mandatory real-writer and deliberately failing mutation tests in
required PR validation, independent of fast/strict validation choices.
Workflow 10 is the owner-designated active unified build/release path; it
packages an exact previously approved commit and must not rerun either
client's PR-level Audio Integrity suite solely because a build is selected.
Workflow 01 remains manually dispatchable but is operationally superseded.
A source change that introduces audio-tag editing is prohibited; changing
tests, policy JSON or a workflow to hide it is not an approved fix.

**Immutable WebP source:** Existing `cover.webp` is read-only (including its
animation and exact bytes); the only permitted derivation is a separate static
`cover.jpg` still after an explicit artwork WRITE. Do not rename, remove,
overwrite or re-encode the original WebP. Require real Windows and WebGUI
folder-artwork tests that reject an attempted WebP replacement.

## 3c. Shared lifecycle contract and WebGUI-specific acceptance

- The owner-directed `LIFE-001..009` shared behavioral acceptance contract is
  defined in `schema/lifecycle-policy.json` and the authoritative
  `docs/POLICY-REGISTER.md`, with source and platform distinctions in
  `docs/lifecycle-policy.md`. It applies to **equivalent supported Windows and
  WebGUI operations**; it does not require identical code, job tables or UI.
- The separately approved `WEB-LIFE-001..009` rules remain WebGUI-specific
  refinements governing browser interactions, Python workers and
  PostgreSQL-owned operations. **GUI-only** remains the complete WebGUI
  application, not Windows Rust/Slint; shared `LIFE` outcomes do not broaden
  the scope of any existing WebGUI-only implementation PR.
- The JSON remains a **declarative behavioral contract**. Native Rust and
  Python consumers now load and fail-closed validate its identity, client
  scope, nine stable requirements and WebGUI refinement mapping before client
  database/catalog startup. The independent canonical-policy gate protects
  the accepted file. This source integration does not copy data between SQLite
  and PostgreSQL, establish automatic revision mirroring, or by itself prove
  every behavior or installed-client parity; see `docs/lifecycle-policy.md`.
- Preserve operation generation authority, bounded nonblocking work, durable
  queue/nonces, honest stage/completion status, current Artist/Album evidence,
  configured COMP embedded-size authority, and cross-client DB ownership.
- Selection, focus or transient hydration must not invent a new Artist/Album
  name classification: preserve existing verified state until current evidence
  establishes a different result. Canonical ranking and unmatched defaults
  retain their approved meanings.
- Preserve Windows-owned synchronized field authority. Unresolved cross-client
  data semantics require separate owner review, never unilateral policy edits.
- Do not amend `AGENTS.md`, normative policy documents or protected policy
  JSON merely because Codex implements or validates a product change.
  Owner-approved **separate documentation scope** is required; report policy
  differences and preserve normative wording while making application fixes.
## 4. Protected contracts and source ownership

- BOTH `schema/media-state-classification.json` and
  `schema/comp-report-policy.json` are protected behavioral contracts.
  An edit/addition/deletion/rename must fail the independent
  **Canonical policy JSON — review gate**. On failure STOP; review the
  exact diff and owner policy. An intentional revision requires the
  separately owner-approved, independently enforced authorization
  procedure. Never edit the gate in the same PR to evade it, forge a
  pass, or claim an owner message turns a failed check green.
- `schema/lifecycle-policy.json` is the shared behavioral lifecycle
  specification. It supplements the two existing canonical JSONs without
  replacing their authority. Native Windows and WebGUI consumers validate the
  checked-in file locally before catalog/database initialization, and the
  independent JSON review gate rejects its add/change/delete/rename. Revisions
  still require a separate explicit owner-reviewed policy scope; validation is
  not permission to synchronize SQLite/PostgreSQL or claim unmeasured
  installed parity.
- `schema/media.sql` is SQLite DDL; `schema/settings-ui.json` is the
  Config v5 UI contract, not another behavior authority. Runtime JSON
  mirrors and persisted PostgreSQL policy revisions are delivery
  mechanisms; report drift without silent rewrite.
- Supported application roots are `build/Windows/` (Rust/Slint,
  authoritative `<scan.cache_dir>/splined.db` for synchronized
  Artist/Album/Track fields) and `build/WEBGui/` (Python/browser,
  PostgreSQL browser catalog/jobs/audit). Respect
  `build/WEBGui/webgui/catalog_authority.py` and documented write
  coordination. Never mutate real music, MBIDs, SQLite, PostgreSQL or
  credentials as a side effect of a documentation or code audit.
- The Windows first-run Config v5 example is
  `build/Windows/config.example.toml`; WebGUI Docker's is
  `build/WEBGui/docker/config.example.toml`. Shared Git-cliff is
  `release/cliff.toml`. Do not restore retired root-native Linux CLI,
  root Config, root fixtures, root cliff or obsolete source paths.

### Non-negotiable audio write authorization

**AUDIO-INTEGRITY-001..006** in `docs/POLICY-REGISTER.md` bind all Rust/Slint Windows and Python WebGUI code. SPLINED is an **artwork-only manager, never an audio tagger**. The only authorized alteration to an audio file is its selected embedded front picture. ID3v2.3/v2.4 input must retain its original version and exact non-artwork frame/value/multivalue structure; no version conversion, tag normalization, MusicBrainz metadata rewrite or audio-stream alteration is allowed. When frame-level verification cannot prove preservation, the artwork operation must fail without replacing the original. Never add an exception based on existing code, MBID matching, or a successful artwork readback.

**COMP is a separate read/matching/artwork-selection policy.** Preserve `schema/comp-report-policy.json` entirely, including percentage-based results, fuzzy Artist recovery, batch revision pinning, verified-write events and unresolved-authority coordination. **COMP must never write MusicBrainz metadata to any media file.** The same universal non-artwork metadata prohibition applies to ordinary Album thumbnail embedding. Keep the canonical media-state JSON, existing classifications, status dots and COMP-track colors unchanged.

Required PR validation must run **mandatory fail-closed real-writer audio integrity tests for every affected client**, even in fast/validation-only mode. WebGUI runtime changes require the WebGUI writer test; Windows runtime changes require the Windows writer test; shared artwork policy/schema or validation-gate changes require both. A selected required test that is skipped is not a pass. A synchronized numeric release-metadata-only change requires the WebGUI writer and full release-contract validation but does not require the unchanged Windows writer compile. Workflow 10 is the package/build cycle for an exact already-approved commit and must rely on that commit's required PR results rather than rerun either writer suite. A standalone schema JSON validation or optional Windows advisory does not certify audio write safety. Protect integrity tests, writer guards and required PR CI checks from removal or dilution without separate, narrow owner authorization.

## 5. Required PR validation and merge stop conditions

**No PR merge until ALL of these are true:**

1. The Draft PR has been deliberately moved to review-ready, its exact
   head/base SHA and intended merge result are known, and mergeability is
   confirmed **without unresolved conflicts**. Recheck after every update.
2. **Every applicable required GitHub check** for the PR's latest
   head/merge result has **completed successfully** (not queued, running,
   failing, cancelled, timed out or stale). Skipped jobs count only when
   explicitly non-applicable; never misrepresent a skip as a pass.
3. The protected JSON gate passed through its proper independent process;
   no failure was hidden by an exception or changed workflow.
4. Required application tests and evidence match the change scope.
   The Windows PR advisory is **NOT** a native Rust/Slint compile.
   Promotion/release acceptance requires separate relevant native
   Windows validation, WebGUI Python/JS/PostgreSQL/Docker validation,
   version reconciliation and, if affected, disposable-data
   migration/synchronization checks.
5. The repository owner explicitly authorized **this PR number and
   this merge action AFTER reviewing the results**. Do not auto-merge,
   assume approval from prior PRs, rely on silence, or treat a completed
   check as authorization.

If any condition fails, **STOP**, identify the blocker and update the
same PR through the approved review path. Do not merge first and repair
later. A release, Store submission or GHCR publication requires its
own explicit authorization after promotion.

## 6. Owner-scoped exceptions, never check bypasses

An exception requires Codex to **ask first**, naming the current PR
number/URL, branches/SHAs, exact restricted step, reason, impact,
risks and proposed checks. Only an explicit affirmative owner decision
for that particular PR and action is valid; record it in the PR.
Approval expires when that PR closes/merges/cancels and is not reusable.
An exception cannot silently broaden scope, circumvent actual GitHub
protections, bypass a failing check, or redefine owner policy.

Repository branch rulesets must independently require pull requests,
the protected policy check and appropriate other status checks,
prevent unauthorized direct updates, and disallow merge while required
checks are unsuccessful. `AGENTS.md` cannot itself configure or prove
these GitHub protections; confirm them in repository settings before
claiming mechanical enforcement.

## 7. Final evidence and security

Record the PR URL/head SHA, approved policy IDs, before/after behavior,
files, tests actually run and skipped, unresolved issues and owner
decision. Source-level, documentation and CI checks are not proof of
real installed-client parity. Preserve secrets and unpublished local
edits; never expose tokens, credentials, private settings or machine
paths. Application Help intended for end users must link to separately
reviewed `main` documentation, not unpromoted `dev` guides.
