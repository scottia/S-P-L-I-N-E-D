# Windows Microsoft Store channel

This is a **development and maintainer reference** for the Windows
Store/portable distribution built from the current source. Public
availability and certification status must be confirmed in Microsoft
Partner Center or the [Store listing](https://apps.microsoft.com/detail/9p8g4gmbbvbs?hl=en-US&gl=US);
an old review result is not proof of a new submission.

## Package and build identity

The checked-in
[`build/Windows/package/Package.appxmanifest`](../build/Windows/package/Package.appxmanifest)
defines the production package identity and minimum OS support:

| Setting | Manifest contract |
| --- | --- |
| Identity | `Psycotix.SPLINED` |
| Publisher | `CN=FE370EF6-D95D-4A6F-9AAB-2654E6DE00FE` |
| Store ID | `9P8G4GMBBVBS` |
| Windows target | `x86_64-pc-windows-msvc`, x64 executable |
| `Windows.Desktop` | `MinVersion=10.0.17763.0`; `MaxVersionTested=10.0.26100.0` |

The same native Rust/Slint `splined.exe` builds the Windows portable
ZIP and the Store MSIX. The Store signs packages through Partner
Center; the locally constructed **unsigned** MSIX is a maintainer
submission artifact, **not** an end-user installer or a public GitHub
Release attachment.

Windows packaged configuration lives in the app's per-user
`LocalState`; the portable version instead uses its configured
persistent data root. Both use the same authoritative SQLite catalog
and owner-approved behavioral JSONs, regardless of shared/standalone
SQLite mode.

## Release and recovery

[Workflow 01](../.github/workflows/01-full-release.yml)
creates a new numeric release from `main` only after explicit
confirmation and the selected builds. Its mutually exclusive
Store options are:

- **Submit Update/No Draft:** automatically stages the package and
  updated Highlights, then requests certification.
- **Review Bundle/No Submit:** prepares a verified package, cumulative
  Highlights and submission audit data, without staging a Partner
  Center draft or submitting certification.

Both require a Windows-capable selection; no Store action is
authorized by this guide. Optional **Extended Windows validation**
adds format, Clippy and Rust library tests. Native packaging always
requires its own clean optimized build and package checks.

[Workflow 02](../.github/workflows/02-store-recovery.yml)
rebuilds and validates the Store package from an **existing** numeric
release tag and uses an explicitly chosen draft/update mode. It
does not create a new tag or GitHub Release. Source compatibility for
older tagged layouts does not reauthorize obsolete root source trees
on current `dev`.

## Store history, highlights and safeguards

[Release scripts](../release/README.md) include pinned git-cliff,
the Store baseline resolver, cumulative Highlights formatting and
synthetic historical Store regression data. Store Highlights are
capped at the supported **1,500-character** boundary, preserving
complete priority-ordered bullets rather than truncating them.
Prior Store JSON examples remain test inputs, not statements about
the currently installed version.

The unsigned MSIX and any Store review bundle are maintained as
GitHub Actions artifacts. A successful GitHub Release or GHCR
publication does not establish Partner Center certification.
Do not overwrite, replace or submit an uncertain pending Store
submission without an explicit owner decision.

## Operator and validation references

Windows installation, its first-run SQLite choice, app backups and
updates: [Windows guide](windows-guide.md). Full release/
recovery boundaries: [release automation](release-automation.md).
The protected media-state/COMP policy remains governed by the
[owner register](POLICY-REGISTER.md).

No Store submission, native package smoke test, or successful
production promotion was performed by restoring this document.
