S:P:L:I:N:E:D — WINDOWS PORTABLE
===========================================

WINDOWS GUI: v3.0.0 Stable
CONFIGURATION: Config v5

FRESH INSTALL
-------------

1. Extract the complete archive into the final folder where SPLINED will live.

2. Run:

   splined.exe

   Keep the runtime folder beside it. runtime\splined-core.exe is the fixed
   worker required for library snapshots, Album scans, and artwork edits.

3. Complete first-run setup. Choose the required library, SQL database,
   temporary run cache, log, and credential locations. SPLINED stores Config v5
   in data\config.toml and interface preferences in data\ui.toml, then creates only the selected
   runtime directories after Save and Continue.

   Database, run-cache, log, and credential fields initially point beneath:

   %LOCALAPPDATA%\SPLINED

   They remain editable. Existing saved locations and UNC paths are never
   changed automatically. %PROGRAMDATA% is not used for per-user defaults.

   The archive does not contain data\, credentials\, _cache\, _logs\, or
   docker_builds\. A fresh extraction therefore performs true first-run setup.

4. Begin in Read mode with a small media selection. Confirm artwork choices
   and output policy before enabling Write mode.

The permanent archive layout contains the fixed root splined.exe GUI and fixed
runtime\splined-core.exe worker. The worker is not embedded in or extracted
from the GUI. Normal startup and scanning do not create, extract, rename,
replace, or delete executable files.

Config v5 is handed from the GUI to the Rust core in memory. No runtime TOML is
created in the portable folder, cache directory, or user profile.

Move or copy the complete folder to carry data\config.toml and data\ui.toml.
Deleting the folder removes those settings. On the first compatible launch,
legacy per-path HKCU ConfigV5/UiV4 values are copied once only when both files
are absent. The old values are left intact for manual cleanup but stop being
runtime authority after migration.

Although the fixed worker is under runtime, portable-relative config, cache,
SQLite/history, log, and credential paths resolve from the directory containing
splined.exe. The worker does not create runtime\_cache\splined.db. Existing
absolute, mapped-drive, and UNC paths remain unchanged, and no persistent-data
file is an updater staging or replacement target. Direct worker diagnostics use
the same data\config.toml default and do not create config\config.toml.


UPGRADE AN EXISTING PORTABLE INSTALL
------------------------------------

Use Help > Check for Update... to check the newest official Windows release.
When prompted, choose automatic update, open its GitHub release page, or install
later. Automatic update runs only after explicit approval.

SPLINED downloads and verifies the official ZIP and separately published
splined-update.exe. The updater is visible, consistently named, and temporary;
it is never embedded in or extracted from splined.exe. It waits for the exact
GUI/core process IDs, stages and verifies the new pair, performs transactional
replacement with recovery copies and rollback, verifies the result, and
restarts SPLINED. The restarted GUI revalidates both installed files before
removing the updater and staging files. No CMD or PowerShell is used.

Older notification-only builds remain able to detect this schema 2 release and
open its GitHub page. One manual upgrade enables automatic updates for later
compatible releases. Automatic installation additionally requires exact
version/commit, archive/updater URLs, sizes, and SHA-256 digests for every
executable payload; missing integrity metadata is never inferred.

To upgrade manually:

1. Close SPLINED.

2. Use File > Backup > Export Backup... to create a selective .spl backup, or
   preserve the data folder and separately back up the configured credential directory and
   <SQL database directory>\splined.db. A .spl backup may be password protected.

3. Extract the new application files into the existing SPLINED folder,
   replacing splined.exe and the complete runtime folder together while
   preserving the persistent locations listed above.

4. Run splined.exe and verify Settings, Config v5 validation, and credential
   status before a production scan.

Downloaded and derived files under the Temporary Run Cache are disposable and
are removed after a run. Optional review samples remain only when enabled.
Diagnostic files under the configured
log directory are not runtime-state authority.

Opening a registered .spl file starts SPLINED's selective restore dialog.
SPLINED does not otherwise discover, import, or move another installation.
For identical selected categories, restoring the same backup into clean
portable folders at different paths produces equivalent Config v5/UI state;
absolute and UNC resources remain unchanged and unselected categories retain
their existing contents.


SAFETY
------

Write mode changes files in album directories. Test with a backup, snapshot,
copy, or staging library first.

Credential JSON files may contain secrets. Do not publish or commit them.
