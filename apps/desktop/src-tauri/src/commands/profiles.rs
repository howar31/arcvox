use arcvox_core::config::{ensure_referenced_engines, Config, Profile};
use arcvox_core::models::Manifest;
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
pub fn reset(
    engine: &Engine,
    id: &ProfileId,
    preset_id: Option<&str>,
    manifest: &Manifest,
) -> CmdResult<Config> {
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
        if let Some(p) = c.profiles.iter_mut().find(|p| &p.id == id) {
            p.reset_to_preset(target);
        }
        ensure_referenced_engines(c, manifest);
    })?;
    Ok((*next).clone())
}

/// Settles a profile whose template's writing style has moved on since the
/// profile last took it: `take` puts the template's current style into the
/// profile, else the profile keeps its own style and the question is
/// closed. A profile that is current already is left as it is.
pub fn template_style(engine: &Engine, id: &ProfileId, take: bool) -> CmdResult<Config> {
    let current = engine.config().current();
    if !current.profiles.iter().any(|p| &p.id == id) {
        return Err(CmdError::new(
            "unknown_profile",
            format!("unknown profile {}", id.0),
        ));
    }
    let next = engine.config().update(|c| {
        if let Some(p) = c.profiles.iter_mut().find(|p| &p.id == id) {
            if let Some(preset) = p.moved_template() {
                if take {
                    p.take_template_style(preset);
                } else {
                    p.keep_own_style(preset);
                }
            }
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
    /// The engines the template was tuned with, the ones the Profiles page
    /// marks as suggested; None for an untuned template.
    pub suggested_asr: Option<String>,
    pub suggested_llm: Option<String>,
    /// The revision of the template's writing style; a profile whose
    /// `preset_revision` is lower has not taken the current one.
    pub revision: u32,
}

pub fn preset_views() -> Vec<PresetView> {
    presets()
        .iter()
        .map(|p| {
            let tuned = p.tuned_with();
            PresetView {
                id: p.id.to_string(),
                app_ids: p.app_ids.iter().map(|a| a.to_string()).collect(),
                suggested_asr: tuned.map(|(asr, _)| asr.to_string()),
                suggested_llm: tuned.map(|(_, llm)| llm.to_string()),
                revision: p.revision(),
            }
        })
        .collect()
}

/// Adds a copy of the template `preset_id`. The id gets a numeric suffix
/// while taken; the template's engines are made when they are not there.
pub fn add_preset(
    engine: &Engine,
    preset_id: &str,
    name: &str,
    manifest: &Manifest,
) -> CmdResult<Config> {
    let preset = presets()
        .iter()
        .find(|p| p.id == preset_id)
        .ok_or_else(|| unknown_preset(preset_id))?;
    let current = engine.config().current();
    let taken: Vec<ProfileId> = current.profiles.iter().map(|p| p.id.clone()).collect();
    let profile = Profile::from_preset(preset, name.to_string(), &taken);
    let next = engine.config().update(|c| {
        c.profiles.push(profile.clone());
        ensure_referenced_engines(c, manifest);
    })?;
    Ok((*next).clone())
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
    let (manifest, _) = state.models.manifest_now();
    let result = add_preset(&engine(&state)?, &preset_id, &name, &manifest);
    followed(&state, result)
}

/// A profile that stops using an engine may leave that engine without a
/// model and without a user; the engines are looked at again.
fn followed(state: &State<'_, AppState>, result: CmdResult<Config>) -> CmdResult<Config> {
    result?;
    state.sync_engines();
    Ok(engine(state)?.config().current().as_ref().clone())
}

#[tauri::command]
pub fn profiles_upsert(state: State<'_, AppState>, profile: Profile) -> CmdResult<Config> {
    let result = upsert(&engine(&state)?, profile);
    followed(&state, result)
}

#[tauri::command]
pub fn profiles_delete(state: State<'_, AppState>, id: ProfileId) -> CmdResult<Config> {
    let result = delete(&engine(&state)?, &id);
    followed(&state, result)
}

#[tauri::command]
pub fn profiles_reset(
    state: State<'_, AppState>,
    id: ProfileId,
    preset: Option<String>,
) -> CmdResult<Config> {
    let (manifest, _) = state.models.manifest_now();
    let result = reset(&engine(&state)?, &id, preset.as_deref(), &manifest);
    followed(&state, result)
}

#[tauri::command]
pub fn profiles_set_default(state: State<'_, AppState>, id: ProfileId) -> CmdResult<Config> {
    set_default(&engine(&state)?, &id)
}

#[tauri::command]
pub fn profiles_template_style(
    state: State<'_, AppState>,
    id: ProfileId,
    take: bool,
) -> CmdResult<Config> {
    template_style(&engine(&state)?, &id, take)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcvox_core::config::ProviderConfig;
    use arcvox_core::types::ProviderId;
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

    fn manifest() -> Manifest {
        Manifest::parse(crate::providers::BUNDLED_MANIFEST).unwrap()
    }

    #[tokio::test]
    async fn add_preset_copies_the_template_and_makes_its_engines() {
        let (_t, e) = engine().await;
        // The default pair moved to another engine; the local pair's rows
        // are gone from the list.
        e.config()
            .update(|c| {
                c.providers.push(ProviderConfig::http_test_provider(
                    "remote",
                    "http://localhost:1/v1",
                    true,
                    true,
                ));
                c.default_engines.asr = ProviderId("remote".into());
                c.default_engines.llm = ProviderId("remote".into());
                c.providers.retain(|p| p.id.0 == "remote");
            })
            .unwrap();
        let c = add_preset(&e, "formal", "正式", &manifest()).unwrap();
        let mail = c
            .profiles
            .iter()
            .find(|p| p.id == ProfileId("formal".into()))
            .unwrap();
        assert_eq!(mail.name, "正式");
        assert!(!mail.is_default);
        // A tuned template names the engines it was tuned with, and the
        // app makes their rows again from the catalogue.
        assert_eq!(
            mail.asr_provider,
            Some(ProviderId(arcvox_core::config::MODEL_ID_DEFAULT_ASR.into()))
        );
        assert!(c
            .providers
            .iter()
            .any(|p| p.id.0 == arcvox_core::config::MODEL_ID_DEFAULT_ASR));
        assert!(!mail.matcher.app_ids.is_empty());
        // Its model is not installed here, so the default serves meanwhile.
        assert_eq!(e.resolve_engines(mail).asr.id, ProviderId("remote".into()));
        // A second copy gets a free id instead of replacing the first.
        let c = add_preset(&e, "formal", "正式 2", &manifest()).unwrap();
        assert_eq!(c.profiles.len(), 3);
        assert!(add_preset(&e, "nope", "x", &manifest()).is_err());
    }

    #[tokio::test]
    async fn reset_returns_to_the_profile_preset_or_the_one_named() {
        let (_t, e) = engine().await;
        let c = add_preset(&e, "verbatim", "逐字", &manifest()).unwrap();
        let mut chat = c
            .profiles
            .iter()
            .find(|p| p.id.0 == "verbatim")
            .unwrap()
            .clone();
        chat.prompt_addition = "tuned".into();
        chat.llm_window_title = false;
        upsert(&e, chat).unwrap();
        let c = reset(&e, &ProfileId("verbatim".into()), None, &manifest()).unwrap();
        let chat = c.profiles.iter().find(|p| p.id.0 == "verbatim").unwrap();
        assert_eq!(
            chat.prompt_addition,
            preset("verbatim").unwrap().prompt_addition()
        );
        assert!(chat.llm_window_title);
        // A profile whose preset is gone is refused unless a preset is named.
        let mut gone = chat.clone();
        gone.preset = Some("retired".into());
        upsert(&e, gone).unwrap();
        let err = reset(&e, &ProfileId("verbatim".into()), None, &manifest()).unwrap_err();
        assert_eq!(err.code, Some("unknown_preset"));
        let c = reset(
            &e,
            &ProfileId("verbatim".into()),
            Some("casual"),
            &manifest(),
        )
        .unwrap();
        let chat = c.profiles.iter().find(|p| p.id.0 == "verbatim").unwrap();
        assert_eq!(
            chat.prompt_addition,
            preset("casual").unwrap().prompt_addition()
        );
        assert_eq!(chat.preset.as_deref(), Some("casual"));
        assert_eq!(chat.name, "逐字");
        assert_eq!(
            reset(&e, &ProfileId("verbatim".into()), Some("nope"), &manifest())
                .unwrap_err()
                .code,
            Some("unknown_preset")
        );
    }

    #[test]
    fn presets_list_every_builtin_preset() {
        let views = preset_views();
        let ids: Vec<&str> = views.iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, ["casual", "formal", "verbatim", "blank"]);
        // The formal template names its applications.
        assert!(views
            .iter()
            .all(|v| v.id != "formal" || !v.app_ids.is_empty()));
    }
}
