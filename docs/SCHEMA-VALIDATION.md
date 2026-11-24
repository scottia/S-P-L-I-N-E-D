# S:P:L:I:N:E:D — current /main source validation ledger

**Basis:** current `main` after the approved build reconstruction and PR #233–#238 policy/code corrections, reviewed November 20, 2026. This ledger distinguishes **owner-approved policy**, **source/test evidence**, and **deployed validation still open**. It does not authorize database repair, a policy rewrite, or release publication.

The owner-approved requirements live exclusively in [POLICY-REGISTER.md](POLICY-REGISTER.md); cross-runtime source ownership is mapped by [POLICY-INDEX.md](POLICY-INDEX.md). A completed source PR does not automatically certify installed-client parity.

## Two supported indexers and catalog authority

| Responsibility | Current source | Validation boundary |
| --- | --- | --- |
| Windows catalog and generations | `build/Windows/src/media_database.rs`, `catalog_index_coordinator.rs`, `index_batch_policy.rs`, `schema/media.sql` | SQLite-owned synchronized identities/state; test v2→v3 preservation and interruption recovery in disposable SQLite, plus a real native Windows build |
| WebGUI PostgreSQL index/generations | `build/WEBGui/webgui/services.py`, `catalog.py`, `index_coordinator.py`, `index_batch_policy.py`, `migrations/021_catalog_index_coordinator.sql` | PostgreSQL catalog/jobs/audit and WebGUI-only embedded validation; test refresh publication and authority with disposable PostgreSQL |
| Cross-client field ownership | `build/WEBGui/webgui/catalog_authority.py` and Windows SQLite | Windows-owned Artist/Album/Track fields cannot be silently replaced by WebGUI Full Refresh; coordination events are not catalog ownership |
| Config/settings | `schema/settings-ui.json` plus `build/Windows/config.example.toml` and `build/WEBGui/docker/config.example.toml` | Field/default/range comparisons require real-client confirmation |

## Media-state and COMP classification

| Approved requirement | Current implementation evidence | Remaining acceptance boundary |
| --- | --- | --- |
| Artist NAME rank: Red > Yellow > Purple > Green | Windows Rust and WebGUI Python policy loaders reject reordered conditions; WebGUI Artist aggregation now includes indexed static Album covers, credit appearances, COMP evidence and explicit error provenance | Check real Artist lists in both themes and runtime modes, without startup-wide filesystem scans |
| Album NAME rank and all seven independent dots | Both validators enforce Dark Grey > Red > Yellow > Purple > Orange > Green; auxiliary Blue is last; Slint/CSS neutral-name overrides were removed | Confirm actual native and browser card/list views and preserve existing banner/frame/COMP row colors |
| Static art versus `cover.webp` | WebGUI separates static-cover evidence from animated previews; missing qualifying still art contributes Yellow | Verify fallback/COMP and real media variants |
| Unprocessed MEDIA FILTER | WebGUI count now comes from raw Album inventory rather than ranked NAME state | Verify shown/count behavior independently of processing/write semantics |
| Preferred embedded thumbnail | Physical WebGUI readback checks exact square side; summary flags missing, undersized, oversized and nonsquare artwork with `outside_preference` | Verify in installed clients and protect physical `retry` on unreadable tags |
| COMP rules | Protected `schema/comp-report-policy.json`; Rust and Python policy consumers | Verify actual COMP batches/track-level behavior and saved revisions without rewriting live state |
| One normalized Artist presentation | Windows already projects by normalized display identity; WebGUI groups same-name PostgreSQL rows independently of MBID/path while preserving their Album keys | Exercise distinct MBIDs, distinct folders, duplicate names, unique Album cards and noncanonical saved Artist Art in disposable catalogs |

**Protected JSON labels versus approved meaning:** The current canonical seed still uses the narrower display text `Missing Artist Art` for Artist Yellow and `Incomplete album tracks` for Album Red. The approved meaning is missing Artist **or static Album** art and independent **Incomplete/Error**, respectively. Rank/color validation is correct; a label-only JSON revision would still require an independently authorized protected-policy PR, not a docs-only edit.

## Check evidence and limitations

The validated application code was introduced through the owner-reviewed reconstruction/correction series (including PR #233/#234 for name rendering, #235 for WebP/Unprocessed evidence, #236 for Artist aggregation/counts, #237 for embedded-size reporting, and #238 for stored rank/dot validation). Their final-head GitHub Actions checks were reported successful. They **do not** prove a new native Windows Rust/Slint compilation or installed UI acceptance; the ordinary Windows PR job is advisory.

WebGUI PR tests exercise Python, browser JavaScript, Docker image startup and disposable PostgreSQL. A nonmutating pre-release check additionally uses both canonical JSON validators, Config v5 metadata and SQLite DDL against an **in-memory database**; these are not live indexer migration tests.

**Operator read-only PostgreSQL evidence (November 20, 2026):** The owner reported one singleton media-state row, revision **3**, updated by `webgui-settings` on November 19, and one COMP report row, revision **1**, updated by `webgui-seed` on November 18. The returned Artist/Album condition/color priorities and seven Album dots match the checked-in canonical order, including auxiliary Blue. The response did **not** include complete palettes, ancillary policy settings or the full COMP JSON; do not call this full stored-policy equality. The effective Windows credential-directory mirrors have not been inspected.

Full production music-library tests, live network shares, installed policy mirrors, source/DB revision drift and real catalog recovery require separate operator-authorized validation. **No live policy or database was changed by this documentation audit.**

## Governance and remaining checks

- The single checked-in contracts are `schema/media-state-classification.json`, `schema/comp-report-policy.json`, `schema/media.sql`, `schema/settings-ui.json`; no runtime-specific duplicate policies.
- `AGENTS.md` and [POLICY-REGISTER.md](POLICY-REGISTER.md) require independent protection of both behavioral JSONs. This PR adds the separate **Canonical policy JSON — review gate** to `validate.yml` and `.github/scripts/check_protected_policies.py` (fail closed on any protected-file diff), with canonical-contract validation. Workflow 01 checks source contracts and shared regression scripts before version stamping; submitting a Store update requires Extended Windows validation. These implement existing requirements, **not** new authority, and GitHub ruleset enforcement remains a separately verified setting.
- Source revisions and PostgreSQL/Windows *saved* policy revisions are different objects. A safe installed comparison reads policies without invoking seeding/updating loader functions. Do not silently replace a saved policy.
- Neither the retired root-native Linux instructions nor old PR acceptance reports belong in active `docs/`. Current supported source roots remain `build/Windows/` and `build/WEBGui/`, with release automation in `release/`.
- Do not request re-approval of settled Artist/Album colors. Record implementation drift, missing runtime evidence and owner decisions in the appropriate future PR.

## November 23, 2026 — owner-approved lifecycle and release-governance update

This ledger's November 20 references to Workflow 01 describe the *historical*
validation setup and do not designate today's active release path. The owner
approved `WEB-LIFE-001..009` in the normative [Policy Register](POLICY-REGISTER.md)
to govern WebGUI selection, queue/nonce, status, background-resource, search/COMP,
presentation and validation acceptance. See [Policy Index](POLICY-INDEX.md)
for source mapping and [AGENTS.md](../AGENTS.md) for the agent compliance boundary.

**Current process:** required PR checks in `.github/workflows/validate.yml`
retain applicable fail-closed writer tests. Workflow 10
(`.github/workflows/10-unified-build.yml`) is the owner-designated unified
build/release path for the exact already-approved commit, without rerunning
either PR writer suite solely for packaging. Workflow 01 is still technically
manually dispatchable and has not been disabled in this documentation PR.

**Validation status:** documentation/source review only. This policy update
changes no runtime, workflow file, protected schema JSON, migration, live
database, media, CI enforcement rules or installed-client evidence. Existing
deployed validation limitations above remain open; new policy acceptance gates
must be checked against product implementation separately.

## November 23, 2026 — proposed cross-client lifecycle parity contract

The owner requested a single new declarative lifecycle JSON under
`schema/lifecycle-policy.json` for **equivalent Windows and WebGUI behaviors**.
The [Policy Register](POLICY-REGISTER.md) enumerates the proposed common
`LIFE-001..009` requirements and retains the previously approved
`WEB-LIFE-001..009` browser/Python refinements. The
[Policy Index](POLICY-INDEX.md) and [Lifecycle mapping](lifecycle-policy.md)
identify each client source surface, without claiming identical implementation.

**Verified at this checkpoint:** the checked-in JSON has nine unique shared
requirements; the documentation identifies both supported runtimes and
existing Windows SQLite ownership / WebGUI PostgreSQL-local boundaries.
Read-only source inspection confirms both clients have distinct indexers,
queues, UI frameworks and catalog stores. No actual runtime consumer or
cross-database transport is introduced by this proposed specification.

**Required activation before declaring the lifecycle policy enforced:**
Windows and WebGUI must load and validate the same canonical JSON via their
native runtimes, with focused behavior-to-rule mapping, and the independent
policy-diff gate must protect the new JSON once it is in the accepted base.
No existing media-state/COMP gate weakening, implicit database write, catalog
migration, or startup network dependency is permitted. The owner requires
these prerequisites before downstream parity acceptance.

**Still not verified:** no production or credential-directory lifecycle
revision is seeded, and the shared semantic requirements have not received
installed-client parity acceptance. The source consumers and independent gate
described below do not create a migration or transport. Any future database,
mirror or installed-runtime change requires separate owner review; existing PR
validation and artwork-only media-writer safeguards remain in force.

## November 23, 2026 — shared lifecycle source activation evidence

The independently reviewed activation stage adds native local consumers and
extends protection after PR #318 established the canonical base:

- Windows embeds and validates the exact checked-in lifecycle JSON once before
  database/provider/catalog startup. Focused Rust tests reject invalid identity,
  runtime scope, rule order and WebGUI refinement mapping.
- WebGUI reads and validates the checked-in/container JSON once before Context,
  PostgreSQL, workers or providers. Focused Python tests prove fail-closed
  ordering and the same identity/scope/rule/refinement requirements.
- The independent canonical-policy diff gate now covers lifecycle policy as
  well as media-state and COMP. Focused negatives reject add, change, delete
  and rename while retaining `--no-renames` fail-closed behavior.
- Release-contract validation invokes the real Python lifecycle validator.
  Docker packaging includes the same checked-in JSON.

This is source activation, not a database revision or installed parity claim.
No SQLite/PostgreSQL schema, policy singleton, credentials, catalog row, queue,
media or production service is changed. Current behavioral evidence and gaps
are listed in [lifecycle-policy.md](lifecycle-policy.md); PR #307 retains the
Windows loading/revalidation authority review, and PR #314 retains WebGUI-only
lifecycle defects and corrections.
