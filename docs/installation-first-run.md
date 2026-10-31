# Installation and first run

SPLINED maintains a native Windows application, an Ubuntu WebGUI/Python stack,
and a separate root-native Rust command-line implementation for Linux and
macOS. Release artifacts vary by selected release target; use a platform's
instructions only when the matching artifact is published, or build the
root-native source deliberately. Keep configuration, credentials, and
databases outside source control.

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
2. Choose the SQLite/database directory; it owns `splined.db` and should be a
   persistent `_db` location separate from the disposable `_cache` directory.
3. Enable **Shared/network SQLite** only after reading the
   [shared-database guide](splined-media-database.md#before-enabling-shared-mode)
   and confirming every client uses the same physical database and reliable
   cross-host locks. Leave it disabled for an independent Windows database.
   The extra Windows `.client-lock` is optional and off by default; enable it
   when every client can acquire it, and never refresh from two clients at once.
4. Choose separate temporary cache, log, and credential directories.
5. Review ignored subdirectories, then select **Save & Open Settings**. The
   button shows progress while the initial library is loaded; any error stays
   visible on Setup so paths can be corrected and retried.
6. Use structured Settings for scan behavior, source order/policies, artwork
   ranges, output, square/upscale, adjustments, and retention options.
7. Add provider JSON in Credentials and complete authorization where required.
8. Refresh Media Library Selection.
9. Run a READ scan before enabling LIVE WRITE.

As an alternative to typing an existing configuration, double-click an
unprotected `.spl` backup. Portable SPLINED imports its Config v5 settings into
`data\config.toml` before startup. When that archive has shared SQLite enabled
but the optional Windows `.client-lock` disabled, SPLINED asks Yes/No whether
to enable the extra lock; No leaves it off. An unavailable archive path opens
Setup with the imported fields for correction rather than preventing the app
window from opening. A password-protected backup opens Backup & Updates with
the archive preselected so its password can be entered.
Setup also offers **Existing settings (.spl)** and **Load into Setup**. This
reads either an unprotected or password-protected archive without saving it,
then fills the Setup fields for review. Use this when moving a portable
installation and correct any paths before selecting **Save & Open Settings**.

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

Maintainer packaging, validation, Store submission, and exact-tag recovery are
documented in the [release automation guide](release-automation.md). See the
[Microsoft Store Windows channel](windows-store.md) for installed identity and
update behavior.

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

Use the Linux portable instructions when a matching release artifact is
published, or build the separate root Cargo project from source. Keep the
`splined` executable and run it from a terminal. Config v5 may be supplied with
`--config`; see the configuration reference.

## macOS

The selected release workflow can package the separate root-native macOS
command-line implementation. Extract a published archive, keep the `splined`
executable, and run it from a terminal. If downloaded-file quarantine applies,
review the file and clear quarantine according to local policy before
execution.

## WebGUI / Python

Deploy the required [WebGUI and PostgreSQL stack](../docker/README.md), open
its browser address, and create the administrator with the deployment token.
Complete Settings with server-visible paths, configure provider credentials,
and run the first full library refresh. Start in Read mode.

PostgreSQL owns the WebGUI catalog and persistent queue; Windows continues to
use its independent SQLite store. The optional SQLite snapshot import is a
one-time server-side initialization aid, not live sharing or synchronization.
It does not rewrite or preflight stored music paths, so prefer a fresh index
unless the snapshot is disposable and already uses the server's path
namespace. See the [WebGUI guide](browser-workspace-guide.md).

## Safe validation

Start with READ mode. Confirm provider access, MusicBrainz identity, candidate
ordering, destination paths, and run reports. Enable LIVE WRITE only after the
reported decisions match the intended policy.

For Windows-specific behavior, see [Windows guide](windows-guide.md). For all
settings, see [Config v5 reference](config-v5-reference.md).
