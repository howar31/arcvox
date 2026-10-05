//! Menu-bar icon and menu.

use std::sync::Mutex;

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Runtime};

use crate::dictation::ui::UiState;
use crate::i18n::{text, Language, Msg};
use crate::identity;
use crate::settings::WidgetIdle;

const ICON_IDLE: Image<'static> = tauri::include_image!("./icons/tray-idle.png");
const ICON_RECORDING: Image<'static> = tauri::include_image!("./icons/tray-recording.png");
const ICON_PROCESSING: Image<'static> = tauri::include_image!("./icons/tray-processing.png");
const ICON_PROBLEM: Image<'static> = tauri::include_image!("./icons/tray-problem.png");

pub const ID_APP: &str = "app";
pub const ID_STATUS: &str = "status";
pub const ID_FIX: &str = "fix";
pub const ID_FIX_BACKUP: &str = "fix-backup";
pub const ID_COPY_LAST: &str = "copy-last";
pub const ID_HISTORY: &str = "history";
pub const ID_STATISTICS: &str = "statistics";
pub const ID_SETTINGS: &str = "settings";
pub const ID_QUIT: &str = "quit";
pub const ID_PLACE: &str = "widget-place";

/// The id of the item that chooses what the widget shows while idle.
fn idle_id(idle: WidgetIdle) -> String {
    format!("widget-idle-{}", idle.name())
}

/// The choice an item of the widget's idle menu stands for.
pub fn idle_of(id: &str) -> Option<WidgetIdle> {
    WidgetIdle::ALL.into_iter().find(|i| idle_id(*i) == id)
}

fn idle_msg(idle: WidgetIdle) -> Msg {
    match idle {
        WidgetIdle::Hidden => Msg::TrayWidgetIdleHidden,
        WidgetIdle::Ring => Msg::TrayWidgetIdleRing,
        WidgetIdle::Label => Msg::TrayWidgetIdleLabel,
    }
}

/// The action that resolves what the status line reports, if it reports a problem.
fn fix_msg(state: UiState) -> Option<Msg> {
    match state {
        UiState::NoAccessibility => Some(Msg::TrayFixAccessibility),
        UiState::EngineError => Some(Msg::TrayFixEngine),
        _ => None,
    }
}

/// A problem that does not stop dictation. Each one puts the badge on the
/// icon and gets a line in the menu with an item that leads to its fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    BackupFolderMissing,
    BackupFolderNotWritable,
}

impl Notice {
    /// What is wrong, as a line of the menu.
    fn line(self) -> Msg {
        match self {
            Notice::BackupFolderMissing => Msg::TrayBackupFolderMissing,
            Notice::BackupFolderNotWritable => Msg::TrayBackupFolderNotWritable,
        }
    }

    /// The item under the line: its id and its label.
    fn fix(self) -> (&'static str, Msg) {
        match self {
            Notice::BackupFolderMissing | Notice::BackupFolderNotWritable => {
                (ID_FIX_BACKUP, Msg::TrayFixBackup)
            }
        }
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
    idle: Mutex<WidgetIdle>,
    placing: Mutex<bool>,
    notices: Mutex<Vec<Notice>>,
}

/// The menu of the menu-bar icon; the widget opens the same one. What its
/// items do is in `app::tray_action`.
pub(super) fn menu<R: Runtime>(
    app: &AppHandle<R>,
    language: Language,
    state: UiState,
    idle: WidgetIdle,
    placing: bool,
    notices: &[Notice],
) -> tauri::Result<(Menu<R>, MenuItem<R>)> {
    // The first line names the app and the running version, so the version
    // can be read without opening the settings window.
    let header = MenuItem::with_id(
        app,
        ID_APP,
        identity::name_and_version(app.package_info()),
        false,
        None::<&str>,
    )?;
    let status = MenuItem::with_id(
        app,
        ID_STATUS,
        text(language, status_msg(state)),
        false,
        None::<&str>,
    )?;
    let menu = Menu::with_items(
        app,
        &[&header, &PredefinedMenuItem::separator(app)?, &status],
    )?;
    if let Some(msg) = fix_msg(state) {
        menu.append(&MenuItem::with_id(
            app,
            ID_FIX,
            text(language, msg),
            true,
            None::<&str>,
        )?)?;
    }
    for notice in notices {
        let (id, label) = notice.fix();
        menu.append(&PredefinedMenuItem::separator(app)?)?;
        menu.append(&MenuItem::new(
            app,
            text(language, notice.line()),
            false,
            None::<&str>,
        )?)?;
        menu.append(&MenuItem::with_id(
            app,
            id,
            text(language, label),
            true,
            None::<&str>,
        )?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    // The results of dictation: the latest one, or all of them.
    menu.append(&MenuItem::with_id(
        app,
        ID_COPY_LAST,
        text(language, Msg::TrayCopyLast),
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        ID_HISTORY,
        text(language, Msg::TrayHistory),
        true,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    // The widget: kept on screen to be dragged into place (ticked while it is),
    // and what it shows while no dictation goes on (the choice in use is ticked).
    menu.append(&CheckMenuItem::with_id(
        app,
        ID_PLACE,
        text(language, Msg::TrayWidgetPlace),
        true,
        placing,
        None::<&str>,
    )?)?;
    let choices = Submenu::with_id(
        app,
        "widget-idle",
        text(language, Msg::TrayWidgetIdle),
        true,
    )?;
    for choice in WidgetIdle::ALL {
        choices.append(&CheckMenuItem::with_id(
            app,
            idle_id(choice),
            text(language, idle_msg(choice)),
            true,
            choice == idle,
            None::<&str>,
        )?)?;
    }
    menu.append(&choices)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    // The windows.
    menu.append(&MenuItem::with_id(
        app,
        ID_STATISTICS,
        text(language, Msg::TrayStatistics),
        true,
        None::<&str>,
    )?)?;
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
        let (first, status) = menu(
            app,
            language,
            UiState::Idle,
            WidgetIdle::default(),
            false,
            &[],
        )?;
        let icon = TrayIconBuilder::with_id("arcvox")
            .icon(ICON_IDLE)
            .icon_as_template(true)
            .menu(&first)
            .show_menu_on_left_click(true)
            .tooltip(identity::name(app.package_info()))
            .on_menu_event(|app, event| crate::app::tray_action(app, event.id().as_ref()))
            .build(app)?;
        Ok(Self {
            app: app.clone(),
            icon,
            status: Mutex::new(status),
            last_state: Mutex::new(UiState::Idle),
            language: Mutex::new(language),
            idle: Mutex::new(WidgetIdle::default()),
            placing: Mutex::new(false),
            notices: Mutex::new(Vec::new()),
        })
    }
}

impl<R: Runtime> Tray<R> {
    /// Builds the menu again from the stored language and state.
    fn rebuild(&self) {
        let language = *self.language.lock().unwrap();
        let state = *self.last_state.lock().unwrap();
        let idle = *self.idle.lock().unwrap();
        let placing = *self.placing.lock().unwrap();
        let notices = self.notices();
        let Ok((next, status)) = menu(&self.app, language, state, idle, placing, &notices) else {
            return;
        };
        let _ = self.icon.set_menu(Some(next));
        *self.status.lock().unwrap() = status;
    }

    pub fn set_state(&self, state: UiState, language: Language) {
        let before = std::mem::replace(&mut *self.last_state.lock().unwrap(), state);
        self.show_icon(state);
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

    /// Ticks the item that keeps the widget on screen to be positioned.
    pub fn set_placing(&self, placing: bool) {
        *self.placing.lock().unwrap() = placing;
        self.rebuild();
    }

    /// Ticks the choice in use for what the widget shows while idle.
    pub fn set_idle(&self, idle: WidgetIdle) {
        *self.idle.lock().unwrap() = idle;
        self.rebuild();
    }

    /// The problems that do not stop dictation, as they are now.
    pub fn set_notices(&self, notices: Vec<Notice>) {
        let before = std::mem::replace(&mut *self.notices.lock().unwrap(), notices.clone());
        if before == notices {
            return;
        }
        self.show_icon(self.state());
        self.rebuild();
    }

    pub fn notices(&self) -> Vec<Notice> {
        self.notices.lock().unwrap().clone()
    }

    fn show_icon(&self, state: UiState) {
        let noticed = !self.notices.lock().unwrap().is_empty();
        let _ = self.icon.set_icon(Some(status_icon(state, noticed)));
        // Replacing the image resets the template flag on macOS; restore it so
        // the icon follows the menu bar's light or dark appearance.
        let _ = self.icon.set_icon_as_template(true);
    }

    /// The state the status line shows.
    pub fn state(&self) -> UiState {
        *self.last_state.lock().unwrap()
    }
}

/// `noticed`: something needs attention although dictation works. It shows
/// while nothing is being recorded or processed.
fn status_icon(state: UiState, noticed: bool) -> Image<'static> {
    match state {
        UiState::Idle | UiState::Inserting if noticed => ICON_PROBLEM,
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
    "backup",
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
            status_icon(UiState::NoAccessibility, false).rgba(),
            status_icon(UiState::Idle, false).rgba()
        );
        assert_eq!(
            status_icon(UiState::EngineError, false).rgba(),
            status_icon(UiState::NoAccessibility, false).rgba()
        );
        assert_eq!(
            status_icon(UiState::Inserting, false).rgba(),
            status_icon(UiState::Idle, false).rgba()
        );
    }

    #[test]
    fn a_problem_that_leaves_dictation_working_shows_while_idle() {
        let problem = status_icon(UiState::NoAccessibility, false);
        assert_eq!(status_icon(UiState::Idle, true).rgba(), problem.rgba());
        assert_eq!(status_icon(UiState::Inserting, true).rgba(), problem.rgba());
        // A dictation in progress keeps its own icon.
        for state in [UiState::Recording, UiState::Locked, UiState::Processing] {
            assert_eq!(
                status_icon(state, true).rgba(),
                status_icon(state, false).rgba()
            );
        }
    }

    #[test]
    fn every_notice_has_a_line_and_an_item_that_leads_to_its_fix() {
        for notice in [Notice::BackupFolderMissing, Notice::BackupFolderNotWritable] {
            assert_eq!(notice.fix(), (ID_FIX_BACKUP, Msg::TrayFixBackup));
            for language in [Language::ZhTw, Language::En] {
                assert!(!text(language, notice.line()).is_empty());
            }
        }
        assert_ne!(
            Notice::BackupFolderMissing.line(),
            Notice::BackupFolderNotWritable.line()
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
