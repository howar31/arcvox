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

/// The base prompt of a new installation.
#[tauri::command]
pub fn prompt_default() -> &'static str {
    arcvox_core::config::base_prompt::DEFAULT_BASE_PROMPT
}

#[tauri::command]
pub fn prompt_restore(state: State<'_, AppState>) -> CmdResult<String> {
    Ok(engine(&state)?.config().restore_base_prompt()?)
}
