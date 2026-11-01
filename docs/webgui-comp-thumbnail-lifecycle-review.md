# WebGUI COMP Thumbnail Lifecycle Review

Status: **implemented for testing** in draft PR #130. The PR remains draft,
open, and unmerged while PC-hosted validation continues.

## Corrected Windows-equivalent contract

The earlier PR #130 implementation exposed the internal thumbnail-preparation
step as a third **EMBED Thumbs** button and renamed the primary action
**EMBED Track**. Owner testing rejected that interpretation.

The Selected Artwork actions now follow the current Windows Rust/Slint UI:

1. **Upscale Preview** applies the Windows Ideal policy to the full-resolution
   current track image or selected replacement, reports the actual applied
   scale, and prepares the configured square thumbnail in memory. Selected
   Artwork then shows that exact thumb with cyan confirmation; audio is still
   unchanged.
2. **EMBED Comp** writes the prepared thumbnail to only the selected COMP track
   and verifies it by reopening the audio.
3. **EMBED Batch** remains the Album Track Embedding action and is disabled in
   every COMP context.

Selecting a track in either an Artist COMP Report or a full Fallback report
loads that track's current embedded artwork into Selected Artwork. The current
thumb can be enhanced, upscaled, and embedded directly; opening MusicBrainz is
not a prerequisite. **LAUNCH COMP** remains a separate, optional workflow for
finding replacement artwork through authority validation, release selection,
and candidate review.

## State and scope rules

The browser distinguishes the explicitly selected report track from a queued
COMP decision. A queued decision owns Selected Artwork only when Artist scope,
Album, and individual track all match. An older MusicBrainz result therefore
cannot replace the current report thumb, supply its upscale source, or hijack
its embed action after the operator changes Artist or track.

For direct work, the browser sends the current track image, current enhancement
settings, upscale intent, and exact Artist/Album/track scope to the appropriate
Artist or full-Fallback endpoint. For queued replacement artwork it sends the
explicitly selected candidate and the same track identity. Neither path can
expand to the compilation Album.

The backend derives the final JPEG with
`output.embedded_thumbnail_size` after the edit/upscale pass, writes only the
target file, reopens the audio, verifies the exact artwork bytes and square
dimensions, and only then records an authoritative completion. A successful
write without validated Recording/Artist authority remains blue and receives
no completion ledger.

The opaque prepared-thumbnail endpoint and digest checks remain internal
write-integrity support invoked by **Upscale Preview**. They are not exposed as
a fourth COMP workflow or as a replacement for the Windows three-button
Selected Artwork contract.

## Implementation surface

| Source | Correction |
|---|---|
| `webgui/static/app.js` | Show the selected track's current thumb, route Upscale Preview from that thumb, show the prepared preference-sized thumb, use **EMBED Comp**, and ignore mismatched queued decisions. |
| `webgui/static/operations.js` | Prepare the internal thumb after a COMP upscale, keep direct and queued writes track-scoped, keep **EMBED Batch** disabled for COMP, and hydrate the exact full-Fallback prompt track. |
| `webgui/services.py` | Report actual applied upscale percentage separately from the configured ceiling; retain preferred-size preparation, verified one-track write, and authority-gated completion. |
| `webgui/tests/test_artist_ui_regressions.mjs` | Cover the action labels, direct preferred-size writes, stale-decision isolation, and queued scope. |
| `webgui/tests/test_webgui_postgres.py` | Exercise current-thumb preview/direct embed plus the independent MusicBrainz candidate route in a rendered browser. |

## Focused acceptance checks

- Artist COMP: select a track with existing art, verify its current thumb and
  dimensions, run Upscale Preview, confirm the actual Ideal transform and the
  separate preference-sized **Embedded Thumb Ready** preview, then **EMBED
  Comp** without opening release results. Read the audio back at that exact
  square size.
- Full Fallback: repeat the same action contract for one explicitly selected
  track and prove sibling tracks are unchanged.
- MusicBrainz replacement: explicitly launch, select one release and one
  candidate, then use **EMBED Comp**. No provider fan-out occurs before release
  selection.
- Scope switching: leave an unrelated queued prompt present, change Artist or
  report track, and prove Selected Artwork and the write target follow the new
  report selection.
- Confirm **EMBED Batch** is disabled in direct and queued COMP review and that
  COMP never enters Album Track Embedding.

Current PC-hosted evidence: 40/40 JavaScript regressions and 212/212
Python/FastAPI tests pass against isolated PostgreSQL; 14/14 CI-scope tests also
pass. The rendered Brave workflow uses generated FLAC media and verifies the
Windows-style Selected Artwork comparison rows: the existing 640 × 640 track
art is **Before**, the prepared configured 300 × 300 thumbnail is **After**,
and the full-resolution **Upscale Preview** remains a separate source row. It
also proves that preparation does not write audio, then reads back the exact
preferred-size result after **EMBED Comp** while the sibling track remains
unchanged.

The JavaScript scale regression separately verifies a 1500 → 1800 transform
is reported as 120% to Ideal rather than as the configured 200% ceiling.
MusicBrainz and artwork-provider replies in browser validation are deterministic
fixtures, not live-provider evidence. Docker/container validation was not
performed.

No Windows source, database migration, release workflow, version, or deployment
configuration is changed. PR #130 must remain draft and unmerged until the
owner explicitly authorizes a later integration step.
