# WebGUI live-write parity — 2026-10-30

This update was reconciled onto release 2.0.9 before promotion through `dev`
to `main`.

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

## Native Windows alignment

Windows now publishes an indexed fallback folder cover into its Album card
artwork map under the same display-only rule. COMP track previews and writes
remain track-scoped.

## Validation

- WebGUI/Python: 151 tests passed; 15 environment-dependent PostgreSQL tests
  skipped when no test database was configured.
- JavaScript: `app.js` and `operations.js` syntax checks passed.
- Windows/Rust: the fallback-card identity test passed after compiling the
  current 2.0.9 native source.
