# Windows and WebGUI function audit

Source baseline: Windows 2.0.4 local work through `1ea2d74`, followed by the Python WebGUI/PostgreSQL migration. This describes source behavior before a new packaged release. The existing published 2.0.4 container predates the migration.

| Workflow | Windows | WebGUI |
| --- | --- | --- |
| Library | Native Rust/SQLite; folder grouping and tagged album authority | Python/PostgreSQL; explicit index refresh, same missing-Album-ID fallback rule |
| Artist COMP Report | Gathers every matching missing-ID album inventory before displaying/verifying tracks; launches checked tracks across album boundaries | Hydrates missing-ID track inventories before projecting the focused artist; queued COMP work presents candidate and MusicBrainz decisions in browser controls |
| Existing COMP preview | Embedded track image appears directly in Selected Artwork and Enhancements | Embedded track image is read from the selected audio file; never falls back to folder cover artwork |
| Cover and Artist Art | Actual native writes | Actual Python writes with configured destinations and preserve policy; read mode disables music writes |
| Embedding | Persistent native queue and exact preference-sized thumbnails | PostgreSQL jobs, persistent staged image/hash, atomic tag writes, byte verification, original embedded-image backup, pause/resume/retry and interrupted-job recovery |
| EMBED Batch navigation | Saves/stages while retaining the current workspace | Saves/stages while retaining the current workspace |
| Queue indicator | Muted purple button surface, outline and text while staging/unfinished work exists | Same cue for unfinished PostgreSQL work |
| Scroll memory | Separate Artists, Albums and Fallback offsets; per-artist/per-album overview offsets | Session memory for library, overview, album grid and workspace; expansion survives return navigation |
| Biography | Five collapsed lines, themed inline more/less at biography font size | Five collapsed lines, same-size inline more/less and theme-aware styling |
| Themes / radios | Native themed controls | Dark/Chalk/Light; fixed square radio bounds remain round. Brave's reported theme override was traced to Night Eye's forced DARK mode |
| Settings / credentials | Native structured surfaces | Browser Config v5 editor, masked credential editing/tests, MusicBrainz PKCE authorization and token renewal |
| Database | Windows SQLite, unchanged by this migration | PostgreSQL authority; named read-only SQLite snapshot import into an empty catalog only |

The selected-item Media Filter counters retain their previous behavior. Album-total status counters were rolled back at the user's request.

## Validation evidence

Backend tests use generated silent FLAC files and dedicated PostgreSQL schemas. They verify actual cover/artist writes, preserved audio and tags, album embedding, staged queue recovery and corruption rejection, authenticated browser/API access, mutation request tokens, configuration persistence, credential masking, MusicBrainz PKCE, and COMP completion through a browser decision adapter. Existing provider, policy, geometry, and fallback report regressions remain in `webgui/tests`.

A browser test against the isolated PostgreSQL service staged a generated album without changing workspaces, started embedding, and displayed both tracks completed. Settings exposed library/output/source policy fields, PostgreSQL configuration, provider credentials, and index refresh. Production music and the live Windows database were not used for these write tests.

Provider fixtures test request contracts; they do not prove every external account or provider is available. Those require the user's own saved-credential tests. Linux container build/start and native checks run in the validation workflow before branch merges.

## Windows runtime observations

Earlier logs from the PC workstation running 2.0.4 showed the Alan Jackson report returning through preparation errors while Aerosmith completed. The report now gathers uncached COMP inventories and preserves completion records before checking thumbnails. Later logs showed queued tracks waiting for a runner; launch handling now starts the queued runner for selected COMP tracks across albums.

The background/foreground transparent-window report had no corresponding panic or fatal renderer event in the supplied logs. The existing local Windows change requests a repaint on reactivation and retries at 80 and 240 ms. Real reproduction on that PC remains the evidence needed to establish whether the compositor issue is resolved.
