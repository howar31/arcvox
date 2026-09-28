use kongwe_core::config::{Config, ProviderConfig};
use kongwe_core::providers::probe::ProbeResult;
use kongwe_core::types::ProviderId;
use kongwe_core::Engine;
use tauri::State;

use super::{engine, CmdResult};
use crate::app::AppState;

pub fn upsert(engine: &Engine, provider: ProviderConfig) -> CmdResult<Config> {
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
    use kongwe_core::{DataDir, MemorySecretStore};
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

    #[tokio::test]
    async fn upsert_replaces_by_id_or_appends() {
        let (_t, e) = engine().await;
        let mut p = e.config().current().providers[0].clone();
        p.label = "Renamed".into();
        let c = upsert(&e, p.clone()).unwrap();
        assert_eq!(c.providers.len(), 2);
        assert_eq!(c.providers[0].label, "Renamed");
        p.id = ProviderId("new".into());
        let c = upsert(&e, p).unwrap();
        assert_eq!(c.providers.len(), 3);
    }

    #[tokio::test]
    async fn delete_of_a_referenced_provider_is_rejected_and_config_unchanged() {
        let (_t, e) = engine().await;
        let asr = e.config().current().providers[0].id.clone();
        let err = delete(&e, &asr).unwrap_err();
        assert!(err.message.contains("unknown provider"), "{}", err.message);
        assert_eq!(e.config().current().providers.len(), 2);
        let mut extra = e.config().current().providers[1].clone();
        extra.id = ProviderId("spare".into());
        upsert(&e, extra).unwrap();
        let c = delete(&e, &ProviderId("spare".into())).unwrap();
        assert_eq!(c.providers.len(), 2);
    }
}
