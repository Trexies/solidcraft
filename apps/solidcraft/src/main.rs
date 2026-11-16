//! SolidCraft desktop app: the window and everything in it is the library (`src/lib.rs`), so Ledger can host the same
//! editor in its own window.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    solidcraft::run()
}
