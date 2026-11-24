# Shared Windows/WebGUI lifecycle policy

The checked-in [`schema/lifecycle-policy.json`](../schema/lifecycle-policy.json)
defines the **common lifecycle acceptance contract** for equivalent operations
in the supported Windows Rust/Slint application and Python/browser WebGUI.
[POLICY-REGISTER.md](POLICY-REGISTER.md) is the owner-authoritative rulebook;
this page explains policy scope and the source/test boundaries. It does not
create a competing approval authority.

## What is shared, and what is not

Both clients must produce the same **observable lifecycle outcomes** wherever
an equivalent workflow exists: a current selection wins, progress describes
real work, stale decisions cannot execute, completion requires verification,
canonical Artist/Album classifications remain stable, and independent writes
respect catalog ownership.

Identical outcome requirements do **not** require the same UI technology,
request transport, queue type, timestamp format, token representation, or
storage engine. Windows uses Rust/Slint and SQLite; WebGUI uses browser
JavaScript, Python and PostgreSQL. The clients may implement equivalent
rules with different native primitives.

**A JSON policy is not a database link.** The contract does not connect
SQLite to PostgreSQL, copy catalog data, create schema migrations, replace
reconciliation, or modify the Windows-managed synchronized field set.
For actual catalog authority and durable artwork events see
[catalog-authority.md](catalog-authority.md).

The previously approved WebGUI-specific `WEB-LIFE-001..009` rules remain
in the [Policy Register](POLICY-REGISTER.md) as refinements. The new
`LIFE-001..009` shared rules apply the equivalent *behavioral guarantees*
to supported Windows workflows too. When a client does not expose an
equivalent operation, record the nonapplicable surface rather than inventing
new controls or copying UI code.

## Shared requirements and source mapping

| Shared ID | Behavioral acceptance | Windows source boundaries | WebGUI source boundaries |
| --- | --- | --- | --- |
| `LIFE-001` | Latest valid operation/selection wins; obsolete work cannot repaint current state | `build/Windows/src/application.rs`, `scan_controller.rs`, `ui/app.slint` | `build/WEBGui/webgui/static/app.js`, `static/operations.js` |
| `LIFE-002` | Render usable indexed library/overview without waiting for unrelated secondary hydration | `build/Windows/src/media_database.rs`, `application.rs`, Slint model/bindings | `build/WEBGui/webgui/catalog.py`, `static/app.js` |
| `LIFE-003` | Bound discovery, metadata, image hydration, indexing probes and coordination; no request storms | `build/Windows/src/scan_controller.rs`, `catalog_index_coordinator.rs`, image-cache paths | `build/WEBGui/webgui/index_coordinator.py`, `static/app.js`, `static/operations.js` |
| `LIFE-004` | Staged/running/review/terminal work stays coherent, recoverable and fail-closed | `build/Windows/src/application.rs`, `scan_runtime.rs`, `scan_controller.rs` | `build/WEBGui/webgui/jobs.py`, `static/operations.js` |
| `LIFE-005` | Status is generation-owned; staging is not verified completion | `build/Windows/src/application.rs`, `scan_controller.rs`, Slint UI | `build/WEBGui/webgui/jobs.py`, `static/operations.js` |
| `LIFE-006` | Selection does not discard verified completion or fabricate Red; preserve independent ranked colors/dots | `build/Windows/src/media_state_policy.rs`, `application.rs`, `ui/app.slint` | `build/WEBGui/webgui/media_state_policy.py`, `catalog.py`, `static/app.js` |
| `LIFE-007` | Typed Artist/Album search, pinned COMP selection, configured embedded dimensions and artwork-only writes | `build/Windows/src/comp_report_policy.rs`, `scan_runtime.rs`, `application.rs` | `build/WEBGui/webgui/comp_report_policy.py`, `services.py`, `static/operations.js` |
| `LIFE-008` | Revalidation and background work preserve authoritative synchronized fields and separate owned evidence | `build/Windows/src/media_database.rs`, `catalog_index_coordinator.rs` | `build/WEBGui/webgui/catalog_authority.py`, `services.py`, `index_coordinator.py` |
| `LIFE-009` | Focused regression acceptance of normal, stale, cancelled, recovered and completed transitions | Applicable Windows Rust/Slint and installed-client tests | Applicable WebGUI Python/browser/PostgreSQL and installed-client tests |

The paths above identify **implementation surfaces to inspect**, not proof
that the newly specified shared lifecycle contract is fully implemented.
Acceptance requires source tracing, targeted tests, and installed-client
evidence for the affected behavior. Do not infer parity from shared terminology
or a successful general CI job.

## Focused source and behavior evidence

The native consumers bind the same accepted IDs to each client, while the
behavior remains implemented by each client's existing mechanisms. The table
records current focused evidence and does not upgrade an open defect into a
pass.

| ID | Windows focused evidence | WebGUI focused evidence | Current parity gap |
| --- | --- | --- | --- |
| `LIFE-001` | `application.rs` selection/COMP event tests and `scan_controller.rs::duplicate_refresh_claims_attach_to_the_active_generation_result` | Browser tests for late API/metadata/COMP responses and single-flight writes | Installed rapid-selection parity remains unmeasured. |
| `LIFE-002` | Resident-snapshot tests exist, but PR #307 records synchronous row artwork and broad model refresh during startup/selection | Visibility-queued thumbnails and stale-response tests; PR #314 retains measured load/selection defects | Windows shell-first/memory-only rows and both clients' accepted performance remain open. |
| `LIFE-003` | Index batch bounds, generation claims and cancellation tests | Periodic-poll, action-refresh, single-flight and thumbnail-queue browser tests | PR #314 request fan-out/coordination/probe findings remain open; Windows image work remains under PR #307 review. |
| `LIFE-004` | Scan queue, explicit COMP target, cancellation and recovery tests | Durable COMP submission, duplicate-selection, STOP and queue-decision browser tests | PR #314 stale SKIP/staged-batch defects remain open. |
| `LIFE-005` | Target completion, failed-target recovery, index cancellation and published-catalog tests | Status ownership, verified COMP readback and refresh cancellation tests | PR #314 stale embedding/completion status remains open. |
| `LIFE-006` | Artist aggregate, override, independent Album-row and media-state policy tests | Ranked classification and partial-hydration browser tests; PR #314 contains the focused GREEN→RED correction | PR #314 is still Draft; installed presentation parity is not claimed. |
| `LIFE-007` | Explicit COMP target, selected MusicBrainz result, shared COMP policy and media-writer integrity tests | Preferred-size COMP, pinned result/track, typed Album view and real-writer integrity tests | PR #314 search typing, post-selection stage and normalization observations remain open. |
| `LIFE-008` | SQLite generation ownership, resume/cancel and published-catalog preservation tests | `catalog_authority.py` field ownership and PostgreSQL refresh completion/cancellation tests | PR #307 documents the missing approved durable revalidation marker; no migration is authorized. |
| `LIFE-009` | Native lifecycle loader negatives plus focused Rust behavior tests above | Native lifecycle loader negatives plus focused Python/browser tests above | Installed-client acceptance and remaining PR #307/#314 defect closure are still required. |

## Rule precedence

The shared lifecycle contract **supplements**, and never replaces:

- [Artist/Album state classification](media-state-classification-policy.md),
  including independently ranked Artist names, Album names, simultaneous
  status dots and the legitimate unmatched Red default;
- [COMP report policy](comp-report-policy.md), its pinned results, percentage
  bands, authoritative completion and selected-Track rules;
- `AUDIO-INTEGRITY-001..006`, requiring fail-closed front-artwork-only
  media writes and preservation of every non-artwork tag and audio byte;
- Windows SQLite authority for designated synchronized Artist/Album/Track
  fields, PostgreSQL-owned validation/jobs/audit, and append-only
  coordination history.

An operation showing Green or `completed` must have the applicable verified
evidence. A partially hydrated UI or an invalid old queue decision cannot
replace it with a fabricated default or green completion.

## Source activation versus behavioral parity

The shared contract now has native, bounded, local fail-closed consumers:

- Windows embeds `schema/lifecycle-policy.json`, validates it once at process
  start, and fails before database/provider/catalog work if identity, runtime
  scope, ordered IDs, refinement mapping or acceptance shape is invalid.
- WebGUI reads the checked-in/container copy once and validates the same fields
  before `Context`, PostgreSQL, workers or providers are initialized.
- The independent canonical-policy gate protects lifecycle policy addition,
  modification, deletion and rename without weakening the existing
  media-state or COMP protections.

There is still no SQLite table, PostgreSQL table, synchronized revision, live
policy mirror, database migration or automatic cross-client exchange. Loader
success proves contract identity and availability, not every behavioral
outcome or installed-client parity; the evidence table above remains binding.

## Activation implementation and remaining acceptance

The accepted JSON was introduced first by PR #318. The independently reviewed
activation stage implements the two source-enforcement prerequisites without
claiming completion of the open behavior work:

1. **Native policy consumers — implemented and focused-test validated.** Both
   clients validate schema version, policy ID, nine stable IDs, both-client
   applicability and WebGUI refinement mapping before database/catalog work.
2. **Independent JSON protection — implemented and focused-test validated.**
   Add/change/delete/rename operations fail closed; existing protections remain.
3. **Measured behavior acceptance — partially evidenced, still open where
   recorded above.** Defects remain in their independently scoped Draft PRs.
4. **Zero coupling — source verified.** Consumer code has no DB, network,
   provider, inventory, persistence or cross-client synchronization path.
5. **Downstream sequencing — pending activation acceptance in `dev`.** PR #307
   and PR #314 may be synchronized and mapped after this activation is accepted
   into their current base; they retain independent review and merge gates.

Do not describe the remaining behavior or installed-client gaps as complete.
Source activation and independent protection are narrower than observed parity.

## Future implementation and review

1. Begin from the accepted `LIFE-001..009` shared outcomes; for WebGUI
   apply the existing `WEB-LIFE-001..009` refinements as well.
2. Keep each client's storage, job queue, cancellation and presentation
   implementation independent. Compare equivalent outcomes, not identical
   function names or database tables.
3. Record which currently supported surfaces satisfy the contract and
   which need a focused correction. Do not silently rewrite approved media,
   COMP or authority policy to make an implementation pass.
4. Run only focused tests appropriate to the changed surface; preserve
   accepted test evidence and report real installed parity separately.
5. Obtain explicit owner approval before any data-ownership, runtime-loader,
   policy-revision persistence or migration implementation. Never update
   production SQLite, PostgreSQL, media or credential state during this review.

This document and the JSON belong to the **single source of truth** in the
repository; no independent Windows-only or WebGUI-only copy is created.
