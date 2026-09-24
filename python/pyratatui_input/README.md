# SPLINED pyratatui input extension

`pyratatui==0.3.0` supplies SPLINED's Ratatui renderer but exposes only key
events. This small ABI3 PyO3 module adds the missing crossterm input surface:

- mouse capture enable/disable;
- key and mouse events from one event reader;
- mouse down/up/drag/move and scroll directions;
- row, column, button, and modifier fields.

It contains no renderer and no SPLINED policy. Production Docker builds compile
an ordinary wheel in a build stage and copy only the wheel into the Python
runtime image, so the final image contains no Rust toolchain.

Upstream delta: this is the event/capture portion that can be proposed for
`pyratatui.Terminal`; it is isolated here to avoid vendoring or forking the
entire pyratatui/Ratatui source tree.
