//! Arcvox desktop client. Everything platform-specific lives under `platform`.

pub mod app;
pub mod context;
pub mod hotkey;
pub mod i18n;
pub mod insert;
pub mod platform;
pub mod secrets;
pub mod settings;
pub mod vad;

pub fn run() {
    app::run()
}
