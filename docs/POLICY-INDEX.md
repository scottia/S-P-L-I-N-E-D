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
| Artwork-only media write boundary | `src/embedded_artwork.rs`, `src/scan_runtime.rs`, `src/safe_write.rs` | `webgui/backend/splined_compilation_authority_policy.py`, `webgui/services.py`, `webgui/backend/splined_scan.py` |

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

| `AUDIO-INTEGRITY-001..006` | **Only embedded front art may change in audio files.** Both runtimes' writer paths above; disposable ID3v2.x multivalue/MBID and other-container regression fixtures; negative stage-mutation cases; mandatory fail-closed PR and Workflow 01/04 integrity checks. Separate COMP candidate/results and media-state JSON policies remain unchanged. |

The test fixture that once lived under root `fixtures/` was retired; it was never a production policy. Independent Windows and WebGUI compilation tests now own the applicable release/category cases. Release scripts and synthetic Store test fixtures belong in root `release/`.

## Audio mutation ownership and mandatory coverage

`AUDIO-INTEGRITY-001..006` from the owner-approved
[Policy Register](POLICY-REGISTER.md) are universal, **non-editable**
audio-file permissions. The independent COMP-only
[`comp-report-policy.json`](../schema/comp-report-policy.json) governs
discovery, fuzzy scoring, revision-pinned batches and completion,
**not** the right to edit MusicBrainz or other audio tags.

| Operation | Windows source | WebGUI source | Safety validation |
| --- | --- | --- | --- |
| Album preferred-size thumbnail and one-track COMP artwork | `src/scan_runtime.rs`, `src/embedded_artwork.rs` | `webgui/services.py`, `webgui/backend/splined_compilation_authority_policy.py` | `src/artwork_integrity.rs` checks MP3/FLAC stages; `webgui/backend/splined_audio_integrity.py` checks supported stages before replacement |
| COMP MBID selection and fuzzy alternate-cover matching | `src/comp_report_policy.rs`, `src/scan_runtime.rs`; SQLite catalog only | `webgui/comp_report_policy.py`, `webgui/backend/splined_scan.py`; session/catalog only | No MusicBrainz/audio-text writes permitted under any branch |
| Regression and required release gate | `src/artwork_integrity.rs` Rust tests | `webgui/tests/test_audio_integrity.py`, `test_comp_job_scope.py` | `.github/workflows/validate.yml`, both `01-full-release.yml` and `04-optimized-build-test.yml` |

Native Windows test evidence, fail-closed supported-format coverage and installed-client
acceptance must be recorded before any release. If a format cannot be proven
metadata-safe, its embedded artwork write must reject the file unchanged,
not silently downgrade tags or skip protection. Protected `/schema` policies
are never edited to legitimize a writer.

## CI and deployment distinctions

`.github/workflows/validate.yml` executes scope checks, WebGUI Python/JavaScript/Docker/PostgreSQL checks when applicable, the independent **Canonical policy JSON — review gate** and a **Windows advisory** (not native Windows compilation). The policy-review job fails on a diff to either protected JSON, while `.github/scripts/validate_release_policy_contracts.py` uses the existing runtime validators plus in-memory SQLite and Config v5 structural checks. Workflow 01 runs that preflight and the shared `.github/scripts/test_*.py` suite **before** release version-stamping; its Store Submit Update mode also requires Extended Windows validation. These are checks of the owner-approved contracts, **not another source of policy rules**, nor proof of live Windows/PostgreSQL mirror parity. GitHub rulesets must separately designate appropriate checks as required; adding a workflow alone does not guarantee branch protection.

**Audio-write safety is independent of the canonical policy JSON review gate.** PR validation must run actual WebGUI and Windows artwork-only mutation regressions. Both release workflows must block release publication if these real-writer tests fail, even when Extended Windows Validation is OFF or the target is WebGUI-only. Required GitHub check contexts are an additional repository-ruleset requirement; a successful optional advisory is not a mandatory integrity pass.

The numbered release workflows live under `.github/workflows/`; source builds and Docker packaging use `build/Windows/` and `build/WEBGui/`, not retired root application folders. [Build layout](build-layout.md).

**Still to establish independently:** native Windows Rust/Slint validation against a real build, installed Artist/Album UI appearance, saved Windows credential-directory policy revision, full deployed PostgreSQL policy/palette comparison, and end-to-end migration/indexer persistence. The owner provided read-only PostgreSQL singleton evidence for media-state revision 3 and COMP revision 1 with matching listed condition/color order; this does not verify all saved fields or Windows mirrors. Prior CI passes do not prove those deployed properties. [Validation ledger](SCHEMA-VALIDATION.md).
