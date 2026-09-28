//! Kongwe desktop client. Everything platform-specific lives under `platform`.

pub mod app;
pub mod context;
pub mod hotkey;
pub mod insert;
pub mod platform;
pub mod settings;
pub mod vad;

pub fn run() {
    app::run()
}
