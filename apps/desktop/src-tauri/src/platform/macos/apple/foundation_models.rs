//! `LlmProvider` over Apple's on-device Foundation Models.

use std::ffi::{c_char, CStr, CString};

use arcvox_core::providers::{LlmOptions, LlmProvider, ProviderError};
use arcvox_core::types::{ChatMessage, ErrorCode, Role};
use async_trait::async_trait;
use tokio::time::Instant;

use super::ffi;

pub struct AppleLlmProvider {
    label: String,
}

impl AppleLlmProvider {
    pub fn new(label: String) -> Self {
        Self { label }
    }
}

/// Splits a chat into the session instructions and the prompt: system
/// messages and few-shot pairs go into the instructions, the last user
/// message is the prompt.
pub fn flatten_for_fm(messages: &[ChatMessage]) -> (String, String) {
    flatten(messages, true)
}

fn flatten(messages: &[ChatMessage], with_examples: bool) -> (String, String) {
    let last_user = messages.iter().rposition(|m| m.role == Role::User);
    let prompt = last_user
        .map(|i| messages[i].content.clone())
        .unwrap_or_default();
    let mut parts: Vec<String> = Vec::new();
    let mut input: Option<&str> = None;
    for (i, m) in messages.iter().enumerate() {
        if Some(i) == last_user {
            continue;
        }
        match m.role {
            Role::System => parts.push(m.content.clone()),
            Role::User => input = Some(&m.content),
            Role::Assistant => {
                if let Some(raw) = input.take() {
                    if with_examples {
                        parts.push(format!(
                            "Example input:\n{raw}\nExample output:\n{}",
                            m.content
                        ));
                    }
                }
            }
        }
    }
    (parts.join("\n\n"), prompt)
}

/// Maps a Foundation Models code from the bridge.
pub fn map_fm_code(code: i32, label: &str) -> ProviderError {
    let unavailable = |msg: &str, hint: &str| {
        ProviderError::new(ErrorCode::NativeUnavailable, format!("{label}: {msg}")).with_hint(hint)
    };
    match code {
        1 => unavailable(
            "Apple Intelligence is not available",
            "this Mac cannot run Apple Intelligence",
        ),
        2 => unavailable(
            "Apple Intelligence is off",
            "turn on Apple Intelligence in System Settings",
        ),
        3 => unavailable(
            "the Apple model is not ready",
            "the Apple model is still downloading",
        ),
        4 => unavailable("Apple Intelligence is not available", "requires macOS 26"),
        5 => ProviderError::new(
            ErrorCode::InferenceFailed,
            format!("{label}: the text was declined"),
        )
        .with_hint("Apple's guardrail declined this text"),
        6 => ProviderError::new(
            ErrorCode::InferenceFailed,
            format!("{label}: the request does not fit the model"),
        )
        .with_hint("prompt too long"),
        7 => unavailable(
            "language not supported",
            "language not supported by the Apple model",
        ),
        c => ProviderError::new(
            ErrorCode::InferenceFailed,
            format!("{label}: generation failed (code {c})"),
        ),
    }
}

fn run(instructions: &str, prompt: &str, max_tokens: i32, temperature: f64) -> Result<String, i32> {
    let ins = CString::new(instructions.replace('\0', "")).expect("no interior NUL");
    let pr = CString::new(prompt.replace('\0', "")).expect("no interior NUL");
    let mut out: *mut c_char = std::ptr::null_mut();
    // SAFETY: both strings and `out` outlive the call; the bridge copies the inputs.
    let code = unsafe {
        ffi::kw_apple_fm_complete(ins.as_ptr(), pr.as_ptr(), max_tokens, temperature, &mut out)
    };
    let text = if out.is_null() {
        None
    } else {
        // SAFETY: a non-null `out` is a NUL-terminated string from the bridge,
        // freed right after the copy.
        let s = unsafe { CStr::from_ptr(out) }
            .to_string_lossy()
            .into_owned();
        unsafe { ffi::kw_apple_free(out) };
        Some(s)
    };
    match (code, text) {
        (0, Some(t)) => Ok(t),
        (0, None) => Err(9),
        (c, _) => Err(c),
    }
}

impl AppleLlmProvider {
    /// One bridge call, bounded by `deadline`. The outer error is a timeout or
    /// a lost worker; the inner one is a bridge code.
    async fn attempt(
        &self,
        instructions: String,
        prompt: String,
        max_tokens: i32,
        temperature: f64,
        deadline: Instant,
    ) -> Result<Result<String, i32>, ProviderError> {
        // The bridge blocks its thread, so it must never run on the main thread.
        let task = tokio::task::spawn_blocking(move || {
            run(&instructions, &prompt, max_tokens, temperature)
        });
        match tokio::time::timeout_at(deadline, task).await {
            Ok(Ok(r)) => Ok(r),
            Ok(Err(join)) => Err(ProviderError::new(
                ErrorCode::InferenceFailed,
                format!("{}: {join}", self.label),
            )),
            Err(_) => Err(ProviderError::new(
                ErrorCode::ProviderTimeout,
                format!("{}: generation timed out", self.label),
            )
            .with_hint("raise the rewrite timeout in Settings > General")),
        }
    }
}

#[async_trait]
impl LlmProvider for AppleLlmProvider {
    async fn complete(
        &self,
        messages: Vec<ChatMessage>,
        opts: &LlmOptions,
    ) -> Result<String, ProviderError> {
        let deadline = Instant::now() + opts.timeout;
        // No token cap: it would truncate rewrites silently, while the context window
        // bounds the reply and `exceededContextWindowSize` is already mapped.
        let max_tokens = 0;
        let temperature = f64::from(opts.temperature);
        let (instructions, prompt) = flatten(&messages, true);
        let mut result = self
            .attempt(
                instructions,
                prompt.clone(),
                max_tokens,
                temperature,
                deadline,
            )
            .await?;
        if result == Err(6) {
            // The few-shot examples are the part that can be dropped.
            let (instructions, _) = flatten(&messages, false);
            result = self
                .attempt(instructions, prompt, max_tokens, temperature, deadline)
                .await?;
        }
        result
            .map(|t| t.trim().to_string())
            .map_err(|c| map_fm_code(c, &self.label))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_puts_examples_in_instructions_and_last_user_in_prompt() {
        let m = vec![
            ChatMessage::system("Rules."),
            ChatMessage::user("raw1"),
            ChatMessage::assistant("out1"),
            ChatMessage::user("<<< now >>>"),
        ];
        let (ins, prompt) = flatten_for_fm(&m);
        assert!(ins.starts_with("Rules."));
        assert!(ins.contains("Example input:\nraw1\nExample output:\nout1"));
        assert_eq!(prompt, "<<< now >>>");
    }

    #[test]
    fn guardrail_is_recoverable_provider_error() {
        let err = map_fm_code(5, "Apple");
        assert_eq!(err.code, ErrorCode::InferenceFailed);
        assert!(err.hint.as_deref().unwrap().contains("guardrail"));
        assert_eq!(map_fm_code(4, "Apple").code, ErrorCode::NativeUnavailable);
        assert_eq!(map_fm_code(2, "Apple").code, ErrorCode::NativeUnavailable);
    }

    #[test]
    fn retry_drops_examples_but_keeps_rules() {
        let m = vec![
            ChatMessage::system("Rules."),
            ChatMessage::user("raw1"),
            ChatMessage::assistant("out1"),
            ChatMessage::user("now"),
        ];
        let (ins, prompt) = flatten(&m, false);
        assert_eq!(ins, "Rules.");
        assert_eq!(prompt, "now");
    }
}
