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

## Documented versus implemented

This policy-review change introduces a **declarative contract** and updates
the register, source index and agent instructions. Neither application has a
new runtime loader for `schema/lifecycle-policy.json` in this PR. There is
no new SQLite table, PostgreSQL table, synchronized revision, live policy
mirror, database migration or automatic cross-client data exchange.

The existing independent canonical-policy JSON review gate protects
`schema/media-state-classification.json` and
`schema/comp-report-policy.json`. It does **not yet include** the new
lifecycle JSON. Any expansion of that enforcement gate or runtime integration
requires a separate owner-reviewed implementation scope; do not claim that
the current gate mechanically protects this new file.

## Activation requirements before claiming enforced parity

The owner requests **closing the current runtime-loader and JSON-protection gaps before downstream PR #307/#314 acceptance**. Adding the contract and its source map is the first stage, **not the completion of enforcement**. The following actions are mandatory acceptance gates for an independently reviewed implementation update:

1. **Native policy consumers:** introduce a bounded, local, fail-closed reader/validator in each Windows Rust and WebGUI Python/browser runtime for the exact checked-in `schema/lifecycle-policy.json`. Validate schema version, policy ID, nine stable `LIFE-00x` IDs, intended client scope and WebGUI refinement mapping. A file read alone is insufficient: map each applicable behavior to focused source/tests, using native queues and presentation mechanisms.
2. **Independent JSON protection:** once the JSON is part of the accepted base, extend the existing canonical policy diff gate to reject unauthorized modification, deletion, addition or rename of `schema/lifecycle-policy.json`, with focused negative controls. **Do not** modify the protected-file gate in the same diff that initially introduces the JSON if that would block the authorized bootstrap; never bypass or weaken the current media-state/COMP gates.
3. **Measured acceptance:** confirm that current selection/queue/status/search, canonical colors, artist/album state, COMP integrity and revalidation authority meet the shared contract. Run only affected Rust, Python and browser tests, adding targeted corrections only for demonstrated defects in their own approved PR scope; record actual installed parity only when tested.
4. **Zero coupling regression:** startup remains local and bounded; loading the shared contract must not trigger database updates, network hydration, provider calls, catalog migration, protected-field modifications or new SQLite/PostgreSQL replication. A shared rulebook does not imply common storage or matching implementation internals.
5. **Promotion sequencing:** keep this Draft as a policy specification. After the JSON is accepted in `dev`, the runtime-consumer and independent gate work must be validated through its own approved implementation review; only then should PR #307 (cross-client parity) and PR #314 (WebGUI corrections) be synchronized and mapped to the active common JSON. Both PRs retain their independent acceptance and merge gates.

**Do not delete or weaken the current `not implemented` statements until the corresponding loader, gate and tests have actually passed.** Those statements describe current implementation state, while this section establishes the owner's required end state.

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
