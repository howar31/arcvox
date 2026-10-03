use arcvox_core::SecretStore;
use tauri::{AppHandle, Wry};

use super::CmdResult;
use crate::identity;
use crate::secrets::KeychainStore;

/// The store of this build: its keys live under the app's own name.
fn store(app: &AppHandle<Wry>) -> KeychainStore {
    KeychainStore::new(identity::name(app.package_info()))
}

#[tauri::command]
pub fn secrets_set(app: AppHandle<Wry>, key_ref: String, value: String) -> CmdResult<()> {
    if value.trim().is_empty() {
        store(&app).delete(&key_ref)?;
    } else {
        store(&app).set(&key_ref, value.trim())?;
    }
    Ok(())
}

#[tauri::command]
pub fn secrets_has(app: AppHandle<Wry>, key_ref: String) -> CmdResult<bool> {
    Ok(store(&app).get(&key_ref)?.is_some())
}

#[tauri::command]
pub fn secrets_delete(app: AppHandle<Wry>, key_ref: String) -> CmdResult<()> {
    store(&app).delete(&key_ref)?;
    Ok(())
}
