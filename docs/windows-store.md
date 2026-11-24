# Microsoft Store Windows channel

SPLINED is approved and publicly available from the Microsoft Store. The Store
is the managed-install/update option for Windows users; the portable ZIP is the
separate no-install/direct option. Both are current supported distribution
channels.

## Production status

- Certification: **Approved**
- Publication: **Live**
- Public installation: **[Microsoft Store — Install SPLINED](https://apps.microsoft.com/detail/9p8g4gmbbvbs?hl=en-US&gl=US)**
- Production signing: supplied by Microsoft Store processing
- Installed updates: delivered through Windows/Microsoft Store package deployment

SPLINED does not download or self-replace its application binaries. Store users
receive Windows-managed installation, updates, and uninstall; portable users
continue to update their separate ZIP installation manually.

The live production package uses the Partner Center identity assigned to the
product. These values are exact release inputs:

| Partner Center field | Production value |
| --- | --- |
| Package identity name | `Psycotix.SPLINED` |
| Publisher | `CN=FE370EF6-D95D-4A6F-9AAB-2654E6DE00FE` |
| Publisher display name | `Psycotix` |
| Package family name | `Psycotix.SPLINED_8pvn5te36e43t` |
| Store ID | `9P8G4GMBBVBS` |

The checked-in `windows/package/Package.appxmanifest` is the authoritative
production manifest. Its application ID remains `SPLINED`, display name is
`SPLINED`, target device family is `Windows.Desktop`, and architecture is
`x64`. It retains the `.spl` file association and `runFullTrust` capability.
Release provisioning writes only the package Identity version to the generated
SPLINED release version plus `.0`; it must never rewrite the operating-system
version gates. The Store package is guarded at `Windows.Desktop`
`MinVersion="10.0.17763.0"` and `MaxVersionTested="10.0.26100.0"`.
Packaging fails if either value changes unexpectedly.

## Runtime architecture

The Store payload uses the same single x64 application executable as the
portable build:

```text
splined.exe
```

`splined.exe` is built directly by Cargo, links the Rust application modules,
and presents them through Slint. It targets `x86_64-pc-windows-msvc` and must
report PE Machine `0x8664`. There is no application DLL bridge, worker
executable, updater executable, hidden SPLINED process, or executable extraction.

The installed package uses its per-user package LocalState for Config v5 and UI
state because the MSIX installation directory is read-only. Configured absolute,
mapped-drive, and UNC paths remain unchanged. The portable ZIP continues to use
root-relative `data\config.toml` and `data\ui.toml`; it is a separate distribution
channel and does not require package identity.

## Store release and update package

[Workflow 22 — Windows Build & Microsoft Store](../.github/workflows/22-windows-build-store.yml)
owns Windows packaging and Store submission. Its standard **BUILD ONLY** path
consumes an approved Workflow 21 candidate and always produces the portable
ZIP, unsigned production MSIX, isolated development-package validation, and a
complete offline local review bundle. Both **PARTIAL UPDATE** and **FULL SOURCE**
use the same final executable for the portable and Store packages.

The local review bundle contains the unsigned MSIX, bounded Highlights,
`STORE-SUBMISSION.json`, and `release-context.json`. It requires no Store
credentials, CLI installation, authentication, baseline query, staging, draft
modification, or certification submission. Its audit explicitly records that
Partner Center was not contacted. An existing pending submission therefore
cannot block a normal Windows build.

**BUILD ONLY, REPORT** validates the approved candidate and proposed build,
cache, packaging, and Store-readiness plan without compiling or packaging.

[Workflow 24 — Complete Release & Publication](../.github/workflows/24-complete-release.yml)
later publishes the previously validated portable artifact and reports MSIX
readiness. It never submits to Partner Center.

After the corresponding GitHub release is published, run Workflow 22 with
**SUBMIT EXISTING MSIX** and **AUTO MSIX SUBMISSION** to request Store
publication. This path requires the exact originating Windows build run ID,
verifies its build manifest, SHA-256 values, candidate commit and published
numeric tag, then waits for explicit protected-environment approval. Only then
does it authenticate, resolve the last published Store baseline, generate
cumulative Highlights and submit the exact previously validated MSIX. Cargo,
portable packaging and MSIX packaging are not present in the submission path.

The production layout is created directly from the Partner Center manifest and
the same version/commit-bound binaries used by the Portable ZIP. It updates
manifest assets and packs this layout without signing:

```text
GitHub Actions artifact: splined-windows-store-submission
Artifact files: SPLINED-x64-store-unsigned.msix, STORE-HIGHLIGHTS.txt,
                STORE-SUBMISSION.json
```

The production layout never runs `create-debug-identity`, never receives a
development certificate, and is never development-signed. CI unpacks the final
MSIX and reads back its manifest. Packaging fails unless the exact identity,
publisher, publisher display name, release version, x64 architecture,
`Windows.Desktop` family, `.spl` association, and `runFullTrust` capability are
present. It also verifies `splined.exe` exists and is AMD64, rejects unexpected
DLL host artifacts, and confirms that no package signature was added.

`SPLINED-x64-store-unsigned.msix` is a maintainer-only Partner Center submission
artifact for future Store releases and updates. It is not an end-user installer
and is not attached to a GitHub Release or included in its checksums. Microsoft
Store processing supplies the production signing and deployment path.
Store-installed updates are delivered by Windows/Microsoft Store package
deployment; SPLINED does not replace its own binaries.

`02. Store Recovery - Tagged Draft or Update` accepts an existing numeric
release tag, rebuilds that exact source version, performs production packaging
and isolated development QA, and generates cumulative Store Highlights. It
then stages a Partner Center draft or submits for certification according to
the selected Store mode. It does not create a version, tag, GitHub Release, or
GHCR image.

## Development package validation

CI creates a second layout under the runner's temporary directory. Only that
layout may run `create-debug-identity`, receive a disposable development
certificate, be signed, installed, activated, inspected, and uninstalled. The
temporary certificate and package are removed after validation. Development
identity and signing never mutate the production layout or Store artifact.

The development package proves x64 installation, Rust/Slint application
activation, package LocalState initialization, package identity, and the `.spl`
association.
A development-signed package is QA-only and is not publicly trusted.

## Help > Check for Updates

The Rust application uses the native current-package-family query as its channel
detector.

- **Microsoft Store package:** GitHub releases and the Portable ZIP are not
  update authority. SPLINED queries
  `Windows.Services.Store.StoreContext.GetAppAndOptionalStorePackageUpdatesAsync`.
  The WinRT operation is awaited directly through the Rust Windows bindings.
  The result reports native Store update availability; installation remains
  owned by Microsoft Store Library > Get updates. If the Store API is
  unavailable, SPLINED reports the exact limitation and never substitutes
  Portable ZIP instructions.
- **Portable package:** GitHub Releases plus `windows-update.json` remain update
  authority. Availability is strictly a numeric semantic-version comparison:
  only an advertised version newer than the installed version is an update.
  Commit metadata remains internal and never drives or appears in normal update
  UI. Manual current checks report **No SPLINED update is available.** and
  automatic current checks are silent. The check is notification-only and may
  open the official GitHub release page for manual replacement. SPLINED never
  downloads or replaces its own Portable binaries.

## Publishing future Store updates

The public listing is already live at the
[official Microsoft Store URL](https://apps.microsoft.com/detail/9p8g4gmbbvbs?hl=en-US&gl=US).
For a normal future Store update, maintainers choose **SUBMIT EXISTING MSIX**
only after the exact Windows build has been published through Workflow 24.
First run Workflow 22 in its standard local-review build mode, inspect the
downloadable package and audit bundle, and publish the corresponding release
without Store access. Then dispatch Workflow 22 again with **SUBMIT EXISTING
MSIX**, the exact prior Windows build run ID, and **AUTO MSIX SUBMISSION**.
The protected submission approval and existing Store credential requirements
remain additional release-policy boundaries. Submit mode stages and submits the
already validated MSIX to Store ID `9P8G4GMBBVBS`, preserves existing package
and listing metadata, and changes only the en-US release-notes field before
certification. Actions reports the Store API status without waiting indefinitely.

GitHub notes cover the immediately previous numeric Git tag through the new
release. Store Highlights instead cover the last package actually published by
Microsoft Store through the new release. This carries GitHub-only releases into
the next Store update without treating an uncertified or rejected submission as
history. The Store status and baseline are resolved before staging. A publishing
mode fails closed when Store has an active or unresolved submission rather than
replacing that draft. Read-only review mode may still use the last published
baseline because it never changes the pending submission. The resolver uses the
machine-readable application record and verifies the retrieved submission
against `lastPublishedApplicationSubmission`.
The 1,500-character Store limit is enforced without silent truncation.

The Actions-only `splined-windows-release` artifact preserves the exact MSIX,
offline review files, build manifest, checksums and candidate provenance for
publication or submission recovery. Store history lookup or publication
requires `AZURE_AD_APPLICATION_CLIENT_ID`,
`AZURE_AD_APPLICATION_SECRET`,
`AZURE_AD_TENANT_ID`, and `SELLER_ID` as GitHub Actions secrets; missing values
fail clearly. Selecting **Review Bundle/No Submit** permits authentication and
read-only published-history lookup but performs no staging or submission.
Standard Workflow 22 builds perform no Partner Center authentication, history
lookup, staging, or Store API submission. They always retain the unsigned MSIX
and offline review bundle. Use
`02. Store Recovery - Tagged Draft or Update` when an existing tag needs
Store publication or a rebuilt package. See
[Release automation](release-automation.md).

The checked-in App Installer template is not the Microsoft Store update path.
It is retained only for a possible separately signed direct-distribution
channel and remains disabled while its HTTPS placeholders are unresolved.

## Privacy, support, and license

- [Privacy policy](https://github.com/scottia/S-P-L-I-N-E-D/blob/main/PRIVACY.md)
- [Support](https://github.com/scottia/S-P-L-I-N-E-D/blob/main/SUPPORT.md)
- [GNU General Public License v3](https://github.com/scottia/S-P-L-I-N-E-D/blob/main/LICENSE)
