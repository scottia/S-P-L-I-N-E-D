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

Windows release CI creates a clean production layout directly from the Partner
Center manifest and the release-built binaries. It updates manifest assets and
packs this layout without signing:

```text
GitHub Actions artifact: splined-windows-store-submission
Artifact file: SPLINED-x64-store-unsigned.msix
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
and is not attached to a GitHub Release. Microsoft Store processing supplies the
production signing and deployment path. Store-installed updates are delivered
by Windows/Microsoft Store package deployment; SPLINED does not replace its own
binaries.

## Development package validation

CI creates a second layout under the runner's temporary directory. Only that
layout may run `create-debug-identity`, receive a disposable development
certificate, be signed, installed, activated, inspected, and uninstalled. The
temporary certificate and package are removed after validation. Development
identity and signing never mutate the production layout or Store artifact.

The development package proves x64 installation, application activation,
in-process Rust DLL initialization, package identity, and the `.spl` association.
A development-signed package is QA-only and is not publicly trusted.

## Publishing future Store updates

The public listing is already live at the
[official Microsoft Store URL](https://apps.microsoft.com/detail/9p8g4gmbbvbs?hl=en-US&gl=US).
For a future Store update, maintainers download
`splined-windows-store-submission` from the corresponding GitHub Actions run and
submit `SPLINED-x64-store-unsigned.msix` to the existing Partner Center product
with Store ID `9P8G4GMBBVBS`. Review, certification responses, listing changes,
and publication remain manual Partner Center operations for each future update;
they are not outstanding steps for the initial public release.

The checked-in App Installer template is not the Microsoft Store update path.
It is retained only for a possible separately signed direct-distribution
channel and remains disabled while its HTTPS placeholders are unresolved.

## Privacy, support, and license

- [Privacy policy](https://github.com/scottia/S-P-L-I-N-E-D/blob/main/PRIVACY.md)
- [Support](https://github.com/scottia/S-P-L-I-N-E-D/blob/main/SUPPORT.md)
- [GNU General Public License v3](https://github.com/scottia/S-P-L-I-N-E-D/blob/main/LICENSE)
