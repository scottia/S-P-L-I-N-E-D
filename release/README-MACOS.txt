S:P:L:I:N:E:D — macOS SETUP
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

3. SPLINED creates the permanent executable and portable data layout:

   ./splined
   config/config.toml
   cache/samples/
   credentials/

   The credentials directory starts empty. SPLINED does not import an old
   OS-level configuration or credentials into a fresh portable install.

4. After setup succeeds, the temporary release files are removed:

   ./setup-splined
   README-MACOS.txt

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

4. Existing portable data is preserved and is not replaced by setup:

   config/
   cache/
   credentials/

5. After a successful upgrade, setup-splined and README-MACOS.txt are
   removed from the application directory.

SPLINED does not automatically discover, import, or move another SPLINED
installation. Move or copy portable data manually when changing directories.


macOS SECURITY
--------------

Because SPLINED is distributed directly, macOS may require first-run
approval in Privacy & Security before allowing the executable to run.


FINAL PORTABLE LAYOUT
---------------------

   splined
   config/
      config.toml
   cache/
      samples/
   credentials/

No executable rename is required.
