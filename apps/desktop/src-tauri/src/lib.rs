//! Arcvox desktop client. Everything platform-specific lives under `platform`.

pub mod app;
pub mod settings;

pub fn run() {
    app::run()
}
