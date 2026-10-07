#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod tauri_app;

#[cfg(windows)]
fn main() {
    if let Err(error) = tauri_app::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The SPLINED desktop application is available on Windows.");
}
