S:P:L:I:N:E:D — WINDOWS PORTABLE
===========================================

WINDOWS GUI: v3.0.0 Stable
CONFIGURATION: Config v5

FRESH INSTALL
-------------

1. Extract the complete archive into the final folder where SPLINED will live.

2. Run:

   splined.exe

3. Complete first-run setup. Choose the required library, cache, log, and
   credential locations. SPLINED stores Config v5 and interface preferences
   internally for the current Windows user, then creates only the selected
   runtime directories after Save and Continue.

   The archive does not create config\, credentials\, _cache\, _logs\,
   docker_builds\, config.toml, ui.toml, or config.location.

4. Begin in Read mode with a small media selection. Confirm artwork choices
   and output policy before enabling Write mode.

The GUI, processing core, watermark, and icon resources are embedded in
splined.exe. No sidecar application files, setup launcher, or executable rename
are required.


UPGRADE AN EXISTING PORTABLE INSTALL
------------------------------------

Use Help > Check for Update... to download, verify, install, and restart the
newest official Windows release. The updater is built into splined.exe and
preserves the persistent locations listed below.

To upgrade manually:

1. Close SPLINED.

2. Use File > Backup > Export Backup... to create a selective .spl backup, or
   separately back up the configured credential directory and
   <cache directory>\splined.db. A .spl backup may be password protected.

3. Extract the new application files into the existing SPLINED folder,
   replacing the program files while preserving the persistent locations
   listed above.

4. Run splined.exe and verify Settings, Config v5 validation, and credential
   status before a production scan.

Other files under _cache\ are disposable and are recreated as needed.
Diagnostic files under _logs\ are not runtime-state authority.

Opening a registered .spl file starts SPLINED's selective restore dialog.
SPLINED does not otherwise discover, import, or move another installation.


SAFETY
------

Write mode changes files in album directories. Test with a backup, snapshot,
copy, or staging library first.

Credential JSON files may contain secrets. Do not publish or commit them.
