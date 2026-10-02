# Installation and First Run

Choose the runtime that matches your system. All supported implementations use
Config v5; application release identities remain independent.

## Windows portable

1. Download the Windows archive from the
   [latest release](https://github.com/scottia/S-P-L-I-N-E-D/releases/latest).
2. Extract the complete archive into its final application directory.
3. Run `splined.exe` directly.
4. Choose the music library and the required cache, log, and credential
   directories, or open Advanced Settings.
5. Select **Save and Continue**.
6. Configure provider credentials through **File > Credentials...** as needed.
7. Start in Read mode against a small selection.

Windows v4 keeps Config v5 and interface state internally for the current user.
Extraction creates no `config`, `credentials`, `_cache`, `_logs`, or
`docker_builds` folders and no `config.location` file. The required directories
are created only at the user-selected locations after settings are saved. No
executable rename or setup launcher is required.

The first-run cache, log, and credential fields default beneath
`%LOCALAPPDATA%\SPLINED` for the current Windows user, not beside the portable
executable and not in machine-wide `%PROGRAMDATA%`. These locations remain
editable before saving; selecting UNC paths does not change the portable
program files. Existing saved locations are never migrated automatically.

The single-file launcher keeps its fingerprinted embedded GUI shell in
`%LOCALAPPDATA%\SPLINED\runtime` so Windows does not repeatedly extract and
security-scan the same build. Older fingerprinted shells are pruned on start.
This private executable cache contains no Config v5 values or credentials.
GUI-to-core Config v5 handoff is memory-only and creates no runtime TOML.

Use **File > Backup > Export Backup...** for a selective `.spl` backup. Internal
settings, interface state, credential JSON, SQLite, and diagnostics are
independent options. A password is optional. Double-clicking a registered
`.spl` file opens SPLINED's restore dialog; no data is restored until the user
chooses the sections and confirms.

### Windows upgrade

The Windows updater is built into `splined.exe`. Stable and dev builds check
their own channel after the library opens and through **Help > Check for
Update...**. Stable builds select the newest official, non-prerelease SPLINED
release that contains the paired Windows manifest and updater. Dev builds use
the temporary rolling `windows-dev` prerelease. Neither channel can consume the
other channel's assets.

An accepted update is verified against the manifest's SHA-256 and byte count,
installed beside the existing executable with rollback protection, and then
restarted. Official Windows releases therefore publish all three artifacts:
the portable ZIP, `windows-update.json`, and `setup-splined.exe`.

This is a complete executable replacement, not an in-place binary patch. The
rolling workflow uses one optimized Windows build and a reusable Rust cache; it
does not repeat the full test and static-analysis matrix before each active-dev
update.

The updater replaces only `splined.exe`. It does not rewrite internal settings,
the selected credential/cache/log directories, or the configured shared SQLite
database. An active Album run must be stopped or completed before installation.
If the download, manifest, size, checksum, replacement, or restart validation
fails, the GUI reports the failure and leaves persistent application data
unchanged.

The updater can still be performed manually by closing SPLINED and extracting
the current official Windows ZIP over the program files. Preserve the selected
credential directory and `<scan.cache_dir>/splined.db`, or create a `.spl`
export first.

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

On Windows, **File > Backup > Export Backup...** can include internal Config v5,
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
