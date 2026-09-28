use tauri::State;

use super::CmdResult;
use crate::app::AppState;
use crate::i18n::{self, Language};
use crate::settings::ClientSettings;

#[tauri::command]
pub fn client_get(state: State<'_, AppState>) -> ClientSettings {
    ClientSettings::load(&state.dirs.config_dir)
}

#[tauri::command]
pub fn client_update(
    state: State<'_, AppState>,
    settings: ClientSettings,
) -> CmdResult<ClientSettings> {
    settings.save(&state.dirs.config_dir)?;
    state.controller.update_settings(settings.clone());
    let language = Language::from_setting(
        settings.ui_language,
        i18n::system_preferred_language().as_deref(),
    );
    *state.language.lock().unwrap() = language;
    state.ui.set_language(language);
    Ok(settings)
}
