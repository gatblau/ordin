//! `ordin`, the desktop program (OrdinDesktop.md *Rust items of the program*: `main`).
// No console window behind the program on Windows in a release build.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// The Tauri shell, then the core (`ordin_desktop::window::run`).
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn main() {
    ordin_desktop::window::run();
}

/// The program ships for macOS and Windows only (HLD 05); elsewhere the library builds and is
/// tested, and the program says so and stops.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn main() {
    eprintln!("ordin runs on macOS and Windows only.");
    std::process::exit(1);
}
