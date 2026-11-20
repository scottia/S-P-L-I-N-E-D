# WebGUI browser workspace

WebGUI is the Python/browser application using PostgreSQL for its
catalog, jobs, audit, administrator and session state. It is separate
from the native Windows Rust/Slint executable and its authoritative
`splined.db` for synchronized catalog fields. These clients share
approved behavioral contracts, not one live database file.

[WebGUI Docker deployment](webgui-docker-deployment.md) describes
the container, Compose, volumes and config paths.

## First run and access

Open the configured WebGUI address (loopback `127.0.0.1:8080`
in the checked-in Compose example). Complete the supported
administrator enrollment using the deployment token; use a
persistent credential and config volume. Browser paths refer to
the **server/container**, never to the device opening the browser.

In **Settings**, configure server-visible library/cache/log and
V/A / OST Artist Art paths, review provider credentials, and save
a validated Config v5 document. Prefer read-only inspection before
enabling actual file-writing operations. Do not use a development
configuration with embedded secrets in production.

## Library indexing and catalog authority

The WebGUI index maintains Artist, Album and Track projections in
PostgreSQL. Its background refresh/delta checks and optional Full
Refresh are not permission to rewrite Windows-owned identity,
state or synchronized fields. Reconciliation uses recorded,
unambiguous Windows associations, with WebGUI-only jobs and
presentation preserved. See [Catalog authority](catalog-authority.md).

An ordinary Album's previewable `cover.webp` must not be confused
with a qualifying static `cover.*`. Artist/Album NAME colors follow
the approved separate ranked policies; all applicable Album status
dots are displayed independently. **Unprocessed is only a MEDIA
FILTER numeric count**, not a name color or status dot.

The opposing client's last successful Album write may grey the
card thumbnail with the accepted **Dark Grey override frame**;
previews and write controls remain usable. COMP-track writer
notices are track scoped. The underlying PostgreSQL coordination
records are evidence of who wrote, not a second music catalog
authority or permission to modify live media automatically.

## Candidate artwork and write actions

An Artist/Album selection exposes local and provider candidate
artwork, original image dimensions, source/range diagnostics,
enhancement and upscale controls. **WRITE Cover**, **WRITE Artist
Art** and track embedding require an intentional permitted write.
Read mode must not alter music files. Saved processing outcomes,
history and browser job state belong to PostgreSQL.

Identified Album embedding uses its own staged queue and preferred
embedded thumbnail. Fallback/Curated COMP work instead uses
Artist/Recording identity and track-scoped MusicBrainz release
results, retaining independently selected artwork and **EMBED Comp**
for the current track. Album-level EMBED Batch is not a shortcut
for an unidentified COMP fallback.

COMP-track presentation remains **Blue pending / Purple thumbnail
attention / Green complete**, independent from Artist/Album name
colors. The approved
[`schema/comp-report-policy.json`](../schema/comp-report-policy.json)
baseline governs provider matching/ranking and source choices
where implemented. Avoid introducing another results screen or
creating a new policy revision merely to document the interface.

## Settings, maintenance and data recovery

[Config v5](config-v5-reference.md) and
[Source Policies](source-policies-range-types.md) describe quality
options, with the current schema/source as the implementation
reference. The same canonical media-state policy applies to
standalone and shared Windows SQLite installations; WebGUI
PostgreSQL persisted copies must be checked for revision drift.

Back up PostgreSQL consistently, along with persistent config,
credentials, artwork caches and any music files modified by a
write. A Windows SQLite snapshot used for reconciliation is not
a PostgreSQL backup; importing it must be an explicit,
read-only-source operation.

**Review status:** The Docker build, Python and browser syntax
tests passed in the prior reviewed development PR. Real
Artist/Album colors, embedded evidence, coordination, browser
interactions and production policy revision parity still need
focused acceptance testing. No media writes were performed
while restoring this guide.
