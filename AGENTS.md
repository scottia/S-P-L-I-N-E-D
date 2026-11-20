# SPLINED repository instructions — active development governance

These instructions are active for the current development tree. The owner has
**already approved** the scoped Artist/Album, visual-state, deployment-mode and
protected-JSON rules in `docs/POLICY-REGISTER.md`. Current source and installed
client behavior remain subject to independent implementation validation;
approval of the register is not proof that every implementation is correct.

## Authority and review

- The repository owner's explicit requirements establish intended behavior.
  Source code, tests, documentation, historical PRs, and running databases are
  evidence to audit, not independent behavioral-policy authority.
- No existing Windows implementation is automatically correct relative to
  WebGUI, and no existing WebGUI implementation is automatically correct
  relative to Windows. Differences must be checked independently against
  the owner-approved rule.
- Unverified guides, past comparison drafts, and older acceptance records
  cannot override `docs/POLICY-REGISTER.md`. Do not promote them to policy
  without explicit owner approval.
- `schema/` JSON, SQL contracts, and `*_policy.py` / `*_policy.rs`
  modules are runtime dependencies where referenced. Their execution does
  not establish their rules as owner-approved policy.
- Previously communicated owner decisions are not voided by this cleanup.
  Reconcile them explicitly before any behavioral correction.
- **Mode-independent classification:** Shared / Network SQLite is not a
  media-state policy choice. All clients use the same approved Artist/Album
  name rankings and status-dot contract from
  `docs/POLICY-REGISTER.md`; an embedded JSON seed or PostgreSQL/local
  mirror is a delivery mechanism, not permission for a different policy.
  `Unprocessed` belongs only to the MEDIA FILTER count, not name colors.

## Approved structural boundary

- Workflow 01 release source consists of `build/WEBGui/` and
  `build/Windows/` only. Treat the exact Workflow 01 checkout, build,
  Dockerfile, Cargo, migrations, and test dependencies as the source map.
- `schema/`, `release/` and `.github/` contain shared-policy and
  release-wide inputs outside the two application roots.
  WebGUI-only Docker deployment sources belong in `build/WEBGui/docker/`.
  Operator/developer Markdown guides belong in `docs/`, not `build/`.
- Do not put operator manuals, historical policy Markdown, or build
  architecture descriptions inside `build/`.
- Do not reintroduce unsupported root `src/`, `Cargo.toml`,
  `Cargo.lock`, `webgui/`, or `windows/` layouts into active
  `dev`. Use only the two Workflow 01 application roots.
- Application Help links must point to approved `/main` documentation
  only, never `/dev`. Help content is currently under review. Do not
  treat an existing `main` Help page as certified merely because it exists.
- Do not modify, merge into, force-push, or publish from `main` without
  the owner's explicit subsequent authorization.

## Documentation audience and accuracy

- Root `README.md` is the **user-facing navigation** for the current
  development source. Organize installation, capabilities, application
  choices, documentation and safety into clear sections with useful
  bulleted links. The `main` README is a **style reference only**;
  never import its obsolete paths, unsupported application targets or
  release claims into `dev`.
- `docs/README.md` indexes development guides.
  `docs/POLICY-REGISTER.md` records **owner-approved requirements**;
  `docs/POLICY-INDEX.md` maps policy/schema consumers;
  `docs/SCHEMA-VALIDATION.md` records implementation evidence and
  unresolved verification. These are different authorities and must
  not be presented as interchangeable.
- The reviewed source-based guides cover Windows and browser operation
  (`windows-guide.md`, `browser-workspace-guide.md`),
  storage and upgrades (`catalog-authority.md`,
  `splined-media-database.md`,
  `windows-schema-upgrade-preparedness.md`),
  settings/sources (`config-v5-reference.md`,
  `source-policies-range-types.md`, `credentials-providers.md`)
  and release/store (`release-automation.md`, `windows-store.md`).
  Read the relevant guide **and current code/config/schema** before
  updating an operational claim. PR #219's documentation review is
  not a blanket certification of deployed UI or synchronization.
- Validate relative links and source paths in every changed README,
  agent instruction and guide. Confirm that headings, Config v5
  controls, supported deployments, SQL/JSON schema roles and
  Windows-versus-WebGUI behavior match current `dev`. Keep old
  source-root or retired root-fixture references out of active docs.
- Do not promote a development guide as public in-app Help before
  separate `main` review, and never present a passing docs/build
  check as evidence of on-device Windows UI or live database parity.

## Catalog, configuration and schema ownership

- The Windows Rust/Slint application uses configured
  `<scan.cache_dir>/splined.db`. SQLite is authoritative for
  synchronized Artist/Album/Track identity, inventory and processing
  fields; WebGUI's PostgreSQL database owns its own jobs, sessions,
  audit, queues and explicitly WebGUI-owned columns. The detailed
  field allowlists are implemented in
  `build/WEBGui/webgui/catalog_authority.py`.
- **Standalone and Shared / Network SQLite use the same approved
  media-state and COMP policy.** Shared SQLite changes how one
  physical file is accessed, not the behavior contract. Optional
  PostgreSQL coordination is a distinct connection and writer
  evidence mechanism; it does not make PostgreSQL authoritative over
  synchronized Windows fields or create a second policy.
- Root `schema/` has **four different canonical contracts**:
  `media-state-classification.json` (protected name/dot policy),
  `comp-report-policy.json` (protected COMP baseline),
  `media.sql` (SQLite DDL) and `settings-ui.json` (Config v5
  presentation/fields). Do not duplicate these as independent client
  contracts or reinterpret SQL/default settings as owner policy.
- Config v5 TOML, runtime credential-directory files, and
  persisted PostgreSQL/local policy revisions are separate from
  checked-in behavioral JSON. Report incompatible persisted revisions
  for deliberate resolution; do not silently rewrite databases,
  media files, saved credentials or existing policy revisions.
- For an operational documentation change, identify whether a
  statement is **approved policy**, **verified current source**,
  **test evidence**, or **unverified deployed behavior**. Preserve
  these distinctions in both README and technical guides.

## Protected JSON policy PR gate

The independent PR check **Canonical policy JSON — review gate** protects
both `schema/media-state-classification.json` and
`schema/comp-report-policy.json`. Editing, adding, deleting, or moving
either file makes the PR check **fail** with a pointer to this file and to
`docs/POLICY-REGISTER.md` and `docs/POLICY-INDEX.md`.

On failure: **STOP**. Inspect the exact diff, the approved owner policy,
and its source/test consumers. Revert an accidental change; for an intended
change obtain an explicit owner decision and document it in the register.
An owner-authorized policy change needs an independently reviewed
authorization procedure before the guard can permit it. Do not edit the
guard in the same PR to bypass the review. The checker does not
automatically revert edits or modify either JSON.

The active policy guard runs independently in PR validation and is loaded
from the target branch's trusted base once installed there.
Configure GitHub rulesets to require **Canonical policy JSON — review gate**
on `dev` and, when approved for promotion, `main`. A failing check alone
does not guarantee merge prevention without the required-check rule.

## Mandatory Codex Pull Request workflow

- For every repository change, Codex must work on a **short-lived topic
  branch** from the approved base and open or update a **Draft Pull Request**.
  The default target is `dev`; changes scoped to `dev-macos` use that
  approved base. Work for `main` requires the owner's explicit direction.
- Codex must **not push directly to `main`, `dev`, or `dev-macos`**,
  merge a PR, publish a release, or bypass a required check by default.
- The PR must state its target, changed files, purpose, applicable
  `docs/POLICY-REGISTER.md` entries, policy/schema impact, executed and
  unexecuted validation, unresolved risks, and requested owner action.
  When a matching PR already exists, update it rather than open a duplicate.
- If branch creation or PR submission is unavailable, stop and report the
  blocker; direct pushes are **not** an automatic fallback. Temporary PR
  branches are removed only after the owner approves the PR disposition.

### Owner-authorized exception — current PR only

The repository owner **may override a specific instruction above**, but
Codex must **request the exception first**. Its request must identify:

1. The **current PR number/URL** and base/head branches;
2. The exact restricted action or procedure needing an exception;
3. Why normal PR review cannot be used, the affected files or checks,
   expected effect, risk, and proposed validation;
4. The precise action it requests permission to perform.

Only the owner's **explicit, affirmative approval** for that **identified
PR and specified action** grants that exception. Record the approval in
the PR discussion (or, if initially given in chat, quote or summarize it
accurately in the PR before acting). Silence, an earlier approval, a
merged PR, or the mere existence of this clause are **not** permission.

The exception **expires when that PR is merged, closed, or cancelled**;
it cannot transfer to another PR, scope, branch, or future change.
Codex must not expand the approved action or silently interpret it as
permission to skip unrelated checks, modify policy JSON, or publish.

**GitHub remains an independent enforcement boundary.** Owner approval
does not make a failed policy check pass, disable branch protection, or
authorize Codex to forge a status. If GitHub itself prevents an approved
exception, stop and explain what the owner must separately authorize or
perform through GitHub's supported controls. In particular, the two
protected behavioral JSON files retain their mandatory owner-review
procedure and failing PR gate until an explicitly reviewed authorization
mechanism is installed.

## Active test and build inputs

A source test or sample belongs in the current repository only where
it supports a verified application build, Workflow 01 release,
Store recovery, or independent PR policy/layout safeguard. Review
actual consumers before removing it. Do not restore retired
root-level example dependencies simply to make a stale test pass.
Native Rust library tests are an opt-in extended Workflow 01
validation; the Windows PR advisory is not a substitute for a
native Windows compile. The WebGUI Python suite, JavaScript checks
and Docker smoke run in the selected WebGUI release lane.

### CI and workflow evidence boundaries

- `.github/workflows/validate.yml` runs PR scope/layout tests, the
  independent canonical JSON policy gate and selected WebGUI checks.
  Its Windows Rust/Slint job is **advisory only**; a green advisory
  is not native Windows compilation or Slint UI acceptance.
- `.github/workflows/01-full-release.yml` owns release builds.
  The WebGUI lane runs Python, JavaScript and container/PostgreSQL
  validation. Native Windows packaging is a distinct Windows lane;
  its extended Cargo fmt/Clippy/library suite runs **only when
  explicitly selected**, including for Store modes.
- `02-store-recovery.yml` and `03-ghcr-recovery.yml` are for authorized
  tagged Store and GHCR recovery, not ordinary docs or PR checks.
  Do not dispatch or relax them to validate a documentation edit.
- For documentation-only work, validate links, paths, schema
  references, formatting and PR checks. Record any runtime,
  database migration, browser or native tests **not run** instead
  of asserting full application parity.

## Mandatory Codex preflight

1. Read this file, `README.md`, `docs/README.md`, `docs/build-layout.md`,
   `docs/POLICY-REGISTER.md`, `docs/POLICY-INDEX.md` and
   `docs/SCHEMA-VALIDATION.md`; inspect the current branch and worktree.
   Preserve unpublished local edits.
2. Confirm the exact supported build input, import, runtime asset, migration,
   packaging, and regression-test reference before proposing source deletion
   or moving any file.
3. For behavior changes, map explicit owner instructions first, then audit
   the current code, effective runtime JSON and tests for both clients
   separately. List unknown or contradictory policy without choosing a
   new rule on the owner's behalf.
4. Only implement the explicitly authorized scope; do not silently modify
   media files, SQLite/PostgreSQL content, artist identities, MBIDs, history,
   schemas or release workflows as a consequence of a documentation audit.
5. For technical and documentation changes, execute the relevant, available
   validation gates; check documentation links and references when applicable.
   Report both actual runs and checks that were **not** run. A passing
   test does not itself approve its assertions.
6. Record changed files, source SHA, independent test evidence, unresolved
   issues, and whether the `main` branch remained untouched. No merge or
   release unless expressly requested.

Never expose credentials, API keys, passwords, tokens, or machine-specific
private settings in repository documentation, commits or reports.

## Policy register status

**Owner-approved requirements already established:** Artist and Album
file-list NAME classifications and rankings, the all-applicable Album
status-dot order, screenshot-approved frames/banners/COMP-track colors,
the MEDIA FILTER-only Unprocessed count, and a single canonical policy
across standalone/shared/coordination deployments. These decisions
are authoritative in `docs/POLICY-REGISTER.md`; Codex must not request
their approval again merely because runtime parity remains unverified.

`docs/POLICY-INDEX.md` links all four shared `schema/` contracts.
The COMP policy is an accepted working baseline (do not speculate or
rewrite its matching rules), while SQLite DDL and Settings UI received
structural inspection. Outstanding runtime validation checks are
**implementation validation**, not pending approval of the owner's
established color policy. See `docs/SCHEMA-VALIDATION.md`.

No unverified implementation, legacy guide, or test assertion may silently
supersede the approved register. The mandatory Codex Draft PR process,
including owner-authorized exceptions limited to the current PR/action,
is active on `dev` following merged PR #217.
