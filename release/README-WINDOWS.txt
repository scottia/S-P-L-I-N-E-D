S:P:L:I:N:E:D — WINDOWS PORTABLE
===========================================

WINDOWS GUI: v3.0.0 Stable
CONFIGURATION: Config v5

FRESH INSTALL
-------------

1. Extract the complete archive into the final folder where SPLINED will live.

2. Run:

   splined.exe

3. Complete first-run setup. SPLINED creates and uses its application-owned
   layout as required:

   splined.exe
   splined-core.exe
   splined-watermark.png
   splined-app-icon.png
   app.ico
   config\
      config.toml
      ui.toml
   credentials\
   _cache\
      samples\
   _logs\
      _history\

4. Begin in Read mode with a small media selection. Confirm artwork choices
   and output policy before enabling Write mode.

The GUI executable, processing core, watermark, and icon resources are one
application bundle. Keep those application files together. No setup launcher
or executable rename is required.


UPGRADE AN EXISTING PORTABLE INSTALL
------------------------------------

1. Close SPLINED.

2. Back up these persistent locations:

   config\
   credentials\
   _logs\

   If history has been configured elsewhere, back up that location too.

3. Extract the new application files into the existing SPLINED folder,
   replacing the program files while preserving the persistent locations
   listed above.

4. Run splined.exe and verify Settings, Config v5 validation, and credential
   status before a production scan.

_cache\ is disposable and is recreated as needed.

SPLINED does not automatically discover, import, or move another portable
installation. Copy persistent data deliberately when changing directories.


SAFETY
------

Write mode changes files in album directories. Test with a backup, snapshot,
copy, or staging library first.

Credential JSON files may contain secrets. Do not publish or commit them.
