# Release automation

SPLINED exposes five manual GitHub Actions workflows. **Workflow 05 prepares
the next committed Windows/WebGUI source version in a Draft PR; Workflow 04
packages or releases only an already-approved committed version.** Workflow 01
retains the original full-release path, and Workflows 02/03 recover existing
releases.
Workflows 01 and 02 use pinned `git-cliff 2.14.2` for release notes or Store
Highlights; no workflow is run merely by editing these files.

| Workflow | Use | Effect |
|---|---|---|
| `01. Full Release - Build Selected Target and Publish` | New versioned release from `main`; choose All, Windows, or WebGUI / Python. | Validates the selected release targets, creates the version commit and numeric tag only after they pass, publishes the GitHub Release and GHCR, and optionally stages or submits Store. |
| `02. Store Recovery - Tagged Draft or Update` | Rebuild a Store MSIX from an **existing** numeric Git tag if workflow 01's Store path failed. | Rebuilds/validates Windows package, generates git-cliff Store Highlights, then stages a draft or submits for certification. Does not create a tag, release, or GHCR image. |
| `03. GHCR Recovery - Existing Published Release` | Safely recover missing GHCR tags for the latest **existing published** numeric release if workflow 01's GHCR path failed. | Verifies existing tag digests and OCI source/version/revision labels, performs no work when both tags are valid, and publishes only missing GHCR tags. Never changes a GitHub Release, release asset, Git tag, version, or Store submission. |
| `04. Optimized Release or Test - Select One of Four Build Presets` | Choose **ALL** (default), **PYTHON / WEBGUI**, **WINDOWS**, or **TEST BUILD**. | Packages or publishes the exact version already committed to approved source. It never advances the source version. The three release presets publish their selected outputs; TEST BUILD performs strict disposable validation without publication. |
| `05. Prepare Next Source Version - Draft PR and Compile` | Start the next patch source cycle after `main` and `dev` are synchronized. | Calculates the next patch, opens a Draft PR to `dev` before stamping, advances every Windows/WebGUI version field together, dispatches required validation, and compiles both candidates. Never merges, tags, releases, publishes GHCR, or contacts Microsoft Store. |

The previous status, authentication-diagnostic, package-only, publish-only, and
one-time 2.0.0 workflows have been retired. GitHub may retain their historical
run pages, but they are no longer runnable from the repository's default
branch. Use Workflow 01 for the original publishing path, Workflows 02/03 for
tagged recovery, Workflow 04 for a preset-driven optimized release or strict
disposable test build, and Workflow 05 for the source-version step that must
precede a new release.

## Workflow 01: full release

Run from `main`. `Release version` may be left blank to use the version already
committed by an approved Workflow 05 source PR, or set to that same numeric
`X.Y.Z` value. It cannot invent a newer version during packaging. The workflow
requires **Create release now** confirmation. `Extended Windows
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

An exact same-repository `dev` to `main` promotion does not repeat the source,
WebGUI, Windows advisory, or real-writer Audio Integrity suites that already
passed before the commit entered `dev`. That promotion runs one provenance
check instead: the PR head must be the current `origin/dev` commit, its base
must be the current `origin/main` commit, and the head repository must be this
repository. Because prior promotions create merge commits only on `main`, the
branches do not need linear ancestry. Instead, a conflict-free computed merge
of the two current heads must produce exactly the `dev` tree; this rejects stale
heads, conflicts, and any `main` content omitted from `dev`. Feature/source PRs
and Workflow 05 candidate dispatches continue to run the complete validation
workflow.

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

## Workflow 04: four optimized build presets

[Workflow 04](../.github/workflows/04-optimized-build-test.yml) contains four
launch flows in one manual form. The selected flow supplies target, strictness,
Store, GHCR and final-action defaults. Every individual control remains visible
under the selector with **Use preset default**, **Yes** and **No** (or the
equivalent target/action choices), so an owner can review or deliberately
override the flow before pressing Run. The first job prints both the form
selection and complete effective plan before expensive build jobs start.

The repository default branch, `main`, is the default GitHub branch selector.
Every publishing preset also enforces `main`; **TEST BUILD** may run from a
Draft PR branch to validate an unmerged Workflow 04 change. Release Version is
populated with the current compiled Windows/WebGUI source version. A contract
test requires the form default to advance with those source versions, and the
release preflight proves it is the next unused numeric tag. Any no-release
selection reports a disposable version while still compiling the source's
embedded numeric identity.

| Preset | Target | Strict | Submit update | Store review bundle | Publish GHCR | Final action |
|---|---|---:|---:|---:|---:|---|
| **ALL** (default) | Windows + WebGUI | No | No | Yes | Yes | Create release now |
| **PYTHON / WEBGUI** | WebGUI | No | No | No | Yes | Create release now |
| **WINDOWS** | Windows | No | No | Yes | No | Create release now |
| **TEST BUILD** | Windows + WebGUI | Yes | No | No | No | No release creation |

ALL is the routine default release preset. Workflow 04 never invents or stamps
an application version: its visible Release Version must equal the version
already committed by the approved source-version PR.

The three publishing presets execute the selected production release graph:
canonical preflight, target builds, numeric tag promotion, git-cliff notes and
GitHub Release. GHCR and Store review behavior then follows the table. In
Workflow 04, **Review Bundle/No Submit is entirely local to GitHub Actions**:
it never authenticates to or queries Partner Center and never stages, replaces,
cancels, or submits a Store draft. It retains the validated MSIX, bounded
Highlights, `STORE-SUBMISSION.json`, and the release-context JSON together in
the downloadable `splined-windows-store-submission` artifact ZIP. Its JSON
explicitly records that Partner Center was not contacted and no Store write was
performed. **Submit Update/No Draft** is the only Workflow 04 selection that
uses Store credentials or the Store CLI. None of the presets automatically
selects that riskier path.

TEST BUILD performs canonical preflight, both target builds, strict Windows
format/Clippy/library validation, WebGUI/PostgreSQL checks, QA-only
release notes and checksum artifacts, plus a local release-channel container
label check. Its temporary commit/tag never reaches `main` or remote tags; it
does not create a GitHub Release, push GHCR, or contact Partner Center.

Workflow 04 does not rerun the Windows or WebGUI real-writer Audio Integrity
gate. Those suites must already have passed for both clients in the required
PR checks on the exact source commit before promotion. Broader Workflow 04
test commands explicitly exclude the same real-writer cases rather than
silently executing the gate a second time.

### Build-time optimization and verification

Windows retains Workflow 01's Cargo registry/Git download cache and adds a separate cached **compiled release dependency graph**, keyed to the pinned Rust toolchain, Windows MSVC target and version-normalized `Cargo.lock`. The production lane reports exact cache hits and elapsed compile time and still compiles a **fresh application executable for the exact staged commit** while Cargo fingerprints invalidate changed dependencies. Strict validation retains the remaining locked library checks while excluding the real-writer cases already required on the source PR. The original PE x64, version/commit identity, clean native-smoke and portable/MSIX archive checks remain in place.

WebGUI runs its unsharded Python/PostgreSQL/Node package-validation suite concurrently with the original Docker build/health and source-archive job. The four real-writer Audio Integrity files already required by PR validation are excluded from this second pass. Both jobs have independent disposable PostgreSQL services; both must pass before Yes mode can publish or No mode can report success.

Workflow 04 reports compiled-cache hits, Rust compile duration, independent WebGUI outcomes and the final stage results. An initial cache miss can still take as long as a cold compile; compare repeated successful runs and cache-transfer overhead before declaring a speed improvement. **No native Windows speedup is established solely by pull-request CI**: the PR Windows lane is advisory, and the full optimized workflow requires an owner-authorized manual dispatch after merge.

Workflow 01 itself remains unchanged. Selecting one of Workflow 04's three
release presets is the explicit publication choice; both release workflows
share the same publication concurrency group to avoid simultaneous release
commits. TEST BUILD is the nonpublishing choice. The two protected behavioral
policy JSONs and [approved register](POLICY-REGISTER.md) remain normative.

## Workflow 05: prepare and compile the next source version

[Workflow 05](../.github/workflows/05-prepare-source-version.yml) separates
source preparation from release packaging. Run it from `main` only after
`origin/main` and `origin/dev` identify the same commit and the current source
version equals the latest numeric tag on `main` history. It advances exactly
one patch version; operators do not type or guess the next number.

The workflow creates a topic branch and a minimal Draft PR targeting `dev`
before changing versioned source. It then updates Cargo metadata, both Windows
manifests, the App Installer template, the WebGUI version and user agent, and
Workflow 04's visible default as one source commit. A mismatch in any of those
fields fails closed.

Because a PR created with the workflow's GitHub token cannot recursively start
another Actions run, Workflow 05 explicitly dispatches the repository's normal
required validation workflow against the exact candidate SHA. That dispatch
uses the same base/head selection, protected-policy gate, both Audio Integrity
writers, WebGUI/PostgreSQL checks, and Windows advisory contract as a normal
source PR. Workflow 05's own candidate compilation does not duplicate Audio
Integrity: it builds the locked optimized Rust/Slint executable, verifies its
version/commit identity, builds and health-checks the WebGUI/PostgreSQL
container, and retains QA-only artifacts.

The PR remains Draft regardless of compilation outcome. A failure leaves the
branch and Draft PR available for diagnosis. Success still requires owner
review, merge to `dev`, separate promotion to `main`, and a later explicit
Workflow 04 release. Workflow 05 has no tag, GitHub Release, GHCR, Store,
automatic readiness, automatic merge, or protected-branch write path.

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

Workflows 01 and 04 presets with **Publish GHCR = Yes** are normal GHCR
publishers. Workflow 03 inspects
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
