//! What the controller tells the user interface. Implemented by the Tauri
//! layer (tray + widget) and by a recorder in tests.

use crate::i18n::Msg;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiState {
    Idle,
    Recording,
    Locked,
    Processing,
    Inserting,
    /// Accessibility permission missing; dictation disabled.
    NoAccessibility,
    /// The engine could not open; dictation disabled.
    EngineError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiMessage {
    pub kind: MessageKind,
    pub msg: Msg,
    /// Extra text from the core (its `message` or `hint`), shown smaller.
    pub detail: Option<String>,
    /// When set, the widget shows this text with a Copy button and stays.
    pub copyable: Option<String>,
}

impl UiMessage {
    pub fn info(msg: Msg) -> Self {
        Self {
            kind: MessageKind::Info,
            msg,
            detail: None,
            copyable: None,
        }
    }
    pub fn error(msg: Msg, detail: Option<String>) -> Self {
        Self {
            kind: MessageKind::Error,
            msg,
            detail,
            copyable: None,
        }
    }
    pub fn copyable(msg: Msg, text: String) -> Self {
        Self {
            kind: MessageKind::Info,
            msg,
            detail: None,
            copyable: Some(text),
        }
    }
}

pub trait Ui: Send + Sync {
    fn set_state(&self, state: UiState);
    /// RMS of the latest audio block, 0.0 to 1.0, while recording.
    fn level(&self, rms: f32);
    fn message(&self, message: UiMessage);
}

#[cfg(test)]
pub mod fake {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct FakeUi {
        pub states: Mutex<Vec<UiState>>,
        pub messages: Mutex<Vec<UiMessage>>,
        pub levels: Mutex<Vec<f32>>,
    }

    impl Ui for FakeUi {
        fn set_state(&self, state: UiState) {
            self.states.lock().unwrap().push(state);
        }
        fn level(&self, rms: f32) {
            self.levels.lock().unwrap().push(rms);
        }
        fn message(&self, message: UiMessage) {
            self.messages.lock().unwrap().push(message);
        }
    }
}
