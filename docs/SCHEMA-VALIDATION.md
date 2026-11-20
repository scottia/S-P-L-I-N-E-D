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

**Protected JSON labels versus approved meaning:** The current canonical seed still uses the narrower display text `Missing Artist Art` for Artist Yellow and `Incomplete album tracks` for Album Red. The approved meaning is missing Artist **or static Album** art and independent **Incomplete/Error**, respectively. Rank/color validation is correct; a label-only JSON revision would still require an independently authorized protected-policy PR, not a docs-only edit.

## Check evidence and limitations

The validated application code was introduced through the owner-reviewed reconstruction/correction series (including PR #233/#234 for name rendering, #235 for WebP/Unprocessed evidence, #236 for Artist aggregation/counts, #237 for embedded-size reporting, and #238 for stored rank/dot validation). Their final-head GitHub Actions checks were reported successful. They **do not** prove a new native Windows Rust/Slint compilation or installed UI acceptance; the ordinary Windows PR job is advisory.

WebGUI PR tests exercise Python, browser JavaScript, Docker image startup and disposable PostgreSQL. Full production music-library tests, live network shares, installed policy mirrors, source/DB revision drift and real catalog recovery require separate operator-authorized validation. **No live policy or database was changed by this documentation audit.**

## Governance and remaining checks

- The single checked-in contracts are `schema/media-state-classification.json`, `schema/comp-report-policy.json`, `schema/media.sql`, `schema/settings-ui.json`; no runtime-specific duplicate policies.
- `AGENTS.md` and [POLICY-REGISTER.md](POLICY-REGISTER.md) require independent protection of both behavioral JSONs. The current `validate.yml` **lacks** the previously documented `policy-review` job and checker script. Do not describe it as an existing passing enforcement gate; fix that gap only through an independently reviewed authorization.
- Source revisions and PostgreSQL/Windows *saved* policy revisions are different objects. A safe installed comparison reads policies without invoking seeding/updating loader functions. Do not silently replace a saved policy.
- Neither the retired root-native Linux instructions nor old PR acceptance reports belong in active `docs/`. Current supported source roots remain `build/Windows/` and `build/WEBGui/`, with release automation in `release/`.
- Do not request re-approval of settled Artist/Album colors. Record implementation drift, missing runtime evidence and owner decisions in the appropriate future PR.
