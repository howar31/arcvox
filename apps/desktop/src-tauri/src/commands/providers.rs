use std::collections::BTreeMap;

use arcvox_core::checks::{EngineCheck, LocalService};
use arcvox_core::config::{validate, Config, EngineRole, ProviderConfig, ProviderKind};
use arcvox_core::models::{ModelRole, ServicePreset};
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

/// A hand-made engine goes only when nothing depends on it: not the
/// default pair, and no profile that names it.
pub fn delete(engine: &Engine, id: &ProviderId) -> CmdResult<Config> {
    let current = engine.config().current();
    if current
        .provider(id)
        .is_some_and(|p| p.kind != ProviderKind::Http)
    {
        return Err(automatic());
    }
    if &current.default_engines.asr == id || &current.default_engines.llm == id {
        return Err(CmdError::new(
            "engine_is_default",
            "the engine is one of the default pair; choose another default first",
        ));
    }
    let users: Vec<String> = current
        .profiles
        .iter()
        .filter(|p| p.asr_provider.as_ref() == Some(id) || p.llm_provider.as_ref() == Some(id))
        .map(|p| p.name.clone())
        .collect();
    if !users.is_empty() {
        return Err(CmdError::with(
            "engine_in_use",
            format!("the engine is used by {}", users.join(", ")),
            &[("profiles", users.join(", "))],
        ));
    }
    let next = engine
        .config()
        .update(|c| c.providers.retain(|p| &p.id != id))?;
    Ok((*next).clone())
}

/// Sets one engine of the default pair; the engine must exist and serve
/// the role, which validation checks.
pub fn set_default(engine: &Engine, role: EngineRole, id: &ProviderId) -> CmdResult<Config> {
    let next = engine.config().update(|c| match role {
        EngineRole::Asr => c.default_engines.asr = id.clone(),
        EngineRole::Llm => c.default_engines.llm = id.clone(),
    })?;
    debug_assert!(validate(&next).is_ok());
    Ok((*next).clone())
}

#[tauri::command]
pub fn default_engine_set(
    state: State<'_, AppState>,
    role: EngineRole,
    id: ProviderId,
) -> CmdResult<Config> {
    set_default(&engine(&state)?, role, &id)
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

/// The catalogue's network services, for the engine form.
#[tauri::command]
pub fn services_list(state: State<'_, AppState>) -> CmdResult<Vec<ServicePreset>> {
    Ok(engine(&state)?.services())
}

/// Opens the page where a key for the catalogue service is made. Only an
/// https address the catalogue gives is opened.
#[tauri::command]
pub fn services_open_key_page(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let url = engine(&state)?
        .services()
        .into_iter()
        .find(|s| s.id == id)
        .and_then(|s| s.key_url)
        .filter(|u| u.starts_with("https://"))
        .ok_or_else(|| CmdError::new("unknown_service", format!("no key page for {id}")))?;
    std::process::Command::new("open").arg(url).spawn()?;
    Ok(())
}

/// Opens the service's terms from the catalogue, an https address only.
#[tauri::command]
pub fn services_open_terms_page(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let url = engine(&state)?
        .services()
        .into_iter()
        .find(|s| s.id == id)
        .and_then(|s| s.terms_url)
        .filter(|u| u.starts_with("https://"))
        .ok_or_else(|| CmdError::new("unknown_service", format!("no terms page for {id}")))?;
    std::process::Command::new("open").arg(url).spawn()?;
    Ok(())
}

/// The catalogue's local services, each asked for its models.
#[tauri::command]
pub async fn services_scan_local(state: State<'_, AppState>) -> CmdResult<Vec<LocalService>> {
    let e = engine(&state)?;
    Ok(e.scan_local_services().await)
}

/// One real request to a network engine for the role; the result is not
/// stored here (a new engine has no id yet).
#[tauri::command]
pub async fn providers_verify(
    state: State<'_, AppState>,
    provider: ProviderConfig,
    role: ModelRole,
) -> CmdResult<EngineCheck> {
    if provider.kind != ProviderKind::Http {
        return Err(automatic());
    }
    let e = engine(&state)?;
    Ok(e.verify_provider(&provider, role).await)
}

#[tauri::command]
pub fn engine_checks(state: State<'_, AppState>) -> CmdResult<BTreeMap<String, EngineCheck>> {
    Ok(engine(&state)?.engine_checks())
}

#[tauri::command]
pub fn engine_check_put(
    state: State<'_, AppState>,
    id: ProviderId,
    check: EngineCheck,
) -> CmdResult<()> {
    engine(&state)?
        .put_engine_check(&id.0, check)
        .map_err(|e| CmdError::new("engine_check_write", e.to_string()))?;
    state.refresh_problems();
    Ok(())
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
            .update(|c| c.profiles[0].asr_provider = Some(ProviderId("remote".into())))
            .unwrap();
        let err = delete(&e, &ProviderId("remote".into())).unwrap_err();
        assert_eq!(err.code, Some("engine_in_use"), "{}", err.message);
        assert!(err.message.contains("Default"), "{}", err.message);
        assert_eq!(e.config().current().providers.len(), 3);
        upsert(&e, http("spare")).unwrap();
        let c = delete(&e, &ProviderId("spare".into())).unwrap();
        assert_eq!(c.providers.len(), 3);
    }

    #[tokio::test]
    async fn the_default_pair_is_set_by_role_and_cannot_be_deleted() {
        let (_t, e) = engine().await;
        upsert(&e, http("remote")).unwrap();
        let c = set_default(&e, EngineRole::Asr, &ProviderId("remote".into())).unwrap();
        assert_eq!(c.default_engines.asr, ProviderId("remote".into()));
        let err = delete(&e, &ProviderId("remote".into())).unwrap_err();
        assert_eq!(err.code, Some("engine_is_default"));
        // An engine of the wrong kind is refused by validation.
        let err = set_default(&e, EngineRole::Llm, &ProviderId("nope".into())).unwrap_err();
        assert_eq!(err.code, Some("config_unknown_default_engine"));
    }
}
