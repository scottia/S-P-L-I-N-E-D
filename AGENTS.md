# SPLINED repository instructions — structure-first checkpoint

These instructions apply to the repository until the owner approves a new,
source-verified policy register. This is a **governance and restructuring
checkpoint**, not a declaration that the present application code is correct.

## Authority and review

- The repository owner's explicit requirements establish intended behavior.
  Source code, tests, documentation, historical PRs, and running databases are
  evidence to audit, not independent behavioral-policy authority.
- No existing Windows implementation is automatically correct relative to
  WebGUI, and no existing WebGUI implementation is automatically correct
  relative to Windows. Differences must be checked independently against
  the owner-approved rule.
- Retired Markdown policies, the historical 38-row comparison, and past
  acceptance records are quarantined in `dev-archive`; do not import them
  as mandatory requirements.
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
- `schema/`, `docker/`, `release/` and `.github/` contain shared or
  release-support inputs outside the two application build folders.
- Do not put operator manuals, historical policy Markdown, or build
  architecture descriptions inside `build/`.
- Preserve `dev-archive` as the read-only historical reference. Do not
  reintroduce old root `src/`, `Cargo.toml`, `Cargo.lock`, `webgui/`
  or `windows/` layouts into active `dev`.
- Application Help links must point to approved `/main` documentation
  only, never `/dev`. Help content is currently under review. Do not
  treat an existing `main` Help page as certified merely because it exists.
- Do not modify, merge into, force-push, or publish from `main` without
  the owner's explicit subsequent authorization.

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

This guard is distinct from the archived/inactive boundary checker.
It is loaded from the target branch's trusted base once installed there.
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

## Mandatory Codex preflight

1. Read this file, `docs/build-layout.md`, `docs/POLICY-REGISTER.md`,
   `docs/POLICY-INDEX.md` and `docs/SCHEMA-VALIDATION.md`; inspect the
   current branch and worktree. Preserve unpublished local edits.
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
5. For technical changes, execute the relevant, available validation gates
   and report both actual runs and checks that were **not** run. A passing
   test does not itself approve its assertions.
6. Record changed files, source SHA, independent test evidence, unresolved
   issues, and whether the `main` branch remained untouched. No merge or
   release unless expressly requested.

Never expose credentials, API keys, passwords, tokens, or machine-specific
private settings in repository documentation, commits or reports.

## Policy register status

**Partial: owner-approved Artist/Album color classification only.**
`docs/POLICY-REGISTER.md` governs that scoped classification. The
`docs/POLICY-INDEX.md` maps four runtime schema artifacts; a checked-in
JSON or code implementation may still diverge. No application-wide
permanent register or precedence is certified for other behavior. The
historical 38-row comparison must not be imported as policy.
