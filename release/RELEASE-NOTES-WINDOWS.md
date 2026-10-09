# Windows migration release notes

- Restored the accepted 1.0.60 WinForms product experience, including mature
  Media Selection, Settings, candidate/artwork editing, MusicBrainz, compilation,
  backup/restore, theme, layout, activity, and help surfaces.
- Retained newer correctness fixes: status colors, complete Select modes and
  protections, per-Album SQLite identity in batch runs, compilation resume versus
  explicit targeting, embedded-only fallback preview, and corrected MusicBrainz
  CURRENT ALBUM authority/filter/order/visited behavior.
- Corrected fallback compilation launch across the WinForms/native boundary:
  blank track targets now mean normal Album resume, while only an explicitly
  checked track enables intentional single-track reopening.
- Kept the active compilation decision visible when another track is clicked
  for preview, updated completed track indicators immediately, and serialized
  embedded-preview caching to prevent concurrent access-denied failures.
- Corrected explicit compilation-track reopening so existing embedded artwork
  remains a local comparison choice without suppressing MusicBrainz release
  selection or higher-resolution provider results.
- Restored fallback Album launch from Show Tracks: clicking the Album row now
  selects normal SQLite resume and enables LAUNCH, while clicking a track still
  changes preview only and checking a track remains explicit one-track reopen.
- Moved authoritative Rust processing into the WinForms process through the
  fixed `runtime\splined-core.dll` and a stable UTF-8 C ABI. Requests, callbacks,
  decisions, cancellation, structured errors, and Rust buffer ownership no
  longer use a child process or redirected streams.
- Added GUI/core version and commit pairing so a mismatched executable and DLL
  are rejected at startup.
- Preserved portable `data\config.toml` and `data\ui.toml`, CWD independence,
  one-time Registry migration, absolute/UNC paths, configured SQLite authority,
  credentials, history, cache, logs, and deterministic selective `.spl` restore.
- Added a distinct installed state model: x64 MSIX runs use stable per-user
  package LocalState for Config/UI while leaving configured external resources
  unchanged.
- Removed the rejected desktop frontend/runtime, custom updater, worker
  executable, updater executable, executable/DLL extraction, hidden lifecycle
  helpers, self-reinvocation, and in-app binary replacement.
- Made portable updates notification-only: SPLINED opens the official release
  page and the user manually replaces only program files.
- Restricted Windows output to `x86_64-pc-windows-msvc` and audits both
  `splined.exe` and `runtime\splined-core.dll` for AMD64 PE Machine `0x8664`.
- Changed the portable ZIP allowlist to exactly `SPLINED\splined.exe`,
  `SPLINED\runtime\splined-core.dll`, and `SPLINED\README-WINDOWS.txt`; a fresh
  archive contains no user state.
- Added the production Microsoft Store identity `Psycotix.SPLINED`, publisher
  `CN=FE370EF6-D95D-4A6F-9AAB-2654E6DE00FE`, publisher display name `Psycotix`,
  PFN `Psycotix.SPLINED_8pvn5te36e43t`, and Store ID `9P8G4GMBBVBS`.
- Isolated the unsigned Partner Center package from development identity QA. The
  Store layout is packed and validated without debug identity or development
  signing; a separate temporary layout handles certificate/install/activation/
  uninstall testing.
- Uploads `SPLINED-x64-store-unsigned.msix` only in the
  `splined-windows-store-submission` Actions artifact, not as a normal GitHub
  Release download. Microsoft Store package deployment owns installed updates.
- Retained the disabled App Installer template only for a possible separately
  signed direct-distribution channel; it is not the Microsoft Store update path.
- Reduced Windows release time by removing the discarded multi-gigabyte compiled
  target cache. CI now reuses dependency downloads while preserving the clean,
  version/commit-bound x64 build and every package validation gate.
