//! Kongwe desktop client. Everything platform-specific lives under `platform`.

pub mod app;
pub mod hotkey;
pub mod settings;
pub mod vad;

pub fn run() {
    app::run()
}
