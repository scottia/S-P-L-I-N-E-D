S:P:L:I:N:E:D — WINDOWS PORTABLE
===========================================

WINDOWS GUI: v3.0.0 Stable
CONFIGURATION: Config v5

FRESH INSTALL
-------------

1. Extract the complete archive into the final folder where SPLINED will live.

2. Run:

   splined.exe

   Keep splined-core.exe beside it; this fixed worker is required for library
   snapshots, Album scans, and artwork edits.

3. Complete first-run setup. Choose the required library, SQL database,
   temporary run cache, log, and credential locations. SPLINED stores Config v5 and interface preferences
   internally for the current Windows user, then creates only the selected
   runtime directories after Save and Continue.

   Database, run-cache, log, and credential fields initially point beneath:

   %LOCALAPPDATA%\SPLINED

   They remain editable. Existing saved locations and UNC paths are never
   changed automatically. %PROGRAMDATA% is not used for per-user defaults.

   The archive does not create config\, credentials\, _cache\, _logs\,
   docker_builds\, config.toml, ui.toml, or config.location.

4. Begin in Read mode with a small media selection. Confirm artwork choices
   and output policy before enabling Write mode.

The archive contains the fixed splined.exe GUI and fixed splined-core.exe
worker. No setup launcher or executable rename is required. Normal startup and
scanning do not extract, generate, replace, or delete executable files.

Config v5 is handed from the GUI to the Rust core in memory. No runtime TOML is
created in the portable folder, cache directory, or user profile.


UPGRADE AN EXISTING PORTABLE INSTALL
------------------------------------

Use Help > Check for Update... to check the newest official Windows release and
open its GitHub release page. SPLINED does not download, install, execute,
self-replace, or relaunch update binaries.

To upgrade manually:

1. Close SPLINED.

2. Use File > Backup > Export Backup... to create a selective .spl backup, or
   separately back up the configured credential directory and
   <SQL database directory>\splined.db. A .spl backup may be password protected.

3. Extract the new application files into the existing SPLINED folder,
   replacing splined.exe and splined-core.exe together while preserving the
   persistent locations listed above.

4. Run splined.exe and verify Settings, Config v5 validation, and credential
   status before a production scan.

Downloaded and derived files under the Temporary Run Cache are disposable and
are removed after a run. Optional review samples remain only when enabled.
Diagnostic files under the configured
log directory are not runtime-state authority.

Opening a registered .spl file starts SPLINED's selective restore dialog.
SPLINED does not otherwise discover, import, or move another installation.


SAFETY
------

Write mode changes files in album directories. Test with a backup, snapshot,
copy, or staging library first.

Credential JSON files may contain secrets. Do not publish or commit them.
