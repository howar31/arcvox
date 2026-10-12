use arcvox_core::config::{Config, Profile};
use arcvox_core::profile::{preset, presets, reset_target};
use arcvox_core::types::ProfileId;
use arcvox_core::Engine;
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
        return Err(CmdError::new(
            "default_profile_protected",
            "the default profile cannot be deleted",
        ));
    }
    let next = engine
        .config()
        .update(|c| c.profiles.retain(|p| &p.id != id))?;
    Ok((*next).clone())
}

fn unknown_preset(preset_id: &str) -> CmdError {
    CmdError::with(
        "unknown_preset",
        format!("unknown preset {preset_id}"),
        &[("preset", preset_id.to_string())],
    )
}

/// Puts a profile's settings back to those of a preset: `preset_id` when
/// given (the page offers the standard one when the profile's own preset is
/// gone), else the preset the profile was made from; see
/// `Profile::reset_to_preset` for what changes.
pub fn reset(engine: &Engine, id: &ProfileId, preset_id: Option<&str>) -> CmdResult<Config> {
    let current = engine.config().current();
    let profile = current
        .profiles
        .iter()
        .find(|p| &p.id == id)
        .ok_or_else(|| CmdError::new("unknown_profile", format!("unknown profile {}", id.0)))?;
    let target = match preset_id {
        Some(pid) => preset(pid).ok_or_else(|| unknown_preset(pid))?,
        None => reset_target(profile)
            .ok_or_else(|| unknown_preset(profile.preset.as_deref().unwrap_or("")))?,
    };
    let next = engine.config().update(|c| {
        let providers = c.providers.clone();
        if let Some(p) = c.profiles.iter_mut().find(|p| &p.id == id) {
            p.reset_to_preset(target, &providers);
        }
    })?;
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
        .ok_or_else(|| unknown_preset(preset_id))?;
    let current = engine.config().current();
    let base = current
        .profiles
        .iter()
        .find(|p| p.is_default)
        .ok_or_else(|| CmdError::new("no_default_profile", "no default profile"))?;
    let taken: Vec<ProfileId> = current.profiles.iter().map(|p| p.id.clone()).collect();
    upsert(
        engine,
        Profile::from_preset(preset, name.to_string(), base, &current.providers, &taken),
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
pub fn profiles_reset(
    state: State<'_, AppState>,
    id: ProfileId,
    preset: Option<String>,
) -> CmdResult<Config> {
    reset(&engine(&state)?, &id, preset.as_deref())
}

#[tauri::command]
pub fn profiles_set_default(state: State<'_, AppState>, id: ProfileId) -> CmdResult<Config> {
    set_default(&engine(&state)?, &id)
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
        let c = add_preset(&e, "formal", "正式").unwrap();
        let mail = c
            .profiles
            .iter()
            .find(|p| p.id == ProfileId("formal".into()))
            .unwrap();
        assert_eq!(mail.name, "正式");
        assert!(!mail.is_default);
        assert_eq!(mail.asr_provider, base.asr_provider);
        assert_eq!(mail.llm_provider, base.llm_provider);
        assert!(!mail.matcher.app_ids.is_empty());
        // A second copy gets a free id instead of replacing the first.
        let c = add_preset(&e, "formal", "正式 2").unwrap();
        assert_eq!(c.profiles.len(), 3);
        assert!(add_preset(&e, "nope", "x").is_err());
    }

    #[tokio::test]
    async fn reset_returns_to_the_profile_preset_or_the_one_named() {
        let (_t, e) = engine().await;
        let c = add_preset(&e, "verbatim", "逐字").unwrap();
        let mut chat = c
            .profiles
            .iter()
            .find(|p| p.id.0 == "verbatim")
            .unwrap()
            .clone();
        chat.prompt_addition = "tuned".into();
        chat.llm_window_title = false;
        upsert(&e, chat).unwrap();
        let c = reset(&e, &ProfileId("verbatim".into()), None).unwrap();
        let chat = c.profiles.iter().find(|p| p.id.0 == "verbatim").unwrap();
        assert_eq!(
            chat.prompt_addition,
            preset("verbatim").unwrap().prompt_addition
        );
        assert!(chat.llm_window_title);
        // A profile whose preset is gone is refused unless a preset is named.
        let mut gone = chat.clone();
        gone.preset = Some("retired".into());
        upsert(&e, gone).unwrap();
        let err = reset(&e, &ProfileId("verbatim".into()), None).unwrap_err();
        assert_eq!(err.code, Some("unknown_preset"));
        let c = reset(&e, &ProfileId("verbatim".into()), Some("casual")).unwrap();
        let chat = c.profiles.iter().find(|p| p.id.0 == "verbatim").unwrap();
        assert_eq!(chat.prompt_addition, "");
        assert_eq!(chat.preset.as_deref(), Some("casual"));
        assert_eq!(chat.name, "逐字");
        assert_eq!(
            reset(&e, &ProfileId("verbatim".into()), Some("nope"))
                .unwrap_err()
                .code,
            Some("unknown_preset")
        );
    }

    #[test]
    fn presets_list_every_builtin_preset() {
        let views = preset_views();
        let ids: Vec<&str> = views.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, ["casual", "formal", "verbatim"]);
        // Only the clean copy of the factory settings names no application.
        assert!(views
            .iter()
            .all(|v| v.id != "formal" || !v.app_ids.is_empty()));
    }
}
