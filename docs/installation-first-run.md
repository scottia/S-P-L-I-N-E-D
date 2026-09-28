# Installation and First Run

Choose the runtime that matches your system. All supported implementations use
Config v5; application release identities remain independent.

## Windows portable

1. Download the Windows archive from the
   [latest release](https://github.com/scottia/S-P-L-I-N-E-D/releases/latest).
2. Extract the complete archive into its final application directory.
3. Run `splined.exe` directly.
4. Choose the music library and keep the portable defaults or open Advanced
   Settings.
5. Configure provider credentials through **Credentials / Status** as needed.
6. Start in Read mode against a small selection.

SPLINED creates its application-owned layout as required:

```text
SPLINED/
├── splined.exe
├── config/
│   ├── config.toml
│   └── ui.toml
├── credentials/
├── _cache/
└── _logs/
    └── _history/
```

No executable rename or setup launcher is required.

### Windows upgrade

1. Close SPLINED.
2. Back up `config/`, `credentials/`, and `_logs/`.
3. Extract the new application files into the existing directory, replacing
   program files.
4. Preserve application-owned data directories.
5. Run `splined.exe` and verify Settings and credential status.

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
5. commits the model to SQLite;
6. opens Select Media with final stable Folder Status colors.

The first build may take time on a large library. Warm launches load the picker
from SQLite and do not repeat an Artist-by-Artist filesystem validation.

Use the explicit Refresh action after external Picard, Beets, Navtagger, or
filesystem changes. Refresh commits the new model before changing the visible
picker, so colors do not progressively change while the user is working.

See [SPLINED media database](splined-media-database.md).

## Persistent and disposable data

Back up for all runtimes:

- `config/`;
- `credentials/`;
- `_logs/` and the configured history directory.

For Python/Docker, also back up:

```text
<scan.cache_dir>/splined.db
```

Other candidate, sample, and transient cache files remain disposable.

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
