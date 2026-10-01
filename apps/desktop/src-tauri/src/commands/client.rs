use tauri::State;

use super::CmdResult;
use crate::app::AppState;
use crate::i18n::{self, Language};
use crate::settings::ClientSettings;
use crate::shortcut::{refusal, sanitized, Key, Refusal, Shortcut};

#[tauri::command]
pub fn client_get(state: State<'_, AppState>) -> ClientSettings {
    ClientSettings::load(&state.dirs.config_dir)
}

#[tauri::command]
pub fn client_update(
    state: State<'_, AppState>,
    mut settings: ClientSettings,
) -> CmdResult<ClientSettings> {
    settings.hotkey.shortcuts = sanitized(&settings.hotkey.shortcuts);
    settings.save(&state.dirs.config_dir)?;
    state.controller.update_settings(settings.clone());
    let language = Language::from_setting(
        settings.ui_language,
        i18n::system_preferred_language().as_deref(),
    );
    *state.language.lock().unwrap() = language;
    state.ui.set_language(language);
    state.ui.set_scale(settings.widget_size.scale());
    Ok(settings)
}

/// Why these keys cannot be added to `existing` as a recording shortcut;
/// `None` when they can.
#[tauri::command]
pub fn shortcut_check(keys: Vec<Key>, existing: Vec<Shortcut>) -> Option<Refusal> {
    refusal(&keys, &existing)
}

/// Starts or ends recording a new shortcut. While it runs the keys pressed
/// arrive as `shortcut-capture` events and no dictation starts. False when
/// keys cannot be recorded (the Accessibility permission is missing).
#[tauri::command]
pub fn shortcut_capture(state: State<'_, AppState>, on: bool) -> bool {
    state.platform.set_shortcut_capture(on)
}
