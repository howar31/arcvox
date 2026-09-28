use kongwe_core::SecretStore;
use tauri::State;

use super::CmdResult;
use crate::app::AppState;
use crate::secrets::KeychainStore;

#[tauri::command]
pub fn secrets_set(_state: State<'_, AppState>, key_ref: String, value: String) -> CmdResult<()> {
    if value.trim().is_empty() {
        KeychainStore::new().delete(&key_ref)?;
    } else {
        KeychainStore::new().set(&key_ref, value.trim())?;
    }
    Ok(())
}

#[tauri::command]
pub fn secrets_has(_state: State<'_, AppState>, key_ref: String) -> CmdResult<bool> {
    Ok(KeychainStore::new().get(&key_ref)?.is_some())
}

#[tauri::command]
pub fn secrets_delete(_state: State<'_, AppState>, key_ref: String) -> CmdResult<()> {
    KeychainStore::new().delete(&key_ref)?;
    Ok(())
}
