//! Provider kinds that are not built into the core (in-process runtimes,
//! platform-native services). The client registers one factory per kind when
//! the engine starts.

use std::sync::Arc;

use async_trait::async_trait;

use crate::config::{General, ProviderConfig, ProviderKind};
use crate::providers::probe::ProbeResult;
use crate::providers::{AsrProvider, LlmProvider, ProviderError};

#[async_trait]
pub trait ProviderFactory: Send + Sync {
    fn kind(&self) -> ProviderKind;
    fn asr(
        &self,
        cfg: &ProviderConfig,
        general: &General,
    ) -> Result<Arc<dyn AsrProvider>, ProviderError>;
    fn llm(
        &self,
        cfg: &ProviderConfig,
        general: &General,
    ) -> Result<Arc<dyn LlmProvider>, ProviderError>;
    async fn probe(&self, cfg: &ProviderConfig) -> ProbeResult;
}
