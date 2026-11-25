//! The `undavi` binary; everything lives in the library (see `lib.rs`).

// No console window next to the app on Windows. (Debug builds keep theirs.)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> iced::Result {
    #[cfg(windows)]
    undavi::attach_console();
    undavi::run()
}
