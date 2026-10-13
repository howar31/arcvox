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
    // The recordings and models folders change only together with the move
    // of their files, through their own commands.
    let next = e.config().update(|c| {
        let recordings_dir = c.general.recordings_dir.take();
        let models_dir = c.general.models_dir.take();
        c.general = general;
        c.general.recordings_dir = recordings_dir;
        c.general.models_dir = models_dir;
    })?;
    Ok((*next).clone())
}
