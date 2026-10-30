# WebGUI Settings Review and Windows Parity Summary

This document records the reviewed WebGUI/PostgreSQL settings workspace and the
remaining native Windows parity work. It contains no credentials, recovery
tokens, database passwords, or machine-specific test values.

## WebGUI settings now represented

- The settings workspace is wide, user-resizable, divided into task sections,
  and uses left-aligned radio-style Boolean controls. **Settings HELP** opens
  `docs/config-v5-reference.md`. One **Advanced ON/OFF · All
  Sections** switch controls specialist fields everywhere and remains usable
  even while an unsaved required field is incomplete.
- **Library & Files** presents the music root, selectable Scan Mode and its
  conditional optional Scan Folder, Persistent Cache Directory, Temporary
  Cache Directory, V/A / OST Artist Images Directory, Provider Credential
  Directory, ignored folders,
  PostgreSQL environment ownership, and a non-recursive storage/runtime
  summary. Ignored folder names are comma-separated configuration values and
  do not need shell quotes when they contain spaces.
- A blank V/A / OST Artist Images Directory uses Persistent Cache Directory
  itself. The operator may select any other persistent server-visible directory
  or preferred folder name. It is an image directory, not a database.
- **Artwork & Embedding** shows `cover`, `artist`, and `banner` as the required
  default filename stems and JPEG, PNG, and WebP as the supported static image
  formats. Both are informational rather than free-form inputs. Preserve,
  square/crop, round-to, and embedded-thumbnail preferences remain editable.
  **Write Sample Cover Images** defaults off and enables its persistent path
  only when selected.
- **Enhancement** includes the OLED Chroma Wheel, editable banner names and
  colors, album-card frame colors, live preview, Default and Custom A-D slider
  presets, detailed adjustment sliders, resolution targets, the 100-800%
  upscale ceiling, and final-image evaluation. WebGUI **Upscale Below Ideal**
  defaults on; disabling it preserves native source resolution.
- **Artwork Sources** includes graphical provider ordering and a selected
  provider policy editor. Provider, range, and optional dimension controls use
  one compact aligned width. Optional numeric fields display `Optional`, and
  **Update Selected Source Policy** saves the selected provider's policy.
- **Connections** includes one compact provider selector and aligned provider
  fields, saved-state masking, Reload/Test/Save actions, and provider-specific
  secret retention. AISPLINED remains visible as a future optional provider,
  selectable but disabled by default.
- **Backup & Recovery** aligns the safe JSON and shared-contract download
  actions with the restore file and restore action. Catalog Maintenance appears
  directly below Restore and keeps full PostgreSQL refresh and deliberate
  read-only SQLite import separate from routine incremental updates.
- **Processing & Logs** separates processing controls from Logging, including
  Log Directory, diagnostic level (`none`, `info`, `debug`, `warning`, or
  `error`), retention, and bounded WebGUI audit events. `none` suppresses
  runtime/access logging after restart without removing PostgreSQL events. The
  inactive legacy Full Library Scan toggle is no longer displayed.
  It does not expose credentials. Native shared-SQLite controls are not shown
  because WebGUI uses PostgreSQL.
- The shared, secret-free presentation contract is
  `schema/settings-ui.json`. Safe settings backup includes WebGUI palette and
  enhancement state but excludes provider secrets and PostgreSQL contents.

## WebGUI administrator behavior

- A new network deployment creates one administrator with username, password,
  confirmation, and the deployment's `SPLINED_WEB_TOKEN`.
- PostgreSQL stores only the administrator username and salted password hash.
  The deployment token remains an environment secret and is not copied into
  PostgreSQL, Config v5, settings JSON, logs, or browser responses.
- **Forgot password?** validates the existing deployment token, replaces only
  the administrator password hash, invalidates active browser sessions, and
  leaves the catalog, provider credentials, and PostgreSQL connection intact.

## Native Windows parity applied

- Library & Files now owns Scan Mode and its conditional optional Scan Folder.
  Processing & Logs owns Log Directory. Temporary Cache Directory and Provider
  Credential Directory use the same reviewed names as the shared contract.
- Artwork & Embedding displays fixed **Default Filename Stems** (`cover`,
  `artist`, `banner`) and **Supported Image Formats** (JPEG, PNG, WebP), and its
  preserve policy explicitly includes banner files.
- **Write Sample Cover Images** defaults off and reveals its directory only
  when selected. **Upscale Below Ideal** defaults on, while 200% remains the
  default maximum and disabling the option preserves native source resolution.
- Enhancement uses the standard section glyph. **Settings HELP** is visible in
  the Settings header and opens `docs/config-v5-reference.md`.
- Diagnostic logging includes `none`, which disables native diagnostic files
  after restart without hiding operational status. The inactive Full Library
  Scan setting is no longer displayed.
- Existing graphical Enhancement, provider-policy, connection, storage,
  backup/restore, and processing presentations remain intact.

Windows intentionally keeps **Database Directory** as its SQLite location and
requires a separate selectable **V/A / OST Artist Images Directory**. It does
not inherit the WebGUI blank-directory fallback because putting images in the
Windows SQLite directory would violate the approved database separation.

Do not port WebGUI-only PostgreSQL connection, catalog import, WebGUI
administrator authentication, browser session, or container path controls into
the native Windows application. Windows retains its separate SQLite catalog and
Microsoft Store/runtime security model.

## Acceptance checks

- No plaintext login password or deployment token may appear in application
  logs, audit-event details, Config v5, safe settings JSON, the shared settings
  contract, or repository files.
- Saving one settings section must not overwrite unrelated sections.
- Advanced ON/OFF must apply consistently after switching sections.
- Fixed filename and format statements must not create writable bindings.
- WebGUI/PostgreSQL tests, settings-contract checks, and JavaScript syntax must
  pass before repository promotion.
