# Release support files

This directory contains active release automation inputs, artifact guidance,
and regression fixtures. It is not a store of hand-maintained release history;
published history belongs in Git tags and GitHub Releases.

## Active automation

- `Generate-ReleaseNotes.ps1`, `Generate-StoreHighlights.ps1`,
  `Resolve-StoreBaseline.ps1`, and `New-LocalStoreReviewBundle.ps1` are called
  by the numbered release workflows. The local review builder packages files
  for download only and never contacts Partner Center.
- `StoreHighlightsPolicy.ps1` keeps over-limit Store notes within Microsoft's
  1,500-character boundary at complete, priority-ordered bullet boundaries and
  adds the full GitHub Release link for omitted details.
- `Install-GitCliff.ps1` installs the pinned release-note engine and validates
  its checksum.
- [`cliff.toml`](cliff.toml) contains the shared release-note and Store
  classification configuration. Historical categories preserve old release
  grouping; they do not declare a current application build target.
- `templates/` contains the active GitHub Release and Microsoft Store Tera
  renderers.
- `Test-ReleaseNoteClassification.ps1`, `Test-StoreBaseline.ps1`, and
  `Test-LocalStoreReviewBundle.ps1` exercise classification, Store history
  selection, cumulative highlights, local-only review audit evidence, and
  failure boundaries.
- `.github/scripts/release_note_baseline.py` resolves the immediately prior
  numeric release. It accepts either a direct ancestor tag or a validated
  temporary release-candidate version stamp and fails closed on unrelated history.
- `tests/` contains intentionally historical, synthetic Microsoft Store JSON
  fixtures. Their old version numbers are regression inputs, not current
  product-version documentation.

## Current guidance

- `README-WINDOWS.txt` ships with the supported Windows portable archive.
- Workflow 23 builds the WebGUI/Python source archive and exact smoke-tested
  container image; Workflow 24 publishes those validated outputs. There is **no supported native Linux
  README or root-native Linux release target** in the two-target `/main` layout.
- macOS is not a release target in the current Workflows 20–24 architecture.

Windows release notes are generated from [`cliff.toml`](cliff.toml) and
`templates/github-release.tera`; a static migration-era release-note file is
not retained here.
