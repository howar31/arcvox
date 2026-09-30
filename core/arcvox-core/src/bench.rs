//! Times one provider at a time, cold (unloaded first) or warm, so a client
//! can compare load and inference cost between providers.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::config::base_prompt::DEFAULT_BASE_PROMPT;
use crate::dictionary::{asr_hint, llm_terms};
use crate::engine::Engine;
use crate::prompt::{assemble, PromptParts};
use crate::providers::{AsrHint, LlmOptions};
use crate::steps::asr::ASR_HINT_BUDGET_BYTES;
use crate::types::{AudioClip, Context, ProviderId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchRole {
    Asr,
    Llm,
}

/// One timed run. `load_ms` covers `load()` only; `infer_ms` covers the
/// single transcription or completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchRun {
    pub provider_id: ProviderId,
    pub role: BenchRole,
    pub cold: bool,
    pub load_ms: u64,
    pub infer_ms: u64,
    pub output: String,
    pub error: Option<String>,
}

impl BenchRun {
    fn failed(provider_id: &ProviderId, role: BenchRole, cold: bool, error: String) -> Self {
        BenchRun {
            provider_id: provider_id.clone(),
            role,
            cold,
            load_ms: 0,
            infer_ms: 0,
            output: String::new(),
            error: Some(error),
        }
    }
}

fn millis(since: Instant) -> u64 {
    since.elapsed().as_millis() as u64
}

impl Engine {
    /// Transcribes `clip` with the default profile's language and dictionary
    /// hint. `cold` unloads the provider first so `load_ms` includes the
    /// model load.
    pub async fn bench_asr(
        &self,
        provider: &ProviderId,
        clip: Arc<AudioClip>,
        cold: bool,
    ) -> BenchRun {
        let role = BenchRole::Asr;
        let config = self.config().current();
        let Some(cfg) = config.provider(provider) else {
            return BenchRun::failed(provider, role, cold, "provider not found".into());
        };
        let p = match self.registry().asr(cfg, &config.general) {
            Ok(p) => p,
            Err(e) => return BenchRun::failed(provider, role, cold, e.message),
        };
        if cold {
            p.unload().await;
        }
        let t0 = Instant::now();
        if let Err(e) = p.load().await {
            return BenchRun {
                load_ms: millis(t0),
                ..BenchRun::failed(provider, role, cold, e.message)
            };
        }
        let load_ms = millis(t0);
        let profile = config
            .profiles
            .iter()
            .find(|p| p.is_default)
            .expect("config validation guarantees a default profile");
        let hint = AsrHint {
            language: profile.language_hint.clone(),
            prompt: asr_hint(&config.dictionary, ASR_HINT_BUDGET_BYTES),
        };
        let t1 = Instant::now();
        let result = p.transcribe(&clip, &hint).await;
        let infer_ms = millis(t1);
        self.touch_idle();
        let (output, error) = match result {
            Ok(t) => (t.text, None),
            Err(e) => (String::new(), Some(e.message)),
        };
        BenchRun {
            provider_id: provider.clone(),
            role,
            cold,
            load_ms,
            infer_ms,
            output,
            error,
        }
    }

    /// Refines `transcript` with the prompt a session under the default
    /// profile would send, without app context. `cold` unloads the provider
    /// first so `load_ms` includes the model load.
    pub async fn bench_llm(&self, provider: &ProviderId, transcript: &str, cold: bool) -> BenchRun {
        let role = BenchRole::Llm;
        let config = self.config().current();
        let Some(cfg) = config.provider(provider) else {
            return BenchRun::failed(provider, role, cold, "provider not found".into());
        };
        let p = match self.registry().llm(cfg, &config.general) {
            Ok(p) => p,
            Err(e) => return BenchRun::failed(provider, role, cold, e.message),
        };
        if cold {
            p.unload().await;
        }
        let t0 = Instant::now();
        if let Err(e) = p.load().await {
            return BenchRun {
                load_ms: millis(t0),
                ..BenchRun::failed(provider, role, cold, e.message)
            };
        }
        let load_ms = millis(t0);
        let profile = config
            .profiles
            .iter()
            .find(|p| p.is_default)
            .expect("config validation guarantees a default profile");
        let base = self
            .config()
            .base_prompt()
            .unwrap_or_else(|_| DEFAULT_BASE_PROMPT.to_string());
        let terms = llm_terms(&config.dictionary);
        let context = Context::default();
        let messages = assemble(&PromptParts {
            base: &base,
            profile_addition: &profile.prompt_addition,
            dictionary_terms: &terms,
            context: &context,
            few_shot: &profile.few_shot,
            utterance: transcript,
            context_limit_bytes: config.general.context_limit_bytes,
        });
        let opts = LlmOptions {
            temperature: config.general.llm_temperature,
            max_tokens: config.general.llm_max_tokens,
            timeout: Duration::from_secs(config.general.llm_timeout_secs),
        };
        let t1 = Instant::now();
        let result = p.complete(messages, &opts).await;
        let infer_ms = millis(t1);
        self.touch_idle();
        let (output, error) = match result {
            Ok(text) => (text.trim().to_string(), None),
            Err(e) => (String::new(), Some(e.message)),
        };
        BenchRun {
            provider_id: provider.clone(),
            role,
            cold,
            load_ms,
            infer_ms,
            output,
            error,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::*;
    use crate::engine::tests::{idle_rig, one_second};
    use crate::SAMPLE_RATE;

    fn one_second_clip() -> AudioClip {
        AudioClip {
            samples: one_second(),
            sample_rate: SAMPLE_RATE,
        }
    }

    #[tokio::test]
    async fn cold_run_unloads_then_loads_and_reports_output() {
        let (fake, _tmp, e) = idle_rig().await;
        let id = e.config().current().profiles[0]
            .llm_provider
            .clone()
            .unwrap();
        let run = e.bench_llm(&id, "原文", true).await;
        assert_eq!(fake.unloads.load(Ordering::SeqCst), 1);
        assert_eq!(run.output, "ok");
        assert!(run.error.is_none());
        assert_eq!(run.role, BenchRole::Llm);
        assert!(run.cold);
        let warm = e.bench_llm(&id, "原文", false).await;
        assert_eq!(fake.unloads.load(Ordering::SeqCst), 1);
        assert!(!warm.cold);
        // The LLM saw the framed transcript as the last user message.
        let calls = fake.calls.lock().unwrap();
        assert!(calls
            .last()
            .unwrap()
            .last()
            .unwrap()
            .content
            .contains("原文"));
    }

    #[tokio::test]
    async fn bench_reports_unknown_provider() {
        let (_fake, _tmp, e) = idle_rig().await;
        let run = e
            .bench_asr(
                &ProviderId("nope".into()),
                Arc::new(one_second_clip()),
                false,
            )
            .await;
        assert_eq!(run.error.as_deref(), Some("provider not found"));
        assert!(run.output.is_empty());
    }

    #[tokio::test]
    async fn asr_run_uses_the_default_profile_hint() {
        let (_fake, _tmp, e) = idle_rig().await;
        let id = e.config().current().profiles[0].asr_provider.clone();
        let run = e.bench_asr(&id, Arc::new(one_second_clip()), false).await;
        assert_eq!(run.output, "原文");
        assert_eq!(run.role, BenchRole::Asr);
    }
}
