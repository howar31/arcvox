use arcvox_core::config::{Config, Profile};
use arcvox_core::types::ProfileId;
use arcvox_core::Engine;
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
}
