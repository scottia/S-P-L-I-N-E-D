# PR #168 acceptance and promotion record

PR #168 completes the reviewed catalog-projection, COMP/Fallback, Artist Art,
MusicBrainz workspace, Album embedding, and disposable-state corrections for
the 2.0.35 baseline. This record defines the accepted scope promoted through
`dev` to `main`; it is not a release announcement.

## Accepted behavior

- Windows and WebGUI project real per-track Artist credits for identified
  shared releases without manufacturing joined Artist identities.
- Soundtrack, Various Artists, featured-Artist, and COMP fallback appearances
  retain their distinct presentation while synthetic owners remain hidden.
- Albums, Artist COMP reports, and COMP fallback tracks use one MusicBrainz
  results workflow per client, with exact hover/selection artwork authority.
- COMP processing retains one Selected Artwork write action, optional provider
  candidate generation, accessible STOP PROCESS, cancellation, and relaunch.
- Album embedding uses the explicitly selected prepared artwork, replaces the
  competing static `cover.jpg` or `cover.png`, and preserves WebP source files.
- Artist Art supports independent portrait and banner selection, local
  discovery, provider policy, and consistent candidate capability reporting.
- Temporary conversion artifacts, reset targeting, database coordination, and
  Windows/WebGUI status projections retain the tested 2.0.35 ownership rules.

## Promotion requirements

Promotion requires all selected GitHub checks to pass at the exact PR head,
including the rendered WebGUI/PostgreSQL browser workflow. The installed
Windows build and isolated WebGUI must use the same test media library and
their configured test databases. Production media and databases are outside
this acceptance record.

## Explicit boundaries

- Windows retains its SQLite catalog authority for this change.
- WebGUI retains its PostgreSQL catalog and WebGUI-exclusive fields.
- No database-authority redesign is included.
- Reserved migrations 018 and 019 are unchanged.
- The historical full-library Artist-count investigation is not declared
  resolved solely from the smaller isolated test library.
- PR #150's separate live catalog synchronization proposal is cancelled and is
  not incorporated by this promotion.

## Validation evidence

The PR conversation contains the detailed root-cause table, live Windows and
WebGUI interaction results, reset-isolation checks, MusicBrainz timing data,
cross-client COMP evidence, installed revisions, and automated suite totals.
This document deliberately records the accepted contract without duplicating
volatile test-run identifiers.

