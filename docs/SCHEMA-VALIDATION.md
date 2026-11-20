# Shared schema review — November 19, 2026

**Baseline:** `/dev` commit `0cad7016b5e5c6d5210b5ab68fffb496fe3b54c2`.
**Scope:** checked-in schemas and read-only trace of source consumers.
This is **not** a full production acceptance test. No PostgreSQL server,
Windows build, GHCR deployment, or media-library write was used.

## Static results

| Artifact | Checks performed | Result |
| --- | --- | --- |
| `schema/media-state-classification.json` | JSON syntax; rank arrays, defaults, forbidden names, status-dot order, current reader/validator behavior | Syntactically valid; rank and dot arrays largely agree with owner order; **implementation and semantic discrepancies confirmed** |
| `schema/comp-report-policy.json` | JSON syntax; Artist recovery parameters; result grouping, completion and coordination keys versus Python/Rust validator requirements | Valid JSON with required fields and in-range values; **functional correctness unverified** |
| `schema/media.sql` | Read DDL structure; table/index enumeration; references from Windows coordinator and WebGUI compatibility code | 22 `CREATE TABLE` and 24 `CREATE INDEX` declarations identified; **DDL execution, migration compatibility and schema ownership NOT TESTED** |
| `schema/settings-ui.json` | JSON syntax; section/field enumeration; duplicate field names; numeric min/max consistency | 8 sections and 65 top-level field entries; no duplicate paths or reversed numeric bounds observed; **actual client settings/default/range parity NOT TESTED** |

The three JSON documents parsed successfully. Static inspection alone does
not establish database correctness or validate actual rendered UI behavior.

## Confirmed media-state divergences and gaps

1. **White names despite policy:** `build/WEBGui/webgui/static/app.css`
   contains `.library-row[data-media-status=unprocessed] .row-copy strong`
   forcing `color:var(--text)`, plus a light-theme override. This bypasses
   the Artist/Album name-color policy. WebGUI also has older base `.library-row
   strong` color declarations, so fix the complete cascade instead of
   removing one rule and assuming success.
2. **Evidence key versus name color:** `build/WEBGui/webgui/static/app.js`
   `mediaStatusKey()` often reuses the catalog status key, and
   `applyStatusPalette()` maps that key through `ui_status_colors`.
   The JSON `artist.ranked_states` and `album.ranked_name_states`
   only help when the correct independent evidence classification reaches
   the name-rendering path. Count keys and name colors must remain distinct.
3. **Validators are not full semantic gates:** WebGUI
   `media_state_policy.py` and Windows `media_state_policy.rs` verify
   required objects/colors, forbidden Artist/Album name colors and a
   `default` entry, but do **not** require the owner's exact ranked
   condition order or complete seven-dot order. A syntactically valid JSON
   can silently reorder the approved policy.
4. **Album red wording:** `album.red_definition` currently says
   `Incomplete album tracks`. Owner policy says `Incomplete/Error`.
   Present source/test assumptions about when errors, timeout and incomplete
   tracks qualify must be checked against that broader intent.
5. **Art and embedded size evidence:** Album card logic in
   `build/WEBGui/webgui/static/app.js` derives missing art,
   embedded-validation and completion flags separately. Verify
   `cover.webp` exclusion, portrait/banner requirements and missing,
   below/above preferred-size embedding on representative fixtures.
   Verify the same independently in Windows.
6. **Override-frame discrepancy:** the media-state seed declares
   `override_presentation.frame_color = dark_grey`; historical approved
   Album-card visual material describes a red frame. Name/dot color does
   not automatically decide frame color. Keep this discrepancy open.
7. **Gold role:** `gold` is explicitly forbidden for Artist names but
   absent from the `colors` palette. Do not fabricate a color code or
   bypassed-Album name/dot rule.
8. **Effective revisions not inspected:** WebGUI reads the revisioned
   `media_state_classification_policy` row and seeds from JSON only when
   absent; Windows can activate local or coordinated mirrors. Changing
   a repository seed alone does not prove an existing installation changed.

## COMP policy: preserve working behavior pending evidence

The checked-in `splined-comp-report-v1` defines selected-Artist
precedence, explicit Artist credit separators, bounded name/alias and
MusicBrainz recovery, Recording-search limits, ordered official release
families, existing-art reuse, outcomes `completed/unresolved/failed`,
batch revision pinning and coordination publication. Python and Rust
implement separate validators; they require core fields and constraints.
The user reports that current COMP processing works well. No speculative
changes to these rules, their matching thresholds or their DB state are
authorized by this review.

## Database and Settings: evidence still required

`media.sql` describes the native SQLite catalog and coordinator tables.
It is neither a complete PostgreSQL schema nor proof of cross-client
synchronization. Before approving it, execute DDL and migration checks in
a disposable SQLite instance, inspect actual catalog code, and reconcile
field ownership and association semantics with owner-approved policy.

`settings-ui.json` describes presentation and validation metadata for
Config v5 settings. For every field, verify the real Rust and Python
configuration parsers, UI exposure, default, allowed range, persistence,
runtime ownership, and platform-specific exception. A syntactically
valid JSON may still misstate existing settings behavior.

## Read-only references

- [WebGUI media-state validator](../build/WEBGui/webgui/media_state_policy.py)
- [Windows media-state validator](../build/Windows/src/media_state_policy.rs)
- [WebGUI Artist/Album presentation](../build/WEBGui/webgui/static/app.js)
- [WebGUI styling](../build/WEBGui/webgui/static/app.css)
- [WebGUI media-state regression tests](../build/WEBGui/webgui/tests/test_media_state_classification_policy.py)
- [WebGUI COMP validator](../build/WEBGui/webgui/comp_report_policy.py)
- [Windows COMP validator](../build/Windows/src/comp_report_policy.rs)
- [Windows schema consumer](../build/Windows/src/catalog_index_coordinator.rs)

**Validation boundary:** No code/runtime/schema changes were made in this
review; the known CSS bypass is recorded as a defect for a focused correction
after updating the approved policy and its enforcement tests. `main`
remains untouched.
