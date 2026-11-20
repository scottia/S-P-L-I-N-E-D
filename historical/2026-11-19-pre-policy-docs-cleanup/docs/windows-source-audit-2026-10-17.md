# Windows source audit — 2026-10-17

> **Historical source audit.** Findings, local state, and test counts below
> describe the named audit date. They do not supersede the current
> [Windows guide](windows-guide.md) or source.

## Scope

This audit covered the Rust/Slint Windows executable end to end: portable startup, SQLite projection, library selection, Artist Overview, fallback-compilation processing, MusicBrainz identity/results, provider discovery, artwork preview/write routing, Navidrome enrichment, image caching, settings/layout persistence, backup/update boundaries, and runtime/debug logging. The Python implementation and shared policy fixtures were run as the cross-runtime authority for compilation behavior.

The existing working tree contained substantial in-progress work before this audit. Unrelated changes were preserved. The installed executable was not used as source authority; it is replaced only after the audited source passes all gates.

## Debug evidence

The recent Windows debug sessions showed three repeatable runtime patterns:

1. A fallback run for one artist remained active while library browsing changed `current_*` state to another artist. This redirected Selected Artwork and made an active embedded preview appear to hang or revert to Artist Art.
2. MusicBrainz hover movement launched duplicate Cover Art Archive downloads for the same result. A single loading slot forgot older in-flight requests, causing preview flicker and unnecessary network work.
3. Multiple lazy thumbnail tasks could miss the Navidrome catalog cache simultaneously and each download the full artist/album catalog before the first task published it.

No panic or memory-corruption signature was present in the inspected sessions. Provider failures were isolated and the runtime continued, but UI state drift made that healthy controller state appear frozen.

## Refactor and fixes

- Separated active fallback transaction authority from library browsing focus. During a run, Artist/Album navigation can change focus without replacing the active album, cover, compilation track, preview, or write target.
- Added an explicit active compilation-track owner so its embedded preview survives harmless selection-set changes until completion or cancellation.
- Prevented fallback browsing from clearing the active transaction's cover/edit paths.
- Guarded both direct and auto-opened single-track fallback paths against starting a second controller run.
- Added a per-result MusicBrainz preview cache and an in-flight set. Hover and click now share one download, completed off-row downloads remain cached, and revisiting a result does not request it again.
- Preserved MusicBrainz row models when their visible data is unchanged so a preview refresh does not destroy the row's `TouchArea`.
- Changed Navidrome artist and album catalogs to asynchronous single-flight cells keyed by server. Concurrent lazy requests now share the same fetch and the in-memory catalog is reused.
- Made failed lazy Navidrome catalog requests retryable instead of permanently recording a failed hydration attempt.
- Kept thumbnail decoding lazy and off the Slint event thread; small images are predecoded and large Selected Artwork buffers remain bounded.
- Reflowed biography expansion into normal layout. Four collapsed lines and the transparent “more” control no longer overlap; expanded text owns its full preferred height and “less” sits below it, pushing later content down.
- Replaced internal fallback `expect` paths with visible unresolved/error events.
- Added elapsed time to sampled UI refresh debug events and always records refreshes slower than 75 ms.

## Regression coverage added

- An active 10,000 Maniacs-style fallback transaction remains authoritative while another artist is browsed.
- MusicBrainz cover previews are single-flight while loading and page-cached after completion.
- The Windows architecture contract checks the flow-based biography layout.

## Verification

- `cargo fmt --all`: passed.
- Windows Rust/Slint tests: 398 passed.
- Root Rust tests and architecture/release contracts: 224 passed.
- Python compilation/parity/TUI tests: 272 passed, 1 intentionally skipped.
- `cargo clippy --manifest-path windows/Cargo.toml --all-targets -- -D warnings`: passed.
- Optimized release build: passed.
- Installed binary SHA-256: `C9450426AFF49466A2CA4905E3425081CD1472414D423907AA037AC856CE07E0` (exact release-artifact match).
- Installed-binary smoke run: remained alive and responsive for the observation window; startup refreshes completed in 7–9 ms with no error, panic, or slow-refresh event in the new debug log.

## Operational invariants

- An album with a Release/Album MBID does not enter the fallback path.
- Fallback albums are missing a Release/Album MBID and operate on explicitly matched artist tracks, never folder `cover.*` artwork.
- Recording ID is queried first. Work expansion is fallback-only and retains recordings belonging to the validated MusicBrainz Artist ID.
- A selected MusicBrainz release supplies the Release ID; editable Advanced IDs contain Recording and Artist IDs.
- Preview and upscale operations are read-only. Writes remain explicit and target either `cover.*`, embedded track artwork, or `artist.*` according to the active transaction mode.

## Residual external risk

Live MusicBrainz, Cover Art Archive, and Navidrome latency cannot be made deterministic. The new single-flight/cache boundaries prevent duplicate work, and debug timing now distinguishes provider delay from UI refresh delay. Runtime smoke testing deliberately avoids WRITE and processing actions because the installed configuration is in live-write mode.
