# Release automation

SPLINED has four supported GitHub Actions workflows. Only the normal release
workflow creates a version, numeric tag, or GitHub Release.

| Workflow | File | Purpose |
|---|---|---|
| `SPLINED (All OS) and GHCR` | `.github/workflows/release-next-patch.yml` | Create the next release from `main`, build the selected platforms, publish GitHub assets and GHCR, and optionally submit the Windows Store update. |
| `SPLINED (Tagged) > MS Store Package Resolution` | `.github/workflows/windows-store-package-resolution.yml` | Rebuild and validate an unsigned Store package plus Store Highlights for an existing numeric tag. It stops at an Actions artifact. |
| `SPLINED > MS Store Publish & Update` | `.github/workflows/windows-store-publish-update.yml` | Rebuild an existing numeric tag and submit its validated package and en-US Store Highlights to the live Store product. |
| `SPLINED (Docker) > GHCR Resolution` | `.github/workflows/docker-ghcr-resolution.yml` | Rebuild and publish Docker tags from an existing numeric tag, then replace that tag's existing GitHub Release body with the authoritative notes. |

Recovery workflows never create, move, or delete a tag. The Store recovery
workflows never create another GitHub Release or publish GHCR. The Docker
recovery workflow never touches Microsoft Store and never creates a second
GitHub Release.

## Normal release

Run `SPLINED (All OS) and GHCR` from `main`. Select Windows, Ubuntu, macOS, or
All. **Microsoft Store** defaults to `true`; **Extended Windows validation**
defaults to `false`.

The workflow prepares an exact untagged release commit, builds and validates
the selected targets, and only then atomically creates the numeric tag and
advances `main` to that exact release commit. The tag and `main` update are
one atomic Git push, so a successful release cannot leave its version commit
outside branch ancestry. A selected Windows build compiles the x64 WinForms
executable and Rust DLL once. The Portable ZIP
always contains that pair. With Microsoft Store enabled, the unsigned Store
MSIX is created and validated from the same pair before tagging.

- **Microsoft Store = false:** no Store package, Store QA, Store-submission
  artifact, Store authentication, or Store API submission runs.
  GitHub/Portable and GHCR publishing continue normally. The shared release-note
  context may still render its unused Store Highlights output; it is not
  published to Store.
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
messages from disappearing. `release/Generate-ReleaseNotes.ps1` asks git-cliff
for one JSON context for the previous-numeric-tag to selected-tag range. It
renders `GITHUB-RELEASE.md` and `STORE-HIGHLIGHTS.txt` from that same context
using `release/templates/github-release.tera` and
`release/templates/store-highlights.tera`. Store Highlights select the
customer-facing Highlights, Windows, Microsoft Store, Fixes, and catch-all
groups while omitting routine documentation and maintenance noise.

## Automated Store publication

The live product is Store ID `9P8G4GMBBVBS`, package
`Psycotix.SPLINED`. Store publication requires these GitHub Actions secrets:

- `AZURE_AD_APPLICATION_CLIENT_ID`
- `AZURE_AD_APPLICATION_SECRET`
- `AZURE_AD_TENANT_ID`
- `SELLER_ID`

Missing credentials fail the Store publication step clearly. Values are passed
as masked environment secrets and are not printed. The pinned Microsoft Store
publisher action installs the pinned `msstore` CLI. Package staging with
`--noCommit` occurs before metadata retrieval and editing. The complete pending
submission is retrieved and preserved; only the existing en-US
`BaseListing.ReleaseNotes` value is changed. The workflow then updates and
publishes that same pending submission without recreating the draft.

If packaging needs correction without Store submission, run
`SPLINED (Tagged) > MS Store Package Resolution`. If an existing release was
created with Store disabled or needs Store republication, run
`SPLINED > MS Store Publish & Update`. Both use an existing numeric tag and the
same git-cliff classification/context process as the normal release.
