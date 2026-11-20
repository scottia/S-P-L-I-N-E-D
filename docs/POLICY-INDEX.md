# S:P:L:I:N:E:D — policy/source index

**Checkpoint:** November 19, 2026 · development base
`9be357951f0ccb4f2a624a7baf9307699d700e8a`.
The owner's color and visual rules are **APPROVED** and have already
been discussed and corrected in `dev` source. Deployment/runtime
acceptance is **not** implied by that approval.

## Status vocabulary

- **APPROVED REQUIREMENT:** owner has specified the behavior; **no
  additional approval is pending to document or enforce it**.
- **SOURCE CORRECTED:** a matching source correction is present on
  `dev`; installed-client behavior may still require testing.
- **STRUCTURALLY INSPECTED:** format, fields and source dependencies
  were examined; not a database or application acceptance test.
- **ACCEPTED WORKING BASELINE:** preserve functioning implementation
  unless specific evidence justifies correction; detailed equivalence
  remains to be demonstrated.
- **VALIDATION OPEN:** runtime, UI, data/migration, or saved revision
  evidence has not yet been demonstrated. **Not a policy approval request.**

## Four shared `/schema` contracts

| File | Role | Decision / validation |
| --- | --- | --- |
| [`media-state-classification.json`](../schema/media-state-classification.json) | **Protected, owner-approved Artist/Album color and dot policy implementation** | **APPROVED REQUIREMENT + SOURCE CORRECTED.** JSON ranking/red-yellow labels and Python/Rust guards updated. CSS and Slint neutral-name exceptions removed. Independent displayed evidence and installed revisions remain VALIDATION OPEN. |
| [`comp-report-policy.json`](../schema/comp-report-policy.json) | **Protected COMP Artist/Recording matching, MusicBrainz ranking, grouping, revision pinning and coordination** | **ACCEPTED WORKING BASELINE + STRUCTURALLY INSPECTED.** No speculative behavior rewrite; independent parity and saved policy revisions remain VALIDATION OPEN. |
| [`media.sql`](../schema/media.sql) | SQLite database DDL / catalog coordinator, **not** a media-name policy | **STRUCTURALLY INSPECTED:** 22 table and 24 index declarations identified. Actual execution, migration preservation and owner-field compatibility remain VALIDATION OPEN. |
| [`settings-ui.json`](../schema/settings-ui.json) | Config v5 and shared Windows/WebGUI settings UI metadata, **not** artwork state authority | **STRUCTURALLY INSPECTED:** parses, 8 sections, 65 unique field paths and no inverted numeric bounds found. Actual UI/default/range/persistence parity remains VALIDATION OPEN. |

All four remain single-sourced in `/schema`. Do not move or copy
them into independent `/build` variants. The Workflow 01/GHCR and
Windows dependencies are described by [build-layout.md](build-layout.md).

## Cross-runtime Album test fixture (not a behavioral-policy JSON)

[`fixtures/cross-runtime-album-policy.json`](../fixtures/cross-runtime-album-policy.json)
is intentionally retained at the **repository root** because it supplies
test cases to both
[`build/Windows/src/compilation.rs`](../build/Windows/src/compilation.rs)
and
[`build/WEBGui/webgui/tests/test_cross_runtime_album_policy.py`](../build/WEBGui/webgui/tests/test_cross_runtime_album_policy.py).
It exercises Album output-target and MusicBrainz release-classification
expectations. It is neither a separately editable production policy nor a
substitute for the two protected canonical JSONs in `schema/`.
Fixture values are regression evidence and may not override explicit
owner-approved behavioral requirements.

## Approved requirements indexed

| Group | IDs | Implementation / evidence |
| --- | --- | --- |
| Artist NAME priority and forbidden colors | [`ART-COLOR-001..006`](POLICY-REGISTER.md#approved-artist-file-list-name-classification) | JSON `artist.ranked_states`; Windows `media_state_policy.rs`; WebGUI `media_state_policy.py`, `catalog.py`, `app.js`, `app.css` |
| Album NAME ranking and all applicable dots | [`ALB-COLOR-001..006`](POLICY-REGISTER.md#approved-album-file-list-name-classification-and-dots) | JSON `album.ranked_name_states` / `status_dots`; Rust/Python validators, WebGUI catalog/card/list, Slint UI |
| Approved existing frames, banners, COMP track rows and animated WebP indicator | [`VISUAL-001..004`](POLICY-REGISTER.md#approved-visuals-that-must-not-change-during-name-color-corrections) | Approved owner screenshot; application status-key/card/track presentation. **Preserve independently of name correction** |
| Same policy in all SQLite/PostgreSQL modes and Unprocessed count separation | [`STATE-POLICY-001..004`](POLICY-REGISTER.md#one-canonical-policy-across-every-installation) | Windows compile-time JSON seed; local credential file if present; WebGUI PostgreSQL revision; MEDIA FILTER count |
| Protected JSON PR review flow | [`GUARD-001..003`](POLICY-REGISTER.md#protected-behavioral-policy-json) | `.github/scripts/check_protected_policies.py` and independent `policy-review` job in `.github/workflows/validate.yml` |

## Live code and test trace

| Scope | Current `dev` reference |
| --- | --- |
| Windows Rust policy and validation | [`build/Windows/src/media_state_policy.rs`](../build/Windows/src/media_state_policy.rs) |
| Windows file-list name rendering | [`build/Windows/ui/app.slint`](../build/Windows/ui/app.slint) |
| WebGUI Python policy and DB revision | [`build/WEBGui/webgui/media_state_policy.py`](../build/WEBGui/webgui/media_state_policy.py) |
| WebGUI catalog/Artist aggregation | [`build/WEBGui/webgui/catalog.py`](../build/WEBGui/webgui/catalog.py) |
| WebGUI presentation and CSS | [`build/WEBGui/webgui/static/app.js`](../build/WEBGui/webgui/static/app.js), [`app.css`](../build/WEBGui/webgui/static/app.css) |
| WebGUI media-state regression | [`build/WEBGui/webgui/tests/test_media_state_classification_policy.py`](../build/WEBGui/webgui/tests/test_media_state_classification_policy.py) |
| COMP runtime consumers | [`build/WEBGui/webgui/comp_report_policy.py`](../build/WEBGui/webgui/comp_report_policy.py), [`build/Windows/src/comp_report_policy.rs`](../build/Windows/src/comp_report_policy.rs) |
| PR policy gate and rendering contract checks | [`.github/scripts/check_protected_policies.py`](../.github/scripts/check_protected_policies.py), [`test_media_state_rendering.py`](../.github/scripts/test_media_state_rendering.py) |

## Required pull-request review boundary

Both protected policy JSONs are checked in the separate **Canonical
policy JSON — review gate**. This gate has been introduced on `dev`
and exercised successfully on the docs-only PR #217. A proposal
changing either protected JSON fails with instructions referencing
`AGENTS.md` and this register. It **does not auto-revert or
authorize** the change.

An authorized JSON update still requires an explicit, independent
owner-reviewed process; the gate currently fails a change rather than
accepting approval simply because a reviewer says "yes." Required
branch-ruleset protection remains unverified because the available
GitHub integration cannot inspect that setting.

Mandatory Codex Draft PRs and current-PR-only owner exceptions are
**proposed in PR #217**, not yet merged. PR #216 is the divergent
`dev` → `main` release-layout/policy draft and must remain on hold.

## Next engineering validation (not policy re-approval)

1. Verify independent Artist/Album name classification on both
   Windows and WebGUI, in light/dark themes and default/unprocessed
   cases; fix any residual catalog-key/CSS/Slint evidence routing.
2. Verify MEDIA FILTER Unprocessed **count** separately from the
   ranked name states, and protect the screenshot-approved dots,
   frames, banners and COMP rows.
3. Validate saved PostgreSQL policy revision and Windows local
   policy copies without silently changing live state.
4. Validate the current working COMP behavior with focused real
   fixtures, then SQLite DDL/migrations and Settings v5 parity.
5. Rebuild public application Help only from verified,
   owner-approved behavior, targeting `main` upon authorized
   promotion—not development-only docs.

Detailed evidence and open issues: [SCHEMA-VALIDATION.md](SCHEMA-VALIDATION.md).
