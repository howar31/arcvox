//! Implements the controller's `Ui` on top of the tray and the widget.
//! Every AppKit call is hopped onto the main thread.

pub mod app_menu;
pub mod placement;
pub mod preview;
pub mod tray;
pub mod widget;
pub mod window_state;

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::menu::ContextMenu;
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::dictation::ui::{MessageKind, Ui, UiMessage, UiState};
use crate::i18n::Language;
use crate::settings::{WidgetFade, WidgetIdle};
use app_menu::AppMenu;
use preview::{Look, Sample, Scene};
use tray::Tray;

const HIDE_AFTER_IDLE: Duration = Duration::from_millis(1200);
/// After a dictation whose final text is shown in the panel: the time the
/// panel takes to turn the recognised text into the final one, and a moment
/// to read it.
const HIDE_AFTER_REWRITE: Duration = Duration::from_millis(3000);
/// How long a message waits in the panel while the recognised text turns
/// into the final one; the message stays that much longer.
const MESSAGE_AFTER_REWRITE: Duration = Duration::from_millis(2600);
/// The same when the processing changed nothing: only the moment to read.
const HIDE_AFTER_SAME_TEXT: Duration = Duration::from_millis(1000);
const HIDE_AFTER_NOTICE: Duration = Duration::from_millis(4000);
/// A message in the panel has more to read than a notice on the widget.
const HIDE_AFTER_ERROR: Duration = Duration::from_millis(6000);
const HIDE_AFTER_COPYABLE: Duration = Duration::from_millis(15_000);
/// How often a dragged widget catches up with the pointer.
const DRAG_TICK: Duration = Duration::from_millis(8);
/// The widget state shown while the user is asked to position the widget.
const PLACING: &str = "placing";
/// The look of a widget at rest while it is dragged: as when it is
/// positioned from the settings window, without the panel that explains it.
const MOVING: &str = "moving";
/// The settings window, which names the state in its sidebar.
const SETTINGS: &str = "settings";
/// How long the windows stay after the widget was told to leave: the
/// length of the leaving motion in the page, and a little more.
const LEAVE: Duration = Duration::from_millis(240);
/// How often the listening scene of the preview sends an input level.
const PREVIEW_LEVEL_TICK: Duration = Duration::from_millis(50);

#[derive(Serialize, Clone)]
struct StatePayload {
    state: &'static str,
    language: &'static str,
    /// The factor of the size the user chose for the widget.
    scale: f64,
    /// What the widget shows while no dictation goes on.
    idle: &'static str,
    /// Whether and how a resting widget turns faint.
    fade: WidgetFade,
    /// A resting widget is faint without waiting: its transparency is
    /// being chosen.
    faint: bool,
}

#[derive(Serialize, Clone)]
struct LevelPayload {
    rms: f32,
}

#[derive(Serialize, Clone)]
struct TranscriptPayload {
    text: String,
    language: &'static str,
}

#[derive(Serialize, Clone)]
struct MessagePayload {
    kind: &'static str,
    key: &'static str,
    detail: Option<String>,
    copyable: Option<String>,
    /// The kind of failure, when the message reports a failed dictation.
    code: Option<&'static str>,
    retry: bool,
    language: &'static str,
}

pub struct TauriUi {
    app: AppHandle<Wry>,
    tray: Tray<Wry>,
    menu: AppMenu<Wry>,
    language: Mutex<Language>,
    /// The factor of the widget size the user chose.
    scale: Mutex<f64>,
    /// What the widget shows while no dictation goes on.
    idle: Arc<Mutex<WidgetIdle>>,
    fade: Mutex<WidgetFade>,
    state: Mutex<UiState>,
    /// Bumped on every show; a scheduled hide fires only if unchanged.
    generation: Arc<AtomicU64>,
    /// The latest deadline a scheduled hide is waiting for. A later
    /// `hide_after` call yields to an already-pending later deadline
    /// instead of cutting it short; `show` clears it outright.
    hide_deadline: Arc<Mutex<Option<Instant>>>,
    /// Position and shape of the widget; locked on the main thread only.
    layout: Arc<Mutex<widget::Layout>>,
    /// True from a show until the widget is told to leave.
    visible: Arc<AtomicBool>,
    /// Bumped on every show and every leave; the windows are hidden after
    /// the leaving motion only if it is unchanged.
    leaving: Arc<AtomicU64>,
    /// True while the user drags the widget.
    dragging: Arc<AtomicBool>,
    /// A hide came due during a drag and waits for the drop.
    hide_waiting: Arc<AtomicBool>,
    /// True while the settings window keeps the widget visible so the user
    /// can position it.
    placing: AtomicBool,
    /// The widget is kept visible by a drag, not by the settings window.
    moving: AtomicBool,
    /// The message on the widget stays until dismissed or replaced.
    sticky: AtomicBool,
    /// The recognised text the panel shows for the dictation in progress.
    transcript: Mutex<Option<String>>,
    /// How long the widget stays after this dictation, when its final text
    /// is shown in the panel.
    linger: Mutex<Option<Duration>>,
    /// When the panel began to turn the recognised text into the final one.
    rewrite_at: Mutex<Option<Instant>>,
    /// The saved look, while the settings window tries out another one.
    saved: Mutex<preview::Saved>,
    /// What the widget shows while it is kept visible.
    scene: Mutex<Scene>,
    /// Bumped whenever a scene begins or the preview ends; what a scene
    /// keeps running stops when it changed.
    preview_run: Arc<AtomicU64>,
    faint: AtomicBool,
}

impl TauriUi {
    pub fn new(
        app: AppHandle<Wry>,
        language: Language,
        scale: f64,
        config_dir: &Path,
    ) -> tauri::Result<Self> {
        let tray = Tray::build(&app, language)?;
        let menu = AppMenu::install(&app, language)?;
        Ok(Self {
            app,
            tray,
            menu,
            language: Mutex::new(language),
            scale: Mutex::new(scale),
            idle: Arc::new(Mutex::new(WidgetIdle::Hidden)),
            fade: Mutex::new(WidgetFade::default()),
            state: Mutex::new(UiState::Idle),
            generation: Arc::new(AtomicU64::new(0)),
            hide_deadline: Arc::new(Mutex::new(None)),
            layout: Arc::new(Mutex::new(widget::Layout::load(
                config_dir.to_path_buf(),
                scale,
            ))),
            visible: Arc::new(AtomicBool::new(false)),
            leaving: Arc::new(AtomicU64::new(0)),
            dragging: Arc::new(AtomicBool::new(false)),
            hide_waiting: Arc::new(AtomicBool::new(false)),
            placing: AtomicBool::new(false),
            moving: AtomicBool::new(false),
            sticky: AtomicBool::new(false),
            transcript: Mutex::new(None),
            linger: Mutex::new(None),
            rewrite_at: Mutex::new(None),
            saved: Mutex::new(preview::Saved::default()),
            scene: Mutex::new(Scene::Place),
            preview_run: Arc::new(AtomicU64::new(0)),
            faint: AtomicBool::new(false),
        })
    }

    pub fn tray(&self) -> &Tray<Wry> {
        &self.tray
    }

    /// What the backup folder was found to be: a folder that cannot be
    /// used is a problem the menu-bar icon reports.
    pub fn set_backup_folder(&self, state: arcvox_core::backup::FolderState) {
        use arcvox_core::backup::FolderState;
        let notices = match state {
            FolderState::Usable => vec![],
            FolderState::Missing => vec![tray::Notice::BackupFolderMissing],
            FolderState::NotWritable | FolderState::Blocked => {
                vec![tray::Notice::BackupFolderNotWritable]
            }
        };
        self.tray.put_notices(tray::Notice::about_backup, notices);
    }

    /// What is wrong with the profiles and the engines as they are set up.
    pub fn set_engine_problems(&self, notices: Vec<tray::Notice>) {
        self.tray.put_notices(tray::Notice::about_engines, notices);
    }

    /// The system's answer on the microphone: one that rules recording out
    /// is a problem the menu-bar icon reports.
    pub fn set_microphone(&self, access: crate::platform::MicAccess) {
        use crate::platform::MicAccess;
        let notices = match access {
            MicAccess::Granted | MicAccess::NotAsked => vec![],
            MicAccess::Denied => vec![tray::Notice::MicrophoneDenied],
            MicAccess::Restricted => vec![tray::Notice::MicrophoneRestricted],
        };
        self.tray
            .put_notices(tray::Notice::about_permissions, notices);
    }

    pub fn menu(&self) -> &AppMenu<Wry> {
        &self.menu
    }

    pub fn set_language(&self, language: Language) {
        *self.language.lock().unwrap() = language;
        self.tray.set_language(language);
        self.menu.set_language(language);
        // The widget measures its labels again in the new language.
        self.emit_current_state();
    }

    /// The factor of the widget size the user chose.
    pub fn scale(&self) -> f64 {
        *self.scale.lock().unwrap()
    }

    pub fn set_scale(&self, scale: f64) {
        self.saved.lock().unwrap().edit(|look| look.scale = scale);
        self.apply_scale(scale);
    }

    fn apply_scale(&self, scale: f64) {
        *self.scale.lock().unwrap() = scale;
        let app = self.app.clone();
        let layout = self.layout.clone();
        let _ = self
            .app
            .run_on_main_thread(move || layout.lock().unwrap().set_scale(&app, scale));
        // The widget and the panel take the size and report what they need.
        self.emit_current_state();
    }

    /// What the widget shows while no dictation goes on.
    pub fn idle(&self) -> WidgetIdle {
        *self.idle.lock().unwrap()
    }

    /// Whether and how a resting widget turns faint.
    pub fn fade(&self) -> WidgetFade {
        *self.fade.lock().unwrap()
    }

    pub fn set_fade(&self, fade: WidgetFade) {
        let fade = fade.bounded();
        self.saved.lock().unwrap().edit(|look| look.fade = fade);
        *self.fade.lock().unwrap() = fade;
        self.emit_current_state();
    }

    /// The user chose what the widget shows while no dictation goes on. A
    /// widget that shows a dictation or a message takes it when it leaves.
    pub fn set_idle(&self, idle: WidgetIdle) {
        self.saved.lock().unwrap().edit(|look| look.idle = idle);
        *self.idle.lock().unwrap() = idle;
        self.tray.set_idle(idle);
        let _ = self.app.emit_to(SETTINGS, "widget-idle", idle.name());
        self.emit_current_state();
        if self.visible.load(Ordering::SeqCst) {
            return;
        }
        let app = self.app.clone();
        let layout = self.layout.clone();
        let _ = self
            .app
            .run_on_main_thread(move || settle(&app, &layout, idle));
    }

    /// The user chose the display the widget stays on, or none.
    pub fn set_display(&self, display: Option<String>) {
        let app = self.app.clone();
        let layout = self.layout.clone();
        let _ = self
            .app
            .run_on_main_thread(move || layout.lock().unwrap().set_fixed(&app, display));
    }

    /// Opens the menu of the menu-bar icon over the widget, at the pointer.
    pub fn popup_menu(&self) {
        let app = self.app.clone();
        let language = self.language();
        let state = *self.state.lock().unwrap();
        let idle = self.idle();
        let placing = self.is_placing();
        let notices = self.tray.notices();
        let _ = self.app.run_on_main_thread(move || {
            let Some(window) = app.get_webview_window(widget::WINDOW) else {
                return;
            };
            match tray::menu(&app, language, state, idle, placing, &notices) {
                Ok((menu, _)) => {
                    if let Err(e) = menu.popup(window.as_ref().window()) {
                        tracing::warn!(error = %e, "widget menu not shown");
                    }
                }
                Err(e) => tracing::warn!(error = %e, "widget menu not built"),
            }
        });
    }

    /// Whether the widget is kept on screen to be positioned.
    pub fn is_placing(&self) -> bool {
        self.placing.load(Ordering::SeqCst) && !self.moving.load(Ordering::SeqCst)
    }

    fn emit_current_state(&self) {
        let state = *self.state.lock().unwrap();
        self.emit_state(state);
    }

    fn language(&self) -> Language {
        *self.language.lock().unwrap()
    }

    fn show(&self) {
        self.sticky.store(false, Ordering::SeqCst);
        *self.hide_deadline.lock().unwrap() = None;
        self.generation.fetch_add(1, Ordering::SeqCst);
        // A hide that waits for the leaving motion is called off.
        self.leaving.fetch_add(1, Ordering::SeqCst);
        let enters = !self.visible.swap(true, Ordering::SeqCst);
        let app = self.app.clone();
        let layout = self.layout.clone();
        let _ = self.app.run_on_main_thread(move || {
            layout.lock().unwrap().show(&app);
            // After the window has its whole size: a folded widget opens
            // from its round end into it.
            if enters {
                for window in [widget::WINDOW, widget::PANEL] {
                    let _ = app.emit_to(window, "widget-enter", ());
                }
            }
        });
    }

    /// The widget measured the length its labels need in each shape.
    pub fn measured(&self, wide: f64, tall: f64) {
        let long = placement::Long::measured(wide, tall);
        let app = self.app.clone();
        let layout = self.layout.clone();
        let _ = self
            .app
            .run_on_main_thread(move || layout.lock().unwrap().set_long(&app, long));
    }

    /// The panel has content of this size, or none.
    pub fn panel_fit(&self, size: Option<(f64, f64)>) {
        let size = size.map(|(w, h)| placement::Size { w, h });
        let app = self.app.clone();
        let layout = self.layout.clone();
        let _ = self
            .app
            .run_on_main_thread(move || layout.lock().unwrap().set_panel(&app, size));
    }

    /// Sends an event to the widget and to its panel.
    fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) {
        for window in [widget::WINDOW, widget::PANEL] {
            let _ = self.app.emit_to(window, event, payload.clone());
        }
    }

    /// Schedules a hide, but never shortens an already-pending later
    /// deadline (e.g. a 6 s error hide must not cut a 15 s copyable-message
    /// hide short just because `Idle` follows right after).
    fn hide_after(&self, delay: Duration) {
        if self.placing.load(Ordering::SeqCst) || self.sticky.load(Ordering::SeqCst) {
            return;
        }
        let deadline = Instant::now() + delay;
        {
            let mut pending = self.hide_deadline.lock().unwrap();
            if let Some(existing) = *pending {
                if existing > deadline {
                    return;
                }
            }
            *pending = Some(deadline);
        }
        let gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let generation = self.generation.clone();
        let hide_deadline = self.hide_deadline.clone();
        let dragging = self.dragging.clone();
        let hide_waiting = self.hide_waiting.clone();
        let layout = self.layout.clone();
        let app = self.app.clone();
        let visible = self.visible.clone();
        let leaving = self.leaving.clone();
        let idle = self.idle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            if generation.load(Ordering::SeqCst) == gen {
                *hide_deadline.lock().unwrap() = None;
                if dragging.load(Ordering::SeqCst) {
                    hide_waiting.store(true, Ordering::SeqCst);
                } else {
                    leave(&app, &layout, &visible, &leaving, &idle);
                }
            }
        });
    }

    /// Dismisses the widget right away instead of after a delay. The
    /// generation is bumped first so a pending `hide_after` sleeper finds a
    /// stale generation when it wakes and does nothing.
    pub fn dismiss(&self) {
        self.sticky.store(false, Ordering::SeqCst);
        self.generation.fetch_add(1, Ordering::SeqCst);
        *self.hide_deadline.lock().unwrap() = None;
        leave(
            &self.app,
            &self.layout,
            &self.visible,
            &self.leaving,
            &self.idle,
        );
    }

    /// Follows the pointer until the mouse button is released, then keeps
    /// the position for the display the widget was dropped on.
    pub fn drag_start(self: Arc<Self>) {
        if self.dragging.swap(true, Ordering::SeqCst) {
            return;
        }
        // A widget at rest is dragged as the whole widget, with the look it
        // has while it is being positioned, so its place can be judged.
        let opened = !self.visible.load(Ordering::SeqCst)
            && self.idle() != WidgetIdle::Hidden
            && !self.placing.load(Ordering::SeqCst);
        if opened {
            self.moving.store(true, Ordering::SeqCst);
            self.set_placing(true);
        } else if self.is_placing() && *self.scene.lock().unwrap() == Scene::Idle {
            // A previewed widget at rest is dragged as the whole widget too.
            self.preview_place();
            self.play(Scene::Place, Sample::default());
        }
        std::thread::spawn(move || {
            // Runs one step on the main thread; true while the drag goes on.
            let step = |begin: bool| {
                let (tx, rx) = std::sync::mpsc::channel();
                let app = self.app.clone();
                let layout = self.layout.clone();
                let sent = self.app.run_on_main_thread(move || {
                    let mut layout = layout.lock().unwrap();
                    let going = if begin {
                        layout.drag_begin(&app)
                    } else {
                        layout.drag_step(&app)
                    };
                    let _ = tx.send(going);
                });
                sent.is_ok() && rx.recv().unwrap_or(false)
            };
            if step(true) {
                loop {
                    std::thread::sleep(DRAG_TICK);
                    if !step(false) {
                        break;
                    }
                }
            }
            self.dragging.store(false, Ordering::SeqCst);
            if self.hide_waiting.swap(false, Ordering::SeqCst) {
                self.hide_after(HIDE_AFTER_IDLE);
            }
            if opened {
                self.set_placing(false);
                self.moving.store(false, Ordering::SeqCst);
            }
        });
    }

    /// Keeps the widget visible so the user can drag it into place, or
    /// ends that and hides it again.
    pub fn set_placing(&self, on: bool) {
        if self.placing.swap(on, Ordering::SeqCst) == on {
            return;
        }
        if !self.moving.load(Ordering::SeqCst) {
            // The menus and the settings window show that it is on.
            self.tray.set_placing(on);
            let _ = self.app.emit_to(SETTINGS, "widget-placing", on);
        }
        if !on {
            // What was tried out is over: the saved look is back.
            self.preview_run.fetch_add(1, Ordering::SeqCst);
            *self.scene.lock().unwrap() = Scene::Place;
            self.faint.store(false, Ordering::SeqCst);
            let saved = self.saved.lock().unwrap().take();
            if let Some(look) = saved {
                *self.fade.lock().unwrap() = look.fade;
                *self.idle.lock().unwrap() = look.idle;
                if look.scale != self.scale() {
                    self.apply_scale(look.scale);
                }
            }
        }
        let state = *self.state.lock().unwrap();
        if is_session(state) {
            // A dictation is on screen; it goes on as usual.
            return;
        }
        self.emit("widget-reset", ());
        self.emit_state(state);
        if on {
            self.show();
        } else {
            self.dismiss();
        }
    }

    /// Forgets the positions chosen by dragging.
    pub fn reset_positions(&self) {
        let app = self.app.clone();
        let layout = self.layout.clone();
        let _ = self
            .app
            .run_on_main_thread(move || layout.lock().unwrap().reset(&app));
    }

    /// Tells the windows what the widget shows in this state. While it is
    /// kept visible and no dictation goes on, the widget shows the scene
    /// chosen for it, and the settings window is told only that it is kept.
    fn emit_state(&self, state: UiState) {
        let kept = self.placing.load(Ordering::SeqCst) && !is_session(state);
        let moving = self.moving.load(Ordering::SeqCst);
        let (shown, told) = match (kept, moving) {
            (false, _) => (Self::state_name(state), Self::state_name(state)),
            (true, true) => (MOVING, MOVING),
            (true, false) => (self.scene.lock().unwrap().state(), PLACING),
        };
        self.emit(
            "widget-state",
            StatePayload {
                state: shown,
                language: self.language().tag(),
                scale: self.scale(),
                idle: self.idle().name(),
                fade: self.fade(),
                faint: self.faint.load(Ordering::SeqCst),
            },
        );
        let _ = self.app.emit_to(SETTINGS, "dictation-state", told);
    }

    /// Shows a state of the widget with a look that is not saved, while the
    /// widget is kept visible and no dictation goes on. `play` begins the
    /// scene again even when it is the one already shown; `faint` shows a
    /// resting widget faint at once.
    pub fn preview(&self, scene: Scene, look: Look, sample: Sample, play: bool, faint: bool) {
        if !self.is_placing() || is_session(*self.state.lock().unwrap()) {
            return;
        }
        self.saved.lock().unwrap().keep(Look {
            scale: self.scale(),
            idle: self.idle(),
            fade: self.fade(),
        });
        *self.fade.lock().unwrap() = look.fade.bounded();
        let idle_changed =
            std::mem::replace(&mut *self.idle.lock().unwrap(), look.idle) != look.idle;
        self.faint.store(faint, Ordering::SeqCst);
        let scene_changed = std::mem::replace(&mut *self.scene.lock().unwrap(), scene) != scene;
        if look.scale != self.scale() {
            self.apply_scale(look.scale);
        }
        if play || scene_changed {
            self.play(scene, sample);
        } else {
            self.emit_current_state();
            if scene == Scene::Idle && idle_changed {
                self.dismiss();
            }
        }
    }

    /// Begins a scene: the widget and its panel get what a dictation in
    /// that state would send them.
    fn play(&self, scene: Scene, sample: Sample) {
        let run = self.preview_run.fetch_add(1, Ordering::SeqCst) + 1;
        *self.scene.lock().unwrap() = scene;
        self.emit("widget-reset", ());
        self.emit_current_state();
        if scene == Scene::Idle {
            // The widget leaves and rests as it does after a dictation.
            self.dismiss();
            return;
        }
        self.show();
        let language = self.language().tag();
        match scene {
            Scene::Place | Scene::Idle => {}
            Scene::Listening => {
                let app = self.app.clone();
                let current = self.preview_run.clone();
                std::thread::spawn(move || {
                    let began = Instant::now();
                    while current.load(Ordering::SeqCst) == run {
                        let rms = preview::level(began.elapsed().as_secs_f32());
                        let _ = app.emit_to(widget::WINDOW, "widget-level", LevelPayload { rms });
                        std::thread::sleep(PREVIEW_LEVEL_TICK);
                    }
                });
            }
            Scene::Processing | Scene::Done => {
                if let Some(text) = sample.raw {
                    self.emit("widget-transcript", TranscriptPayload { text, language });
                    if let (Scene::Done, Some(text)) = (scene, sample.refined) {
                        self.emit("widget-refined", TranscriptPayload { text, language });
                    }
                }
                if scene == Scene::Done {
                    self.emit("widget-inserted", ());
                }
            }
            Scene::Failed => self.emit(
                "widget-message",
                MessagePayload {
                    kind: "error",
                    key: crate::i18n::Msg::WidgetAsrFailed.key(),
                    detail: None,
                    copyable: sample.detail,
                    code: Some(arcvox_core::types::ErrorCode::ProviderUnreachable.name()),
                    retry: false,
                    language,
                },
            ),
        }
    }

    /// The widget goes back to being shown for dragging, and the settings
    /// window is told so.
    fn preview_place(&self) {
        if std::mem::replace(&mut *self.scene.lock().unwrap(), Scene::Place) == Scene::Place {
            return;
        }
        self.preview_run.fetch_add(1, Ordering::SeqCst);
        let _ = self
            .app
            .emit_to(SETTINGS, "widget-preview-scene", Scene::Place.name());
    }

    /// The name of the state the settings window shows when it opens.
    pub fn state_label(&self) -> &'static str {
        Self::state_name(*self.state.lock().unwrap())
    }

    fn state_name(state: UiState) -> &'static str {
        match state {
            UiState::Idle => "idle",
            UiState::Recording => "recording",
            UiState::Locked => "locked",
            UiState::Processing => "processing",
            UiState::Inserting => "inserting",
            UiState::NoAccessibility => "no_accessibility",
            UiState::EngineError => "engine_error",
        }
    }
}

/// Whether the state belongs to a dictation in progress.
fn is_session(state: UiState) -> bool {
    matches!(
        state,
        UiState::Recording | UiState::Locked | UiState::Processing | UiState::Inserting
    )
}

/// Puts the widget into what it shows while no dictation goes on: off the
/// screen, or at rest on it. Main thread only.
fn settle(app: &AppHandle<Wry>, layout: &Arc<Mutex<widget::Layout>>, idle: WidgetIdle) {
    let mut layout = layout.lock().unwrap();
    match idle {
        WidgetIdle::Hidden => {
            layout.hidden();
            widget::hide(app);
        }
        WidgetIdle::Ring | WidgetIdle::Label => layout.rest(app, idle == WidgetIdle::Ring),
    }
}

/// Tells the widget and its panel to leave, and once the leaving motion is
/// over hides their windows or leaves the widget at rest, unless the widget
/// is shown again meanwhile.
fn leave(
    app: &AppHandle<Wry>,
    layout: &Arc<Mutex<widget::Layout>>,
    visible: &Arc<AtomicBool>,
    leaving: &Arc<AtomicU64>,
    idle: &Arc<Mutex<WidgetIdle>>,
) {
    let token = leaving.fetch_add(1, Ordering::SeqCst) + 1;
    let wait = if visible.swap(false, Ordering::SeqCst) {
        for window in [widget::WINDOW, widget::PANEL] {
            let _ = app.emit_to(window, "widget-leave", ());
        }
        LEAVE
    } else {
        Duration::ZERO
    };
    let app = app.clone();
    let layout = layout.clone();
    let leaving = leaving.clone();
    let idle = idle.clone();
    std::thread::spawn(move || {
        std::thread::sleep(wait);
        let target = app.clone();
        let _ = app.run_on_main_thread(move || {
            if leaving.load(Ordering::SeqCst) != token {
                return;
            }
            let idle = *idle.lock().unwrap();
            settle(&target, &layout, idle);
        });
    });
}

impl Ui for TauriUi {
    fn set_state(&self, state: UiState) {
        self.tray.set_state(state, self.language());
        *self.state.lock().unwrap() = state;
        let placing = self.placing.load(Ordering::SeqCst);
        if placing && matches!(state, UiState::Recording | UiState::Locked) {
            // A real dictation takes the place of a previewed state.
            self.preview_place();
        }
        self.emit_state(state);
        match state {
            UiState::Recording | UiState::Locked | UiState::Processing | UiState::Inserting => {
                if matches!(state, UiState::Recording | UiState::Locked) {
                    // A new dictation: nothing of the last one is shown.
                    *self.transcript.lock().unwrap() = None;
                    *self.linger.lock().unwrap() = None;
                    *self.rewrite_at.lock().unwrap() = None;
                }
                self.show()
            }
            // Stays as it is: visible, with its message if it has one.
            UiState::Idle if placing => {}
            UiState::Idle => {
                let linger = self.linger.lock().unwrap().take();
                self.hide_after(linger.unwrap_or(HIDE_AFTER_IDLE))
            }
            UiState::NoAccessibility | UiState::EngineError => {}
        }
        if state == UiState::Idle {
            self.history_changed();
        }
    }

    fn history_changed(&self) {
        // The backup folder follows the history.
        if let Some(state) = self.app.try_state::<crate::app::AppState>() {
            state.backup.touch();
        }
        let _ = self
            .app
            .emit_to(window_state::SETTINGS_WINDOW, "history-changed", ());
    }

    fn level(&self, rms: f32) {
        let _ = self
            .app
            .emit_to(widget::WINDOW, "widget-level", LevelPayload { rms });
    }

    fn refined(&self, text: String) {
        // Without the recognised text on the panel there is nothing to turn into the final text.
        let Some(transcript) = self.transcript.lock().unwrap().clone() else {
            return;
        };
        *self.linger.lock().unwrap() = Some(if transcript == text {
            HIDE_AFTER_SAME_TEXT
        } else {
            *self.rewrite_at.lock().unwrap() = Some(Instant::now());
            HIDE_AFTER_REWRITE
        });
        let language = self.language();
        self.emit(
            "widget-refined",
            TranscriptPayload {
                text,
                language: language.tag(),
            },
        );
    }

    fn inserted(&self) {
        self.emit("widget-inserted", ());
    }

    fn copied(&self) {
        self.emit("widget-copied", ());
    }

    fn transcript(&self, text: String) {
        *self.transcript.lock().unwrap() = Some(text.clone());
        let language = self.language();
        self.emit(
            "widget-transcript",
            TranscriptPayload {
                text,
                language: language.tag(),
            },
        );
    }

    fn message(&self, message: UiMessage) {
        let language = self.language();
        let copyable = message.copyable.is_some();
        let sticky = message.sticky;
        let notice = message.is_notice();
        // The message takes the panel, after the change of the text when
        // there is one to show.
        *self.linger.lock().unwrap() = None;
        let wait = self
            .rewrite_at
            .lock()
            .unwrap()
            .take()
            .map_or(Duration::ZERO, |at| {
                MESSAGE_AFTER_REWRITE.saturating_sub(at.elapsed())
            });
        self.emit(
            "widget-message",
            MessagePayload {
                kind: match message.kind {
                    MessageKind::Info => "info",
                    MessageKind::Error => "error",
                },
                key: message.msg.key(),
                detail: message.detail,
                copyable: message.copyable,
                code: message.code.map(|c| c.name()),
                retry: message.retry,
                language: language.tag(),
            },
        );
        self.show();
        self.sticky.store(sticky, Ordering::SeqCst);
        self.hide_after(
            wait + if copyable {
                HIDE_AFTER_COPYABLE
            } else if notice {
                HIDE_AFTER_NOTICE
            } else {
                HIDE_AFTER_ERROR
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_dictation_states_count_as_a_session() {
        for state in [
            UiState::Recording,
            UiState::Locked,
            UiState::Processing,
            UiState::Inserting,
        ] {
            assert!(is_session(state), "{state:?}");
        }
        for state in [
            UiState::Idle,
            UiState::NoAccessibility,
            UiState::EngineError,
        ] {
            assert!(!is_session(state), "{state:?}");
        }
    }
}
