# Release automation — current Workflow 01/02/03

This guide describes the **checked-in release workflow controls** on
`dev`. It is a source-verified release reference, **not** authorization
to publish or modify `main`. The supported application targets are
`build/WEBGui/` (Python/browser image and
PostgreSQL integration) and `build/Windows/` (Rust/Slint,
portable Windows and Microsoft Store packaging). There is no separate
root-native Linux CLI release target.

## Entrypoints and outcomes

| Workflow | Intent | Important limits |
| --- | --- | --- |
| [01 — Full Release](../.github/workflows/01-full-release.yml) | Explicitly confirmed, versioned release from `main`. Builds the selected Windows/WebGUI targets; creates the numeric release tag and GitHub Release only after required selected builds succeed. Publishes GHCR through the WebGUI release path. | Must be dispatched from `main`. Normal release provisioning is not a read-only status check. The Windows Store actions have separate inputs and safeguards. |
| [02 — Store Recovery](../.github/workflows/02-store-recovery.yml) | Rebuild and process a Microsoft Store package from an **existing published numeric Git release tag**, using the current reviewed release tooling. | Exactly one Store mode must be selected; it cannot create a new Git tag, GitHub Release, or GHCR image. |
| [03 — GHCR Recovery](../.github/workflows/03-ghcr-recovery.yml) | Verify and, where provably safe, recover a **missing** `vX.Y.Z` or `latest` GHCR tag for the latest published release. | Never creates/edits a GitHub Release, Store submission, release asset or Git tag; never rebuilds when both GHCR tags are absent. |

**Workflow 01** exposes target selection (`All`, `Windows`,
`WebGUI / Python`), explicit **Create release now** confirmation,
optional **Extended Windows validation**, and mutually exclusive
Store choices. Its versioning updates the selected release source
metadata only after validating the current main tag history.

**Workflow 02** requires an existing numeric version and a single
Store mode: create a Store draft for review, or stage and submit
for certification. Both are controlled by the Microsoft Store
submission safeguards.

**Workflow 03** verifies published-release identity, OCI
source/version/commit labels and registry digests. It re-tags
only a verified surviving image if exactly one tag is missing.
Unexpected provenance, digest conflicts, and both-tags-missing
conditions fail closed; do not silently overwrite existing tags.

## Build and package boundaries

The WebGUI build uses
[`build/WEBGui/Dockerfile`](../build/WEBGui/Dockerfile) with the
**repository root build context**, because it packages both
`build/WEBGui/webgui/` and canonical `schema/` files. Local
development Compose and the Config v5 example are under
[`build/WEBGui/docker/`](../build/WEBGui/docker);
see [WebGUI Docker deployment](webgui-docker-deployment.md).

Windows packages use the native
[`build/Windows/Cargo.toml`](../build/Windows/Cargo.toml)
source and Windows package manifests. The portable archive includes
[`release/README-WINDOWS.txt`](../release/README-WINDOWS.txt).
Neither deployment mode defines an independent Artist/Album state
policy: both consume the approved contracts in `schema/`.

The [release-support inventory](../release/README.md) lists the
active scripts, pinned git-cliff 2.14.2 installer, Tera templates,
Store-baseline processing and synthetic regression fixtures. Those
historic fixture versions are deliberate test cases—not stale
public release content—and must not be deleted by age alone.

## Validation and approval boundary

- Run code and PR checks before requesting any change to `main`;
  owner approval for a development PR is **not** approval to
  publish a release.
- The ordinary PR Windows check is **advisory**, not a native
  Windows build. Workflow 01 can run optional extended Windows
  Rust/Slint validation against the proposed release source.
- Workflow 01's actual inputs do **not** programmatically require
  Extended Windows validation merely because a Store mode is
  selected. Earlier descriptions of that extra requirement
  must **not** be presented as an enforced gate without a
  separate explicit owner decision and matching source change.
- The two behavioral JSON contracts in `schema/` remain
  protected by the independent **Canonical policy JSON —
  review gate**. Release configuration must never silently
  bypass its approved process.
- Production publication or `main` promotion requires
  its **own explicit owner instruction**, followed by applicable
  Windows/WebGUI/Store evidence. This document grants no
  such instruction.

**Source-checkpoint note:** This guide supersedes obsolete source
paths and historical status narratives for active `dev` only.
The authoritative behavior requirements remain in
[POLICY-REGISTER.md](POLICY-REGISTER.md), with implementation
status in [SCHEMA-VALIDATION.md](SCHEMA-VALIDATION.md).
