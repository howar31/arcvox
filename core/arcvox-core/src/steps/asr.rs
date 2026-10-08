//! Consumes the buffered audio and produces the raw transcript.

use std::sync::Arc;

use async_trait::async_trait;

use crate::dictionary::asr_hint;
use crate::providers::{AsrHint, AsrProvider, ProviderError};
use crate::steps::{PipelineContext, Step, StepError, StepInput, StepOutput};
use crate::types::{ErrorCode, StepName};

/// Byte budget for the hotword prompt sent to Whisper-style servers.
pub const ASR_HINT_BUDGET_BYTES: usize = 600;

pub struct AsrStep {
    provider: Arc<dyn AsrProvider>,
}

impl AsrStep {
    pub fn new(provider: Arc<dyn AsrProvider>) -> Self {
        Self { provider }
    }
}

impl From<ProviderError> for StepError {
    fn from(e: ProviderError) -> Self {
        StepError {
            code: e.code,
            message: e.message,
            recoverable: true,
            hint: e.hint,
        }
    }
}

#[async_trait]
impl Step for AsrStep {
    fn name(&self) -> StepName {
        StepName::Asr
    }

    async fn run(&self, input: StepInput, ctx: &PipelineContext) -> Result<StepOutput, StepError> {
        let audio = input.audio.ok_or_else(|| {
            StepError::fatal(ErrorCode::Internal, "asr step called without audio")
        })?;
        let hint = AsrHint {
            language: ctx.profile.language_hint.clone(),
            prompt: asr_hint(&ctx.dictionary, ASR_HINT_BUDGET_BYTES),
        };
        let t = self
            .provider
            .transcribe(&audio, &hint)
            .await
            .map_err(|e| StepError {
                recoverable: false,
                ..StepError::from(e)
            })?;
        Ok(StepOutput {
            text: t.text,
            language: t.language,
            ..Default::default()
        })
    }
}

#[cfg(test)]
pub(crate) mod fakes {
    use std::sync::Mutex;

    use super::*;
    use crate::types::{AudioClip, Transcript};

    pub struct FakeAsr {
        pub result: Result<Transcript, ProviderError>,
        pub hints: Mutex<Vec<AsrHint>>,
    }

    impl FakeAsr {
        pub fn ok(text: &str) -> Arc<Self> {
            Arc::new(Self {
                result: Ok(Transcript {
                    text: text.into(),
                    language: Some("zh".into()),
                }),
                hints: Mutex::new(Vec::new()),
            })
        }
        pub fn failing(code: ErrorCode) -> Arc<Self> {
            Arc::new(Self {
                result: Err(ProviderError::new(code, "boom")),
                hints: Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait]
    impl AsrProvider for FakeAsr {
        async fn transcribe(
            &self,
            _audio: &AudioClip,
            hint: &AsrHint,
        ) -> Result<Transcript, ProviderError> {
            self.hints.lock().unwrap().push(hint.clone());
            self.result.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fakes::FakeAsr;
    use super::*;
    use crate::config::DictionaryEntry;
    use crate::steps::test_context;
    use crate::types::AudioClip;

    fn audio() -> Arc<AudioClip> {
        Arc::new(AudioClip {
            samples: vec![0.0; 16_000],
            sample_rate: 16_000,
        })
    }

    #[tokio::test]
    async fn passes_language_and_dictionary_hint() {
        let fake = FakeAsr::ok("原文");
        let mut ctx = test_context();
        ctx.dictionary = Arc::new(vec![DictionaryEntry {
            term: "Arcvox".into(),
            aliases: vec![],
            asr_hint: true,
        }]);
        let out = AsrStep::new(fake.clone())
            .run(
                StepInput {
                    text: String::new(),
                    audio: Some(audio()),
                },
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(out.text, "原文");
        let hints = fake.hints.lock().unwrap();
        assert_eq!(hints[0].language.as_deref(), Some("zh-TW"));
        assert_eq!(hints[0].prompt.as_deref(), Some("Arcvox"));
    }

    #[tokio::test]
    async fn provider_failure_is_fatal() {
        let fake = FakeAsr::failing(ErrorCode::ProviderUnreachable);
        let err = AsrStep::new(fake)
            .run(
                StepInput {
                    text: String::new(),
                    audio: Some(audio()),
                },
                &test_context(),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ProviderUnreachable);
        assert!(!err.recoverable);
    }
}
