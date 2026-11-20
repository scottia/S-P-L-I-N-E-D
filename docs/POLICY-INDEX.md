# SPLINED Policy Index — source/authority map

**Status:** Initial index after schema-level review, November 19, 2026.
This is a map of artifacts and review status—not a blanket certification.

**Document flow:** [Owner policy register](POLICY-REGISTER.md) →
source/contract index (this file) → executable JSON/SQL defaults →
Windows and WebGUI implementation → regression evidence.

The register defines *approved intent*. The index points to the actual file
that implements or describes it, even while that implementation is incorrect.
Do not convert unapproved source observations into register entries.

## The four shared schema files

| Artifact | Type and purpose | Runtime readers and validation | Status |
| --- | --- | --- | --- |
| [`schema/media-state-classification.json`](../schema/media-state-classification.json) | **Behavioral policy seed** for Artist/Album name ranking, Album status dots, palette, override presentation and database monitor | `build/WEBGui/webgui/media_state_policy.py`, `build/Windows/src/media_state_policy.rs`, browser UI and tests | **Owner policy defined; focused /dev correction prepared** (red/yellow wording, ranking/dot validators, CSS unprocessed-name bypass, Album status inference); full client parity and live PostgreSQL/Windows revisions unverified |
| [`schema/comp-report-policy.json`](../schema/comp-report-policy.json) | **Behavioral policy seed** for COMP Artist/Recording matching, ranking, batch outcomes and coordination | `build/WEBGui/webgui/comp_report_policy.py`, `build/Windows/src/comp_report_policy.rs`, respective tests | JSON structure and required fields reviewed; **behavioral approval pending**; working runtime not independently tested |
| [`schema/media.sql`](../schema/media.sql) | **SQLite database DDL**; not a color policy | Windows index coordinator and native catalog initialization; WebGUI compatibility/SQLite import path | 22 table and 24 index declarations identified; **SQL execution, migrations and semantic compatibility unverified** |
| [`schema/settings-ui.json`](../schema/settings-ui.json) | **Config/UI presentation contract**, not the authority for artwork processing outcomes | `build/WEBGui/webgui/server.py`, settings UI, Windows/UI compatibility checks | Parses as JSON; 8 sections, 65 field paths, no duplicate paths or inverted numeric bounds found; **runtime/default parity unverified** |

These files remain together under `schema/` for now because **both
applications have explicit runtime and compile-time dependencies on those
paths**. Grouping them in this register/index does not require moving them.
A later decision to create `policy/` would require a coordinated,
validated update to Rust `include_str!`, Python path resolution, GHCR
Dockerfile, release workflows, tests, and PostgreSQL seed/mirror handling.

## Current approved policy entries

- [Artist names and state ranking — `ART-COLOR-001..006`](POLICY-REGISTER.md#media-filter-artist-name-policy)
- [Album names and status dots — `ALB-COLOR-001..006`](POLICY-REGISTER.md#album-name-and-status-dot-policy)
- [Deployment-independent policy — `STATE-POLICY-001..004`](POLICY-REGISTER.md#deployment-independent-policy-authority)

## Implementation evidence (not approval)

- [Media-state audit and outstanding discrepancies](SCHEMA-VALIDATION.md)
- Active WebGUI policy: PostgreSQL revisioned row and in-memory state.
- Coordinated Windows: local validated JSON mirror and asynchronous
  PostgreSQL-revision checks.
- **Standalone Windows:** embeds the exact same checked-in
  `schema/media-state-classification.json` as its offline policy source;
  this is not a second or separately approved default.
- **Shared / Network SQLite:** changes the SQLite storage/coordination
  mode, not name classification, rank order, or Album-dot rules.
- Local credential-directory copies and PostgreSQL revisions remain
  implementation/runtime state and must not fork the owner-approved
  rules. Drift requires explicit validation and reporting.
- Both clients must be checked against the **effective** runtime policy.
- Existing historical sources are retained only in
  [`dev-archive`](https://github.com/scottia/S-P-L-I-N-E-D/tree/dev-archive).

## Remaining review order

1. Validate the focused media-state correction under Windows and WebGUI,
   including no-white names, red/yellow evidence, all applicable dots,
   theme/CSS behavior, and effective runtime revisions. Resolve remaining
   evidence and presentation gaps before promotion.
2. Independently validate COMP report policy and its effective revisions;
   preserve presently functioning behavior until a concrete defect is proven.
3. Validate SQLite `media.sql` by applying the DDL in a disposable database,
   comparing runtime migrations/ownership and checking catalog fixtures.
4. Validate `settings-ui.json` against real Windows/WebGUI controls, saved
   Config v5 defaults, ranges, platform scopes, and persistence.
5. Expand the register only with further **owner-approved** decisions and
   rewrite the operating guides from that validated policy.

Do not merge to `main`, dispatch Workflow 01, or alter live PostgreSQL or
SQLite as part of this index.
