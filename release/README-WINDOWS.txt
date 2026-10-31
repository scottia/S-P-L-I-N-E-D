S:P:L:I:N:E:D — Windows x64 portable release

CONTENTS

  splined.exe
  README-WINDOWS.txt

splined.exe is the complete Cargo-built Rust application with its Slint GUI.

FIRST RUN

1. Extract the complete SPLINED directory to a writable location.
2. Run splined.exe.
3. Complete structured Setup and select Save & Open Settings.

The fresh archive has no data directory. Launch does not create configuration,
database, cache, log, credential, or obsolete bootstrap folders. A validated
explicit first save creates:

  data\config.toml
  data\ui.toml

Move or copy the complete directory to carry portable settings with the app.
Deleting it removes SPLINED-owned portable settings. Relative paths resolve from
the directory containing splined.exe, independent of process CWD. Absolute,
mapped-drive, and UNC paths remain unchanged.

CONFIGURATION AND DATA

Config v5 controls the music library, SQLite database/cache directory,
temporary run cache, credentials, logs, and history. Existing external paths are
not relocated. Existing data\config.toml is discovered automatically; missing
or invalid configuration opens Setup rather than silently loading defaults.
The Rust application opens the configured SQLite database directly. Optional
shared mode is a Windows/native shared-file arrangement: every participating
client must open the same physical SQLite file, use compatible path mappings,
and follow the documented locking and single-refresh precautions. WebGUI uses
a separate PostgreSQL database and is not a shared-SQLite publisher.

BACKUP

The Backup screen exports and restores selected .spl categories. Selected
categories overwrite their destinations. Unselected categories remain
unchanged. Password protection is optional. Double-clicking an unprotected .spl
backup automatically imports its Config v5 settings and loads the configured
database. Password-protected backups open the Backup screen with the file
preselected so its password can be entered. Portable SPLINED maintains the
current-user .spl file association when it starts.

PORTABLE UPDATES

Check for Updates identifies this as SPLINED Portable, reads GitHub
Releases plus windows-update.json, and compares semantic release versions. A
different commit at the installed version is not an update. Manual checks say
"No SPLINED update is available." when current; automatic no-update checks are
silent. When a newer version exists, SPLINED can open its official release page.
Portable SPLINED does not download or replace its own binaries. Close SPLINED,
extract the new release separately, then replace only:

  splined.exe
  README-WINDOWS.txt

Never replace or delete data\, splined.db, credentials, history, cache, logs, or
configured external resources during an upgrade.

Microsoft Store SPLINED is the equally supported managed-install/update Windows
channel. Store installations use Windows/Microsoft Store package deployment;
this Portable package remains the supported no-install/direct channel.

SECURITY

Normal startup and scanning do not extract, generate, rename, replace, launch,
or delete executable code. There is no managed GUI host, application DLL bridge,
worker executable, updater executable, hidden helper, command-shell lifecycle,
or self-reinvocation. Windows is x64 only (`x86_64-pc-windows-msvc`).

SUPPORT

Use Run Activity, the configured log directory, and run reports when reporting
a problem. Do not include credential file contents.
