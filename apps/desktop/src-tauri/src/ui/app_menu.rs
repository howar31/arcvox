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
    app: AppHandle<R>,
    confirm: QuitConfirm,
}

/// Labels of the menu, Traditional Chinese then English.
struct Labels {
    about: &'static str,
    services: &'static str,
    hide: &'static str,
    hide_others: &'static str,
    edit: &'static str,
    undo: &'static str,
    redo: &'static str,
    cut: &'static str,
    copy: &'static str,
    paste: &'static str,
    select_all: &'static str,
    window: &'static str,
    minimize: &'static str,
    close_window: &'static str,
}

fn labels(language: Language) -> Labels {
    match language {
        Language::ZhTw => Labels {
            about: "關於 Kongwe",
            services: "服務",
            hide: "隱藏 Kongwe",
            hide_others: "隱藏其他",
            edit: "編輯",
            undo: "還原",
            redo: "重做",
            cut: "剪下",
            copy: "複製",
            paste: "貼上",
            select_all: "全選",
            window: "視窗",
            minimize: "縮到最小",
            close_window: "關閉視窗",
        },
        Language::En => Labels {
            about: "About Kongwe",
            services: "Services",
            hide: "Hide Kongwe",
            hide_others: "Hide Others",
            edit: "Edit",
            undo: "Undo",
            redo: "Redo",
            cut: "Cut",
            copy: "Copy",
            paste: "Paste",
            select_all: "Select All",
            window: "Window",
            minimize: "Minimize",
            close_window: "Close Window",
        },
    }
}

fn build<R: Runtime>(app: &AppHandle<R>, language: Language) -> tauri::Result<Menu<R>> {
    let l = labels(language);
    let quit = MenuItem::with_id(
        app,
        ID_QUIT,
        text(language, Msg::TrayQuit),
        true,
        Some("CmdOrCtrl+Q"),
    )?;
    Menu::with_items(
        app,
        &[
            &Submenu::with_items(
                app,
                app.package_info().name.clone(),
                true,
                &[
                    &PredefinedMenuItem::about(app, Some(l.about), None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::services(app, Some(l.services))?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::hide(app, Some(l.hide))?,
                    &PredefinedMenuItem::hide_others(app, Some(l.hide_others))?,
                    &PredefinedMenuItem::separator(app)?,
                    &quit,
                ],
            )?,
            &Submenu::with_items(
                app,
                l.edit,
                true,
                &[
                    &PredefinedMenuItem::undo(app, Some(l.undo))?,
                    &PredefinedMenuItem::redo(app, Some(l.redo))?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::cut(app, Some(l.cut))?,
                    &PredefinedMenuItem::copy(app, Some(l.copy))?,
                    &PredefinedMenuItem::paste(app, Some(l.paste))?,
                    &PredefinedMenuItem::select_all(app, Some(l.select_all))?,
                ],
            )?,
            &Submenu::with_items(
                app,
                l.window,
                true,
                &[
                    &PredefinedMenuItem::minimize(app, Some(l.minimize))?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::close_window(app, Some(l.close_window))?,
                ],
            )?,
        ],
    )
}

impl<R: Runtime> AppMenu<R> {
    /// Builds the menu and installs it as the application's menu.
    pub fn install(app: &AppHandle<R>, language: Language) -> tauri::Result<Self> {
        app.set_menu(build(app, language)?)?;
        Ok(Self {
            app: app.clone(),
            confirm: QuitConfirm::default(),
        })
    }

    /// The menu is built again in the new language.
    pub fn set_language(&self, language: Language) {
        if let Ok(menu) = build(&self.app, language) {
            let _ = self.app.set_menu(menu);
        }
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
