# PR #131 acceptance report

Branch: `fix/webgui-functional-recovery-audit`  
Base: `dfe0bf8` (`2.0.18`, current `origin/main`)  
State: draft work only; unmerged; no release, tag, image publication, deployment, `dev` change, or Windows-source change.

## Root causes corrected

- Both COMP launch paths selected only blue rows. Explicit purple and green selections were discarded before enqueueing.
- Ordinary queued/running jobs always entered `reviewJob()`'s modal path, even when no decision or intervention was required.
- Preserved cover replacement changed the destination to `cover-(n).*`, leaving the old exact `cover.*` file as the displayed canonical cover.
- `/api/album` performed mutable filesystem track hydration before returning an already indexed album. A stale or inaccessible media path could therefore break Albums navigation after the list had loaded.
- Similar Artists used arbitrary local catalog rows when Navidrome did not return a recommendation, presenting unrelated Artists as similarity results.
- Artist information and Media Filter initial markup were permanently expanded.
- The legacy SQLite import enforced an empty PostgreSQL album catalog and offered no reviewable difference history.
- WebGUI events lacked end-to-end action/trace/job correlation and a complete structured export.

## Reviewable commits

1. `1923fea` — structured WebGUI audit diagnostics and export.
2. `4120e88` — COMP, queue modal, album navigation, cover replacement, Similar Artists, collapsed UI, and WebGUI About/version card.
3. `ef2204a` — incremental SQLite reconciliation, migration, Advanced UI, scheduler safeguards, and focused tests.
4. Acceptance artifacts and reproducible disposable render harness (this report's commit).

## Functional results

- Explicit blue, purple, and green tracks retain exact Artist/Album/track scope through both Artist COMP and full Fallback launch paths. With no explicit track, only the existing blue pending set is selected. REPAIR COMP remains separate.
- Queued/running/completed scan and COMP jobs stay in the status area. Waiting decisions, failures, and intervention-required states still use the dialog.
- A replacement image becomes the exact canonical `cover.*`; a prior exact cover is preserved as `cover-(n).*` when configured. Byte-for-byte readback and dimensions are verified before refresh.
- Album selection reads the durable index; fallback track inventory hydrates only when its indexed count is incomplete.
- Similar Artists are projected only from the configured provider. Local matches supply authorized local portraits; provider cover art can be cached behind signed local image URLs; absence is labeled and audited.
- Artist information is collapsed by default behind the accessible cyan `⏬` button. The Artist name is yellow below the thumbnail. Media Filter is initially collapsed and retains manual state during navigation.
- The WebGUI header includes an About card showing the running version (`2.0.18` in the acceptance harness), WebGUI/PostgreSQL runtime, mode, catalog, and project links. Windows sources were not modified.

## Audit diagnostics

Migration `009_audit_diagnostics.sql` extends the existing `web_audit` authority with ordered severity, action/trace/job identifiers, scope, stage/status, elapsed time, state transitions, and sanitized errors. Browser actions, API responses, workers, provider results, verified writes, and reconciliation use the same sequence.

The authenticated Logging UI filters by severity, action, job/trace/action correlation, and time. `/api/audit/export` returns a valid `audit.json` document for the selected period. Retention is bounded (14 days and at most 50,000 retained events), and polling/image/audit recursion is excluded. Nested secrets, authorization headers, credential URLs, and exception payloads are redacted. Sensitive values are not retained merely for correlation, so no secret fingerprint is needed by the current event contract.

Example export: [`pr-131-audit-sample.json`](pr-131-audit-sample.json).

## SQLite reconciliation migration and safeguards

Migration `010_sqlite_reconciliation.sql` creates the required table named exactly `salite_diff`, plus run and disabled-by-default schedule tables.

- Sources are schema-v2 SQLite snapshots selected only from the configured persistent cache and opened read-only.
- Windows/POSIX source paths require an explicit original-library-root mapping to the configured WebGUI music root.
- Matching prefers durable keys, MusicBrainz identities, mapped paths, and normalized Artist identity.
- Safe inserts and strictly newer supported authority changes are recorded separately from conflicts. Existing cache authority is never overwritten automatically.
- Apply uses a PostgreSQL advisory lock, one transaction, per-difference savepoints, and optimistic old-digest checks.
- No target deletes, job imports, or implicit conflict applications exist. COMP progress and Artist/Album aggregates are recalculated from completion authority rather than copied.
- The uniqueness key on source fingerprint, entity/table key, difference type, and old/new digest makes the same snapshot/target comparison idempotent.
- Scheduled comparison is disabled by default. Saved schedules retry at their interval, may apply only rows still classified `safe`, and never apply conflicts.
- The original empty-catalog importer remains available for compatibility.

## Exact verification results

- Python: `227` tests passed, `34` skipped by existing PostgreSQL environment guards.
- Browser logic: `43` Node tests passed.
- Focused audit/reconciliation/defect/render-contract tests: `18` passed.
- Python compilation, JavaScript syntax check, and `git diff --check`: passed.
- Disposable localhost render harness: started successfully and returned authenticated WebGUI status with version `2.0.18`; it opens no PostgreSQL connection and no media files.

The available PC environment has no PostgreSQL service, Docker CLI, or `psql`, so a real isolated PostgreSQL migration/apply rerun was not executed here. The PostgreSQL-dependent test group remained skipped; this report does not claim that acceptance.

Brave is installed but rendered PC acceptance was unavailable: the Windows automation service was denied permission to launch Brave, and Work settings reported `Local browser use is disabled`. The localhost harness was stopped after the health check. This report therefore does not claim Brave visual acceptance or audio-format write acceptance.

No production music, existing PostgreSQL data, live schedule, deployment, or audio file was read or changed during verification.
