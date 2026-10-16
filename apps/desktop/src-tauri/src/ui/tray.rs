//! Menu-bar icon and menu.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use arcvox_core::types::ProfileId;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::dictation::ui::UiState;
use crate::i18n::{text, Language, Msg};
use crate::identity;
use crate::settings::WidgetIdle;

use super::tray_anim::{Goal, Outcome, Phase};

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
pub const ID_PAUSE: &str = "pause";
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
    /// A profile, by name and id, that runs on the default engine because
    /// the one it names is gone or cannot run.
    ProfileOnDefault(String, String),
    /// The default speech engine cannot run.
    DefaultAsrNotReady,
    /// The default rewrite engine cannot run.
    DefaultLlmNotReady,
    /// An engine, by name, whose own model file is gone, and the Engines
    /// tab it is listed on (`asr` or `llm`).
    ModelFileMissing(String, &'static str),
    BackupFolderMissing,
    BackupFolderNotWritable,
}

impl Notice {
    /// What is wrong, as a line of the menu.
    fn line(&self, language: Language) -> String {
        let (msg, name) = match self {
            Notice::MicrophoneDenied => (Msg::TrayMicrophoneDenied, None),
            Notice::MicrophoneRestricted => (Msg::TrayMicrophoneRestricted, None),
            Notice::ProfileOnDefault(n, _) => (Msg::TrayProfileOnDefault, Some(n)),
            Notice::DefaultAsrNotReady => (Msg::TrayDefaultAsrNotReady, None),
            Notice::DefaultLlmNotReady => (Msg::TrayDefaultLlmNotReady, None),
            Notice::ModelFileMissing(n, _) => (Msg::TrayModelFileMissing, Some(n)),
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
            Notice::ProfileOnDefault(..) => (ID_FIX_FLOW, Msg::TrayFixFlow),
            Notice::DefaultAsrNotReady
            | Notice::DefaultLlmNotReady
            | Notice::ModelFileMissing(..) => (ID_FIX_ENGINES, Msg::TrayFixEngine),
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

    /// A missing permission or a default engine that cannot run stops
    /// dictation; a profile on the default engine, a model file gone from an
    /// engine nobody falls back on, and the backup folder do not.
    pub fn level(&self) -> Level {
        match self {
            Notice::MicrophoneDenied
            | Notice::MicrophoneRestricted
            | Notice::DefaultAsrNotReady
            | Notice::DefaultLlmNotReady => Level::Fails,
            Notice::ProfileOnDefault(..)
            | Notice::ModelFileMissing(..)
            | Notice::BackupFolderMissing
            | Notice::BackupFolderNotWritable => Level::Attention,
        }
    }

    /// Where the fix item leads: the page, opened at the profile or the
    /// Engines tab the problem is in.
    pub fn target(&self) -> String {
        match self {
            Notice::ProfileOnDefault(_, id) => format!("flow/{id}"),
            Notice::DefaultAsrNotReady => "engines/asr".into(),
            Notice::DefaultLlmNotReady => "engines/llm".into(),
            Notice::ModelFileMissing(_, role) => format!("engines/{role}"),
            _ => self.page().into(),
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

/// How much a problem matters: dictation fails until it is fixed, or
/// dictation goes on and the problem asks for attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Attention,
    Fails,
}

/// A settings page with a problem to show, at the level of its worst one.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PageProblem {
    pub page: &'static str,
    pub level: Level,
}

/// The settings pages that have a problem to show, each once.
fn problem_pages(state: UiState, notices: &[Notice]) -> Vec<PageProblem> {
    let mut pages: Vec<PageProblem> = Vec::new();
    let mut add = |page: &'static str, level: Level| match pages.iter_mut().find(|p| p.page == page)
    {
        Some(p) => p.level = p.level.max(level),
        None => pages.push(PageProblem { page, level }),
    };
    if state == UiState::NoAccessibility {
        add("general", Level::Fails);
    }
    for notice in notices {
        add(notice.page(), notice.level());
    }
    pages
}

pub struct Tray<R: Runtime> {
    app: AppHandle<R>,
    icon: Arc<TrayIcon<R>>,
    /// The image's motion; a thread turns its frames while a phase runs.
    motion: Arc<Motion>,
    /// The status line of the menu now shown; replaced with every new menu.
    status: Mutex<MenuItem<R>>,
    /// What the menu was last built from: a change of language or state
    /// builds it again from these.
    last_state: Mutex<UiState>,
    language: Mutex<Language>,
    idle: Mutex<WidgetIdle>,
    placing: Mutex<bool>,
    paused: Mutex<bool>,
    notices: Mutex<Vec<Notice>>,
    profiles: Mutex<Vec<ProfileEntry>>,
}

/// The menu of the menu-bar icon; the widget opens the same one. What its
/// items do is in `app::tray_action`.
#[allow(clippy::too_many_arguments)]
pub(super) fn menu<R: Runtime>(
    app: &AppHandle<R>,
    language: Language,
    state: UiState,
    idle: WidgetIdle,
    placing: bool,
    paused: bool,
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
    // Dictation paused for a game or a presentation: ticked while it is,
    // the shortcuts then do nothing and every key reaches the application.
    menu.append(&CheckMenuItem::with_id(
        app,
        ID_PAUSE,
        text(language, Msg::TrayPause),
        true,
        paused,
        None::<&str>,
    )?)?;
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
            false,
            &[],
            &[],
        )?;
        let icon = TrayIconBuilder::with_id("arcvox")
            .icon(Phase::Rest.image())
            .icon_as_template(true)
            .menu(&first)
            .show_menu_on_left_click(true)
            .tooltip(identity::name(app.package_info()))
            .on_menu_event(|app, event| crate::app::tray_action(app, event.id().as_ref()))
            .build(app)?;
        let icon = Arc::new(icon);
        let motion = Arc::new(Motion::default());
        Motion::run(Arc::clone(&motion), Arc::clone(&icon));
        Ok(Self {
            app: app.clone(),
            icon,
            motion,
            status: Mutex::new(status),
            last_state: Mutex::new(UiState::Idle),
            language: Mutex::new(language),
            idle: Mutex::new(WidgetIdle::default()),
            placing: Mutex::new(false),
            paused: Mutex::new(false),
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
        let paused = *self.paused.lock().unwrap();
        let notices = self.notices();
        let profiles = self.profiles.lock().unwrap().clone();
        let Ok((next, status)) = menu(
            &self.app, language, state, idle, placing, paused, &notices, &profiles,
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

    /// Ticks the pause item while the shortcuts are paused.
    pub fn set_paused(&self, paused: bool) {
        *self.paused.lock().unwrap() = paused;
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
    pub fn problem_pages(&self) -> Vec<PageProblem> {
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

    /// Where the fix item `fix_id` leads: the first problem it serves, or
    /// `page` when none is listed.
    pub fn fix_target(&self, fix_id: &str, page: &str) -> String {
        self.notices()
            .iter()
            .find(|n| n.fix().0 == fix_id)
            .map_or_else(|| page.to_string(), Notice::target)
    }

    fn show_icon(&self, state: UiState) {
        let noticed = !self.notices.lock().unwrap().is_empty();
        let goal = Goal::of(state, noticed);
        let still = reduce_motion();
        self.motion.drive(&self.icon, |m| {
            m.goal = goal;
            m.still = still;
            m.phase.toward(goal, still)
        });
    }

    /// Shows how the dictation ended for a moment, before the image rests.
    pub fn outcome(&self, outcome: Outcome) {
        self.motion
            .drive(&self.icon, |m| m.phase.with_outcome(outcome, m.goal));
    }

    /// The state the status line shows.
    pub fn state(&self) -> UiState {
        *self.last_state.lock().unwrap()
    }
}

/// The system asks for reduced motion: the image jumps between still pictures.
fn reduce_motion() -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::platform::macos::reduce_motion()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// The phase the image is in, what it moves towards, and when its frame is due.
struct MotionState {
    phase: Phase,
    goal: Goal,
    still: bool,
    due: Option<Instant>,
}

/// The image's motion. A change of state or an outcome sets the phase at
/// once; the thread shows the frames that follow at their times.
struct Motion {
    state: Mutex<MotionState>,
    wake: Condvar,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            state: Mutex::new(MotionState {
                phase: Phase::Rest,
                goal: Goal::Rest,
                still: false,
                due: None,
            }),
            wake: Condvar::new(),
        }
    }
}

fn show<R: Runtime>(icon: &TrayIcon<R>, phase: Phase) {
    // The image and its template flag go in one step: set apart, the image
    // is drawn once without the flag and the frames flicker.
    let _ = icon.set_icon_with_as_template(Some(phase.image()), true);
}

impl Motion {
    /// Applies a change and shows the new phase when it is another one.
    fn drive<R: Runtime>(
        &self,
        icon: &TrayIcon<R>,
        change: impl FnOnce(&mut MotionState) -> Phase,
    ) {
        let mut m = self.state.lock().unwrap();
        let next = change(&mut m);
        if next == m.phase {
            return;
        }
        m.phase = next;
        m.due = next.wait(m.still).map(|wait| Instant::now() + wait);
        show(icon, next);
        self.wake.notify_one();
    }

    /// Turns the frames for the life of the process.
    fn run<R: Runtime>(motion: Arc<Self>, icon: Arc<TrayIcon<R>>) {
        std::thread::Builder::new()
            .name("tray-motion".into())
            .spawn(move || loop {
                let next = {
                    let mut m = motion.state.lock().unwrap();
                    loop {
                        match m.due {
                            None => m = motion.wake.wait(m).unwrap(),
                            Some(due) => {
                                let now = Instant::now();
                                if due <= now {
                                    break;
                                }
                                m = motion.wake.wait_timeout(m, due - now).unwrap().0;
                            }
                        }
                    }
                    let next = m.phase.next(m.goal, m.still);
                    m.phase = next;
                    // Counted from the time this frame was due, not from
                    // now: the frames keep their pace whatever the main
                    // thread's delay in drawing them.
                    let due = m.due.unwrap_or_else(Instant::now);
                    m.due = next.wait(m.still).map(|wait| due + wait);
                    next
                };
                show(&icon, next);
            })
            .expect("tray motion thread");
    }
}

fn status_msg(state: UiState) -> Msg {
    match state {
        UiState::Idle | UiState::Inserting => Msg::TrayStatusIdle,
        UiState::Recording | UiState::Locked => Msg::TrayStatusRecording,
        UiState::Processing => Msg::TrayStatusProcessing,
        UiState::NoAccessibility => Msg::TrayStatusNoAccessibility,
        UiState::EngineError => Msg::TrayStatusEngineError,
        UiState::Paused => Msg::TrayStatusPaused,
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

/// A page, a page and its argument as `page/arg` (a profile id, an
/// Engines tab), or a page and one of its sections as `page@section`, when
/// every part is safe to put into the script that selects them.
fn target(page: &str) -> Option<String> {
    let (route, section) = match page.split_once('@') {
        Some((route, section)) => (route, Some(section)),
        None => (page, None),
    };
    let (name, arg) = match route.split_once('/') {
        Some((name, arg)) => (name, Some(arg)),
        None => (route, None),
    };
    if !SETTINGS_PAGES.contains(&name) {
        return None;
    }
    let safe = |s: &str, extra: &[char]| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || extra.contains(&c))
    };
    let mut out = name.to_string();
    match arg {
        None => {}
        Some(a) if safe(a, &['-', '_']) => out = format!("{out}/{a}"),
        Some(_) => return None,
    }
    match section {
        None => Some(out),
        Some(s) if safe(s, &['-']) => Some(format!("{out}@{s}")),
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
        assert_ne!(Phase::Problem.image().rgba(), Phase::Rest.image().rgba());
        assert_eq!(Goal::of(UiState::NoAccessibility, false), Goal::Problem);
        assert_eq!(Goal::of(UiState::EngineError, false), Goal::Problem);
        assert_eq!(Goal::of(UiState::Idle, false), Goal::Rest);
    }

    #[test]
    fn a_pause_is_neither_rest_nor_a_problem() {
        assert_eq!(Goal::of(UiState::Paused, false), Goal::Paused);
        assert_eq!(Goal::of(UiState::Paused, true), Goal::Paused);
        assert_ne!(Phase::Paused.image().rgba(), Phase::Rest.image().rgba());
        assert_ne!(Phase::Paused.image().rgba(), Phase::Problem.image().rgba());
        assert_eq!(fix_msg(UiState::Paused), None);
        assert_eq!(status_msg(UiState::Paused), Msg::TrayStatusPaused);
    }

    #[test]
    fn a_problem_that_leaves_dictation_working_shows_while_idle() {
        assert_eq!(Goal::of(UiState::Idle, true), Goal::Problem);
        // A dictation in progress keeps its own motion; the text going in is part of it.
        for state in [
            UiState::Recording,
            UiState::Locked,
            UiState::Processing,
            UiState::Inserting,
        ] {
            assert_eq!(Goal::of(state, true), Goal::of(state, false));
            assert_ne!(Goal::of(state, true), Goal::Problem);
        }
    }

    #[test]
    fn every_notice_has_a_line_and_an_item_that_leads_to_its_fix() {
        let all = [
            (Notice::MicrophoneDenied, ID_FIX_PERMISSIONS, "general"),
            (Notice::MicrophoneRestricted, ID_FIX_PERMISSIONS, "general"),
            (
                Notice::ProfileOnDefault("Mail".into(), "mail".into()),
                ID_FIX_FLOW,
                "flow",
            ),
            (Notice::DefaultAsrNotReady, ID_FIX_ENGINES, "engines"),
            (Notice::DefaultLlmNotReady, ID_FIX_ENGINES, "engines"),
            (
                Notice::ModelFileMissing("Own".into(), "llm"),
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
        assert!(Notice::ProfileOnDefault("Mail".into(), "mail".into())
            .line(Language::En)
            .contains("Mail"));
        assert_eq!(
            Notice::ModelFileMissing(String::new(), "asr").fix().1,
            Msg::TrayFixEngine
        );
    }

    #[test]
    fn a_fix_leads_to_the_profile_or_the_tab_with_the_problem() {
        assert_eq!(
            Notice::ProfileOnDefault("Mail".into(), "mail-2".into()).target(),
            "flow/mail-2"
        );
        assert_eq!(Notice::DefaultLlmNotReady.target(), "engines/llm");
        assert_eq!(
            Notice::ModelFileMissing("Own".into(), "asr").target(),
            "engines/asr"
        );
        assert_eq!(Notice::BackupFolderMissing.target(), "backup");
        assert_eq!(target("flow/mail-2").as_deref(), Some("flow/mail-2"));
        assert_eq!(
            target("engines/llm@default").as_deref(),
            Some("engines/llm@default")
        );
        assert_eq!(target("flow/x'y"), None);
        assert_eq!(target("flow/"), None);
        assert_eq!(target("nowhere/a"), None);
    }

    #[test]
    fn notices_with_the_same_fix_share_its_item() {
        let all = [
            Notice::MicrophoneDenied,
            Notice::ProfileOnDefault("A".into(), "a".into()),
            Notice::ProfileOnDefault("B".into(), "b".into()),
            Notice::DefaultAsrNotReady,
            Notice::ModelFileMissing("C".into(), "asr"),
            Notice::BackupFolderMissing,
        ];
        let sizes: Vec<usize> = runs(&all).iter().map(Vec::len).collect();
        assert_eq!(sizes, vec![1, 2, 2, 1]);
        assert!(runs(&[]).is_empty());
    }

    #[test]
    fn a_page_with_a_problem_is_named_once() {
        let all = [
            Notice::MicrophoneDenied,
            Notice::ProfileOnDefault("A".into(), "a".into()),
            Notice::ProfileOnDefault("B".into(), "b".into()),
            Notice::BackupFolderNotWritable,
        ];
        let pages = |state, notices: &[Notice]| -> Vec<(&str, Level)> {
            problem_pages(state, notices)
                .into_iter()
                .map(|p| (p.page, p.level))
                .collect()
        };
        assert_eq!(
            pages(UiState::NoAccessibility, &all),
            vec![
                ("general", Level::Fails),
                ("flow", Level::Attention),
                ("backup", Level::Attention)
            ]
        );
        assert_eq!(pages(UiState::Idle, &[]), vec![]);
        assert_eq!(
            pages(UiState::NoAccessibility, &[]),
            vec![("general", Level::Fails)]
        );
        // The engine that never opened has its own notice on every page.
        assert_eq!(pages(UiState::EngineError, &[]), vec![]);
        // A page takes the level of its worst problem.
        assert_eq!(
            pages(
                UiState::Idle,
                &[
                    Notice::ModelFileMissing("A".into(), "llm"),
                    Notice::DefaultLlmNotReady
                ]
            ),
            vec![("engines", Level::Fails)]
        );
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
