//! Menu-bar icon and menu.

use std::sync::Mutex;

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Runtime};

use crate::dictation::ui::UiState;
use crate::i18n::{text, Language, Msg};

const ICON_IDLE: Image<'static> = tauri::include_image!("./icons/tray-idle.png");
const ICON_RECORDING: Image<'static> = tauri::include_image!("./icons/tray-recording.png");
const ICON_PROCESSING: Image<'static> = tauri::include_image!("./icons/tray-processing.png");
const ICON_PROBLEM: Image<'static> = tauri::include_image!("./icons/tray-problem.png");

pub const ID_STATUS: &str = "status";
pub const ID_FIX: &str = "fix";
pub const ID_COPY_LAST: &str = "copy-last";
pub const ID_SETTINGS: &str = "settings";
pub const ID_QUIT: &str = "quit";
/// The action that resolves what the status line reports, if it reports a problem.
fn fix_msg(state: UiState) -> Option<Msg> {
    match state {
        UiState::NoAccessibility => Some(Msg::TrayFixAccessibility),
        UiState::EngineError => Some(Msg::TrayFixEngine),
        _ => None,
    }
}

pub struct Tray<R: Runtime> {
    app: AppHandle<R>,
    icon: TrayIcon<R>,
    /// The status line of the menu now shown; replaced with every new menu.
    status: Mutex<MenuItem<R>>,
    /// What the menu was last built from: a change of language or state
    /// builds it again from these.
    last_state: Mutex<UiState>,
    language: Mutex<Language>,
}

fn menu<R: Runtime>(
    app: &AppHandle<R>,
    language: Language,
    state: UiState,
) -> tauri::Result<(Menu<R>, MenuItem<R>)> {
    let status = MenuItem::with_id(
        app,
        ID_STATUS,
        text(language, status_msg(state)),
        false,
        None::<&str>,
    )?;
    let menu = Menu::with_items(app, &[&status])?;
    if let Some(msg) = fix_msg(state) {
        menu.append(&MenuItem::with_id(
            app,
            ID_FIX,
            text(language, msg),
            true,
            None::<&str>,
        )?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        ID_COPY_LAST,
        text(language, Msg::TrayCopyLast),
        true,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        ID_SETTINGS,
        text(language, Msg::TraySettings),
        true,
        Some("CmdOrCtrl+,"),
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        ID_QUIT,
        text(language, Msg::TrayQuit),
        true,
        None::<&str>,
    )?)?;
    Ok((menu, status))
}

impl Tray<tauri::Wry> {
    pub fn build(app: &AppHandle<tauri::Wry>, language: Language) -> tauri::Result<Self> {
        let (first, status) = menu(app, language, UiState::Idle)?;
        let icon = TrayIconBuilder::with_id("arcvox")
            .icon(ICON_IDLE)
            .icon_as_template(true)
            .menu(&first)
            .show_menu_on_left_click(true)
            .tooltip("Arcvox")
            .on_menu_event(|app, event| crate::app::tray_action(app, event.id().as_ref()))
            .build(app)?;
        Ok(Self {
            app: app.clone(),
            icon,
            status: Mutex::new(status),
            last_state: Mutex::new(UiState::Idle),
            language: Mutex::new(language),
        })
    }
}

impl<R: Runtime> Tray<R> {
    /// Builds the menu again from the stored language and state.
    fn rebuild(&self) {
        let language = *self.language.lock().unwrap();
        let state = *self.last_state.lock().unwrap();
        let Ok((next, status)) = menu(&self.app, language, state) else {
            return;
        };
        let _ = self.icon.set_menu(Some(next));
        *self.status.lock().unwrap() = status;
    }

    pub fn set_state(&self, state: UiState, language: Language) {
        let before = std::mem::replace(&mut *self.last_state.lock().unwrap(), state);
        let _ = self.icon.set_icon(Some(status_icon(state)));
        // Replacing the image resets the template flag on macOS; restore it so
        // the icon follows the menu bar's light or dark appearance.
        let _ = self.icon.set_icon_as_template(true);
        if fix_msg(before) != fix_msg(state) {
            // The line under the status appears or goes away.
            self.rebuild();
        } else {
            let _ = self
                .status
                .lock()
                .unwrap()
                .set_text(text(language, status_msg(state)));
        }
    }

    /// Shows every label in the new language. The status line keeps showing
    /// the state it last displayed.
    pub fn set_language(&self, language: Language) {
        *self.language.lock().unwrap() = language;
        self.rebuild();
    }

    /// The state the status line shows.
    pub fn state(&self) -> UiState {
        *self.last_state.lock().unwrap()
    }
}

fn status_icon(state: UiState) -> Image<'static> {
    match state {
        UiState::Idle | UiState::Inserting => ICON_IDLE,
        UiState::Recording | UiState::Locked => ICON_RECORDING,
        UiState::Processing => ICON_PROCESSING,
        UiState::NoAccessibility | UiState::EngineError => ICON_PROBLEM,
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
    "flow",
    "engines",
    "general",
    "recording",
    "data",
    "advanced",
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
    fn a_problem_shows_in_the_icon_itself() {
        assert_ne!(
            status_icon(UiState::NoAccessibility).rgba(),
            status_icon(UiState::Idle).rgba()
        );
        assert_eq!(
            status_icon(UiState::EngineError).rgba(),
            status_icon(UiState::NoAccessibility).rgba()
        );
        assert_eq!(
            status_icon(UiState::Inserting).rgba(),
            status_icon(UiState::Idle).rgba()
        );
    }

    #[test]
    fn only_a_problem_gets_an_action_under_the_status() {
        assert_eq!(fix_msg(UiState::Idle), None);
        assert_eq!(fix_msg(UiState::Recording), None);
        assert_eq!(
            fix_msg(UiState::NoAccessibility),
            Some(Msg::TrayFixAccessibility)
        );
        assert_eq!(fix_msg(UiState::EngineError), Some(Msg::TrayFixEngine));
    }

    #[test]
    fn settings_pages_are_reachable_and_unknown_pages_are_not() {
        for page in [
            "home",
            "flow",
            "engines",
            "speed",
            "general",
            "models",
            "providers",
        ] {
            assert!(SETTINGS_PAGES.contains(&page), "{page}");
        }
        assert!(!SETTINGS_PAGES.contains(&"javascript:alert(1)"));
    }
}
