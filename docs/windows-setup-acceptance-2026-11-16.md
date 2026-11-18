# Windows Setup acceptance — 2026-11-16

This acceptance record covers the Windows first-run Setup and supporting
documentation prepared from the v3.0.6 source line. It records portable test
evidence only; it contains no operator paths, credentials, or machine-specific
configuration.

## Accepted behavior

- A clean Portable launch copies the repository's Windows Config v5 example to
  a disposable local `splined-setup.tmp` draft. It creates neither `config.toml`
  nor `ui.toml` until **Save & Open Settings** succeeds, then removes the draft.
- Configuration defaults to the folder containing `splined.exe`. A deliberate
  alternate Configuration Path is recorded in Config v5 and by a path-only,
  installation-scoped Windows bootstrap;
  a different Portable copy cannot inherit a path selected by another test or
  installation.
- Setup initially enables Configuration Path, optional Settings Archive and
  password, and Music Library. Choosing Music Library unlocks category,
  database, cache, log, credential, sample, and related controls.
- Every Setup path displays a concise purpose statement and an individual
  **Guide ↗** link to its exact Windows guide section. Category-path help
  explains precedence and Artist/file-list behavior without exposing a local
  path.
- The top-right Setup action and **Save & Open Settings** share the same
  validation and save workflow.
- A completed first Setup offers a Settings Archive before opening normal
  Settings. The chosen archive directory becomes the saved Settings Backup /
  Restore location.
- New installs start with Amazon enabled for Album and Artist artwork search;
  migration preserves an existing installation's prior provider choice.
- Candidate Filter starts collapsed for new interface state.
- Runtime logs are written to `<Log directory>/run/`. A live debug session was
  inspected and contained startup, API, selection, and Album category audit
  events, confirming that the reported missing log was a subdirectory
  discovery issue rather than a logging failure.

## Automated verification

| Verification | Result |
| --- | --- |
| Root Rust test suite | 235 passed (202 library, 5 public config, 11 release automation, 17 architecture) |
| Windows all-target compile | Passed |
| Windows Rust/SQLite library suite | 563 passed |
| Windows executable/UI suite | 130 passed |
| Windows source format check | Passed |
| Path and credential diff audit | No operator path, IP address, username, password, or API key found |

## Local optimized executable

The repository's incremental optimized installer compiled, PE-validated,
copied, hash-verified, and launched the local Windows test executable without
changing its configuration files.

```text
SHA-256 FAE3CF303F7B4A80C924B133EC75516C960D5C982051D4C9692ABC734E46ED72
```

The executable is a local acceptance artifact, not a release tag or Store
submission.
