S:P:L:I:N:E:D — macOS PORTABLE
=========================================

CONFIGURATION: Config v5

FRESH INSTALL
-------------

1. Extract the complete archive into the final directory where SPLINED will
   live.

2. Restore executable permission if necessary:

   chmod +x ./splined

3. Run SPLINED directly:

   ./splined --help

4. SPLINED creates and uses its application-owned layout as required:

   splined
   config/
      config.toml
   credentials/
   _cache/
      splined.db
      samples/
   _logs/
      run/

The native portable runtime may also create docker_builds/ for local container
build artifacts. It is not required for normal scanning.

No setup launcher or executable rename is required.


UPGRADE AN EXISTING PORTABLE INSTALL
------------------------------------

1. Stop SPLINED.

2. Back up these persistent locations:

   config/
   credentials/
   _cache/splined.db

3. Extract the new application files into the existing directory, replacing
   the program files while preserving the persistent locations above.

4. Restore executable permission when needed and run:

   chmod +x ./splined
   ./splined --help

Other files under _cache/ are disposable and are recreated as needed.
Diagnostic files under _logs/ are not runtime-state authority.

SPLINED does not automatically discover, import, or move another portable
installation. Copy persistent data deliberately when changing directories.


macOS SECURITY
--------------

Because SPLINED is distributed directly, macOS may require first-run approval
in Privacy & Security before allowing the executable to run.


SAFETY
------

Write mode changes files in album directories. Test with a backup, snapshot,
copy, or staging library first.

Credential JSON files may contain secrets. Do not publish or commit them.
