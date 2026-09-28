//! Everything the client needs from the operating system, behind one trait
//! so the controller can be tested with a fake. `macos` is the only
//! implementation in phase 1.

use std::sync::mpsc::Sender;
use std::time::Instant;

#[cfg(target_os = "macos")]
pub mod macos;

#[derive(Debug, Clone, Copy)]
pub enum ModifierEvent {
    FnDown {
        at: Instant,
    },
    FnUp {
        at: Instant,
    },
    /// Any non-modifier key went down system-wide. Only the virtual key
    /// code is carried; it is never logged.
    KeyDown {
        key_code: u16,
        at: Instant,
    },
}

impl ModifierEvent {
    /// When the underlying OS event actually happened, as opposed to when
    /// the controller gets around to handling it (the controller thread can
    /// be busy, for example opening the microphone stream, for tens of
    /// milliseconds).
    pub fn at(&self) -> Instant {
        match self {
            ModifierEvent::FnDown { at }
            | ModifierEvent::FnUp { at }
            | ModifierEvent::KeyDown { at, .. } => *at,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrontmostApp {
    pub pid: i32,
    pub bundle_id: Option<String>,
    pub name: Option<String>,
    pub window_title: Option<String>,
    /// Only browsers report a URL.
    pub url: Option<String>,
}

/// Every pasteboard item with every type it carried, so files, rich text,
/// and images survive a round trip through a paste.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClipboardSnapshot {
    pub items: Vec<Vec<(String, Vec<u8>)>>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlatformError {
    #[error("accessibility permission not granted")]
    NotTrusted,
    #[error("accessibility call failed with code {0}")]
    Ax(i32),
    #[error("{0}")]
    Other(String),
}

pub trait Platform: Send + Sync {
    /// Starts delivering fn key transitions and key presses. Must be called
    /// on the main thread. Returns an error when the monitor cannot start
    /// (for example, accessibility not granted).
    fn start_modifier_monitor(&self, tx: Sender<ModifierEvent>) -> Result<(), PlatformError>;
    fn frontmost_app(&self) -> Option<FrontmostApp>;
    fn selected_text(&self, pid: i32) -> Option<String>;
    fn clipboard_text(&self) -> Option<String>;
    fn secure_input_active(&self) -> bool;
    fn clipboard_snapshot(&self) -> ClipboardSnapshot;
    fn clipboard_write_text(&self, text: &str);
    fn clipboard_restore(&self, snapshot: ClipboardSnapshot);
    fn post_paste(&self);
    fn type_unicode(&self, text: &str);
    fn ax_insert(&self, pid: i32, text: &str) -> Result<(), PlatformError>;
    fn accessibility_trusted(&self, prompt: bool) -> bool;
}

#[cfg(test)]
pub mod fake {
    use super::*;
    use std::sync::Mutex;

    /// Records what the client asked the OS to do.
    #[derive(Default)]
    pub struct FakePlatform {
        pub frontmost: Mutex<Option<FrontmostApp>>,
        pub selected: Mutex<Option<String>>,
        pub clipboard: Mutex<Option<String>>,
        pub secure_input: Mutex<bool>,
        pub trusted: Mutex<bool>,
        /// `Some(err)` makes `ax_insert` fail with that error.
        pub ax_insert_failure: Mutex<Option<PlatformError>>,
        pub monitor_tx: Mutex<Option<Sender<ModifierEvent>>>,
        pub calls: Mutex<Vec<String>>,
    }

    impl FakePlatform {
        pub fn new() -> Self {
            let f = Self::default();
            *f.trusted.lock().unwrap() = true;
            f
        }
        pub fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
        fn record(&self, s: String) {
            self.calls.lock().unwrap().push(s);
        }
        /// Sends a modifier event to the monitor's subscriber.
        pub fn emit(&self, ev: ModifierEvent) {
            if let Some(tx) = self.monitor_tx.lock().unwrap().as_ref() {
                let _ = tx.send(ev);
            }
        }
    }

    impl Platform for FakePlatform {
        fn start_modifier_monitor(&self, tx: Sender<ModifierEvent>) -> Result<(), PlatformError> {
            *self.monitor_tx.lock().unwrap() = Some(tx);
            Ok(())
        }
        fn frontmost_app(&self) -> Option<FrontmostApp> {
            self.frontmost.lock().unwrap().clone()
        }
        fn selected_text(&self, _pid: i32) -> Option<String> {
            self.selected.lock().unwrap().clone()
        }
        fn clipboard_text(&self) -> Option<String> {
            self.clipboard.lock().unwrap().clone()
        }
        fn secure_input_active(&self) -> bool {
            *self.secure_input.lock().unwrap()
        }
        fn clipboard_snapshot(&self) -> ClipboardSnapshot {
            self.record("snapshot".into());
            ClipboardSnapshot {
                items: vec![vec![(
                    "public.utf8-plain-text".into(),
                    self.clipboard
                        .lock()
                        .unwrap()
                        .clone()
                        .unwrap_or_default()
                        .into_bytes(),
                )]],
            }
        }
        fn clipboard_write_text(&self, text: &str) {
            self.record(format!("write:{text}"));
            *self.clipboard.lock().unwrap() = Some(text.to_string());
        }
        fn clipboard_restore(&self, snapshot: ClipboardSnapshot) {
            self.record("restore".into());
            let text = snapshot
                .items
                .first()
                .and_then(|i| i.first())
                .map(|(_, b)| String::from_utf8_lossy(b).into_owned());
            *self.clipboard.lock().unwrap() = text;
        }
        fn post_paste(&self) {
            self.record("paste".into());
        }
        fn type_unicode(&self, text: &str) {
            self.record(format!("type:{text}"));
        }
        fn ax_insert(&self, _pid: i32, text: &str) -> Result<(), PlatformError> {
            self.record(format!("ax:{text}"));
            match self.ax_insert_failure.lock().unwrap().clone() {
                Some(e) => Err(e),
                None => Ok(()),
            }
        }
        fn accessibility_trusted(&self, _prompt: bool) -> bool {
            *self.trusted.lock().unwrap()
        }
    }
}
