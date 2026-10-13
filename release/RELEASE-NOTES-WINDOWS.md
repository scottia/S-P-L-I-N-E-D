# Windows migration release notes

## Unreleased — native Rust/Slint architecture migration

- Replaced the production C#/WinForms host, C# entry point, `NativeCore` bridge,
  and runtime DLL pair with one Cargo-built `splined.exe`. Slint is now the only
  Windows presentation layer and Rust owns application state, selection, scan
  control, candidate decisions, MusicBrainz review, editing, and services.
- Added a deliberately basic functional Slint workspace for Artists, Albums,
  Folders, filtering, selection, READ/LIVE WRITE queues, STOP, artwork review,
  MusicBrainz authority, local artwork, editing/upscale, Config v5, credentials,
  selective backup/restore, and channel-aware updates. The graphical canvas
  redesign remains a separate follow-up after this functional migration.
- Kept `splined.db`, Config v5, configured paths, provider ordering, credential
  separation, and album-file safety as the durable authorities. Config v1-v4
  documents found at the established Config v5 location are upgraded atomically
  before startup.
- Preserved Navidrome as a supplemental OpenSubsonic companion for Artist/Album
  enrichment and cached artwork, with password or API-key authentication,
  sanitized failures, explicit capabilities, and proxy bypass for loopback
  servers.
- Updated Portable and Microsoft Store packaging to ship the same single x64
  Rust executable. The portable ZIP contains only `splined.exe` and its README;
  Store packages retain the established identity, file association, LocalState,
  and Store-managed update path.
- Corrected Windows extended-path handling so local `\\?\X:\...` SQLite paths
  are never mistaken for UNC network shares while extended UNC paths continue
  to use the shared-database adapter.

## Microsoft Store 1.0.79 update highlights

The Microsoft Store-certified baseline is `1.0.71`. The next Store submission
for `1.0.79` is cumulative across repository releases `1.0.72` through
`1.0.79`. Its customer-facing Windows highlights are:

- Corrected update detection so release versions, rather than differing Git
  commits, determine whether an update is available. Current installations stay
  quiet and normal dialogs no longer expose commit hashes.
- Refined fallback-compilation review so Album resume, explicit track reopening,
  embedded-track artwork, completed-track state, and current-Album focus remain
  distinct and predictable.
- Restored and stabilized the full artwork-review workspace, including Settings,
  candidate findings, source and resolution filters, MusicBrainz actions,
  Album bypass controls, preview-only upscale behavior, and persistent layout.
- Kept the Artwork preview square with joined horizontal and vertical dividers,
  eliminated dead layout space, and made panel resizing safe at startup and
  across repeated candidate selections.
- Added the spectrum-based artwork controls and unified SPLINED button, glyph,
  typography, and multicolor-frame presentation.
- Made all eight upscale controls a DPI-aware single row that responds to the
  live window, Remote Desktop session, display, and splitter geometry without
  rebuilding or scrolling. Each 20-segment slider now shows its current signed
  value as a bold white number inside the bar.
- Hardened live panel resizing against transient WinForms splitter extents and
  made continued interface exceptions write actionable, redacted details to the
  current run log.

These highlights are derived from the repository range `1.0.71..1.0.79`; no
intermediate GitHub-only build is treated as a separate Microsoft Store
baseline.

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
- Generates cumulative Store Highlights from the last package published by
  Microsoft Store through the target tag, records the exact range and checksum
  in `STORE-SUBMISSION.json`, and refuses to replace an active Store submission.
- Retained the disabled App Installer template only for a possible separately
  signed direct-distribution channel; it is not the Microsoft Store update path.
- Reduced Windows release time by removing the discarded multi-gigabyte compiled
  target cache. CI now reuses dependency downloads while preserving the clean,
  version/commit-bound x64 build and every package validation gate.
