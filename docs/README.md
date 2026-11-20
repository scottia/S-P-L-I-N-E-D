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

**Precedence:** explicit current owner decisions → approved register →
checked-in JSON/SQL/UI contracts → application code/tests as verifiable
implementation evidence. Source, a stale PR, or historical documents may
not silently replace approved behavior. The old **38-row comparison is
discarded as policy**.

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

## Changes and pull requests

`/dev` currently contains the corrected JSON wording, Python/Rust
rank/dot validators, removal of WebGUI CSS and Windows Slint's forced
neutral Unprocessed/All **name** colors, and WebP-only static-cover
separation. See [validation notes](SCHEMA-VALIDATION.md) for the precise
remaining defects; do not claim full visual acceptance yet.

- [Draft PR #216](https://github.com/scottia/S-P-L-I-N-E-D/pull/216)
  is the accumulated `dev` → `main` restructuring/policy work.
  It remains **HOLD** because of branch divergence and missing complete
  runtime verification.
- [Draft PR #217](https://github.com/scottia/S-P-L-I-N-E-D/pull/217)
  targets `dev` and proposes Codex's mandatory PR workflow with
  **explicit owner approval for a named action on the current PR only**.
  Its governance text is not merged until the owner approves.

The independent **Canonical policy JSON — review gate** exists in CI.
It fails changes to either policy JSON and directs review to `AGENTS.md`;
the required GitHub branch-ruleset setting remains unverified. Owner
approval never automatically makes a failing GitHub check pass.

## Public Help versus development documents

Development documents here are **not** automatically public Help.
Application Help links should use **reviewed `main` documentation only**,
never `dev`. Current Help is acknowledged as inaccurate and is not being
silently rewritten or promoted.

Historical documentation and the old source tree are preserved on
[`dev-archive`](https://github.com/scottia/S-P-L-I-N-E-D/tree/dev-archive).
They are evidence, not approved replacements. `main` and deployed
PostgreSQL/SQLite policy revisions must remain unchanged until their
separate authorized workflows.
