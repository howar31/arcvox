use tauri::{AppHandle, State, Wry};

use super::CmdResult;
use crate::app::AppState;

#[tauri::command]
pub fn ui_language(state: State<'_, AppState>) -> String {
    state.language.lock().unwrap().tag().to_string()
}

#[tauri::command]
pub fn widget_copy(state: State<'_, AppState>, text: String) -> CmdResult<()> {
    state.platform.clipboard_write_text(&text);
    Ok(())
}

#[tauri::command]
pub fn system_open_settings_window(app: AppHandle<Wry>, page: Option<String>) {
    crate::ui::tray::open_settings(&app, page.as_deref());
}
