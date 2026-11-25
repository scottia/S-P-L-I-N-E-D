# Release automation

SPLINED separates quick Windows evaluation, release preparation, platform
builds, publication, and Microsoft Store submission. Each operation is
independently dispatchable and retains a comprehensive GitHub Actions summary.

| Workflow | Responsibility |
| --- | --- |
| `20. Quick Portable — Windows` | Compile or report on the exact current `origin/dev` Windows source. The standalone executable is QA-only and is never a release input. |
| `21. Release Preparation` | Compare `origin/main` and `origin/dev`, audit source and policy contracts, resolve the version, and optionally retain an approved immutable release candidate. It never compiles or packages an application. |
| `22. Windows Build & Microsoft Store` | Build complete Windows portable and Store outputs from a Workflow 21 candidate, or independently submit an existing validated MSIX. |
| `23. Python / WebGUI Build` | Build and validate the Python source archive and exact smoke-tested container image from a Workflow 21 candidate. |
| `24. Complete Release & Publication` | Audit prior build artifacts, create or verify the numeric tag, publish missing GitHub Release assets, and publish the exact smoke-tested image to GHCR. It never recompiles or repackages. |

The normal release path is:

1. Merge approved source pull requests into `dev` after required validation.
2. Run Workflow 21 and select **PREPARE RELEASE CANDIDATE**. Record its run ID.
3. Run Workflow 22 and/or Workflow 23 with that exact candidate run ID.
4. Run Workflow 24 with the selected platform build run IDs.
5. If a Microsoft Store submission is required after publication, run
   Workflow 22 with **SUBMIT EXISTING MSIX** and the exact Windows build run ID.

Workflow 22 and Workflow 23 may run independently or in parallel. A failed
platform build can be rerun against the same approved candidate without
repeating a successful build for the other platform.

## Source and candidate identity

Workflow 21 is dispatched from `main`, fetches both long-lived branches, and
records their exact hashes. `main` is the workflow-definition and comparison
baseline; `dev` is the reviewed product-source candidate. The branches are not
required to have an old historical ancestry relationship. This is important
when `dev` has been recreated from the current `main` commit.

The source resolver accepts:

- the same commit or complete Git tree on both branches;
- `dev` directly advancing the current `main` commit; or
- `dev` advancing the exact source parent of a content-preserving promotion
  merge on `main`.

Any other differing-tree relationship fails before candidate preparation.
The release-candidate manifest records the exact `main` hash, exact selected
`dev` hash, relationship result, synchronized source version, release version,
target, recommended compile mode, originating run, and source-bundle SHA-256.

If the committed version is unused, the candidate points directly at the
selected source. If approved product source changed after that version was
published, Workflow 21 may create a temporary candidate commit whose parent is
the selected `dev` commit and whose only changes are the synchronized version
fields. No protected branch is written.

Workflow 21 **BUILD ONLY, REPORT** performs the same source, policy, version,
target, cache-plan, and credential-readiness audit but does not request
approval or create a candidate artifact.

## Workflow 20 quick portable

Workflow 20 builds the exact current `origin/dev` source using the same locked,
optimized Rust/Slint target and compatible Cargo caches as release builds. It
verifies build identity, version, commit, AMD64 PE architecture, native smoke,
size, and SHA-256, then retains a standalone `splined.exe` artifact.

This path reflects the repository's Quick Local Update compilation behavior
without invoking the local installer, replacing an installed executable, or
opening the application. Its manifest is explicitly `release_eligible: false`.
It never creates an MSIX, release candidate, tag, release, or Store operation.

**BUILD ONLY, REPORT** performs cache lookup and source identity reporting
without restoring payloads, compiling, packaging, or uploading an executable.

## Workflow 22 Windows build and Store submission

For **BUILD ONLY**, enter the originating Workflow 21 run ID and select one
compilation mode:

- **PARTIAL UPDATE** restores compatible Cargo dependencies and optimized
  intermediates. Cargo fingerprints still rebuild every changed input.
- **FULL SOURCE** clears target-specific release output and performs a clean
  locked optimized compilation.

Both modes produce and validate all of the following from the same executable:

- standalone portable `splined.exe` and complete ZIP;
- update identity metadata;
- unsigned production MSIX;
- isolated development-package installation/activation smoke;
- offline local Store review bundle and Highlights;
- SHA-256 ledger and build provenance manifest.

The local review path never authenticates to, queries, stages, cancels, or
modifies Partner Center. An existing pending submission cannot block it.

**BUILD ONLY, REPORT** validates the candidate and proposed cache/packaging
plan without compiling or packaging.

**SUBMIT EXISTING MSIX** ignores the compilation checkboxes. It requires the
run ID of a successful Workflow 22 build, a published GitHub release whose tag
matches that build's candidate commit, the explicit automatic-submission
selection, and protected environment approval. It verifies the build manifest
and checksums, resolves the last published Store baseline, and submits that
exact MSIX without rebuilding or repackaging it.

## Workflow 23 WebGUI build

Workflow 23 accepts the approved candidate run ID and one compilation mode.
Partial mode restores compatible Docker BuildKit layers; full mode uses a clean
container build. Both modes run the retained Python/PostgreSQL/Node package
validation, start the production image against PostgreSQL, verify database and
authentication health, retain the exact tested image, and generate the Python
source archive.

The build manifest records measured validation, container, packaging, layer
reuse, compression and artifact sizes. GHCR publication is excluded.
**BUILD ONLY, REPORT** validates the candidate and reports anticipated layer
reuse without running Docker or creating an archive.

## Workflow 24 publication

Workflow 24 requires exact Workflow 22 and/or Workflow 23 run IDs. Before its
publication approval boundary it:

- verifies every build manifest and SHA-256 entry;
- rejects mismatched candidates, versions, targets, expired artifacts, missing
  validations, and tag/source conflicts;
- generates and validates incremental release notes;
- determines which selected GitHub Release assets are missing;
- reports unsigned MSIX readiness without contacting Partner Center; and
- retains a checksummed publication-readiness artifact.

**COMPLETE RELEASE, REPORT** stops there. **COMPLETE RELEASE** requires the
protected publication approval, creates or verifies the numeric tag at the
exact candidate commit, creates the GitHub Release or adds only missing target
assets, and publishes the exact smoke-tested WebGUI image to GHCR. Existing
release assets and versioned GHCR images are never overwritten with different
content.

If publication fails after tag creation, rerun Workflow 24 with the same build
run IDs. It verifies the existing exact tag and continues only missing work;
it does not rebuild either platform.

## Reporting and validation boundaries

Every workflow reports selections, source/version identity, cache results,
durations, sizes, SHA-256 values, artifact/run provenance, MSIX status,
publication readiness, failures, and required next actions. Anticipated cache
reuse is distinguished from measured reuse.

Required source pull-request validation remains the fail-closed authority for
`AUDIO-INTEGRITY-001..006`. Release packaging consumes the exact approved
commit and does not repeat either platform's real-writer suite merely because
a distributable was selected. Canonical policy JSON review remains independent
of build and publication automation.

## Cache and performance acceptance

Benchmark Windows and WebGUI independently on the same runner image with three
scenarios: a cold full-source build with no compatible cache, a warm repeat of
the exact source and inputs, and a warm partial update after changing an
application input while retaining compatible dependencies. Run each scenario
three times and compare medians. Record cache or layer reuse, compilation,
validation, packaging, compression, artifact-transfer and total elapsed time,
plus the retained artifact sizes and checksums. Cache lookup-only reporting is
not a measured cache hit and must be labelled anticipated.

The acceptance objectives are:

- the warm-repeat compilation median is no more than 25 percent of the cold
  compilation median;
- the warm-with-source-change compilation median is no more than 75 percent of
  the cold compilation median;
- cache restore and save overhead together remain lower than the compilation
  time they avoid;
- standard Windows builds create and validate the portable files, MSIX and
  local Store review bundle once, with no repeated compilation or packaging in
  Store submission or publication;
- report-only selections invoke no compiler, container build, packager,
  release mutation or external Store mutation; and
- a cache miss changes performance, not artifact contents, validation scope or
  provenance requirements.

Accept the split only after the cold, warm-repeat and warm-with-source-change
matrix completes without artifact, checksum, validation or source-identity
regression. Retain the originating run IDs and job summaries as the benchmark
record so future runner, dependency or cache-key changes can be compared on the
same basis.

The existing Store and GHCR recovery workflows remain available for their
separately documented recovery scopes. Release notes and Store Highlights use
the pinned shared [`release/cliff.toml`](../release/cliff.toml) configuration
and checked-in templates.
