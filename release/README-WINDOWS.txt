S:P:L:I:N:E:D — Windows x64 portable release

CONTENTS

  splined.exe
  runtime\splined-core.dll
  README-WINDOWS.txt

The WinForms frontend loads the fixed Rust processing DLL in the same process.
Keep splined.exe and runtime\splined-core.dll from the same release together.

FIRST RUN

1. Extract the complete SPLINED directory to a writable location.
2. Run splined.exe.
3. Complete Settings and save Config v5.

The fresh archive has no data directory. First save or one-time migration
creates:

  data\config.toml
  data\ui.toml

Move or copy the complete directory to carry portable settings with the app.
Deleting it removes SPLINED-owned portable settings. Relative paths resolve from
the directory containing splined.exe, independent of process CWD. Absolute,
mapped-drive, and UNC paths remain unchanged.

CONFIGURATION AND DATA

Config v5 controls the music library, SQLite database/cache directory,
temporary run cache, credentials, logs, and history. Existing external paths are
not relocated. Rust remains the sole SQLite processing authority.

BACKUP

The Backup screen exports and restores selected .spl categories. Selected
categories overwrite their destinations. Unselected categories remain
unchanged. Password protection is optional.

UPDATES

Help > Check for Updates shows official release information and can open the
official release page. Portable SPLINED does not download or replace its own
binaries. Close SPLINED, extract the new release separately, then replace only:

  splined.exe
  runtime\splined-core.dll
  README-WINDOWS.txt

Never replace or delete data\, splined.db, credentials, history, cache, logs, or
configured external resources during an upgrade.

SECURITY

Normal startup and scanning do not extract, generate, rename, replace, launch,
or delete executable code. There is no worker executable, updater executable,
hidden helper, command-shell lifecycle, or self-reinvocation. Windows is x64
only (`x86_64-pc-windows-msvc`).

SUPPORT

Use Run Activity, the configured log directory, and run reports when reporting
a problem. Do not include credential file contents.
