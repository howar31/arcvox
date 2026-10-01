//! What the controller tells the user interface. Implemented by the Tauri
//! layer (tray + widget) and by a recorder in tests.

use crate::i18n::Msg;
use kongwe_core::types::ErrorCode;

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
    /// Stays until it is dismissed or the next dictation starts.
    pub sticky: bool,
    /// The kind of failure, for a failed dictation.
    pub code: Option<ErrorCode>,
    /// The failed dictation was kept and can be retried from the history.
    pub retry: bool,
}

impl UiMessage {
    pub fn info(msg: Msg) -> Self {
        Self {
            kind: MessageKind::Info,
            msg,
            detail: None,
            copyable: None,
            sticky: false,
            code: None,
            retry: false,
        }
    }
    pub fn error(msg: Msg, detail: Option<String>) -> Self {
        Self {
            kind: MessageKind::Error,
            msg,
            detail,
            copyable: None,
            sticky: false,
            code: None,
            retry: false,
        }
    }
    pub fn copyable(msg: Msg, text: String) -> Self {
        Self {
            kind: MessageKind::Info,
            msg,
            detail: None,
            copyable: Some(text),
            sticky: false,
            code: None,
            retry: false,
        }
    }
    /// A failed dictation: the detail can be copied and the message stays.
    /// `retry` says the recording was kept for a retry from the history.
    pub fn failure(msg: Msg, detail: String, code: ErrorCode, retry: bool) -> Self {
        Self {
            kind: MessageKind::Error,
            msg,
            detail: None,
            copyable: Some(detail),
            sticky: true,
            code: Some(code),
            retry,
        }
    }

    /// A short notice that the widget itself shows, with nothing further
    /// to read in the panel.
    pub fn is_notice(&self) -> bool {
        matches!(self.msg, Msg::WidgetCancelled | Msg::WidgetNoSpeech)
    }
}

pub trait Ui: Send + Sync {
    fn set_state(&self, state: UiState);
    /// RMS of the latest audio block, 0.0 to 1.0, while recording.
    fn level(&self, rms: f32);
    fn message(&self, message: UiMessage);
    /// The final ASR text, sent while the rewrite step is still running.
    fn transcript(&self, text: String);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_and_no_speech_are_notices() {
        assert!(UiMessage::info(Msg::WidgetCancelled).is_notice());
        assert!(UiMessage::info(Msg::WidgetNoSpeech).is_notice());
        // The engine reports an utterance without speech as an error with a detail.
        assert!(UiMessage::error(Msg::WidgetNoSpeech, Some("no speech".into())).is_notice());
        assert!(!UiMessage::error(Msg::WidgetEmptyResult, None).is_notice());
        assert!(!UiMessage::copyable(Msg::WidgetAppChanged, "text".into()).is_notice());
        let failure =
            UiMessage::failure(Msg::WidgetAsrFailed, "x".into(), ErrorCode::Internal, false);
        assert!(!failure.is_notice());
    }
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
        /// Every state and message in the order it actually arrived, as
        /// `"state:idle"` / `"message:widget.degraded"` entries. `states`
        /// and `messages` each lose ordering relative to the other; tests
        /// that need to prove one happened before the other read this.
        pub log: Mutex<Vec<String>>,
        pub transcripts: Mutex<Vec<String>>,
    }

    impl Ui for FakeUi {
        fn set_state(&self, state: UiState) {
            self.states.lock().unwrap().push(state);
            let name = format!("{state:?}").to_lowercase();
            self.log.lock().unwrap().push(format!("state:{name}"));
        }
        fn level(&self, rms: f32) {
            self.levels.lock().unwrap().push(rms);
        }
        fn message(&self, message: UiMessage) {
            self.log
                .lock()
                .unwrap()
                .push(format!("message:{}", message.msg.key()));
            self.messages.lock().unwrap().push(message);
        }
        fn transcript(&self, text: String) {
            self.log.lock().unwrap().push(format!("transcript:{text}"));
            self.transcripts.lock().unwrap().push(text);
        }
    }
}
