use arcvox_core::config::{Config, DictionaryEntry};
use arcvox_core::Engine;
use tauri::State;

use super::{engine, CmdResult};
use crate::app::AppState;

pub fn set(engine: &Engine, entries: Vec<DictionaryEntry>) -> CmdResult<Config> {
    let next = engine.config().update(|c| c.dictionary = entries.clone())?;
    Ok((*next).clone())
}

#[tauri::command]
pub fn dictionary_set(
    state: State<'_, AppState>,
    entries: Vec<DictionaryEntry>,
) -> CmdResult<Config> {
    set(&engine(&state)?, entries)
}
