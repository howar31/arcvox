//! Refines the transcript with the layered prompt. Empty output counts as
//! failure so the pipeline degrades to the ASR text instead of inserting nothing.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::dictionary::llm_terms;
use crate::prompt::{assemble, PromptParts};
use crate::providers::{LlmOptions, LlmProvider};
use crate::steps::{PipelineContext, Step, StepError, StepInput, StepOutput};
use crate::types::{ErrorCode, StepName};

pub struct LlmStep {
    provider: Arc<dyn LlmProvider>,
}

impl LlmStep {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl Step for LlmStep {
    fn name(&self) -> StepName {
        StepName::Llm
    }

    async fn run(&self, input: StepInput, ctx: &PipelineContext) -> Result<StepOutput, StepError> {
        let terms = llm_terms(&ctx.dictionary);
        let messages = assemble(&PromptParts {
            base: &ctx.base_prompt,
            profile_addition: &ctx.profile.prompt_addition,
            dictionary_terms: &terms,
            context: &ctx.context,
            few_shot: &ctx.profile.few_shot,
            utterance: &input.text,
            context_limit_bytes: ctx.general.context_limit_bytes,
        });
        let opts = LlmOptions {
            temperature: ctx.general.llm_temperature,
            max_tokens: ctx.general.llm_max_tokens,
            timeout: Duration::from_secs(ctx.general.llm_timeout_secs),
        };
        let text = self.provider.complete(messages, &opts).await?;
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err(StepError::recoverable(
                ErrorCode::ProviderError,
                "llm returned empty output",
            ));
        }
        Ok(StepOutput {
            text,
            language: None,
        })
    }
}

#[cfg(test)]
pub(crate) mod fakes {
    use std::sync::Mutex;

    use super::*;
    use crate::providers::ProviderError;
    use crate::types::ChatMessage;

    pub struct FakeLlm {
        pub result: Result<String, ProviderError>,
        pub calls: Mutex<Vec<Vec<ChatMessage>>>,
    }

    impl FakeLlm {
        pub fn ok(text: &str) -> Arc<Self> {
            Arc::new(Self {
                result: Ok(text.into()),
                calls: Mutex::new(Vec::new()),
            })
        }
        pub fn failing(code: ErrorCode) -> Arc<Self> {
            Arc::new(Self {
                result: Err(ProviderError::new(code, "boom")),
                calls: Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait]
    impl LlmProvider for FakeLlm {
        async fn complete(
            &self,
            messages: Vec<ChatMessage>,
            _opts: &LlmOptions,
        ) -> Result<String, ProviderError> {
            self.calls.lock().unwrap().push(messages);
            self.result.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fakes::FakeLlm;
    use super::*;
    use crate::steps::test_context;

    #[tokio::test]
    async fn sends_layered_prompt_and_trims_output() {
        let fake = FakeLlm::ok("  整理後\n");
        let out = LlmStep::new(fake.clone())
            .run(
                StepInput {
                    text: "原文".into(),
                    audio: None,
                },
                &test_context(),
            )
            .await
            .unwrap();
        assert_eq!(out.text, "整理後");
        let calls = fake.calls.lock().unwrap();
        assert!(calls[0][0].content.starts_with("你是口述文字整理員"));
        assert_eq!(calls[0].last().unwrap().content, "原文");
    }

    #[tokio::test]
    async fn empty_output_is_recoverable_error() {
        let fake = FakeLlm::ok("   ");
        let err = LlmStep::new(fake)
            .run(
                StepInput {
                    text: "原文".into(),
                    audio: None,
                },
                &test_context(),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ProviderError);
        assert!(err.recoverable);
    }

    #[tokio::test]
    async fn provider_failure_is_recoverable() {
        let fake = FakeLlm::failing(ErrorCode::ProviderTimeout);
        let err = LlmStep::new(fake)
            .run(
                StepInput {
                    text: "原文".into(),
                    audio: None,
                },
                &test_context(),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ProviderTimeout);
        assert!(err.recoverable);
    }
}
