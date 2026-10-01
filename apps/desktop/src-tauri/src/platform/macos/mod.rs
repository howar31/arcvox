//! macOS implementation of `Platform`.

pub mod apple;
pub mod ax;
pub mod frontmost;
pub mod modifier_monitor;
pub mod pasteboard;
pub mod post_keys;
pub mod secure_input;
pub mod terminate;

use std::sync::mpsc::Sender;

use crate::platform::{ClipboardSnapshot, FrontmostApp, ModifierEvent, Platform, PlatformError};

#[derive(Default)]
pub struct MacPlatform;

impl MacPlatform {
    pub fn new() -> Self {
        Self
    }
}

impl Platform for MacPlatform {
    fn start_modifier_monitor(&self, tx: Sender<ModifierEvent>) -> Result<(), PlatformError> {
        if !ax::is_trusted(false) {
            return Err(PlatformError::NotTrusted);
        }
        if modifier_monitor::install(tx) {
            Ok(())
        } else {
            Err(PlatformError::Other(
                "event monitor could not be installed".into(),
            ))
        }
    }

    fn frontmost_app(&self) -> Option<FrontmostApp> {
        frontmost::frontmost_app(ax::is_trusted(false))
    }

    fn selected_text(&self, pid: i32) -> Option<String> {
        ax::selected_text(pid)
    }

    fn clipboard_text(&self) -> Option<String> {
        pasteboard::text()
    }

    fn secure_input_active(&self) -> bool {
        secure_input::is_active()
    }

    fn clipboard_snapshot(&self) -> ClipboardSnapshot {
        pasteboard::snapshot()
    }

    fn clipboard_write_text(&self, text: &str) {
        pasteboard::write_text(text)
    }

    fn clipboard_restore(&self, snapshot: ClipboardSnapshot) {
        pasteboard::restore(snapshot)
    }

    fn post_paste(&self) {
        post_keys::post_paste()
    }

    fn type_unicode(&self, text: &str) {
        post_keys::type_chunks(&crate::insert::executors::chunk_utf16(text))
    }

    fn ax_insert(&self, pid: i32, text: &str) -> Result<(), PlatformError> {
        ax::insert_at_selection(pid, text)
    }

    fn accessibility_trusted(&self, prompt: bool) -> bool {
        ax::is_trusted(prompt)
    }
}
