// No console window behind the app on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! The desktop program. RATA itself is the library (`lib.rs`), which a
//! phone starts its own way.

fn main() {
    rata_app_lib::run()
}
