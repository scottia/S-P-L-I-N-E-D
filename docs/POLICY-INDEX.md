# S:P:L:I:N:E:D — policy and indexer source map

**Basis:** Windows/WebGUI implementation paths audited November 20, 2026, supplemented by an owner-directed shared lifecycle contract review on November 23. This index maps approved requirements and proposed shared lifecycle outcomes to actual source surfaces; mapping alone does not prove deployed compliance. [POLICY-REGISTER.md](POLICY-REGISTER.md) remains the owner-authoritative behavioral register.

## Two application roots and indexers

| Scope | Windows — `build/Windows/` | WebGUI — `build/WEBGui/` |
| --- | --- | --- |
| Persistent catalog | `src/media_database.rs`; SQLite `splined.db` | `webgui/catalog.py`, `webgui/services.py`; separate PostgreSQL catalog |
| Index generations and batches | `src/catalog_index_coordinator.rs`, `src/index_batch_policy.rs` | `webgui/index_coordinator.py`, `webgui/index_batch_policy.py`, `webgui/migrations/021_catalog_index_coordinator.sql` |
| Refresh/selection | `src/scan_controller.rs`, `src/scan.rs` | `webgui/services.py`, `webgui/catalog.py` |
| Synchronized field ownership | SQLite catalog is authoritative for designated Artist/Album/Track fields | `webgui/catalog_authority.py`; PostgreSQL-only jobs, audit, validation and coordination remain local |
| Artist/Album NAME and status dots | `src/media_state_policy.rs`, `ui/app.slint` | `webgui/media_state_policy.py`, `webgui/catalog.py`, `webgui/static/app.js`, `app.css` |
| Normalized Artist presentation and physical Art | `src/media_database.rs`, `src/application.rs` | `webgui/catalog.py`; display groups retain independent rows and Album keys |
| COMP contract | `src/comp_report_policy.rs` | `webgui/comp_report_policy.py` |
| Artwork-only media write boundary | `src/embedded_artwork.rs`, `src/scan_runtime.rs`, `src/safe_write.rs` | `webgui/backend/splined_compilation_authority_policy.py`, `webgui/services.py`, `webgui/backend/splined_scan.py` |

[Catalog authority](catalog-authority.md) and [Windows media database](splined-media-database.md) describe write ownership and indexer persistence; [WebGUI guide](browser-workspace-guide.md) covers the browser's index/queue operation. Neither client's live database is a substitute policy definition.

## Five checked-in `schema/` contracts

| Source | Owner and usage | Review status |
| --- | --- | --- |
| [`media-state-classification.json`](../schema/media-state-classification.json) | Owner-approved Artist/Album NAME colors, all Album dots and serialized presentation settings. Embedded by Windows for offline defaults, loaded/revisioned by WebGUI PostgreSQL and optionally mirrored in Windows credentials. | Exact priority validation added in both languages; deployed revision/palette comparison remains a separate read-only task. |
| [`comp-report-policy.json`](../schema/comp-report-policy.json) | Shared Artist/Recording identity, MusicBrainz ranking, source/COMP options and batch revision pinning. | Protected accepted working contract; test runtime parity independently. |
| [`lifecycle-policy.json`](../schema/lifecycle-policy.json) | Owner-directed common lifecycle behavioral acceptance for equivalent Windows/SQLite and WebGUI/PostgreSQL workflows, with WebGUI-specific `WEB-LIFE` refinements. | Native Rust and Python consumers validate the checked-in contract before catalog/database startup; the independent review gate protects add/change/delete/rename. Behavioral and installed parity remain separately measured. |
| [`media.sql`](../schema/media.sql) | Windows/compatible SQLite catalog DDL; not an Artist/Album color policy. | Validate schema and migrations with disposable databases before any change. |
| [`settings-ui.json`](../schema/settings-ui.json) | Config v5 settings/UI field descriptions; no independent database or artwork authority. | Check defaults/ranges against both active implementations. |

The three independently protected behavioral JSONs are media-state, COMP and shared lifecycle. Native Rust and Python lifecycle consumers validate the exact checked-in contract locally and fail closed before catalog/database startup. Its `LIFE-001..009` requirements concern equivalent behavior and do not alter the canonical Artist ranking **Red > Yellow > Purple > Green**, Album NAME ranking **Dark Grey > Red > Yellow > Purple > Orange > Green**, or the independent Album-dot ordering **Dark Grey > Red > Yellow > Purple > Orange > Green > auxiliary Blue**. Loader/gate activation is not installed parity or database transport. See [POLICY-REGISTER.md](POLICY-REGISTER.md) and [lifecycle-policy.md](lifecycle-policy.md) for exact acceptance, evidence and open gaps.

## Requirements mapped to live code and regression tests

| Approved group | Source/tests |
| --- | --- |
| `ART-COLOR-001..006`, `ALB-COLOR-001..006` | Windows `src/media_state_policy.rs`, `ui/app.slint`; WebGUI `media_state_policy.py`, `catalog.py`, `static/app.js` and `app.css`; `webgui/tests/test_media_state_classification_policy.py`, `.github/scripts/test_media_state_rendering.py` |
| `ART-001..ART-004`, `SAFE-001` | Shared conformance input `.github/test-data/artist-identity-conformance.json`; Windows ordered ownership/credit projection, alias grouping, deterministic label selection and typed Artist search in `src/media_database.rs`/`src/application.rs`; WebGUI equivalent credit resolution, `_artist_groups`, `_artist_identity_rows`, label selection, Artist search/overview and thumbnail resolution in `webgui/catalog.py` and `backend/splined_media_tags.py`. Focused Rust and Python tests require identical presentation identities and association outcomes while preserving the independent COMP matcher. |
| `UI-001` | Windows job/progress publication in `src/application.rs` and Slint bindings; WebGUI job state in `services.py`/`server.py` and polling/rendering in `static/operations.js` |
| `LIFE-001..009` — shared outcomes | Canonical [shared lifecycle JSON](../schema/lifecycle-policy.json), [source/evidence map](lifecycle-policy.md), Windows loader `build/Windows/src/lifecycle_policy.rs` invoked by `main.rs`, and WebGUI loader `build/WEBGui/webgui/lifecycle_policy.py` invoked before `Context` creation in `server.py`. Native loader tests prove policy identity/scope/refinement validation; mapped behavior tests and documented gaps remain the evidence boundary for each rule. |
| `WEB-LIFE-001..009` — WebGUI refinements | Existing approved WebGUI lifecycle refinements in the [Policy Register](POLICY-REGISTER.md) for its browser/Python/PostgreSQL queue and interaction mechanisms. The common `LIFE` contract applies to equivalent operations in both runtimes, while WebGUI-only browser/nonce details remain here. No permission to rewrite Windows-owned synchronized fields. |
| `VISUAL-001..004` | Existing status dots, card frames/banners, override provenance and COMP-track presentation; do not infer those from NAME classification |
| `STATE-POLICY-001..004` | Same canonical ordering in both runtimes; Windows policy loader/mirror, WebGUI PostgreSQL policy revision, inventory-derived Unprocessed count |
| `GUARD-001..004` | Owner-approved review contract in `AGENTS.md` and the register; `.github/workflows/validate.yml` runs the independent `policy-review` job and `.github/scripts/check_protected_policies.py` rejects any diff to the media-state, COMP or lifecycle policy JSON. Focused negatives cover lifecycle add/change/delete/rename. Official Workflows 01–03 audit canonical contracts and the exact selected `main` source before building. Required GitHub ruleset enforcement remains a separate repository-setting check. |

| `AUDIO-INTEGRITY-001..006` | **Only embedded front art may change in audio files.** Both runtimes' writer paths above; disposable ID3v2.x multivalue/MBID and other-container regression fixtures; negative stage-mutation cases; mandatory fail-closed, scope-appropriate required-PR integrity checks. Official Workflows 01–03 package the exact approved `main` commit without treating Workflow 20 evaluation artifacts as release inputs. Separate COMP candidate/results and media-state JSON policies remain unchanged. |

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
| Regression and required release gate | `src/artwork_integrity.rs` Rust tests | `webgui/tests/test_audio_integrity.py`, `test_comp_job_scope.py` | Required real-writer PR validation in `.github/workflows/validate.yml`; official Workflows 01–03 audit/build the exact already-approved `main` source without accepting Workflow 20 artifacts as release evidence |

Native Windows test evidence, fail-closed supported-format coverage and installed-client
acceptance must be recorded before any release. If a format cannot be proven
metadata-safe, its embedded artwork write must reject the file unchanged,
not silently downgrade tags or skip protection. Protected `/schema` policies
are never edited to legitimize a writer.

## CI and deployment distinctions

`.github/workflows/validate.yml` performs scope-appropriate required PR checks, including
real-writer Windows and WebGUI Audio Integrity regressions when their source
or shared behavior is affected, the independent **Canonical policy JSON — review gate**,
and relevant Python, browser, and structural checks. The Windows PR advisory
is not proof of native Windows compilation. The release-policy contract
validator uses existing canonical validators, Config v5 checks and in-memory
SQLite DDL; passing structural checks alone is not runtime or deployed parity.

**The former Workflow 10 and split Workflows 21–24 are retired.** Official
Workflows 01–03 run from `main`, retain candidate and final Build Reports, and
publish only their exact selected targets. Workflow 20 remains evaluation-only
and cannot legitimize a failing or skipped required PR writer test. Protected
JSON review and audio-write safety remain independent fail-closed gates.

**Shared lifecycle acceptance:** `LIFE-001..009` in the
[shared JSON](../schema/lifecycle-policy.json) define common behavioral
outcomes for equivalent Windows and WebGUI operations. The approved
`WEB-LIFE-001..009` remain WebGUI-specific refinements. Refer to
[lifecycle-policy.md](lifecycle-policy.md) for native source mapping,
native runtime-load/validation and fail-closed JSON protection, mapped behavior
evidence, and outstanding parity gaps. The source JSON and its loaders do not
implement SQLite/PostgreSQL synchronization or prove installed behavioral parity.
The numbered release workflows live under `.github/workflows/`; source builds and Docker packaging use `build/Windows/` and `build/WEBGui/`, not retired root application folders. [Build layout](build-layout.md).

**Still to establish independently:** native Windows Rust/Slint validation against a real build, installed Artist/Album UI appearance, saved Windows credential-directory policy revision, full deployed PostgreSQL policy/palette comparison, and end-to-end migration/indexer persistence. The owner provided read-only PostgreSQL singleton evidence for media-state revision 3 and COMP revision 1 with matching listed condition/color order; this does not verify all saved fields or Windows mirrors. Prior CI passes do not prove those deployed properties. [Validation ledger](SCHEMA-VALIDATION.md).
