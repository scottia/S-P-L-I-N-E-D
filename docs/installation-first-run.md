# Installation and First Run

Choose the runtime that matches your system. All supported implementations use
Config v5; their application release identities remain independent.

## Windows portable

1. Download the Windows archive from the
   [latest release](https://github.com/scottia/S-P-L-I-N-E-D/releases/latest).
2. Extract the complete archive into its final application directory.
3. Run `splined.exe` directly.
4. Choose the music library and keep the portable defaults or open Advanced
   Settings.
5. Configure provider credentials through **Credentials / Status** as needed.
6. Start in Read mode against a small selection.

SPLINED creates and uses its application-owned layout as required:

```text
SPLINED/
├── splined.exe
├── config/
│   ├── config.toml
│   └── ui.toml
├── credentials/
├── _cache/
│   └── samples/
└── _logs/
    └── _history/
```

No executable rename or setup launcher is required.

### Windows upgrade

1. Close SPLINED.
2. Back up `config/`, `credentials/`, and `_logs/`.
3. Extract the new application files into the existing directory, replacing
   the program files.
4. Preserve the application-owned data directories.
5. Run `splined.exe` and verify Settings and credential status.

`_cache/` is disposable and can be recreated. SPLINED does not automatically
discover or import another portable installation.

See [Windows portable instructions](../release/README-WINDOWS.txt).

## Linux portable

Extract the Linux archive into its final directory, restore executable
permission if needed, then run:

```bash
chmod +x ./splined
./splined --help
```

See [Linux portable instructions](../release/README-LINUX.txt).

## macOS portable

Extract the macOS archive into its final directory, restore executable
permission if needed, then run:

```bash
chmod +x ./splined
./splined --help
```

macOS may require first-run approval in **Privacy & Security**.

See [macOS portable instructions](../release/README-MACOS.txt).

## Docker

Use the published container image:

```text
ghcr.io/scottia/splined:latest
```

Begin with [`docker/config.example.toml`](../docker/config.example.toml). It is
Config v5 with Docker-specific container paths.

See [Docker installation](../docker/README.md) for mounts and commands.

Interactive Python/Docker operational scans use the Ratatui TUI when stdin and
stdout are terminals. Scripted or redirected execution remains plain. See
[Python Ratatui TUI](ratatui-tui.md) for OLED/CHALK theme selection, keyboard
controls, and terminal requirements.

## Persistent and disposable data

Back up:

- `config/`;
- `credentials/`;
- `_logs/` and the configured history directory.

Disposable:

- `_cache/`.

Credential files may contain API keys and OAuth tokens. Never commit or share
them.

## First safe scan

1. Confirm the library and scan paths.
2. Validate Config v5 in Settings on Windows.
3. Use **Select Media** to choose a small artist or album set.
4. Use Read mode first.
5. Review provider candidates and local-artwork decisions.
6. Enable Write mode only after confirming output and replacement policy.

Write mode changes album folders. Use a backup, snapshot, copy, or staging
library for initial validation.
