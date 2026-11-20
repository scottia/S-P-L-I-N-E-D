# Release automation — current Workflow 01–03 reference

**Scope:** The active `dev` source paths and numbered GitHub Actions
workflows. This is a technical runbook derived from their current definitions;
it is **not** authorization to publish a release or evidence that a complete
Windows/WebGUI release was tested. Published releases require the owner's
separate approval. The approved behavioral policy remains in
[POLICY-REGISTER.md](POLICY-REGISTER.md).

## Where release sources live

| Component | Authoritative repository path |
| --- | --- |
| Workflow 01, full release | [`.github/workflows/01-full-release.yml`](../.github/workflows/01-full-release.yml) |
| Workflow 02, Store recovery | [`.github/workflows/02-store-recovery.yml`](../.github/workflows/02-store-recovery.yml) |
| Workflow 03, GHCR recovery | [`.github/workflows/03-ghcr-recovery.yml`](../.github/workflows/03-ghcr-recovery.yml) |
| Windows source and packaging | [`build/Windows/`](../build/Windows/) |
| GHCR WebGUI Dockerfile | [`build/WEBGui/Dockerfile`](../build/WEBGui/Dockerfile) |
| WebGUI Compose/env/config | [`build/WEBGui/docker/`](../build/WEBGui/docker/) |
| Shared runtime JSON/SQL | [`schema/`](../schema/) |
| Release scripts, notes templates and Store regression data | [`release/`](../release/) including [`cliff.toml`](../release/cliff.toml) |

Workflow 01 is the normal publisher. Workflow 02 does not create new release
tags or GHCR images. Workflow 03 only repairs a missing GHCR tag using a
verified existing registry image. They must not be used as read-only status
queries or triggered as a side effect of documentation edits.

## Workflow 01 — full versioned release

**Entry:** Manually dispatch from `main` with explicit
`Create release now` confirmation. Supported platform selections are
`All`, `Windows`, and `WebGUI / Python`. The release version can be
omitted to select the next numeric patch, or supplied as a new `X.Y.Z`
greater than the active source and numeric tags on the `main` release line.

The workflow prepares the exact release commit and builds selected targets
before publishing a numeric tag and GitHub Release. It separately publishes
GHCR version and `latest` tags. Store processing, when selected, follows
the existing Windows package and GitHub Release preparation gates.

- **Windows / All:** Builds the native optimized Rust/Slint executable from
  `build/Windows/Cargo.toml`; creates the Windows portable and Store
  package artifacts. The optional `extended_windows_validation` adds strict
  Rust format, Clippy and library regression checks. The ordinary build is
  not proof that the extended suite ran.
- **WebGUI / Python / All:** Runs Python tests, browser JavaScript syntax
  checks, and builds/smoke-tests the container against PostgreSQL. It creates
  a WebGUI source archive containing `build/WEBGui/`, canonical
  `schema/`, `docs/` and root release-support metadata.
- **Store modes:** Workflow 01 has mutually exclusive
  `Submit Update/No Draft` and `Review Bundle/No Submit` options. The
  latter produces review artifacts without a Partner Center submission.
  Store options require a Windows-capable target and applicable credentials.
  Never infer permission to submit from a successful build alone.

**Extended Windows validation is opt-in, including Store modes.**
Workflow 01 does **not** automatically enable or require
`extended_windows_validation` when either Store mode is selected. Unless
explicitly enabled, the native build and package checks run without the
separate strict `cargo fmt`, Clippy and library-test suite. Earlier
claims that Store selection itself enforces these extended checks are
not supported by the current workflow inputs.

**Current workflow validation concern:** For `platform = Windows`,
`build-webgui` is skipped (as confirmed by `finalize-tag` and the
`release-success` gate), but the GHCR publication stage still executes.
Thus a Windows-only release **does not provide the same container test
evidence** as `WebGUI / Python` or `All`. This is an implementation
gap to review separately before considering the release gate complete.
Do not change the workflow without an authorized PR and focused tests.

Version stamping and manifests are under `build/Windows/`; the release
workflow does not build the withdrawn root-native CLI as a third target.
The current WebGUI Docker build context is the repository root, not the
nested Compose directory.

## Workflow 02 — Store recovery from an existing tag

**Entry:** Manually dispatch from `main` with an existing numeric
`X.Y.Z` release tag, and exactly one Store mode:
`Submit Update/No Draft` or `Submit Draft/No Update`.

This workflow checks out the exact existing tag, verifies release version
and Windows package manifest, rebuilds and smoke-tests the Windows MSIX,
then resolves the last-published Microsoft Store baseline and generates
cumulative Highlights. It uses release automation scripts from the
current approved release engine but builds tagged Windows source.

Workflow 02 does **not** mint a new numeric tag, create a GitHub Release or
publish GHCR. Store modes can affect Partner Center; no recovery dispatch
is authorized by this document.

**Compatibility observation:** The current Store-recovery workflow
retains a legacy fallback for older tags containing
`windows/Cargo.toml`. This is a tagged-source compatibility branch,
not a supported current `dev` source root. Do not relocate old source
back into active `dev` because of it.

## Workflow 03 — recovery of an existing published GHCR release

**Entry:** Manually dispatch from `main` for the **latest existing
published** numeric `X.Y.Z` GitHub Release. The workflow shares the
normal release concurrency group with Workflow 01.

It inspects `ghcr.io/scottia/splined:vX.Y.Z` and
`ghcr.io/scottia/splined:latest`:

- Both present, with matching digest and expected provenance: no-op.
- Exactly one missing: reuse the verified image and publish **only the
  missing tag**.
- Both missing, conflicting digest/labels, or unprovable absence: fail
  closed. It does **not** rebuild an unknown-equivalence image.

Workflow 03 neither creates/edits release tags or GitHub Releases nor
touches Store submissions or existing release assets.

## Test roles and source-retention review

| Check | When it runs | Disposition |
| --- | --- | --- |
| WebGUI Python policy/Album tests | Workflow 01 `WebGUI / Python` or `All`, and applicable PR validation | Retain independent active source tests |
| WebGUI JavaScript checks and Docker/PostgreSQL health | Same WebGUI build and selected PR validation | Retain as application release gates |
| Native Windows Cargo format/Clippy/library tests | Workflow 01 Windows/All **only** with extended validation enabled | Retain; do not claim the PR advisory executed them |
| Native optimized Windows application and package validation | Workflow 01 Windows/All release build | Retain |
| Release-note/Store baseline regression scripts and historic synthetic JSON | Workflow 01/02 release support and applicable packaging checks | Retain; historical version numbers are deliberate |
| Repository layout, scope and protected JSON guards | PR validation, separate from the release engine | Retain as controls for Workflow 01 source/layout and owner policy |

Duplicated root-level Album example data was retired from active
`dev` after confirming that local Windows and WebGUI tests already
covered the cases. A WebGUI-specific audit example remains with the
WebGUI test suite. Do not preserve an obsolete root path simply to
satisfy an outdated test, and do not remove active release evidence
because its sample version is older.

## Active `/release` content

[Release support files](../release/README.md) and the checked-in scripts
remain in service. `Generate-ReleaseNotes.ps1`,
`Generate-StoreHighlights.ps1`, `Resolve-StoreBaseline.ps1`,
`StoreHighlightsPolicy.ps1` and `Install-GitCliff.ps1` support the
numbered workflows. The shared classification is at
[`release/cliff.toml`](../release/cliff.toml), and Git-cliff is pinned to
**2.14.2**. Historical categories remain release-note grouping rules,
not supported application targets.
`templates/` contains release-note and Store Tera templates; the
`Test-*.ps1` scripts and `tests/` hold regression cases, including
deliberately old **synthetic** Store-version JSON fixtures. Old fixture
versions are **not** grounds for deleting those test files.

The Windows portable `release/README-WINDOWS.txt` is a packaged
operator instruction, not a separate media-state policy. Standalone and
Shared SQLite use the same canonical policy defined in the register.

## Review and promotion safeguards

1. Changes are prepared through a temporary branch and reviewed in a
   Draft PR against `dev`; `main` is not automatically updated.
2. Test claims must name the workflow or fixture actually executed.
   Source-path inspection and PR advisory checks do not establish full
   Windows release acceptance.
3. Protect both `schema/media-state-classification.json` and
   `schema/comp-report-policy.json` through their existing PR gate.
   Neither a packaging change nor a saved runtime revision approves a
   behavioral policy change.
4. Do not activate Workflow 01, Workflow 02, Workflow 03, Partner Center
   submission, or GHCR publication without a new explicit owner decision.

**Documentation restoration rule:** Relevant current guides can be
reconstructed from source and owner-approved policy one at a time. Do not
restore obsolete paths, discarded policies, or unverified instructions.
