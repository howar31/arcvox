use arcvox_core::config::{Config, General};
use tauri::State;

use super::{engine, CmdResult};
use crate::app::AppState;

#[tauri::command]
pub fn config_get(state: State<'_, AppState>) -> CmdResult<Config> {
    Ok((*engine(&state)?.config().current()).clone())
}

#[tauri::command]
pub fn config_update_general(state: State<'_, AppState>, general: General) -> CmdResult<Config> {
    let e = engine(&state)?;
    let next = e.config().update(|c| c.general = general)?;
    Ok((*next).clone())
}
