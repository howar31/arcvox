use kongwe_core::config::{Config, Profile};
use kongwe_core::profile::presets;
use kongwe_core::types::ProfileId;
use kongwe_core::Engine;
use serde::Serialize;
use tauri::State;

use super::{engine, CmdError, CmdResult};
use crate::app::AppState;

pub fn upsert(engine: &Engine, profile: Profile) -> CmdResult<Config> {
    let next = engine.config().update(|c| {
        if profile.is_default {
            for p in c.profiles.iter_mut() {
                p.is_default = false;
            }
        }
        match c.profiles.iter_mut().find(|p| p.id == profile.id) {
            Some(existing) => *existing = profile.clone(),
            None => c.profiles.push(profile.clone()),
        }
    })?;
    Ok((*next).clone())
}

pub fn delete(engine: &Engine, id: &ProfileId) -> CmdResult<Config> {
    let current = engine.config().current();
    if current.profiles.iter().any(|p| &p.id == id && p.is_default) {
        return Err(CmdError {
            message: "the default profile cannot be deleted".into(),
        });
    }
    let next = engine
        .config()
        .update(|c| c.profiles.retain(|p| &p.id != id))?;
    Ok((*next).clone())
}

pub fn set_default(engine: &Engine, id: &ProfileId) -> CmdResult<Config> {
    let next = engine.config().update(|c| {
        for p in c.profiles.iter_mut() {
            p.is_default = &p.id == id;
        }
    })?;
    Ok((*next).clone())
}

/// A built-in preset as the page lists it; the page supplies the display
/// name, since names are translated.
#[derive(Debug, Clone, Serialize)]
pub struct PresetView {
    pub id: String,
    pub app_ids: Vec<String>,
}

pub fn preset_views() -> Vec<PresetView> {
    presets()
        .iter()
        .map(|p| PresetView {
            id: p.id.to_string(),
            app_ids: p.app_ids.iter().map(|a| a.to_string()).collect(),
        })
        .collect()
}

/// Adds a profile built from the preset `preset_id`, using the default
/// profile's providers. The id gets a numeric suffix while taken.
pub fn add_preset(engine: &Engine, preset_id: &str, name: &str) -> CmdResult<Config> {
    let preset = presets()
        .iter()
        .find(|p| p.id == preset_id)
        .ok_or_else(|| CmdError {
            message: format!("unknown preset {preset_id}"),
        })?;
    let current = engine.config().current();
    let base = current
        .profiles
        .iter()
        .find(|p| p.is_default)
        .ok_or_else(|| CmdError {
            message: "no default profile".into(),
        })?;
    let taken: Vec<ProfileId> = current.profiles.iter().map(|p| p.id.clone()).collect();
    upsert(
        engine,
        Profile::from_preset(preset, name.to_string(), base, &taken),
    )
}

#[tauri::command]
pub fn profiles_presets() -> CmdResult<Vec<PresetView>> {
    Ok(preset_views())
}

#[tauri::command]
pub fn profiles_add_preset(
    state: State<'_, AppState>,
    preset_id: String,
    name: String,
) -> CmdResult<Config> {
    add_preset(&engine(&state)?, &preset_id, &name)
}

#[tauri::command]
pub fn profiles_upsert(state: State<'_, AppState>, profile: Profile) -> CmdResult<Config> {
    upsert(&engine(&state)?, profile)
}

#[tauri::command]
pub fn profiles_delete(state: State<'_, AppState>, id: ProfileId) -> CmdResult<Config> {
    delete(&engine(&state)?, &id)
}

#[tauri::command]
pub fn profiles_set_default(state: State<'_, AppState>, id: ProfileId) -> CmdResult<Config> {
    set_default(&engine(&state)?, &id)
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
    async fn exactly_one_default_is_kept() {
        let (_t, e) = engine().await;
        let mut second = e.config().current().profiles[0].clone();
        second.id = ProfileId("slack".into());
        second.name = "Slack".into();
        second.is_default = false;
        let c = upsert(&e, second.clone()).unwrap();
        assert_eq!(c.profiles.len(), 2);
        let c = set_default(&e, &ProfileId("slack".into())).unwrap();
        assert_eq!(c.profiles.iter().filter(|p| p.is_default).count(), 1);
        assert!(c.profiles[1].is_default);
        let err = delete(&e, &ProfileId("slack".into())).unwrap_err();
        assert!(err.message.contains("default"));
        set_default(&e, &ProfileId("default".into())).unwrap();
        let c = delete(&e, &ProfileId("slack".into())).unwrap();
        assert_eq!(c.profiles.len(), 1);
        let unknown = set_default(&e, &ProfileId("nope".into())).unwrap_err();
        assert!(
            unknown.message.contains("default profile"),
            "{}",
            unknown.message
        );
    }

    #[tokio::test]
    async fn add_preset_creates_a_profile_with_the_default_pair() {
        let (_t, e) = engine().await;
        let base = e.config().current().profiles[0].clone();
        let c = add_preset(&e, "mail", "郵件").unwrap();
        let mail = c
            .profiles
            .iter()
            .find(|p| p.id == ProfileId("mail".into()))
            .unwrap();
        assert_eq!(mail.name, "郵件");
        assert!(!mail.is_default);
        assert_eq!(mail.asr_provider, base.asr_provider);
        assert_eq!(mail.llm_provider, base.llm_provider);
        assert!(!mail.matcher.app_ids.is_empty());
        // A second copy gets a free id instead of replacing the first.
        let c = add_preset(&e, "mail", "郵件 2").unwrap();
        assert_eq!(c.profiles.len(), 3);
        assert!(add_preset(&e, "nope", "x").is_err());
    }

    #[test]
    fn presets_list_every_builtin_preset() {
        let views = preset_views();
        let ids: Vec<&str> = views.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, ["mail", "chat", "code"]);
        assert!(views.iter().all(|v| !v.app_ids.is_empty()));
    }
}
