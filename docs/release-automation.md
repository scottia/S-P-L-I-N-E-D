# Release automation

SPLINED has four supported GitHub Actions workflows. Only the normal release
workflow creates a version, numeric tag, or GitHub Release.

| Workflow | File | Purpose |
|---|---|---|
| `02. Store - Build Package Only` | `.github/workflows/windows-store-package-resolution.yml` | Rebuild and validate an unsigned Store package plus Store Highlights for an existing numeric tag. It stops at an Actions artifact. |
| `03. GHCR - Rebuild Published Tag` | `.github/workflows/docker-ghcr-resolution.yml` | Rebuild and publish Docker tags from an existing numeric tag, then replace that tag's existing GitHub Release body with the authoritative notes. |
| `04. Release - Build/Publish (Store auto-submit default)` | `.github/workflows/release-next-patch.yml` | Create the next release from `main`, build the selected platforms, publish GitHub assets and GHCR, and optionally submit the Windows Store update. |
| `05. Store - Submit Package (Auto-submit)` | `.github/workflows/windows-store-publish-update.yml` | Publish an already validated retained Store artifact for an existing numeric tag and update its en-US Store Highlights. It never rebuilds binaries or MSIX packages. |

`01. Store - Check Status (Read-only)` is a separate diagnostic workflow; it
does not create a release, package, or Store submission.

Recovery workflows never create, move, or delete a tag. The Store recovery
workflows never create another GitHub Release or publish GHCR. The Docker
recovery workflow never touches Microsoft Store and never creates a second
GitHub Release.

## Normal release

Run `04. Release - Build/Publish (Store auto-submit default)` from `main`. Select Windows, Ubuntu, macOS, or
All. **Microsoft Store** defaults to `true`; **Extended Windows validation**
defaults to `false`. Leave **Release version** blank for the automatic next
version, or enter an explicit numeric version. The current
`1.0.99` release line has one intentional exception: leaving the field blank
selects `2.0.0`; after that, a blank field resumes normal patch increments
(`2.0.1`, and so on). An explicit version must exceed the current source
version and numeric tags reachable from `main`. Historical tags on divergent
branches do not advance that baseline, but an existing tag with the exact
requested version still prevents a duplicate. The workflow—not the
maintainer's branch merge—updates all release metadata and creates the tag
after validation.

The workflow prepares an exact untagged release commit, builds and validates
the selected targets, and only then atomically creates the numeric tag and
advances `main` to that exact release commit. The tag and `main` update are
one atomic Git push, so a successful release cannot leave its version commit
outside branch ancestry. A selected Windows build compiles the x64 Rust/Slint
executable once. The Portable ZIP
always contains that pair. With Microsoft Store enabled, the unsigned Store
MSIX is created and validated from the same pair before tagging.

- **Microsoft Store = false:** no Store package, Store QA, Store-submission
  artifact, Store authentication, or Store API submission runs.
  GitHub/Portable and GHCR publishing continue normally. No Store history is
  queried and no Store Highlights are rendered.
- **Microsoft Store = true** for Windows or All: the workflow creates and
  validates `SPLINED-x64-store-unsigned.msix`, publishes the GitHub Release and
  GHCR, stages the exact MSIX with Microsoft, updates only the existing en-US
  `ReleaseNotes` field, and submits the draft for Store processing.

The Store MSIX is never attached to the public GitHub Release and is never
included in public checksums. For audit/recovery, Actions retains
`splined-windows-store-submission`, containing:

```text
SPLINED-x64-store-unsigned.msix
STORE-HIGHLIGHTS.txt
STORE-SUBMISSION.json
```

A successful Store step means the Store API accepted the package and metadata
submission for processing. The workflow reports the returned status but does
not wait indefinitely for certification.

## Release notes: git-cliff 2.14.2

`git-cliff` is the only release-note and highlight generator. CI downloads the
official `git-cliff 2.14.2` archive selected for the runner, validates its pinned
SHA-256, and fails unless `git-cliff --version` reports exactly `2.14.2`.
Repology is a package/version reference only and is not queried by CI.

`cliff.toml` is the sole classification authority. It recognizes conventional
and direct SPLINED commits and assigns every commit to one of:

- `✨ Highlights`
- `🪟 Windows`
- `🛍️ Microsoft Store`
- `🐧 Linux / Ubuntu`
- `🍎 macOS`
- `🐳 Docker / GHCR`
- `🛠️ Fixes & Reliability`
- `📚 Documentation`
- `🔧 Maintenance`
- `📦 Other Changes`

The unconditional `📦 Other Changes` parser prevents unconventional commit
messages from disappearing. GitHub Release notes and Store Highlights use the
same pinned git-cliff executable, `cliff.toml` classification, and checked-in
Tera templates, but deliberately use different release ranges:

- `release/Generate-ReleaseNotes.ps1` creates the incremental GitHub context
  from the immediately previous numeric Git tag on the selected release's
  ancestry to the selected tag.
- `release/Resolve-StoreBaseline.ps1` queries the live product before package
  staging and resolves the last **published** Store package version to its
  matching numeric Git tag.
- `release/Generate-StoreHighlights.ps1` creates the Store context from that
  published Store tag through the selected tag. Therefore, GitHub-only releases
  are included automatically the next time a Store update is submitted.

For example, if Store version `1.0.71` is published and `1.0.72` through
`1.0.78` are GitHub-only, a `1.0.79` Store submission renders
`1.0.71..1.0.79`. A failed or rejected Store submission does not advance the
baseline; only a published package does. Merge and release-preparation noise is
excluded from Store Highlights. The complete customer-facing result must fit
the Microsoft Store **What's new** limit of 1,500 characters; generation fails
instead of silently truncating history.

`STORE-SUBMISSION.json` records the Store product, published baseline package,
target version and commits, exact range, git-cliff version, character count, and
Highlights SHA-256. Publication verifies this audit record before staging.

## Automated Store publication

The live product is Store ID `9P8G4GMBBVBS`, package
`Psycotix.SPLINED`. Store publication requires these GitHub Actions secrets:

- `AZURE_AD_APPLICATION_CLIENT_ID`
- `AZURE_AD_APPLICATION_SECRET`
- `AZURE_AD_TENANT_ID`
- `SELLER_ID`

Missing credentials fail the Store publication step clearly. Values are passed
as masked environment secrets and are not printed. The pinned Microsoft Store
publisher action installs the pinned `msstore` CLI. Before staging, the workflow
uses machine-readable application metadata to require no pending submission,
identify `lastPublishedApplicationSubmission`, and read its package version.
Any pending, certification, failed, or otherwise unresolved submission stops
the workflow so a draft cannot be replaced and cannot be mistaken for
release-history authority. Package staging with
`--noCommit` occurs before metadata retrieval and editing. The complete pending
submission is retrieved and preserved; only the existing en-US
`BaseListing.ReleaseNotes` value is changed. The workflow then updates and
publishes that same pending submission without recreating the draft. Normal and
recovery publication jobs also share one non-canceling Store concurrency group,
so two SPLINED submissions cannot stage over each other.

If packaging needs correction without Store submission, run
`02. Store - Build Package Only`. That workflow rebuilds,
validates, queries Store history read-only, generates cumulative Highlights,
and retains `splined-windows-store-submission`. It requires the same Store
credentials to identify the published baseline, but it never stages or submits
a package.

`05. Store - Submit Package (Auto-submit)` is publication-only. It authenticates
before doing package work, resolves the live published baseline, locates a
retained non-expired `splined-windows-store-submission` artifact, verifies the
MSIX identity and requested numeric version, and regenerates
`STORE-HIGHLIGHTS.txt` from current Store history immediately before submission.
A stale retained Highlights file is never trusted. It never compiles
Rust/Slint executable or rebuilds an MSIX. If no matching retained package exists, it
fails and directs the operator to run the tagged Store Package Resolution
workflow first.
