# Config v5 and Settings contract

**Implementation reference, not a new policy authority.** Config v5 is
the shared configuration schema; application release versions and
configuration schema versions are independent. The checked-in
[`schema/settings-ui.json`](../schema/settings-ui.json) describes
Settings presentation/fields, while runtime TOML and source modules
supply validation and persistence. Runtime-specific controls need
independent testing; similar field names do not prove identical UI.

The current WebGUI Docker Config example is at
[`build/WEBGui/docker/config.example.toml`](../build/WEBGui/docker/config.example.toml).
See [Docker deployment](webgui-docker-deployment.md).

## File location and application ownership

| Client | Active configuration |
| --- | --- |
| WebGUI | Server-visible Config v5 at `SPLINED_CONFIG`, normally container `/config/config.toml`; PostgreSQL stores its catalog, jobs and sessions |
| Native Windows | Portable or packaged persistent Config v5 state; the native `splined.db` under configured `scan.cache_dir` remains authoritative for synchronized catalog fields |

Browser paths refer to the container/server, **not** the computer
running the browser. Windows local, mapped and UNC paths are resolved
against its configured state root without inventing an alternative
library or catalog. Credentials remain separate JSON files under the
configured directory, not inline Config v5 secrets.

## Settings sections

The current [Settings UI contract](../schema/settings-ui.json)
defines eight sections: **Library & Files**, **Artwork & Embedding**,
**Enhancement**, **Artwork Sources**, **Connections**, **Backup &
Recovery**, **Processing & Logs**, and **Catalog Maintenance**. Some controls are marked Windows-only or WebGUI-only.

| Config group | Selected current concerns |
| --- | --- |
| `[library]` | Music Library, optional V/A / OST Artist Images root, optional Curated Compilations root, library path mapping |
| `[scan]` | Persistent SQL Database Directory / cache, ignored folders, bounded indexing, Windows-only `sqlite_shared`, `postgres_coordination` and `windows_client_lock` |
| `[output]` | Cover, Artist Art and banner stems/formats, preservation, square and embedded thumbnail preferences, explicit enhancement and upscaling |
| `[webgui_ui]` | Browser presentation options including animated WebP preview, without changing saved source artwork bytes |
| `[range]`, `[sources]`, `[source_policies.*]` | Preferred/ideal resolution quality ladder, provider enablement/order, range policies, decoded-content Strict Override and optional source overrides |
| `[samples]` | Optional sample image/report output |
| `[credentials]` | `credential_dir` only; secrets and provider tokens remain in individual protected provider files |
| `[logging]`, `[history]`, `[aisplined]` | Operational diagnostics, retention and optional enhancement-model settings where supported |

For the authoritative fields, types, ranges and runtime flags, inspect
the checked-in JSON and actual
[Windows configuration](../build/Windows/src/config.rs) and
[WebGUI runtime](../build/WEBGui/webgui/runtime.py).
A UI help description does not override a validated config value.

## One classification policy regardless of mode

**`scan.sqlite_shared` changes how the Windows SQLite file is
accessed; it does not select a different policy.** Windows may embed
the same canonical checked-in media-state/COMP behavioral JSON.
Existing Windows copies or PostgreSQL-saved revisions must be
checked for semantic drift rather than treated as competing policy
authorities. See the approved [Policy Register](POLICY-REGISTER.md)
and the [Policy Index](POLICY-INDEX.md).

The media-state and COMP JSON contracts belong in root `schema/`,
**not** Config v5 and not independently editable defaults chosen
by Setup. Intentional changes to those protected JSONs require
the independent PR review gate.

## Path, credential and backup safety

Keep persistent cache, database, log and credential paths explicit.
When relocating a Config v5 file, validate relative-path resolution
before opening or recreating a catalog. Back up Config v5, SQLite
and PostgreSQL separately according to the client, and keep API keys
and OAuth credentials out of documentation and Git history.

Detailed source-policy semantics are in
[Source Policies and Range Types](source-policies-range-types.md).
Provider credentials are in
[Credentials and Provider Setup](credentials-providers.md).
**Validation open:** exact Settings controls, persisted defaults,
range limits and cross-client parity require real runtime checks.
