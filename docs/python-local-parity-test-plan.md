# Local Python artwork run-through

This plan uses the MEDIAPC Python shortcut `splined` and the copied test library.
Close any Python TUI started before these source changes, then relaunch it so the
new code and database audit are active. The shortcut runs the standalone
`E:\apps\ChatGpt\SPLINED-local-test-library\python-local` copy, including its
own virtual environment. It does not execute from the Git `dev` checkout.
No Git build or push is needed.

## Confirm the test boundaries

Press `A` in Select Media and check these paths:

| Item | Expected local path |
| --- | --- |
| Python source | `E:\apps\ChatGpt\SPLINED-local-test-library\python-local\python` |
| Python executable | `E:\apps\ChatGpt\SPLINED-local-test-library\python-local\.venv\Scripts\python.exe` |
| Library | `E:\apps\ChatGpt\SPLINED-local-test-library\music` |
| Python SQLite | `E:\apps\ChatGpt\SPLINED-local-test-library\state\cache-python-local-index-20261027\splined.db` |
| Artist images | `E:\apps\ChatGpt\SPLINED-local-test-library\state\artist-art` |
| Logs | `E:\apps\ChatGpt\SPLINED-local-test-library\state\logs` |
| DB activity | `<Logs>\db-audit` |

The isolated Windows database is under `state\cache-lab-index-20261027`, so it
is a different SQLite file. The Python configuration has `sqlite_shared=false`
and `logging.database_audit=true`. If either database or library path differs,
stop the run before writing images or audio tags.

## Home and Media Filter checks

Select Media opens with the **Artist Overview** in the center. Its background
and round portrait use the saved local `banner.*` and `artist.*`; album cards
show local `cover.*` images. Click an artist on the left to change the overview.
Click an album card to select it and load the right-side artwork. At a narrow
terminal width the three areas stack instead of squeezing the images.

1. Click **Media Filter +**. Artist Search, Album Search, status filters,
   selection, and scan controls should appear in one overlay. Click the close
   control or press `Esc` to return to the overview. The images underneath
   should reappear without changing the selection.
2. Click **Media Library −/+** and **Albums −/+** to collapse and restore those
   sections. Click **G Artist Art** or **O Album Art** to open the corresponding
   editor. The `?` button opens help. `A` still opens About, where the configured
   local paths can be checked.
3. Resize the terminal if practical. Artist and album selections should remain
   clickable in both wide and stacked layouts. The home images should use the
   same smooth native terminal rendering as the selected album preview.

## Artwork checks

In Artist Art, click a candidate, choose **Use as
Artist** or **Use as Banner**, then click the pane title to activate its radio.
The slider selector, adjustment steps, zoom, position, upscale, reset, write,
and back controls are clickable. Wheel over an active image to zoom; hold and
drag it to position. Album Art has clickable adjustment, upscale, reset, image
path, write, and back controls. About has clickable update and back controls.

1. Focus **3 Doors Down** and press `G`. Check that the saved package, artist
   image, banner, and Selected Artwork appear as smooth native images, like the
   selected Album thumbnail in Select Media, rather than large terminal pixels.
   There should be local Artist and
   Banner candidates. Press `2` or `3` to edit one component; use `[`/`]` to
   choose a control and `+`/`-` to change it. The chosen pane and Selected
   Artwork should visibly change, while the current package remains unchanged.
2. Press `4` for the combined package. Editing should require `T` to enable
   Themed Packaging. Switch back to `2` and confirm its earlier adjustment is
   retained. Press `U` to toggle the upscale preview; the panes should remain
   visible. Press `R` to reset the active preview.
3. To exercise a write on the copied library, select the desired Artist or
   Banner component and press `W` twice. One `W` only asks for confirmation.
   Reopen `G` and confirm the saved package and local candidates reflect the
   written `artist.*` or `banner.*`. `Esc` leaves without writing.
4. Focus an A-artist Album with a saved `cover.*` and press `O`. Compare Original,
   Enhanced Preview, and Selected Artwork. Change Brightness or Contrast and
   verify visible pixel changes; `U` toggles the 2× preview. `W` twice writes
   the still cover on the copied Album. Opening `O`, editing, or using `Esc`
   must not stage or embed any track. If the Album has only `cover.webp`, the
   approved still write creates `cover.jpg` and leaves `cover.webp` intact.
5. In a MusicBrainz result list, verify **Best Match**, **Artist Release
   Priority**, and **Other Releases** sections, each family sorted by match
   percentage. An Existing result belongs in Best Match even below 90%.

## Database activity check

After the restarted session, inspect the newest audit file:

```powershell
$audit = Get-ChildItem 'E:\apps\ChatGpt\SPLINED-local-test-library\state\logs\db-audit\*-python-db-*.log' |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
$audit.FullName
Get-Content $audit.FullName | Select-String 'session|open|write_attempt|transaction|restore_'
```

The filename begins with `[MEDIAPC]` on MEDIAPC. The `session` line names the
expected Python SQLite path. Every `open` line identifies the actual path and
whether it matches. `write_attempt` records an operation and table without SQL
values; `COMMIT`/`ROLLBACK` show transaction boundaries. The audit records
Python SQLite activity only. An image write to `artist.*`, `banner.*`, or
`cover.*` is a filesystem write and is not a database write. Windows activity
remains in its separate Windows logs. A write attempt alone does not prove a
committed database change.

## Scope of this run-through

This Python pass covers MusicBrainz result presentation, local Artist Art
packages, individual and themed image controls, local Album cover enhancement,
and separate database activity logging. Existing Python fallback track review
and the engine-owned scan and embed path remain available. The Windows remote
Artist Art provider strip, GUI filter presets, COMP Report bulk actions, and
Album Track Embedding queue are not yet exposed in the Python TUI; do not use
this run-through to assess those Windows features.
