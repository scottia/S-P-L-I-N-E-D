# Windows and WebGUI library-loading parity review

**Review basis:** current `dev` at
`7aa8ac9e3e9de9cf6159b2f28da0c75fbc6b304f`, plus operator-provided Windows
3.0.24 UI diagnostics from installed commit
`c30489d9a3f91c4ad253e9ee5b19b45d3d16547c`.

**Decision boundary:** This is a source and diagnostic review, not an
implementation approval. It changes no runtime behavior, database, schema,
protected policy JSON, renderer, media file or release metadata.

## Executive finding

WebGUI does not request one database record per Album during its initial
Artist load. It obtains the complete Artist **summary projection** from
PostgreSQL in one request and creates the matching browser rows. Its important
performance boundary is that the summary projection does not resolve or decode
physical Artist images. Near-viewport thumbnails are requested afterward,
three at a time. Album and artwork detail is scoped to the selected Artist and
then to the selected Album.

Windows already uses a virtual Slint `ListView`, but its model's `row_data()`
performs synchronous filesystem validation and image loading. Frequent full
UI synchronization and whole-model replacement therefore turn virtual row
requests into repeated NAS metadata reads and image decodes. The Windows
3.0.24 diagnostics show that this presentation path, not SQLite startup, is
the dominant delay.

The appropriate parity change is to adopt WebGUI's **database-summary first,
viewport artwork later, selected-scope detail** architecture in Windows. The
browser implementation should not be copied literally because it retains some
selected-Artist and Fallback filesystem work described below.

## WebGUI loading process

### 1. Paint the workspace before loading the catalog

`initialize()` authenticates, loads settings, calls `renderAll()`, and restores
the layout before awaiting `loadLibrary()`. This lets the browser paint a usable
shell before the Artist request completes. A complete NAS change probe is
deliberately scheduled after the workspace becomes usable.

Source:

- `build/WEBGui/webgui/static/operations.js:99-134`

### 2. Load one database-only Artist summary projection

`loadLibrary()` requests `/api/artists` once for Artists, or `/api/albums` once
for the selected Album library view. The Artist catalog query retrieves and
groups the PostgreSQL rows, computes Album/status summaries and returns a stable
authenticated thumbnail URL. It intentionally does not walk every Artist Art
directory or resolve Navidrome fallback files during this projection.

The Artist response is therefore not paginated per Album and not lazy per
database record. It is a complete, database-backed list whose expensive image
payload is deferred.

Source:

- `build/WEBGui/webgui/static/app.js:2742-2757`
- `build/WEBGui/webgui/server.py:1015-1025`
- `build/WEBGui/webgui/catalog.py:1951-2102`

### 3. Request only near-viewport Artist thumbnails

The browser creates the matching Artist rows, but Artist images are observed
relative to the file-list viewport with a 160-pixel prefetch margin. An
`IntersectionObserver` queues only intersecting images, and the queue permits a
maximum of three concurrent thumbnail requests. Successful and missing results
are retained so ordinary re-renders do not immediately repeat the request.

The per-Artist endpoint resolves saved Art across the normalized Artist's alias
rows only when the browser requests that thumbnail. The response is browser
cacheable for five minutes.

Source:

- `build/WEBGui/webgui/static/app.js:251-317`
- `build/WEBGui/webgui/static/app.js:696-742`
- `build/WEBGui/webgui/server.py:1071-1076`
- `build/WEBGui/webgui/catalog.py:2177-2245`

### 4. Hydrate the selected Artist, not the complete library

After an Artist is selected, `/api/artist/overview` returns that normalized
Artist, the distinct Album summaries associated with the Artist, the Artist's
COMP Album projection and cached metadata seed. The browser preserves the
existing file-list DOM while the request is outstanding, renders the returned
overview, then starts COMP precompilation and provider metadata refresh in the
background.

The selected Artist projection does inspect the selected Artist's applicable
folders and resolves the covers for that Artist's Album cards. It does not do
the same work for every Artist in the library.

Source:

- `build/WEBGui/webgui/static/app.js:2836-2880`
- `build/WEBGui/webgui/server.py:1060-1069`
- `build/WEBGui/webgui/catalog.py:2354-2529`

### 5. Hydrate full Album detail only after Album selection

Selecting an Album requests `/api/album`. Only this detailed call enumerates
the selected Album folder's artwork candidates and builds embedded-artwork
summary information. Normal Artist overview cards do not enumerate candidates
for every Album.

Source:

- `build/WEBGui/webgui/static/app.js:2882-2925`
- `build/WEBGui/webgui/catalog.py:2531-2586`

### WebGUI limitations that should not be copied

- The browser still creates every matching Artist DOM row; only thumbnail
  retrieval is viewport-lazy.
- A completed Artist response calls `renderAll()`, which can rebuild the
  library DOM. Retained thumbnail results avoid most refetches, but this is not
  a general incremental model implementation.
- A selected Artist overview resolves physical cover and Artist Art paths for
  that Artist's Albums.
- Fallback Album index construction currently resolves cover paths eagerly,
  unlike the ordinary Album index's per-Album thumbnail URL.

These bounded exceptions are materially different from performing image work
for the complete library on each Windows refresh.

## Windows loading process

### 1. Load the database and application model before the event loop

Windows loads application state and constructs `AppWindow`, then invokes the
full `sync_ui()` as many as three times while binding workers before `ui.run()`
starts. The window may exist during this period, but the event loop has not yet
begun normal painting and interaction.

Source:

- `build/Windows/src/main.rs:4373-4539`
- `build/Windows/src/main.rs:4689-4722`
- `build/Windows/src/main.rs:5499`

### 2. Perform synchronous image work from virtual row access

Slint uses a virtual `ListView`, but `LibraryUiModel::row_data()` immediately
calls `load_scaled_image()` for the row's artwork. A row request can therefore
perform cache validation, filesystem metadata, file reading and decoding on the
UI thread before returning `UiLibraryRow`.

The Slint delegates also emit `thumbnail-needed`, but that asynchronous signal
does not prevent the earlier synchronous image call in `row_data()`.

Source:

- `build/Windows/src/main.rs:1133-1174`
- `build/Windows/ui/app.slint:781-802`

### 3. Replace the complete library model when its signature changes

`sync_media_filter_ui()` regenerates library rows, hashes all presentation
fields, and replaces `library-rows` whenever the signature differs. The source
already notes that Slint recreates virtual delegates when the model is
replaced. Artwork-path, status, selection, expansion or lock changes can thus
republish the complete model and trigger new `row_data()` requests.

Source:

- `build/Windows/src/main.rs:20431-20506`
- `build/Windows/src/main.rs:22062-22113`

### 4. Route most state changes through one full synchronization path

`refresh_weak()` schedules several preview, COMP and embedded-state tasks and
then invokes full `sync_ui()`. There are more than 200 `refresh_weak()`
references in `main.rs`. `sync_ui()` holds the global `ApplicationState` mutex
while synchronizing the file list, Artist overview, artwork, status, candidates,
settings and operation state.

The same locked function performs `Path::is_file()` and scaled-image requests
for overview and Selected Artwork. Slow filesystem or image work therefore
also delays state publication by background workers.

Source:

- `build/Windows/src/main.rs:20089-20118`
- `build/Windows/src/main.rs:20575-22060`

### 5. Revalidate files even on image-cache hits

Every file-backed `cached_scaled_image()` hit calls `std::fs::metadata()` to
compare length and modification time. A miss then performs another metadata
read, reads the complete file and decodes it. This is especially expensive for
artwork on a NAS.

The rendered cache holds 256 small images, 16 medium images and four large
images. Eviction scans matching hash-map keys and removes arbitrary matching
entries rather than maintaining recency. A library with thousands of rows can
therefore evict and decode the same artwork repeatedly.

Source:

- `build/Windows/src/main.rs:438-441`
- `build/Windows/src/main.rs:25390-25596`

## Direct variance

| Stage | WebGUI | Windows |
| --- | --- | --- |
| First paint | Renders authenticated shell before catalog request | Loads state and performs repeated full UI synchronization before event loop |
| Initial Artist records | One complete PostgreSQL summary projection | Complete in-memory library/model projection |
| Initial physical Artist Art | Stable URLs only; no all-Artist Art walk | Virtual row access calls image loader synchronously |
| Thumbnail scheduling | Near-viewport `IntersectionObserver`, 160 px margin, concurrency 3 | `thumbnail-needed` exists, but synchronous `row_data()` work happens first |
| Album summary scope | Selected Artist, or selected category index | Held in complete application model and reconsidered during full sync |
| Full Album candidate scope | Selected Album only | Full sync can revisit unrelated presentation state |
| State publication | Narrow fetches and browser renders; remote work follows | Broad `refresh_weak()` → locked `sync_ui()` path |
| Image freshness | Versioned URL/browser cache for published handles | `metadata()` on every file-backed cache hit |
| Model update | Browser can update a row directly, though some paths still rebuild DOM | Signature change replaces complete Slint model |

## Windows 3.0.24 diagnostic evidence

The installed 3.0.24 diagnostics show successful catalog loading rather than a
database failure:

- SQLite database load: **1,283 ms** for **3,895 Albums**, shared mode.
- Startup model construction: **662 ms**.
- Complete startup state load: **2,024 ms**.
- Main window creation marker: approximately **4.8 seconds** after process
  start.
- Repeated full UI refreshes: commonly **4.0-6.0 seconds**.
- Artist file-list selection: **5.2-7.9 seconds**.
- Focused Artist/provider refresh: up to **9.24 seconds**, including later
  requests whose provider artwork was already cached.
- One late refresh increased cumulative image cache accesses from roughly
  **500** to **70,000** and file decodes from roughly **150** to **7,500**.
- Session totals reached **71,227** image-memory accesses and **7,633** file
  decodes, with zero COMP inventory errors in the observed reports.

The operator also re-indexed the database before this review without improving
the behavior. That result is consistent with the timings: database/model load
is a small bounded part of the session, while repeated presentation and image
work dominates interaction.

## Shared lifecycle parity mapping

The accepted `schema/lifecycle-policy.json` supplies equivalent outcome
requirements for both clients. It does not require identical internals and it
does not transfer SQLite/PostgreSQL ownership. The WebGUI-specific
`WEB-LIFE-001..009` remain refinements for browser/Python/PostgreSQL behavior,
not Windows implementation instructions.

| Shared requirement | Windows evidence and gap | WebGUI comparison evidence and gap |
| --- | --- | --- |
| `LIFE-001` current selection identity | Selection is explicit, but broad refresh and synchronous row work can delay publication after a newer selection. A later implementation must prove stale workers cannot repaint a replaced Artist/Album. | Selection generations and stale-response checks remain the comparison boundary for demonstrated navigation/input races. |
| `LIFE-002` nonblocking progressive hydration | SQLite/model construction is bounded, but repeated pre-event-loop synchronization and synchronous `row_data()` image work violate the desired shell-first, memory-only presentation boundary. | The shell paints before the Artist request and near-viewport thumbnails are bounded; direct selection and initial projection latency remain open. |
| `LIFE-003` bounded background work | Scan/index generation is bounded, while presentation image work and full refresh fan-out are not yet narrowly scheduled or measured. | Thumbnail work is bounded to three; COMP coordination, filter fan-out and index-probe polling remain open. |
| `LIFE-004` durable queue authority | Existing scan/COMP queues and cancellation must be preserved; this audit authorizes no replacement queue or nonce model. | Existing durable job/nonces remain authoritative; stale decision and staged-batch defects remain tracked lifecycle work. |
| `LIFE-005` truthful current status | Global-lock/full-sync latency can delay local status publication; later work must publish narrow progress before remote or artwork work. | Stale embedding completion and status-ownership evidence remains tracked lifecycle work. |
| `LIFE-006` canonical current presentation | Existing media-state policy, Album rows, frames, banners and dots remain unchanged; any row-level publication must retain current evidence. | Focused complete-Artist GREEN→RED behavior and its higher-priority Red/Yellow/Purple controls remain the regression boundary. |
| `LIFE-007` coherent interaction stages | Selected Artist/Album detail should be scoped and asynchronous, but this audit changes no interaction or COMP stage. | Search, COMP discovery, preferred-size and post-selection stage defects remain tracked under the existing COMP policy. |
| `LIFE-008` database lifecycle authority | Windows SQLite remains synchronized-field authority. No proposed loading correction may merge/rewrite database identities or fields. | The transferred earlier-version reminder requires separate PostgreSQL-owned revalidation evidence; protected Windows fields must not be rewritten. |
| `LIFE-009` focused regression evidence | Source diagnostics identify the hot path, but native build and installed timing acceptance remain required for a later behavior change. | Existing focused browser evidence is comparative only; implementation and owner acceptance remain separate. |

This mapping records parity evidence and gaps only. Native lifecycle-policy
loader/protection activation is reviewed separately; its
presence or eventual acceptance does not make the open behaviors pass.

## Approval-gated Windows recommendation

### Phase 1 — match WebGUI's loading boundary

1. **Paint the shell first.** Create and show the Slint shell and enter the
   event loop before catalog hydration. Publish an explicit Library-loading
   state, then install the database summary model from the event loop.
2. **Make `row_data()` pure and memory-only.** Return text, policy state and an
   already-prepared image/placeholder. It must not stat, read or decode a file.
3. **Use visible-row thumbnail requests.** Keep Slint virtualization and make
   `thumbnail-needed` the only trigger for file-list artwork. Load a bounded
   near-viewport queue off the UI thread, analogous to WebGUI's concurrency-3
   queue.
4. **Update one row, not the complete model.** Store the library in a mutable
   `VecModel` or custom model with `ModelNotify`; publish a prepared thumbnail
   with `set_row_data()`/row-changed notification. Do not change the model
   signature merely because decoded pixels became available.
5. **Scope detail to selection.** Load Artist overview cards only for the
   selected normalized Artist and full folder/candidate detail only for the
   selected Album. Preserve ART-001..003 and do not merge database identities.

### Phase 2 — remove repeated NAS and lock contention

6. **Replace per-hit metadata validation with revision invalidation.** Validate
   an artwork path when it is first published and invalidate it after a known
   artwork write, explicit refresh or catalog generation change. Retain a
   bounded negative cache for missing files.
7. **Use a bounded recency cache.** Replace arbitrary hash-map trimming with an
   LRU/clock policy sized for the visible window plus a small prefetch margin.
8. **Snapshot before I/O.** Copy the minimal immutable state needed for a UI
   update while holding the mutex, release it, perform filesystem/image work in
   workers, then publish prepared results on the Slint event loop.
9. **Split refresh domains.** Replace broad refreshes with dirty publication for
   Library, Artist Overview, Selected Artwork, status/progress, candidates and
   settings. UI-001 progress updates must not wait for artwork or remote
   coordination.

### Phase 3 — modularity and binary-size measurement

10. Split the 31,000-line `main.rs` into startup, library model, image cache,
    UI synchronization, Artist Overview and background-job modules. Modules
    improve maintainability; separate stable crates are required to materially
    improve incremental compilation boundaries.
11. Measure the approximately 64.8 MB executable with dependency-size tooling
    before changing release profiles. Evaluate symbol stripping and thin LTO
    separately. Those options can reduce distribution size while increasing
    compile time; they do not correct UI latency.

Do **not** change the software renderer in these phases. It is deliberately
selected because the earlier OpenGL path produced white frames and multi-window
context failures. The measured synchronous I/O/model churn must be removed
before a renderer experiment would be meaningful.

## Required preservation and validation

An implementation PR must preserve:

- `ART-001..003`, `UI-001` and `SAFE-001`;
- `VISUAL-001..004` and `STATE-POLICY-001..004`;
- `AUDIO-INTEGRITY-001..006`;
- all protected policy JSON files without modification;
- all distinct Album rows, paths, credits and MBIDs;
- the existing accepted Slint frames, banners, status dots and COMP row colors.

Before installed-client acceptance, instrument and compare:

- time to first painted shell;
- time to database summary model publication;
- Artist/V/A OST/Fallback switch time;
- Artist and Album selection response time;
- filesystem metadata calls, file reads and decodes per visible-row scroll;
- full-model replacements versus single-row publications;
- time holding `ApplicationState` during UI publication;
- memory and cache-entry bounds after repeated scrolling;
- local operation/progress publication independent of remote coordination.

Native Windows compilation and installed-client testing remain required for a
later behavior PR. This documentation review supplies source evidence and an
implementation boundary only.

## Transferred WebGUI issue: persistent earlier-version reminder

The tracked WebGUI lifecycle evidence reports that WebGUI continues to display the earlier-major
catalog reminder after a successful Full Refresh. This issue is transferred to
this parity review because the reminder is derived from synchronized catalog
ownership, not from WebGUI presentation alone.

### Current evidence

- WebGUI `upgrade_status` counts Artist, Album and track rows whose
  `splined_version` major differs from the running application and presents the
  Full Refresh recommendation while any such rows remain.
- `splined_version` belongs to the Windows synchronized field set for all three
  entity types. Full Refresh therefore protects it on Windows-managed rows,
  just as it protects the other synchronized identity and state fields.
- Full Refresh may revalidate file-derived PostgreSQL data on protected rows,
  but it does not transfer field ownership or rewrite the Windows source
  version. That is required by shared `LIFE-008`, refined for WebGUI by
  `WEB-LIFE-008`, plus `STATE-POLICY-001..004` and `SAFE-001`.
- Existing catalog-generation records establish refresh generations and
  manifests, but they do not retain a durable, per-entity application-version
  assertion that can safely replace Windows' source `splined_version`.

### Correct ownership and synchronization approach

Do not advance `splined_version` on protected Windows-owned rows. The reminder
should instead distinguish source-version provenance from successful WebGUI
revalidation:

1. Unmanaged PostgreSQL rows may continue to use their source
   `splined_version` as revalidation evidence.
2. Windows-managed rows require separate PostgreSQL-owned evidence recording
   the entity identity, completed refresh generation and validated application
   major.
3. That evidence may be published only after the relevant Full Refresh
   generation completes successfully; cancelled, failed, unread or unseen rows
   remain eligible for the reminder.
4. The reminder may clear a protected row only when its current durable marker
   proves successful validation for the running major. It must never infer a
   Windows field rewrite or database ownership transfer.

### Approval boundary and acceptance criteria

No implementation is included in this Draft. The current PostgreSQL schema
does not expose an approved durable marker with this exact ownership contract.
A new marker or schema change therefore requires explicit owner approval before
code or migration work begins.

Any later implementation must demonstrate that:

- Full Refresh never changes protected Windows synchronized fields;
- a successful refresh clears only rows actually validated by that generation;
- failed, cancelled and incomplete refreshes leave the reminder intact;
- subsequent major-version changes require revalidation again; and
- Windows SQLite authority, protected policy JSON, required source validation,
  and Workflows 20–24 release boundaries remain unchanged.
