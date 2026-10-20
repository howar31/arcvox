//! Everything the client needs from the operating system, behind one trait
//! so the controller can be tested with a fake. `macos` is the only
//! implementation in phase 1.

use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Instant;

use crate::shortcut::{Key, Shortcut};

#[cfg(target_os = "macos")]
pub mod macos;

#[derive(Debug, Clone, Copy)]
pub enum ModifierEvent {
    /// A recording shortcut went down.
    TriggerDown { at: Instant },
    /// The recording shortcut that was down is no longer held.
    TriggerUp { at: Instant },
    /// Any non-modifier key went down system-wide. Only the virtual key
    /// code is carried; it is never logged.
    KeyDown { key_code: u16, at: Instant },
}

impl ModifierEvent {
    /// When the underlying OS event actually happened, as opposed to when
    /// the controller gets around to handling it (the controller thread can
    /// be busy, for example opening the microphone stream, for tens of
    /// milliseconds).
    pub fn at(&self) -> Instant {
        match self {
            ModifierEvent::TriggerDown { at }
            | ModifierEvent::TriggerUp { at }
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

/// The system's answer on recording with the microphone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MicAccess {
    Granted,
    /// The system has not asked the user yet; it asks at the first recording.
    NotAsked,
    Denied,
    /// Ruled out by the device's management; the user cannot change it.
    Restricted,
}

/// What recording a new shortcut reports to the settings window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureEvent {
    /// The keys held now, or with `done` the combination that was pressed
    /// and let go.
    Keys { keys: Vec<Key>, done: bool },
    /// Escape alone, or nothing was pressed for a while.
    Cancelled,
}

pub type CaptureSink = Arc<dyn Fn(CaptureEvent) + Send + Sync>;

/// What the focused element of an application holds, read through
/// Accessibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AxText {
    /// The application has no focused element now.
    Gone,
    /// The focused element gives no text value (a terminal, a canvas).
    Unsupported,
    Text(String),
}

pub trait Platform: Send + Sync {
    /// Starts delivering the recording shortcut's transitions and key
    /// presses. Must be called on the main thread. Returns an error when
    /// the monitor cannot start (for example, accessibility not granted).
    fn start_modifier_monitor(&self, tx: Sender<ModifierEvent>) -> Result<(), PlatformError>;
    /// Stops delivering shortcut events until the monitor is started again.
    /// Called when the Accessibility trust is taken away while the app runs.
    fn stop_modifier_monitor(&self) {}
    /// Calls `notify` whenever the system reports a change of the
    /// Accessibility trust; a platform without such a report does nothing.
    fn watch_accessibility(&self, _notify: Box<dyn Fn() + Send + Sync>) {}
    /// The shortcuts that start a recording.
    fn set_shortcuts(&self, _shortcuts: Vec<Shortcut>) {}
    /// Where the keys pressed while recording a new shortcut are reported.
    fn set_capture_sink(&self, _sink: CaptureSink) {}
    /// Starts or ends recording a new shortcut. False when this platform
    /// cannot record one right now.
    fn set_shortcut_capture(&self, _on: bool) -> bool {
        false
    }
    /// Whether an Escape press would cancel a dictation now. While it
    /// would, the press is kept from the application in front; otherwise
    /// Escape reaches the application as any key does.
    fn set_escape_cancels(&self, _on: bool) {}
    /// Pauses the shortcuts without taking the monitor down: every key and
    /// mouse button reaches the application in front until resumed, and a
    /// shortcut held at that moment is released.
    fn set_shortcuts_paused(&self, _on: bool) {}
    fn frontmost_app(&self) -> Option<FrontmostApp>;
    fn selected_text(&self, pid: i32) -> Option<String>;
    fn clipboard_text(&self) -> Option<String>;
    fn secure_input_active(&self) -> bool;
    fn clipboard_snapshot(&self) -> ClipboardSnapshot;
    /// Puts `text` on the clipboard; `false` when the system refused it.
    fn clipboard_write_text(&self, text: &str) -> bool;
    /// Writes text that is on the clipboard only to be pasted and is taken
    /// off again; marked so that clipboard histories can leave it out.
    fn clipboard_write_passing(&self, text: &str) -> bool {
        self.clipboard_write_text(text)
    }
    fn clipboard_restore(&self, snapshot: ClipboardSnapshot);
    fn post_paste(&self);
    fn type_unicode(&self, text: &str);
    fn ax_insert(&self, pid: i32, text: &str) -> Result<(), PlatformError>;
    /// Where the caret stands in the focused text element of `pid`, in
    /// UTF-16 units from the start of its value; None when the element
    /// gives no selection range.
    fn ax_caret(&self, _pid: i32) -> Option<usize> {
        None
    }
    /// The value of the focused text element of `pid`.
    fn ax_text(&self, _pid: i32) -> AxText {
        AxText::Unsupported
    }
    fn accessibility_trusted(&self, prompt: bool) -> bool;
    /// Whether the system lets the process watch the keyboard without
    /// letting it act on it: a permission of its own on macOS, which is not
    /// the one the app needs.
    fn keyboard_watch_only(&self) -> bool {
        false
    }
    fn microphone_access(&self) -> MicAccess {
        MicAccess::Granted
    }
    /// Asks the system for the microphone and waits for the answer.
    fn request_microphone(&self) -> MicAccess {
        self.microphone_access()
    }
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
        pub microphone: Mutex<Option<MicAccess>>,
        /// `Some(err)` makes `ax_insert` fail with that error.
        pub ax_insert_failure: Mutex<Option<PlatformError>>,
        /// `true` makes every clipboard write fail.
        pub clipboard_failure: Mutex<bool>,
        pub monitor_tx: Mutex<Option<Sender<ModifierEvent>>>,
        pub calls: Mutex<Vec<String>>,
        /// Every value `set_escape_cancels` was given, in order.
        pub escape_cancels: Mutex<Vec<bool>>,
        /// What `ax_caret` answers.
        pub ax_caret: Mutex<Option<usize>>,
        /// What `ax_text` answers; None is `AxText::Unsupported`.
        pub ax_text: Mutex<Option<String>>,
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
        fn set_escape_cancels(&self, on: bool) {
            self.escape_cancels.lock().unwrap().push(on);
        }
        fn set_shortcuts_paused(&self, on: bool) {
            self.record(format!("shortcuts_paused:{on}"));
        }
        fn stop_modifier_monitor(&self) {
            self.record("stop_monitor".into());
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
        fn clipboard_write_text(&self, text: &str) -> bool {
            self.record(format!("write:{text}"));
            if *self.clipboard_failure.lock().unwrap() {
                return false;
            }
            *self.clipboard.lock().unwrap() = Some(text.to_string());
            true
        }
        fn clipboard_write_passing(&self, text: &str) -> bool {
            self.record(format!("passing:{text}"));
            if *self.clipboard_failure.lock().unwrap() {
                return false;
            }
            *self.clipboard.lock().unwrap() = Some(text.to_string());
            true
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
        fn ax_caret(&self, _pid: i32) -> Option<usize> {
            *self.ax_caret.lock().unwrap()
        }
        fn ax_text(&self, _pid: i32) -> AxText {
            self.record("ax_text".into());
            match self.ax_text.lock().unwrap().clone() {
                Some(t) => AxText::Text(t),
                None => AxText::Unsupported,
            }
        }
        fn accessibility_trusted(&self, _prompt: bool) -> bool {
            *self.trusted.lock().unwrap()
        }
        fn microphone_access(&self) -> MicAccess {
            self.microphone
                .lock()
                .unwrap()
                .unwrap_or(MicAccess::Granted)
        }
    }
}
