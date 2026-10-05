//! Provider factory for `native` providers backed by Apple frameworks.

use std::sync::Arc;
use std::time::Duration;

use arcvox_core::config::{
    General, ProviderConfig, ProviderKind, NATIVE_APPLE_DICTATION, NATIVE_APPLE_FM,
};
use arcvox_core::providers::probe::{ProbeFailure, ProbeResult};
use arcvox_core::providers::{AsrProvider, LlmProvider, ProviderError};
use arcvox_core::types::ErrorCode;
use arcvox_core::ProviderFactory;
use async_trait::async_trait;

use super::{
    dictation_status, fm_status, locale_for_hint, AppleAsrProvider, AppleLlmProvider, NativeStatus,
};

pub struct AppleFactory;

fn check_native_id(cfg: &ProviderConfig, expected: &str) -> Result<(), ProviderError> {
    match cfg.native_id.as_deref() {
        Some(id) if id == expected => Ok(()),
        Some(id) => Err(ProviderError::new(
            ErrorCode::InvalidConfig,
            format!(
                "{}: native provider {id} does not support this role",
                cfg.label
            ),
        )),
        None => Err(ProviderError::new(
            ErrorCode::InvalidConfig,
            format!("{}: no native provider is set", cfg.label),
        )),
    }
}

fn reason(status: NativeStatus) -> String {
    match status {
        NativeStatus::Ready => "ready".into(),
        NativeStatus::NeedsDownload => "language not installed".into(),
        NativeStatus::LocaleUnsupported => "language not supported".into(),
        NativeStatus::OsTooOld => "requires macOS 26".into(),
        NativeStatus::NotEligible => "this Mac cannot run Apple Intelligence".into(),
        NativeStatus::IntelligenceOff => "turn on Apple Intelligence in System Settings".into(),
        NativeStatus::ModelNotReady => "the Apple model is still downloading".into(),
        NativeStatus::Failed(c) => format!("status check failed (code {c})"),
    }
}

fn probe_result(status: NativeStatus) -> ProbeResult {
    match status {
        NativeStatus::Ready => ProbeResult {
            ok: true,
            failure: None,
        },
        other => ProbeResult {
            ok: false,
            failure: Some(ProbeFailure::NativeUnavailable {
                reason: reason(other),
            }),
        },
    }
}

// Construction never calls into the bridge; only transcribe, complete and
// probe do.
#[async_trait]
impl ProviderFactory for AppleFactory {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Native
    }

    fn asr(
        &self,
        cfg: &ProviderConfig,
        general: &General,
    ) -> Result<Arc<dyn AsrProvider>, ProviderError> {
        check_native_id(cfg, NATIVE_APPLE_DICTATION)?;
        Ok(Arc::new(AppleAsrProvider::from_hint(
            cfg.label.clone(),
            Duration::from_secs(general.asr_timeout_secs),
        )))
    }

    fn llm(
        &self,
        cfg: &ProviderConfig,
        _general: &General,
    ) -> Result<Arc<dyn LlmProvider>, ProviderError> {
        check_native_id(cfg, NATIVE_APPLE_FM)?;
        Ok(Arc::new(AppleLlmProvider::new(cfg.label.clone())))
    }

    async fn probe(&self, cfg: &ProviderConfig) -> ProbeResult {
        let status = match cfg.native_id.as_deref() {
            Some(NATIVE_APPLE_DICTATION) => {
                tokio::task::spawn_blocking(|| dictation_status(locale_for_hint(None))).await
            }
            Some(NATIVE_APPLE_FM) => tokio::task::spawn_blocking(fm_status).await,
            other => {
                return ProbeResult {
                    ok: false,
                    failure: Some(ProbeFailure::Other {
                        message: format!("unknown native provider {}", other.unwrap_or("(none)")),
                    }),
                }
            }
        };
        match status {
            Ok(s) => probe_result(s),
            Err(e) => ProbeResult {
                ok: false,
                failure: Some(ProbeFailure::Other {
                    message: e.to_string(),
                }),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcvox_core::config::Capabilities;

    #[test]
    fn unknown_native_id_is_invalid_config() {
        let f = AppleFactory;
        let mut cfg = ProviderConfig::native("x", "X", "apple.nope", Capabilities::default());
        assert_eq!(
            f.llm(&cfg, &General::default()).err().unwrap().code,
            ErrorCode::InvalidConfig
        );
        cfg.native_id = None;
        assert_eq!(
            f.asr(&cfg, &General::default()).err().unwrap().code,
            ErrorCode::InvalidConfig
        );
    }

    #[test]
    fn known_native_ids_build_providers() {
        let f = AppleFactory;
        assert!(f
            .asr(
                &ProviderConfig::native("d", "D", "apple.dictation", Capabilities::default()),
                &General::default()
            )
            .is_ok());
        assert!(f
            .llm(
                &ProviderConfig::native(
                    "m",
                    "M",
                    "apple.foundation_models",
                    Capabilities::default()
                ),
                &General::default()
            )
            .is_ok());
    }

    #[test]
    fn needs_download_probes_as_not_installed() {
        let r = probe_result(NativeStatus::NeedsDownload);
        assert!(!r.ok);
        assert_eq!(
            r.failure,
            Some(ProbeFailure::NativeUnavailable {
                reason: "language not installed".into()
            })
        );
        assert!(probe_result(NativeStatus::Ready).ok);
    }
}
