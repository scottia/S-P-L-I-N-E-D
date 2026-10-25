# Windows Setup follow-ups (unreleased)

This is a maintainer checklist for source-only changes awaiting end-to-end
testing. It does not describe the behavior of the currently installed EXEs.
Do not replace an installed EXE or refresh the shared database merely to verify
these UI items.

## Setup wizard

- [ ] **Load a selected `.spl` archive:** Browsing must read and stage the
  archive, not merely display its path. Typing a path and choosing **Load into
  Setup** must do the same. Source change is present; native file-picker tests
  for unprotected and password-protected archives remain.
- [ ] **Gate Save & Open Settings:** A selected archive must load successfully
  before Save is enabled. On load failure, keep Setup open with an actionable
  error; never silently save the prefilled default paths. Source change is
  present; first-run UI verification remains.
- [ ] **Clarify the Setup button:** The header's Setup control previously
  appeared clickable without an action. It is disabled while Setup is already
  displayed in source; verify the behavior in a test build.
- [ ] **Verify shared SQLite guidance in a test build:** Setup and Advanced
  Settings now link to the shared-database guide, describe cross-host locking
  risk, and recommend the extra `.client-lock` when every client can acquire
  it. Python/Docker is the normal first publisher for a cross-mounted shared
  inventory; an explicit Windows Refresh can also update it. Verify the
  guidance fits and the link opens in both screens.

## Related backup verification

- [ ] Confirm manual and scheduled `.spl` and SQLite backup names include the
  machine name, and retention on one PC does not prune another PC's files.
  Source change and unit tests are present; cross-PC UI verification remains.

Test one SPLINED instance at a time for any operation that writes the shared
database. The current MEDIAPC Beyoncé display investigation is separate from
this Setup checklist and should be observed before any refresh.
