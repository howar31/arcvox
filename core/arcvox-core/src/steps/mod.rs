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
pub mod translate;

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

/// A step's stage record with the service's use added, when it reported any.
pub(crate) fn with_usage(
    mut detail: serde_json::Value,
    usage: Option<serde_json::Value>,
) -> serde_json::Value {
    if let Some(usage) = usage {
        detail["usage"] = usage;
    }
    detail
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct StepError {
    pub code: ErrorCode,
    pub message: String,
    /// `true` means the pipeline may skip this step and continue degraded.
    pub recoverable: bool,
    pub hint: Option<String>,
    /// The HTTP status the service answered with.
    pub status: Option<u16>,
    /// The service's own words.
    pub answer: Option<String>,
    /// The service's answer as it came.
    pub raw: Option<String>,
}

impl StepError {
    pub fn recoverable(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            recoverable: true,
            hint: None,
            status: None,
            answer: None,
            raw: None,
        }
    }
    pub fn fatal(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            recoverable: false,
            ..Self::recoverable(code, message)
        }
    }
    /// The stage record of the step that failed.
    pub fn stage_detail(&self) -> serde_json::Value {
        let mut detail = serde_json::json!({
            "error": self.code.name(),
            "message": self.message,
        });
        if let Some(status) = self.status {
            detail["status"] = status.into();
        }
        if let Some(answer) = &self.answer {
            detail["answer"] = answer.as_str().into();
        }
        if let Some(raw) = &self.raw {
            detail["raw"] = raw.as_str().into();
        }
        detail
    }
    /// The line under a failure: the service's own words, else the hint.
    pub fn shown_hint(&self) -> Option<String> {
        match (self.status, &self.answer) {
            (Some(status), Some(answer)) => Some(format!("HTTP {status}: {answer}")),
            (None, Some(answer)) => Some(answer.clone()),
            _ => self.hint.clone(),
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
    /// The translation's rules, from `prompts/translate.md`.
    pub translate_prompt: String,
    pub general: General,
    /// The transcript once the speech recognition step is done, for a run
    /// that is cut short after it.
    pub recognised: Arc<std::sync::Mutex<Option<String>>>,
    /// A sample of writing given to the speech engine as if it were the
    /// transcript before this one, so the engine writes in its style
    /// (script, punctuation, spacing); placed before the dictionary terms.
    pub asr_sample: Option<String>,
    /// How often each dictionary term was replaced, for choosing the hints
    /// when not all of them fit.
    pub term_hits: Arc<Vec<crate::history::TermHits>>,
    /// The speech engine's session that took the audio while it was
    /// recorded; the recognition step finishes it instead of sending the
    /// recording again.
    pub live: Arc<std::sync::Mutex<Option<crate::providers::live::LiveStream>>>,
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
        translate_prompt: crate::config::base_prompt::DEFAULT_TRANSLATE_PROMPT.to_string(),
        general: General::default(),
        recognised: Default::default(),
        asr_sample: None,
        term_hits: Default::default(),
        live: Default::default(),
    }
}
