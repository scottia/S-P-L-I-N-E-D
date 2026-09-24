use std::io::{self, Write};
use std::time::Duration;

use crossterm::cursor::Show;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{LeaveAlternateScreen, disable_raw_mode};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

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

#[pyclass(module = "splined_pyratatui_input", name = "EventReader", unsendable)]
struct EventReader {
    mouse_captured: bool,
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
        }
    }

    fn __enter__(mut slf: PyRefMut<'_, Self>) -> PyResult<PyRefMut<'_, Self>> {
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
