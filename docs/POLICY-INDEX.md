# S:P:L:I:N:E:D — policy and indexer source map

**Basis:** `main` as audited November 20, 2026 after the approved Windows/WebGUI reconstruction and media-state corrections through PR #238. This index maps existing **approved requirements to current source**; it does not invent new rules or create another 38-row draft. [POLICY-REGISTER.md](POLICY-REGISTER.md) remains the owner-authoritative behavioral register.

## Two application roots and indexers

| Scope | Windows — `build/Windows/` | WebGUI — `build/WEBGui/` |
| --- | --- | --- |
| Persistent catalog | `src/media_database.rs`; SQLite `splined.db` | `webgui/catalog.py`, `webgui/services.py`; separate PostgreSQL catalog |
| Index generations and batches | `src/catalog_index_coordinator.rs`, `src/index_batch_policy.rs` | `webgui/index_coordinator.py`, `webgui/index_batch_policy.py`, `webgui/migrations/021_catalog_index_coordinator.sql` |
| Refresh/selection | `src/scan_controller.rs`, `src/scan.rs` | `webgui/services.py`, `webgui/catalog.py` |
| Synchronized field ownership | SQLite catalog is authoritative for designated Artist/Album/Track fields | `webgui/catalog_authority.py`; PostgreSQL-only jobs, audit, validation and coordination remain local |
| Artist/Album NAME and status dots | `src/media_state_policy.rs`, `ui/app.slint` | `webgui/media_state_policy.py`, `webgui/catalog.py`, `webgui/static/app.js`, `app.css` |
| COMP contract | `src/comp_report_policy.rs` | `webgui/comp_report_policy.py` |

[Catalog authority](catalog-authority.md) and [Windows media database](splined-media-database.md) describe write ownership and indexer persistence; [WebGUI guide](browser-workspace-guide.md) covers the browser's index/queue operation. Neither client's live database is a substitute policy definition.

## Four canonical `schema/` contracts

| Source | Owner and usage | Review status |
| --- | --- | --- |
| [`media-state-classification.json`](../schema/media-state-classification.json) | Owner-approved Artist/Album NAME colors, all Album dots and serialized presentation settings. Embedded by Windows for offline defaults, loaded/revisioned by WebGUI PostgreSQL and optionally mirrored in Windows credentials. | Exact priority validation added in both languages; deployed revision/palette comparison remains a separate read-only task. |
| [`comp-report-policy.json`](../schema/comp-report-policy.json) | Shared Artist/Recording identity, MusicBrainz ranking, source/COMP options and batch revision pinning. | Protected accepted working contract; test runtime parity independently. |
| [`media.sql`](../schema/media.sql) | Windows/compatible SQLite catalog DDL; not an Artist/Album color policy. | Validate schema and migrations with disposable databases before any change. |
| [`settings-ui.json`](../schema/settings-ui.json) | Config v5 settings/UI field descriptions; no independent database or artwork authority. | Check defaults/ranges against both active implementations. |

The two behavioral JSONs are **protected by approved repository policy**. Their governing Artist ranking is **Red > Yellow > Purple > Green**, Album NAME ranking **Dark Grey > Red > Yellow > Purple > Orange > Green**, and Album-dot ordering **Dark Grey > Red > Yellow > Purple > Orange > Green > auxiliary Blue**, with every applicable dot retained. See [POLICY-REGISTER.md](POLICY-REGISTER.md) for all conditions, exclusions, allowed defaults and separate COMP row semantics.

## Requirements mapped to live code and regression tests

| Approved group | Source/tests |
| --- | --- |
| `ART-COLOR-001..006`, `ALB-COLOR-001..006` | Windows `src/media_state_policy.rs`, `ui/app.slint`; WebGUI `media_state_policy.py`, `catalog.py`, `static/app.js` and `app.css`; `webgui/tests/test_media_state_classification_policy.py`, `.github/scripts/test_media_state_rendering.py` |
| `VISUAL-001..004` | Existing status dots, card frames/banners, override provenance and COMP-track presentation; do not infer those from NAME classification |
| `STATE-POLICY-001..004` | Same canonical ordering in both runtimes; Windows policy loader/mirror, WebGUI PostgreSQL policy revision, inventory-derived Unprocessed count |
| `GUARD-001..003` | Owner-approved review contract in `AGENTS.md` and the register; `.github/workflows/validate.yml` runs the independent `policy-review` job and `.github/scripts/check_protected_policies.py` rejects any diff to the two protected JSON files. Workflow 01 validates canonical contracts before creating release metadata. Required GitHub ruleset enforcement remains a separate repository-setting check. |

The test fixture that once lived under root `fixtures/` was retired; it was never a production policy. Independent Windows and WebGUI compilation tests now own the applicable release/category cases. Release scripts and synthetic Store test fixtures belong in root `release/`.

## CI and deployment distinctions

`.github/workflows/validate.yml` executes scope checks, WebGUI Python/JavaScript/Docker/PostgreSQL checks when applicable, the independent **Canonical policy JSON — review gate** and a **Windows advisory** (not native Windows compilation). The policy-review job fails on a diff to either protected JSON, while `.github/scripts/validate_release_policy_contracts.py` uses the existing runtime validators plus in-memory SQLite and Config v5 structural checks. Workflow 01 runs that preflight and the shared `.github/scripts/test_*.py` suite **before** release version-stamping; its Store Submit Update mode also requires Extended Windows validation. These are checks of the owner-approved contracts, **not another source of policy rules**, nor proof of live Windows/PostgreSQL mirror parity. GitHub rulesets must separately designate appropriate checks as required; adding a workflow alone does not guarantee branch protection.

The numbered release workflows live under `.github/workflows/`; source builds and Docker packaging use `build/Windows/` and `build/WEBGui/`, not retired root application folders. [Build layout](build-layout.md).

**Still to establish independently:** native Windows Rust/Slint validation against a real build, installed Artist/Album UI appearance, saved Windows credential-directory policy revision, full deployed PostgreSQL policy/palette comparison, and end-to-end migration/indexer persistence. The owner provided read-only PostgreSQL singleton evidence for media-state revision 3 and COMP revision 1 with matching listed condition/color order; this does not verify all saved fields or Windows mirrors. Prior CI passes do not prove those deployed properties. [Validation ledger](SCHEMA-VALIDATION.md).
