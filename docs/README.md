# S:P:L:I:N:E:D — current /main documentation

This directory documents **only the supported source in** `build/Windows/` and `build/WEBGui/`, the shared indexer/catalog authority, and the approved contracts under `schema/`. It is not an archive of old PRs, test reports, or development-branch promotion instructions.

## Windows — `build/Windows/`

- [Install and first run](installation-first-run.md) — setup, supported distributions and safe first checks.
- [Windows application guide](windows-guide.md) — Artist/Album selection, artwork, COMP workflow and operator actions.
- [Windows source/build guide](windows-build-guide.md) — Rust/Slint crate, local validation and packaging.
- [Windows SQLite catalog and indexer](splined-media-database.md) — identity, refresh, generations, state, schema upgrade and sharing.
- [SQLite v2→v3 upgrade safeguards](windows-schema-upgrade-preparedness.md) — operator preparation and backup.
- [Microsoft Store](windows-store.md) — Store-managed distribution, distinct from portable ZIP.

## WebGUI — `build/WEBGui/`

- [WebGUI workspace guide](browser-workspace-guide.md) — browser operations, PostgreSQL catalog/index maintenance and artwork review.
- [Docker deployment](webgui-docker-deployment.md) — canonical `build/WEBGui/docker/` Compose, Config v5, persistent volumes and shared `schema/`.
- [Catalog field authority](catalog-authority.md) — Windows-owned synchronized fields, PostgreSQL-only state and coordination.

## Shared contracts, policy and indexing

- [Approved policy register](POLICY-REGISTER.md) — **owner-authoritative** Artist/Album rankings, dots, visual safeguards and review boundaries.
- [Shared Windows/WebGUI lifecycle](lifecycle-policy.md) — [lifecycle-policy.json](../schema/lifecycle-policy.json) common behavior outcomes, native activation requirements, separate SQLite/PostgreSQL authority, WebGUI refinements and remaining validation gates.
- [Policy and indexer source map](POLICY-INDEX.md) — each canonical contract's actual Windows/WebGUI consumers and tests.
- [Source validation status](SCHEMA-VALIDATION.md) — completed source checks versus outstanding native/deployed verification.
- [Artist/Album state policy delivery](media-state-classification-policy.md) and [COMP report policy](comp-report-policy.md) — operational handling of the two protected JSON contracts. Neither creates a second rulebook.
- [Config v5 reference](config-v5-reference.md) and [source policies/ranges](source-policies-range-types.md) — settings and candidate/artwork rules.
- [Credentials/providers](credentials-providers.md), [MusicBrainz OAuth](musicbrainz-oauth.md) and [OAuth validation](oauth-validation.md).

The five checked-in shared contracts are `schema/media-state-classification.json`, `schema/comp-report-policy.json`, `schema/lifecycle-policy.json`, `schema/media.sql` and `schema/settings-ui.json`. They are not copied into competing Windows or WebGUI policy directories. The new lifecycle JSON is a **declarative, proposed shared contract**, not yet loaded by either client or protected by the existing two-JSON gate. Windows SQLite remains authoritative for designated synchronized catalog fields; WebGUI maintains a separate PostgreSQL catalog. A shared policy does not itself synchronize them.

## Release and build boundaries

- [Build directory layout](build-layout.md) — current source paths and release inputs.
- [Workflows 20–24 release architecture](release-automation.md) — quick Windows evaluation, immutable candidate preparation, independent Windows/WebGUI builds, publication-only release completion, and separate Microsoft Store submission.
- [Root release support](../release/README.md) — scripts, templates and synthetic regression fixtures.

Historical PR acceptance records, old screenshots, source audits and superseded platform instructions have been retired from active `docs/`; Git history retains them. The former fallback workflow guide was superseded by the current source-policy and COMP contracts and is not an independent authority.

This documentation reflects source checked into `main`, not proof of installed Windows/WebGUI behavior. GitHub's Windows advisory is not a native Windows compile. Source edits, policy changes and releases require separate review per [AGENTS.md](../AGENTS.md).

[Repository home](../README.md)
