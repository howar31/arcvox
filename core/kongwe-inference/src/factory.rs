//! Provider factory for `embedded` providers: resolves the model file and
//! builds the in-process speech and language model providers.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use kongwe_core::config::{General, ProviderConfig, ProviderKind, Runtime};
use kongwe_core::models::{ModelError, ModelStore};
use kongwe_core::providers::probe::{ProbeFailure, ProbeResult};
use kongwe_core::providers::{AsrProvider, LlmProvider, ProviderError};
use kongwe_core::types::ErrorCode;
use kongwe_core::ProviderFactory;

use crate::llama::LlamaLlmProvider;
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
                .with_hint("download or import the model in Settings > Models")
        }
        other => ProviderError::new(ErrorCode::InvalidConfig, format!("{}: {other}", cfg.label)),
    }
}

fn fail(failure: ProbeFailure) -> ProbeResult {
    ProbeResult {
        ok: false,
        failure: Some(failure),
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
        if cfg.runtime != Some(Runtime::Whisper) {
            return Err(wrong_runtime(cfg, "whisper", "speech recognition"));
        }
        let path = self.store.resolve(cfg).map_err(|e| model_error(cfg, e))?;
        Ok(Arc::new(WhisperAsrProvider::new(
            path,
            cfg.label.clone(),
            Duration::from_secs(general.asr_timeout_secs),
            self.use_gpu,
        )))
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
        match kongwe_core::models::magic::detect(&path) {
            Ok(Some(rt)) if Some(rt) == cfg.runtime => ProbeResult {
                ok: true,
                failure: None,
            },
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
    use kongwe_core::config::Capabilities;
    use kongwe_core::DataDir;
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
    fn asr_requires_whisper_runtime() {
        let tmp = tempfile::tempdir().unwrap();
        let f = EmbeddedFactory::new(store(tmp.path()), false);
        let cfg = ProviderConfig::embedded("e", "E", Runtime::Llama, "w", Capabilities::default());
        assert_eq!(
            f.asr(&cfg, &General::default()).err().unwrap().code,
            ErrorCode::InvalidConfig
        );
    }

    #[test]
    fn missing_file_is_model_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let f = EmbeddedFactory::new(store(tmp.path()), false);
        let cfg =
            ProviderConfig::embedded("e", "E", Runtime::Whisper, "w", Capabilities::default());
        let err = f.asr(&cfg, &General::default()).err().unwrap();
        assert_eq!(err.code, ErrorCode::ModelMissing);
        assert!(err.hint.as_deref().unwrap().contains("Models"));
    }

    #[tokio::test]
    async fn probe_reports_missing_then_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());
        let f = EmbeddedFactory::new(s.clone(), false);
        let cfg =
            ProviderConfig::embedded("e", "E", Runtime::Whisper, "w", Capabilities::default());
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
