# S:P:L:I:N:E:D 1.0.28

S:P:L:I:N:E:D 1.0.28 is a cross-platform artwork-management release focused
on a unified operator-review workflow, persistent SQLite media authority,
MusicBrainz-assisted matching, curated-compilation track artwork, and a much
more capable native Windows application.

## Highlights

- Added a persistent SQLite media index for fast library startup and stable
  Artist/Album navigation.
- Unified normal Albums and curated compilations around one MusicBrainz and
  artwork-candidate review workflow.
- Added MusicBrainz and Amazon artwork policies alongside the existing
  configured providers.
- Added per-track embedded-artwork replacement for eligible curated
  compilations without changing folder-level `cover.*`.
- Added a native Windows v4 workspace with internal settings, portable backup,
  embedded artwork preview, integrated MusicBrainz Matches, and update checks.
- Preserved deterministic source policy, configured Range Type, output rules,
  shared-SQLite compatibility, and credential privacy.

## Python and Ratatui

- Added a resident SQLite-backed Artist and Album picker with fast warm loads,
  resumable index checkpoints, folder-based Artist ownership, status colors,
  filters, mouse selection, and contextual `[?]` help.
- Preserved the fast representative-track index while retaining the track
  identities required by manual compilation matching.
- Added Ctrl+Click multi-Album selection and additive multi-Artist selection.
- Added Select `[ALL]`, `[NONE]`, and `[FILTERED]`, plus independent Auto Scan
  `[ALL]` and `[SELECTED]` scope.
- Kept Select `[ALL]` separate from Auto Scan: bulk selection remains an
  operator-reviewed queue and never silently enables unattended acceptance.
- Made ordinary Python Select launches wait for an explicit candidate choice;
  only an explicitly selected Auto Scan mode can accept an Ideal candidate
  unattended.
- Fixed the Python Auto Scan handoff so `[ALL]`/`[SELECTED]` scope survives the
  required READ/LIVE WRITE choice. Auto Scan now accepts only a
  policy-qualified `Ideal` candidate and returns every non-Ideal result to
  operator review.
- Documented that `Ideal` measures configured resolution, geometry, and source
  policy—not semantic cover accuracy—so manual review remains the safer choice
  for providers that may return merchandise, inserts, discs, or photographs.
- Changed multi-Album runs to be fully lazy. Python now resolves authority,
  discovers candidates, waits for the current Album decision, and completes
  that Album before reading tags or querying MusicBrainz for the next Album.
- Removed the former batch-wide MusicBrainz authority sweep and fallback-first
  reordering that delayed the first candidate screen on large selections.
- Kept MusicBrainz source-result caching local to the active Album so pressing
  `M`, comparing editions, and returning to an inspected release do not repeat
  provider work.
- Added detailed per-Album run reports, unresolved-track reporting, incomplete
  compilation status, and resumable compilation progress.
- Hardened MusicBrainz result scrolling so list boundaries cannot close the
  application or accidentally submit a release.

## MusicBrainz matching

- Added integrated MusicBrainz Matches for ordinary Albums and curated
  compilations.
- Grouped results by newest-to-oldest decade and populated release type.
- Displayed Artist, country, date, release type, release title, inspected
  resolution, and preview/authority URLs.
- Used release-group front artwork for lightweight preview when a release-group
  MBID is available, with exact-release fallback.
- Marked the current inspected release green and earlier inspected releases
  blue; both restore cached source candidates and diagnostics.
- Added session-only Artist, Release, and Recording MBID correction with UUID
  and MusicBrainz relationship validation.
- Added exact Apple Music/iTunes relationship discovery before ordinary search
  where MusicBrainz provides the relationship.
- MusicBrainz work continues to use the configured JSON credential options,
  including request delay, recording timeout, and retry maximum. These values
  are never hard-coded or invented by the runtime.

## Curated compilations

Albums tagged `compilation=1` with no Album/Release MBID use an explicit
per-track workflow:

- SPLINED checks exact Recording-ID and Artist-ID evidence in SQLite first.
- A local Artist/Album match can reuse indexed local artwork as a `[LOCAL]`
  candidate without a MusicBrainz request.
- MusicBrainz is queried only after a local miss and only within configured
  delay, timeout, and retry limits.
- Automatic authority prefers Album, then Soundtrack, then Compilation.
- The operator can inspect broader Artist/Track release results and preview
  artwork URLs before approval.
- Live Write replaces only the approved track's embedded front artwork.
- Folder `cover.*` is never silently created, replaced, or removed for this
  compilation branch.
- Progress is committed per track; leaving early marks the Album Incomplete and
  the next run resumes verified remaining work.
- SPLINED does not assign an official Album/Release MBID to the user's curated
  compilation.

## Artwork sources and policy

Configurable artwork policy now covers:

- Local folder and embedded artwork
- iTunes / Apple Music
- Fanart.tv
- Last.fm
- MusicBrainz / Cover Art Archive
- Cover Art Archive
- Amazon Store
- Discogs
- Deezer

Provider candidates retain configured priority, minimum Range Type,
below-minimum fallback policy, primary/front-image policy, geometry, format,
and deterministic ranking. Amazon thumbnail transforms are normalized to the
original media image. Candidate and authority URLs can be previewed before the
operator approves an image.

## Native Windows application

- Added a native Windows v4 workspace with independently scrollable Media
  Selection, Scan Activity, and Artwork Candidates panels.
- Added Balanced, Wider Select Media, Wider Decisions, Stacked, and draggable
  custom panel layouts.
- Added a multicolor `S:P:L:I:N:E:D` wordmark, spectrum panel borders, a warmer
  Light theme, and solid-green selected/red-outline unselected controls.
- Added Album Information that follows the active processing Album or manually
  focused Album, uses the Album status color, and clears stale context.
- Added a square Artwork pane showing indexed `cover.*`, year, track count,
  resolution, file counts, and path without rerunning providers.
- Added `View > Show Artwork` and routed enabled candidate/URL hover into the
  shared Artwork pane.
- Integrated MusicBrainz Matches into the main workspace so match URLs preview
  in the Artwork pane instead of opening a disconnected modal workflow.
- Ordered candidate cards with `[LOCAL]` first, the recommended candidate
  second, and remaining candidates by descending resolution.
- Added candidate-only purple, green, and clear glass surfaces with responsive
  square thumbnails; Artwork, MusicBrainz, and Compare remain un-tinted.
- Added Python-equivalent Auto Scan behavior: `[SELECTED]` queues explicit
  selections, while `[ALL]` queues every Unprocessed Album plus explicit
  selections.
- Made Windows Auto Scan opt-in and aligned it with Python: unattended
  processing accepts only a policy-qualified `Ideal` candidate; otherwise the
  Album pauses for operator review. Select modes only build the reviewed queue.
- Separated Incomplete Album filtering from Artist Bypass filtering and added
  live selection and Folder Status counts.
- Kept Windows Album processing lazy: each selected Album runs to its operator
  decision and completion before the next Album process starts.

## Windows settings, portability, and backup

- Moved Windows Config v5 and interface state into internal per-user settings.
- Removed the requirement for external `config.toml`, `ui.toml`, and
  `config.location` files in the portable Windows application.
- Setup no longer creates `config`, `credentials`, `_cache`, `_logs`, or
  `docker_builds` beside the executable.
- Added separate required paths for the SQL Database Directory, Temporary Run
  Cache, Credentials Directory, and Logs Directory, with mapped-drive and UNC
  support.
- Added selective Backup Import and Export using the `.spl` extension, optional
  password protection, integrity checking, and individual configuration,
  interface, credential, database, and diagnostic sections.
- Added verified stable/dev Windows update checks, SHA-256 validation,
  restart-based installation, and cleanup of successful update backups.

## SQLite authority and cross-platform behavior

- SQLite is the sole runtime authority for Processed, Bypassed, Timeout,
  Incomplete, and compilation-progress state.
- Merely finding a `cover.*` file no longer marks an Album Processed; only a
  successful SPLINED outcome creates Processed authority.
- Shared databases retain rollback journal mode, FULL synchronization, bounded
  busy handling, and Python/Windows schema compatibility.
- Python/Docker remains the shared inventory authority while Windows can safely
  read and update compatible runtime status and compilation ledgers.
- Added canonical POSIX-to-Windows path mapping, UNC support, Unicode
  normalization, and legacy-decoded SMB directory resolution.
- Retired JSON history as runtime authority; status and selection history now
  remain in SQLite.

## Performance and diagnostics

- Added compact snapshot, provider, download, candidate, post-cover, and Album
  completion timing diagnostics.
- Avoided redundant post-cover filesystem and SQLite work when current runtime
  data is already authoritative.
- Overlapped independent provider discovery where safe without changing source
  priority or final ranking.
- Added per-provider success, zero-result, reference/candidate count, elapsed
  time, and sanitized error reporting.
- Kept temporary candidate images separate from `splined.db` and removed them
  after a completed run.
- Preserved fast SQLite warm loads and avoided whole-library filesystem scans
  during ordinary picker navigation.

## Security and reliability

- Credentials, OAuth tokens, authorization headers, passwords, private values,
  and credential-file contents are never written to logs.
- Configuration failures return a real failure instead of masquerading as an
  incompatible or empty SQLite snapshot.
- Artwork downloads are validated by content and dimensions before selection
  or installation.
- Safe replacement preserves existing artwork when validation or final writing
  fails.
- Read mode never installs artwork or commits false completion authority.

## Upgrade notes

- Review Config v5 and provider policies after upgrading, especially the new
  MusicBrainz and Amazon entries.
- Python/Docker users should rebuild or refresh the media index after changing
  library topology or tags.
- Shared Windows users should point the SQL Database Directory at the same
  physical `splined.db`, enable shared SQLite on both runtimes, and keep the
  Temporary Run Cache local when practical.
- Windows settings migrate into the internal settings store; persistent data
  directories remain wherever the operator configured them.
