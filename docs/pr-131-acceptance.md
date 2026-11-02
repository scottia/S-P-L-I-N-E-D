# PR #131 PC acceptance report

Repository: `scottia/S-P-L-I-N-E-D`

Branch: `fix/webgui-functional-recovery-audit`

Base: `dfe0bf8ce8274d7272882a57a6dc3fc10d4897e2` (`main`, release `2.0.18`)

Implementation revision exercised: `39463bc5a5ae9384d60c98920a8fb8a3e886a493`

State: draft, open, unmerged. No release, tag, GHCR publication, production deployment, `dev` change, or Windows-source change was made.

## PR #130 test bed retained

PR #131 was installed over the same PR #130 PC test bed without changing its paths or service configuration:

- Runtime root: `E:\apps\SPLINED-WebGUI`
- Source checkout: `E:\apps\ChatGpt\S.P.L.I.N.E.D`
- Launcher: `E:\apps\SPLINED-WebGUI\Start-WebGUI.ps1`
- WebGUI: `http://127.0.0.1:8080`
- Configuration: `E:\apps\ChatGpt\SPLINED-local-test-library\state\config\config.toml`
- PostgreSQL: `127.0.0.1:55432/splined`
- Existing media root: `N:\media\music`
- Screenshot path: `E:\apps\SPLINED-WebGUI\test-screenshots-pr130`

The service is the existing native FastAPI/PowerShell test service, not a Docker or published `2.0.18` image. It was gracefully restarted from the PR branch. Public health reports version `2.0.18`, PostgreSQL ready, and authentication required. The authenticated audit session records both version and Git revision.

## Backup and migration

Before migration/restart, the existing test configuration and PostgreSQL database were backed up to:

`E:\apps\SPLINED-WebGUI\backups\pr131-before-migration-20261101-171654`

- Configuration SHA-256 before and after: `AA208680243AB46118613B5623E86A4761520F4F2B44359BB08F5D3600C0BFB5`
- PostgreSQL dump SHA-256: `949F215A1D5E84E7410A77E0D54033BCDEB25859E2F0510818BBC0DCDC200F18`
- Applied schema versions: `1` through `11`
- Required tables verified: `salite_diff`, `web_audit_sessions`
- Reconciliation schedule: `enabled=false`
- Automatic safe application: `auto_apply_safe=false`
- Saved SQLite source: none

No SQLite differences or conflicts were applied. The reconciliation implementation remains Compare/Dry Run only for owner acceptance; Apply Safe Differences, scheduling, and automatic application remain disabled.

## Root causes corrected

- COMP launch filtered selected rows back to blue-only pending work. Explicit blue, purple, and green selections now retain exact Artist/Album/track scope through both Artist COMP and full Fallback launch controls. REPAIR COMP remains separate.
- Ordinary queued/running work always entered the modal review path. Only decisions, intervention-required errors, and explicit confirmations now open dialogs; normal progress stays in status and Audit.
- Preserved cover replacement wrote a numbered file while the old exact `cover.*` remained canonical. Replacement now preserves the prior file when configured, publishes the new exact canonical cover, verifies readback, and refreshes the library view.
- Album selection hydrated mutable filesystem state before returning the indexed Album. Albums navigation now reads durable PostgreSQL state and performs only bounded fallback hydration, so stale media paths do not crash the view.
- Similar Artists substituted arbitrary local catalog rows when provider results were absent. Only genuine configured-provider recommendations are displayed; authorized local images may decorate an identity match, while absence/provider failure is represented accurately and audited.
- Artist information and Media Filter were permanently expanded. Artist details now use the accessible cyan `⏬` control and both requested surfaces default to collapsed while preserving manual state.
- The CI browser test filled hidden `#artistSearch` after the intentional Media Filter change. The test now asserts the collapsed default, opens it, fills the field, and verifies that navigation preserves the user's expanded state.
- Rendered acceptance reused broad response predicates during asynchronous MusicBrainz actions. The waits now match the intended action payload and prompt nonce, removing the false race without weakening assertions.
- Windows could briefly hold the disposable FLAC during atomic publication. A bounded retry now surrounds only `os.replace`; source audio is never edited in place.
- Legacy audit events lacked session/build identity, truthful export completeness, and recoverable storage failure handling. The existing `web_audit` authority now supplies these capabilities without a competing log.

## Functional and rendered results

The disposable PostgreSQL/MP3/FLAC suite and headless installed Brave validated:

- Blue, purple, and green COMP tracks through both launch controls, including Artist COMP and full Fallback scope isolation.
- Completed Album artwork replacement, re-embedding, canonical-cover refresh, and verified readback.
- Fresh Albums view selection, view switching, identified/incomplete/completed/COMP projections, and recoverable errors without a WebGUI crash.
- Genuine Similar Artists identity, local/provider imagery rules, unavailable state, and asynchronous Artist identity retention.
- Absence of ordinary queue-state modals while required decisions and failures remain visible.
- Artist Overview and Media Filter collapsed defaults plus manual-state persistence.
- Read/write boundaries, queue deduplication, preserved files, and MusicBrainz authority from PRs #129/#130.

No test read or changed production media. All write tests used disposable MP3/FLAC fixtures and disposable PostgreSQL schemas.

## Audit validation and export

Migration `009_audit_diagnostics.sql` extends ordered structured events, and `011_audit_sessions_recovery.sql` adds diagnostic sessions plus recoverable fallback identity. Every session records application version and revision. Browser actions, selections, API request/response pairs, provider results, job transitions, verified writes, reconciliation, warnings, debug, verbose, and errors use action/trace/job correlation.

One deliberately failed Album bypass action was executed against a disposable PostgreSQL schema and downloaded through the authenticated `/api/audit/export` endpoint. The six selected events are in strict sequence:

1. `button-click`
2. `api-request`
3. `api-error`
4. `api-response` (`404`)
5. `api-client-error`
6. `browser-action-failed`

All six contain the same action and trace IDs, session ID, version `2.0.18`, and revision `39463bc5a5ae9384d60c98920a8fb8a3e886a493`. The injected synthetic password is `[REDACTED]`; the raw value is absent. Export metadata reports six selected records, no truncation, no pending fallback records, and `complete_for_retained_window=true`.

- PC export: `E:\apps\SPLINED-WebGUI\audit\pr131-audit.json`
- PC export SHA-256: `976187E19079A3DA8DCF8FC0F683A0AD5E767FA01536AEBC7BD7907508FED075`
- Repository sample: [`pr-131-audit-sample.json`](pr-131-audit-sample.json)

If PostgreSQL audit insertion fails, bounded redacted JSONL records remain recoverable and are replayed idempotently in original order. Export v2 reports retained bounds, pruned-before-window count, pending fallback count/bytes, selection truncation, and the reason for any truncation. Audit preview and download remain authenticated.

## Exact verification

- Full PC suite with required `PYTHONPATH`, actual PostgreSQL, and installed Brave: `233` tests in `241.488s`, `OK`.
- Focused audit failure/export: `1` test in `3.674s`, `OK`.
- PostgreSQL integration module before the final audit case: `34` tests in `223.518s`, `OK`.
- Focused rendered Brave workflow: `1` test in `78.153s`, `OK`.
- GitHub Actions run `41364590140` on `39463bc`: success (`webgui` success, Windows advisory success, native correctly skipped by scope).
- Python compilation, JavaScript syntax checks, boundary tests, and `git diff --check`: passed.
- Expected non-failing warning: Starlette's installed `httpx` compatibility deprecation. The PC Python 3.12 environment already contains `psycopg 3.3.6` and `httpx 0.28.1`; no dependency download was needed.

## Live Brave limitation

Native Brave is open at the restarted PC-hosted WebGUI login page. The existing administrator is `user`, but its saved password is not available to automation. The PostgreSQL service credential is intentionally not a WebGUI administrator credential. No password was guessed, reset, or extracted.

Therefore, the rendered Brave acceptance above is real and passed against the installed Brave executable, but an owner-authenticated manual pass against the long-running service is not claimed. After the owner signs in with the existing saved password, the same live URL is ready for final visual/interactive acceptance without changing the PR #130 paths.

## Changed areas

- Audit: `webgui/audit.py`, `webgui/server.py`, `webgui/services.py`, `webgui/jobs.py`, migrations `009` and `011`, Logging UI, and audit tests/sample.
- Functional/UI: `webgui/catalog.py`, `webgui/static/index.html`, `app.js`, `app.css`, `operations.js`, guide, and reported-defect/rendered tests.
- Reconciliation: `webgui/reconciliation.py`, migration `010_sqlite_reconciliation.sql`, Advanced UI, and isolated safety tests.
- PR #129/#130 authority: the existing compilation authority backend was preserved; only the bounded Windows atomic-publication retry and its test were added.

The draft PR remains stopped for owner acceptance. No production deployment or merge was performed.
