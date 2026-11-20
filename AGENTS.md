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
