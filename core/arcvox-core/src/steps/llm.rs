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

/// How much longer than the input the output may be. A rewrite is about as
/// long as the transcript; the room above it covers punctuation and a
/// tokenizer that splits the output finer than the estimate.
const OUTPUT_TO_INPUT_TOKENS: u32 = 2;

/// Whether a character is one the tokenizer tends to keep whole: Chinese,
/// Japanese and Korean script and their punctuation.
fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF
    )
}

/// A rough token count of a transcript: a CJK character is about one
/// token, other text about one token per three characters.
pub(crate) fn estimated_tokens(text: &str) -> u32 {
    let (cjk, other) = text
        .chars()
        .fold((0u32, 0u32), |(cjk, other), c| match is_cjk(c) {
            true => (cjk + 1, other),
            false => (cjk, other + 1),
        });
    cjk + other.div_ceil(3)
}

/// The output limit for a rewrite of `input`: the configured value is the
/// floor, and a long transcript raises it so the rewrite is not cut short.
/// `None` leaves the length to the model.
fn output_tokens(configured: Option<u32>, input: &str) -> Option<u32> {
    configured.map(|floor| floor.max(estimated_tokens(input) * OUTPUT_TO_INPUT_TOKENS))
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
        // The profile decides what besides the utterance the model is told.
        let terms = if ctx.profile.llm_dictionary_terms {
            llm_terms(&ctx.dictionary)
        } else {
            Vec::new()
        };
        let mut context = ctx.context.clone();
        if !ctx.profile.llm_window_title {
            context.window_title = None;
        }
        let messages = assemble(&PromptParts {
            base: &ctx.base_prompt,
            profile_addition: &ctx.profile.prompt_addition,
            dictionary_terms: &terms,
            context: &context,
            few_shot: &ctx.profile.few_shot,
            utterance: &input.text,
            context_limit_bytes: ctx.general.context_limit_bytes,
        });
        // What the model was given, for the stage record.
        let prompt = serde_json::json!({
            "base_chars": ctx.base_prompt.trim().chars().count(),
            "addition_chars": ctx.profile.prompt_addition.trim().chars().count(),
            "terms": terms.len(),
            "few_shot": ctx.profile.few_shot.len(),
            "window_title": context.window_title,
            "app_name": context.app_name,
        });
        let opts = LlmOptions {
            temperature: ctx.general.llm_temperature,
            max_tokens: output_tokens(ctx.general.llm_max_tokens, &input.text),
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
        if ctx.profile.llm_short_fallback
            && input_chars > MIN_INPUT_CHARS_FOR_RATIO_GUARD
            && (output_chars as f64) < (input_chars as f64) * MIN_OUTPUT_RATIO
        {
            return Err(StepError::recoverable(
                ErrorCode::ProviderError,
                "llm output rejected as a reply rather than a rewrite",
            ));
        }
        if copies_window_title(&input.text, &text, &context) {
            return Err(StepError::recoverable(
                ErrorCode::ProviderError,
                "llm output rejected: it contains the window title",
            ));
        }
        Ok(StepOutput {
            detail: serde_json::json!({
                "in_chars": input_chars,
                "out_chars": output_chars,
                "prompt": prompt,
            }),
            text,
            language: None,
            ..Default::default()
        })
    }
}

/// Placeholder step used when a step's language model provider could not
/// be built at pipeline-build time (for example a missing API key). It
/// always fails with the recorded error, so the pipeline carries the text
/// on at run time instead of aborting the whole session during
/// `Pipeline::build`.
pub(crate) struct UnavailableStep {
    pub(crate) name: StepName,
    pub(crate) error: StepError,
}

#[async_trait]
impl Step for UnavailableStep {
    fn name(&self) -> StepName {
        self.name
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
        /// The output limit each call asked for.
        pub limits: Mutex<Vec<Option<u32>>>,
        pub loads: std::sync::atomic::AtomicUsize,
        pub unloads: std::sync::atomic::AtomicUsize,
        /// When set, `complete` sleeps this long before answering.
        pub delay: Option<Duration>,
    }

    impl FakeLlm {
        pub fn ok(text: &str) -> Arc<Self> {
            Arc::new(Self {
                result: Ok(text.into()),
                calls: Mutex::new(Vec::new()),
                limits: Mutex::new(Vec::new()),
                loads: std::sync::atomic::AtomicUsize::new(0),
                unloads: std::sync::atomic::AtomicUsize::new(0),
                delay: None,
            })
        }
        pub fn slow(text: &str, delay: Duration) -> Arc<Self> {
            Arc::new(Self {
                result: Ok(text.into()),
                calls: Mutex::new(Vec::new()),
                limits: Mutex::new(Vec::new()),
                loads: std::sync::atomic::AtomicUsize::new(0),
                unloads: std::sync::atomic::AtomicUsize::new(0),
                delay: Some(delay),
            })
        }
        pub fn failing(code: ErrorCode) -> Arc<Self> {
            Arc::new(Self {
                result: Err(ProviderError::new(code, "boom")),
                calls: Mutex::new(Vec::new()),
                limits: Mutex::new(Vec::new()),
                loads: std::sync::atomic::AtomicUsize::new(0),
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
            opts: &LlmOptions,
        ) -> Result<String, ProviderError> {
            self.calls.lock().unwrap().push(messages);
            self.limits.lock().unwrap().push(opts.max_tokens);
            if let Some(d) = self.delay {
                tokio::time::sleep(d).await;
            }
            self.result.clone()
        }
        async fn load(&self) -> Result<(), ProviderError> {
            self.loads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
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

    #[test]
    fn tokens_are_estimated_by_script() {
        assert_eq!(estimated_tokens(""), 0);
        assert_eq!(estimated_tokens("今天天氣很好"), 6);
        assert_eq!(estimated_tokens("the weather is fine"), 7);
        assert_eq!(estimated_tokens("今天 weather 很好"), 4 + 3);
    }

    #[test]
    fn a_short_input_keeps_the_configured_limit() {
        assert_eq!(output_tokens(Some(1024), "今天天氣很好"), Some(1024));
        assert_eq!(output_tokens(None, "今天天氣很好"), None);
    }

    #[test]
    fn a_long_input_widens_the_limit() {
        let long = "好".repeat(900);
        assert_eq!(output_tokens(Some(1024), &long), Some(1800));
        assert_eq!(output_tokens(Some(2000), &long), Some(2000));
    }

    #[tokio::test]
    async fn the_limit_sent_follows_the_input_length() {
        let fake = FakeLlm::ok("整理後");
        let step = LlmStep::new(fake.clone());
        let ctx = test_context();
        for text in ["原文", &"好".repeat(900)] {
            // The fake's short answer trips the ratio guard on the long
            // input; only the limit asked for matters here.
            let _ = step
                .run(
                    StepInput {
                        text: text.into(),
                        audio: None,
                    },
                    &ctx,
                )
                .await;
        }
        assert_eq!(
            *fake.limits.lock().unwrap(),
            vec![ctx.general.llm_max_tokens, Some(1800)]
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

    fn with_profile(ctx: &mut PipelineContext, edit: impl FnOnce(&mut crate::config::Profile)) {
        let mut profile = (*ctx.profile).clone();
        edit(&mut profile);
        ctx.profile = Arc::new(profile);
    }

    #[tokio::test]
    async fn short_output_is_accepted_when_the_guard_is_off() {
        let input: String = "測".repeat(MIN_INPUT_CHARS_FOR_RATIO_GUARD + 1);
        let mut ctx = test_context();
        with_profile(&mut ctx, |p| p.llm_short_fallback = false);
        let out = LlmStep::new(FakeLlm::ok("好"))
            .run(
                StepInput {
                    text: input,
                    audio: None,
                },
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(out.text, "好");
    }

    #[tokio::test]
    async fn terms_are_left_out_when_off() {
        let fake = FakeLlm::ok("整理後");
        let mut ctx = test_context();
        ctx.dictionary = Arc::new(vec![crate::config::DictionaryEntry {
            term: "Arcvox".into(),
            aliases: vec![],
            asr_hint: true,
            ..Default::default()
        }]);
        with_profile(&mut ctx, |p| p.llm_dictionary_terms = false);
        let out = LlmStep::new(fake.clone())
            .run(
                StepInput {
                    text: "原文".into(),
                    audio: None,
                },
                &ctx,
            )
            .await
            .unwrap();
        let calls = fake.calls.lock().unwrap();
        assert!(
            !calls[0][0].content.contains("Arcvox"),
            "{}",
            calls[0][0].content
        );
        assert_eq!(out.detail["prompt"]["terms"], 0);
    }

    #[tokio::test]
    async fn window_title_is_withheld_when_off() {
        let fake = FakeLlm::ok("週報 社群圖片下載工具 整理後");
        let mut ctx = titled("週報 社群圖片下載工具");
        with_profile(&mut ctx, |p| p.llm_window_title = false);
        // The title is neither in the prompt nor used to refuse the output.
        let out = LlmStep::new(fake.clone())
            .run(
                StepInput {
                    text: "原文".into(),
                    audio: None,
                },
                &ctx,
            )
            .await
            .unwrap();
        let calls = fake.calls.lock().unwrap();
        assert!(
            !calls[0][0].content.contains("週報"),
            "{}",
            calls[0][0].content
        );
        assert_eq!(
            out.detail["prompt"]["window_title"],
            serde_json::Value::Null
        );
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

    async fn guard_verdict(input: &str, output: &str) -> Result<StepOutput, StepError> {
        LlmStep::new(FakeLlm::ok(output))
            .run(
                StepInput {
                    text: input.into(),
                    audio: None,
                },
                &test_context(),
            )
            .await
    }

    #[tokio::test]
    async fn output_at_exactly_one_third_is_accepted() {
        // 15 input chars, 5 output chars: the ratio is met, not undercut.
        let out = guard_verdict("一二三四五六七八九十甲乙丙丁戊", "一二三四五")
            .await
            .unwrap();
        assert_eq!(out.text, "一二三四五");
        let err = guard_verdict("一二三四五六七八九十甲乙丙丁戊", "一二三四")
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ProviderError);
    }

    #[tokio::test]
    async fn filler_heavy_inputs_are_rejected_by_the_ratio_guard() {
        // The guard cannot tell a collapsed filler run from a chat reply;
        // these degrade to the raw transcript by design. Pinned so the
        // threshold can be tuned against them.
        for (input, output) in [
            ("那個那個那個那個那個幫我關燈", "幫我關燈"),
            ("uh uh uh uh uh uh uh uh yes", "Yes."),
        ] {
            let err = guard_verdict(input, output).await.unwrap_err();
            assert_eq!(err.code, ErrorCode::ProviderError, "{input}");
            assert!(err.recoverable, "{input}");
        }
    }

    #[tokio::test]
    async fn an_english_rewrite_within_the_ratio_is_accepted() {
        for (input, output) in [
            (
                "um so I think we should uh meet tomorrow at ten no at eleven",
                "I think we should meet tomorrow at eleven.",
            ),
            // A filler run with a long enough tail stays above the ratio.
            (
                "okay so so so so so so so so turn off the light",
                "Turn off the light.",
            ),
        ] {
            let out = guard_verdict(input, output).await.unwrap();
            assert_eq!(out.text, output);
        }
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
        let short = titled("Arcvox");
        assert!(!copies_window_title(
            "語音很好用",
            "Arcvox 很好用。",
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
