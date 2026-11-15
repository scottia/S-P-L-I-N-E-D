# PR #185 Windows acceptance and promotion record

PR #185 carries the native Windows startup, catalog projection, Artist Art,
settings, and credential-safety corrections accepted for promotion from its
feature branch through `dev` to `main`. This record documents the approved
source and PC-test evidence without publishing machine-specific paths or
credential values.

## Accepted corrections

### Startup and catalog projection

- Replaced quadratic startup projection scans with normalized lookup maps.
- Kept SQLite as the Windows catalog authority and performed no music-file
  rescan to repair the resident projection.
- Preserved identified Albums in the Albums view and grouped only missing
  MusicBrainz Album ID rows into the Album-keyed Fallback view.
- Verified the operator-priority COMP/Fallback result at 81 grouped Albums.
  Ordinary Artist-count parity remains outside this acceptance claim.
- Removed totals from the Artists, Albums, and Fallback tab labels; operational
  counts remain available inside the relevant view.

### Artist identity and artwork

- Preserved real multi-value artist credits without manufacturing a combined
  selectable Artist identity.
- Restored folder-cover thumbnails for COMP/Fallback Album cards while keeping
  individual COMP artwork review strictly track-scoped.
- Made round thumbnails the default presentation for Artist lists, Artist
  Overview, Navidrome, and Similar Artists, including a saved banner fallback
  when no portrait exists.
- Changed the native Artist Art workflow to match the current WebGUI sequence:
  entering Artists Art Enhancements does not search; the explicit LAUNCH action
  performs the Artist search.
- Artist Art provider identity now uses only the indexed selected Artist MBID.
  It never borrows a Navidrome-derived Album-owner identity for a projected
  Artist.
- Navidrome Artist Art uses an exact-name lookup and contributes one
  high-resolution candidate, matching the WebGUI candidate contract.

The live Rob Wasserman regression check returned the same five Rob-only results
as WebGUI: Deezer, two Discogs results, iTunes, and Navidrome. Fanart was not
queried because the indexed projected Artist has no Artist MBID, and Aaron
Neville did not appear.

### Settings, paths, and credentials

- Added Configuration Storage to Setup and Library & Files. An explicit save
  can relocate `config.toml` and `ui.toml` atomically and remove the superseded
  application-owned copies.
- Replaced folder Browse actions with the expandable directory picker used by
  Setup and Settings.
- Kept ordinary non-Library settings saves from reopening or reprojecting the
  SQLite catalog.
- Preserved the saved confirmation-message preference.
- Masked populated usernames, passwords, API keys, and tokens in the editor.
  Saving an unchanged mask preserves the stored value; entering a replacement
  validates and atomically overwrites it.
- No credential value, credential document, local configuration path, or test
  environment address is included in this PR or its documentation.

## Validation

The approved working tree passed:

| Validation | Result |
| --- | ---: |
| Windows desktop/UI/runtime tests | 126 passed, 0 failed |
| Windows core/library tests | 551 passed, 0 failed |
| Windows executable architecture tests | 16 passed, 0 failed |
| `git diff --check` | passed |
| Quick incremental release build | passed |
| Installed executable SHA-256 verification | passed |

The locally installed acceptance build had SHA-256
`D25CD11FD3A4BA720106F796AE571EC8185F287864039F5C8A4597CC94057BDD`.
It launched successfully, remained responsive, and was left running on the
verified Rob Wasserman Artist Art results. The test did not write artwork,
change settings, or modify music files.

## Promotion boundary

The accepted source is promoted through PR #185 into `dev`, then through a
separate `dev`-to-`main` pull request after required checks pass. Release tags
and release workflows remain separate automation; this acceptance record does
not create or publish a release.
