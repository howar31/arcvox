//! macOS implementation of `Platform`.

pub mod apple;
pub mod ax;
pub mod dock_icon;
pub mod event_tap;
pub mod frontmost;
pub mod hardware;
pub mod hover;
pub mod microphone;
pub mod modifier_monitor;
pub mod pasteboard;
pub mod post_keys;
pub mod screens;
pub mod secure_input;
pub mod terminate;

use std::sync::mpsc::Sender;
use std::sync::Arc;

use crate::platform::{
    AxField, CaptureSink, ClipboardSnapshot, FrontmostApp, MicAccess, ModifierEvent, Platform,
    PlatformError,
};
use crate::shortcut::Shortcut;

/// Whether the system asks for reduced motion.
pub fn reduce_motion() -> bool {
    objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

pub struct MacPlatform {
    tap: Arc<event_tap::TapState>,
}

impl Default for MacPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl MacPlatform {
    pub fn new() -> Self {
        Self {
            tap: event_tap::TapState::new(),
        }
    }
}

impl Platform for MacPlatform {
    fn start_modifier_monitor(&self, tx: Sender<ModifierEvent>) -> Result<(), PlatformError> {
        if !event_tap::can_tap_keyboard() {
            return Err(PlatformError::NotTrusted);
        }
        if event_tap::install(&self.tap, tx.clone()) {
            return Ok(());
        }
        // Without the tap only the fn key can be watched.
        tracing::warn!("event tap not available; watching the fn key only");
        if modifier_monitor::install(tx) {
            Ok(())
        } else {
            Err(PlatformError::Other(
                "event monitor could not be installed".into(),
            ))
        }
    }

    fn stop_modifier_monitor(&self) {
        self.tap.suspend();
    }

    fn watch_accessibility(&self, notify: Box<dyn Fn() + Send + Sync>) {
        ax::watch_trust(notify);
    }

    fn set_shortcuts(&self, shortcuts: Vec<Shortcut>) {
        self.tap.set_shortcuts(shortcuts);
    }

    fn set_capture_sink(&self, sink: CaptureSink) {
        self.tap.set_sink(sink);
    }

    fn set_escape_cancels(&self, on: bool) {
        self.tap.set_escape_cancels(on);
    }

    fn set_shortcuts_paused(&self, on: bool) {
        self.tap.set_paused(on);
    }

    fn set_shortcut_capture(&self, on: bool) -> bool {
        if !self.tap.running() || self.tap.paused() {
            return false;
        }
        self.tap.set_capture(on);
        true
    }

    fn frontmost_app(&self) -> Option<FrontmostApp> {
        frontmost::frontmost_app(event_tap::can_tap_keyboard())
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

    fn clipboard_write_text(&self, text: &str) -> bool {
        pasteboard::write_text(text)
    }

    fn clipboard_write_png(&self, png: &[u8]) -> bool {
        pasteboard::write_png(png)
    }

    fn clipboard_write_passing(&self, text: &str) -> bool {
        pasteboard::write_passing(text)
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

    fn ax_field(&self, pid: i32) -> Option<Box<dyn AxField>> {
        ax::field(pid).map(|f| Box::new(f) as Box<dyn AxField>)
    }

    fn accessibility_trusted(&self, prompt: bool) -> bool {
        if prompt {
            // Shows the system's request; its answer is not relied on.
            ax::is_trusted(true);
        }
        event_tap::can_tap_keyboard()
    }

    fn keyboard_watch_only(&self) -> bool {
        !event_tap::can_tap_keyboard() && event_tap::can_listen_keyboard()
    }

    fn microphone_access(&self) -> MicAccess {
        microphone::access()
    }

    fn request_microphone(&self) -> MicAccess {
        if microphone::access() == MicAccess::NotAsked {
            let (tx, rx) = std::sync::mpsc::channel();
            microphone::request(Box::new(move || {
                let _ = tx.send(());
            }));
            let _ = rx.recv();
        }
        microphone::access()
    }
}
