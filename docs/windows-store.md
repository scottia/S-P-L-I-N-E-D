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

The Store payload is the same x64 application pair used by the portable build:

```text
splined.exe
runtime\splined-core.dll
```

The WinForms frontend loads the Rust core DLL in-process. Both files target
`x86_64-pc-windows-msvc` and must report PE Machine `0x8664`. There is no worker
executable, updater executable, hidden SPLINED process, or executable extraction.

The installed package uses its per-user package LocalState for Config v5 and UI
state because the MSIX installation directory is read-only. Configured absolute,
mapped-drive, and UNC paths remain unchanged. The portable ZIP continues to use
root-relative `data\config.toml` and `data\ui.toml`; it is a separate distribution
channel and does not require package identity.

## Store release and update package

`SPLINED (All OS) and GHCR` is the only workflow that creates a release version
and numeric tag. For Windows or All releases its **Microsoft Store** input
defaults to enabled. The Windows job performs one clean x64
build, uses that exact executable/DLL pair for the Portable ZIP and Store MSIX,
and validates both before the workflow may create the numeric tag. Ubuntu-only
and macOS-only runs ignore this Windows option.

The production layout is created directly from the Partner Center manifest and
the same version/commit-bound binaries used by the Portable ZIP. It updates
manifest assets and packs this layout without signing:

```text
GitHub Actions artifact: splined-windows-store-submission
Artifact files: SPLINED-x64-store-unsigned.msix, STORE-HIGHLIGHTS.txt
```

The production layout never runs `create-debug-identity`, never receives a
development certificate, and is never development-signed. CI unpacks the final
MSIX and reads back its manifest. Packaging fails unless the exact identity,
publisher, publisher display name, release version, x64 architecture,
`Windows.Desktop` family, `.spl` association, and `runFullTrust` capability are
present. It also verifies `splined.exe` and `runtime\splined-core.dll` exist and
are AMD64, and confirms that no package signature was added.

`SPLINED-x64-store-unsigned.msix` is a maintainer-only Partner Center submission
artifact for future Store releases and updates. It is not an end-user installer
and is not attached to a GitHub Release or included in its checksums. Microsoft
Store processing supplies the production signing and deployment path.
Store-installed updates are delivered by Windows/Microsoft Store package
deployment; SPLINED does not replace its own binaries.

`SPLINED (Tagged) > MS Store Package Resolution` is the package-only recovery
workflow. Given an existing numeric release tag, it rebuilds that exact source
version, performs the same strict production packaging and isolated development
QA, generates Store Highlights, and uploads the same Actions artifact. It does
not publish to Store. `SPLINED > MS Store Publish & Update` uses an existing
numeric tag for the same build and QA, then submits that package and its Store
Highlights to the live product. Neither workflow creates or changes a version
or tag, creates another GitHub Release, or publishes GHCR.

## Development package validation

CI creates a second layout under the runner's temporary directory. Only that
layout may run `create-debug-identity`, receive a disposable development
certificate, be signed, installed, activated, inspected, and uninstalled. The
temporary certificate and package are removed after validation. Development
identity and signing never mutate the production layout or Store artifact.

The development package proves x64 installation, application activation,
in-process Rust DLL initialization, package identity, and the `.spl` association.
A development-signed package is QA-only and is not publicly trusted.

## Help > Check for Updates

The Windows GUI uses `ConfigStore.IsPackaged` as its channel detector.

- **Microsoft Store package:** GitHub releases and the Portable ZIP are not
  update authority. SPLINED queries
  `Windows.Services.Store.StoreContext.GetAppAndOptionalStorePackageUpdatesAsync`.
  **UPDATE NOW** requests
  `RequestDownloadAndInstallStorePackageUpdatesAsync`, so Windows/Microsoft
  Store owns download and installation. **LATER** makes no change. If the Store
  API is unavailable or cannot complete safely, SPLINED offers the official
  [Store product page](https://apps.microsoft.com/detail/9p8g4gmbbvbs?hl=en-US&gl=US)
  and reports the limitation; it never shows Portable ZIP instructions.
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
For a normal future Store update, maintainers leave **Microsoft Store** enabled
in the corresponding `SPLINED (All OS) and GHCR` run. After selected build QA,
the workflow creates the numeric tag, renders GitHub notes and concise Store
Highlights from one git-cliff 2.14.2 context, publishes the GitHub Release and
GHCR, then stages and submits the exact validated MSIX to the existing product
with Store ID `9P8G4GMBBVBS`. It retrieves the pending submission after package
staging, preserves all existing package/listing metadata, and changes only the
en-US `BaseListing.ReleaseNotes` field before submission. Actions reports the
Store API status without waiting indefinitely for certification.

The Actions-only `splined-windows-store-submission` artifact preserves the
exact MSIX and `STORE-HIGHLIGHTS.txt` for audit or recovery. Store publication
requires `AZURE_AD_APPLICATION_CLIENT_ID`, `AZURE_AD_APPLICATION_SECRET`,
`AZURE_AD_TENANT_ID`, and `SELLER_ID` as GitHub Actions secrets; missing values
fail clearly. Selecting **Microsoft Store = false** performs no Store package,
QA, authentication, Store-submission artifact, or Store API submission work. Use
`SPLINED (Tagged) > MS Store Package Resolution` after a package rejection, or
`SPLINED > MS Store Publish & Update` when an existing tag needs Store
publication/republication. See [Release automation](release-automation.md).

The checked-in App Installer template is not the Microsoft Store update path.
It is retained only for a possible separately signed direct-distribution
channel and remains disabled while its HTTPS placeholders are unresolved.

## Privacy, support, and license

- [Privacy policy](https://github.com/scottia/S-P-L-I-N-E-D/blob/main/PRIVACY.md)
- [Support](https://github.com/scottia/S-P-L-I-N-E-D/blob/main/SUPPORT.md)
- [GNU General Public License v3](https://github.com/scottia/S-P-L-I-N-E-D/blob/main/LICENSE)
