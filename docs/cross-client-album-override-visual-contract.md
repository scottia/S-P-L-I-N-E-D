# Cross-client Album override indicator — UI contract

**Status:** Approved visual contract from PR #147, extended during PR #148 owner acceptance with the configurable **OVERRIDE** banner described below.

## Purpose

Windows SQLite and WebGUI PostgreSQL may browse, search, preview, upscale, edit, write, and embed artwork for the same Album. A **successful** artwork write or embed by one application makes the other application's Album card display a **persistent override indicator**. This is visual provenance of the last successful writer, **not** a lock, disability, error, incomplete status, or request for approval.

Viewing, selecting, searching, previewing, upscaling without writing, and unsuccessful or cancelled writes do not change the indicator. Both applications retain full functionality even while the indicator is displayed.

## Album card visual contract

When the **opposing application** was the last successful writer for the Album:

- Render the **Album-card thumbnail image in grayscale**. Do not alter or rewrite the underlying image.
- Render the **Album-card frame in red**, replacing only the normal frame color. Do **not** gray the whole card container.
- Preserve every existing **Album status dot** and its independently determined color, including any existing red processing/status dot. Do not add an override status dot.
- Replace the normal category banner with the configurable **OVERRIDE** banner. Its baseline is a white frame (`#FFFFFF`), light-grey background (`#D3D3D3`), and red lettering (`#FF344F`). The banner remains colored; only the artwork thumbnail is grayscale.
- Keep the real `cover.*`, `artist.*`, and `banner.*` artwork **in full color** in Selected Artwork, previews, searches, enhancements, upscaling, editing areas, and written files.
- Keep Album text, interaction controls, and status semantics unchanged. The red frame does **not** imply an error, disabled state, or physical artwork validation failure.

Render grayscale **only on the Album thumbnail**, not on the banner, status dots, or content surrounding the image. The frame is independently red.

## Configurable OVERRIDE banner

Both clients expose **OVERRIDE** directly below **ARTIST** in Settings → Enhancement → Banners. Background, frame, and lettering are edited and saved through the same controls as the five category banners. **Reset** restores the OVERRIDE baseline without changing another banner.

OVERRIDE is a presentation layer over the Album's existing category; it does not change whether the Album is ARTIST, FEATURING, SOUNDTRACK, VARIOUS, or COMPILED. When the local client is again the last successful writer, the ordinary category banner and its configured colors return. The BANNERS column of the Album Status Key includes OVERRIDE and counts the currently displayed opposing-writer banners independently of the category counts.

## Album Status Key — existing column 2 only

In the existing multi-column **Album Status Key**, under **column 2: ALBUM CARD FRAMES**, add exactly one ordinary frame definition alongside existing Cyan, Green, Orange/Purple, and Green/Purple entries:

**Red — Overridden by opposing application**

This is a frame-color definition, **not** a new section, column, information slot, or status dot. The configurable OVERRIDE banner is listed separately in the existing BANNERS column.

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

## Architecture boundary

The display reflects durable, Album-identity-scoped last-successful-writer provenance shared by Windows and WebGUI; neither application disables browsing or preview while an opposing write is recorded. The implementation distinguishes that provenance from processing/embedding validation, retains Windows SQLite authority for SNAP catalog state, and never treats a Compare/Dry Run, refresh, or attempted write as a successful artwork write.
