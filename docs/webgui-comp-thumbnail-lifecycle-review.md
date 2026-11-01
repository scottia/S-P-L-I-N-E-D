# WebGUI COMP Thumbnail Lifecycle Review

Status: **implemented for testing** in draft PR #130. The review was based on
`main` at `229fde223c323839c8f462b45eb627f33e096a7f`, after PR #129; the owner
approved the bounded implementation described below. The PR remains draft and
unmerged while PC-hosted validation continues.

## Implemented outcome

WebGUI now separates **Upscale Preview**, **EMBED Thumbs**, and **EMBED Track**.
EMBED Thumbs prepares the exact configured square JPEG in the authorized cache,
binds it to the Artist/Album/track and queued decision, displays it in Selected
Artwork with a cyan ready state, and does not write audio. EMBED Track accepts
that opaque preparation identity, verifies its digest and target, writes only
the selected track, reopens the audio, verifies the exact bytes and square
dimensions, and returns a fresh read-back image handle. Skipping EMBED Thumbs
retains the existing write-time preparation behavior.

Queued candidate selection is retained across polling of the same prompt, and
the prepared image remains visible while the write is pending. The persistent
job record stores only the opaque preparation ID, digest, and target—not an
instance-signed URL or unrestricted path. A preparation for another Artist,
Album, or track is rejected.

Implementation also corrected the blocker observed during PC testing: an ID3
TPE1 frame containing multiple performers was previously stringified with a
NUL separator, which PostgreSQL JSONB cannot store. Tag decoding now preserves
each performer as a semicolon-delimited credit before job progress or identity
matching, consistent with the Windows multi-credit interpretation.

## Scope and expected contract

The reported sequence is:

1. **Upscale Preview** correctly displays the full-resolution 1800 x 1800
   result.
2. The configured 1200 x 1200 **EMBED Thumbs** preview and its cyan ready
   indication never appear.
3. **EMBED Track** leaves candidate review and Selected Artwork shows the
   previous embedded thumbnail.

Windows keeps the full-resolution edit and the embedded-only derivative as
separate state. `ApplicationState` owns an
`embedded_thumbnail_preview_path` and an owner key
(`windows/src/application.rs:2332-2336`). `embedded_thumbnail_owner_key`,
`embedded_thumbnail_ready`, and `add_embedded_thumbnail_decision` bind that
preview to the exact phase, track, and edit preview
(`windows/src/main.rs:368-406`).

`prepare_embedded_thumbnail_preview` selects the active candidate or
MusicBrainz/Existing source, applies the current edit/upscale profile, derives
the configured square thumbnail in memory, and publishes it only if the owner
is still current (`windows/src/main.rs:15040-15134`). Selected Artwork then
prefers that thumbnail and reports its actual dimensions and `Embedded Thumb`
type (`windows/src/main.rs:17627-17630`, `17727-17816`). The audio file is not
written by this preview operation. The final COMP pipeline separately prepares
full-resolution artwork and derives the embedded image
(`windows/src/scan_runtime.rs:3619-3678`), while
`prepare_embedded_thumbnail` verifies the exact requested square dimensions
(`windows/src/final_artwork.rs:96-146`). The documented Windows workflow makes
EMBED Thumbs an optional preparation step before EMBED Comp
(`docs/musicbrainz-fallback-workflow.md:24`).

## Findings and root-cause hypotheses

### Confirmed structural gaps

1. **WebGUI has no prepared COMP-thumbnail state.** The browser state contains
   `upscalePreview` and `renderedPreview`, but no thumbnail preview, owner, or
   prepared-write fingerprint (`webgui/static/app.js:56-74`). Selected Artwork
   therefore chooses the rendered 1800-pixel preview, the upscale preview, a
   candidate, or the old embedded track image
   (`webgui/static/app.js:1251-1328`).

2. **There is no EMBED Thumbs action or API.** The shared action row contains
   Upscale Preview, the write button, and EMBED Batch
   (`webgui/static/index.html:67`). COMP rendering relabels the write action to
   EMBED Track and disables EMBED Batch, including during queued review
   (`webgui/static/app.js:1349-1364`; `webgui/static/operations.js:171-205`).
   `/api/preview/upscale` only returns the full-resolution render
   (`webgui/server.py:567-575`).

3. **The 1200 x 1200 image exists only inside the write operation.**
   `Services._prepare_comp_embedded` renders the selected source and immediately
   calls `imaging.prepare_embedded_thumbnail`; it is invoked by direct and
   queued embedding, not by a preview endpoint
   (`webgui/services.py:845-854`, `1019-1035`, `1457-1486`). The WebGUI helper
   already matches Windows' near-square crop/wide-image contain behavior
   (`webgui/imaging.py:139-164`), so the smallest correction should reuse it
   rather than introduce another resize implementation.

4. **Candidate state is cleared before a queued write is known to have
   succeeded.** The browser submits the candidate URL, adjustments, and an
   upscale boolean (`webgui/static/operations.js:107-138`), then
   `submitQueuedDecision` calls `clearQueuedDecision` immediately
   (`webgui/static/operations.js:287-296`, `360-373`). Until the worker finishes
   and `refreshQueuedAlbumAfterSave` reloads the track, Selected Artwork can
   only fall back to the old `compTrack.image` or `fallbackTrack.image`
   (`webgui/static/operations.js:500-554`). A failed worker has already lost the
   visible candidate/preparation state.

5. **A successful write response does not identify the pixels the browser
   should display.** Direct embedding verifies the bytes and dimensions before
   recording completion, but returns dimensions and outcome without a
   read-back image handle or digest (`webgui/services.py:1019-1078`). The later
   track endpoint does create a content-addressed image URL from the current
   embedded bytes (`webgui/catalog.py:1204-1214`, `225-271`). Consequently, a
   successful refresh should produce a different URL when the pixels differ,
   but the UI cannot currently distinguish “verified write, refresh pending,”
   “verified write, image load failed,” and “write failed.”

### Hypotheses to confirm during implementation

- The observed previous thumbnail is most likely the confirmed early UI
  fallback combined with a delayed, failed, or superseded track refresh—not a
  browser reusing the new image URL. The existing content-addressed image path
  makes ordinary cache reuse less likely when the embedded bytes changed.
- A track/Artist change while preview work is in flight can leave an otherwise
  valid result without a Windows-style owner check. Existing request counters
  protect several loads, but they do not bind a prepared thumbnail to the
  candidate, adjustment profile, job nonce, Artist, Album, and track together.
- If the write succeeds but the report refresh fails, PostgreSQL and the audio
  file can be correct while the old in-memory track object remains visible.
  Network and image-load assertions are needed to distinguish this from a
  failed audio write.

## Approved implementation

No database migration is required.

### Backend preparation and write contract

1. Factor the existing `_prepare_comp_embedded` result into a reusable,
   read-only COMP thumbnail preparation service. Add Artist-scoped and
   full-Fallback preview routes with the same Album/track membership validation
   as their embed routes.
2. Store the already encoded thumbnail as a content-addressed snapshot under
   the existing authorized cache. Return a signed presentation URL plus width,
   height, byte count, SHA-256 digest, and an opaque preparation identifier.
   Do not expose a filesystem path and do not persist a signed URL, because the
   signing key is instance-specific.
3. Bind the preparation identifier to an owner fingerprint containing scope,
   Artist when applicable, Album, track, candidate/source digest, adjustments,
   upscale intent, and queued job/decision nonce when applicable.
4. On EMBED Track, resolve the prepared snapshot by identifier and owner,
   verify its digest and configured square dimensions, write only the selected
   track, reopen it, and compare the embedded bytes. Return the read-back digest
   and a fresh image handle. Reject an owner mismatch or changed preparation
   rather than silently regenerating different pixels.
5. Preserve the current Windows-compatible fallback: if the owner chooses not
   to press EMBED Thumbs, EMBED Track may derive the thumbnail once during the
   verified write. Whether the explicit preview should instead be mandatory is
   an owner-review question below.
6. For queued work, retain the preparation identifier, digest, and recipe in
   existing `web_jobs.progress` JSON before the decision is consumed. This
   avoids a schema change and lets a failure/retry explain which prepared image
   was being written. A persistent cache snapshot, not its signed URL, is the
   restart-safe authority.

### Browser state and controls

Use the existing third action position as **EMBED Thumbs** in COMP context and
leave it as **EMBED Batch** for regular Artist Albums.

Add one owned `compThumbnailPreview` state and a small `pendingCompWrite` state.
The owner key must cover the active scope, Artist, Album, track, candidate,
adjustments, upscale state, and queued nonce. Candidate, MusicBrainz release,
track, Artist, or enhancement changes invalidate a prepared thumbnail that no
longer has the same owner.

Selected Artwork should prefer the prepared thumbnail over the 1800-pixel
upscale result, show its actual 1200 x 1200 dimensions and byte count, and use a
cyan **Embedded Thumb Ready · no audio written** indication. The preparation
action must not update the completion ledger or audio modification time.

When EMBED Track begins, keep the prepared image and target identity visible
with a writing/verification state. Clear the candidate and prepared state only
after readback succeeds, the refreshed track identity still matches, and the
new image has loaded. On failure, retain the prepared preview and show the
write/readback error. If audio verification succeeds but the browser image
cannot refresh, report **write verified; preview refresh failed** and never
present the previous thumbnail as the successful result.

### Proposed state transitions

| State | Selected Artwork | Allowed next action | Invalidation |
|---|---|---|---|
| Candidate selected | Original candidate | Upscale Preview or EMBED Thumbs | Candidate/release/track/Artist change |
| Full-resolution preview | Edited/upscaled source, e.g. 1800 x 1800 | EMBED Thumbs | Candidate, adjustment, track, Artist, or job nonce change |
| Thumbnail ready | Exact configured square, e.g. 1200 x 1200; cyan ready status | EMBED Track | Owner fingerprint change |
| Write pending | Same prepared thumbnail with target track | Wait/cancel only | No unrelated navigation may retarget it |
| Verified complete | Fresh read-back thumbnail on the same report track | Continue/close report | Normal report navigation |
| Write or readback failed | Prepared thumbnail retained; explicit red error | Retry or return to candidates | Explicit selection change |
| Preview refresh failed after verified write | Prepared/read-back digest retained; refresh warning | Retry preview refresh | Never downgrade to “write failed” |

## Implementation surface

| File/function | Implemented change |
|---|---|
| `webgui/services.py` — `preview`, `_prepare_comp_embedded`, `embed_comp_track`, `run_comp.prepare_selected_artwork` | Add read-only thumbnail preparation, content-addressed snapshot ownership, prepared-digest validation, and a verified read-back image/digest result. |
| `webgui/server.py` — preview and COMP routes | Add full-Fallback and Artist-scoped thumbnail-preview routes without weakening existing scope validation. |
| `webgui/backend/splined_scan.py` — `_manual_choose_candidate`, selected-artwork write branch | Carry the opaque preparation identifier/digest and reject stale prepared decisions before queued embedding. |
| `webgui/jobs.py` — `JobAdapter.read`/job progress handling | Retain the selected prepared recipe/digest in existing job progress JSON before the waiting decision is consumed; no schema change. |
| `webgui/static/app.js` — `state`, `selectCandidate`, `selectedImage`, `renderReview`, track-selection and Upscale handlers | Add owned thumbnail state, invalidation, cyan metadata, dynamic EMBED Thumbs control, and image-load-aware refresh state. |
| `webgui/static/operations.js` — `writeArtwork`, `clearQueuedDecision`, `submitQueuedDecision`, `refreshQueuedAlbumAfterSave`, `pollJobs` | Submit the prepared identifier/digest, preserve queued candidate state through verification, and refresh only the exact target track. |
| `webgui/static/index.html`, `webgui/static/app.css` | Reuse the third action as context-sensitive EMBED Thumbs/EMBED Batch and style the cyan ready/pending confirmation accessibly. |
| `webgui/tests/test_artist_ui_regressions.mjs` | Cover action separation, owner invalidation, candidate retention, and success/failure refresh states. |
| `webgui/tests/test_comp_job_scope.py`, `test_splined_manual_compilation_scan.py`, `test_webgui_postgres.py` | Cover scope/digest rejection, queued payload preservation, generated audio readback, PostgreSQL lifecycle, and rendered Brave acceptance. |

`webgui/imaging.py` and `webgui/catalog.py` are supporting references, not
expected behavior changes: the resize helper and content-addressed authorized
image publication already provide the needed primitives. Documentation of
user-visible behavior should be updated only with the implemented and executed
result.

## Focused acceptance tests

### PC-hosted Brave/FastAPI

- Artist COMP and full-Fallback paths: select a deterministic candidate,
  confirm Upscale Preview reports 1800 x 1800, press EMBED Thumbs, and confirm
  Selected Artwork reports 1200 x 1200 with the cyan no-write indication.
- Before EMBED Track, assert the target audio modification time, embedded-art
  digest, and completion ledger are unchanged.
- Change an enhancement, candidate, track, Artist, MusicBrainz edition, and job
  nonce in separate cases; each must invalidate only the stale prepared
  thumbnail and must never retarget it.
- Submit queued and direct writes. Keep the prepared image visible while the
  write is pending; after success require a different content digest/URL and a
  completed image load for the exact track before clearing transient state.
- Simulate an image-refresh failure after verified write and a backend
  write/readback failure. The messages and retained state must distinguish the
  two cases.

### Generated MP3 and FLAC

- Begin with a known previous embedded image. Prepare the new thumbnail without
  changing the file, then embed it through the actual direct and queued WebGUI
  paths.
- Reopen each audio file and require byte-for-byte equality with the prepared
  snapshot, exact configured square dimensions, the intended path, preserved
  Recording/Artist and unrelated metadata, and unchanged sibling files.
- Force digest mismatch and failed readback. Neither may create a completion
  row or green report state.

### Isolated PostgreSQL and UI regressions

- Verify prepared job identity/digest survives normal polling and a worker
  pause/retry through existing JSON state without a migration or signed-URL
  persistence.
- Verify one Artist's prepared state cannot be used for another Artist, Album,
  track, or full-Fallback scope.
- Verify the completion ledger is written only after audio readback and the UI
  refreshes the same Artist-scoped or full-Fallback report.
- Retain all PR #129 scope, MusicBrainz, candidate, Album queue, authority, and
  embedding-integrity regressions.

## Risks and dependencies

- Prepared snapshots need bounded cleanup, but cleanup must not remove a
  snapshot referenced by active queued work.
- Signed image URLs are presentation handles only; persisting one would repeat
  the restart problem already addressed for staged Album artwork.
- Re-rendering JPEG at write time can diverge from the pixels reviewed. The
  prepared encoded snapshot and digest, not an assumed deterministic rerender,
  should be authoritative.
- The same controls serve Artist COMP, full Fallback, regular Albums, and queued
  review. Context-sensitive labels and enablement must not make COMP eligible
  for Album Track Embedding.
- A verified audio write and a failed browser refresh are different outcomes.
  Completion integrity must remain backend-authoritative while the UI reports
  preview refresh separately.

## Approved decisions and validation checkpoint

- EMBED Thumbs remains optional, matching Windows; EMBED Track can still
  prepare the configured thumbnail during a verified write.
- The ready state is cyan and reports **Embedded Thumb Ready**, the actual
  configured dimensions, and **no audio written**.
- A prepared preview remains available only while its complete browser owner
  key still matches. Candidate, enhancement, Artist, Album, track, or queued
  prompt changes invalidate it.
- This bounded change retains the existing WebGUI COMP JPEG policy.

Executed evidence includes generated FLAC preparation/write/readback tests,
wrong-track digest rejection, generated multi-value MP3/ID3 parsing, isolated
PostgreSQL job-progress storage, JavaScript state/payload regressions, and the
PC-hosted FastAPI workflow rendered in Brave for both Artist-scoped and full
Fallback COMP work. External artwork and MusicBrainz replies in that browser
test are deterministic fixtures; they are not live-provider evidence. The
final local checkpoint passed 211/211 Python/FastAPI/PostgreSQL tests, 34/34
direct JavaScript regressions, both JavaScript syntax checks, and 14/14
GitHub-validation-scope tests. Docker/container validation was not performed;
the owner requested the existing PC-hosted service rather than a temporary
deployment. Brave screenshots were captured at normal and maximized viewports
for the cyan thumbnail-ready state.

PR #130 remains draft and must not merge, deploy, release, modify Windows, or
change `dev` at this checkpoint.
