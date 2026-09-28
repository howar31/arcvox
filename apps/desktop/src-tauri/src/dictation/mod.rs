//! The dictation controller: one thread that owns the state machine and
//! receives everything through a single channel.

pub mod controller;
pub mod ui;

use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::thread;

use arcvox_core::types::SessionId;
use arcvox_core::{Engine, Event};

use crate::audio::{AudioMsg, AudioSource};
use crate::platform::{ModifierEvent, Platform};
use crate::settings::ClientSettings;
use crate::vad::Vad;
use ui::Ui;

pub enum ControllerMsg {
    Modifier(ModifierEvent),
    Audio(AudioMsg),
    Engine { session_id: SessionId, event: Event },
    Settings(ClientSettings),
    EngineReady(Engine),
    Shutdown,
}

pub struct Deps {
    pub platform: Arc<dyn Platform>,
    pub audio: Arc<dyn AudioSource>,
    pub ui: Arc<dyn Ui>,
    pub vad_factory: Box<dyn Fn() -> Box<dyn Vad> + Send>,
    /// Tokio handle used for `record_final` and the event forwarder.
    pub runtime: tokio::runtime::Handle,
}

#[derive(Clone)]
pub struct ControllerHandle {
    tx: Sender<ControllerMsg>,
}

impl ControllerHandle {
    pub fn send(&self, msg: ControllerMsg) {
        let _ = self.tx.send(msg);
    }

    /// A sender the platform monitor can use; a forwarding thread maps
    /// events into `ControllerMsg`.
    pub fn modifier_sender(&self) -> Sender<ModifierEvent> {
        forward(self.tx.clone(), ControllerMsg::Modifier)
    }

    pub fn audio_sender(&self) -> Sender<AudioMsg> {
        forward(self.tx.clone(), ControllerMsg::Audio)
    }

    pub fn set_engine(&self, engine: Engine) {
        self.send(ControllerMsg::EngineReady(engine));
    }

    pub fn update_settings(&self, settings: ClientSettings) {
        self.send(ControllerMsg::Settings(settings));
    }
}

/// Spawns a thread that maps `T` into `ControllerMsg` and forwards it.
pub fn forward<T: Send + 'static>(
    tx: Sender<ControllerMsg>,
    map: impl Fn(T) -> ControllerMsg + Send + 'static,
) -> Sender<T> {
    let (in_tx, in_rx) = channel::<T>();
    thread::Builder::new()
        .name("arcvox-forward".into())
        .spawn(move || {
            while let Ok(v) = in_rx.recv() {
                if tx.send(map(v)).is_err() {
                    break;
                }
            }
        })
        .expect("spawn forwarder");
    in_tx
}

/// Starts the controller thread. `engine` may be `None` when the engine
/// failed to open; it can be supplied later with `set_engine`.
pub fn spawn(deps: Deps, settings: ClientSettings, engine: Option<Engine>) -> ControllerHandle {
    let (tx, rx) = channel::<ControllerMsg>();
    let handle = ControllerHandle { tx: tx.clone() };
    let mut controller = controller::Controller::new(deps, settings, engine, tx);
    thread::Builder::new()
        .name("arcvox-dictation".into())
        .spawn(move || controller.run(rx))
        .expect("spawn controller");
    handle
}
