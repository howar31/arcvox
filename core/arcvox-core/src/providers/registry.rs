//! Builds provider instances from config and caches them by id. An instance
//! is rebuilt when its config changes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::config::{General, ProviderConfig, ProviderKind};
use crate::providers::http_asr::HttpAsrProvider;
use crate::providers::http_llm::HttpLlmProvider;
use crate::providers::{AsrProvider, LlmProvider, ProviderError};
use crate::secrets::SecretStore;
use crate::types::{ErrorCode, ProviderId};

#[allow(clippy::type_complexity)]
pub struct ProviderRegistry {
    client: reqwest::Client,
    secrets: Arc<dyn SecretStore>,
    asr: Mutex<HashMap<ProviderId, (ProviderConfig, u64, Option<String>, Arc<dyn AsrProvider>)>>,
    llm: Mutex<HashMap<ProviderId, (ProviderConfig, Option<String>, Arc<dyn LlmProvider>)>>,
}

impl ProviderRegistry {
    pub fn new(client: reqwest::Client, secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            client,
            secrets,
            asr: Mutex::new(HashMap::new()),
            llm: Mutex::new(HashMap::new()),
        }
    }

    pub fn resolve_key(&self, cfg: &ProviderConfig) -> Result<Option<String>, ProviderError> {
        match &cfg.api_key_ref {
            None => Ok(None),
            Some(r) => self
                .secrets
                .get(r)
                .map_err(|e| ProviderError::new(ErrorCode::Internal, e.to_string()))?
                .map(Some)
                .ok_or_else(|| {
                    ProviderError::new(
                        ErrorCode::ProviderUnauthorized,
                        format!("{}: API key `{r}` not found", cfg.label),
                    )
                    .with_hint("enter the API key in provider settings")
                }),
        }
    }

    pub fn asr(
        &self,
        cfg: &ProviderConfig,
        general: &General,
    ) -> Result<Arc<dyn AsrProvider>, ProviderError> {
        let key = self.resolve_key(cfg)?;
        let mut cache = self.asr.lock().unwrap();
        if let Some((cached_cfg, cached_timeout, cached_key, p)) = cache.get(&cfg.id) {
            if cached_cfg == cfg
                && *cached_timeout == general.asr_timeout_secs
                && *cached_key == key
            {
                return Ok(p.clone());
            }
        }
        let provider: Arc<dyn AsrProvider> = match cfg.kind {
            ProviderKind::Http => {
                let base = require_base_url(cfg)?;
                Arc::new(HttpAsrProvider::new(
                    self.client.clone(),
                    base,
                    cfg.model.clone().unwrap_or_default(),
                    key.clone(),
                    cfg.extra_headers.clone(),
                    cfg.label.clone(),
                    Duration::from_secs(general.asr_timeout_secs),
                    cfg.capabilities.language_hint,
                    cfg.capabilities.hotwords,
                ))
            }
            ProviderKind::Embedded | ProviderKind::Native => return Err(not_available(cfg)),
        };
        cache.insert(
            cfg.id.clone(),
            (cfg.clone(), general.asr_timeout_secs, key, provider.clone()),
        );
        Ok(provider)
    }

    pub fn llm(&self, cfg: &ProviderConfig) -> Result<Arc<dyn LlmProvider>, ProviderError> {
        let key = self.resolve_key(cfg)?;
        let mut cache = self.llm.lock().unwrap();
        if let Some((cached_cfg, cached_key, p)) = cache.get(&cfg.id) {
            if cached_cfg == cfg && *cached_key == key {
                return Ok(p.clone());
            }
        }
        let provider: Arc<dyn LlmProvider> = match cfg.kind {
            ProviderKind::Http => {
                let base = require_base_url(cfg)?;
                Arc::new(HttpLlmProvider::new(
                    self.client.clone(),
                    base,
                    cfg.model.clone().unwrap_or_default(),
                    key.clone(),
                    cfg.extra_headers.clone(),
                    cfg.label.clone(),
                ))
            }
            ProviderKind::Embedded | ProviderKind::Native => return Err(not_available(cfg)),
        };
        cache.insert(cfg.id.clone(), (cfg.clone(), key, provider.clone()));
        Ok(provider)
    }
}

fn require_base_url(cfg: &ProviderConfig) -> Result<&url::Url, ProviderError> {
    cfg.base_url.as_ref().ok_or_else(|| {
        ProviderError::new(
            ErrorCode::InvalidConfig,
            format!("{}: base_url is missing", cfg.label),
        )
    })
}

fn not_available(cfg: &ProviderConfig) -> ProviderError {
    ProviderError::new(
        ErrorCode::InvalidConfig,
        format!(
            "{}: {:?} providers are not available in this build",
            cfg.label, cfg.kind
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::secrets::MemorySecretStore;

    fn registry() -> ProviderRegistry {
        ProviderRegistry::new(
            reqwest::Client::new(),
            Arc::new(MemorySecretStore::default()),
        )
    }

    #[test]
    fn caches_by_id_and_rebuilds_on_config_change() {
        let r = registry();
        let cfg = Config::first_run();
        let ollama = cfg
            .provider(&ProviderId("local-ollama".into()))
            .unwrap()
            .clone();
        let a = r.llm(&ollama).unwrap();
        let b = r.llm(&ollama).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        let mut changed = ollama.clone();
        changed.model = Some("other".into());
        let c = r.llm(&changed).unwrap();
        assert!(!Arc::ptr_eq(&a, &c));
    }

    #[test]
    fn asr_rebuilds_when_timeout_changes() {
        let r = registry();
        let cfg = Config::first_run();
        let sp = cfg
            .provider(&ProviderId("local-speaches".into()))
            .unwrap()
            .clone();
        let g1 = General::default();
        let g2 = General {
            asr_timeout_secs: 5,
            ..Default::default()
        };
        let a = r.asr(&sp, &g1).unwrap();
        let b = r.asr(&sp, &g2).unwrap();
        assert!(!Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn missing_key_and_unavailable_kinds_are_errors() {
        let r = registry();
        let cfg = Config::first_run();
        let mut ollama = cfg
            .provider(&ProviderId("local-ollama".into()))
            .unwrap()
            .clone();
        ollama.api_key_ref = Some("gemini".into());
        assert_eq!(
            r.llm(&ollama).err().unwrap().code,
            ErrorCode::ProviderUnauthorized
        );
        ollama.api_key_ref = None;
        ollama.kind = ProviderKind::Embedded;
        assert_eq!(r.llm(&ollama).err().unwrap().code, ErrorCode::InvalidConfig);
        ollama.kind = ProviderKind::Http;
        ollama.base_url = None;
        assert_eq!(r.llm(&ollama).err().unwrap().code, ErrorCode::InvalidConfig);
    }

    #[test]
    fn rebuilds_when_secret_rotates() {
        let secrets = Arc::new(MemorySecretStore::default());
        secrets.set("gemini", "k1").unwrap();
        let dyn_secrets: Arc<dyn SecretStore> = secrets.clone();
        let r = ProviderRegistry::new(reqwest::Client::new(), dyn_secrets);
        let cfg = Config::first_run();
        let mut ollama = cfg
            .provider(&ProviderId("local-ollama".into()))
            .unwrap()
            .clone();
        ollama.api_key_ref = Some("gemini".into());

        let a = r.llm(&ollama).unwrap();
        secrets.set("gemini", "k2").unwrap();
        let b = r.llm(&ollama).unwrap();
        assert!(!Arc::ptr_eq(&a, &b));
        let c = r.llm(&ollama).unwrap();
        assert!(Arc::ptr_eq(&b, &c));
    }
}
