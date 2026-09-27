use std::io::{self, Read, Write};
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

use crossterm::cursor::{MoveTo, RestorePosition, SavePosition, Show};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{LeaveAlternateScreen, disable_raw_mode};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use ratatui::{
    backend::{Backend, CrosstermBackend},
    buffer::{Buffer, CellDiffOption},
    layout::{Rect, Size},
    widgets::Widget,
};
use ratatui_image::{
    Image as RatatuiImage,
    Resize,
    picker::{Picker, ProtocolType},
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct EventFields {
    kind: String,
    code: String,
    button: String,
    row: u16,
    column: u16,
    ctrl: bool,
    alt: bool,
    shift: bool,
}

#[pyclass(
    module = "splined_pyratatui_input",
    name = "InputEvent",
    from_py_object
)]
#[derive(Clone, Debug)]
struct PyInputEvent {
    #[pyo3(get)]
    kind: String,
    #[pyo3(get)]
    code: String,
    #[pyo3(get)]
    button: String,
    #[pyo3(get)]
    row: u16,
    #[pyo3(get)]
    column: u16,
    #[pyo3(get)]
    ctrl: bool,
    #[pyo3(get)]
    alt: bool,
    #[pyo3(get)]
    shift: bool,
}

impl From<EventFields> for PyInputEvent {
    fn from(value: EventFields) -> Self {
        Self {
            kind: value.kind,
            code: value.code,
            button: value.button,
            row: value.row,
            column: value.column,
            ctrl: value.ctrl,
            alt: value.alt,
            shift: value.shift,
        }
    }
}

#[pymethods]
impl PyInputEvent {
    fn __repr__(&self) -> String {
        format!(
            "InputEvent(kind={:?}, code={:?}, button={:?}, row={}, column={}, ctrl={}, alt={}, shift={})",
            self.kind,
            self.code,
            self.button,
            self.row,
            self.column,
            self.ctrl,
            self.alt,
            self.shift
        )
    }
}

fn key_code(code: &KeyCode) -> String {
    match code {
        KeyCode::Char(value) => value.to_string(),
        KeyCode::Enter => "Enter".into(),
        KeyCode::Esc => "Esc".into(),
        KeyCode::Backspace => "Backspace".into(),
        KeyCode::Delete => "Delete".into(),
        KeyCode::Tab => "Tab".into(),
        KeyCode::BackTab => "BackTab".into(),
        KeyCode::Up => "Up".into(),
        KeyCode::Down => "Down".into(),
        KeyCode::Left => "Left".into(),
        KeyCode::Right => "Right".into(),
        KeyCode::Home => "Home".into(),
        KeyCode::End => "End".into(),
        KeyCode::PageUp => "PageUp".into(),
        KeyCode::PageDown => "PageDown".into(),
        KeyCode::Insert => "Insert".into(),
        KeyCode::F(number) => format!("F{number}"),
        KeyCode::Null => "Null".into(),
        _ => "Unknown".into(),
    }
}

fn mouse_button(button: MouseButton) -> String {
    match button {
        MouseButton::Left => "left".into(),
        MouseButton::Right => "right".into(),
        MouseButton::Middle => "middle".into(),
    }
}

fn modifier_fields(modifiers: KeyModifiers) -> (bool, bool, bool) {
    (
        modifiers.contains(KeyModifiers::CONTROL),
        modifiers.contains(KeyModifiers::ALT),
        modifiers.contains(KeyModifiers::SHIFT),
    )
}

fn convert_event(value: Event) -> Option<EventFields> {
    match value {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            let (ctrl, alt, shift) = modifier_fields(key.modifiers);
            Some(EventFields {
                kind: "key".into(),
                code: key_code(&key.code),
                button: "none".into(),
                row: 0,
                column: 0,
                ctrl,
                alt,
                shift,
            })
        }
        Event::Mouse(mouse) => {
            let (code, button) = match mouse.kind {
                MouseEventKind::Down(button) => ("down", mouse_button(button)),
                MouseEventKind::Up(button) => ("up", mouse_button(button)),
                MouseEventKind::Drag(button) => ("drag", mouse_button(button)),
                MouseEventKind::Moved => ("moved", "none".into()),
                MouseEventKind::ScrollUp => ("scroll_up", "none".into()),
                MouseEventKind::ScrollDown => ("scroll_down", "none".into()),
                MouseEventKind::ScrollLeft => ("scroll_left", "none".into()),
                MouseEventKind::ScrollRight => ("scroll_right", "none".into()),
            };
            let (ctrl, alt, shift) = modifier_fields(mouse.modifiers);
            Some(EventFields {
                kind: "mouse".into(),
                code: code.into(),
                button,
                row: mouse.row,
                column: mouse.column,
                ctrl,
                alt,
                shift,
            })
        }
        Event::Resize(column, row) => Some(EventFields {
            kind: "resize".into(),
            code: "resize".into(),
            button: "none".into(),
            row,
            column,
            ctrl: false,
            alt: false,
            shift: false,
        }),
        _ => None,
    }
}

fn input_error(error: io::Error) -> PyErr {
    PyRuntimeError::new_err(error.to_string())
}

static IMAGE_PICKER: OnceLock<RwLock<Picker>> = OnceLock::new();

fn image_picker() -> &'static RwLock<Picker> {
    IMAGE_PICKER.get_or_init(|| RwLock::new(Picker::halfblocks()))
}

fn protocol_name(protocol: ProtocolType) -> &'static str {
    match protocol {
        ProtocolType::Halfblocks => "Halfblocks",
        ProtocolType::Sixel => "Sixel",
        ProtocolType::Kitty => "Kitty",
        ProtocolType::Iterm2 => "iTerm2",
    }
}

fn xtsmgraphics_reports_sixel_bytes(data: &[u8]) -> bool {
    // XTSMGRAPHICS color-register query response:
    //   CSI ? 1 ; 0 ; <register-count> S
    // A positive register count is sufficient proof that SIXEL graphics are
    // available, even when DA1 deliberately omits SIXEL capability bit 4.
    const PREFIX: &[u8] = b"\x1b[?1;0;";
    let mut offset = 0usize;
    while let Some(relative) = data[offset..]
        .windows(PREFIX.len())
        .position(|window| window == PREFIX)
    {
        let start = offset + relative + PREFIX.len();
        let tail = &data[start..];
        let Some(end) = tail.iter().position(|byte| *byte == b'S') else {
            return false;
        };
        let value = &tail[..end];
        if !value.is_empty()
            && value.iter().all(u8::is_ascii_digit)
            && std::str::from_utf8(value)
                .ok()
                .and_then(|text| text.parse::<u32>().ok())
                .is_some_and(|count| count > 0)
        {
            return true;
        }
        offset = start + end + 1;
        if offset >= data.len() {
            break;
        }
    }
    false
}

fn probe_xtsmgraphics_sixel() -> bool {
    // WebSSH intentionally keeps the stock xterm.js DA1 response and therefore
    // does not advertise SIXEL there, but it does answer XTSMGRAPHICS. Append
    // DSR so the blocking read has a deterministic end marker even when the
    // graphics query is unsupported.
    let mut stdout = io::stdout();
    if stdout
        .write_all(b"\x1b[?1;1;0S\x1b[5n")
        .and_then(|_| stdout.flush())
        .is_err()
    {
        return false;
    }

    let mut response = Vec::with_capacity(256);
    let mut stdin = io::stdin();
    let mut chunk = [0u8; 128];
    loop {
        let Ok(read) = stdin.read(&mut chunk) else {
            return false;
        };
        if read == 0 {
            return false;
        }
        response.extend_from_slice(&chunk[..read]);
        if response.windows(4).any(|window| window == b"\x1b[0n") {
            break;
        }
        if response.len() >= 4096 {
            break;
        }
    }
    xtsmgraphics_reports_sixel_bytes(&response)
}

fn detect_image_picker() -> String {
    // ratatui-image explicitly requires this query after alternate-screen
    // entry and before terminal-event reading. EventReader::__enter__ is that
    // boundary in SPLINED.
    let mut picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());

    // Some terminals (notably WebSSH) render SIXEL but deliberately omit the
    // SIXEL bit from DA1. ratatui-image then correctly falls back to Halfblocks
    // because it cannot know better. XTSMGRAPHICS provides the missing
    // terminal-side proof, so promote only a Halfblocks result that positively
    // answers that query.
    let xtsm_sixel = picker.protocol_type() == ProtocolType::Halfblocks
        && probe_xtsmgraphics_sixel();
    if xtsm_sixel {
        picker.set_protocol_type(ProtocolType::Sixel);
    }

    let name = if xtsm_sixel {
        "Sixel (XTSMGRAPHICS)".to_string()
    } else {
        protocol_name(picker.protocol_type()).to_string()
    };
    if let Ok(mut active) = image_picker().write() {
        *active = picker;
    }
    name
}

fn current_image_picker() -> Picker {
    image_picker()
        .read()
        .map(|picker| picker.clone())
        .unwrap_or_else(|_| Picker::halfblocks())
}

#[pyclass(module = "splined_pyratatui_input", name = "EventReader", unsendable)]
struct EventReader {
    mouse_captured: bool,
    image_protocol: String,
}

impl EventReader {
    fn disable_capture_best_effort(&mut self) {
        if self.mouse_captured {
            let _ = execute!(io::stdout(), DisableMouseCapture);
            let _ = io::stdout().flush();
            self.mouse_captured = false;
        }
    }
}

#[pyclass(module = "splined_pyratatui_input", name = "ImageOverlay")]
struct ImageOverlay {
    buffer: Buffer,
    width: u16,
    height: u16,
    protocol: String,
}

#[pymethods]
impl ImageOverlay {
    #[getter]
    fn width(&self) -> u16 {
        self.width
    }

    #[getter]
    fn height(&self) -> u16 {
        self.height
    }

    #[getter]
    fn protocol(&self) -> &str {
        &self.protocol
    }

    fn draw(&self, x: u16, y: u16) -> PyResult<()> {
        let mut stdout = io::stdout();
        execute!(stdout, SavePosition).map_err(input_error)?;
        {
            let mut backend = CrosstermBackend::new(&mut stdout);
            let buffer = &self.buffer;
            let width = self.width;
            let height = self.height;
            let cells = (0..height).flat_map(move |row| {
                (0..width).filter_map(move |column| {
                    buffer.cell((column, row)).and_then(|cell| {
                        if matches!(cell.diff_option, CellDiffOption::Skip) {
                            None
                        } else {
                            Some((x + column, y + row, cell))
                        }
                    })
                })
            });
            backend.draw(cells).map_err(input_error)?;
            Backend::flush(&mut backend).map_err(input_error)?;
        }
        execute!(stdout, RestorePosition).map_err(input_error)?;
        stdout.flush().map_err(input_error)?;
        Ok(())
    }
}

#[pyfunction]
#[pyo3(signature = (data, width, height, max_side=1000))]
fn prepare_image_overlay(
    data: &Bound<'_, PyBytes>,
    width: u16,
    height: u16,
    max_side: u32,
) -> PyResult<ImageOverlay> {
    if width == 0 || height == 0 {
        return Err(PyRuntimeError::new_err(
            "ratatui-image overlay requires a non-zero area",
        ));
    }

    let mut image = image::load_from_memory(data.as_bytes())
        .map_err(|error| PyRuntimeError::new_err(format!("image decode failed: {error}")))?;
    let limit = max_side.max(1);
    if image.width() > limit || image.height() > limit {
        image = image.resize(
            limit,
            limit,
            image::imageops::FilterType::Lanczos3,
        );
    }

    let picker = current_image_picker();
    let protocol_name = protocol_name(picker.protocol_type()).to_string();
    let protocol = picker
        .new_protocol(
            image,
            Size::new(width, height),
            Resize::Fit(None),
        )
        .map_err(|error| {
            PyRuntimeError::new_err(format!(
                "ratatui-image {protocol_name} encode failed: {error}"
            ))
        })?;
    let rendered = protocol.size();
    let area = Rect::new(0, 0, rendered.width, rendered.height);
    let mut buffer = Buffer::empty(area);
    RatatuiImage::new(&protocol).render(area, &mut buffer);

    Ok(ImageOverlay {
        buffer,
        width: rendered.width,
        height: rendered.height,
        protocol: protocol_name,
    })
}


#[pyfunction]
fn clear_image_area(x: u16, y: u16, width: u16, height: u16) -> PyResult<()> {
    if width == 0 || height == 0 {
        return Ok(());
    }
    let mut stdout = io::stdout();
    execute!(stdout, SavePosition).map_err(input_error)?;
    for row in 0..height {
        execute!(stdout, MoveTo(x, y.saturating_add(row))).map_err(input_error)?;
        // ECH is the same terminal-cell erasure primitive ratatui-image uses
        // before Sixel/iTerm2 placement.  Using it here removes stale native
        // image pixels before Ratatui repaints the underlying view.
        write!(stdout, "\x1b[{width}X").map_err(input_error)?;
    }
    execute!(stdout, RestorePosition).map_err(input_error)?;
    stdout.flush().map_err(input_error)?;
    Ok(())
}


#[pyfunction]
fn emergency_restore() -> PyResult<()> {
    // Safe to call after partial initialization: each operation is idempotent
    // enough for a terminal cleanup path and later failures do not prevent the
    // remaining restoration operations from being attempted.
    let raw_result = disable_raw_mode();
    let screen_result = execute!(
        io::stdout(),
        DisableMouseCapture,
        LeaveAlternateScreen,
        Show
    );
    let flush_result = io::stdout().flush();
    raw_result.map_err(input_error)?;
    screen_result.map_err(input_error)?;
    flush_result.map_err(input_error)?;
    Ok(())
}

impl Drop for EventReader {
    fn drop(&mut self) {
        self.disable_capture_best_effort();
    }
}

#[pymethods]
impl EventReader {
    #[new]
    fn new() -> Self {
        Self {
            mouse_captured: false,
            image_protocol: "Undetected".into(),
        }
    }

    fn __enter__(mut slf: PyRefMut<'_, Self>) -> PyResult<PyRefMut<'_, Self>> {
        // Query image capabilities before mouse capture/event polling. This is
        // the ordering required by ratatui-image.
        slf.image_protocol = detect_image_picker();
        slf.enable_mouse_capture()?;
        Ok(slf)
    }

    fn __exit__(
        &mut self,
        _exc_type: &Bound<'_, PyAny>,
        _exc_val: &Bound<'_, PyAny>,
        _exc_tb: &Bound<'_, PyAny>,
    ) -> bool {
        self.disable_capture_best_effort();
        false
    }

    fn enable_mouse_capture(&mut self) -> PyResult<()> {
        if !self.mouse_captured {
            execute!(io::stdout(), EnableMouseCapture).map_err(input_error)?;
            io::stdout().flush().map_err(input_error)?;
            self.mouse_captured = true;
        }
        Ok(())
    }

    fn disable_mouse_capture(&mut self) -> PyResult<()> {
        if self.mouse_captured {
            execute!(io::stdout(), DisableMouseCapture).map_err(input_error)?;
            io::stdout().flush().map_err(input_error)?;
            self.mouse_captured = false;
        }
        Ok(())
    }

    #[getter]
    fn mouse_capture_enabled(&self) -> bool {
        self.mouse_captured
    }

    #[getter]
    fn image_protocol(&self) -> &str {
        &self.image_protocol
    }

    #[pyo3(signature = (timeout_ms=0))]
    fn poll_event(&self, timeout_ms: u64) -> PyResult<Option<PyInputEvent>> {
        let timeout = Duration::from_millis(timeout_ms);
        if !event::poll(timeout).map_err(input_error)? {
            return Ok(None);
        }
        let raw = event::read().map_err(input_error)?;
        Ok(convert_event(raw).map(PyInputEvent::from))
    }
}

#[pymodule]
fn _native(_py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<EventReader>()?;
    module.add_class::<PyInputEvent>()?;
    module.add_class::<ImageOverlay>()?;
    module.add_function(wrap_pyfunction!(prepare_image_overlay, module)?)?;
    module.add_function(wrap_pyfunction!(clear_image_area, module)?)?;
    module.add_function(wrap_pyfunction!(emergency_restore, module)?)?;
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyEventState, MouseEvent};

    #[test]
    fn key_events_keep_pyratatui_names_and_modifiers() {
        let event = Event::Key(KeyEvent {
            code: KeyCode::PageDown,
            modifiers: KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        });
        let converted = convert_event(event).expect("key event");
        assert_eq!(converted.kind, "key");
        assert_eq!(converted.code, "PageDown");
        assert!(converted.ctrl);
        assert!(converted.shift);
        assert!(!converted.alt);
    }

    #[test]
    fn mouse_events_expose_button_coordinates_and_modifiers() {
        let event = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 42,
            row: 7,
            modifiers: KeyModifiers::ALT,
        });
        let converted = convert_event(event).expect("mouse event");
        assert_eq!(converted.kind, "mouse");
        assert_eq!(converted.code, "down");
        assert_eq!(converted.button, "left");
        assert_eq!((converted.column, converted.row), (42, 7));
        assert!(converted.alt);
    }

    #[test]
    fn xtsmgraphics_positive_register_response_proves_sixel() {
        assert!(xtsmgraphics_reports_sixel_bytes(
            b"junk\x1b[?1;0;256S\x1b[0n"
        ));
        assert!(xtsmgraphics_reports_sixel_bytes(
            b"\x1b[?1;0;4096S"
        ));
        assert!(!xtsmgraphics_reports_sixel_bytes(
            b"\x1b[?1;0;0S\x1b[0n"
        ));
        assert!(!xtsmgraphics_reports_sixel_bytes(
            b"\x1b[?1;2c\x1b[0n"
        ));
    }

    #[test]
    fn image_protocol_labels_cover_every_ratatui_backend() {
        assert_eq!(protocol_name(ProtocolType::Halfblocks), "Halfblocks");
        assert_eq!(protocol_name(ProtocolType::Sixel), "Sixel");
        assert_eq!(protocol_name(ProtocolType::Kitty), "Kitty");
        assert_eq!(protocol_name(ProtocolType::Iterm2), "iTerm2");
    }

    #[test]
    fn mouse_move_events_are_exposed_for_url_hover() {
        let event = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            column: 19,
            row: 8,
            modifiers: KeyModifiers::NONE,
        });
        let converted = convert_event(event).expect("mouse move event");
        assert_eq!(converted.kind, "mouse");
        assert_eq!(converted.code, "moved");
        assert_eq!(converted.button, "none");
        assert_eq!((converted.column, converted.row), (19, 8));
    }

    #[test]
    fn wheel_directions_are_stable() {
        for (kind, expected) in [
            (MouseEventKind::ScrollUp, "scroll_up"),
            (MouseEventKind::ScrollDown, "scroll_down"),
            (MouseEventKind::ScrollLeft, "scroll_left"),
            (MouseEventKind::ScrollRight, "scroll_right"),
        ] {
            let event = Event::Mouse(MouseEvent {
                kind,
                column: 1,
                row: 2,
                modifiers: KeyModifiers::NONE,
            });
            assert_eq!(convert_event(event).expect("wheel event").code, expected);
        }
    }
}
