# WebGUI Settings Review and Windows Parity Summary

This document records the reviewed WebGUI/PostgreSQL settings workspace and the
remaining native Windows parity work. It contains no credentials, recovery
tokens, database passwords, or machine-specific test values.

## WebGUI settings now represented

- The settings workspace is wide, user-resizable, divided into task sections,
  and uses left-aligned radio-style Boolean controls. **Settings HELP** opens
  the repository's `main/docs` directory. One **Advanced ON/OFF · All
  Sections** switch controls specialist fields everywhere and remains usable
  even while an unsaved required field is incomplete.
- **Library & Files** presents the music and scan roots, persistent cache,
  temporary cache, log and credential directories, ignored folders,
  PostgreSQL environment ownership, and a non-recursive storage/runtime
  summary. Ignored folder names are comma-separated configuration values and
  do not need shell quotes when they contain spaces.
- A blank V/A / OST Artist Images Directory follows the hard-coded
  `<Cache Directory>/artist-art` rule. The subdirectory is created only when
  first needed. It is an image directory, not a database.
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
- **Processing & Logs** separates processing controls from bounded WebGUI audit
  events. It does not expose credentials. Native shared-SQLite controls are not
  shown because WebGUI uses PostgreSQL.
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

## Native Windows parity checklist

Apply these presentation/default changes to the native Settings workspace when
the Windows update is approved:

1. Change the V/A / OST Artist Images help copy to state that blank uses the
   hard-coded `<Cache Directory>/artist-art` subdirectory, created when needed,
   and that it is an image directory rather than a database.
2. State that ignored folder names are comma-separated and do not require
   quotes for spaces.
3. Replace editable **Filename Stem** and **Formats** controls with fixed
   **Default Filename Stems** (`cover`, `artist`, `banner`) and **Supported
   Image Formats** (JPEG, PNG, WebP) statements, unless native configuration is
   deliberately retaining an advanced-only override.
4. Rename **Write Artist Sample Images** to **Write Sample Cover Images**,
   default it off, require a persistent directory only when enabled, and use
   the same supported-format explanation.
5. Change **Upscale Below Ideal** to default on while keeping the explicit
   off/native-resolution behavior and 200% default maximum.
6. Keep Boolean controls left aligned and radio styled; make the Advanced state
   global across all settings sections and never block its toggle because an
   unrelated unsaved field is incomplete.
7. Add an always-visible Settings Help route, compact and align Artwork Source
   and Connection selectors/fields, and align Backup and Restore action groups.
8. Retain the graphical Enhancement editor, provider policy editor, connection
   testing, storage summary, backup/restore, and processing/log presentation
   already shared with the native design where applicable.

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
