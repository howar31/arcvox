//! Menu-bar icon and menu.

use std::sync::Mutex;

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_autostart::ManagerExt;

use crate::dictation::ui::UiState;
use crate::i18n::{text, Language, Msg};

const ICON_IDLE: Image<'static> = tauri::include_image!("./icons/tray-idle.png");
const ICON_RECORDING: Image<'static> = tauri::include_image!("./icons/tray-recording.png");
const ICON_PROCESSING: Image<'static> = tauri::include_image!("./icons/tray-processing.png");

pub const ID_STATUS: &str = "status";
pub const ID_SETTINGS: &str = "settings";
pub const ID_HISTORY: &str = "history";
pub const ID_MODELS: &str = "models";
pub const ID_AUTOSTART: &str = "autostart";
pub const ID_QUIT: &str = "quit";

pub struct Tray<R: Runtime> {
    icon: TrayIcon<R>,
    status: MenuItem<R>,
    settings: MenuItem<R>,
    history: MenuItem<R>,
    models: MenuItem<R>,
    autostart: CheckMenuItem<R>,
    quit: MenuItem<R>,
    /// The status label depends on both the last `UiState` and the current
    /// language; `set_language` needs the former to re-render the label
    /// when only the language changes.
    last_state: Mutex<UiState>,
}

impl<R: Runtime> Tray<R> {
    pub fn build(app: &AppHandle<R>, language: Language) -> tauri::Result<Self> {
        let status = MenuItem::with_id(
            app,
            ID_STATUS,
            text(language, Msg::TrayStatusIdle),
            false,
            None::<&str>,
        )?;
        let settings = MenuItem::with_id(
            app,
            ID_SETTINGS,
            text(language, Msg::TraySettings),
            true,
            Some("CmdOrCtrl+,"),
        )?;
        let history = MenuItem::with_id(
            app,
            ID_HISTORY,
            text(language, Msg::TrayHistory),
            true,
            None::<&str>,
        )?;
        let models = MenuItem::with_id(
            app,
            ID_MODELS,
            text(language, Msg::TrayModels),
            true,
            None::<&str>,
        )?;
        let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
        let autostart = CheckMenuItem::with_id(
            app,
            ID_AUTOSTART,
            text(language, Msg::TrayLaunchAtLogin),
            true,
            autostart_on,
            None::<&str>,
        )?;
        let quit = MenuItem::with_id(
            app,
            ID_QUIT,
            text(language, Msg::TrayQuit),
            true,
            Some("CmdOrCtrl+Q"),
        )?;
        let menu = Menu::with_items(
            app,
            &[
                &status,
                &PredefinedMenuItem::separator(app)?,
                &settings,
                &history,
                &models,
                &autostart,
                &PredefinedMenuItem::separator(app)?,
                &quit,
            ],
        )?;
        let icon = TrayIconBuilder::with_id("arcvox")
            .icon(ICON_IDLE)
            .icon_as_template(true)
            .menu(&menu)
            .show_menu_on_left_click(true)
            .tooltip("Arcvox")
            .on_menu_event(|app, event| match event.id().as_ref() {
                ID_SETTINGS => open_settings(app, None),
                ID_HISTORY => open_settings(app, Some("history")),
                ID_MODELS => open_settings(app, Some("engines")),
                ID_AUTOSTART => {
                    let al = app.autolaunch();
                    let on = al.is_enabled().unwrap_or(false);
                    let _ = if on { al.disable() } else { al.enable() };
                    if let Some(state) = app.try_state::<crate::app::AppState>() {
                        state.ui.tray().set_autostart(!on);
                    }
                }
                ID_QUIT => app.exit(0),
                _ => {}
            })
            .build(app)?;
        Ok(Self {
            icon,
            status,
            settings,
            history,
            models,
            autostart,
            quit,
            last_state: Mutex::new(UiState::Idle),
        })
    }

    pub fn set_state(&self, state: UiState, language: Language) {
        *self.last_state.lock().unwrap() = state;
        let _ = self.icon.set_icon(Some(status_icon(state)));
        // Replacing the image resets the template flag on macOS; restore it so
        // the icon follows the menu bar's light or dark appearance.
        let _ = self.icon.set_icon_as_template(true);
        let _ = self.status.set_text(text(language, status_msg(state)));
    }

    pub fn set_autostart(&self, on: bool) {
        let _ = self.autostart.set_checked(on);
    }

    /// Re-renders every menu item's label in the new language. The status
    /// item keeps showing whatever `UiState` it last displayed; only the
    /// wording changes.
    pub fn set_language(&self, language: Language) {
        let state = *self.last_state.lock().unwrap();
        let _ = self.status.set_text(text(language, status_msg(state)));
        let _ = self.settings.set_text(text(language, Msg::TraySettings));
        let _ = self.history.set_text(text(language, Msg::TrayHistory));
        let _ = self.models.set_text(text(language, Msg::TrayModels));
        let _ = self
            .autostart
            .set_text(text(language, Msg::TrayLaunchAtLogin));
        let _ = self.quit.set_text(text(language, Msg::TrayQuit));
    }
}

fn status_icon(state: UiState) -> Image<'static> {
    match state {
        UiState::Idle | UiState::Inserting | UiState::NoAccessibility | UiState::EngineError => {
            ICON_IDLE
        }
        UiState::Recording | UiState::Locked => ICON_RECORDING,
        UiState::Processing => ICON_PROCESSING,
    }
}

fn status_msg(state: UiState) -> Msg {
    match state {
        UiState::Idle | UiState::Inserting => Msg::TrayStatusIdle,
        UiState::Recording | UiState::Locked => Msg::TrayStatusRecording,
        UiState::Processing => Msg::TrayStatusProcessing,
        UiState::NoAccessibility => Msg::TrayStatusNoAccessibility,
        UiState::EngineError => Msg::TrayStatusEngineError,
    }
}

/// Settings pages that can be selected through `open_settings`'s `page`
/// argument. Anything else is ignored rather than interpolated into `eval`.
const SETTINGS_PAGES: &[&str] = &[
    "home",
    "engines",
    "general",
    "providers",
    "profiles",
    "prompt",
    "dictionary",
    "history",
    "models",
    "speed",
    "about",
];

/// Shows the settings window; `page` selects a page through a URL hash.
/// While the window is open the app uses the regular activation policy so it
/// has a Dock icon and appears in the application switcher.
pub fn open_settings<R: Runtime>(app: &AppHandle<R>, page: Option<&str>) {
    let page = page
        .filter(|p| SETTINGS_PAGES.contains(p))
        .map(str::to_string);
    let handle = app.clone();
    // Activation policy and window calls must run on the main thread.
    let _ = app.run_on_main_thread(move || {
        #[cfg(target_os = "macos")]
        let _ = handle.set_activation_policy(tauri::ActivationPolicy::Regular);
        if let Some(w) = handle.get_webview_window("settings") {
            if let Some(p) = page {
                let _ = w.eval(format!("window.location.hash = '#{p}'"));
            }
            let _ = w.show();
            let _ = w.set_focus();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_pages_are_reachable_and_unknown_pages_are_not() {
        for page in ["home", "engines", "speed", "general", "models", "providers"] {
            assert!(SETTINGS_PAGES.contains(&page), "{page}");
        }
        assert!(!SETTINGS_PAGES.contains(&"javascript:alert(1)"));
    }
}
