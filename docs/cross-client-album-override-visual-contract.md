# Cross-client Album override indicator — proposed UI contract

**Status:** Design specification for a future Windows Rust/Slint and WebGUI/PostgreSQL coordination feature. Not implemented by this document. No executable behavior is changed.

## Purpose

Windows SQLite and WebGUI PostgreSQL may browse, search, preview, upscale, edit, write, and embed artwork for the same Album. A **successful** artwork write or embed by one application makes the other application's Album card display a **persistent override indicator**. This is visual provenance of the last successful writer, **not** a lock, disability, error, incomplete status, or request for approval.

Viewing, selecting, searching, previewing, upscaling without writing, and unsuccessful or cancelled writes do not change the indicator. Both applications retain full functionality even while the indicator is displayed.

## Album card visual contract

When the **opposing application** was the last successful writer for the Album:

- Render the **Album-card thumbnail image in grayscale**. Do not alter or rewrite the underlying image.
- Render the **Album-card frame in red**, replacing only the normal frame color. Do **not** gray the whole card container.
- Preserve every existing **Album status dot** and its independently determined color, including any existing red processing/status dot. Do not add an override status dot.
- Preserve **COMPILED, FEATURING, SOUNDTRACK, VARIOUS, and ARTIST** banner colors exactly as configured in Settings.
- Keep the real `cover.*`, `artist.*`, and `banner.*` artwork **in full color** in Selected Artwork, previews, searches, enhancements, upscaling, editing areas, and written files.
- Keep Album text, interaction controls, and status semantics unchanged. The red frame does **not** imply an error, disabled state, or physical artwork validation failure.

Render grayscale **only on the Album thumbnail**, not on the banner, status dots, or content surrounding the image. The frame is independently red.

## Album Status Key — existing column 2 only

In the existing multi-column **Album Status Key**, under **column 2: ALBUM CARD FRAMES**, add exactly one ordinary frame definition alongside existing Cyan, Green, Orange/Purple, and Green/Purple entries:

**Red — Overridden by opposing application**

This is a frame-color definition, **not** a new section, column, information slot, or status dot. Do not change existing definitions, counting logic, or customizable banner palette.

## Albums summary count

In the Albums summary/count line associated with the Status Key, add a separately labeled **Overridden** count:

- **Overridden** label: red.
- Number (`#count`): neutral grey.
- Count Albums in the current displayed Album set whose **opposing application** is the last successful writer.
- Do not subtract from or redefine existing `shown`, `Completed`, or `in Process` totals. Overridden is an independent presentation attribute, not an Album processing state.

Illustration: `Albums 3 shown / 2 Completed / 1 in Process / Overridden 1` (red label, grey count). The number and label update for the local application's perspective.

## Selected Artwork — short override notice

In the existing Selected Artwork preview information/banner area (including its yellow or blue presentation), add a **brief red text notice** when the opposing application last wrote the selected Album:

- Windows: **`WebGUI override`**
- WebGUI: **`Windows override`**

Keep the notice short. Do not replace, recolor, or disable the existing yellow/blue banner, preview, or controls. No notice is required when the local application is the latest writer.

## Persistent last-writer transitions

| Last successful writer | Windows thumbnail / frame | WebGUI thumbnail / frame |
| --- | --- | --- |
| No recorded cross-client write | Normal / normal | Normal / normal |
| WebGUI | Grayscale / red | Color / original frame |
| Windows | Color / original frame | Grayscale / red |
| Windows writes after WebGUI | Color / original frame | Grayscale / red |
| WebGUI writes after Windows | Grayscale / red | Color / original frame |

The indicator remains across application restarts and refreshes until a **successful local WRITE or EMBED** makes the local application the last writer. The local card then returns to full-color thumbnail and its normal status-derived frame; the opposing card becomes grayscale with a red frame. No manual accept/clear step is needed. Preview and edit operations alone never clear or transfer it.

## Architecture boundary for later implementation

The display reflects durable, Album-identity-scoped last-successful-writer provenance shared by Windows and WebGUI; neither application needs to disable browsing or preview while an opposing write is recorded. A future implementation must distinguish that provenance from processing/embedding validation, retain Windows SQLite authority for SNAP catalog state, and avoid treating a mere Compare/Dry Run, refresh, or attempted write as a successful artwork write.

**Documentation-only design checkpoint:** This file does not implement any coordination table, connection, write notification, database migration, Rust/Slint UI control, or WebGUI UI change. Those require their own scoped implementation and validation.
