use kongwe_core::config::{Config, General};
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
    // The recordings folder changes only together with the move of the
    // recordings, through its own command.
    let next = e.config().update(|c| {
        let recordings_dir = c.general.recordings_dir.take();
        c.general = general;
        c.general.recordings_dir = recordings_dir;
    })?;
    Ok((*next).clone())
}
