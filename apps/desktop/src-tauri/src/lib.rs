//! Kongwe desktop client. Everything platform-specific lives under `platform`.

pub mod app;
pub mod audio;
pub mod commands;
pub mod context;
pub mod dictation;
pub mod hotkey;
pub mod i18n;
pub mod insert;
pub mod logging;
pub mod platform;
pub mod providers;
pub mod secrets;
pub mod settings;
pub mod shortcut;
pub mod ui;
pub mod vad;

pub fn run() {
    app::run()
}
