//! Refines the transcript with the layered prompt. Empty output counts as
//! failure so the pipeline degrades to the ASR text instead of inserting nothing.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::dictionary::llm_terms;
use crate::prompt::{assemble, PromptParts};
use crate::providers::{LlmOptions, LlmProvider};
use crate::steps::{PipelineContext, Step, StepError, StepInput, StepOutput};
use crate::types::{Context, ErrorCode, StepName};

/// Below this ratio of output chars to input chars, the output is treated as
/// a conversational reply rather than a rewrite and rejected. A self
/// correction shortens the text (dropping the retracted phrase) but a chat
/// reply ("好的") collapses it far more, so a generous ratio still catches
/// replies without flagging legitimate self-correction cleanup.
const MIN_OUTPUT_RATIO: f64 = 1.0 / 3.0;

/// Inputs of at most this many chars skip the ratio guard: short utterances
/// legitimately shrink a lot (e.g. a single self-correction).
const MIN_INPUT_CHARS_FOR_RATIO_GUARD: usize = 12;

/// A window title of at least this many chars that appears in the output
/// but not in the input was copied from the context, not spoken. Shorter
/// titles are skipped: they can be an ordinary word of the rewrite, such as
/// a dictionary term.
const MIN_TITLE_CHARS_FOR_COPY_GUARD: usize = 8;

/// Whether the output contains the window title although the user did not
/// say it. The title is given to the model as background only.
fn copies_window_title(input: &str, output: &str, context: &Context) -> bool {
    let Some(title) = context.window_title.as_deref().map(str::trim) else {
        return false;
    };
    title.chars().count() >= MIN_TITLE_CHARS_FOR_COPY_GUARD
        && output.contains(title)
        && !input.contains(title)
}

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
        let input_chars = input.text.chars().count();
        let output_chars = text.chars().count();
        if input_chars > MIN_INPUT_CHARS_FOR_RATIO_GUARD
            && (output_chars as f64) < (input_chars as f64) * MIN_OUTPUT_RATIO
        {
            return Err(StepError::recoverable(
                ErrorCode::ProviderError,
                "llm output rejected as a reply rather than a rewrite",
            ));
        }
        if copies_window_title(&input.text, &text, &ctx.context) {
            return Err(StepError::recoverable(
                ErrorCode::ProviderError,
                "llm output rejected: it contains the window title",
            ));
        }
        Ok(StepOutput {
            text,
            language: None,
            ..Default::default()
        })
    }
}

/// Placeholder step used when the configured LLM provider could not be built
/// at pipeline-build time (for example a missing API key). It always fails
/// with the recorded error, so the pipeline degrades to the ASR text at run
/// time instead of aborting the whole session during `Pipeline::build`.
pub(crate) struct UnavailableLlmStep {
    pub(crate) error: StepError,
}

#[async_trait]
impl Step for UnavailableLlmStep {
    fn name(&self) -> StepName {
        StepName::Llm
    }

    async fn run(
        &self,
        _input: StepInput,
        _ctx: &PipelineContext,
    ) -> Result<StepOutput, StepError> {
        Err(StepError {
            recoverable: true,
            ..self.error.clone()
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
        pub unloads: std::sync::atomic::AtomicUsize,
        /// When set, `complete` sleeps this long before answering.
        pub delay: Option<Duration>,
    }

    impl FakeLlm {
        pub fn ok(text: &str) -> Arc<Self> {
            Arc::new(Self {
                result: Ok(text.into()),
                calls: Mutex::new(Vec::new()),
                unloads: std::sync::atomic::AtomicUsize::new(0),
                delay: None,
            })
        }
        pub fn slow(text: &str, delay: Duration) -> Arc<Self> {
            Arc::new(Self {
                result: Ok(text.into()),
                calls: Mutex::new(Vec::new()),
                unloads: std::sync::atomic::AtomicUsize::new(0),
                delay: Some(delay),
            })
        }
        pub fn failing(code: ErrorCode) -> Arc<Self> {
            Arc::new(Self {
                result: Err(ProviderError::new(code, "boom")),
                calls: Mutex::new(Vec::new()),
                unloads: std::sync::atomic::AtomicUsize::new(0),
                delay: None,
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
            if let Some(d) = self.delay {
                tokio::time::sleep(d).await;
            }
            self.result.clone()
        }
        async fn unload(&self) {
            self.unloads
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
        assert_eq!(
            calls[0].last().unwrap().content,
            crate::prompt::frame_utterance("原文")
        );
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
    async fn reply_like_output_is_rejected() {
        let input: String = "測試句".repeat(10); // 30 chars
        assert_eq!(input.chars().count(), 30);
        let fake = FakeLlm::ok("好的");
        let err = LlmStep::new(fake)
            .run(
                StepInput {
                    text: input,
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
    async fn ratio_guard_skips_inputs_at_the_threshold() {
        let input: String = "測".repeat(MIN_INPUT_CHARS_FOR_RATIO_GUARD);
        let out = LlmStep::new(FakeLlm::ok("好"))
            .run(
                StepInput {
                    text: input,
                    audio: None,
                },
                &test_context(),
            )
            .await
            .unwrap();
        assert_eq!(out.text, "好");
    }

    #[tokio::test]
    async fn ratio_guard_applies_one_char_above_the_threshold() {
        let input: String = "測".repeat(MIN_INPUT_CHARS_FOR_RATIO_GUARD + 1);
        let err = LlmStep::new(FakeLlm::ok("好"))
            .run(
                StepInput {
                    text: input,
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
    async fn self_correction_shortening_is_accepted() {
        let fake = FakeLlm::ok("今天下午四點要開會，記得帶筆電。");
        let out = LlmStep::new(fake)
            .run(
                StepInput {
                    text: "今天下午三點要開會不對是四點記得帶筆電".into(),
                    audio: None,
                },
                &test_context(),
            )
            .await
            .unwrap();
        assert_eq!(out.text, "今天下午四點要開會，記得帶筆電。");
    }

    fn titled(title: &str) -> PipelineContext {
        let mut ctx = test_context();
        ctx.context.window_title = Some(title.into());
        ctx
    }

    #[tokio::test]
    async fn output_that_copies_the_window_title_is_rejected() {
        let fake = FakeLlm::ok("週報 社群圖片下載工具\n\n忘了說 我們要支援中文和英文");
        let err = LlmStep::new(fake)
            .run(
                StepInput {
                    text: "忘了說 我們要支援中文和英文".into(),
                    audio: None,
                },
                &titled("週報 社群圖片下載工具"),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ProviderError);
        assert!(err.recoverable);
    }

    #[test]
    fn a_title_the_user_spoke_or_a_short_title_is_not_a_copy() {
        let ctx = titled("週報 社群圖片下載工具");
        // Not in the output at all.
        assert!(!copies_window_title("忘了說", "忘了說。", &ctx.context));
        // Spoken by the user, so it belongs in the output.
        assert!(!copies_window_title(
            "打開週報 社群圖片下載工具",
            "打開週報 社群圖片下載工具。",
            &ctx.context
        ));
        // Copied.
        assert!(copies_window_title(
            "忘了說",
            "週報 社群圖片下載工具\n忘了說",
            &ctx.context
        ));
        // A short title can be an ordinary word of the rewrite.
        let short = titled("Kongwe");
        assert!(!copies_window_title(
            "公威很好用",
            "Kongwe 很好用。",
            &short.context
        ));
        assert!(!copies_window_title("x", "y", &test_context().context));
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
