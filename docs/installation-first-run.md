# Installation and First Run

Choose the runtime that matches your system. All supported implementations use
Config v5; application release identities remain independent.

## Windows portable

1. Download the Windows archive from the
   [latest release](https://github.com/scottia/S-P-L-I-N-E-D/releases/latest).
2. Extract the complete archive into its final application directory.
3. Keep the extracted `runtime` folder with `splined.exe`, then run
   `splined.exe`.
4. Choose the music library and the required SQL database, temporary run cache,
   log, and credential directories, or open Advanced Settings.
5. Select **Save and Continue**.
6. Configure provider credentials through **File > Credentials...** as needed.
7. Start in Read mode against a small selection.

The release ZIP contains no `data` directory, so a fresh extraction performs
true first-run setup. Saving creates portable `data\config.toml` and
`data\ui.toml` beside `splined.exe`. Moving or copying the complete application
folder carries those settings; deleting the folder removes them. The selected
runtime directories are created only after settings are saved. No executable
rename or setup launcher is required.

When both portable files are initially absent, a compatible build copies the
former per-path HKCU `ConfigV5`/`UiV4` values once, records migration, and leaves
the old values intact for manual cleanup. They are not runtime authority after
migration and cannot reconstruct deleted portable files.

The first-run SQL database, temporary run cache, log, and credential fields default beneath
`%LOCALAPPDATA%\SPLINED` for the current Windows user, not beside the portable
executable and not in machine-wide `%PROGRAMDATA%`. These locations remain
editable before saving; selecting UNC paths does not change the portable
program files. Existing saved locations are never migrated automatically.

The permanent portable layout has two fixed executable artifacts with stable
roles: `splined.exe` is the only user-facing root executable and
`runtime\splined-core.exe` is its shipped Rust worker. The worker is not
embedded in or extracted from the GUI. Normal startup and scanning do not
create, extract, rename, replace, or delete executable files. GUI-to-core
Config v5 handoff is memory-only and creates no runtime TOML.

Use **File > Backup > Export Backup...** for a selective `.spl` backup. Portable
Config v5, interface state, credential JSON, SQLite, and diagnostics are
independent options. A password is optional. Double-clicking a registered
`.spl` file opens SPLINED's restore dialog; no data is restored until the user
chooses the sections and confirms.

### Windows upgrade

SPLINED checks official, non-prerelease GitHub release metadata after the
library opens and through **Help > Check for Update...**. When a newer release
is available, choose automatic update, open the official release page, or
install later. Automatic update occurs only after explicit approval.

The GUI downloads the official portable ZIP and consistently named temporary
`splined-update.exe`, validates their sizes and SHA-256 digests, and then
closes. The visible updater waits for the exact GUI/core process IDs, stages
and verifies the new `splined.exe` and `runtime\splined-core.exe`, performs
transactional replacement with recovery copies and rollback, verifies the
installed pair, and restarts the GUI. The restarted GUI confirms its commit and
both file hashes before removing the finished updater and staging files. The
helper is a separate release asset; it is never embedded in or extracted from
`splined.exe`. No CMD or PowerShell update machinery is used.

The added verification fields remain backward-compatible with the existing
schema 2 notification manifest. Notification-only builds continue to detect
the release and open its page; after one manual upgrade to this automatic-update
architecture, later compatible releases can be installed from the prompt. A
schema 2 document is eligible for automatic installation only when its exact
release version/commit, archive and updater URLs, sizes, and SHA-256 digests for
the archive, updater, GUI, and core are all present and validated. Missing
integrity fields are never inferred or defaulted.

The next-patch release workflow uses the same pinned Rust toolchain and Windows
dependency/target cache. A warm cache recompiles SPLINED itself without rebuilding
the complete Rust dependency graph; the first run for a new lockfile or toolchain
still performs a cold build and may take longer.

For a manual update, close SPLINED, download the official Windows ZIP, and
replace `splined.exe` plus the complete `runtime` folder together. Automatic
and manual executable replacement never touches `data\config.toml`,
`data\ui.toml`, or the selected credential, SQLite/history, temporary-cache,
and log locations.
Preserve the selected credential directory and `<scan.cache_dir>/splined.db`,
or create a `.spl` export first.

The relocated `runtime\splined-core.exe` resolves portable-relative paths from
the parent directory containing `splined.exe`, so it does not create a second
`runtime\_cache\splined.db`. Existing absolute, mapped-drive, and UNC paths are
preserved exactly.

See [Windows portable instructions](../release/README-WINDOWS.txt).

## Linux portable

Extract the archive into its final directory, restore executable permission if
needed, then run:

```bash
chmod +x ./splined
./splined --help
```

See [Linux portable instructions](../release/README-LINUX.txt).

## macOS portable

Extract the archive into its final directory, restore executable permission if
needed, then run:

```bash
chmod +x ./splined
./splined --help
```

macOS may require first-run approval in **Privacy & Security**.

See [macOS portable instructions](../release/README-MACOS.txt).

## Docker

Use the published image:

```text
ghcr.io/scottia/splined:latest
```

Begin with [`docker/config.example.toml`](../docker/config.example.toml). It is
Config v5 with Docker-specific container paths.

See [Docker installation](../docker/README.md) for mounts and commands.

Interactive Python/Docker operational scans use the Ratatui TUI when stdin and
stdout are terminals. Scripted or redirected execution remains plain.

## Python/Docker first media-index build

The first interactive launch creates:

```text
<scan.cache_dir>/splined.db
```

Default Docker path:

```text
/_cache/splined.db
```

The one-time build:

1. inventories Artist and Album folders;
2. reads one representative audio file per Album with Mutagen;
3. identifies Artists and Albums from MusicBrainz/tag values;
4. records local `cover.*` and selected Album statistics;
5. saves resumable per-Album SQLite checkpoints;
6. validates and publishes the completed snapshot marker-last;
7. opens Select Media with final stable Album Status colors.

The first build may take time on a large library. Warm launches load the picker
read-only from SQLite and do not repeat an Artist-by-Artist filesystem
validation. WAL-free databases on NAS/bind mounts may be staged temporarily on
local container storage for faster compact reads.

The Artist Picker groups Albums by the first physical directory below the
library root. Album Artist/MusicBrainz tag identity remains artwork/search
authority and may have a different count from visible physical Artist folders.

Use the explicit Refresh action after external tagger or
filesystem changes. Refresh commits the new model before changing the visible
picker, so colors do not progressively change while the user is working.

See [SPLINED media database](splined-media-database.md).

## Persistent and disposable data

On Windows, **File > Backup > Export Backup...** can include portable Config v5,
interface state, credentials, SQLite, and diagnostics independently. On
file-backed runtimes, back up `config.toml`, credentials, and the authoritative
runtime database:

```text
<scan.cache_dir>/splined.db
```

Run logs and other candidate, sample, and transient cache files remain
disposable.

Credential files may contain API keys and OAuth tokens. Never commit or share
them.

## First safe scan

1. Confirm library and scan paths.
2. Validate Config v5 where the runtime exposes validation.
3. Allow the Python/Docker media index to complete on first TUI launch.
4. Use **Select Media** to choose a small Artist or Album set.
5. Use Read first.
6. Review provider candidates and local/embedded artwork decisions.
7. Enable Write only after confirming output and replacement policy.

Write changes Album folders. Use a backup, snapshot, copy, or staging library
for initial validation.
