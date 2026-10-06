# Windows Guide

This is the single user guide for the native Windows application in the current
S:P:L:I:N:E:D release. It covers the Windows Forms interface, side-by-side Rust
processing core, Config v5 settings, SQLite media database, updates, backup,
and troubleshooting. Historical Windows build notes belong in GitHub Releases,
not in the current product documentation.

## Portable application model

The Windows release is distributed as a fixed `splined.exe` GUI with a fixed
side-by-side `splined-core.exe` worker. The application can be extracted to any
writable final folder and that folder can later be moved or replaced without
moving the media library or selected persistent data.

| Data | Authority and location | Lifecycle |
| --- | --- | --- |
| GUI program | `splined.exe` in the selected portable folder | Replaced manually from an official release |
| Processing worker | `splined-core.exe` beside the GUI | Replaced together with the GUI |
| Config v5 | Current user's internal Windows application settings | Persists until reset or restored |
| Interface state | Current user's internal Windows application settings | Persists theme, layout, filters, and selection |
| SQL database | User-selected database directory; first-run default is `%LOCALAPPDATA%\SPLINED\cache` | Persistent; contains `splined.db` |
| Temporary run cache | User-selected disposable directory; first-run default is `%LOCALAPPDATA%\SPLINED\run-cache` | Candidate and derived images are removed after the run |
| Logs | User-selected log directory; first-run default is `%LOCALAPPDATA%\SPLINED\logs` | Diagnostic and disposable |
| Credentials | User-selected credential directory; first-run default is `%LOCALAPPDATA%\SPLINED\credentials` | Persistent and sensitive |
| GUI-to-core configuration | Private child-process environment | Memory-only; disappears with the child process |

`%PROGRAMDATA%` is not used for per-user defaults because it is shared by all
users on the computer. `%LOCALAPPDATA%` is the correct Windows per-user data
location. The database, temporary-cache, log, and credential fields remain editable, including to
UNC or mapped-drive locations. Existing saved paths are never moved
automatically.

The portable folder does not require or create `config.toml`, `ui.toml`,
`config.location`, `config`, `credentials`, `_cache`, `_logs`, `docker_builds`,
or a runtime configuration file. Python, Docker, Linux, and macOS still use a
file-backed Config v5 document.

## First run

1. Extract the complete Windows archive into its final folder, keeping
   `splined.exe` and `splined-core.exe` together.
2. Run `splined.exe`.
3. Choose the required music-library, SQL database, temporary run cache, log,
   and credential directories.
4. Review Read/Write, artwork, range, source, history, and shared-SQLite
   settings under **Advanced**.
5. Select **Save and Continue**.
6. Configure enabled provider credentials through **File > Credentials...**.
7. Begin with **Filtered Scan [READ]** and a small Album selection.

The first-run fields are mandatory. SPLINED validates them before saving and
creates the selected database, temporary-cache, log, and credential directories only after
**Save and Continue**. Merely extracting or opening Setup does not create those
directories.

Settings are later available from **File > Settings...**. **Validate Saved
Settings** checks the internal Config v5 record without exporting it. The GUI
passes a validated in-memory copy to each Rust snapshot or Album process; the
display label `Windows internal settings` is never treated as a filename.

## UNC, NAS, and mapped paths

The Windows library, database, run-cache, log, and credential fields accept normal absolute
paths, mapped drives, and UNC paths. A UNC path is preferred when a drive letter
may vary between devices or sessions.

For a database shared with Python/Docker:

- point both runtimes at the same physical `splined.db`;
- enable `sqlite_shared` in both runtimes;
- let Python/Docker own library inventory and ignored-folder policy;
- use **File > Refresh Library Index** in Windows after the Python index is
  rebuilt;
- keep rollback journal mode, FULL synchronization, and the configured busy
  timeout—do not weaken durability to hide network latency.

Windows maps the canonical Linux library root stored in the shared database to
the configured Windows library root for file I/O. Unicode normalization and
legacy-decoded SMB names are resolved against physical directory entries; the
user should not rename an Album merely because its indexed and visible Windows
forms differ.

See [SPLINED media database](splined-media-database.md) for ownership,
journaling, schema, and path-mapping details.

## Main window

The compact top bar contains the multicolor `S:P:L:I:N:E:D` wordmark and four
menus:

- **File** — Settings, Credentials, Backup import/export, Refresh Library
  Index, and Exit;
- **View** — System/Dark/Light appearance, panel-layout presets, and the
  persisted **Show Artwork** switch;
- **Status** — configured providers and credential state;
- **Help** — documentation, update check, and About.

Major panels use the spectrum border colors. Light appearance uses a warm
cream canvas so artwork, state colors, and the watermark retain contrast.
**View > Panel Layout** offers Balanced, Wider Select Media, Wider Decisions,
and Stacked presets. Panel dividers remain draggable; a custom layout persists
for the current user. Media Library Selection uses a scroll canvas contained
inside its spectrum frame, while Scan Activity and Decisions and Artwork
Candidates and Preview retain independent scroll canvases. This prevents the
native scrollbar from painting across the Media Selection frame in Stacked
layout and keeps controls reachable when a panel is reduced.

Windows uses the same red/green state language throughout the picker, candidate
cards, Settings, credentials, and Backup surfaces. A round indicator has a red
outline when off and becomes solid green when on; no checkbox/checkmark glyph
is used. Folder-tree selection uses the same round red/green states.

### Media Library Selection

The hollow double-left arrow at the upper right of the title reduces Media
Library Selection to a narrow spectrum-framed rail instead of removing it.
The double-right arrow on that rail restores the selector. Both arrows use the
larger header font and remain visible in the dark theme.

Select Media contains:

- Artist and Album text filters;
- Select `[ALL]`, `[NONE]`, and `[FILTERED]` scope;
- Auto Scan `[ALL]` or `[SELECTED]`;
- exactly one launch mode: Filtered Scan `[READ]` or `[LIVE WRITE]`;
- Folder Status filters and counts;
- a persistent **Show Tracks** view switch below Bypassed;
- the Artist/Album tree.

**Show Tracks** leaves the normal Album view unchanged while off. When switched
on after focusing an indexed compilation, the tree shows that Album and its
tracks in filename order. With no compilation focused, the library root (for
example, `music`) contains the physical Artist/Album hierarchy and can be
expanded manually. Filenames longer than 75 characters are shortened in the
tree; hovering always shows the complete filename. A green `●` identifies a
track whose completed embedded-art write is recorded in SQLite; `○` identifies
a track without that completion record.

Checking a track selects only that track and changes the primary action to
**LAUNCH (1 TRACK)**. Launching reopens the track's current embedded front image
as the local candidate when present, including tracks already completed in an
earlier run. The existing compilation editor, preview, and LIVE WRITE safety
path are reused; no `cover.*` file is created or changed. The hierarchy and
completion markers come from the warm SQLite index. If an older compilation
does not yet have a complete cached filename list, expanding that Album performs
one non-recursive directory listing for that folder only; it does not scan the
library or open audio tags. The Show Tracks choice and focused track survive normal
focus changes, closure, and interface-settings backup. Completing the targeted
run consumes that one queued track while leaving Show Tracks enabled.

The only control that starts or stops processing is the primary **LAUNCH**
button beneath the Artwork Candidates pane. There is no second Launch button
inside Select Media.

The `«` control at the top right reduces the Media Album Selector to its `≫`
reopen rail and returns the remaining space to processing. **View > Show Media
Album Selector** also restores it and preserves the choice in Windows interface
settings. If the selector is reduced when a selected batch finishes
successfully, Windows restores it automatically so the completed status and the
next selection are visible.

Auto Scan is opt-in. With neither Auto Scan choice active, selected Albums form
an operator-reviewed queue. **Auto Scan `[SELECTED]`** processes only explicitly
selected Albums, while **Auto Scan `[ALL]`** processes every Unprocessed Album
plus any Album selected explicitly. Read versus Live Write remains an
independent, required choice; changing scan scope never changes mutation mode.
At application startup the two Launch controls initialize from Config v5
`mode`; an older saved interface preference cannot silently replace a configured
LIVE WRITE session with READ. A deliberate Launch-mode change still applies to
the current session and is recorded as `windows.batch.start` before processing.

The unattended path accepts only a candidate classified `Ideal`, accepted by
its active source policy, and eligible under strict validation when enabled.
An enlarged candidate must also begin at or above the global `Minimum`; resize
projection cannot promote a source image from `BelowMinimum` into Preferred or
Auto eligibility. If no candidate qualifies, Windows pauses for operator
review. `Ideal` verifies configured resolution, geometry, and source policy—not
the picture's semantic accuracy. It cannot by itself detect merchandise,
inserts, disc cases, or unrelated photographs returned by a provider.

Select `[ALL]` only changes the checked Album set. It never enables Auto Scan.

Ctrl+Click supports independent Album selections. Selecting multiple Artists
selects their normally eligible Albums while leaving orange Processed Albums
manual. Bulk selection matches Python:

- **Select `[ALL]`** replaces the current selection with only the active
  Artist's Unprocessed Albums;
- **Select `[NONE]`** clears selection and temporary bypass overrides;
- **Select `[FILTERED]`** requires Artist or Album filter text, replaces the
  current selection, and includes text-matching Unprocessed and Processed
  Albums. Folder Status visibility does not redefine that text-filter scope.

### Album status

SQLite—not the presence of an arbitrary `cover.*` file—owns Album status.

| Album color | Meaning |
| --- | --- |
| White | Unprocessed |
| Orange | Processed by SPLINED; manually reprocessable |
| Red | Bypassed; removal must be confirmed before processing |
| Purple | Timeout active |
| Blue | Started but unfinished compilation track-art work |

Artist colors aggregate the Album states: green means complete, purple means
partial, and blue means the Artist contains a bypass. See [Select Media and
status colors](media-filter-status-colors.md) for the full state model.

A successful LIVE WRITE completion updates the resident Album and Artist state
immediately, then reloads the authoritative SQLite snapshot. This prevents a
completed Album from remaining White while the final database reconciliation
is in progress. READ results remain non-mutating and never become Processed;
their report says **Albums reviewed**, not **Albums processed**.

## Read and Live Write

**Filtered Scan [READ]** runs authority, provider, image, ranking, and preview
logic without installing folder artwork, replacing embedded artwork, or
committing completion state.

**Filtered Scan [LIVE WRITE]** can alter Album files. For an ordinary Album it
installs the approved folder artwork according to Config v5. For an eligible
curated compilation it replaces only the approved track's embedded front image.

The Launch queue is a snapshot of selected Albums. A completed or
operator-stopped Album is consumed from that queue; a failed Album and Albums
not yet started remain selected. After the batch, LAUNCH becomes available for
another independent selection.

## Artwork candidates and preview

**Scan Activity and Decisions** is divided into Album information on the left
and an Artwork surface on the right. Selecting any Album—including an orange
Processed Album—shows the SQLite-indexed Artist, Album, year, track count,
status, path, root-file count, current `cover.*` filename, and recorded
resolution. The image is loaded from the indexed local cover path; selecting an
Album does not rerun providers or rescan the Album directory. The in-memory
preview cache checks the file size and modification time, so artwork replaced
at the same path by another supported runtime is reloaded on the next selection.

The Album information follows the Album currently being processed, then returns
to the Album focused manually in the tree. It clears when neither context has
an Album and uses that Album's status color. Both Album information and Artwork
use the same spectrum frame as the other major work surfaces.

The horizontal divider above Artwork Candidates also controls the Artwork
surface geometry. Moving it up reduces the Artwork column with the available
height; moving it down expands the column only while a square Artwork panel can
coexist with the minimum Album/activity width. The divider is constrained at
that point instead of stretching the Artwork panel.

**View > Show Artwork** hides or restores the right-side Artwork surface and is
saved as Windows interface state. When shown, **Enable Hover** routes candidate
image and candidate-URL hover into that surface. Leaving the candidate restores
the selected Album cover. Clicking a candidate URL also places its cached image
in the Artwork surface before the authority URL opens. When Show Artwork is
off, Enable Hover retains the floating resizable preview behavior.

MusicBrainz review is an intentional exception to the ordinary hover switch.
While MusicBrainz Matches is active, it replaces the left Scan Activity surface
and temporarily shows the existing Artwork surface at right even if **Show
Artwork** was off. Hovering or selecting an artwork `[URL]` previews the
release-group front image there without requiring **Enable Hover**. Leaving the
URL restores the selected Album cover; leaving MusicBrainz Matches restores
Scan Activity and the user's saved Show Artwork preference.

The processing panel reports local evidence, authority resolution, provider
timings, candidate policy, strict-content evidence, and errors. The Artwork
Candidates pane displays every manually reviewable strict candidate.
An unverified strict candidate is marked **Manual only**: its image and URL stay
available, but it cannot become Recommended or be accepted by Auto Scan.

The spectrum-framed **Artwork Filter** activation button remains above the
candidate cards. Opening it uses the same workspace-swap pattern as
MusicBrainz Matches: the four-column filter replaces **Scan Activity and
Decisions** beside **Selected Album Artwork**, so it never drops over or moves
the source thumbnails. The four compact columns and every Advanced control
use the app's multicolor spectrum frame. They use the same framed-group
structure, headings, padding, and row spacing as **Select Media**, rather than
stretching loose controls across the window. Filtering does not rerank
providers or change the underlying result set. The expanded surface is built
from the current Album:

- **Source Selection** follows enabled Artwork Source Priority and lists only
  sources that actually returned a result;
- **Image Type** keeps Recommended, Upscalable, Rejected, and Local visible;
- **Policy** keeps Acceptable and Strict visible;
- **Wanted** and **Unwanted** classify the original downloaded short side,
  before crop or enlargement.
- **Upscale / Advanced** holds the saved enlargement preview and processing
  profile.

The four result columns are separated by vertical rules and use compact fixed
widths rather than stretching across the entire Activity workspace. Each
filter result column reserves a measured option-name cell immediately before
its count. Explicit absolute row heights keep every available choice together
at the top instead of distributing the final choices down the full panel. The
expanded surface and its contents share one rounded spectrum frame and scroll
when the window is narrower than the compact layout. Candidate Findings, Source
Selection, and Resolution form the top row; Upscale / Advanced uses the full
row beneath them. Every category
and option remains in its expected location with a live candidate count;
zero-count choices are disabled and gray rather than removing Wanted or
changing the panel structure between Albums. Changing a source, type, policy, or
range immediately updates the visible cards and recalculates the other
dimensions. Choices with no possible result are automatically unchecked and
grayed; they restore automatically when another selection makes them possible
again. Source, type, policy, and range exclusions are saved immediately in
Windows Interface Settings. They therefore remain unchanged for later Albums,
concurrent batches, and application restarts until the operator changes them;
Interface Settings export also includes them in a `.spl` backup. Counts sit
directly beside their option names so the expanded filter
uses only the width its current results require. When an Album with an existing
`cover.*` has focus, that local file is loaded as an editable result immediately;
the filter therefore remains available without launching provider discovery or
changing Album history. With no focused local cover and no active provider
results, the dropdown disables and collapses instead of opening an empty
surface. **Show all results** clears explicit exclusions.

- **Use Selected** approves one active provider candidate. For a focused
  existing cover it becomes **Save Existing** and applies the previewed profile
  directly through the native safe-write pipeline in Write mode.
- **Compare** compares multiple active candidates.
- **Skip Album** leaves the current Album unchanged and applies the selected
  bypass behavior.
- **Enable Hover** toggles automatic candidate preview and persists as
  interface state. Candidate-to-candidate hover changes only the Artwork
  preview; a short exit delay prevents card boundaries from repeatedly
  rebuilding or flickering the filter and Advanced controls.
- Provider and candidate URLs open the corresponding artwork or authority page
  for inspection before approval.

LAUNCH and the ordinary candidate actions occupy the same header row immediately to the
right of **Artwork Filter**. There is no separate bottom action bar. Conditional
actions such as Keep Local, Refine Fallback, MusicBrainz Matches, and Back to MB
Matches appear in that row only when applicable. **Upscale Preview**, **Upscale
Show Full**, and **Apply default upscale** live in the filter's Upscale /
Advanced group.

Opening Artwork Filter temporarily gives the Scan Activity pane enough height
for the editing workspace and restores the user's previous Activity/Candidate
divider when the filter closes. Its filter and Advanced regions fill that
workspace without nested scrollbars. **Upscale Preview** and **Upscale Show Full**
are fixed-width actions placed side by side before the active 1:1 source-to-edit
resolution, not full-column bars. Show Full opens the current upscaled and
edited result at its actual pixels in a scrollable resizable window; it never
substitutes the untouched source image or scales the preview to fit. The eight
advanced values use one contained row of equal-width framed vertical controls.
Each spectrum frame centers its name and matching symbol, current value above
the adjustment, and single square reset control below it as one vertical unit. They are
Picture (color intensity), Sharpen, Softness, Contrast,
Exposure, Brightness, Gamma Correction, and Color Correction (Cool through
Middle to Warm). Numeric defaults are `0%`; Color Correction defaults to
`Middle`. Selecting exactly one result, including local `cover.*`, immediately
focuses that image in Selected Album Artwork as the editing target. The live
Artwork preview then updates as values change.

Candidate order remains deterministic. Local evidence is displayed first,
the recommended result follows, and remaining results are ordered by descending
pixel resolution with their stable candidate index as the final tie-breaker.
Only candidate-card backgrounds use the subtle glass treatment: local is light
purple, a policy-qualified enlargement within Maximum Upscale is magenta,
BelowMinimum/rejected is red, recommended is green, and ordinary results are
clear. Magenta takes precedence over a negative range when enlargement to Ideal
does not exceed the configured percentage. Candidates beyond that limit remain
red/manual-only. The thumbnail itself always uses an opaque neutral surface and
is never color tinted.

Each card now identifies the provider at its top, with compact symbols for
Local, Recommended, Upscalable, and negative results. Hover the information
indicator, heading, thumbnail, or card to see original and projected
resolution, Range Type, format, square state, Approved/Acceptable state,
strict Preferred/Auto eligibility, and policy reason. This replaces the former
three lines of repeated card metadata. Thumbnails remain square and grow or
shrink with the candidate pane. The shared Artwork preview,
MusicBrainz Matches, and Compare surfaces do not inherit candidate glass.

When exactly one cached result is active, **Upscale Preview** and **Upscale
Show Full** become available. This includes Ideal, Ladder, Above Ladder,
Below Minimum, remote-provider, alternative MusicBrainz-release, and local
results: manual editing is not restricted to candidates that qualify for an
automatic upscale. Selecting an Album with an existing local cover exposes and
preselects it before any scan; provider results are not required. The editing
controls and live preview therefore remain available at any resolution or prior
processing state. The preview renders the configured result in memory in the
shared Artwork pane without writing the Album or changing candidate ranking.
After MusicBrainz Matches returns a new release's candidates, **Use Selected**
remains available during the decision-event handoff so an alternate-release
selection can be submitted and saved. After **Use Selected** or **Save Existing**
finishes, temporary MusicBrainz and Artwork Filter workspaces close and the
final Album run report is brought back to the front.
The preview uses the same final-art pipeline as selection and preserves aspect
ratio and the color profile where the output format permits it. **Apply default
upscale** keeps the analyze-first profile: no sharpening, bounded adaptive
brightness/contrast, and no change to balanced artwork. The Advanced controls
can explicitly add Sharpen, Contrast, Exposure, Brightness, or Cool/Warm color
correction. Each `↕` control expands its slider, the `<value>` label changes
immediately, and an active preview is regenerated after the value changes.
Advanced values are persistent Config v5 defaults, not one-card transient
effects. Explicitly previewing or adjusting a selected result marks that choice
for the same profile during finalization, even when its dimensions do not need
enlargement. This explicit manual path does not make the candidate Recommended,
Preferred, or Auto eligible; validation, policy, and quality rules retain sole
authority over those statuses. Brightness, contrast, and exposure are bounded
to ±20%, sharpen to 0–20%, and color correction to -100 Cool through +100 Warm.

Candidate image bytes are retained in a bounded 256 MB process-memory cache
after their first read. Candidate cards, the shared preview, and repeated edit
previews therefore reuse RAM rather than rereading the same Temporary Run Cache
file from NAS. The cache is disposable, uses FIFO eviction at its fixed limit,
and is cleared when SPLINED closes; `splined.db` is never copied into it.
Concurrent provider work may reduce
waiting time, but configured source order, source policy, Range Type, geometry,
distance, and tie-breaking still determine the displayed ranking.

Upscale possibility alone never grants Preferred or Auto eligibility. An
enlarged candidate must still satisfy range, source, strict-content, and image
quality validation. A flat or badly clipped image stays visible for manual
review but cannot be selected automatically. Album processing still audits
authoritative tags on every track. MP3 authority reads are bounded to the
leading ID3v2 block and never seek across the MPEG container for ID3v1, Lyrics3,
APE, audio properties, or embedded artwork. Other formats use Lofty with audio
properties and cover art disabled. This preserves MusicBrainz release,
compilation, title, and artist validation while avoiding per-track end-of-file
SMB round trips on large albums and box sets. Untouched same-format
artwork is preserved byte-for-byte. Processed JPEG and PNG output carries its
embedded ICC profile when present; decoded WebP or cross-format conversion
preserves the visible color values but may normalize container metadata.

At batch completion, the Album Run Report records the selected provider,
original and final resolution, resize/conversion state, and the actual upscale
backend. For an upscale it also records adaptive-default state, brightness,
contrast, exposure, sharpen, color temperature, and whether the
source passed the automatic-quality gate. READ reports remain `ReadOnly`, but a successfully rendered preview is
reported explicitly as `gpu-lanczos3` or `cpu-lanczos3` instead of being lost
when the live activity panel is replaced. The GUI drains redirected core output
before constructing the process completion report and reconciles a final event
by Album path even if Windows reports process exit first. A successful
`Unchanged`, `ReadOnly`, or installed decision therefore cannot be overwritten
by the default `Incomplete` fallback.

## MusicBrainz Matches

Every Album candidate review can open **MusicBrainz Matches...**, including an
Album that already has a valid Album/Release MBID. Matches are integrated into
the main decision workspace rather than opened as a separate modal window, so
the shared Artwork panel remains available throughout authority review.
Ordinary rows have no decade grouping. Named Artists sort first A–Z, followed
by the fixed special-family order Soundtrack, Compilation, and then `Various
Artists`. Within each Artist or special family, dates sort newest to oldest
with unknown dates last, then country sorts with US first, other named
countries A–Z, and unknown last. Release title and Release MBID provide the
final deterministic tie-breakers. Visible headings identify Release Type only.

A yellow `[CURRENT ALBUM]` category and exactly one yellow `[*]` authority row
remain first in the list. This row is separate from the ordinary MusicBrainz
results and represents the queried track's Artist, Release, and Recording MBIDs
shown in the authority fields. It is never derived from the first result and is
never hidden by filtering. Selecting it and choosing **Open MB Page** opens its
current Release MBID. Its Release Type cell is blank so `[CURRENT ALBUM]`
appears visually exactly once, in the category header.

**Filter by Artist ▼** and **Filter by Release Type ▼** open Ctrl multi-select
lists; press Enter to apply. Artist choices come from the ordinary result
Artist column, including `Various Artists`. Release Type choices narrow to the
active Artist selection. A visible ordinary row must satisfy both filters;
empty release-type headings are omitted and headings are rebuilt from the
filtered rows. The authority category and row remain fixed above them.

Rows show:

- Artist;
- country;
- release date;
- release type;
- release title;
- known inspected artwork resolution;
- an artwork-preview `[URL]`, plus **Open MB Page** for the exact MusicBrainz
  release page.

The artwork `[URL]` uses the Cover Art Archive release-group front endpoint,
falling back to the exact-release front endpoint. Hover and row selection load
that image asynchronously into the existing Artwork surface and record its
actual dimensions beneath the image. The match-list Resolution column remains
the exact-Release-MBID evidence from inspected configured source results, not
the release-group preview size. Preview bytes are bounded and cached for the
active Windows session; previewing does not rerun provider discovery or alter
source ranking.

Selecting an ordinary row runs the configured artwork providers for that
release. **Back to MB Matches** returns to the same cached list. The most
recently visited ordinary release is green and earlier visited releases are
orange. Re-selecting an
inspected row restores its source candidates, resolution evidence, diagnostics,
and deterministic ordering without repeating provider discovery or image
downloads. **Return to Source Results** restores the original Album results.

The Matches workspace also has session-only Artist, Release, and Recording MBID
fields. **Apply IDs** requires a canonical Release UUID and validates any
supplied Artist or Recording UUIDs. It updates the separate `[*]` authority row
and its **Open MB Page** Release-ID target, then loads artwork-source results for
that exact session authority. The operator must still choose artwork through
the normal candidate workflow; applying authority never writes MusicBrainz tags.
Ordinary result rows, numbering, URLs, visited state, selection, and filter state
are unchanged.
The removed **Search Artist / Track** action no longer starts a second free-text
result branch. Automatic MusicBrainz lookup/search used by compilation matching
remains available when track authority is missing.

For iTunes, exact official Apple Music/iTunes relationships from MusicBrainz
are tried before ordinary Artist/Album searches. Current no-slug Album URLs and
older slugged `/id...` URLs are both recognized.

## Curated compilations

An Album enters compilation track-art handling only when its representative
track has no Album/Release MBID and the Album is tagged `compilation=1`. The
yellow warning remains explicit. This path never creates, changes, or removes
folder-level `cover.*` artwork.

Select Media retains the snapshot's compilation marker. A compilation remains
eligible for LAUNCH until SQLite records `embedded-compilation` completion,
even when older Album-level processed or bypass history exists. Selecting the
Album or its Artist therefore activates LAUNCH for the outstanding per-track
embedded-art work without deleting that older history. A completed compilation
is not selected again automatically.

For each track SPLINED:

1. checks the exact Recording-ID/Artist-ID SQLite cache;
2. lazily checks indexed Albums belonging to the matching Artist identity;
3. uses bounded MusicBrainz work only after a local miss;
4. applies the automatic order Album, Soundtrack, then Compilation;
5. lets the operator inspect broader Artist/Track MusicBrainz results;
6. runs the ordinary configured artwork providers and policies;
7. replaces only the approved embedded front image in Live Write;
8. commits the track ledger and Album progress immediately.

Tracks without authoritative evidence remain unchanged and appear unresolved.
Timeouts and upstream errors likewise leave the existing embedded image alone.
Incomplete Albums remain blue and resumable; already completed track decisions
are skipped on the next run.

## Settings and credentials

Windows displays Config v5 in native controls; it does not require the user to
edit TOML. Provider credentials remain separate JSON files in the configured
credential directory. Config v5 stores only that directory—not tokens,
passwords, authorization headers, client secrets, or provider filenames.

**Artwork Options** is arranged as Image Processing, **Default Upscale /
Advanced**, and Output Formats. The middle section owns Upscale below ideal,
Maximum upscale, Apply adaptive defaults, and the eight advanced values also
shown in Artwork Filter. Both surfaces edit the same saved Config v5 profile.
The profile is applied when an eligible source below Ideal is enlarged. Ideal
and higher-resolution validated artwork is never reduced automatically.

Any cached candidate is available for explicit manual editing regardless of
its range or automatic-upscale eligibility. An existing local `cover.*` also
remains available regardless of its resolution or whether it was previously
upscaled. Selecting a result and using Upscale Preview, Upscale Show Full, or
changing an advanced value marks that selected candidate for editing at its
native/projected resolution; simply choosing Keep Local still leaves an
unedited local cover's bytes untouched. Manual review carries an existing
Ideal cover into candidate results instead of ending at local preflight; Auto
mode retains the fast local-Ideal acceptance path. This does not require
resetting or lowering the Album's completed status.

Each provider in **Sources & Matching** exposes **Strict Override**. When on,
Windows compares the decoded complete image with exact-release CAA Front art,
a validated local cover, or independent front-art consensus. Strict Override
uses the global range and supersedes Source Override while retaining its saved
custom values. Candidates without strict evidence remain visible and
previewable for explicit operator selection. Amazon defaults to strict mode.

Credential writes are atomic and Windows applies a current-user/System ACL
where the selected filesystem supports it. A NAS may not support Windows ACLs;
SPLINED reports that limitation without exposing the credential value.

MusicBrainz timing and retry behavior comes from the saved credential options,
including `min_delay`, `recording_timeout`, and `retry_max`. No fallback value
is silently hard-coded when a configured value exists.

See [Credentials and provider setup](credentials-providers.md) and
[MusicBrainz OAuth](musicbrainz-oauth.md).

## Backup and restore

**File > Backup** provides separate Import and Export commands. A `.spl` backup
may include any combination of:

- internal Config v5 settings;
- interface state, panel layout, and Artwork Filter exclusions;
- credential JSON files;
- `splined.db`;
- diagnostic metadata.

The sections are independent; a backup is not forced to include secrets or the
database. Exports include integrity protection. Optional password protection
uses PBKDF2-derived encryption and authentication.

Opening a registered `.spl` file launches SPLINED directly into the selective
restore dialog. No section is restored until the operator selects it and
confirms. A backup created from an active shared database must still represent
a consistent database copy; stop writers or use the database owner's backup
procedure first.

## Diagnostics

Debug output is written to:

```text
<configured log directory>\run\splined-<level>-<timestamp>-p<pid>.log
```

Windows keeps one current GUI session: prior `splined-*.log` files in that
`run` directory are removed at the next start. Logs are diagnostic only and do
not own Album status, history, bypass, timeout, or compilation progress.

Compact records include snapshot timing, media-tree selection changes and
compilation eligibility, per-provider elapsed time, candidate/reference counts,
post-cover persistence timing, and error class.
Credentials, tokens, passwords, authorization headers, and private values are
redacted and must never be logged.

## Updates

**Help > Check for Update...** checks the newest official non-prerelease GitHub
release containing the Windows portable archive and notification metadata. If
the release differs from the installed build, SPLINED offers to open the
official GitHub release page.

The portable application never downloads, stages, executes, installs,
self-replaces, relaunches, or cleans up replacement executables. Close SPLINED,
download the official archive in the browser, and replace `splined.exe` and
`splined-core.exe` together. Internal settings and the configured database,
temporary cache, logs, and credentials remain external and survive program-file
replacement.

## Troubleshooting

### `Windows internal settings` reported as a missing file

Current builds pass internal Config v5 in memory. They do not send the display
label through `--config-path`. Update the Windows executable if an older build
logs `Unable to read SPLINED configuration Windows internal settings`.

### `SPLINED returned an incompatible SQLite media snapshot`

Inspect the immediately preceding `media_snapshot.exit` record. Current builds
return a nonzero core exit and the actual configuration/database error instead
of treating empty output as a successful snapshot.

### `database is locked` or `attempt to write a readonly database`

Confirm that the configured SQL Database Directory and `splined.db` are writable by
the current Windows user and that every runtime opening the same physical
database has `sqlite_shared=true`. Do not place a shared database in WAL mode.
Close obsolete test executables before retrying.

### No Albums appear in Select Media

Validate Settings, confirm the cache points to the intended `splined.db`, and
check the snapshot log. For shared mode, rebuild inventory through Python and
then use **File > Refresh Library Index** in Windows. Windows does not apply a
second ignored-folder policy to the Python-owned shared inventory.

### An accented or legacy-decoded Album directory is not found

Use the real Windows library root in Settings and do not rewrite the SQLite
canonical root. Current builds compare normalized and legacy-decoded directory
forms before reporting the physical path missing. Attach the current run log if
translation still fails.

### A provider returns zero candidates or an upstream error

Provider diagnostics distinguish success, zero results, timeout, and error.
One provider failure does not change another provider's ranking or make the
Album Processed. Verify the candidate URL independently and retain the run log
without adding credential files.

## Invariants

- SQLite is Album/status authority; an arbitrary existing `cover.*` does not
  become Processed authority.
- Shared SQLite retains rollback journal, FULL synchronization, and bounded
  busy handling.
- Source order, source policy, ranking, and deterministic tie-breaking do not
  change because of interface navigation or concurrent provider discovery.
- Read mode does not mutate artwork or durable completion state.
- A curated compilation never silently falls through to folder artwork.
- Config v5 and interface state are internal on Windows; credentials remain
  separate and secrets never enter logs.
- `fixtures/cross-runtime-album-policy.json` is consumed by Rust and Python
  policy tests to keep eligibility and output targets aligned.
