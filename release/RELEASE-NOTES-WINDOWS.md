# Windows architecture release notes

- Replaced the former split GUI/processing arrangement with one permanent
  `splined.exe`; the authoritative Rust processing implementation now runs
  in-process behind Tauri commands and events.
- Rebuilt Media Library Selection, scan decisions, candidate review, advanced
  artwork controls, MusicBrainz Matches, compilation-track workflow, Settings,
  Credentials, backup/restore, appearance, update, activity, and preview
  surfaces in the Windows frontend.
- Preserved MusicBrainz CURRENT ALBUM as a separate, unfiltered authority row;
  result filtering, deterministic ordering, URLs, indexes, and visited state
  remain independent.
- Preserved compilation resume authority in SQLite. Track focus previews only;
  explicit track checks reopen a track, and normal Album launch resumes at the
  first unfinished track.
- Fallback compilation preview now uses only the current track's embedded front
  artwork and never a folder-level cover file.
- Made `data/config.toml` and `data/ui.toml` the portable Windows authority,
  including CWD-independent relative paths and one-time legacy migration.
- Preserved absolute/UNC paths, the configured `splined.db`, credentials,
  history, caches, logs, and deterministic selective `.spl` restore.
- Replaced custom executable swapping with the official updater package flow.
  The final x64 NSIS artifact is signed and verified with the stable updater key
  before `latest.json` is generated. Invalid or missing signatures fail closed,
  with no hash-only fallback and no Authenticode requirement.
- The portable Windows ZIP now contains only `splined.exe` and
  `README-WINDOWS.txt`; `data\` is created after first-run save or migration.
- Corrected Select Media status colors to Blue/Incomplete,
  Orange/Processed, Red/Bypassed, and Purple/Timeout, while retaining the
  separate Artist aggregate colors.
- Restored Select `[ALL]`, `[FILTERED]`, and `[NONE]`, Artist selection cascade,
  Ctrl additive selection, temporary bypass confirmation, timeout protection,
  and Auto Scan `[ALL]`/`[SELECTED]` queue semantics.
- Bound every Album in a multi-Album run to its own indexed path and SQLite key,
  preventing later results from being written through the focused Album's
  identity.
- Restored `.spl` Windows shell association and startup Restore routing without
  making Registry state an authority for portable Config v5 or UI settings.
- Aligned the Windows Cargo/Tauri version with the root project version. Release
  provisioning now advances the current source major/minor line and validates
  only the three updater-key secrets before creating a release tag.
- Restricted Windows releases to `x86_64-pc-windows-msvc`, including an AMD64
  (`0x8664`) PE-header audit and rejection of x86/ARM release artifacts.
