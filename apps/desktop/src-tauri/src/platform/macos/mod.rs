//! macOS implementation of `Platform`.

pub mod ax;
pub mod frontmost;
pub mod modifier_monitor;
pub mod secure_input;

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
        unimplemented!("pasteboard and key posting are not wired yet")
    }

    fn secure_input_active(&self) -> bool {
        secure_input::is_active()
    }

    fn clipboard_snapshot(&self) -> ClipboardSnapshot {
        unimplemented!("pasteboard and key posting are not wired yet")
    }

    fn clipboard_write_text(&self, _text: &str) {
        unimplemented!("pasteboard and key posting are not wired yet")
    }

    fn clipboard_restore(&self, _snapshot: ClipboardSnapshot) {
        unimplemented!("pasteboard and key posting are not wired yet")
    }

    fn post_paste(&self) {
        unimplemented!("pasteboard and key posting are not wired yet")
    }

    fn type_unicode(&self, _text: &str) {
        unimplemented!("pasteboard and key posting are not wired yet")
    }

    fn ax_insert(&self, pid: i32, text: &str) -> Result<(), PlatformError> {
        ax::insert_at_selection(pid, text)
    }

    fn accessibility_trusted(&self, prompt: bool) -> bool {
        ax::is_trusted(prompt)
    }
}
