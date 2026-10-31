# S:P:L:I:N:E:D Documentation

This index separates current operating guidance from maintainer procedures and
historical evidence. Follow the current guides for product behavior. Audits,
dated reconciliation notes, and unverified follow-ups are retained for context,
not as instructions for an installed release.

## Current user guides

Start here:

1. [Install SPLINED and complete the first run](installation-first-run.md).
2. Choose the guide for your interface:
   [Windows](windows-guide.md) or [Ubuntu WebGUI](browser-workspace-guide.md).
3. Review [Config v5](config-v5-reference.md) and
   [credential/provider setup](credentials-providers.md).
4. Back up persistent state and begin with Read mode on a small selection.

The current guides have distinct ownership:

- [Windows application guide](windows-guide.md) — Windows library browsing,
  artwork review, MusicBrainz choices, Write behavior, backup, updates, and
  troubleshooting.
- [WebGUI guide](browser-workspace-guide.md) — browser setup, PostgreSQL index
  and queue behavior, artwork/COMP review, writes, recovery, and deployment
  boundaries.
- [Windows Select Media and status colors](media-filter-status-colors.md) —
  native filters, selection scope, counts, status meaning, and accessibility.
- [Microsoft Store channel](windows-store.md) — installed Windows identity,
  Store-managed updates, and the separation from portable releases.

## Authoritative technical references

- [Config v5 reference](config-v5-reference.md) owns configuration fields,
  defaults, path rules, and validation.
- [Source policies and Range Types](source-policies-range-types.md) owns
  candidate eligibility, ranking, local artwork, output evaluation, and the
  COMP/per-track policy.
- [Windows SQLite media database](splined-media-database.md) owns native SQLite
  identity, indexing, durable state, sharing, and backup boundaries. It also
  records that WebGUI uses a separate PostgreSQL catalog.
- [Credentials and provider setup](credentials-providers.md) owns credential
  files, supported provider contracts, protection, relocation, and backup.
- [MusicBrainz OAuth](musicbrainz-oauth.md) and
  [provider validation](oauth-validation.md) own authorization and diagnostic
  procedures.
- [MusicBrainz fallback workflow](musicbrainz-fallback-workflow.md) records the
  current Windows fallback identity/UI contract; source-policy rules remain
  authoritative when wording overlaps.

Provider secrets never belong in Config v5, source control, screenshots, or
diagnostic reports.

## Maintainer procedures and architecture

- [Release automation](release-automation.md) — numbered release/recovery
  workflows, validation selection, Store handling, and release-note authority.
- [Windows source/build guide](../windows/README.md) — native Rust/Slint build,
  runtime layout, packaging, and source validation.
- [Docker deployment](../docker/README.md) — Compose, PostgreSQL, persistent
  mounts, backup, and container validation.
- [Browser workspace schematic](browser-workspace-schematic.md) — maintainer
  map of browser regions and source locations; not a parity guarantee.
- [Windows Rust/Slint architecture checklist](windows-rust-slint-migration-checklist.md)
  — current single-executable architecture and retired implementation boundary.

Portable artifact instructions are distributed with the artifacts:
[Windows](../release/README-WINDOWS.txt),
[Linux](../release/README-LINUX.txt), and
[macOS](../release/README-MACOS.txt). An instruction file documents an artifact
format; it does not by itself guarantee that every release publishes that
platform.

## Historical audits and unverified follow-ups

These records preserve review evidence or work that still required validation.
Do not use them in place of the current guides above:

- [Windows Setup follow-ups](windows-setup-follow-ups.md) — source-only items
  still awaiting end-to-end verification.
- [Windows/WebGUI function audit](windows-web-function-audit.md) — migration-era
  comparison and runtime observations.
- [WebGUI live-write parity record](webgui-live-write-parity-2026-10-30.md) and
  [WebGUI settings parity summary](webgui-settings-parity-summary-2026-10-29.md).
- [Windows baseline reconciliation](windows-baseline-reconciliation-2026-10-18.md)
  and [Windows source audit](windows-source-audit-2026-10-17.md).
- [Windows 1.0.88 functional-parity audit](windows-functional-parity-1.0.88.md)
  and [Windows graphical-workspace QA](windows-gui-redesign-qa.md).

Future design ideas live in
[project notes](../project-notes/artwork-composition.md) and are not supported
behavior until implemented, validated, and documented in a current guide.

## Platform boundaries and safety

| Runtime | Interface | Persistent authority |
| --- | --- | --- |
| Windows | Native Rust/Slint application | SQLite plus Windows-owned Config/UI state |
| Ubuntu WebGUI | Python/browser application deployed with Docker | PostgreSQL plus mounted config, credentials, cache, and artwork state |
| Root-native Linux/macOS | Separate Rust command-line implementation | Runtime-selected native configuration and cache/database paths |

The runtimes share concepts, not a live database or guaranteed interface
parity. Avoid simultaneous writers to the same music files.

- Read mode evaluates without installing music-library artwork.
- Write mode changes album or embedded artwork; test with a backup, snapshot,
  staging library, or small selection first.
- Back up configuration, credentials, and the runtime's persistent database.
  Temporary candidates and diagnostic logs are not substitutes for that state.

Return to the [repository README](../README.md).
