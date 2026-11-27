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

Official Workflows 01 and 02 own Windows release packaging. The candidate
validation stage creates no MSIX and retains a Build Report with the internal
candidate identity. After that report passes, the same run performs a clean
full-source build and creates the portable ZIP plus unsigned production MSIX
from one version- and commit-bound executable.

The production layout is created directly from the Partner Center manifest.
CI unpacks the final MSIX and verifies the exact identity, publisher, publisher
display name, release version, x64 architecture, Windows.Desktop family,
.spl association, runFullTrust capability, AMD64 executable, and absence of an
unexpected signature or host DLL.

SPLINED-x64-store-unsigned.msix is a maintainer-only Microsoft Store draft
artifact. It is not an end-user installer and is not attached to the GitHub
Release. Microsoft Store processing remains responsible for production signing
and deployment.

After the GitHub Release is published, the same workflow uses pinned git-cliff
release context to create an offline Store review bundle containing the
unsigned MSIX, bounded Highlights, STORE-SUBMISSION.json, and
release-context.json. It performs no Partner Center authentication, history
query, staging, draft mutation, or certification submission. No separate run
or copied build ID is required.

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

For an official Windows release, dispatch Workflow 01 or 02 from main. The
single run validates the candidate, builds the exact Windows assets, publishes
the GitHub Release, and retains the local Microsoft Store draft/review bundle.
Review that artifact before any separately authorized Partner Center action.
The official release workflows do not authenticate to or modify Partner Center.

GitHub release notes cover the immediately previous numeric Git tag through the
new release. Store Highlights are rendered from the same pinned release context
and remain bounded to the 1,500-character Store limit without silent
truncation.

The checked-in App Installer template is not the Microsoft Store update path.
It is retained only for a possible separately signed direct-distribution
channel and remains disabled while its HTTPS placeholders are unresolved.

## Privacy, support, and license

- [Privacy policy](https://github.com/scottia/S-P-L-I-N-E-D/blob/main/PRIVACY.md)
- [Support](https://github.com/scottia/S-P-L-I-N-E-D/blob/main/SUPPORT.md)
- [GNU General Public License v3](https://github.com/scottia/S-P-L-I-N-E-D/blob/main/LICENSE)
