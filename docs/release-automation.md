# Release automation

SPLINED exposes three manual GitHub Actions workflows. Only workflow 01 creates
a version, tag, or GitHub Release. All three use pinned `git-cliff 2.14.2` for
release notes or Store Highlights; no workflow is run merely by editing these
files.

| Workflow | Use | Effect |
|---|---|---|
| `01. Full Release - Build Selected OS and Publish` | New versioned release from `main`; choose All, Windows, macOS, or Ubuntu. | Builds the selected OS targets, creates the version commit and numeric tag only after selected builds pass, publishes GitHub Release and GHCR, and optionally stages or submits Store. |
| `02. Store Recovery - Tagged Draft or Update` | Rebuild a Store MSIX from an **existing** numeric Git tag if workflow 01's Store path failed. | Rebuilds/validates Windows package, generates git-cliff Store Highlights, then stages a draft or submits for certification. Does not create a tag, release, or GHCR image. |
| `03. GHCR Recovery - Tagged Image and Ubuntu Asset` | Repair GHCR for an **existing** numeric Git tag if workflow 01's GHCR path failed. | Rebuilds/pushes version and latest GHCR images, adds a missing Ubuntu asset to that existing GitHub Release, and refreshes git-cliff notes. Does not touch Store. |

The previous status, authentication-diagnostic, package-only, publish-only, and
one-time 2.0.0 workflows have been retired. GitHub may retain their historical
run pages, but they are no longer runnable from the repository's default
branch. Pin the three numbered workflows for routine use.

## Workflow 01: full release

Run from `main`. `Release version` may be left blank for the next patch version
or set to a new numeric `X.Y.Z` version greater than the current version. The
workflow requires **Create release now** confirmation. `Extended Windows
validation` is off by default; turn it on for `Windows` or `All` to run the
Windows Rust formatting, Clippy, and library test checks. The standard Windows
build always compiles the Rust/Slint executable; the checkbox adds strict
validation rather than enabling Slint compilation.

The two Store options are both off by default and mutually exclusive:

- **Submit Update/No Draft:** build and validate the Store MSIX, stage it with
  Partner Center, update en-US Highlights, then immediately submit it for
  certification. The Store CLI creates a draft internally, but there is no
  manual draft-review pause.
- **Submit Draft/No Update:** build and validate the same MSIX and Highlights,
  then stage them in Partner Center and stop. Review and submit that draft
  manually when ready.

Either Store option requires `Windows` or `All`. Selecting both, or requesting
Windows-only validation for a non-Windows build, fails before release work.
Leaving both Store options off produces GitHub Release and GHCR outputs only.
Store and GHCR publication are independent after GitHub Release creation, so
one can succeed if the other fails.

The release commit and numeric Git tag are published atomically after all
selected builds pass. Before the tag exists, workflows 02 and 03 cannot recover
the release; rerun workflow 01 after correcting the build failure. Once the tag
and GitHub Release exist, use the appropriate tagged recovery workflow.

Git release tags are three-part numeric versions, such as `2.0.0`. The Windows
MSIX manifest uses the corresponding four-part version, `2.0.0.0`. Workflow
02 checks that correspondence before staging anything.

The unsigned Store MSIX is never attached to the public GitHub Release. The
full-release workflow retains an Actions audit artifact named
`splined-windows-store-submission` with the validated MSIX, Highlights, and
submission metadata.

## Workflow 02: Store recovery

Enter the existing numeric tag `X.Y.Z` and select exactly one of the same two
Store modes. Workflow 02 checks out that tag, verifies its source and Store
manifest version, rebuilds and validates the Windows package, resolves the
last **published** Store baseline, and renders cumulative Store Highlights
with git-cliff before staging. It cannot silently substitute a different tag
or published version. Draft mode stops before certification submission;
Update mode submits after staging.

Store publication is serialized across workflows 01 and 02. Both fail closed
if Partner Center already has a pending or unresolved submission. In
particular, if workflow 01 failed **after** it staged a draft, workflow 02 will
not overwrite or cancel that draft. Inspect the Partner Center draft and
submit or resolve it there before rerunning recovery. This is deliberate:
an ambiguous active draft cannot safely be identified as a retry target.

## Workflow 03: GHCR and Ubuntu recovery

Enter the existing numeric tag `X.Y.Z`. Workflow 03 requires its published,
non-prerelease GitHub Release, rebuilds the tagged image, and pushes both
`ghcr.io/scottia/splined:vX.Y.Z` and `:latest`. If the Ubuntu tarball is absent
from that same release, it builds the exact tagged source and uploads the
tarball plus a companion SHA-256 file. It never replaces an existing Ubuntu
asset or creates a second release. It refreshes the existing release body with
the shared git-cliff generator.

## Notes and Store safety

The shared `cliff.toml` and checked-in Tera templates classify release changes.
GitHub notes cover the previous numeric tag through the current tag. Store
Highlights cover the **last published Store package** through the target tag,
so intervening GitHub-only releases are included. The Store Highlights file
must fit the 1,500-character Store limit; generation fails instead of
truncating. `STORE-SUBMISSION.json` records the target, published baseline,
range, git-cliff version, and Highlights checksum.

Store product `9P8G4GMBBVBS` uses the repository's
`AZURE_AD_APPLICATION_CLIENT_ID`, `AZURE_AD_APPLICATION_SECRET`,
`AZURE_AD_TENANT_ID`, and `SELLER_ID` Actions secrets. The publisher retrieves
the pending submission after staging and changes only its existing en-US
`BaseListing.ReleaseNotes`, preserving all other listing and package fields.
No release workflow should be used as a read-only status check; inspect Partner
Center directly for certification progress.
