S:P:L:I:N:E:D v3.0.0 Stable
============================

Stable Windows release
----------------------

- The main Windows title is now the compact S:P:L:I:N:E:D identity.
- Help > Help opens the canonical repository documentation. Help > About shows
  the concise v3.0.0 Stable identity and uses the same Help target.
- The former Documentation popup and Python Help and Config Coverage window
  were removed. Settings retains Config v5 validation and folder tools.
- Folder status bullets are larger, and each status label uses its matching
  semantic color for faster visual recognition.
- Information controls and folder-list selection boxes receive a fully opaque
  theme-owned paint pass, removing residual native white text/outline artifacts.
- Tooltips remain owner-drawn on an opaque Fluent Compact surface.
- Scan Activity now uses the same blue panel family as Artwork Candidates in
  READ and WRITE modes instead of the former red-brown WRITE background.
- Album headings use orange [position/total], purple Artist, and yellow Album
  segments from the existing semantic activity palette.
- A successfully finished batch clears transient activity and shows a per-album
  run report containing outcome, discovery path, review state, candidate totals,
  policy-hidden totals, provider notes, duration, and result destination. The
  authoritative library is reloaded before the report is shown.
- The collapsed Select control and bottom candidate-action row include explicit
  bottom breathing room so their Fluent outlines are no longer clipped.
- Each successfully completed or operator-stopped launch Album is consumed from
  the active selection. Failed and unattempted Albums remain selected for resume.
  Equivalent UNC paths with trailing separators are matched correctly, so an old
  first album cannot lead a newly selected artist.
- The top navigation no longer repeats the Media Library path. Appearance now
  lives under View > Appearance, with System as the default theme preference.
- Panel guidance was moved into information tooltips beside Media Library
  Selection, Scan Activity and Decisions, and Artwork Candidates and Preview.
- The main LAUNCH / WAITING / STOP control is now the first button in the bottom
  Artwork Candidates action row; its execution behavior is unchanged.
- The former Media Filter panel is now a single collapsible Select Media control,
  without a redundant Media Filter button/title. It provides mutually exclusive
  Select [ALL], Select [NONE], and Select [FILTERED] choices plus the temporary
  READ/WRITE Scan Mode. Bulk behavior matches Python: ALL replaces selection
  with the active Artist's Unprocessed Albums, NONE clears it, and FILTERED
  requires Artist or Album filter text.
- Main library/right-workspace splitter sizes and Select-panel expansion state
  are restored from the GUI-local UI state on the next launch.

Fluent Compact + SPLINED identity
---------------------------------

Buttons, drop-downs, text fields, numeric settings, tabs, state controls, tooltips,
cards, scrollable surfaces, and the Dark/Light/System theme paths continue to
share the centralized SPLINED Fluent Compact theme. Active round states use
green with a yellow center; inactive states use red, with no checkmark glyph.
Folder and album status colors remain a separate semantic data palette.

Settings layout canvas
----------------------

An external drag-and-drop Settings layout canvas accompanies this release. It
lets the user arrange the existing Settings sections across the three current
tabs before requesting an implementation. The canvas does not change the live
application, Config v5, or any setting by itself.

Behavior preserved
------------------

v3.0.0 Stable preserves Config v5, credential-directory isolation, credentials,
providers, MusicBrainz, library discovery, source policy, artwork evaluation
and writing, Media Filter, history, retention, bypass, timeout, selection,
hover behavior, and the LAUNCH / WAITING / STOP lifecycle.

LAUNCH queues only Album rows that are already selected. Select [FILTERED] is
the explicit text-filter action and remains visibly active afterward.

Package contents
----------------

splined-windows-x86_64.zip contains exactly:

    splined.exe
    README-WINDOWS.txt

The Rust core, WinForms interface, watermark, and application icon are embedded
in splined.exe. The archive contains no sidecar assets, config.toml, credential
JSON, cache data, logs, history, or other user data.
