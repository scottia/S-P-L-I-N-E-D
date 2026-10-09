# S:P:L:I:N:E:D Documentation

This directory documents the current **S:P:L:I:N:E:D 1.0.31** behavior and
Config v5 schema. It is organized by task so one subject has one authoritative
home. Historical changes and release notes are maintained in
[GitHub Releases](https://github.com/scottia/S-P-L-I-N-E-D/releases), not mixed
with the current product documentation.

## Start here

1. [Install and complete the first run](installation-first-run.md).
2. Choose the interface guide for the runtime you use:
   [Windows](windows-guide.md) or [Python Ratatui](ratatui-tui.md).
3. Review [Config v5](config-v5-reference.md) and
   [credentials/provider setup](credentials-providers.md).
4. Start in Read mode with a small selection before enabling Live Write.

## Guides by task

### Install and operate

- [Installation and first run](installation-first-run.md) — Windows portable,
  Linux, macOS, Docker, initial indexing, and the first safe scan.
- [Windows guide](windows-guide.md) — the native interface, portable settings,
  candidate review, MusicBrainz Matches, backup, updates, and troubleshooting.
- [Python Ratatui guide](ratatui-tui.md) — persistent Select Media, controls,
  candidate decisions, MusicBrainz review, reports, and terminal cleanup.

### Configure artwork and providers

- [Config v5 reference](config-v5-reference.md) — every configuration section,
  path rule, runtime option, and validation requirement.
- [Source policies and Range Types](source-policies-range-types.md) — global
  range, source overrides, strict content validation, ranking eligibility,
  compilation authority, local artwork, and AISPLINE boundaries.
- [Credentials and provider setup](credentials-providers.md) — supported
  providers, credential files, protection, relocation, and backup.
- [MusicBrainz OAuth](musicbrainz-oauth.md) — authorization, refresh, timing,
  retries, and recovery.
- [API/OAuth validation](oauth-validation.md) — safe provider checks and
  provider-specific recovery guidance.

### Understand library state

- [SPLINED media database](splined-media-database.md) — SQLite authority,
  cross-platform sharing, identity, indexing, status persistence, backup, and
  inspection.
- [Select Media and status colors](media-filter-status-colors.md) — filters,
  selection scope, Album and Artist states, counts, refresh, and accessibility.

### Platform packaging

- [Microsoft Store Windows channel](windows-store.md) — live installation,
  exact production identity, Store-managed in-app update requests, dedicated
  existing-tag packaging, isolated development QA, and Partner Center publishing.
- [Docker installation](../docker/README.md)
- [Windows source and build guide](../windows/README.md)
- [Windows portable instructions](../release/README-WINDOWS.txt)
- [Linux portable instructions](../release/README-LINUX.txt)
- [macOS portable instructions](../release/README-MACOS.txt)

## Runtime map

| Runtime | User interface | Configuration | Persistent state |
| --- | --- | --- | --- |
| Windows | WinForms + in-process Rust DLL | Portable `data/config.toml`/`data/ui.toml`, or per-user package LocalState for MSIX | `splined.db` plus separately stored credentials |
| Python/Docker | Ratatui when attached to a terminal; plain CLI when redirected or disabled | File-backed Config v5 | `splined.db` plus separately stored credentials |
| Linux/macOS native CLI | Command line | File-backed Config v5 | Runtime-selected cache and database paths |

All supported runtimes share Config v5 concepts, source policy language, and
SQLite Album/status semantics. Interface controls and configuration storage
differ by platform; the linked interface guides describe those differences.

## Documentation boundaries

- The Config reference defines fields and validation, while the source-policy
  guide defines how those fields affect candidate eligibility and ranking.
- The media-database guide owns SQLite, identity, inventory, and durable status
  behavior. The Select Media guide owns their visual projection and controls.
- Provider credentials are documented separately from Config because secrets
  never belong in `config.toml` or Windows interface settings.
- GitHub Releases owns version history. Files in `/docs` describe only the
  currently supported behavior.

## Safety baseline

- Never commit credential JSON, API keys, OAuth tokens, or private values.
- Read mode evaluates without installing artwork or committing completion.
- Test Live Write against a backup, snapshot, staging library, or small Album
  selection first.
- Back up Config/settings, credentials, and `splined.db`; temporary candidate
  images and diagnostic logs are disposable.

Return to the [repository README](../README.md).
