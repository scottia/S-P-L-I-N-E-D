# SPLINED — schema and media-state implementation validation

**Updated for the November 19, 2026 owner decisions.**
Baseline audited: `dev`
`9be357951f0ccb4f2a624a7baf9307699d700e8a`.
This report distinguishes **approved behavior**, **committed
corrections**, **static checks**, and **runtime testing not yet run**.
The approved color policy is [POLICY-REGISTER.md](POLICY-REGISTER.md);
it is **not pending owner re-approval**.

## Source correction ledger

| Topic | Before | Now in `dev` | Remaining verification |
| --- | --- | --- | --- |
| WebGUI white Unprocessed Artist/Album names | `app.css` forced `.library-row[data-media-status=unprocessed] .row-copy strong` to neutral `var(--text)` (white in dark theme), including light-theme override | Those specific forced-name CSS rules **removed**. Semantic name colors use `--status-color` / `--status-color-light` | Verify actual Artist/Album list names and competing CSS specificity in browser |
| Windows neutral Unprocessed/All names | Slint `LibraryItem` supplied theme foreground rather than `item.status-color` when status was Unprocessed or All | Special-case name override **removed**; `root.item.primary` takes `root.item.status-color` | Verify native Windows list projections assign independently ranked colors and remain valid with actual Slint compilation |
| JSON policy wording | Album Red was labeled "Incomplete album tracks"; Artist Yellow only "Missing Artist Art" | `media-state-classification.json` now labels Red **Incomplete / Error** and Artist Yellow **Missing Artist or Album Art** | Verify effective persisted PostgreSQL policy row and Windows local files; the shipped JSON seed is not an automatic migration of installed copies |
| Ranking/dot validator gaps | Python/Rust allowed syntactically valid rank and dot reordering | Both validators now reject order deviations from approved Artist/Album ranking and seven-dot order | Run Python and native Rust test suites against real configured policy revisions |
| Album Red from unprocessed label | WebGUI could produce independent Red/error solely for a missing cover on an `unprocessed` row | Removed that inferred error; missing static art produces Yellow evidence absent independent Red | Test categories, stale status and selected Album/Artist projection |
| Album Orange from existing cover | WebGUI could infer "Processed" from a mere existing cover | That shortcut removed | Verify actual processed state is derived from a legitimate processing event |
| Animated WebP versus static cover | Previewable `cover.webp` could be confused with qualifying static `cover.*` | WebGUI catalog now exports `static_cover_found` separately from previewable `cover_found`; UI evidence checks static cover separately | Verify real album paths, saved summaries, and fallback/COMP category exceptions |
| Approved visuals | Historic descriptions conflicted with the owner's screenshot | **Owner settled the visual authority:** preserve current status dots, Album card frames including **Dark Grey override frame**, banners and COMP Blue/Green/Purple track rows | Verify those visuals/counts are unchanged in both clients |

**Do not resurrect earlier resolved disputes:** Red means **Incomplete /
Error** and Red is also the default unmatched *name* category; the
Dark Grey override **frame** in the owner's screenshot is correct.
"Unprocessed" must remain **only a MEDIA FILTER numeric count** in
the UI, not a file-list name or dot category.

## Current source checks versus full runtime acceptance

**Static/source checks performed:**

- The three JSON documents parse as JSON.
- The Owner's Artist ranking is `Red > Yellow > Purple > Green`
  with Red default, and Album ranking is
  `Dark Grey > Red > Yellow > Purple > Orange > Green`
  with Red default; both name-color exclusion sets are present.
- The Album dot array is `Dark Grey > Red > Yellow > Purple >
  Orange > Green > auxiliary Blue`.
- Focused condition-subset inspections evaluated **16 Artist**
  and **64 Album** combinations without selecting forbidden names.
- Verified source no longer contains the specific WebGUI CSS
  white-name rule or the Windows Slint Unprocessed/All
  neutral-name override.
- Focused source-level tests and independent guard tests exist.
  Their presence is **not proof** of installed-client correctness.
- GitHub Actions run
  [44664332291](https://github.com/scottia/S-P-L-I-N-E-D/actions/runs/44664332291)
  for PR #217's initial documentation-only commit completed successfully,
  including the canonical JSON review gate. That run was documentation
  scope only and does not certify runtime policy behavior. The later
  [PR #217 Docker-source run 44673870120](https://github.com/scottia/S-P-L-I-N-E-D/actions/runs/44673870120)
  passed Python, JavaScript and container checks for the relocated source,
  **not** native Windows build or effective PostgreSQL/Windows policy parity.

**Still untested/undemonstrated at this checkpoint:**

- Full WebGUI Python suite, live PostgreSQL migrations and browser
  behavior against current `dev` (light and dark name text).
- Native Windows Rust/Slint compile and complete native UI suite
  against the current source.
- Effective PostgreSQL policy JSON/revision versus checked-in seed,
  existing Windows credential-directory copies, and offline
  embedded policy behavior.
- Real media fixtures for missing Artist/Album art, only `cover.webp`,
  incomplete/error states, overridden Albums, and embedded art
  missing/below/above the configured preferred size.
- Regression of the screenshot-approved Album dots, card frames,
  banners, COMP-track rows and category handling.

## Current implementation risks requiring code verification

1. **Do not equate processing state and name ranking.** WebGUI
   `app.js::mediaStatusKey()` still falls back to a raw catalog
   processing key for ordinary Artist rows. It is not independent
   proof that all applicable Artist/Album evidence was ranked.
2. **The Unprocessed count may be wrong despite correct name CSS.**
   WebGUI `renderMediaStatusChips()` populates its map using the
   categorized `albumCardState().stateKey`, which returns ranked
   statuses (or Red default), then looks up the `"unprocessed"`
   key. That can yield **zero regardless of real unprocessed
   Albums**. Verify/rewrite that count from the proper inventory
   without adding Unprocessed to name-color or dot classifications.
3. **Artist aggregation must include all relevant evidence**, not
   only whether portrait artwork exists. Inspect associated Album
   static covers, true Artist Art, COMP-track embedding and
   opposing override state without introducing expensive
   whole-library NAS scans at startup.
4. **Album contexts must provide correct Artist evidence.** In
   some V/A OST/Fallback views, an absent currently selected
   Artist object could be interpreted as missing Artist Art;
   verify the correct Album/Artist projection before displaying
   Yellow.
5. **Do not infer override/banner/COMP styling from name color.**
   The approved screenshot preserves the independently
   configured frame/badge, banner and track-row presentation.
6. **Policy revision drift:** Windows embeds the checked-in JSON
   but may load an existing credential-directory policy;
   WebGUI's PostgreSQL row can retain a saved revision.
   A repository edit does not automatically reconcile those
   installed versions. Fail/report conflicts and require
   deliberate owner action before any live change.
7. **Future production promotion:** PR #216 was closed without
   merging into `main`. The protected policy JSON gate and independent
   cross-client validation remain required for any new proposal to
   promote the development tree to `main`.

None of these pending engineering checks is a request for
the owner to restate or reapprove the existing color policy.

## Other `schema/` contracts: what has been validated

| Contract | Results | Still required |
| --- | --- | --- |
| [`comp-report-policy.json`](../schema/comp-report-policy.json) | Valid JSON with selected-Artist precedence, Artist aliases, Recording/MB score bounds, official release-family grouping, batch outcomes and revision pinning; Python and Rust validator source inspected. **Accepted working behavior; left unchanged.** | Test both clients and saved policy revisions, with real COMP fixtures before any new changes |
| [`media.sql`](../schema/media.sql) | DDL inspected: **22 table declarations and 24 index declarations**, plus native/compatibility consumers identified | Execute schema/migrations in a disposable SQLite DB, verify history and authority integrity; not yet an execution pass |
| [`settings-ui.json`](../schema/settings-ui.json) | Valid JSON: **8 sections, 65 unique field paths**, no inverted numeric bounds found | Compare every real Windows/WebGUI field/default/range/persistence behavior against Config v5 |

All four contracts remain in `/schema` and are linked through
[POLICY-INDEX.md](POLICY-INDEX.md); they do not need a second copy in
each client.

## PR governance and deployment boundary

- `.github/workflows/validate.yml` has an independent
  **Canonical policy JSON — review gate** for **both** protected
  JSON files. It reports a failure for modifications/renames/deletions,
  directing Codex to `AGENTS.md` and the approved register.
  It does not automatically revert source or grant an override.
- Mandatory Codex Draft PR use and **explicit current-PR-only owner
  exceptions** are active in `AGENTS.md` after
  [merged PR #217](https://github.com/scottia/S-P-L-I-N-E-D/pull/217).
- The guard's status as a **required** branch-ruleset check
  cannot be confirmed by the current GitHub integration.
- [PR #216](https://github.com/scottia/S-P-L-I-N-E-D/pull/216)
  is closed unmerged. Do not promote `dev` to `main` or release Workflow 01
  without independently verified Windows, WebGUI and policy-revision
  acceptance. This documentation update does not alter production media,
  SQLite, PostgreSQL or `main`.

This review uses the current approved register and source evidence.
Older comparison drafts do not establish or replace policy.
