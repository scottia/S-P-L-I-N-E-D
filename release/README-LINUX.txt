S:P:L:I:N:E:D — LINUX SETUP
========================================

SEARCHABLE:PIXEL:LINKS:IDENTIFIED:NORMALIZED:ENRICHED:DEFINED

FRESH INSTALL
-------------

1. Extract the entire archive into the final directory where you want
   SPLINED to live.

2. Run:

   ./setup-splined

   If executable permission needs to be restored:

   chmod +x ./setup-splined

3. SPLINED creates the permanent executable and application-owned layout:

   ./splined
   config/config.toml
   credentials/
   docker_builds/
   _cache/samples/
   _logs/_history/

   The credentials directory starts empty. SPLINED does not import an old
   OS-level configuration or credentials into a fresh portable install.

   _cache is disposable runtime data. Logs and history are kept under _logs.

4. After setup succeeds, the temporary release files are removed:

   ./setup-splined
   README-LINUX.txt

5. For all future launches, use:

   ./splined


UPGRADE AN EXISTING PORTABLE INSTALL
------------------------------------

1. Extract the new archive into the existing SPLINED application directory.

2. Run:

   ./setup-splined

3. Setup stages and replaces the permanent program executable:

   ./splined

   If replacement fails, setup attempts to restore the previous executable.

4. Persistent application data is preserved and is not replaced by setup:

   config/
   credentials/
   _logs/

   _cache is disposable and is recreated as needed by scan operations.

5. After a successful upgrade, setup-splined and README-LINUX.txt are
   removed from the application directory.

SPLINED does not automatically discover, import, or move another SPLINED
installation. Move or copy portable data manually when changing directories.


FINAL PORTABLE LAYOUT
---------------------

   splined
   config/
      config.toml
   credentials/
   docker_builds/
   _cache/
      samples/
   _logs/
      _history/

No executable rename is required.
