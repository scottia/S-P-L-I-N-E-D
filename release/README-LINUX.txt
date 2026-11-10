S:P:L:I:N:E:D — LINUX NATIVE SOURCE BUILD
==================================================

CONFIGURATION: Config v5

BUILD AND INSTALL
-----------------

1. Install stable Rust and check out the intended SPLINED tag or branch.

2. Build the locked root-native command-line implementation:

   cargo build --locked --release

3. Copy target/release/splined into a writable application directory. Restore
   executable permission if necessary:

   chmod +x ./splined

4. Run SPLINED directly:

   ./splined --help

5. SPLINED creates and uses its application-owned layout as required:

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

The numbered release workflow does not publish a Linux archive. This file
documents a deliberate source build; no setup launcher or executable rename is
required.


UPDATE AN EXISTING SOURCE BUILD
-------------------------------

1. Stop SPLINED.

2. Back up these persistent locations:

   config/
   credentials/
   _cache/splined.db

3. Check out the intended new tag or branch, run the same locked release build,
   and replace only the `splined` program while preserving the persistent
   locations above.

4. Restore executable permission when needed and run:

   chmod +x ./splined
   ./splined --help

Other files under _cache/ are disposable and are recreated as needed.
Diagnostic files under _logs/ are not runtime-state authority.

SPLINED does not automatically discover, import, or move another portable
installation. Copy persistent data deliberately when changing directories.


SAFETY
------

Write mode changes files in album directories. Test with a backup, snapshot,
copy, or staging library first.

Credential JSON files may contain secrets. Do not publish or commit them.
