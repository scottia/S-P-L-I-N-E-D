# Windows 1.0.88 functional-parity audit

This audit compares the supported Rust/Slint Windows application with the
user-visible behavior of the pre-migration Windows application at commit
`39d2f9e9c7e924ad9a843d6e36c9fbe5cdb2c3d1`. The retired C#/WinForms source was
read only as a behavioral specification. It was not restored, compiled, or
used as a runtime host.

The pass deliberately keeps the current functional Slint shell. It does not
implement the separate graphical redesign.

## Parity checklist

| 1.0.88 feature | Rust/Slint before this pass | Disposition | Validation |
| --- | --- | --- | --- |
| Artists, Albums, and Folders backed by the complete SQLite inventory | Present | Preserved | Live Config v5/database: 1,014 Artists, 3,869 Albums, and 3,869 Folders; every model scrolled in the running application. |
| Artist activation scopes album navigation and selection | Basic artist row activation | Restored mature focus behavior | Live artist activation visibly selected the artist and scoped Albums; **All Artists** restored 3,869 Albums. |
| Album activation populates Current Album and local cover | Present | Preserved and live-verified | A real album selection populated artist/title/path and rendered its existing cover. |
| Folder activation uses the same album key and selection authority | Present | Preserved | Folders projected and scrolled 3,869 real paths from the resident model. |
| Show Tracks / Embedded Art compilation navigation | Only focused-compilation tracks | Restored | **Show Tracks / Embedded Art** projects cached compilation tracks across the visible library; direct track reopen remains explicit and clears on completion. |
| Folder Status categories, counts, and semantic colors | One mutually exclusive status | Restored | Independent colored toggles now cover Unprocessed, Partial / Error, Artist Bypass, Artist Complete, Processed, Incomplete, and Bypassed; **All** enables the complete set. |
| Artist Filter, Album Filter, and combined search | One general search string | Restored | Separate artist/album inputs compose with general search and status filtering. |
| Select ALL, FILTERED, and NONE | Generic all/none | Restored mature scope | ALL selects eligible albums for the focused artist (or visible library), FILTERED requires active text/status scope and selects reviewable results, and NONE clears all selections. Live NONE returned 0. |
| Bypass, timeout, incomplete-compilation, and one-run override rules | Core protection existed | Preserved and surfaced | Queue construction, status projection, NO Bypass, and the one-run bypass override remain Rust-owned and unit tested. |
| Auto Scan ALL / SELECTED | Scope label existed | Restored active behavior | Auto Scan now passes the configured ideal-only choice into the in-process controller instead of changing only a label. |
| Sequential run progress, STOP, failure recovery, and album reports | Controller existed; sparse counters | Restored presentation state | Run total/completed/failed and current report text update from controller/runtime events; STOP still cancels in-process waits. |
| Dynamic Config v5 source order and exclusions | Present | Preserved | Include/exclude and left/right priority actions continue to atomically update the single Config v5 authority. |
| Candidate source, type, policy, and resolution-range filters | Source/range only | Restored | Full filter dimensions are generated from real candidate state and can be combined. |
| Include / neutral / disregard filter state | Missing | Restored | Every candidate filter cycles through included, neutral, and disregarded; included and excluded sets persist in `ui.toml`. |
| Front, back, artist, booklet/media, and other image categories | Not projected | Restored | Runtime evidence is categorized without fabricating provider metadata. |
| Recommended, selected, policy, native/projected dimensions, and source state | Partial | Restored | Candidate rows now preserve all of these fields, including local origin and actual upscale eligibility. |
| Candidate comparison | Missing | Restored | A separate resizable comparison window displays selected candidates horizontally and returns the chosen row to active review without writing files. |
| Hover preview | Missing | Restored | Optional hover projects a candidate into Artwork Preview while preserving the actual selected decision. |
| Fallback/refine after no accepted result | Runtime retry only | Restored | Runtime reason and editable fallback artist/album fields are shown; retry submits the existing fallback decision contract. |
| Apply Selected, Keep Local, Skip Album, Bypass, and NO Bypass | Present | Preserved and regrouped | Buttons invoke the existing in-process Rust decisions; READ/LIVE WRITE safety is unchanged. |
| MusicBrainz CURRENT ALBUM authority row | Partial static authority | Restored | Current artist/release/recording authority is separate from result rows and may be edited and validated before Apply IDs. |
| MusicBrainz artist/release-type filters and mature ordering | Missing | Restored | Results filter by artist and release type and sort by normal album, soundtrack, compilation, Various Artists/blank fallback, date/country/title. |
| Result selection distinct from Use Release | Conflated | Restored | Selecting a row updates current/visited state; **Use Release** separately submits the release decision. |
| MusicBrainz retry, history, and return/back | Partial | Restored | Retry availability, visited/current rows, and Back to MB state are retained across the active review. |
| Apply IDs validation | Missing editable path | Restored | Recording, one-or-more artist IDs, and release ID are normalized and UUID-validated before the authority decision is submitted. |
| Candidate editing and existing-cover editing | Basic controls | Preserved and completed | Both paths continue through the same final-artwork validation and safe-write services. |
| Preview versus Apply upscale | Partial | Restored | Mutually exclusive preview/apply state is persisted and included in the real candidate decision. Preview never enables an output write. |
| Custom Upscale comparison/editor | Missing | Restored | Artwork Preview opens a separate resizable Current/Upscaled window; its resolution/ratio overlays are display-only. |
| Picture, Sharpen, Softness, Contrast, Exposure, Brightness, Gamma, Color Temperature | Values existed | Restored full interaction | All eight adjustments have increment, decrement, and reset controls in the workspace and Custom Upscale editor. |
| Five selectable/reorderable primary adjustments | Missing | Restored | Exactly five primary controls are persisted; non-primary promotion replaces the last primary and controls can move left/right. |
| Adaptive defaults | Config-only | Restored | Custom Upscale exposes the established adaptive-default setting without creating a second authority. |
| Native artwork above Ideal, square/crop, and maximum upscale policy | Present in final-artwork core | Preserved | Complete Windows Rust tests cover native-resolution preservation, crop/square behavior, and `upscale_max_percent`. |
| Collapsible Media Selector, candidate filter, editing, and artwork regions | Missing | Restored | Expansion state is persisted in `ui.toml` and reapplied without rebuilding business state. |
| Draggable horizontal and vertical workspace dividers | Fixed temporary Slint widths | Restored | Media, center, artwork, candidate/results, and activity regions resize continuously; Rust clamps and persists useful geometry in `ui.toml`. |
| Window maximize/restore and saved pane proportions | Window size only | Restored | The last normal size, maximized state, pane sizes, and pre-collapse expanded dimensions restore safely across smaller windows and changed displays. |
| System / Light / Dark appearance preference | Missing | Restored | The lightweight Slint palette preference is persisted in `ui.toml`; it does not create a second Config v5 authority. |
| Persisted filters, view, scan choices, hover, preview/apply, and window size | Limited | Restored | Interface preferences round-trip independently of Config v5; Config v5 remains application authority. |
| Backup/restore, `.spl` association, Updates, Settings, credentials, Navidrome | Present | Preserved | Root architecture tests, Windows tests, and native smoke/package checks cover these paths; the parity pass does not replace their authorities. |

## Intentionally retired or deferred

These are explicit decisions, not silent omissions:

- The C#/WinForms executable, `MainForm`, `NativeCore.cs`, `csc.exe`, managed
  theme renderer, application DLL bridge, and worker-host architecture remain
  retired. Restoring any of them would violate the supported Rust/Slint
  architecture.
- The old WinForms theme renderer is not reproduced pixel-for-pixel. The native
  Slint shell now honors a lightweight System/Light/Dark preference; the later
  graphical redesign remains a separate task.
- WinForms `SplitContainer` itself remains retired. Native Slint drag dividers
  provide equivalent resizable-pane behavior, with Rust-owned persistence and
  bounds validation instead of copying legacy pixel coordinates.
- WinForms-specific modal dialogs and control focus quirks are not preserved.
  Equivalent validation, errors, compare, and Custom Upscale workflows use
  native Slint windows and Rust-owned state.

No provider result, rating, count, image, identifier, or metadata is fabricated
to mimic the retired UI.

## Validation evidence

- `cargo fmt --all`
- Windows `cargo check`
- Windows `cargo clippy --all-targets -- -D warnings`
- Windows Rust/Slint library suite: 313 passed
- Root Rust/library/architecture suite: 221 passed
- Python parity suite: 271 passed, 1 expected skip
- Optimized x64 Rust/Slint build
- Native clean-portable smoke and two-file portable allowlist audit
- Unsigned production Store package validation against the existing manifest
- Live application verification against the existing Config v5 and shared
  `splined.db`, using read-only navigation/filter/selection checks

The live verification did not launch a scan or perform a write-mode artwork
operation against the real library.
