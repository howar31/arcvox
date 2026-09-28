//! Provider traits and shared error type. Three families exist (Http,
//! Embedded, Native); phase 1 ships Http only.

use std::time::Duration;

use async_trait::async_trait;

use crate::types::{AudioClip, ChatMessage, ErrorCode, Transcript};

pub mod http_common;
pub mod http_llm;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct ProviderError {
    pub code: ErrorCode,
    pub message: String,
    pub hint: Option<String>,
}

impl ProviderError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: None,
        }
    }
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AsrHint {
    /// BCP-47-ish tag the provider understands, for example `zh` or `zh-TW`.
    pub language: Option<String>,
    /// Hotword prompt (Whisper `prompt` / `initial_prompt`).
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LlmOptions {
    pub temperature: f32,
    pub max_tokens: Option<u32>,
    pub timeout: Duration,
}

#[async_trait]
pub trait AsrProvider: Send + Sync {
    async fn transcribe(
        &self,
        audio: &AudioClip,
        hint: &AsrHint,
    ) -> Result<Transcript, ProviderError>;

    /// Embedded providers load model files here. HTTP providers do nothing.
    async fn load(&self) -> Result<(), ProviderError> {
        Ok(())
    }
    async fn unload(&self) {}
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(
        &self,
        messages: Vec<ChatMessage>,
        opts: &LlmOptions,
    ) -> Result<String, ProviderError>;

    async fn load(&self) -> Result<(), ProviderError> {
        Ok(())
    }
    async fn unload(&self) {}
}
