//! Provider factory for `embedded` providers: resolves the model file and
//! builds the in-process speech and language model providers.

use std::sync::Arc;
use std::time::Duration;

use arcvox_core::config::{General, ProviderConfig, ProviderKind, Runtime};
use arcvox_core::models::{ModelError, ModelStore};
use arcvox_core::providers::probe::{ProbeFailure, ProbeResult};
use arcvox_core::providers::{AsrProvider, LlmProvider, ProviderError};
use arcvox_core::types::ErrorCode;
use arcvox_core::ProviderFactory;
use async_trait::async_trait;

use crate::llama::LlamaLlmProvider;
use crate::llama_asr::LlamaAsrProvider;
use crate::whisper::WhisperAsrProvider;

pub struct EmbeddedFactory {
    store: Arc<ModelStore>,
    use_gpu: bool,
}

impl EmbeddedFactory {
    pub fn new(store: Arc<ModelStore>, use_gpu: bool) -> Self {
        Self { store, use_gpu }
    }
}

fn wrong_runtime(cfg: &ProviderConfig, expected: &str, what: &str) -> ProviderError {
    ProviderError::new(
        ErrorCode::InvalidConfig,
        format!("{}: runtime must be {expected} for {what}", cfg.label),
    )
}

fn model_error(cfg: &ProviderConfig, e: ModelError) -> ProviderError {
    match e {
        ModelError::NotInstalled(_) | ModelError::NotInCatalogue(_) => {
            ProviderError::new(ErrorCode::ModelMissing, format!("{}: {e}", cfg.label))
                .with_hint("download or import the model in Settings > Engines and models")
        }
        other => ProviderError::new(ErrorCode::InvalidConfig, format!("{}: {other}", cfg.label)),
    }
}

fn encoder_missing(cfg: &ProviderConfig) -> ProviderError {
    ProviderError::new(
        ErrorCode::ModelMissing,
        format!("{}: no audio encoder beside the model", cfg.label),
    )
    .with_hint("put the model's mmproj file beside it in Settings > Engines and models")
}

fn fail(failure: ProbeFailure) -> ProbeResult {
    ProbeResult {
        ok: false,
        failure: Some(failure),
        models: Vec::new(),
    }
}

impl EmbeddedFactory {
    /// A llama.cpp engine that transcribes and has no audio encoder to hear through.
    fn needs_encoder(&self, cfg: &ProviderConfig) -> bool {
        cfg.runtime == Some(Runtime::Llama)
            && cfg.capabilities.transcribe
            && !matches!(self.store.resolve_encoder(cfg), Ok(Some(_)))
    }
}

// The path is resolved and the runtime checked at construction so a session
// fails before recording; the provider re-checks when it loads the file.
#[async_trait]
impl ProviderFactory for EmbeddedFactory {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Embedded
    }

    fn asr(
        &self,
        cfg: &ProviderConfig,
        general: &General,
    ) -> Result<Arc<dyn AsrProvider>, ProviderError> {
        let timeout = Duration::from_secs(general.asr_timeout_secs);
        let path = self.store.resolve(cfg).map_err(|e| model_error(cfg, e))?;
        match cfg.runtime {
            Some(Runtime::Whisper) => Ok(Arc::new(WhisperAsrProvider::new(
                path,
                cfg.label.clone(),
                timeout,
                self.use_gpu,
            ))),
            // A llama.cpp model hears through its audio encoder.
            Some(Runtime::Llama) => {
                let encoder = self
                    .store
                    .resolve_encoder(cfg)
                    .map_err(|e| model_error(cfg, e))?
                    .ok_or_else(|| encoder_missing(cfg))?;
                Ok(Arc::new(LlamaAsrProvider::new(
                    path,
                    encoder,
                    cfg.label.clone(),
                    timeout,
                    self.use_gpu,
                )))
            }
            None => Err(wrong_runtime(cfg, "whisper or llama", "speech recognition")),
        }
    }

    fn llm(
        &self,
        cfg: &ProviderConfig,
        _general: &General,
    ) -> Result<Arc<dyn LlmProvider>, ProviderError> {
        if cfg.runtime != Some(Runtime::Llama) {
            return Err(wrong_runtime(cfg, "llama", "text rewriting"));
        }
        let path = self.store.resolve(cfg).map_err(|e| model_error(cfg, e))?;
        Ok(Arc::new(LlamaLlmProvider::new(
            path,
            cfg.label.clone(),
            self.use_gpu,
        )))
    }

    fn ready(&self, cfg: &ProviderConfig) -> bool {
        self.store.resolve(cfg).is_ok() && !self.needs_encoder(cfg)
    }

    async fn probe(&self, cfg: &ProviderConfig) -> ProbeResult {
        let path = match self.store.resolve(cfg) {
            Ok(p) => p,
            Err(ModelError::NotInstalled(_)) | Err(ModelError::NotInCatalogue(_)) => {
                return fail(ProbeFailure::ModelMissing)
            }
            Err(e) => {
                return fail(ProbeFailure::Other {
                    message: e.to_string(),
                })
            }
        };
        if self.needs_encoder(cfg) {
            return fail(ProbeFailure::ModelMissing);
        }
        match arcvox_core::models::magic::detect(&path) {
            Ok(Some(rt)) if Some(rt) == cfg.runtime => ProbeResult::ok(),
            Ok(_) => fail(ProbeFailure::ModelCorrupt),
            Err(e) => fail(ProbeFailure::Other {
                message: e.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcvox_core::DataDir;
    use std::path::Path;

    fn store(tmp: &Path) -> Arc<ModelStore> {
        let dirs = DataDir::custom(tmp);
        dirs.ensure().unwrap();
        Arc::new(ModelStore::new(
            &dirs,
            reqwest::Client::new(),
            r#"{"schema_version":1,"models":[{"id":"w","runtime":"whisper","role":"asr","label":"W","url":"https://h/ggml-w.bin","sha256":"00","size_bytes":4,"license":"MIT","recommended":true}]}"#,
            None,
        ))
    }

    #[test]
    fn a_llama_model_without_an_audio_encoder_cannot_transcribe() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());
        let f = EmbeddedFactory::new(s.clone(), false);
        let p = s.model_path("w");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"GGUF0000").unwrap();
        let mut cfg = ProviderConfig::embedded("e", "E", Runtime::Llama, "w");
        cfg.capabilities.transcribe = true;
        let err = f.asr(&cfg, &General::default()).err().unwrap();
        assert_eq!(err.code, ErrorCode::ModelMissing);
        assert!(err.message.contains("audio encoder"));
        assert!(!f.ready(&cfg));
    }

    #[test]
    fn a_custom_speech_model_with_its_encoder_makes_a_provider() {
        use arcvox_core::models::gguf::testing::{encoder, speech_model};
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());
        let f = EmbeddedFactory::new(s.clone(), false);
        let custom = s.models_dir().join("custom");
        std::fs::create_dir_all(&custom).unwrap();
        std::fs::write(custom.join("asr.gguf"), speech_model(1024)).unwrap();
        std::fs::write(custom.join("mmproj-asr.gguf"), encoder(Some(1024))).unwrap();
        let m = s
            .installed()
            .into_iter()
            .find(|m| m.id == "custom/asr.gguf")
            .unwrap();
        let mut cfg = ProviderConfig::embedded("custom/asr.gguf", "Q", Runtime::Llama, "x");
        cfg.model_id = None;
        cfg.model_path = Some(m.path);
        cfg.capabilities = m.capabilities;
        assert!(f.asr(&cfg, &General::default()).is_ok());
        assert!(f.ready(&cfg));
    }

    #[test]
    fn llm_requires_llama_runtime() {
        let tmp = tempfile::tempdir().unwrap();
        let f = EmbeddedFactory::new(store(tmp.path()), false);
        let cfg = ProviderConfig::embedded("e", "E", Runtime::Whisper, "w");
        assert_eq!(
            f.llm(&cfg, &General::default()).err().unwrap().code,
            ErrorCode::InvalidConfig
        );
    }

    #[test]
    fn missing_file_is_model_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let f = EmbeddedFactory::new(store(tmp.path()), false);
        let cfg = ProviderConfig::embedded("e", "E", Runtime::Whisper, "w");
        let err = f.asr(&cfg, &General::default()).err().unwrap();
        assert_eq!(err.code, ErrorCode::ModelMissing);
        assert!(err.hint.as_deref().unwrap().contains("Engines and models"));
    }

    #[tokio::test]
    async fn probe_reports_missing_then_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());
        let f = EmbeddedFactory::new(s.clone(), false);
        let cfg = ProviderConfig::embedded("e", "E", Runtime::Whisper, "w");
        assert_eq!(
            f.probe(&cfg).await.failure,
            Some(ProbeFailure::ModelMissing)
        );
        let p = s.model_path("w");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"GGUF0000").unwrap();
        assert_eq!(
            f.probe(&cfg).await.failure,
            Some(ProbeFailure::ModelCorrupt)
        );
        std::fs::write(&p, b"lmgg0000").unwrap();
        assert!(f.probe(&cfg).await.ok);
    }
}
