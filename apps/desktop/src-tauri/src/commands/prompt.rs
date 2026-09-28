use tauri::State;

use super::{engine, CmdResult};
use crate::app::AppState;

#[tauri::command]
pub fn prompt_get(state: State<'_, AppState>) -> CmdResult<String> {
    Ok(engine(&state)?.config().base_prompt()?)
}

#[tauri::command]
pub fn prompt_set(state: State<'_, AppState>, text: String) -> CmdResult<()> {
    engine(&state)?.config().set_base_prompt(&text)?;
    Ok(())
}

#[tauri::command]
pub fn prompt_restore(state: State<'_, AppState>) -> CmdResult<String> {
    Ok(engine(&state)?.config().restore_base_prompt()?)
}
