# Windows application guide

SPLINED for Windows is a native Rust application with a Slint presentation
layer. One Cargo-built `splined.exe` contains the application, workflow, and UI;
there is no managed frontend, application DLL bridge, or helper process.

The current Slint interface is deliberately a functional migration surface.
It restores real application operation and establishes the native architecture
for the separate graphical redesign.

## First run and state

Portable mode uses the directory containing `splined.exe` as its writable state
root. MSIX mode obtains the current package family through Windows and uses:

```text
%LOCALAPPDATA%\Packages\<package-family>\LocalState\SPLINED
```

SPLINED creates these application-owned locations as needed:

```text
data\config.toml       Config v5
data\ui.toml           interface preferences
credentials\*.json    separate provider credentials
splined.db             authoritative media/workflow database (configured path)
_cache\                provider and preview cache
_logs\                 diagnostics
```

The first generated Config v5 is neutral: the music-library and scan-directory
fields are empty. Open **Settings & Credentials**, enter valid library paths,
save the complete document, and use **Reload DB** or **Rebuild Index**.

## Media Library

The left panel is projected from the resident Rust database model. It provides
three views over the same album authority:

- **Artists** shows aggregate artist status. Activating an artist scopes the
  album set and cascades selection to eligible albums.
- **Albums** provides direct and additive album selection.
- **Folders** displays physical album paths while retaining the same album keys
  and selection set.

Text and status filters change only visibility. They never rewrite history,
bypass, timeout, processed state, provider policy, or database identity.
Available states are Unprocessed, Incomplete, Processed, Bypassed,
Partial/Timeout, Artist Complete, and Artist Contains Bypass.

**All**, **Filtered**, and **None** operate on the Rust-owned selection set.
Bypassed albums require the explicit one-run override; active timeouts remain
protected. Incomplete compilation work remains eligible according to the
authoritative database snapshot.

## Running work

Choose an auto-scan scope and launch mode:

- **ALL** queues every currently eligible album.
- **SELECTED** queues only explicitly selected eligible albums.
- **READ** performs review without installing artwork or mutating album files.
- **LIVE WRITE** permits the selected artwork operation to update album files.

Launch is disabled when no valid queue exists. During a run the control state is
owned by `ScanController`, and **STOP** requests cancellation of the active
Rust operation, including an operator-decision wait. Albums are processed
sequentially; a failed album remains recoverable and later queued albums are not
silently marked complete.

## Candidate artwork and sources

The source row is derived from the Config v5 `cover_sources` order. Include,
exclude, and priority actions save back to the same authoritative source list;
there is no UI-only provider order. Local artwork retains its fixed review role.
Provider status and candidate counts are projections of runtime events.

Candidate rows contain real source provenance, dimensions, range class, policy
status, cache path, and recommendation state. Source and range controls filter
the resident result set. They do not repeat provider discovery.

Available decisions include:

- Apply Selected
- Keep Local
- Bypass
- NO Bypass (clear persistent bypass authority)
- MusicBrainz retry/authority review
- Skip Album

In READ mode those decisions remain review-only. In LIVE WRITE mode the same
Rust pipeline is permitted to install validated output.

## MusicBrainz authority

**CURRENT ALBUM** is rendered separately from ordinary search results and is
the authority row. It carries the current recording, artist, and release IDs.
Applying IDs submits an authority decision to the Rust runtime. Ordinary result
selection retains its release-row identity and visited state. After an authority
choice the existing artwork workflow resumes in the same run.

## Artwork editing and upscale

The editing area exposes Picture, Sharpen, Softness, Contrast, Exposure,
Brightness, Gamma, and Color/temperature. Values are application state, not
widget-local state. Saving defaults writes the established Config v5 output
fields atomically.

**Upscale Preview** preserves source resolution and disables the output edit
profile for the candidate decision. **Apply Upscale** enables the configured
profile and existing maximum-upscale enforcement. The real final-artwork code
continues to preserve native artwork above Ideal resolution, apply square/crop
policy, and enforce `upscale_max_percent`.

Existing-cover editing requires LIVE WRITE and a current local cover. It uses
the same validated final-artwork pipeline and safe replacement behavior.

## Settings and credentials

The Config v5 editor validates the complete document before an atomic save.
Application configuration is never duplicated in a Slint-specific format.

Provider credentials remain separate JSON files. The credential surface
supports Navidrome/OpenSubsonic, Fanart.tv, Last.fm, Discogs, and MusicBrainz.
On Windows, credential writes request user-specific filesystem protection and
report when the storage location cannot apply it.

Navidrome remains a supplemental source; `splined.db` is authoritative. A live
connection test calls the OpenSubsonic `ping.view` API. Password mode uses the
standard salted token contract, and API-key mode sends `apiKey` without sending
username/password. A successful status is shown only after a real server
response.

## Backup and restore

The maintenance page exports or restores selected Config v5 settings,
credentials, SQLite, interface preferences, and diagnostics in the established
`.spl` envelope. Protected backups use PBKDF2-HMAC-SHA1, AES-256-CBC, and an
HMAC-SHA256 envelope compatible with existing Windows backups. Restore validates
paths and content and uses atomic replacement. Restart after restoring settings
or SQLite.

Opening an associated `.spl` file, or launching `splined.exe --restore <file>`,
opens the maintenance/restore surface with that archive preselected. SPLINED
does not restore automatically: the operator must review the components and
confirm the restore in the application.

## Updates

Packaged installations query the native Microsoft Store API. Portable builds
read only the pinned official stable GitHub notification manifest and compare
semantic versions. Portable update handling is notification-only; SPLINED does
not download a replacement executable, invoke a cleanup shell, or overwrite its
running binary.

## Troubleshooting

- **No library rows:** verify `library.music_library`, the database path, and
  path mappings in Config v5, then rebuild the index.
- **Launch disabled:** select an eligible album; bypassed albums need a one-run
  override and timeout-protected albums cannot launch.
- **No candidate image:** inspect Activity and provider status, then verify the
  selected source credential and cache path.
- **Navidrome says unavailable:** use Test in Provider Credentials. A status is
  not considered connected until `ping.view` succeeds.
- **Packaged state appears empty:** verify the Store package family LocalState
  directory rather than the package installation directory.

Diagnostics redact secrets and retain a bounded application activity log.
