# Release support files

This directory contains active release automation inputs, artifact guidance,
and regression fixtures. It is not a store of hand-maintained release history;
published history belongs in Git tags and GitHub Releases.

## Active automation

- `Generate-ReleaseNotes.ps1`, `Generate-StoreHighlights.ps1`, and
  `Resolve-StoreBaseline.ps1` are called by the numbered release workflows.
- `Install-GitCliff.ps1` installs the pinned release-note engine and validates
  its checksum.
- `templates/` contains the active GitHub Release and Microsoft Store Tera
  renderers.
- `Test-ReleaseNoteClassification.ps1` and `Test-StoreBaseline.ps1` exercise
  classification, Store history selection, cumulative highlights, and failure
  boundaries.
- `tests/` contains intentionally historical, synthetic Microsoft Store JSON
  fixtures. Their old version numbers are regression inputs, not current
  product-version documentation.

## Current guidance

- `README-WINDOWS.txt` ships with the supported Windows portable archive.
- `README-LINUX.txt` documents the separate native Linux source build. The
  numbered release workflow does not currently publish a Linux archive.
- macOS instructions and development artifacts exist only on the experimental
  `dev-macos` branch. macOS is not a release target on `main` or `dev`.

Windows release notes are generated from `cliff.toml` and
`templates/github-release.tera`; a static migration-era release-note file is
not retained here.
