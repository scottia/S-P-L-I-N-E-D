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
  runtime\
    splined-core.dll
  README-WINDOWS.txt
```

The WinForms executable loads the fixed Rust DLL in-process. Rust remains the
authoritative processing implementation. Normal startup and scans do not
extract executable code or launch a SPLINED processing child; the application
runs as one normal process.

On first save, the application creates:

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

If neither portable file exists, one legacy ConfigV5/UiV4 pair may be imported
once. Imported values are written to the portable files. Legacy values are not
deleted, are never written by the new desktop, and cannot override an existing
portable file or resurrect after the migration marker is recorded.

### Windows first-run checklist

1. Open Settings.
2. Set `library.music_library` and `scan.scan_library_dir`.
3. Confirm `scan.cache_dir`; `splined.db` remains authoritative there.
4. Confirm temporary cache, log, history, and credential locations.
5. Save Config v5.
6. Add provider JSON in Credentials and complete authorization where required.
7. Refresh Media Library Selection.
8. Run a READ scan before enabling LIVE WRITE.

### Windows installed Microsoft Store MSIX

The managed-install/update path is
**[Microsoft Store — Install SPLINED](https://apps.microsoft.com/detail/9p8g4gmbbvbs?hl=en-US&gl=US)**.
The live Store package provides Windows-managed installation, Microsoft Store
production signing and updates, package identity, Start menu activation,
manifest-owned `.spl` association, and normal Windows uninstall. Its payload is
the same x64 `splined.exe` plus `runtime\splined-core.dll`.

Because an MSIX installation directory is read-only, Config v5 and interface
state live under the package's per-user LocalState directory. Configured
absolute, mapped, and UNC resources remain unchanged. This differs from the
portable ZIP above, which requires no package installation and keeps its
root-relative state under `<portable-root>\data`.

For maintainers, `SPLINED (All OS) and GHCR` builds the x64 Windows runtime pair
once. The same `splined.exe` and `runtime\splined-core.dll` feed the Portable ZIP
and, when **Microsoft Store** is enabled, the unsigned Store MSIX. Portable and
selected Store validation must both succeed before the numeric release tag is
created. After the GitHub Release and GHCR publish, the workflow automatically
stages the MSIX, preserves the existing Store submission metadata, updates only
the en-US Store Highlights, and submits it for Microsoft processing. The MSIX
is retained with `STORE-HIGHLIGHTS.txt` in the Actions-only
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
`windows-update.json`. If a release is newer, it identifies the Portable
channel, explains that updating is manual, and can open the official release
page. The user downloads the current portable ZIP, closes SPLINED, and replaces
only:

```text
splined.exe
runtime\splined-core.dll
README-WINDOWS.txt
```

Never replace or delete `data\`, SQLite, credentials, history, cache, logs, or
configured external state. No updater executable, hidden helper, command shell,
or self-replacement loop participates in this process.

For Microsoft Store installations, the same menu command uses Microsoft Store
package APIs instead of GitHub. It reports when the Store package is current;
when an update exists, **UPDATE NOW** asks Windows/Microsoft Store to download
and install it and **LATER** defers it. If direct Store integration is
unavailable, SPLINED offers the official Store product page. Store users never
receive Portable ZIP replacement instructions. The separate App Installer
template remains disabled and is not the Store update path. Windows builds
target only `x86_64-pc-windows-msvc`.

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

## Safe validation

Start with READ mode. Confirm provider access, MusicBrainz identity, candidate
ordering, destination paths, and run reports. Enable LIVE WRITE only after the
reported decisions match the intended policy.

For Windows-specific behavior, see [Windows guide](windows-guide.md). For all
settings, see [Config v5 reference](config-v5-reference.md).
