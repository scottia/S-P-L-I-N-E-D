# Release support files

This directory contains active release automation inputs, artifact guidance,
and regression fixtures. It is not a store of hand-maintained release history;
published history belongs in Git tags and GitHub Releases.

## Active automation

- `Generate-ReleaseNotes.ps1`, `Generate-StoreHighlights.ps1`, and
  `Resolve-StoreBaseline.ps1` are called by the numbered release workflows.
- `StoreHighlightsPolicy.ps1` keeps over-limit Store notes within Microsoft's
  1,500-character boundary at complete, priority-ordered bullet boundaries and
  adds the full GitHub Release link for omitted details.
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
- Workflow 01 separately builds and publishes the WebGUI/Python GHCR image
  and can attach a WebGUI source archive. There is **no supported native Linux
  README or root-native Linux release target** in the two-target `/main` layout.
- macOS is not a release target in the current `/dev` Workflow 01 source.

Windows release notes are generated from `cliff.toml` and
`templates/github-release.tera`; a static migration-era release-note file is
not retained here.
