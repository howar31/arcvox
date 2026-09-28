//! Implements the controller's `Ui` on top of the tray and the widget.
//! Every AppKit call is hopped onto the main thread.

pub mod tray;
pub mod widget;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    /// The latest deadline a scheduled hide is waiting for. A later
    /// `hide_after` call yields to an already-pending later deadline
    /// instead of cutting it short; `show` clears it outright.
    hide_deadline: Arc<Mutex<Option<Instant>>>,
    /// Whether the widget is currently showing a copyable message (one
    /// with a Copy button). While true, `set_state(Idle)` must leave the
    /// panel's click-through state alone: `Idle` fires the instant
    /// `insert()` finishes, milliseconds after the copyable message was
    /// shown, and resetting click-through then would make the Copy button
    /// unclickable for the rest of the message's `HIDE_AFTER_COPYABLE`
    /// window.
    copyable_showing: Arc<AtomicBool>,
}

impl TauriUi {
    pub fn new(app: AppHandle<Wry>, language: Language) -> tauri::Result<Self> {
        let tray = Tray::build(&app, language)?;
        Ok(Self {
            app,
            tray,
            language: Mutex::new(language),
            generation: Arc::new(AtomicU64::new(0)),
            hide_deadline: Arc::new(Mutex::new(None)),
            copyable_showing: Arc::new(AtomicBool::new(false)),
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
        *self.hide_deadline.lock().unwrap() = None;
        self.generation.fetch_add(1, Ordering::SeqCst);
        let app = self.app.clone();
        let _ = self
            .app
            .run_on_main_thread(move || widget::show_at_bottom(&app));
    }

    /// Schedules a hide, but never shortens an already-pending later
    /// deadline (e.g. a 4 s error hide must not cut a 15 s copyable-message
    /// hide short just because `Idle` follows right after).
    fn hide_after(&self, delay: Duration) {
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
        let copyable_showing = self.copyable_showing.clone();
        let app = self.app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            if generation.load(Ordering::SeqCst) == gen {
                *hide_deadline.lock().unwrap() = None;
                // The panel is actually going away now, so whatever
                // message (copyable or not) was showing is gone with it;
                // click-through goes back to true so a stray click after
                // the panel reappears never lands on the old content.
                copyable_showing.store(false, Ordering::SeqCst);
                let target = app.clone();
                let _ = app.run_on_main_thread(move || {
                    widget::set_click_through(&target, true);
                    widget::hide(&target);
                });
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

/// A `set_state` or `message` call, the two events that can change the
/// widget's click-through state.
enum ClickThroughEvent {
    State(UiState),
    Message { copyable: bool },
}

/// What click-through value to apply for `event`, or `None` to leave the
/// panel's current click-through state alone. The only event that must be
/// left alone is `Idle` while a copyable message is still showing (see
/// `TauriUi::copyable_showing`); every other state resets to click-through,
/// and a message always sets an explicit value.
fn click_through_for(event: ClickThroughEvent, copyable_showing: bool) -> Option<bool> {
    match event {
        ClickThroughEvent::State(UiState::Idle) if copyable_showing => None,
        ClickThroughEvent::State(_) => Some(true),
        ClickThroughEvent::Message { copyable } => Some(!copyable),
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
        // Any state other than `Idle` starts a new session (or a
        // permission/engine banner) and ends whatever copyable message
        // was showing; `Idle` itself must not clear it here; `insert()`
        // calls `go_idle()` right after showing a copyable message, and
        // the message's own `HIDE_AFTER_COPYABLE` timer (or a later
        // session) is what ends it instead.
        if !matches!(state, UiState::Idle) {
            self.copyable_showing.store(false, Ordering::SeqCst);
        }
        let showing = self.copyable_showing.load(Ordering::SeqCst);
        if let Some(ignore) = click_through_for(ClickThroughEvent::State(state), showing) {
            let app = self.app.clone();
            let _ = self
                .app
                .run_on_main_thread(move || widget::set_click_through(&app, ignore));
        }
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
        // Only a copyable message has a Copy button to click, so only it
        // makes the panel take mouse events. Recorded before the
        // `Idle` that typically follows right after (from `go_idle()`)
        // so that state change knows not to undo this.
        self.copyable_showing.store(copyable, Ordering::SeqCst);
        let ignore = click_through_for(ClickThroughEvent::Message { copyable }, copyable)
            .expect("a message always has an explicit click-through value");
        let app = self.app.clone();
        let _ = self
            .app
            .run_on_main_thread(move || widget::set_click_through(&app, ignore));
        self.show();
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
    fn idle_with_a_copyable_message_showing_is_left_alone() {
        assert_eq!(
            click_through_for(ClickThroughEvent::State(UiState::Idle), true),
            None
        );
    }

    #[test]
    fn idle_with_nothing_copyable_resets_to_click_through() {
        assert_eq!(
            click_through_for(ClickThroughEvent::State(UiState::Idle), false),
            Some(true)
        );
    }

    #[test]
    fn an_active_state_resets_to_click_through() {
        assert_eq!(
            click_through_for(ClickThroughEvent::State(UiState::Recording), true),
            Some(true)
        );
    }

    #[test]
    fn a_copyable_message_makes_the_panel_interactive() {
        assert_eq!(
            click_through_for(ClickThroughEvent::Message { copyable: true }, false),
            Some(false)
        );
    }

    #[test]
    fn a_plain_message_stays_click_through() {
        assert_eq!(
            click_through_for(ClickThroughEvent::Message { copyable: false }, false),
            Some(true)
        );
    }
}
