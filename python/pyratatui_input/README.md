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

The extension version is `0.1.0`, uses crossterm `0.29`, PyO3 `0.28`, and the
stable CPython ABI3 surface for Python 3.10 and newer. Build a local wheel with:

```text
python -m pip install maturin==1.9.6
python -m maturin build --manifest-path python/pyratatui_input/Cargo.toml --release --locked
python -m pip install python/pyratatui_input/target/wheels/splined_pyratatui_input-*.whl
```

`EventReader` enables capture on context entry and disables it on context exit
and object drop. `emergency_restore()` additionally disables raw mode and mouse
capture, leaves the alternate screen, and shows the cursor after a partial
terminal initialization failure.

Upstream delta: this is the event/capture portion that can be proposed for
`pyratatui.Terminal`; it is isolated here to avoid vendoring or forking the
entire pyratatui/Ratatui source tree.
