//! llama.cpp language model provider.
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use async_trait::async_trait;
use arcvox_core::providers::{LlmOptions, LlmProvider, ProviderError};
use arcvox_core::types::{ChatMessage, ErrorCode};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::{send_logs_to_tracing, LogOptions};

use crate::chat::{self, role_name};
use crate::{CancelFlag, CancelOnDrop};

/// Context window used for every completion.
pub const N_CTX: u32 = 4096;
/// Prompt tokens decoded per `llama_decode` call; also the context batch size.
const N_BATCH: usize = 512;
/// Tokens kept free for the chat template's own markup.
const TEMPLATE_MARGIN: usize = 64;
const DEFAULT_MAX_TOKENS: u32 = 1024;
const SEED: u32 = 1234;

/// llama.cpp allows a single backend per process.
fn backend() -> &'static LlamaBackend {
    static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();
    BACKEND.get_or_init(|| {
        // Routes llama.cpp and ggml logs to `tracing` instead of stderr.
        send_logs_to_tracing(LogOptions::default());
        LlamaBackend::init().expect("llama backend initialised once per process")
    })
}

#[derive(Default)]
struct SharedModel {
    slot: Mutex<Option<Arc<LlamaModel>>>,
    /// Held for the whole load so concurrent callers never load the model twice,
    /// while `slot` stays free for `unload`.
    loading: Mutex<()>,
}

pub struct LlamaLlmProvider {
    path: PathBuf,
    label: String,
    use_gpu: bool,
    model: Arc<SharedModel>,
}

impl LlamaLlmProvider {
    pub fn new(path: PathBuf, label: String, use_gpu: bool) -> Self {
        Self {
            path,
            label,
            use_gpu,
            model: Arc::default(),
        }
    }

    /// Loads the model in a blocking task. Not bounded by the LLM timeout: a
    /// first load is not an inference stall.
    async fn model(&self) -> Result<Arc<LlamaModel>, ProviderError> {
        let loader = Loader {
            path: self.path.clone(),
            label: self.label.clone(),
            use_gpu: self.use_gpu,
            model: self.model.clone(),
        };
        match tokio::task::spawn_blocking(move || loader.model()).await {
            Ok(r) => r,
            Err(join) => Err(inference_failed(&self.label, join)),
        }
    }
}

/// Everything needed to load the model from inside a blocking task.
struct Loader {
    path: PathBuf,
    label: String,
    use_gpu: bool,
    model: Arc<SharedModel>,
}

impl Loader {
    /// Returns the loaded model, loading it on first use.
    fn model(&self) -> Result<Arc<LlamaModel>, ProviderError> {
        let _loading = self.model.loading.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(m) = self
            .model
            .slot
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            return Ok(m.clone());
        }
        let m = Arc::new(load_model(&self.path, &self.label, self.use_gpu)?);
        *self.model.slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(m.clone());
        Ok(m)
    }
}

fn load_model(path: &Path, label: &str, use_gpu: bool) -> Result<LlamaModel, ProviderError> {
    if !path.is_file() {
        return Err(ProviderError::new(
            ErrorCode::ModelMissing,
            format!("{label}: model file not found"),
        )
        .with_hint("download or import the model in Settings > Engines and models"));
    }
    let params = LlamaModelParams::default().with_n_gpu_layers(if use_gpu { u32::MAX } else { 0 });
    LlamaModel::load_from_file(backend(), path, &params).map_err(|e| {
        ProviderError::new(
            ErrorCode::ModelCorrupt,
            format!("{label}: cannot load model ({e})"),
        )
        .with_hint("re-download or re-import the model in Settings > Engines and models")
    })
}

fn inference_failed(label: &str, e: impl std::fmt::Display) -> ProviderError {
    ProviderError::new(ErrorCode::InferenceFailed, format!("{label}: {e}"))
}

fn prompt(model: &LlamaModel, messages: &[ChatMessage]) -> String {
    let Ok(tpl) = model.chat_template(None) else {
        return chat::chatml(messages);
    };
    // Contents are NUL-free (see `generate`), so building a message cannot fail.
    let msgs: Vec<LlamaChatMessage> = messages
        .iter()
        .filter_map(|m| LlamaChatMessage::new(role_name(m.role).into(), m.content.clone()).ok())
        .collect();
    model
        .apply_chat_template(&tpl, &msgs, true)
        .unwrap_or_else(|_| chat::chatml(messages))
}

fn generate(
    model: &LlamaModel,
    label: &str,
    messages: Vec<ChatMessage>,
    opts: &LlmOptions,
    deadline: Instant,
    cancel: &CancelFlag,
) -> Result<String, ProviderError> {
    let stop = || -> Result<(), ProviderError> {
        if cancel.is_cancelled() {
            return Err(ProviderError::new(
                ErrorCode::Cancelled,
                format!("{label}: generation cancelled"),
            ));
        }
        if Instant::now() >= deadline {
            return Err(ProviderError::new(
                ErrorCode::ProviderTimeout,
                format!("{label}: generation timed out"),
            )
            .with_hint("raise the rewrite timeout in Settings > General"));
        }
        Ok(())
    };
    let max_tokens = opts.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS) as usize;
    // llama.cpp takes C strings; interior NUL bytes would make tokenizing fail.
    let messages: Vec<ChatMessage> = messages
        .into_iter()
        .map(|mut m| {
            if m.content.contains('\0') {
                m.content = m.content.replace('\0', "");
            }
            m
        })
        .collect();
    let count = |s: &str| {
        model
            .str_to_token(s, AddBos::Never)
            .map(|t| t.len())
            .unwrap_or(s.len() / 2)
    };
    let budget = (N_CTX as usize)
        .saturating_sub(max_tokens)
        .saturating_sub(TEMPLATE_MARGIN);
    let messages = chat::fit_messages(messages, budget, count);
    let prompt = prompt(model, &messages);
    let tokens = model
        .str_to_token(&prompt, AddBos::Always)
        .map_err(|e| inference_failed(label, e))?;
    if tokens.is_empty() || tokens.len() >= N_CTX as usize {
        return Err(inference_failed(
            label,
            format!("prompt of {} tokens does not fit the context", tokens.len()),
        ));
    }

    let threads = std::thread::available_parallelism()
        .map(|n| n.get().min(4) as i32)
        .unwrap_or(4);
    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(N_CTX))
        .with_n_batch(N_BATCH as u32)
        .with_n_threads(threads)
        .with_n_threads_batch(threads);
    let mut ctx = model
        .new_context(backend(), ctx_params)
        .map_err(|e| inference_failed(label, e))?;

    let mut batch = LlamaBatch::new(N_BATCH, 1);
    let last = tokens.len() - 1;
    for (start, chunk) in (0..).step_by(N_BATCH).zip(tokens.chunks(N_BATCH)) {
        stop()?;
        batch.clear();
        for (j, t) in chunk.iter().enumerate() {
            let pos = start + j;
            batch
                .add(*t, pos as i32, &[0], pos == last)
                .map_err(|e| inference_failed(label, e))?;
        }
        ctx.decode(&mut batch)
            .map_err(|e| inference_failed(label, e))?;
    }

    let mut sampler = LlamaSampler::chain_simple([
        LlamaSampler::penalties(model.n_vocab(), 64, 1.1, 0.0, 0.0),
        LlamaSampler::top_p(0.9, 1),
        LlamaSampler::temp(opts.temperature),
        LlamaSampler::dist(SEED),
    ]);
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut out = String::new();
    let end = (tokens.len() + max_tokens).min(N_CTX as usize);
    for pos in tokens.len()..end {
        stop()?;
        // `sample` also accepts the token into the sampler chain.
        let token = sampler.sample(&ctx, batch.n_tokens() - 1);
        if model.is_eog_token(token) {
            return Ok(out);
        }
        let piece = model
            .token_to_piece(token, &mut decoder, true, None)
            .map_err(|e| inference_failed(label, e))?;
        out.push_str(&piece);
        if pos + 1 == end {
            break;
        }
        batch.clear();
        batch
            .add(token, pos as i32, &[0], true)
            .map_err(|e| inference_failed(label, e))?;
        ctx.decode(&mut batch)
            .map_err(|e| inference_failed(label, e))?;
    }
    Err(truncated(label))
}

/// The token budget ran out before an end-of-generation token, so the reply is incomplete.
fn truncated(label: &str) -> ProviderError {
    ProviderError::new(
        ErrorCode::ProviderError,
        format!("{label}: generation hit the token limit before finishing"),
    )
}

#[async_trait]
impl LlmProvider for LlamaLlmProvider {
    async fn complete(
        &self,
        messages: Vec<ChatMessage>,
        opts: &LlmOptions,
    ) -> Result<String, ProviderError> {
        let model = self.model().await?;
        let opts = opts.clone();
        let label = self.label.clone();
        let deadline = Instant::now() + opts.timeout;
        let cancel = CancelFlag::default();
        let _guard = CancelOnDrop(cancel.clone());
        // The task owns its own Arc of the model, so unload() during a run only
        // frees the model once this task has finished with it.
        let task = tokio::task::spawn_blocking(move || {
            generate(&model, &label, messages, &opts, deadline, &cancel)
        });
        task.await
            .map_err(|join| inference_failed(&self.label, join))?
    }

    async fn load(&self) -> Result<(), ProviderError> {
        self.model().await.map(|_| ())
    }

    async fn unload(&self) {
        *self.model.slot.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcvox_core::providers::{LlmOptions, LlmProvider};
    use arcvox_core::types::{ChatMessage, ErrorCode};
    use std::path::PathBuf;
    use std::time::Duration;

    #[tokio::test]
    async fn missing_file_is_model_missing() {
        let p = LlamaLlmProvider::new(PathBuf::from("/nonexistent/m.gguf"), "Local".into(), false);
        let err = p
            .complete(
                vec![ChatMessage::user("hi")],
                &LlmOptions {
                    temperature: 0.2,
                    max_tokens: Some(8),
                    timeout: Duration::from_secs(5),
                },
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ModelMissing);
    }

    #[test]
    fn truncation_is_a_recoverable_provider_error() {
        let err = truncated("Local");
        assert_eq!(err.code, ErrorCode::ProviderError);
        assert!(err.message.contains("token limit"));
    }
}
