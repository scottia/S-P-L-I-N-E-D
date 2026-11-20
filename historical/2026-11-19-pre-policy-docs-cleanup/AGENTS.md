# SPLINED repository instructions

These instructions apply to the entire repository.

## Mandatory policy review

Before auditing, changing, building, testing, documenting, committing, or
preparing a pull request, read the complete policy register in this file.

- Treat the user's current wording in the policy register as the authoritative
  definition of SPLINED behavior.
- Map every requested behavior change to the applicable policy entry before
  editing source.
- If the request conflicts with the policy, or the relevant behavior is absent,
  blank, or ambiguous, identify the conflict or gap and obtain the user's
  direction before changing source.
- Do not infer a policy change from existing code, tests, database contents,
  release behavior, commit history, or pull requests. Those are implementation
  evidence and may reveal a mismatch, but they do not silently override policy.
- Do not edit the policy register unless the user explicitly asks for a policy
  change.
- Never place credentials, API keys, passwords, credential hashes, private
  configuration values, or machine-specific test paths in the policy,
  repository changes, commits, pull requests, or test reports.

## Policy authority and implementation evidence

The owner approves required behavior; code and data do not. Use three separate
labels in audit material:

- **Approved policy:** explicitly owner-authorized requirements in this
  register or an unambiguous approved scoped contract.
- **Source-observed:** actual behavior traced through the current execution
  path. This is implementation evidence, not approval.
- **Unresolved:** missing, contradictory, ambiguous, or unverified policy.
  Record it and request owner direction before changing dependent behavior.

Never choose Windows over WebGUI, WebGUI over Windows, a JSON default over an
owner decision, or one document over another merely because one already
implements a behavior. An approved rule governs *both* clients wherever
applicable. Differences require explicit policy authorization. Do not turn
source observations, diagnostic database snapshots, historical PRs, or passing
tests into new policy.

Current owner instructions can clarify a rule; preserve their scope and exact
meaning. When existing documents conflict with a more recent explicit owner
decision, flag the stale document for reconciliation rather than silently
enforcing both. Do not edit other contracts without task authorization.

## Required Codex sequence

1. **Preflight:** read this entire file, relevant approved policy documents
   and active JSON contracts, and inspect the working tree before touching
   files. Never reset, replace, or discard unpublished local AGENTS.md policy
   drafts or owner annotations.
2. **Map:** cite stable policy IDs for the request. Identify absent or
   contradictory rules before designing the fix.
3. **Trace:** independently inspect the Windows and WebGUI execution paths,
   database ownership, derived presentations, configurations, and applicable
   tests; record path:line references and revision.
4. **Classify:** distinguish compliant behavior from divergence, policy
   contradictions, evidence gaps, and intentional platform-specific rules.
   Do not describe a source behavior as "correct" until it matches approved
   policy.
5. **Correct only authorized scope:** do not mutate databases, repair MBIDs,
   merge identity rows, alter media tags, perform destructive refreshes, or
   expand migrations merely to make a symptom disappear. Request a decision
   for ambiguous or destructive repairs.
6. **Enforce:** add or update focused regression coverage for the *approved*
   behavior, including negative and cross-client cases. A green test proves
   only what that test asserts; check the assertion against policy.
7. **Report:** policy IDs; source references; before/after behavior; tests
   and results; remaining conflicts; and exact file changes. Never claim
   parity or completeness without evidence. Repository writes and merges
   still require the owner's express authorization.

**Default for unresolved policy is no speculative source correction.** A
diagnostic may identify evidence and propose alternatives, but it must not
choose a new domain rule for the owner.

## Owner-directed requirements and evidence-bound safeguards

The owner expressly directed Artist-name grouping with Album-level MBID
associations, physical Artist-Art discovery, JSON/status distinction, immediate
progress publication, and policy-first verification. The entries below pair
those directions with established scoped repository contracts. **Do not treat
technical elaborations or inferred exceptions as separately owner-approved.**
Where the owner's exact requirement, a scoped contract, and observed behavior
diverge or leave scope uncertain, record an unresolved policy question before
correcting source. These entries are behavioral targets, **not** findings that
either client passes validation.

- **ART-001 — Artist presentation:** in the Artist listing, duplicate
  normalized Artist names collapse to one visible Artist presentation.
  Different, duplicate, missing, or conflicting MusicBrainz Artist IDs on
  associated Albums do **not** create additional visible Artist rows. Keep
  physical folder ownership and underlying Album associations independent of
  the display grouping. Verify Featured, Soundtrack, Various Artists, and
  other scoped presentation exceptions against approved policy before edits.
- **ART-002 — Album associations:** the Artist display grouping is not an
  Artist-MBID rewrite rule. Preserve distinct underlying Album associations,
  per-Album identities, and legitimate Artist credits. Display each distinct
  Album card once; do not coalesce unrelated Albums or promote a misassigned
  MBID into an Artist's canonical identity. A conflicting ID requires an
  evidence-based identity diagnosis, not an automatic name-only data merge.
- **ART-003 — Physical Artist Art:** discover shared `artist.*` and
  `banner.*` under the resolved owned Artist folder through the established
  bounded inventory/refresh/cache path. Blank Album release MBIDs and missing
  Artist MBIDs cannot prevent physical folder or Artist-Art discovery.
  Opening an Artist must not initiate an unbounded filesystem scan.
- **DB-001 — Synchronized catalog authority:** Windows SQLite `splined.db`
  owns synchronized Artist/Album/Track catalog identity, names, paths, MusicBrainz
  and release metadata, inventory, and processing/bypass/timeout fields, as
  scoped by `docs/catalog-authority.md`. WebGUI PostgreSQL retains its own
  jobs, audit, authentication, validation and browser-specific state. Ownership
  projections into PostgreSQL must follow the existing explicit, safe
  reconciliation protocol; a PostgreSQL connection or Full Refresh never
  transfers synchronized-field catalog authority away from SQLite.
- **DB-002 — Coordination versus reconciliation:** writer coordination is
  durable provenance, not a catalog overwrite or identity merge. Apply
  opposing COMP-track writes at track scope only; never inflate them into an
  Album-level OVERRIDE, completed Album, or rewritten Artist identity.
  Preserve valid local processing/completion history until verified
  authority justifies a transition.
- **STATE-001 — JSON-backed presentation:** active validated shared
  classification policy governs applicable Artist/Album name colors,
  precedence, status dots, and override presentation. Hard-coded CSS or
  native UI shortcuts must not override the effective JSON policy.
  The White/Unprocessed **count** is not a rule that an unprocessed Artist
  **name** must be white. Artist names must follow the approved shared
  Artist-name classification and its allowed colors.
- **STATE-002 — Independent aggregates:** Artist display state, Album
  processing state, missing Artist Art, COMP-track progress, and
  opposing-writer provenance are distinct evidence. Derive displayed
  aggregations from their approved priority and eligibility rules rather
  than copying colors or completion states between scopes.
- **UI-001 — Immediate status/progress:** Windows and WebGUI Header,
  Embedding Center, Status Bar, processing notices, and equivalent progress
  surfaces update promptly from the newest locally committed state.
  Coordination polling, reconciliation, slow per-row queries, and remote
  checks must not block unrelated progress rendering. Reconcile eventual
  opposing-client state asynchronously without inventing completion.
- **SAFE-001 — Identity/reconciliation safety:** multiple MBIDs or duplicate
  visible rows are diagnostic evidence, not proof of corrupt catalog authority.
  Preserve legitimate unrelated Artists and Album associations. Report
  ambiguous mappings instead of guessing, auto-merging, replacing IDs, or
  performing unapproved bulk repairs.
- **PARITY-001 — Policy-first parity:** verify each client against approved
  policy independently. Matching another client's code, UI, or tests is
  insufficient if both conflict with the governing policy.

## Known documentation disagreements to surface, not silently codify

- `docs/media-filter-status-colors.md` describes White physical Artist names
  for all-unprocessed folders, while
  `docs/media-state-classification-policy.md` forbids White Artist names.
  **STATE-001** establishes the owner's distinction between Unprocessed
  counts and Artist-name color; the older text needs a separate authorized
  documentation reconciliation.
- `docs/catalog-authority.md` describes a red opposing-override frame while
  `docs/media-state-classification-policy.md` describes a dark-grey frame.
  Do not select a frame rule from whichever client currently renders it;
  verify the owner's effective approved policy before any correction.
- Physical-folder ownership, tagged Artist identities, and visible
  normalized-name aggregation are different concerns. Use
  `docs/splined-media-database.md` for the indexed/physical distinction,
  and **ART-001/002** for the required display and association boundaries.
- Inspect live revisioned PostgreSQL and validated local JSON policy mirrors
  before claiming a documented default is actually active.

## Mandatory complete policy-inventory scope

For a comprehensive AGENTS.md register audit, inspect current `origin/dev`
source—not just the preexisting register count—and record each independently
meaningful rule with exact path:line evidence and a stable ID. Include:

- Provider registration, enabling, priority, eligibility, exclusion, fallback,
  timeouts, rate limits, retries, credential safety, and source authority.
- MusicBrainz Artist/Album/Release/Recording identity, MBID verification,
  release grouping, queue order, scoring/ranking, candidate selection,
  deduplication, caching, and negative-result behavior.
- Local artwork, `cover.*`, `artist.*`, `banner.*`, embedded art, search
  fallbacks, image previews, enhancement, upscale, dimensions, quality,
  conversion, provenance, and write confirmation.
- Artist/Album/COMP/Fallback workflows, search queues, track switching,
  pending-first processing, batch and single EMBED, WRITE, stop/cancel,
  retry, bypass, timeout, recovery, and completion persistence.
- Physical Artist/Album membership, Featured/Soundtrack/Various Artists
  associations, grouping, sorting, navigation, selection, status colors,
  counts, overview/biography, banners, frames, override indicators, and
  accessibility.
- Every exposed setting/default/range/dependency/validation/persistence
  rule, configuration import/export, JSON policy revisions, backups, and
  Windows Portable, Store, WebGUI, and Docker differences.
- SQLite and PostgreSQL catalog/field authority, identity and path mapping,
  change detection, indexing generations, batching, checkpoints,
  atomic publication, synchronization, coordination, migrations,
  schema-upgrade preparedness, resource limits, and cleanup.
- Startup, authentication, shutdown, background workers, concurrency,
  real-time progress, logs, diagnostics, error paths, deployment, updates,
  and integrations including Navidrome.

Keep a **companion evidence/compliance audit** separate from enforceable
AGENTS.md instructions. For every entry identify policy provenance
(approved/source-observed/unresolved), both implementations, JSON/schema
authority, tests, exceptions, and discrepancies. Provisional observations
must not become mandatory policy merely because they are documented.

**A historical 38-row WebGUI/Windows comparison is an assessment, not an
approved AGENTS.md register.** It may contain stale source observations,
incorrect parity classifications, implementation histories mislabeled as
policy differences, or unapproved prescriptions. Never import, enforce, or
count its rows as accepted policy without independently verifying each against
owner decisions, current scoped contracts, and current source. Unreviewed
subsystems and missing evidence are explicit gaps. Do not alter
other source files or run full regression suites for a documentation-only
policy inventory unless separately requested.

## Change validation

Before presenting a completed change:

1. compare the implementation with every affected policy entry;
2. run checks appropriate to the changed behavior;
3. report the policy entries reviewed, tests performed, results, and any
   unresolved difference; and
4. do not claim WebGUI and Windows parity unless the policy and observed tests
   both support that claim.

Keep work local for review unless the user explicitly authorizes committing,
pushing, opening or updating a pull request, or merging.

## Authoritative policy register

The user owns policy wording and approval. Source code, tests, releases,
database contents, commits, and historical pull requests provide implementation
evidence; they do not independently redefine policy.

- `WEBGui` and `Windows` describe the required behavior for each architecture.
- `Varied` accepts `current` when no policy difference exists or `changed` when
  a reviewed difference or change exists.
- `Change summary` records only the concise, relevant reason and review or pull
  request reference for a `changed` entry.
- `Comment/Change` is the editable working column for the user's corrections,
  decisions, and proposed policy revisions.
- Blank or ambiguous entries are not permission to infer behavior. Obtain the
  user's direction before implementing behavior not governed clearly here.

| WEBGui | Windows | Varied | Change summary | Comment/Change |
| --- | --- | --- | --- | --- |
| **ART-001** — One normalized-name Artist presentation per applicable owned grouping; MBID variation does not split display. | Same approved grouping requirement. |  |  |  |
| **ART-002** — Preserve distinct Album identities and credits; no duplicate cards or speculative MBID rewrites. | Same approved Album-association requirement. |  |  |  |
| **ART-003** — Resolve Artist Art from owned folders even when release MBIDs are absent. | Same approved physical discovery requirement. |  |  |  |
| **DB-001** — Preserve SQLite-owned synchronized catalog fields; PostgreSQL-only fields remain WebGUI-owned. | SQLite is the authority for synchronized catalog fields; explicit reconciliation only. |  |  |  |
| **DB-002** — COMP opposing writes coordinate per track; no Album-level override promotion. | Same per-track coordination and independent completion requirement. |  |  |  |
| **STATE-001** — Enforce effective validated JSON Artist-name/Album-state colors; Unprocessed count never forces White Artist names. | Same shared JSON presentation requirement. |  |  |  |
| **STATE-002** — Separate Artist aggregates, Album state, Art completeness, COMP progress, and writer provenance. | Same independent aggregation requirement. |  |  |  |
| **UI-001** — Report locally committed job/progress state immediately; never await override polling. | Same nonblocking Header/Embedding/Status Bar requirement. |  |  |  |
| **SAFE-001** — Report ambiguous identity mappings; never auto-merge or rewrite authoritative IDs. | Same identity/reconciliation safety requirement. |  |  |  |
| **PARITY-001** — Independently verify WebGUI against approved policy. | Independently verify Windows against approved policy. |  |  |  |

The requirements above are owner-approved **targets**, not verified
implementation status. Keep `Varied` blank until source verification and
owner review; use only `current` or `changed` with the meanings defined
above. Keep the `Comment/Change` column available for owner decisions. Add
all other approved rules during a source-backed inventory; keep unapproved
observations in the companion audit, not in this enforceable table.

