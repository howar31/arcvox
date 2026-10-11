//! Menu-bar icon and menu.

use std::sync::Mutex;

use arcvox_core::types::ProfileId;

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Manager, Runtime};

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
pub const ID_FIX_PERMISSIONS: &str = "fix-permissions";
pub const ID_FIX_FLOW: &str = "fix-flow";
pub const ID_FIX_ENGINES: &str = "fix-engines";
/// Tells the settings window which of its pages have a problem to show.
pub const PROBLEM_PAGES_EVENT: &str = "problem-pages";
/// Where every prompt about a permission leads: the one place that opens
/// the system's settings.
pub const PERMISSIONS_PAGE: &str = "general@permissions";
pub const ID_COPY_LAST: &str = "copy-last";
pub const ID_HISTORY: &str = "history";
pub const ID_STATISTICS: &str = "statistics";
pub const ID_SETTINGS: &str = "settings";
pub const ID_ABOUT: &str = "about";
pub const ID_QUIT: &str = "quit";
pub const ID_PLACE: &str = "widget-place";
pub const ID_PROFILES_MANAGE: &str = "profiles-manage";
const PROFILE_PREFIX: &str = "profile:";

/// A profile as the menu lists it: picking one makes it the default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileEntry {
    pub id: ProfileId,
    pub name: String,
    pub is_default: bool,
}

fn profile_item_id(id: &ProfileId) -> String {
    format!("{PROFILE_PREFIX}{}", id.0)
}

/// The profile an item of the profiles menu stands for.
pub fn profile_of(id: &str) -> Option<ProfileId> {
    id.strip_prefix(PROFILE_PREFIX)
        .map(|rest| ProfileId(rest.to_string()))
}

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
        UiState::NoAccessibility => Some(Msg::TrayFixPermissions),
        UiState::EngineError => Some(Msg::TrayFixEngine),
        _ => None,
    }
}

/// A problem that does not stop dictation. Each one puts the badge on the
/// icon and gets a line in the menu, followed by an item that leads to its
/// fix. They are listed in this order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Notice {
    MicrophoneDenied,
    MicrophoneRestricted,
    /// A profile, by name, that names an engine that is gone.
    ProfileEngineMissing(String),
    /// A profile, by name, whose engine has no model to run.
    ProfileModelMissing(String),
    /// An engine, by name, whose own model file is gone.
    ModelFileMissing(String),
    BackupFolderMissing,
    BackupFolderNotWritable,
}

impl Notice {
    /// What is wrong, as a line of the menu.
    fn line(&self, language: Language) -> String {
        let (msg, name) = match self {
            Notice::MicrophoneDenied => (Msg::TrayMicrophoneDenied, None),
            Notice::MicrophoneRestricted => (Msg::TrayMicrophoneRestricted, None),
            Notice::ProfileEngineMissing(n) => (Msg::TrayProfileEngineMissing, Some(n)),
            Notice::ProfileModelMissing(n) => (Msg::TrayProfileModelMissing, Some(n)),
            Notice::ModelFileMissing(n) => (Msg::TrayModelFileMissing, Some(n)),
            Notice::BackupFolderMissing => (Msg::TrayBackupFolderMissing, None),
            Notice::BackupFolderNotWritable => (Msg::TrayBackupFolderNotWritable, None),
        };
        match name {
            Some(name) => text(language, msg).replace("{name}", name),
            None => text(language, msg).to_string(),
        }
    }

    /// The item that leads to the fix: its id and its label.
    fn fix(&self) -> (&'static str, Msg) {
        match self {
            Notice::MicrophoneDenied | Notice::MicrophoneRestricted => {
                (ID_FIX_PERMISSIONS, Msg::TrayFixPermissions)
            }
            Notice::ProfileEngineMissing(_) | Notice::ProfileModelMissing(_) => {
                (ID_FIX_FLOW, Msg::TrayFixFlow)
            }
            Notice::ModelFileMissing(_) => (ID_FIX_ENGINES, Msg::TrayFixEngine),
            Notice::BackupFolderMissing | Notice::BackupFolderNotWritable => {
                (ID_FIX_BACKUP, Msg::TrayFixBackup)
            }
        }
    }

    /// The settings page where the problem shows and is resolved.
    fn page(&self) -> &'static str {
        match self.fix().0 {
            ID_FIX_PERMISSIONS => "general",
            ID_FIX_FLOW => "flow",
            ID_FIX_ENGINES => "engines",
            _ => "backup",
        }
    }

    pub fn about_permissions(&self) -> bool {
        self.fix().0 == ID_FIX_PERMISSIONS
    }

    pub fn about_backup(&self) -> bool {
        self.fix().0 == ID_FIX_BACKUP
    }

    pub fn about_engines(&self) -> bool {
        matches!(self.fix().0, ID_FIX_FLOW | ID_FIX_ENGINES)
    }
}

/// The notices whose line goes under the status line, before its item:
/// with the status already about a permission, one item serves them all.
fn under_status(state: UiState, notices: &[Notice]) -> Vec<Notice> {
    if state != UiState::NoAccessibility {
        return Vec::new();
    }
    notices
        .iter()
        .filter(|n| n.about_permissions())
        .cloned()
        .collect()
}

/// The notices that follow, in runs that share the item leading to their fix.
fn runs(notices: &[Notice]) -> Vec<Vec<&Notice>> {
    let mut out: Vec<Vec<&Notice>> = Vec::new();
    for notice in notices {
        match out.last_mut() {
            Some(run) if run[0].fix() == notice.fix() => run.push(notice),
            _ => out.push(vec![notice]),
        }
    }
    out
}

/// The settings pages that have a problem to show, each once.
fn problem_pages(state: UiState, notices: &[Notice]) -> Vec<&'static str> {
    let mut pages = Vec::new();
    if state == UiState::NoAccessibility {
        pages.push("general");
    }
    for notice in notices {
        if !pages.contains(&notice.page()) {
            pages.push(notice.page());
        }
    }
    pages
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
    profiles: Mutex<Vec<ProfileEntry>>,
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
    profiles: &[ProfileEntry],
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
    let joined = under_status(state, notices);
    for notice in &joined {
        menu.append(&MenuItem::new(
            app,
            notice.line(language),
            false,
            None::<&str>,
        )?)?;
    }
    if let Some(msg) = fix_msg(state) {
        menu.append(&MenuItem::with_id(
            app,
            ID_FIX,
            text(language, msg),
            true,
            None::<&str>,
        )?)?;
    }
    let rest: Vec<Notice> = notices
        .iter()
        .filter(|n| !joined.contains(n))
        .cloned()
        .collect();
    for run in runs(&rest) {
        let (id, label) = run[0].fix();
        menu.append(&PredefinedMenuItem::separator(app)?)?;
        for notice in run {
            menu.append(&MenuItem::new(
                app,
                notice.line(language),
                false,
                None::<&str>,
            )?)?;
        }
        menu.append(&MenuItem::with_id(
            app,
            id,
            text(language, label),
            true,
            None::<&str>,
        )?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    // The results of dictation: the latest one, all of them, and their figures.
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
    menu.append(&MenuItem::with_id(
        app,
        ID_STATISTICS,
        text(language, Msg::TrayStatistics),
        true,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    // The default profile: ticked, picking another makes it the default; the
    // last item manages the profiles.
    let profiles_menu = Submenu::with_id(app, "profiles", text(language, Msg::TrayProfiles), true)?;
    for p in profiles {
        profiles_menu.append(&CheckMenuItem::with_id(
            app,
            profile_item_id(&p.id),
            &p.name,
            true,
            p.is_default,
            None::<&str>,
        )?)?;
    }
    if !profiles.is_empty() {
        profiles_menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    profiles_menu.append(&MenuItem::with_id(
        app,
        ID_PROFILES_MANAGE,
        text(language, Msg::TrayProfilesManage),
        true,
        None::<&str>,
    )?)?;
    menu.append(&profiles_menu)?;
    // The widget: kept on screen to be dragged into place (ticked while it is),
    // and what it shows while no dictation goes on (the choice in use is ticked).
    let widget_menu = Submenu::with_id(app, "widget", text(language, Msg::TrayWidget), true)?;
    widget_menu.append(&CheckMenuItem::with_id(
        app,
        ID_PLACE,
        text(language, Msg::TrayWidgetPlace),
        true,
        placing,
        None::<&str>,
    )?)?;
    widget_menu.append(&PredefinedMenuItem::separator(app)?)?;
    widget_menu.append(&MenuItem::new(
        app,
        text(language, Msg::TrayWidgetIdle),
        false,
        None::<&str>,
    )?)?;
    for choice in WidgetIdle::ALL {
        widget_menu.append(&CheckMenuItem::with_id(
            app,
            idle_id(choice),
            text(language, idle_msg(choice)),
            true,
            choice == idle,
            None::<&str>,
        )?)?;
    }
    menu.append(&widget_menu)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    // The app itself: about, settings (and, later, updates).
    menu.append(&MenuItem::with_id(
        app,
        ID_ABOUT,
        text(language, Msg::TrayAbout),
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
            profiles: Mutex::new(Vec::new()),
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
        let profiles = self.profiles.lock().unwrap().clone();
        let Ok((next, status)) = menu(
            &self.app, language, state, idle, placing, &notices, &profiles,
        ) else {
            return;
        };
        let _ = self.icon.set_menu(Some(next));
        *self.status.lock().unwrap() = status;
    }

    pub fn set_state(&self, state: UiState, language: Language) {
        let before = std::mem::replace(&mut *self.last_state.lock().unwrap(), state);
        self.show_icon(state);
        if (before == UiState::NoAccessibility) != (state == UiState::NoAccessibility) {
            self.announce_pages();
        }
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

    /// The profiles as the menu lists them; the menu is built again when
    /// they changed.
    pub fn set_profiles(&self, profiles: Vec<ProfileEntry>) {
        let before = std::mem::replace(&mut *self.profiles.lock().unwrap(), profiles.clone());
        if before != profiles {
            self.rebuild();
        }
    }

    /// The problems that do not stop dictation, as they are now.
    pub fn set_notices(&self, notices: Vec<Notice>) {
        let before = std::mem::replace(&mut *self.notices.lock().unwrap(), notices.clone());
        if before == notices {
            return;
        }
        self.show_icon(self.state());
        self.rebuild();
        self.announce_pages();
    }

    /// The settings pages that have a problem to show.
    pub fn problem_pages(&self) -> Vec<&'static str> {
        problem_pages(self.state(), &self.notices())
    }

    fn announce_pages(&self) {
        let _ = self
            .app
            .emit_to("settings", PROBLEM_PAGES_EVENT, self.problem_pages());
    }

    /// Replaces the notices of one kind (those `kind` says yes to) and
    /// leaves the others as they are.
    pub fn put_notices(&self, kind: fn(&Notice) -> bool, now: Vec<Notice>) {
        let mut all: Vec<Notice> = self.notices().into_iter().filter(|n| !kind(n)).collect();
        all.extend(now);
        all.sort();
        self.set_notices(all);
    }

    /// The profiles as the menu lists them now.
    pub fn profiles(&self) -> Vec<ProfileEntry> {
        self.profiles.lock().unwrap().clone()
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
    "providers",
    "profiles",
    "prompt",
    "dictionary",
    "history",
    "models",
    "speed",
    "about",
];

/// A page, or a page and one of its sections as `page@section`, when both
/// are safe to put into the script that selects them.
fn target(page: &str) -> Option<String> {
    let (name, section) = match page.split_once('@') {
        Some((name, section)) => (name, Some(section)),
        None => (page, None),
    };
    if !SETTINGS_PAGES.contains(&name) {
        return None;
    }
    match section {
        None => Some(name.to_string()),
        Some(s) if !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c == '-') => {
            Some(format!("{name}@{s}"))
        }
        Some(_) => None,
    }
}

/// Shows the settings window; `page` selects a page, or a section of a page
/// (`page@section`), through a URL hash. While the window is open the app
/// uses the regular activation policy so it has a Dock icon and appears in
/// the application switcher.
pub fn open_settings<R: Runtime>(app: &AppHandle<R>, page: Option<&str>) {
    let page = page.and_then(target);
    let handle = app.clone();
    // Activation policy and window calls must run on the main thread.
    let _ = app.run_on_main_thread(move || {
        #[cfg(target_os = "macos")]
        {
            let _ = handle.set_activation_policy(tauri::ActivationPolicy::Regular);
            // The Dock tile is new, so it starts from the bundle's icon.
            crate::platform::macos::dock_icon::apply();
        }
        if let Some(w) = handle.get_webview_window("settings") {
            if let Some(p) = page {
                // The same hash again announces nothing by itself, and the
                // section still has to be scrolled to.
                let _ = w.eval(format!(
                    "location.hash === '#{p}' ? window.dispatchEvent(new Event('hashchange')) : (location.hash = '#{p}')"
                ));
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
        let all = [
            (Notice::MicrophoneDenied, ID_FIX_PERMISSIONS, "general"),
            (Notice::MicrophoneRestricted, ID_FIX_PERMISSIONS, "general"),
            (
                Notice::ProfileEngineMissing("Mail".into()),
                ID_FIX_FLOW,
                "flow",
            ),
            (
                Notice::ProfileModelMissing("Mail".into()),
                ID_FIX_FLOW,
                "flow",
            ),
            (
                Notice::ModelFileMissing("Own".into()),
                ID_FIX_ENGINES,
                "engines",
            ),
            (Notice::BackupFolderMissing, ID_FIX_BACKUP, "backup"),
            (Notice::BackupFolderNotWritable, ID_FIX_BACKUP, "backup"),
        ];
        let mut lines = std::collections::BTreeSet::new();
        for (notice, id, page) in &all {
            assert_eq!(notice.fix().0, *id);
            assert_eq!(notice.page(), *page);
            for language in [Language::ZhTw, Language::En] {
                let line = notice.line(language);
                assert!(!line.is_empty() && !line.contains("{name}"), "{line}");
                assert!(lines.insert(line));
            }
        }
        assert!(Notice::ProfileEngineMissing("Mail".into())
            .line(Language::En)
            .contains("Mail"));
        assert_eq!(
            Notice::ModelFileMissing(String::new()).fix().1,
            Msg::TrayFixEngine
        );
    }

    #[test]
    fn notices_with_the_same_fix_share_its_item() {
        let all = [
            Notice::MicrophoneDenied,
            Notice::ProfileEngineMissing("A".into()),
            Notice::ProfileModelMissing("B".into()),
            Notice::ModelFileMissing("C".into()),
            Notice::BackupFolderMissing,
        ];
        let sizes: Vec<usize> = runs(&all).iter().map(Vec::len).collect();
        assert_eq!(sizes, vec![1, 2, 1, 1]);
        assert!(runs(&[]).is_empty());
    }

    #[test]
    fn a_page_with_a_problem_is_named_once() {
        let all = [
            Notice::MicrophoneDenied,
            Notice::ProfileEngineMissing("A".into()),
            Notice::ProfileModelMissing("B".into()),
            Notice::BackupFolderNotWritable,
        ];
        assert_eq!(
            problem_pages(UiState::NoAccessibility, &all),
            vec!["general", "flow", "backup"]
        );
        assert_eq!(problem_pages(UiState::Idle, &[]), Vec::<&str>::new());
        assert_eq!(
            problem_pages(UiState::NoAccessibility, &[]),
            vec!["general"]
        );
        // The engine that never opened has its own notice on every page.
        assert_eq!(problem_pages(UiState::EngineError, &[]), Vec::<&str>::new());
    }

    #[test]
    fn only_a_problem_gets_an_action_under_the_status() {
        assert_eq!(fix_msg(UiState::Idle), None);
        assert_eq!(fix_msg(UiState::Recording), None);
        assert_eq!(
            fix_msg(UiState::NoAccessibility),
            Some(Msg::TrayFixPermissions)
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
            "about",
        ] {
            assert!(SETTINGS_PAGES.contains(&page), "{page}");
        }
        assert!(!SETTINGS_PAGES.contains(&"javascript:alert(1)"));
    }

    #[test]
    fn a_section_is_reached_through_its_page_and_nothing_else_gets_through() {
        assert_eq!(target("general"), Some("general".into()));
        assert_eq!(target(PERMISSIONS_PAGE), Some("general@permissions".into()));
        assert_eq!(target("general@"), None);
        assert_eq!(target("general@a';alert(1)//"), None);
        assert_eq!(target("nowhere@permissions"), None);
    }

    #[test]
    fn permission_notices_share_the_item_of_a_status_about_a_permission() {
        let all = [Notice::MicrophoneDenied, Notice::BackupFolderMissing];
        assert_eq!(
            under_status(UiState::NoAccessibility, &all),
            vec![Notice::MicrophoneDenied]
        );
        assert!(under_status(UiState::Idle, &all).is_empty());
        assert!(under_status(UiState::EngineError, &all).is_empty());
    }
}
