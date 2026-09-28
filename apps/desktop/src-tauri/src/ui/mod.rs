//! Implements the controller's `Ui` on top of the tray and the widget.
//! Every AppKit call is hopped onto the main thread.

pub mod tray;
pub mod widget;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Wry};

use crate::dictation::ui::{MessageKind, Ui, UiMessage, UiState};
use crate::i18n::Language;
use tray::Tray;

const HIDE_AFTER_IDLE: Duration = Duration::from_millis(1200);
const HIDE_AFTER_ERROR: Duration = Duration::from_millis(4000);
const HIDE_AFTER_COPYABLE: Duration = Duration::from_millis(15_000);

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
    language: Mutex<Language>,
    /// Bumped on every show; a scheduled hide fires only if unchanged.
    generation: Arc<AtomicU64>,
}

impl TauriUi {
    pub fn new(app: AppHandle<Wry>, language: Language) -> tauri::Result<Self> {
        let tray = Tray::build(&app, language)?;
        Ok(Self {
            app,
            tray,
            language: Mutex::new(language),
            generation: Arc::new(AtomicU64::new(0)),
        })
    }

    pub fn tray(&self) -> &Tray<Wry> {
        &self.tray
    }

    pub fn set_language(&self, language: Language) {
        *self.language.lock().unwrap() = language;
    }

    fn language(&self) -> Language {
        *self.language.lock().unwrap()
    }

    fn show(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let app = self.app.clone();
        let _ = self
            .app
            .run_on_main_thread(move || widget::show_at_bottom(&app));
    }

    fn hide_after(&self, delay: Duration) {
        let gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let generation = self.generation.clone();
        let app = self.app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            if generation.load(Ordering::SeqCst) == gen {
                let target = app.clone();
                let _ = app.run_on_main_thread(move || widget::hide(&target));
            }
        });
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

impl Ui for TauriUi {
    fn set_state(&self, state: UiState) {
        let language = self.language();
        self.tray.set_state(state, language);
        let _ = self.app.emit_to(
            widget::WINDOW,
            "widget-state",
            StatePayload {
                state: Self::state_name(state),
                language: language.tag(),
            },
        );
        match state {
            UiState::Recording | UiState::Locked | UiState::Processing | UiState::Inserting => {
                self.show()
            }
            UiState::Idle => self.hide_after(HIDE_AFTER_IDLE),
            UiState::NoAccessibility | UiState::EngineError => {}
        }
    }

    fn level(&self, rms: f32) {
        let _ = self
            .app
            .emit_to(widget::WINDOW, "widget-level", LevelPayload { rms });
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
        self.show();
        self.hide_after(if copyable {
            HIDE_AFTER_COPYABLE
        } else {
            HIDE_AFTER_ERROR
        });
    }
}
