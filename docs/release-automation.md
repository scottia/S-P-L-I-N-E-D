# Release automation

SPLINED exposes four manual GitHub Actions workflows. Only workflow 01 creates
a version, tag, or GitHub Release or performs normal GHCR publication.
Workflows 01 and 02 use pinned `git-cliff 2.14.2` for release notes or Store
Highlights; no workflow is run merely by editing these files.

| Workflow | Use | Effect |
|---|---|---|
| `01. Full Release - Build Selected Target and Publish` | New versioned release from `main`; choose All, Windows, or WebGUI / Python. | Validates the selected release targets, creates the version commit and numeric tag only after they pass, publishes the GitHub Release and GHCR, and optionally stages or submits Store. |
| `02. Store Recovery - Tagged Draft or Update` | Rebuild a Store MSIX from an **existing** numeric Git tag if workflow 01's Store path failed. | Rebuilds/validates Windows package, generates git-cliff Store Highlights, then stages a draft or submits for certification. Does not create a tag, release, or GHCR image. |
| `03. GHCR Recovery - Existing Published Release` | Safely recover missing GHCR tags for the latest **existing published** numeric release if workflow 01's GHCR path failed. | Verifies existing tag digests and OCI source/version/revision labels, performs no work when both tags are valid, and publishes only missing GHCR tags. Never changes a GitHub Release, release asset, Git tag, version, or Store submission. |
| `04. Optimized Build - Test Selected Target (No Publish)` | Test `All`, `Windows` or `WebGUI / Python` with optional native/Store QA and timed Rust compilation. | Reuses compiled Rust dependencies; can compare an isolated full cold build; produces QA-only Actions artifacts but **never publishes a release, tag, GHCR image or Store submission**. |

The previous status, authentication-diagnostic, package-only, publish-only, and
one-time 2.0.0 workflows have been retired. GitHub may retain their historical
run pages, but they are no longer runnable from the repository's default
branch. Use Workflow 01 for publishing, Workflows 02/03 for tagged recovery, and Workflow 04 for isolated testing.

## Workflow 01: full release

Run from `main`. `Release version` may be left blank for the next patch version
or set to a new numeric `X.Y.Z` version greater than the current version. The
workflow requires **Create release now** confirmation. `Extended Windows
validation` is off by default; turn it on for `Windows` or `All` to run the
Windows Rust formatting, Clippy, and library test checks. The standard Windows
build always compiles the Rust/Slint executable; the checkbox adds strict
validation rather than enabling Slint compilation. Under the owner's release
policy, Microsoft Store submissions require **Extended Windows validation** to
be enabled in addition to the existing release confirmation and explicit Store
submission choice. Do not stage or submit a Store package without those owner
confirmations. **Workflow 01 now enforces this dependency in preflight**: choosing **Submit Update/No Draft** without Extended Windows validation fails before any release metadata change. **Review Bundle/No Submit** is a separate, non-submitting review mode that still requires explicit release confirmation.

**Canonical source-contract preflight (every target):** After checking out `main` but **before** version stamping, Workflow 01 runs `.github/scripts/validate_release_policy_contracts.py` and the shared `.github/scripts/test_*.py` regression suite. This exercises both JSONs with their existing Python validators, validates Config v5 settings metadata and executes `schema/media.sql` only against an **in-memory SQLite database**. Any failure prevents creating a release commit, tag, or published artifact. The authoritative rules remain in [POLICY-REGISTER.md](POLICY-REGISTER.md) and the two protected JSONs, not in the workflow. This check neither opens live credentials nor contacts a deployed PostgreSQL database.

**Independent pull-request gate:** `.github/workflows/validate.yml` also runs **Canonical policy JSON — review gate**, blocking any change, rename, addition or deletion of either protected JSON while reporting the current contract validity. Branch rulesets must separately require the check; it does not substitute for owner approval.

The **WebGUI / Python** target is the browser application, not a native Ubuntu
Rust build. It runs the Python/PostgreSQL suite, validates browser JavaScript,
builds and smoke-tests the container against PostgreSQL, and publishes a
WebGUI/Python source archive with the release. `All` runs that validation along
with Windows. GitHub release notes classify explicit `webgui`,
`python`, and `postgres` commits under **WebGUI / Python** before evaluating
the separate Microsoft Store and Windows categories. A release fixture guards
against classifying `restore` as Store or `webgui` as generic Windows GUI work.
The [Docker deployment guide](webgui-docker-deployment.md) documents
`build/WEBGui/docker/`, the repository-root Compose build context, canonical
`schema/` dependencies, and the distinction between source-built Compose and
the separately published GHCR image.

Validate changes on a short-lived feature branch based on `main`, open a
Draft PR targeting `main` before implementation, complete all applicable PR
checks, and obtain explicit owner approval for the specific merge. Repository
promotion and publication remain distinct; no PR merge alone authorizes a
versioned release.

Checks follow changed files: WebGUI, shared-contract, and release-automation
changes run Python/PostgreSQL, JavaScript, and container checks. The archived
root-native CLI no longer has an active Rust validation lane. Windows retains
the separate advisory PR check and Workflow 01 release validation. Strict Windows Rust/Slint validation is never executed automatically
for pull requests targeting `main`, including Windows source, Slint,
shared-schema, dependency, and workflow changes.
Those scopes still run every other applicable validation lane.

GitHub retains the **Windows Rust/Slint — advisory** check name for branch-rule
compatibility. It runs only a lightweight Ubuntu report with the outcome
**Not run — permanent PR policy exception**. It does not provision a Windows
runner or Rust toolchain and does not execute Cargo formatting, `cargo check`,
Clippy, or Windows regression tests. An unexecuted diagnostic is never reported
as passed. This standing policy remains in effect until the repository owner
explicitly changes it.

Useful targeted Windows compilation and regression testing remains part of the
isolated E: acceptance workflow. For a Windows release, Workflow 01 retains the
optional **Extended Windows validation** suite against the exact versioned
source before packaging. That suite owns strict formatting, Clippy, and Windows
library tests; the ordinary Windows release path still performs its clean
optimized production Rust/Slint build when the option is off.

For routine PC iterations, close SPLINED and double-click
`E:\apps\SPLINED\Apply Quick Local Update.cmd`. It reuses unchanged optimized
Cargo objects, verifies and installs the complete executable, and reopens the
app. Before repository promotion or Microsoft Store preparation, close SPLINED
and double-click
`E:\apps\SPLINED\Apply Local Test Build.cmd`. The helper clears the
target-specific release output, disables Cargo incremental code generation,
uses the same locked optimized build command as the GitHub Windows release,
verifies and installs the local executable, and reopens the app without changing
Git or starting Actions. An
unoptimized Windows build is not used against the real library because its UI
refresh cost can make Windows report the application as unresponsive. This
local test binary is never a release input. After explicit owner approval, merge
the tested topic PR into `main`; pull requests publish the permanent-exception
compatibility report rather than Windows diagnostics.
Workflow 01 always performs a clean optimized build from the versioned `main`
source before producing the portable archive or Store MSIX.

macOS is deliberately absent from Workflow 01. Development-only macOS checks
belong to the `dev-macos` branch's dedicated workflow and do not create SPLINED
tags, releases, or published assets.

The two Store options are both off by default and mutually exclusive:

- **Submit Update/No Draft:** build and validate the Store MSIX, stage it with
  Partner Center, update en-US Highlights, then immediately submit it for
  certification. The Store CLI creates a draft internally, but there is no
  manual draft-review pause.
- **Review Bundle/No Submit:** build, validate, and smoke-test the MSIX; read
  the last published Partner Center package history; generate cumulative Store
  Highlights and the submission audit JSON; and retain both downloadable Store
  artifacts. This mode never stages a draft or submits for certification.

Either Store option requires `Windows` or `All`. Selecting both, or requesting
Windows-only validation for a non-Windows build, fails before release work.
Every `Windows` or `All` run also builds, strictly validates, development-smoke
tests, and uploads the unsigned `splined-windows-store-package` Actions artifact
for manual Microsoft Store submission. Selecting **Review Bundle/No Submit**
also uploads `splined-windows-store-submission`, containing the exact MSIX,
`STORE-HIGHLIGHTS.txt`, and `STORE-SUBMISSION.json`. It authenticates only to
read the published Store baseline and performs no staging or submission.
Leaving both Store options off skips Partner Center authentication, history
lookup, staging, and submission without suppressing the package-only artifact.
Store and GHCR publication are independent after GitHub Release creation, so
one can succeed if the other fails.

The release commit and numeric Git tag are published atomically after all
selected builds pass. Before the tag exists, workflows 02 and 03 cannot recover
the release; rerun workflow 01 after correcting the build failure. Once the tag
and GitHub Release exist, use the appropriate tagged recovery workflow.

Git release tags are three-part numeric versions, such as `2.0.0`. The Windows
MSIX manifest uses the corresponding four-part version, `2.0.0.0`. Workflow
02 checks that correspondence before staging anything.

The unsigned Store MSIX is never attached to the public GitHub Release. Every
Windows/All run retains the validated MSIX in the Actions artifact
`splined-windows-store-package`. Selecting either Store mode additionally
retains `splined-windows-store-submission` with the exact MSIX, Highlights, and
submission metadata. The review mode stops after uploading those artifacts;
only **Submit Update/No Draft** stages and submits the package.

## Workflow 04: optimized build tests — no publication

[Workflow 04](../.github/workflows/04-optimized-build-test.yml) is a **manually dispatched, nonpublishing QA workflow**. It leaves the exact production Workflow 01 release pipeline unchanged. Select the same **All**, **Windows**, or **WebGUI / Python** platform as Workflow 01. **All** runs Windows and WebGUI validation in separate jobs. The equivalent `Extended Windows validation` toggle executes formatting, Clippy, and Rust library tests. The `Submit-mode` and `Review-mode` Store selections are **test-only simulations**: they create and validate unsigned Store-identity MSIX packages, development-sign/install/native-smoke/uninstall only on the isolated Windows runner, and upload QA-labelled Actions artifacts. They never authenticate to Partner Center, stage a draft, or submit an update. Both Store QA choices together are rejected; Submit-mode QA requires Extended Windows validation.

The familiar `Release version` input is a **test label only**: unlike Workflow 01, it does not change source or create a release version. The safety confirmation reads **Run optimized test only** and defaults to **Stop - no test**. The additional `Benchmark mode` choices are:

- **Optimized only (default):** reuse downloaded and compiled Windows release dependencies from a cache keyed to the pinned Rust toolchain/target, normalized locked dependencies and package manifest. Clean only `splined-windows`, then compile a **fresh**, locked optimized Rust/Slint executable for the exact selected SHA. Verify x64 PE, build version and commit, SHA-256, and clean native smoke behavior.
- **Cold and optimized:** perform the optimized build and also build the complete Rust graph in an isolated temporary target directory for timing comparison. This adds considerable runner time but does not replace or contaminate the cached build.

The first invocation may be a **cold cache miss** and provide no improvement. Repeat with the same dependency graph to measure a real cache hit, comparing the elapsed compile times, optional isolated cold baseline, and cache restore/upload overhead in the Actions job summary. Cached files are **compiler artifacts, never trusted final application binaries**: the project package is rebuilt and its executable identity checked on every test. QA binaries are downloadable **from the workflow run only**, labelled **NOT A RELEASE**. Caches may be costly to transfer or evicted, so judge success by measured end-to-end job duration rather than assuming guaranteed speedups.

This workflow has only `contents: read` permission and no release/registry/Partner Center publishing steps. It also runs the same canonical source-policy preflight as Workflow 01, plus the WebGUI Python/Node/PostgreSQL/Docker suite for applicable selections. It **does not replace Workflow 01's strict clean production build, release-tag checks, Store compliance, or owner authorization**. A newly added manual workflow requires an owner-approved merge onto the default branch before it appears as a runnable Actions dispatch.

## Workflow 02: Store recovery

Enter the existing numeric tag `X.Y.Z` and select exactly one of Workflow 02's
Partner Center modes: stage a draft or submit an update. Workflow 02 checks out
that tag, verifies its source and Store
manifest version, rebuilds and validates the Windows package, resolves the
last **published** Store baseline, and renders cumulative Store Highlights
with git-cliff before staging. It cannot silently substitute a different tag
or published version. Draft mode stops before certification submission;
Update mode submits after staging.

Automated Store publication is serialized across workflows 01 and 02. Workflow
01's **Submit Update/No Draft** mode and both Workflow 02 modes fail closed if
Partner Center already has a pending or unresolved submission. If an automated
update fails after internally staging its package, neither workflow overwrites
or cancels the resulting draft. Inspect and resolve that submission in Partner
Center before rerunning recovery. This is deliberate: an ambiguous active
submission cannot safely be identified as a retry target. Workflow 01's
**Review Bundle/No Submit** mode may still read the last published baseline
while a pending submission exists because it cannot stage, replace, cancel, or
submit that pending work.

## Workflow 03: GHCR-only recovery

Enter the existing numeric tag `X.Y.Z`. Workflow 03 requires that tag to have a
published, non-prerelease GitHub Release and refuses any version other than the
current latest release. It shares workflow 01's release concurrency group so
normal publication and recovery cannot race each other.

Workflow 01 remains the sole normal GHCR publisher. Workflow 03 inspects
`ghcr.io/scottia/splined:vX.Y.Z` and `:latest` before doing any recovery:

- If both tags exist, have the expected source/version/revision OCI labels, and
  resolve to the same manifest digest, recovery is a no-op.
- If exactly one tag is missing, the verified existing image is reused and only
  the missing tag is published.
- If both tags are missing, recovery fails closed. Workflow 01 does not record a
  published manifest digest or signed build attestation, and its historical
  container inputs are not fully immutable, so an equivalent rebuild cannot be
  proven. A potentially different image is never published under an existing
  release version; create a new versioned release instead.
- A retry after a partial failure reuses the one verified tag that succeeded and
  publishes only the other tag instead of rebuilding or overwriting either one.
- If an existing tag has missing or unexpected provenance labels, the two tags
  have different digests, or the registry cannot prove a target tag is absent,
  recovery fails closed without overwriting either tag.

The OCI source, release version, release commit, and registry digest are the
available provenance checked by workflows 01 and 03. Workflow 01 does not emit
a separately verifiable signed GHCR build attestation, so workflow 03 reports
that limitation rather than claiming signed provenance.

Workflow 03 never creates or edits a GitHub Release, moves or creates a Git tag,
or adds, replaces, or deletes release assets. Published release assets are final
when Release Immutability is enabled. A missing or defective archive or checksum
therefore requires a new versioned release; it cannot be repaired in place.

## Notes and Store safety

The shared [`release/cliff.toml`](../release/cliff.toml) and checked-in Tera templates classify release changes.
GitHub notes cover the previous numeric tag through the current tag. Store
Highlights cover the **last published Store package** through the target tag,
so intervening GitHub-only releases are included. The Store Highlights file
must fit the 1,500-character Store limit. If the complete cumulative history
exceeds that boundary, generation retains the highest-priority complete
bullets that fit and appends the target GitHub Release link. It never cuts a
bullet or sentence mid-text. `STORE-SUBMISSION.json` records the source and
final character counts, candidate/emitted/omitted bullet counts, whether
compaction occurred, the target, published baseline, range, git-cliff version,
and Highlights checksum.

The active scripts, templates, and intentionally historical Store regression
fixtures are inventoried in the [release support-file guide](../release/README.md).

Store product `9P8G4GMBBVBS` uses the repository's
`AZURE_AD_APPLICATION_CLIENT_ID`, `AZURE_AD_APPLICATION_SECRET`,
`AZURE_AD_TENANT_ID`, and `SELLER_ID` Actions secrets. The publisher retrieves
the pending submission after staging and changes only its existing en-US
`BaseListing.ReleaseNotes`, preserving all other listing and package fields.
No release workflow should be used as a read-only status check; inspect Partner
Center directly for certification progress.
