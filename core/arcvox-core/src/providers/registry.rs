//! Builds provider instances from config and caches them by id. An instance
//! is rebuilt when its config changes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::config::{General, ProviderConfig, ProviderKind};
use crate::providers::factory::ProviderFactory;
use crate::providers::http_asr::HttpAsrProvider;
use crate::providers::http_llm::HttpLlmProvider;
use crate::providers::{AsrProvider, LlmProvider, ProviderError};
use crate::secrets::SecretStore;
use crate::types::{ErrorCode, ProviderId};

/// A provider built from a config, with what it was built from: the
/// config, the timeout and retry count, and the API key it was given.
/// Any of these changing means a new provider.
struct Cached<P: ?Sized> {
    cfg: ProviderConfig,
    timing: (u64, u32),
    key: Option<String>,
    provider: Arc<P>,
}

/// One role's providers by id; a lock poisoned by a panic elsewhere stays
/// usable, the map being whole at every point.
struct Cache<P: ?Sized>(Mutex<HashMap<ProviderId, Cached<P>>>);

impl<P: ?Sized> Cache<P> {
    fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<ProviderId, Cached<P>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The cached provider when it was built from the same config, timing
    /// and key.
    fn get(
        &self,
        cfg: &ProviderConfig,
        timing: (u64, u32),
        key: &Option<String>,
    ) -> Option<Arc<P>> {
        let cache = self.lock();
        let c = cache.get(&cfg.id)?;
        (c.cfg == *cfg && c.timing == timing && c.key == *key).then(|| c.provider.clone())
    }

    fn insert(
        &self,
        cfg: &ProviderConfig,
        timing: (u64, u32),
        key: Option<String>,
        provider: Arc<P>,
    ) {
        self.lock().insert(
            cfg.id.clone(),
            Cached {
                cfg: cfg.clone(),
                timing,
                key,
                provider,
            },
        );
    }

    /// Removes the providers whose config matches `pred` and returns them.
    fn evict_where(&self, pred: &impl Fn(&ProviderConfig) -> bool) -> Vec<Arc<P>> {
        let mut out = Vec::new();
        self.lock().retain(|_, c| {
            if pred(&c.cfg) {
                out.push(c.provider.clone());
                false
            } else {
                true
            }
        });
        out
    }
}

pub struct ProviderRegistry {
    client: reqwest::Client,
    secrets: Arc<dyn SecretStore>,
    asr: Cache<dyn AsrProvider>,
    llm: Cache<dyn LlmProvider>,
    factories: Vec<Arc<dyn ProviderFactory>>,
}

/// Providers removed from the cache by [`ProviderRegistry::evict_where`].
pub type EvictedProviders = (Vec<Arc<dyn AsrProvider>>, Vec<Arc<dyn LlmProvider>>);

impl ProviderRegistry {
    pub fn new(client: reqwest::Client, secrets: Arc<dyn SecretStore>) -> Self {
        Self::with_factories(client, secrets, Vec::new())
    }

    pub fn with_factories(
        client: reqwest::Client,
        secrets: Arc<dyn SecretStore>,
        factories: Vec<Arc<dyn ProviderFactory>>,
    ) -> Self {
        Self {
            client,
            secrets,
            asr: Cache::new(),
            llm: Cache::new(),
            factories,
        }
    }

    pub fn factory_for(&self, kind: ProviderKind) -> Option<Arc<dyn ProviderFactory>> {
        self.factories.iter().find(|f| f.kind() == kind).cloned()
    }

    /// Whether the engine can run now, by its kind's factory; an engine of
    /// a kind no factory serves cannot.
    pub fn ready(&self, cfg: &ProviderConfig) -> bool {
        match cfg.kind {
            ProviderKind::Http => true,
            kind => self.factory_for(kind).is_some_and(|f| f.ready(cfg)),
        }
    }

    pub fn llm_providers(&self) -> Vec<Arc<dyn LlmProvider>> {
        self.llm
            .lock()
            .values()
            .map(|c| c.provider.clone())
            .collect()
    }

    /// Removes every cached provider whose config matches `pred` and returns
    /// them, so the caller can unload them outside the cache locks.
    pub fn evict_where(&self, pred: impl Fn(&ProviderConfig) -> bool) -> EvictedProviders {
        (self.asr.evict_where(&pred), self.llm.evict_where(&pred))
    }

    /// `evict_where` for the speech cache only.
    pub fn evict_asr_where(
        &self,
        pred: impl Fn(&ProviderConfig) -> bool,
    ) -> Vec<Arc<dyn AsrProvider>> {
        self.asr.evict_where(&pred)
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
        let timing = (general.asr_timeout_secs, general.retry_count);
        if let Some(p) = self.asr.get(cfg, timing, &key) {
            return Ok(p);
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
                    general.retry_count,
                    cfg.capabilities.language_hint,
                    cfg.capabilities.hotwords,
                ))
            }
            kind @ (ProviderKind::Embedded | ProviderKind::Native) => {
                match self.factory_for(kind) {
                    Some(f) => f.asr(cfg, general)?,
                    None => return Err(not_available(cfg)),
                }
            }
        };
        self.asr.insert(cfg, timing, key, provider.clone());
        Ok(provider)
    }

    pub fn llm(
        &self,
        cfg: &ProviderConfig,
        general: &General,
    ) -> Result<Arc<dyn LlmProvider>, ProviderError> {
        let key = self.resolve_key(cfg)?;
        let timing = (general.llm_timeout_secs, general.retry_count);
        if let Some(p) = self.llm.get(cfg, timing, &key) {
            return Ok(p);
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
                    general.retry_count,
                ))
            }
            kind @ (ProviderKind::Embedded | ProviderKind::Native) => {
                match self.factory_for(kind) {
                    Some(f) => f.llm(cfg, general)?,
                    None => return Err(not_available(cfg)),
                }
            }
        };
        self.llm.insert(cfg, timing, key, provider.clone());
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
pub(crate) mod tests {
    use super::*;
    use crate::config::Config;
    use crate::secrets::MemorySecretStore;

    pub(crate) struct FakeFactory;
    #[async_trait::async_trait]
    impl ProviderFactory for FakeFactory {
        fn kind(&self) -> ProviderKind {
            ProviderKind::Embedded
        }
        fn asr(
            &self,
            _cfg: &ProviderConfig,
            _g: &General,
        ) -> Result<Arc<dyn AsrProvider>, ProviderError> {
            Ok(crate::steps::asr::fakes::FakeAsr::ok("hi"))
        }
        fn llm(
            &self,
            _cfg: &ProviderConfig,
            _g: &General,
        ) -> Result<Arc<dyn LlmProvider>, ProviderError> {
            Ok(crate::steps::llm::fakes::FakeLlm::ok("hi"))
        }
        async fn probe(&self, _cfg: &ProviderConfig) -> crate::providers::probe::ProbeResult {
            crate::providers::probe::ProbeResult {
                ok: true,
                failure: None,
                models: Vec::new(),
            }
        }
    }

    pub(crate) fn embedded_cfg() -> ProviderConfig {
        let mut p = Config::http_test_config().providers[0].clone();
        p.id = ProviderId("emb".into());
        p.kind = ProviderKind::Embedded;
        p.base_url = None;
        p.runtime = Some(crate::config::Runtime::Whisper);
        p.model_id = Some("m".into());
        p
    }

    #[test]
    fn evict_returns_removed_providers_and_next_build_is_fresh() {
        let r = ProviderRegistry::with_factories(
            reqwest::Client::new(),
            Arc::new(MemorySecretStore::default()),
            vec![Arc::new(FakeFactory)],
        );
        let cfg = embedded_cfg();
        let a = r.asr(&cfg, &General::default()).unwrap();
        let l = r.llm(&cfg, &General::default()).unwrap();
        let (asr, llm) = r.evict_where(|c| c.model_id.as_deref() == Some("m"));
        assert_eq!(asr.len(), 1);
        assert_eq!(llm.len(), 1);
        assert!(Arc::ptr_eq(&asr[0], &a));
        assert!(Arc::ptr_eq(&llm[0], &l));
        assert!(r.llm_providers().is_empty());
        let b = r.asr(&cfg, &General::default()).unwrap();
        assert!(!Arc::ptr_eq(&a, &b));
        let (none_a, none_l) = r.evict_where(|c| c.model_id.as_deref() == Some("other"));
        assert!(none_a.is_empty() && none_l.is_empty());
    }

    fn registry() -> ProviderRegistry {
        ProviderRegistry::new(
            reqwest::Client::new(),
            Arc::new(MemorySecretStore::default()),
        )
    }

    #[test]
    fn caches_by_id_and_rebuilds_on_config_change() {
        let r = registry();
        let cfg = Config::http_test_config();
        let ollama = cfg
            .provider(&ProviderId("local-ollama".into()))
            .unwrap()
            .clone();
        let a = r.llm(&ollama, &General::default()).unwrap();
        let b = r.llm(&ollama, &General::default()).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        let mut changed = ollama.clone();
        changed.model = Some("other".into());
        let c = r.llm(&changed, &General::default()).unwrap();
        assert!(!Arc::ptr_eq(&a, &c));
    }

    #[test]
    fn asr_rebuilds_when_timeout_changes() {
        let r = registry();
        let cfg = Config::http_test_config();
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
        let cfg = Config::http_test_config();
        let mut ollama = cfg
            .provider(&ProviderId("local-ollama".into()))
            .unwrap()
            .clone();
        ollama.api_key_ref = Some("gemini".into());
        assert_eq!(
            r.llm(&ollama, &General::default()).err().unwrap().code,
            ErrorCode::ProviderUnauthorized
        );
        ollama.api_key_ref = None;
        ollama.kind = ProviderKind::Embedded;
        assert_eq!(
            r.llm(&ollama, &General::default()).err().unwrap().code,
            ErrorCode::InvalidConfig
        );
        ollama.kind = ProviderKind::Http;
        ollama.base_url = None;
        assert_eq!(
            r.llm(&ollama, &General::default()).err().unwrap().code,
            ErrorCode::InvalidConfig
        );
    }

    #[test]
    fn rebuilds_when_secret_rotates() {
        let secrets = Arc::new(MemorySecretStore::default());
        secrets.set("gemini", "k1").unwrap();
        let dyn_secrets: Arc<dyn SecretStore> = secrets.clone();
        let r = ProviderRegistry::new(reqwest::Client::new(), dyn_secrets);
        let cfg = Config::http_test_config();
        let mut ollama = cfg
            .provider(&ProviderId("local-ollama".into()))
            .unwrap()
            .clone();
        ollama.api_key_ref = Some("gemini".into());

        let a = r.llm(&ollama, &General::default()).unwrap();
        secrets.set("gemini", "k2").unwrap();
        let b = r.llm(&ollama, &General::default()).unwrap();
        assert!(!Arc::ptr_eq(&a, &b));
        let c = r.llm(&ollama, &General::default()).unwrap();
        assert!(Arc::ptr_eq(&b, &c));
    }

    #[test]
    fn embedded_kind_dispatches_to_its_factory_and_caches() {
        let r = ProviderRegistry::with_factories(
            reqwest::Client::new(),
            Arc::new(MemorySecretStore::default()),
            vec![Arc::new(FakeFactory)],
        );
        let cfg = embedded_cfg();
        let a = r.asr(&cfg, &General::default()).unwrap();
        let b = r.asr(&cfg, &General::default()).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert!(r.llm(&cfg, &General::default()).is_ok());
    }

    #[test]
    fn kind_without_factory_is_invalid_config() {
        let r = registry();
        let err = r.asr(&embedded_cfg(), &General::default()).err().unwrap();
        assert_eq!(err.code, ErrorCode::InvalidConfig);
    }

    #[test]
    fn llm_providers_lists_cached_instances() {
        let r = ProviderRegistry::with_factories(
            reqwest::Client::new(),
            Arc::new(MemorySecretStore::default()),
            vec![Arc::new(FakeFactory)],
        );
        assert!(r.llm_providers().is_empty());
        r.llm(&embedded_cfg(), &General::default()).unwrap();
        assert_eq!(r.llm_providers().len(), 1);
    }

    #[test]
    fn llm_rebuilds_when_timeout_changes() {
        let r = registry();
        let cfg = Config::http_test_config();
        let ollama = cfg
            .provider(&ProviderId("local-ollama".into()))
            .unwrap()
            .clone();
        let g2 = General {
            llm_timeout_secs: 5,
            ..Default::default()
        };
        let a = r.llm(&ollama, &General::default()).unwrap();
        let b = r.llm(&ollama, &g2).unwrap();
        assert!(!Arc::ptr_eq(&a, &b));
    }
}
