//! Implements the controller's `Ui` on top of the tray and the widget.
//! Every AppKit call is hopped onto the main thread.

pub mod app_menu;
pub mod placement;
pub mod tray;
pub mod widget;
pub mod window_state;

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Wry};

use crate::dictation::ui::{MessageKind, Ui, UiMessage, UiState};
use crate::i18n::Language;
use app_menu::AppMenu;
use tray::Tray;

const HIDE_AFTER_IDLE: Duration = Duration::from_millis(1200);
const HIDE_AFTER_ERROR: Duration = Duration::from_millis(4000);
const HIDE_AFTER_COPYABLE: Duration = Duration::from_millis(15_000);
/// How often a dragged widget catches up with the pointer.
const DRAG_TICK: Duration = Duration::from_millis(8);
/// The widget state shown while the user is asked to position the widget.
const PLACING: &str = "placing";

#[derive(Serialize, Clone)]
struct StatePayload {
    state: &'static str,
    language: &'static str,
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
    language: &'static str,
}

pub struct TauriUi {
    app: AppHandle<Wry>,
    tray: Tray<Wry>,
    menu: AppMenu<Wry>,
    language: Mutex<Language>,
    state: Mutex<UiState>,
    /// Bumped on every show; a scheduled hide fires only if unchanged.
    generation: Arc<AtomicU64>,
    /// The latest deadline a scheduled hide is waiting for. A later
    /// `hide_after` call yields to an already-pending later deadline
    /// instead of cutting it short; `show` clears it outright.
    hide_deadline: Arc<Mutex<Option<Instant>>>,
    /// Position and shape of the widget; locked on the main thread only.
    layout: Arc<Mutex<widget::Layout>>,
    /// True while the user drags the widget.
    dragging: Arc<AtomicBool>,
    /// A hide came due during a drag and waits for the drop.
    hide_waiting: Arc<AtomicBool>,
    /// True while the settings window keeps the widget visible so the user
    /// can position it.
    placing: AtomicBool,
}

impl TauriUi {
    pub fn new(app: AppHandle<Wry>, language: Language, config_dir: &Path) -> tauri::Result<Self> {
        let tray = Tray::build(&app, language)?;
        let menu = AppMenu::install(&app, language)?;
        Ok(Self {
            app,
            tray,
            menu,
            language: Mutex::new(language),
            state: Mutex::new(UiState::Idle),
            generation: Arc::new(AtomicU64::new(0)),
            hide_deadline: Arc::new(Mutex::new(None)),
            layout: Arc::new(Mutex::new(widget::Layout::load(config_dir.to_path_buf()))),
            dragging: Arc::new(AtomicBool::new(false)),
            hide_waiting: Arc::new(AtomicBool::new(false)),
            placing: AtomicBool::new(false),
        })
    }

    pub fn tray(&self) -> &Tray<Wry> {
        &self.tray
    }

    pub fn menu(&self) -> &AppMenu<Wry> {
        &self.menu
    }

    pub fn set_language(&self, language: Language) {
        *self.language.lock().unwrap() = language;
        self.tray.set_language(language);
        self.menu.set_language(language);
    }

    fn language(&self) -> Language {
        *self.language.lock().unwrap()
    }

    /// Shows the widget. `compact` says it has only the ring and a short
    /// label to show, which lets a widget docked to a side edge be vertical.
    /// Only a message needs the horizontal pill there; the transcript is
    /// shown in the horizontal pill only.
    fn show(&self, compact: bool) {
        *self.hide_deadline.lock().unwrap() = None;
        self.generation.fetch_add(1, Ordering::SeqCst);
        let app = self.app.clone();
        let layout = self.layout.clone();
        let _ = self
            .app
            .run_on_main_thread(move || layout.lock().unwrap().show(&app, compact));
    }

    /// Schedules a hide, but never shortens an already-pending later
    /// deadline (e.g. a 4 s error hide must not cut a 15 s copyable-message
    /// hide short just because `Idle` follows right after).
    fn hide_after(&self, delay: Duration) {
        if self.placing.load(Ordering::SeqCst) {
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
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            if generation.load(Ordering::SeqCst) == gen {
                *hide_deadline.lock().unwrap() = None;
                if dragging.load(Ordering::SeqCst) {
                    hide_waiting.store(true, Ordering::SeqCst);
                } else {
                    hide_panel_now(&app, &layout);
                }
            }
        });
    }

    /// Dismisses the widget right away instead of after a delay. The
    /// generation is bumped first so a pending `hide_after` sleeper finds a
    /// stale generation when it wakes and does nothing.
    pub fn dismiss(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        *self.hide_deadline.lock().unwrap() = None;
        hide_panel_now(&self.app, &self.layout);
    }

    /// Follows the pointer until the mouse button is released, then keeps
    /// the position for the display the widget was dropped on.
    pub fn drag_start(self: Arc<Self>) {
        if self.dragging.swap(true, Ordering::SeqCst) {
            return;
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
        });
    }

    /// Keeps the widget visible so the user can drag it into place, or
    /// ends that and hides it again.
    pub fn set_placing(&self, on: bool) {
        if self.placing.swap(on, Ordering::SeqCst) == on {
            return;
        }
        let state = *self.state.lock().unwrap();
        if is_session(state) {
            // A dictation is on screen; it goes on as usual.
            return;
        }
        if on {
            self.emit_state(PLACING);
            self.show(true);
        } else {
            self.emit_state(Self::state_name(state));
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

    fn emit_state(&self, state: &'static str) {
        let _ = self.app.emit_to(
            widget::WINDOW,
            "widget-state",
            StatePayload {
                state,
                language: self.language().tag(),
            },
        );
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

fn hide_panel_now(app: &AppHandle<Wry>, layout: &Arc<Mutex<widget::Layout>>) {
    let target = app.clone();
    let layout = layout.clone();
    let _ = app.run_on_main_thread(move || {
        layout.lock().unwrap().hidden();
        widget::hide(&target);
    });
}

impl Ui for TauriUi {
    fn set_state(&self, state: UiState) {
        self.tray.set_state(state, self.language());
        *self.state.lock().unwrap() = state;
        let placing = self.placing.load(Ordering::SeqCst);
        self.emit_state(if placing && !is_session(state) {
            PLACING
        } else {
            Self::state_name(state)
        });
        match state {
            UiState::Recording | UiState::Locked | UiState::Processing | UiState::Inserting => {
                self.show(true)
            }
            // Stays as it is: visible, and wide if a message is on it.
            UiState::Idle if placing => {}
            UiState::Idle => self.hide_after(HIDE_AFTER_IDLE),
            UiState::NoAccessibility | UiState::EngineError => {}
        }
    }

    fn level(&self, rms: f32) {
        let _ = self
            .app
            .emit_to(widget::WINDOW, "widget-level", LevelPayload { rms });
    }

    fn transcript(&self, text: String) {
        let language = self.language();
        let _ = self.app.emit_to(
            widget::WINDOW,
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
        let _ = self.app.emit_to(
            widget::WINDOW,
            "widget-message",
            MessagePayload {
                kind: match message.kind {
                    MessageKind::Info => "info",
                    MessageKind::Error => "error",
                },
                key: message.msg.key(),
                detail: message.detail,
                copyable: message.copyable,
                language: language.tag(),
            },
        );
        self.show(false);
        self.hide_after(if copyable {
            HIDE_AFTER_COPYABLE
        } else {
            HIDE_AFTER_ERROR
        });
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
