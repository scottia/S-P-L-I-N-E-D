# Windows replacement GUI validation

This record covers the replacement WinForms artwork workspace introduced from the
authoritative `SPLINED_GUI_Redesign_Implementation_Pack` references. The legacy
`MainForm` composition remains only as retired implementation code; the production
constructor activates the replacement shell.

## Visual comparison

The WinForms preview harness renders deterministic local artwork and real application
controls. It does not contact providers or fabricate ratings, AI model names, provider
results, or release metadata.

| Reference | Production state captured | Result |
| --- | --- | --- |
| `01_FIND_MATCH_ARTWORK.png` | Top shell, graphical library, dynamic source strip, collapsible filters/editing, dominant Selected Artwork, candidate strip | Composition present |
| `02_EDIT_ARTWORK.png` | Dedicated editing composition with persisted ordered primary controls | Composition present |
| `03_UPSCALE_ENHANCE.png` | Before/after workspace using SPLINED's implemented upscale and adjustment semantics | Composition present |
| `04_MUSICBRAINZ_RESULTS.png` | Existing authority/results behavior embedded in the replacement workspace | Composition present |
| `05_CUSTOM_UPSCALE.png` | Dedicated resizable comparison and editing window opened from Selected Artwork | Composition present |
| Annotated responsive requirements | 1280 x 800 compact layout with horizontal source and candidate scrolling | Composition present |

Generated artifacts are written by `windows/gui/TEST-PREVIEW-WINDOWS-GUI.cmd` to:

- `target-windows/winforms-qa/preview/replacement/actual_01_FIND_MATCH_ARTWORK.png`
- `target-windows/winforms-qa/preview/replacement/actual_02_EDIT_ARTWORK.png`
- `target-windows/winforms-qa/preview/replacement/actual_03_UPSCALE_ENHANCE.png`
- `target-windows/winforms-qa/preview/replacement/actual_04_MUSICBRAINZ_RESULTS.png`
- `target-windows/winforms-qa/preview/replacement/actual_05_CUSTOM_UPSCALE.png`
- `target-windows/winforms-qa/preview/replacement/actual_06_NAVIDROME_CREDENTIALS.png`
- `target-windows/winforms-qa/preview/replacement/actual_07_RESPONSIVE_1280x800.png`

## Functional validation

- Windows GUI self-test: replacement production shell, Config v5 source projection and
  reorder, excluded-source position, horizontal strips, tri-state filters, five-primary
  adjustment state, graphical library, responsive bounds, MusicBrainz integration,
  deterministic Navidrome matching, and credential isolation.
- Root Rust suite: 193 unit tests and 23 integration/architecture tests.
- Windows Rust suite: 273 unit tests.
- Python suite: 271 tests passed, 1 platform-conditional test skipped.
- Release Windows build: `splined.exe` plus the in-process `splined_core.dll` built for
  `x86_64-pc-windows-msvc`.

## Deliberate reference adaptations

- Controls use native WinForms drawing, repository branding, and actual SPLINED data;
  generated illustration assets and unsupported stars/model labels are omitted.
- Custom Upscale is a separate resizable WinForms window so comparison work has a
  focused surface without duplicating it below the main workspace. Closing it restores
  the existing selection and workspace.
- Navidrome is an OpenSubsonic-first, read-only enrichment/candidate source. No schema
  change was necessary: artwork is cached under the configured cache directory and
  deterministic identity is held in the active projection. `splined.db` remains the
  only durable media/workflow authority.
- Optional server path enrichment is reported only when the compatible album response
  supplies paths. It is never used to overwrite canonical SPLINED paths.
