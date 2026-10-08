S:P:L:I:N:E:D — Windows portable release

CONTENTS

  splined.exe
  README-WINDOWS.txt

FIRST RUN

1. Extract both files to a writable directory.
2. Run splined.exe.
3. Complete Settings and save Config v5.

The data directory is created only after settings are saved or a one-time
legacy settings migration succeeds:

  data\config.toml
  data\ui.toml

Move or copy the complete directory to carry portable settings with the app.
Deleting the directory removes SPLINED-owned portable settings. Relative paths
resolve from the directory containing splined.exe, independent of process CWD.
Absolute, mapped-drive, and UNC paths remain unchanged.

PROCESSING

The frontend and authoritative Rust processing backend are contained in the
single splined.exe process. Normal startup and scanning do not extract, create,
rename, replace, launch, or delete executable code.

CONFIGURATION AND DATA

Config v5 controls the music library, SQLite database/cache directory,
temporary run cache, credentials, logs, and history. Existing external paths
are not relocated. The application does not create a second database.

BACKUP

The Backup screen exports and restores selected .spl categories. Selected
categories overwrite their destinations. Unselected categories remain
unchanged. Password protection is optional.

UPDATES

Production Windows releases use update metadata carrying a cryptographic
signature for the standard x64 update package. Installation requires explicit
approval, and the package must verify with the stable updater key. The update
targets this portable directory and preserves data\ plus all configured
external state. Builds without an updater verification key cannot install
public updates. Authenticode signing is not required.

SUPPORT

Use the Run Activity panel, configured log directory, and run reports when
reporting a problem. Do not include credential file contents.
