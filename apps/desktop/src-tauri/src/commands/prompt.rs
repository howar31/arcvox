use serde::Serialize;
use tauri::State;

use super::{engine, CmdResult};
use crate::app::AppState;

/// The rewrite's rules, from `prompts/base.md`.
#[tauri::command]
pub fn prompt_get(state: State<'_, AppState>) -> CmdResult<String> {
    Ok(engine(&state)?.config().base_prompt()?)
}

#[tauri::command]
pub fn prompt_set(state: State<'_, AppState>, text: String) -> CmdResult<()> {
    engine(&state)?.config().set_base_prompt(&text)?;
    Ok(())
}

/// The rewrite's rules of a new installation.
#[tauri::command]
pub fn prompt_default() -> &'static str {
    arcvox_core::config::base_prompt::DEFAULT_BASE_PROMPT
}

#[tauri::command]
pub fn prompt_restore(state: State<'_, AppState>) -> CmdResult<String> {
    Ok(engine(&state)?.config().restore_base_prompt()?)
}

/// The openings the app sends before the rules; shown on the page, never
/// edited.
#[derive(Serialize)]
pub struct FixedPrompts {
    rewrite: &'static str,
    translate: &'static str,
}

#[tauri::command]
pub fn prompt_fixed() -> FixedPrompts {
    FixedPrompts {
        rewrite: arcvox_core::prompt::REWRITE_HEAD,
        translate: arcvox_core::steps::translate::TRANSLATE_HEAD,
    }
}

/// The translation's rules, from `prompts/translate.md`.
#[tauri::command]
pub fn translate_prompt_get(state: State<'_, AppState>) -> CmdResult<String> {
    Ok(engine(&state)?.config().translate_prompt()?)
}

#[tauri::command]
pub fn translate_prompt_set(state: State<'_, AppState>, text: String) -> CmdResult<()> {
    engine(&state)?.config().set_translate_prompt(&text)?;
    Ok(())
}

#[tauri::command]
pub fn translate_prompt_default() -> &'static str {
    arcvox_core::config::base_prompt::DEFAULT_TRANSLATE_PROMPT
}

#[tauri::command]
pub fn translate_prompt_restore(state: State<'_, AppState>) -> CmdResult<String> {
    Ok(engine(&state)?.config().restore_translate_prompt()?)
}
