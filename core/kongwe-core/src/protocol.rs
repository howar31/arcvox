//! Event protocol between the engine and its clients. These types are the
//! exact JSON payloads a future WebSocket server sends, so every variant has
//! a snapshot test.

use serde::{Deserialize, Serialize};

use crate::types::{ErrorCode, HistoryId, ProfileId, SessionId, StepName};

/// Bump when a field is renamed or removed. Adding fields is compatible.
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    SessionStarted {
        session_id: SessionId,
        profile_id: ProfileId,
        protocol_version: u32,
    },
    Transcript {
        text: String,
        is_final: bool,
        language: Option<String>,
    },
    StepStarted {
        step: StepName,
    },
    StepFinished {
        step: StepName,
        text: String,
        elapsed_ms: u64,
    },
    Refined {
        text: String,
        degraded: bool,
    },
    Error {
        step: Option<StepName>,
        code: ErrorCode,
        message: String,
        recoverable: bool,
        hint: Option<String>,
    },
    Done {
        session_id: SessionId,
        history_id: Option<HistoryId>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snap(event: Event, expected: serde_json::Value) {
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value, expected, "serialized form changed");
        let back: Event = serde_json::from_value(value).unwrap();
        assert_eq!(back, event, "round trip changed");
    }

    #[test]
    fn error_event_model_missing_snapshot() {
        let e = Event::Error {
            step: Some(StepName::Asr),
            code: ErrorCode::ModelMissing,
            message: "model file not found".into(),
            recoverable: false,
            hint: Some("download the model in Settings > Engines and models".into()),
        };
        let v: serde_json::Value = serde_json::to_value(&e).unwrap();
        assert_eq!(v["code"], "model_missing");
    }

    #[test]
    fn session_started() {
        snap(
            Event::SessionStarted {
                session_id: SessionId("s1".into()),
                profile_id: ProfileId("default".into()),
                protocol_version: PROTOCOL_VERSION,
            },
            json!({"type":"session_started","session_id":"s1","profile_id":"default","protocol_version":1}),
        );
    }

    #[test]
    fn transcript() {
        snap(
            Event::Transcript {
                text: "你好".into(),
                is_final: true,
                language: Some("zh".into()),
            },
            json!({"type":"transcript","text":"你好","is_final":true,"language":"zh"}),
        );
    }

    #[test]
    fn step_started_and_finished() {
        snap(
            Event::StepStarted {
                step: StepName::Llm,
            },
            json!({"type":"step_started","step":"llm"}),
        );
        snap(
            Event::StepFinished {
                step: StepName::OpenCc,
                text: "臺灣".into(),
                elapsed_ms: 3,
            },
            json!({"type":"step_finished","step":"opencc","text":"臺灣","elapsed_ms":3}),
        );
    }

    #[test]
    fn refined() {
        snap(
            Event::Refined {
                text: "整理後".into(),
                degraded: false,
            },
            json!({"type":"refined","text":"整理後","degraded":false}),
        );
    }

    #[test]
    fn error() {
        snap(
            Event::Error {
                step: Some(StepName::Asr),
                code: ErrorCode::ProviderUnreachable,
                message: "connection refused".into(),
                recoverable: false,
                hint: Some("check that Speaches is running".into()),
            },
            json!({"type":"error","step":"asr","code":"provider_unreachable","message":"connection refused","recoverable":false,"hint":"check that Speaches is running"}),
        );
    }

    #[test]
    fn done() {
        snap(
            Event::Done {
                session_id: SessionId("s1".into()),
                history_id: Some(HistoryId(42)),
            },
            json!({"type":"done","session_id":"s1","history_id":42}),
        );
    }
}
