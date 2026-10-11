//! Pipeline steps. Each step takes text (and, for ASR, audio) and returns text.

use std::sync::Arc;

use async_trait::async_trait;

use crate::config::{DictionaryEntry, General, Profile};
use crate::dictionary::AliasCounts;
use crate::types::{AudioClip, Context, ErrorCode, StepName};

pub mod asr;
pub mod llm;
pub mod opencc;
pub mod rules;

#[derive(Debug, Clone)]
pub struct StepInput {
    pub text: String,
    /// Present only for the ASR step.
    pub audio: Option<Arc<AudioClip>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StepOutput {
    pub text: String,
    /// Language detected by ASR, when the provider reports one.
    pub language: Option<String>,
    /// Dictionary replacements this step made.
    pub aliases: AliasCounts,
    /// Facts of the step's own for the stage record: an object, or Null.
    pub detail: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct StepError {
    pub code: ErrorCode,
    pub message: String,
    /// `true` means the pipeline may skip this step and continue degraded.
    pub recoverable: bool,
    pub hint: Option<String>,
}

impl StepError {
    pub fn recoverable(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            recoverable: true,
            hint: None,
        }
    }
    pub fn fatal(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            recoverable: false,
            hint: None,
        }
    }
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// Everything a step may read while running. Built once per session.
#[derive(Clone)]
pub struct PipelineContext {
    pub profile: Arc<Profile>,
    pub context: Context,
    pub dictionary: Arc<Vec<DictionaryEntry>>,
    pub base_prompt: String,
    pub general: General,
    /// The transcript once the speech recognition step is done, for a run
    /// that is cut short after it.
    pub recognised: Arc<std::sync::Mutex<Option<String>>>,
}

#[async_trait]
pub trait Step: Send + Sync {
    fn name(&self) -> StepName;
    async fn run(&self, input: StepInput, ctx: &PipelineContext) -> Result<StepOutput, StepError>;
}

#[cfg(test)]
pub(crate) fn test_context() -> PipelineContext {
    let config = crate::config::Config::first_run();
    PipelineContext {
        profile: Arc::new(config.default_profile().clone()),
        context: Context::default(),
        dictionary: Arc::new(Vec::new()),
        base_prompt: crate::config::base_prompt::DEFAULT_BASE_PROMPT.to_string(),
        general: General::default(),
        recognised: Default::default(),
    }
}
