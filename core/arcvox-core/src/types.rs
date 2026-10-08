//! Data types shared by every module.

use serde::{Deserialize, Serialize};

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

string_id!(ProviderId);
string_id!(ProfileId);
string_id!(SessionId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HistoryId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepName {
    Asr,
    #[serde(rename = "opencc")]
    OpenCc,
    Llm,
    Rules,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    ProviderUnreachable,
    ProviderUnauthorized,
    ProviderTimeout,
    ProviderError,
    InvalidConfig,
    NoAudio,
    NoSpeech,
    Cancelled,
    Internal,
    ModelMissing,
    ModelCorrupt,
    NativeUnavailable,
    InferenceFailed,
    /// The dictation was recorded and never reached a result.
    Interrupted,
    /// The result was produced, but its row or its recording could not be
    /// written; recoverable, sent before `Done`.
    StorageFailed,
}

impl ErrorCode {
    /// The name the code has on the wire and in stored rows.
    pub fn name(self) -> &'static str {
        match self {
            ErrorCode::ProviderUnreachable => "provider_unreachable",
            ErrorCode::ProviderUnauthorized => "provider_unauthorized",
            ErrorCode::ProviderTimeout => "provider_timeout",
            ErrorCode::ProviderError => "provider_error",
            ErrorCode::InvalidConfig => "invalid_config",
            ErrorCode::NoAudio => "no_audio",
            ErrorCode::NoSpeech => "no_speech",
            ErrorCode::Cancelled => "cancelled",
            ErrorCode::Internal => "internal",
            ErrorCode::ModelMissing => "model_missing",
            ErrorCode::ModelCorrupt => "model_corrupt",
            ErrorCode::NativeUnavailable => "native_unavailable",
            ErrorCode::InferenceFailed => "inference_failed",
            ErrorCode::Interrupted => "interrupted",
            ErrorCode::StorageFailed => "storage_failed",
        }
    }
}

/// What the client knows about the frontmost application.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppInfo {
    /// Bundle id on macOS, executable name on Windows.
    pub app_id: Option<String>,
    pub app_name: Option<String>,
    pub window_title: Option<String>,
    pub url: Option<String>,
}

/// Context fields already filtered by the client's per-field opt-ins.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Context {
    pub app_name: Option<String>,
    pub window_title: Option<String>,
    pub selected_text: Option<String>,
    pub clipboard: Option<String>,
}

/// Mono audio, f32 samples in [-1.0, 1.0].
#[derive(Debug, Clone, PartialEq)]
pub struct AudioClip {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl AudioClip {
    pub fn duration_secs(&self) -> f32 {
        self.samples.len() as f32 / self.sample_rate as f32
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transcript {
    pub text: String,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_serialize_as_plain_strings() {
        assert_eq!(
            serde_json::to_string(&ProviderId("ollama".into())).unwrap(),
            "\"ollama\""
        );
        assert_eq!(serde_json::to_string(&HistoryId(7)).unwrap(), "7");
    }

    #[test]
    fn step_name_uses_opencc_spelling() {
        assert_eq!(
            serde_json::to_string(&StepName::OpenCc).unwrap(),
            "\"opencc\""
        );
        let back: StepName = serde_json::from_str("\"opencc\"").unwrap();
        assert_eq!(back, StepName::OpenCc);
    }

    #[test]
    fn a_code_name_is_its_wire_form() {
        for code in [
            ErrorCode::ProviderUnreachable,
            ErrorCode::ProviderTimeout,
            ErrorCode::NoSpeech,
            ErrorCode::ModelMissing,
            ErrorCode::InferenceFailed,
        ] {
            assert_eq!(serde_json::to_value(code).unwrap(), code.name());
        }
    }

    #[test]
    fn clip_duration() {
        let clip = AudioClip {
            samples: vec![0.0; 8_000],
            sample_rate: 16_000,
        };
        assert!((clip.duration_secs() - 0.5).abs() < 1e-6);
    }
}
