# WebGUI live-write parity — 2026-10-30

> **Historical validation record.** This page records the reviewed state at the
> date and commits named below. Use the [WebGUI guide](browser-workspace-guide.md)
> and current source for supported behavior.

This update was reconciled through release 2.0.11 before promotion through
`dev` to `main`.

## Corrected behavior

- Artist Overview and Fallback cards discover the configured cover stem and
  the compatible `cover.jpg`, `cover.jpeg`, `cover.png`, and `cover.webp`
  names even when an older PostgreSQL/SQLite row has no saved `cover_path`.
  The card image is presentation identity only; COMP tracks still use their
  own embedded artwork and completion ledger.
- Artist list thumbnails resolve only real saved `artist.*` or managed Artist
  Art files. Missing speculative thumbnails no longer generate one HTTP 404
  request per Artist.
- COMP work is track-scoped. **EMBED Thumb** sends only the selected indexed
  track into the candidate/MusicBrainz decision and returns to the same open
  COMP Report. It does not process or display the entire compilation folder.
- Candidate cards select and preview artwork. The final **EMBED Thumb** or
  **WRITE Cover** action stays in Selected Artwork; duplicate per-card save
  buttons and the extra decision panel were removed.
- **CLOSE REPORT** explicitly closes an open COMP Report.
- Network-filesystem tag replacement copies file content first and treats a
  denied metadata-mode copy as non-fatal. Atomic tag replacement, verification,
  and original-art backup remain in force.
- Animated WebP preview is configurable and off by default. Unedited animated
  WebP writes and staged artwork retain the original bytes and `.webp`
  extension.
- WebGUI runtime and failed-job events are written through the configured,
  redacted runtime logger as well as the bounded PostgreSQL audit history.

## Artist identity and appearance hotfix

- Artist identities and aliases are canonicalized into one Media Library row.
  A MusicBrainz identity wins over a legacy name-only duplicate while every
  compatible alias still contributes its indexed appearances.
- Artist Overview now publishes the complete Artist, Featured, Soundtrack,
  Various Artist, and COMP appearance projection on the first selection. The
  projection is persisted in PostgreSQL and reused if Navidrome is temporarily
  unavailable; an explicit catalog refresh invalidates it.
- The COMP summary loads only after the canonical Artist response, removing the
  first-click race that could show primary albums alone until the Artist was
  selected again.
- Catalog indexing and scan-maintenance jobs no longer appear as green Album
  Track Embedding work. The embedding queue renders only album and COMP jobs.
- Versioned artwork handles tolerate an atomic replacement at the same allowed
  path, preventing a successful EMBED Batch stage from reporting an invalid
  artwork handle.
- Existing Artist Art packages can be filtered, fitted, upscaled, and written
  directly. **LAUNCH** is the deliberate artist-image search action and covers
  Local Files, Fanart.tv, Navidrome, Deezer, Last.fm, and Discogs.

## Native Windows alignment

Windows now publishes an indexed fallback folder cover into its Album card
artwork map under the same display-only rule. COMP track previews and writes
remain track-scoped.

## Validation

- WebGUI/Python: 155 tests passed; 15 environment-dependent PostgreSQL tests
  skipped when no test database was configured.
- JavaScript: `app.js` and `operations.js` syntax checks passed.
- This hotfix changes only WebGUI/Python, its PostgreSQL migration, browser
  interface, tests, and documentation. Native Windows source is unchanged.
