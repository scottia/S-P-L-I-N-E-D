# Artist and Playlist Artwork Composition

Status: Future concept. This is a design reference, not an implemented feature or a release commitment.

S:P:L:I:N:E:D could create new Artist Art and Playlist Art from a chosen series of album covers or a randomized selection. The existing Upscale / Enhance workspace, saved slider presets, and theme-aware color controls could provide the visual treatment. One creative direction is *mirage-style* imagery: layered, blended cover elements that form a cohesive artist or playlist image rather than a simple grid of covers.

## Possible workflow

1. Choose Artist Art or Playlist Art and select source covers manually or by a constrained random selection.
2. Preview compositions and color variations, including a mirage-style option, using the current enhancement controls and saved presets where appropriate.
3. Review the result at its intended output size, then explicitly save or export it.

Original covers and existing Artist Art should remain unchanged unless the user explicitly chooses to replace them. Creating artwork should not silently alter playlists, media tags, or the library database.

## Decisions to make before implementation

- Define which covers are eligible for an artist or playlist and how random selection is scoped.
- Define the mirage effect, composition controls, output sizes, file naming, and destination.
- Decide whether Playlist Art is export-only or integrated with a playlist provider.
- Specify how the color wheel, brightness/darkness range, random color harmonies, and enhancement presets feed into generation.
- Provide a clear preview and replacement/undo path before any write.

When related artwork, palette, or enhancement features change, revisit this note and record decisions here. Do not present the concept as a supported feature until it is implemented and validated.
