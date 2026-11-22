# Release automation

SPLINED keeps source review, branch promotion, compilation and publication as
separate owner actions.

1. Accumulate application and version changes through reviewed pull requests
   into `dev`.
2. Close each source pull request by merging it to `dev` after its required
   checks and owner approval.
3. Promote the exact approved `dev` tree to `main` through the repository's
   promotion pull request.
4. Run Workflow 10 from `main` to build the committed source version.

Workflow 10 never creates a pull request, writes to `dev`, stamps a version, or
pushes a source commit to `main`. The Windows and WebGUI source versions must
already agree on one numeric `X.Y.Z` version in the promoted source.

| Workflow | Purpose |
|---|---|
| `01. Full Release - Build Selected Target and Publish` | Retained original release and optional Store-submission path. |
| `02. Store Recovery - Tagged Draft or Update` | Rebuild Store material from an existing numeric tag. |
| `03. GHCR Recovery - Existing Published Release` | Restore a missing GHCR tag for an existing published release when provenance can be proved. |
| `10. Unified Source Build - Full or Incremental Windows and WebGUI` | Build, validate and optionally publish the exact source already promoted to `main`. |

The former Workflow 04 preset launcher and Workflow 05 source-PR generator are
retired. GitHub retains their historical run pages, but they are no longer
runnable from the repository.

## Workflow 10

[Workflow 10](../.github/workflows/10-unified-build.yml) exposes three choices:

- **Target:** `ALL`, `WINDOWS`, or `WEBGUI`.
- **Compile mode:** `FULL SOURCE` or `PARTIAL UPDATE`.
- **Outcome:** `CREATE OR COMPLETE RELEASE` or `BUILD ONLY`.

The first job resolves the complete plan from the current committed source and
checks for an existing release. The `workflow-10-plan-approval` environment is
the manual approval boundary before compilation. Repository administrators must
configure that environment with required reviewers before the first run;
an environment name alone does not mechanically create a pause. Once configured,
rejecting the plan performs no build, tag, release, registry or Store work.

### Full source and partial update

Both modes produce complete distributables. A partial update is never a binary
patch or a collection of changed files.

- **FULL SOURCE** performs a clean Windows Cargo release compile and a no-cache
  WebGUI container build.
- **PARTIAL UPDATE** lets Cargo and Docker BuildKit restore compatible compiled
  intermediates. Their fingerprints invalidate changed inputs, and the jobs
  still stage and audit the complete Windows ZIP/MSIX or WebGUI archive/image.

Windows reports dependency-cache status, compiled-cache status and elapsed
compile time. WebGUI stores reusable BuildKit layers. The exact WebGUI image
that passes the PostgreSQL health smoke is saved as an Actions artifact; GHCR
publishes that image instead of rebuilding it.

### Current source and release completion

Workflow 10 reads the version from both `build/Windows/Cargo.toml` and
`build/WEBGui/webgui/backend/splined.py`; disagreement fails before building.
It does not accept an entered version and does not invent a next version.

For a new version, the numeric tag points directly at the unchanged promoted
`main` commit. The workflow never adds a release-preparation commit to `main`.

If the same version already has a GitHub Release, Workflow 10 may complete a
missing target:

- an existing Windows archive suppresses the Windows build;
- an existing WebGUI source archive suppresses the WebGUI build;
- an explicitly selected target that already exists fails as a safe no-op;
- existing release assets and GHCR version tags are never overwritten;
- the tagged product source must match current `main` under `build/Windows/`,
  `build/WEBGui/` and `schema/`, or a new numeric source version is required.

This supports Windows-first or WebGUI-first publication without rebuilding the
completed target. A completion upload receives a target-specific checksum file
and leaves the existing checksum asset untouched.

`BUILD ONLY` builds the selected current version and retains checksum-verified
Actions artifacts without creating a tag, GitHub Release, GHCR tag or Store
write. If that version is already published, the exact tagged source is used.

### Publication boundaries

`WINDOWS` produces the portable ZIP, update manifest, unsigned MSIX and local
Store review ZIP. Workflow 10 never contacts Partner Center or submits a Store
draft. `WEBGUI` produces the source archive and exact smoke-tested container
image and publishes GHCR only for a publishing run. `ALL` performs both paths
in parallel.

Workflow 10 relies on required pull-request checks already completed for the
exact source before promotion. It does not rerun either real-writer Audio
Integrity suite. Its package-level WebGUI checks explicitly omit those required
writer cases; Workflow 01 and required PR validation remain the fail-closed
`AUDIO-INTEGRITY-001..006` gates.

## Recovery workflows

Workflow 02 is the Store recovery path for an existing numeric tag. Workflow 03
can restore only a missing GHCR tag when an existing image proves the expected
source/version/revision. Neither recovery workflow changes source, policy,
release assets or Git tags.

Release notes and Store highlights continue to use the pinned shared
[`release/cliff.toml`](../release/cliff.toml) configuration and checked-in Tera
templates. Protected policy JSON review remains independent of release
packaging.
