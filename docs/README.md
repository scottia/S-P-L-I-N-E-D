# S:P:L:I:N:E:D Documentation

**S:P:L:I:N:E:D** — **SEARCHABLE:PIXEL:LINKS:IDENTIFIED:NORMALIZED:ENRICHED:DEFINED**

This directory is the canonical project documentation for S:P:L:I:N:E:D.

The repository documentation is versioned with the source. The GitHub Wiki, when enabled, should act as a navigation and presentation layer around these pages rather than becoming a second independent source of technical truth.

## Documentation status

- **Windows GUI:** documentation target is **v3.0.0 Stable**.
- **Windows configuration:** **Config v5**.
- **Python / Docker:** maintained as a separate supported implementation and release line.
- **Config schema version and application release version are separate concepts.**

The finalized Windows GUI source has not yet been integrated into this repository. Until that source lands, the Windows v3.0.0 / Config v5 pages in this directory define the intended public behavior and should be reviewed against the final local Windows source before release packaging is published.

---

## Getting started

### [Installation and first run](installation-first-run.md)
Install S:P:L:I:N:E:D on Windows or with Docker, understand the portable directory layout, and complete the first safe run.

### [Config v5 reference](config-v5-reference.md)
Windows GUI Config v5 concepts, path rules, artwork ranges, source policies, credential separation, and compatibility expectations.

---

## Configuration and providers

These pages are the next documentation set to be added:

- **Credentials and provider setup** — provider credential storage, filesystem protection, provider-specific setup, and credential-path behavior.
- **MusicBrainz OAuth** — authentication flow, token storage, runtime options, reauthorization, and troubleshooting.
- **Source policies and Range Types** — global artwork range policy, per-source overrides, fallback behavior, advanced dimensions, and provider capability notes.

---

## Windows GUI

Planned public guides:

- **Windows GUI walkthrough** — main screen, media tree, candidate review, Launch / Stop lifecycle, Settings, status displays, and help surfaces.
- **Media Filter and status colors** — live Artist/Album filtering, status filters, and tree-state meanings.
- **History, retention, bypass, and timeout** — persistent history, processed state, temporary reprocessing, bypass authority, timeout-active albums, and retention behavior.

### Status color summary

The Windows tree uses status colors as execution-state indicators rather than decoration:

| Color | Meaning |
| --- | --- |
| White | Default / unprocessed |
| Orange | Processed; may be manually reprocessed |
| Red | Bypassed; requires explicit bypass override |
| Purple | Partial artist state or timeout-active album, depending on row type/context |
| Green | Artist is fully processed |
| Blue | Artist contains one or more bypassed albums |

The GUI derives these states from the same history / bypass / timeout authority used by execution. It should not maintain an independent persistent GUI status database.

---

## Python / Docker

Planned public guides:

- **Python / Docker usage** — GHCR image, mounts, commands, runtime layout, and operational entrypoint.
- **CLI / Python command equivalents** — Windows GUI actions mapped to native/Python command equivalents where supported.

Existing Docker-specific information remains available in [`../docker/README.md`](../docker/README.md).

---

## Operations and recovery

Planned public guides:

- **Backup and portable layout** — application-owned directories, what is disposable, what should be backed up, and moving an installation.
- **Troubleshooting** — configuration parsing, credentials, providers, artwork discovery, history, Docker mounts, and common Windows GUI issues.
- **FAQ** — common operational and policy questions.

---

## Releases and compatibility

Planned public guide:

- **Release notes and versioning** — Windows GUI versioning, Python/Docker release versioning, Config schema versions, migration rules, and compatibility policy.

Important distinction:

```text
Windows GUI application version != Python/Docker application version != Config schema version
```

Do not infer one version from another unless a release explicitly states that they are synchronized.

---

## Documentation principles

1. **Repository docs are authoritative.** Wiki pages should link to or mirror versioned repository documentation.
2. **No credentials in docs.** Examples must use synthetic values only.
3. **No machine-specific private paths in public examples.** Use neutral placeholders.
4. **Behavior before implementation detail.** Document what users can rely on, then explain implementation details where they help operations or troubleshooting.
5. **Windows and Python/Docker differences are explicit.** Do not imply parity where platform-specific behavior intentionally differs.
6. **Config schema and release version are separate.** Config v5 does not mean application v5.
7. **Write-mode safety is explicit.** Users should validate against a backup, copy, snapshot, or staging library before changing a production library.

---

## Project links

- [Repository README](../README.md)
- [Windows portable setup notes](../release/README-WINDOWS.txt)
- [Docker installation](../docker/README.md)
- [Example repository configuration](../config.example.toml)

When the repository becomes public, the Windows GUI Help menu should link to stable public documentation locations rooted here, including the documentation home, Windows GUI guide, Config v5 reference, credentials/providers guide, and troubleshooting guide.
