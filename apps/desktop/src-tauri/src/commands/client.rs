use arcvox_core::config::General;
use serde::Serialize;
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

/// The settings of a new installation, for the way back to them that the
/// settings pages offer.
#[derive(Serialize)]
pub struct Defaults {
    client: ClientSettings,
    general: General,
}

#[tauri::command]
pub fn settings_defaults() -> Defaults {
    Defaults {
        client: ClientSettings::default(),
        general: General::default(),
    }
}

/// Stores the settings and makes every part of the app follow them.
pub(crate) fn apply_client(
    state: &AppState,
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
    state.ui.set_display(settings.widget_display.clone());
    state.ui.set_idle(settings.widget_idle);
    state.ui.set_fade(settings.widget_fade);
    state.backup.touch();
    Ok(settings)
}

#[tauri::command]
pub fn client_update(
    state: State<'_, AppState>,
    settings: ClientSettings,
) -> CmdResult<ClientSettings> {
    apply_client(&state, settings)
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
