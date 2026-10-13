# Installation and first run

SPLINED ships supported Windows, Linux, macOS, Python, and container release
artifacts. Keep configuration, credentials, and databases outside source
control.

## Windows portable

Extract the Windows ZIP to a writable directory and run `splined.exe`. The
fresh archive contains exactly:

```text
SPLINED\
  splined.exe
  README-WINDOWS.txt
```

The Cargo-built executable contains the Rust application and Slint GUI. Normal
startup and scans do not extract executable code, load an application bridge,
or launch a SPLINED processing child; the application runs as one process.

Before first save, launching the executable creates none of the configuration,
database, cache, log, credential, or obsolete bootstrap directories. The
ordinary workspace stays unavailable and structured Setup opens. After valid
choices are explicitly saved, the application creates:

```text
SPLINED\
  data\
    config.toml
    ui.toml
```

These files are the only Config v5 and interface-state authority. Process CWD
does not affect resolution. Relative paths resolve from the directory containing
`splined.exe`; absolute, mapped-drive, and UNC paths remain unchanged. Copying
the complete directory carries portable settings. Deleting it removes
SPLINED-owned settings.

No Registry value is consulted as current or migration authority. An existing
`data\config.toml` is discovered automatically; the user is never asked to
browse to it. If that file is missing, invalid, or operationally incomplete,
Setup displays the real error instead of silently continuing with defaults.

### Windows first-run checklist

1. In Setup, choose the Music Library.
2. Choose the SQLite/database directory; it owns `splined.db`.
3. Enable **Shared/network SQLite** only when Python/Docker owns that same
   inventory. Leave it disabled for an independent Windows database.
4. Choose separate temporary cache, log, and credential directories.
5. Review ignored subdirectories, then select **Save and Continue**.
6. Use structured Settings for scan behavior, source order/policies, artwork
   ranges, output, square/upscale, adjustments, and retention options.
7. Add provider JSON in Credentials and complete authorization where required.
8. Refresh Media Library Selection.
9. Run a READ scan before enabling LIVE WRITE.

As an alternative to typing an existing configuration, double-click an
unprotected `.spl` backup. Portable SPLINED imports its Config v5 settings into
`data\config.toml` before startup. A password-protected backup opens Backup &
Updates with the archive preselected so its password can be entered.

### Windows installed Microsoft Store MSIX

The managed-install/update path is
**[Microsoft Store — Install SPLINED](https://apps.microsoft.com/detail/9p8g4gmbbvbs?hl=en-US&gl=US)**.
The live Store package provides Windows-managed installation, Microsoft Store
production signing and updates, package identity, Start menu activation,
manifest-owned `.spl` association, and normal Windows uninstall. Its application
payload is the same x64 Rust/Slint `splined.exe`.

Because an MSIX installation directory is read-only, Config v5 and interface
state live under
`%LOCALAPPDATA%\Packages\<package-family>\LocalState\SPLINED\data`. Configured
absolute, mapped, and UNC resources remain unchanged. This differs from the
portable ZIP above, which requires no package installation and keeps its
root-relative state under `<portable-root>\data`.

For maintainers, `SPLINED (All OS) and GHCR` builds the x64 Windows executable
once. The same `splined.exe` feeds the Portable ZIP
and, when **Microsoft Store** is enabled, the unsigned Store MSIX. Portable and
selected Store validation must both succeed before the numeric release tag is
created. After the GitHub Release and GHCR publish, the workflow automatically
stages the MSIX, preserves the existing Store submission metadata, updates only
the en-US Store Highlights, and submits it for Microsoft processing. The MSIX
is retained with `STORE-HIGHLIGHTS.txt` and its `STORE-SUBMISSION.json` history
audit in the Actions-only
`splined-windows-store-submission` artifact; it is not an end-user installer or
a normal GitHub Release download. Development identity and signing use a
separate temporary QA layout and cannot mutate the Store artifact.
`SPLINED (Tagged) > MS Store Package Resolution` rebuilds a package without
submitting it; `SPLINED > MS Store Publish & Update` publishes an existing tag.
See the [release automation guide](release-automation.md) and
[Microsoft Store Windows channel](windows-store.md) for the live product,
identity, and update-publication procedure.

### Windows upgrade

For Portable Windows, **Help > Check for Updates** reads GitHub Releases and
`windows-update.json` and compares numeric semantic release versions. A newer
version produces the Portable notification, explains that updating is manual,
and can open the official release page. The same version with a different build
commit is current, not an update. A manual current check displays **No SPLINED
update is available.**; an automatic/startup current check is silent. The user
downloads the current portable ZIP, closes SPLINED, and replaces only:

```text
splined.exe
README-WINDOWS.txt
```

Never replace or delete `data\`, SQLite, credentials, history, cache, logs, or
configured external state. No updater executable, hidden helper, command shell,
or self-replacement loop participates in this process.

For Microsoft Store installations, the same action uses native Microsoft Store
package APIs instead of GitHub and reports the available Store update count.
Installation remains owned by Microsoft Store (Library > Get updates). Store
users never receive Portable ZIP replacement instructions. The separate App
Installer template remains disabled and is not the Store update path. Windows
builds target only `x86_64-pc-windows-msvc`.

## Linux

Extract the archive, keep `splined` executable, and run it from a terminal.
Config v5 may be supplied with `--config`; see the configuration reference.

## macOS

Extract the archive, keep `splined` executable, and run it from a terminal.
If downloaded-file quarantine applies, review the file and clear quarantine
according to local policy before execution.

## Python

Install the dependencies declared by the supported Python package or use the
published container image. Python and native implementations share Config v5
and SQLite authority. Do not run concurrent writers against one database.

Python's semantic-version update check is notification-only, and selective
unprotected `.spl` archives provide cross-platform state transfer:

```text
splined --check-for-updates
splined --backup-export state.spl --backup-components settings,interface,credentials,database
splined --backup-restore state.spl --backup-components settings,credentials
```

The Ratatui `E` settings surface saves validated Config v5 values; `A` opens
runtime diagnostics. See the Python Ratatui guide for candidate filters,
MusicBrainz filters, comparison, and explicit compilation-track reopen.

## Safe validation

Start with READ mode. Confirm provider access, MusicBrainz identity, candidate
ordering, destination paths, and run reports. Enable LIVE WRITE only after the
reported decisions match the intended policy.

For Windows-specific behavior, see [Windows guide](windows-guide.md). For all
settings, see [Config v5 reference](config-v5-reference.md).
