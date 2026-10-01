//! Application menu, and the rule that quitting takes two requests so one
//! stray keystroke does not end dictation.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::i18n::{text, Language, Msg};
use crate::ui::window_state::SETTINGS_WINDOW;

pub const ID_QUIT: &str = "app-quit";
/// How long after the first press a second press quits.
const CONFIRM_WINDOW: Duration = Duration::from_secs(3);

/// Tells a confirming second press from a first one.
#[derive(Default)]
struct QuitConfirm {
    armed_at: Mutex<Option<Instant>>,
}

impl QuitConfirm {
    /// True when this press confirms an earlier one; otherwise the press
    /// arms the confirmation.
    fn press(&self, now: Instant) -> bool {
        let mut armed_at = self.armed_at.lock().unwrap();
        let confirmed = armed_at.is_some_and(|at| now.duration_since(at) <= CONFIRM_WINDOW);
        *armed_at = if confirmed { None } else { Some(now) };
        confirmed
    }
}

pub struct AppMenu<R: Runtime> {
    quit: MenuItem<R>,
    confirm: QuitConfirm,
}

impl<R: Runtime> AppMenu<R> {
    /// Builds the menu and installs it as the application's menu.
    pub fn install(app: &AppHandle<R>, language: Language) -> tauri::Result<Self> {
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
                &Submenu::with_items(
                    app,
                    app.package_info().name.clone(),
                    true,
                    &[
                        &PredefinedMenuItem::about(app, None, None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::services(app, None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::hide(app, None)?,
                        &PredefinedMenuItem::hide_others(app, None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &quit,
                    ],
                )?,
                &Submenu::with_items(
                    app,
                    "Edit",
                    true,
                    &[
                        &PredefinedMenuItem::undo(app, None)?,
                        &PredefinedMenuItem::redo(app, None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::cut(app, None)?,
                        &PredefinedMenuItem::copy(app, None)?,
                        &PredefinedMenuItem::paste(app, None)?,
                        &PredefinedMenuItem::select_all(app, None)?,
                    ],
                )?,
                &Submenu::with_items(
                    app,
                    "Window",
                    true,
                    &[
                        &PredefinedMenuItem::minimize(app, None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::close_window(app, None)?,
                    ],
                )?,
            ],
        )?;
        app.set_menu(menu)?;
        Ok(Self {
            quit,
            confirm: QuitConfirm::default(),
        })
    }

    pub fn set_language(&self, language: Language) {
        let _ = self.quit.set_text(text(language, Msg::TrayQuit));
    }

    /// Handles a request to quit. The first request shows a hint in the
    /// settings window and returns false; a second one within the
    /// confirmation window returns true. A hidden or minimized settings
    /// window is brought back first so the hint has a place to appear.
    pub fn quit_requested(&self, app: &AppHandle<R>) -> bool {
        if self.confirm.press(Instant::now()) {
            return true;
        }
        if let Some(w) = app.get_webview_window(SETTINGS_WINDOW) {
            let _ = w.unminimize();
        }
        crate::ui::tray::open_settings(app, None);
        let _ = app.emit_to(
            SETTINGS_WINDOW,
            "quit-armed",
            CONFIRM_WINDOW.as_millis() as u64,
        );
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_press_within_the_window_confirms() {
        let confirm = QuitConfirm::default();
        let start = Instant::now();
        assert!(!confirm.press(start));
        assert!(confirm.press(start + Duration::from_secs(2)));
    }

    #[test]
    fn a_late_second_press_arms_again() {
        let confirm = QuitConfirm::default();
        let start = Instant::now();
        assert!(!confirm.press(start));
        let late = start + CONFIRM_WINDOW + Duration::from_millis(1);
        assert!(!confirm.press(late));
        assert!(confirm.press(late + Duration::from_secs(1)));
    }

    #[test]
    fn a_confirmed_press_clears_the_state() {
        let confirm = QuitConfirm::default();
        let start = Instant::now();
        assert!(!confirm.press(start));
        assert!(confirm.press(start));
        assert!(!confirm.press(start));
    }
}
