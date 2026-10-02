# Windows v4 Interface and Runtime

Windows v4 keeps the Windows Forms shell and Rust processing core aligned with
the shared SPLINED authority, ranking, and SQLite rules while using native
desktop controls.

## Interface structure

- The top menu is compact and the `S:P:L:I:N:E:D` wordmark uses the shared
  spectrum colors.
- Major cards use spectrum border accents instead of uniform grey-blue frames.
- Light appearance uses a warm cream canvas so the logo and status colors keep
  useful contrast.
- Select Media contains Artist and Album filters, selection scope, Album scan
  scope, one required READ/LIVE WRITE launch-mode choice, and a two-column
  Folder Status filter. The duplicate launch control was removed.
- The only action that starts or stops processing is the primary **LAUNCH**
  button beneath Artwork Candidates and Preview.
- **View > Panel Layout** supplies Balanced, Wider Select Media, Wider
  Decisions, and Stacked presets. The panel splitters are draggable; a custom
  arrangement and the selected preset persist internally.

## Internal settings and first run

The Config v5 document and interface state are stored in the current user's
internal Windows application settings. `config.toml`, `ui.toml`, and
`config.location` are not Windows runtime authority. The Rust core receives a
temporary Config v5 file for one launch; the GUI removes it after processing.

First run requires:

- music-library directory;
- cache directory (and therefore `splined.db`);
- log directory;
- credential directory.

The selected runtime directories are created only after **Save and Continue**.
Setup extraction does not manufacture application-data folders.

## Backup and restore

**File > Backup** has separate Import and Export commands. A `.spl` backup can
include any combination of:

- internal Config v5 settings;
- interface state and panel layout;
- credential JSON files;
- `splined.db`;
- diagnostic metadata.

Exports use an integrity digest by default. Optional password protection uses
PBKDF2-derived encryption and authentication. Opening an associated `.spl`
file starts SPLINED at the selective restore dialog; it never restores sections
without operator confirmation.

## MusicBrainz Matches

Every Album candidate review exposes **MusicBrainz Matches...**, even when its
original Album/Release ID was valid. Results are grouped by newest-to-oldest
decade and populated release type (Album, Single, EP, Soundtrack, Compilation,
and additional official types). Rows show Artist, country, date, type, release,
known inspected resolution, and a direct MusicBrainz URL.

Selecting a release runs the unchanged configured provider pipeline. **Back to
MB Matches** returns to the same list. The currently inspected release is green
and earlier inspected releases are blue. Re-selecting an inspected release
reconstructs the original candidate order, resolution, diagnostics, and ranking
from its in-memory source-result cache without new provider discovery or image
downloads. **Return to Source Results** restores the Album's original candidate
cards.

The Matches dialog also exposes session-only Artist, Release, and Recording
MBID fields. **Apply IDs** validates canonical UUIDs in the Rust core, verifies
Recording/Artist and Recording/Release relationships where that authority is
available, and rebuilds the list. These corrections choose search authority
only; SPLINED does not write edited MBIDs into media tags.

For iTunes, an exact MusicBrainz release lookup retains official Apple
Music/iTunes URL relationships. Valid collection IDs are tried through the
iTunes lookup endpoint before the ordinary Artist/Album searches, including
both current no-slug Apple Album URLs and older slugged `/id...` forms.

## Curated compilation output

An Album whose representative track has no Album/Release MBID and is tagged
`compilation=1` enters per-track embedded-artwork handling before local folder
art preflight. It never creates, changes, or removes folder `cover.*`.

For each track SPLINED:

1. checks the exact Recording-ID/Artist-ID SQL cache;
2. lazily inspects only indexed Albums under the matching Artist identity;
3. uses a bounded MusicBrainz request only after a local miss;
4. applies the strict automatic release order Album, Soundtrack, Compilation;
5. lets the operator search Artist/Track and inspect broader official releases;
6. evaluates normal configured artwork sources and policies;
7. replaces only the approved embedded front image in LIVE WRITE;
8. commits the track ledger and Album progress immediately.

Incomplete Albums remain resumable, and already verified tracks are skipped.
READ previews the same choices without changing tags, embedded bytes, folder
artwork, or durable completion state.

## Shared invariants

- SQLite remains Album/status authority; an arbitrary existing `cover.*` does
  not become Processed authority.
- Shared SQLite continues to use rollback journal, FULL synchronization, and
  bounded busy handling.
- Source order, source policy, ranking, and deterministic tie-breaking are not
  changed by concurrent provider discovery or interface navigation.
- Credentials, tokens, authorization headers, and private values are never
  emitted to GUI or persistent diagnostics.
- `fixtures/cross-runtime-album-policy.json` is consumed by both the Rust and
  Python policy suites to keep missing-Album-ID eligibility, strict automatic
  release types, and folder-versus-embedded output targets aligned.
