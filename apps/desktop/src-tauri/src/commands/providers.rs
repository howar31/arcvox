use arcvox_core::config::{Config, ProviderConfig, ProviderKind};
use arcvox_core::providers::probe::ProbeResult;
use arcvox_core::types::ProviderId;
use arcvox_core::Engine;
use tauri::State;

use super::{engine, CmdError, CmdResult};
use crate::app::AppState;

/// An embedded engine is its model's and a native one its service's; the
/// app makes, renames and removes them, never the user.
fn automatic() -> CmdError {
    CmdError::new(
        "engine_automatic",
        "an engine made by the app follows its model or service and cannot be edited",
    )
}

pub fn upsert(engine: &Engine, provider: ProviderConfig) -> CmdResult<Config> {
    if provider.kind != ProviderKind::Http {
        return Err(automatic());
    }
    let next =
        engine.config().update(
            |c| match c.providers.iter_mut().find(|p| p.id == provider.id) {
                Some(existing) => *existing = provider.clone(),
                None => c.providers.push(provider.clone()),
            },
        )?;
    Ok((*next).clone())
}

pub fn delete(engine: &Engine, id: &ProviderId) -> CmdResult<Config> {
    let current = engine.config().current();
    if current
        .provider(id)
        .is_some_and(|p| p.kind != ProviderKind::Http)
    {
        return Err(automatic());
    }
    let next = engine
        .config()
        .update(|c| c.providers.retain(|p| &p.id != id))?;
    Ok((*next).clone())
}

#[tauri::command]
pub fn providers_upsert(state: State<'_, AppState>, provider: ProviderConfig) -> CmdResult<Config> {
    upsert(&engine(&state)?, provider)
}

#[tauri::command]
pub fn providers_delete(state: State<'_, AppState>, id: ProviderId) -> CmdResult<Config> {
    delete(&engine(&state)?, &id)
}

#[tauri::command]
pub async fn providers_probe(
    state: State<'_, AppState>,
    provider: ProviderConfig,
) -> CmdResult<ProbeResult> {
    let e = engine(&state)?;
    Ok(e.probe_provider(&provider).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcvox_core::{DataDir, MemorySecretStore};
    use std::sync::Arc;

    async fn engine() -> (tempfile::TempDir, Engine) {
        let tmp = tempfile::tempdir().unwrap();
        let e = Engine::open(
            DataDir::custom(tmp.path()),
            Arc::new(MemorySecretStore::default()),
        )
        .await
        .unwrap();
        (tmp, e)
    }

    fn http(id: &str) -> ProviderConfig {
        ProviderConfig::http_test_provider(id, "http://localhost:1/v1", true, false)
    }

    #[tokio::test]
    async fn upsert_replaces_by_id_or_appends() {
        let (_t, e) = engine().await;
        let mut p = http("remote");
        let c = upsert(&e, p.clone()).unwrap();
        assert_eq!(c.providers.len(), 3);
        p.label = "Renamed".into();
        let c = upsert(&e, p.clone()).unwrap();
        assert_eq!(c.providers.len(), 3);
        assert_eq!(c.providers[2].label, "Renamed");
        p.id = ProviderId("new".into());
        let c = upsert(&e, p).unwrap();
        assert_eq!(c.providers.len(), 4);
    }

    #[tokio::test]
    async fn app_made_engines_are_not_edited_or_deleted_by_hand() {
        let (_t, e) = engine().await;
        let mut local = e.config().current().providers[0].clone();
        local.label = "Renamed".into();
        let err = upsert(&e, local.clone()).unwrap_err();
        assert_eq!(err.code, Some("engine_automatic"));
        let err = delete(&e, &local.id).unwrap_err();
        assert_eq!(err.code, Some("engine_automatic"));
        assert_eq!(e.config().current().providers.len(), 2);
    }

    #[tokio::test]
    async fn delete_of_a_referenced_provider_is_rejected_and_config_unchanged() {
        let (_t, e) = engine().await;
        upsert(&e, http("remote")).unwrap();
        e.config()
            .update(|c| c.profiles[0].asr_provider = ProviderId("remote".into()))
            .unwrap();
        let err = delete(&e, &ProviderId("remote".into())).unwrap_err();
        assert!(err.message.contains("unknown provider"), "{}", err.message);
        assert_eq!(e.config().current().providers.len(), 3);
        upsert(&e, http("spare")).unwrap();
        let c = delete(&e, &ProviderId("spare".into())).unwrap();
        assert_eq!(c.providers.len(), 3);
    }
}
