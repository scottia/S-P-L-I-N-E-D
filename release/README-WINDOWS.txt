S:P:L:I:N:E:D — WINDOWS SETUP
========================================

SEARCHABLE:PIXEL:LINKS:IDENTIFIED:NORMALIZED:ENRICHED:DEFINED

FRESH INSTALL
-------------

1. Extract the entire archive into the final folder where you want
   SPLINED to live.

2. Run:

   setup-splined.exe

3. SPLINED creates the permanent executable and application-owned layout:

   splined.exe
   config\config.toml
   credentials\
   docker_builds\
   _cache\samples\
   _logs\_history\

   The credentials folder starts empty. SPLINED does not import an old
   AppData configuration or credentials into a fresh portable install.

   _cache is disposable runtime data. Logs and history are kept under _logs.

4. After setup succeeds, the temporary release files are removed:

   setup-splined.exe
   README-WINDOWS.txt

   Windows completes deletion of setup-splined.exe immediately after the
   setup process exits because a running executable cannot delete itself.

5. For all future launches, use:

   splined.exe


UPGRADE AN EXISTING PORTABLE INSTALL
------------------------------------

1. Extract the new archive into the existing SPLINED application folder.

2. Run:

   setup-splined.exe

3. Setup stages and replaces the permanent program executable:

   splined.exe

   If replacement fails, setup attempts to restore the previous executable.

4. Persistent application data is preserved and is not replaced by setup:

   config\
   credentials\
   _logs\

   _cache is disposable and is recreated as needed by scan operations.

5. After a successful upgrade, setup-splined.exe and README-WINDOWS.txt
   are removed from the application folder.

SPLINED does not automatically discover, import, or move another SPLINED
installation. Move or copy portable data manually when changing folders.


FINAL PORTABLE LAYOUT
---------------------

   splined.exe
   config\
      config.toml
   credentials\
   docker_builds\
   _cache\
      samples\
   _logs\
      _history\

No executable rename is required.
