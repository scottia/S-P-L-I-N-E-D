# S:P:L:I:N:E:D — /dev documentation index

**Checkpoint: November 19, 2026.** The owner-approved Artist/Album
classification and appearance requirements are **established**, not pending
a repeat approval. The matching checked-in media-state JSON and focused
source corrections have been committed to `dev`. Full installed-client
validation is still outstanding; that does not revoke owner approval.

## Documentation hierarchy

| Reference | Purpose and authority |
| --- | --- |
| [POLICY-REGISTER.md](POLICY-REGISTER.md) | **Approved owner requirements** for Artist/Album name ranking, Album status dots, the approved existing frames/banners/COMP-track colors, deployment-independent classification, and policy change protection |
| [POLICY-INDEX.md](POLICY-INDEX.md) | Maps approved policy IDs and all four `schema/` artifacts to runtime code, tests, protection, and **current validation status** |
| [SCHEMA-VALIDATION.md](SCHEMA-VALIDATION.md) | **Current state of implementation**: what changed on `dev`, what was inspected, what passed static checks, and what runtime evidence remains |
| [build-layout.md](build-layout.md) | Workflow 01's two release roots and shared-dependency boundary |
| [webgui-docker-deployment.md](webgui-docker-deployment.md) | WebGUI Docker Compose, local environment, and GHCR build inputs |
| [release-automation.md](release-automation.md) | Source-verified Workflows 01–03, Store/GHCR recovery and release-gate limitations |
| [catalog-authority.md](catalog-authority.md) | Windows SQLite ownership, PostgreSQL-only state, reconciliation and artwork provenance |
| [splined-media-database.md](splined-media-database.md) | Windows SQLite index, shared-file safety, v3 coordinator and backup |
| [windows-schema-upgrade-preparedness.md](windows-schema-upgrade-preparedness.md) | v2→v3 one-way schema migration and verified downgrade backup |
| [config-v5-reference.md](config-v5-reference.md) | Shared configuration contract, storage paths and runtime-specific Settings |
| [credentials-providers.md](credentials-providers.md) | Credential-directory contract, providers and optional PostgreSQL coordination |
| [source-policies-range-types.md](source-policies-range-types.md) | Image quality/range vocabulary, provider controls and separate enhancement policy |
| [windows-store.md](windows-store.md) | Native MSIX/Store build, package identity and tagged recovery |
| [windows-guide.md](windows-guide.md) | Windows user workflow, SQLite setup, artwork, status colors and backup |
| [browser-workspace-guide.md](browser-workspace-guide.md) | Browser/PostgreSQL use, indexing, artwork queue and COMP operations |

**Precedence:** explicit current owner decisions → approved register →
checked-in JSON/SQL/UI contracts → application code/tests as verifiable
implementation evidence. No unverified document, source behavior, or test
assertion may silently replace approved policy.

## Approved classification at a glance

- **Artist NAME:** Red > Yellow > Purple > Green. No matching category:
  **Red**. Never White, Dark Grey, Orange, Gold, or Blue.
- **Album NAME:** Dark Grey > Red > Yellow > Purple > Orange > Green.
  No matching category: **Red**. Never White or Blue.
- **Album status dots:** show **all applicable**, in order Dark Grey,
  Red, Yellow, Purple, Orange, Green, auxiliary Blue (`cover.webp` only).
- **Unprocessed** is a **MEDIA FILTER numeric count only**; it must
  never act as a name color, a status dot, or a shortcut to Red.
- The owner's screenshot approves the existing **status dots, Album
  card frames, banners, and Blue/Green/Purple COMP track-row colors**.
  Do not change these merely to correct list-name classification.
- Standalone SQLite, Shared / Network SQLite and PostgreSQL coordination
  use **the same canonical color policy**. Windows may embed the same
  checked-in JSON for offline use; database mode is not a policy selector.

## The four `schema/` artifacts

Both `media-state-classification.json` and `comp-report-policy.json` are
protected behavioral-policy files. `media.sql` is the SQLite DDL; 
`settings-ui.json` is the Config v5 UI contract. All four remain
single-sourced in [`/schema`](../schema) and are referenced in the
[policy index](POLICY-INDEX.md).

The COMP policy remains the owner's **accepted working baseline**, with
no speculative matching/ranking edits. Its detailed runtime conformity
has **not** been independently certified by this documentation review.
The SQL and Settings contracts passed the stated **structural** inspection;
execution/migration and real-client parity are still to be checked.

## Documentation review boundary

These restored guides have been **reviewed and revised for the current
two-target source tree**. They distinguish implemented configuration
and release behavior from untested live UI, media-write, and
cross-client equivalence. A guide does not establish an independent
behavioral policy; the owner's approved register remains normative.
No unsupported third release target or competing catalog authority is
introduced by a document.

## Changes and pull requests

`/dev` currently contains the corrected JSON wording, Python/Rust
rank/dot validators, removal of WebGUI CSS and Windows Slint's forced
neutral Unprocessed/All **name** colors, and WebP-only static-cover
separation. See [validation notes](SCHEMA-VALIDATION.md) for the precise
remaining defects; do not claim full visual acceptance yet.

- [PR #217](https://github.com/scottia/S-P-L-I-N-E-D/pull/217) **merged
  into `dev`**: mandatory Codex Draft PR process, current-PR-only owner
  exceptions, corrected documentation and WebGUI Docker relocation.
  Its successful Python/JavaScript/container tests cover that PR's scope,
  not full Windows native acceptance or deployed policy revision parity.
- [PR #216](https://github.com/scottia/S-P-L-I-N-E-D/pull/216) **closed
  unmerged**: the development restructuring was already on `dev`;
  nothing was promoted to `main`. New production promotion requires
  a separate authorized and fully validated PR.

The independent **Canonical policy JSON — review gate** exists in CI.
It fails changes to either policy JSON and directs review to `AGENTS.md`;
the required GitHub branch-ruleset setting remains unverified. Owner
approval never automatically makes a failing GitHub check pass.

## Public Help versus development documents

Development documents here are **not** automatically public Help.
Application Help links should use **reviewed `main` documentation only**,
never `dev`. Current Help is acknowledged as inaccurate and is not being
silently rewritten or promoted.

Additional guides may be added here selectively after their accuracy is
checked against the approved register and current runtime behavior.
`main` and deployed PostgreSQL/SQLite policy revisions must remain
unchanged until their separately authorized workflows.
