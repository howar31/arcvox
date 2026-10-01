//! State machine: Idle -> Armed (fn down, stream open) -> Recording ->
//! Processing -> insertion -> Idle. See the design's section 6.2.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use arcvox_core::types::{ErrorCode, SessionId};
use arcvox_core::{Engine, Event, Session, SessionRequest};
use tokio_stream::StreamExt;

use super::ui::{UiMessage, UiState};
use super::{ControllerMsg, Deps};
use crate::audio::recorder::Recorder;
use crate::audio::{AudioHandle, AudioMsg};
use crate::context::{self, Captured};
use crate::hotkey::{Gesture, Input, Recognizer, KEY_ESCAPE};
use crate::i18n::Msg;
use crate::insert::{self, Outcome, ShowReason, Situation};
use crate::platform::ModifierEvent;
use crate::settings::ClientSettings;
use crate::vad;

struct Capture {
    handle: Option<Box<dyn AudioHandle>>,
    recorder: Option<Recorder>,
    /// Set for a hands-free dictation that ends by itself after a pause.
    pause: Option<vad::SilenceWatch>,
}

impl Capture {
    fn push(&mut self, rate: u32, mono: &[f32]) -> f32 {
        self.recorder
            .get_or_insert_with(|| Recorder::new(rate))
            .push(mono)
    }
    /// True when the speaker has paused for as long as the setting says.
    fn paused(&mut self) -> bool {
        match (&mut self.pause, &self.recorder) {
            (Some(watch), Some(recorder)) => watch.paused(recorder.samples()),
            _ => false,
        }
    }
    fn secs(&self) -> f32 {
        self.recorder.as_ref().map(|r| r.len_secs()).unwrap_or(0.0)
    }
    fn stop(mut self) -> Vec<f32> {
        if let Some(h) = self.handle.take() {
            h.stop();
        }
        self.recorder.take().map(|r| r.finish()).unwrap_or_default()
    }
}

enum State {
    Idle,
    /// fn is down, the stream is open, the hold threshold not yet reached.
    Armed {
        capture: Capture,
    },
    Recording {
        session: Session,
        capture: Capture,
        captured: Captured,
        locked: bool,
    },
    Processing {
        session: Session,
        captured: Captured,
        refined: Option<String>,
        failure: Option<UiMessage>,
        /// A recoverable-error notice (for example "LLM unavailable")
        /// queued while processing, paired with the error code it came
        /// from. Held here instead of shown right away because
        /// `Refined`/`Done` typically follow within milliseconds and drive
        /// the widget into `Inserting`, which clears any message already
        /// on screen; the notice is shown only after insertion finishes
        /// and the widget is back at `Idle`. It is also only ever shown
        /// after a real insertion (`Outcome::Inserted`): a `Shown` outcome
        /// or a failure already occupies the widget's one message slot
        /// with a copyable or an error message, and showing the notice on
        /// top of that would silently replace it. Every degraded insertion
        /// shows it again, so the user always learns why the inserted text
        /// is the raw transcript.
        notice: Option<UiMessage>,
    },
}

pub struct Controller {
    deps: Deps,
    engine: Option<Engine>,
    settings: ClientSettings,
    pending_settings: Option<ClientSettings>,
    recognizer: Recognizer,
    state: State,
    tx: Sender<ControllerMsg>,
}

impl Controller {
    pub fn new(
        deps: Deps,
        settings: ClientSettings,
        engine: Option<Engine>,
        tx: Sender<ControllerMsg>,
    ) -> Self {
        let recognizer = Recognizer::new(&settings.hotkey);
        Self {
            deps,
            engine,
            settings,
            pending_settings: None,
            recognizer,
            state: State::Idle,
            tx,
        }
    }

    pub fn run(&mut self, rx: Receiver<ControllerMsg>) {
        loop {
            let timeout = self
                .recognizer
                .next_deadline()
                .map(|d| d.saturating_duration_since(Instant::now()))
                .unwrap_or(Duration::from_secs(3600));
            match rx.recv_timeout(timeout) {
                Ok(ControllerMsg::Shutdown) => break,
                Ok(msg) => self.handle(msg),
                Err(RecvTimeoutError::Timeout) => {
                    let gestures = self.recognizer.on_tick(Instant::now());
                    for g in gestures {
                        self.on_gesture(g);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    fn handle(&mut self, msg: ControllerMsg) {
        match msg {
            ControllerMsg::Modifier(ev) => self.on_modifier(ev),
            ControllerMsg::Audio(a) => self.on_audio(a),
            ControllerMsg::Engine { session_id, event } => self.on_engine_event(session_id, event),
            ControllerMsg::Settings(s) => {
                if matches!(self.state, State::Idle) {
                    self.pending_settings = None;
                    self.apply_settings(s);
                } else {
                    self.pending_settings = Some(s);
                }
            }
            ControllerMsg::EngineReady(e) => self.engine = Some(e),
            ControllerMsg::Shutdown => {}
        }
    }

    fn apply_settings(&mut self, s: ClientSettings) {
        self.recognizer = Recognizer::new(&s.hotkey);
        self.settings = s;
    }

    fn go_idle(&mut self) {
        self.deps.ui.set_state(UiState::Idle);
        self.settle_idle();
    }

    /// Returns to `Idle` and applies any settings queued while busy,
    /// without emitting a `UiState::Idle` message. Used by `go_idle` and by
    /// every other path that leaves the controller idle without a
    /// user-visible "done" transition (armed-stream teardown, engine
    /// unavailable, a microphone error while armed), so a settings change
    /// sent while busy is never dropped regardless of which path returns
    /// the controller to `Idle`.
    fn settle_idle(&mut self) {
        self.state = State::Idle;
        if let Some(s) = self.pending_settings.take() {
            self.apply_settings(s);
        }
    }

    fn on_modifier(&mut self, ev: ModifierEvent) {
        // The event's own timestamp, not `Instant::now()`: the controller
        // thread can be busy (for example opening the microphone stream)
        // for tens of milliseconds after the OS event actually happened,
        // and the hold/tap thresholds must be measured from the latter.
        let now = ev.at();
        let input = match ev {
            ModifierEvent::FnDown { .. } => {
                if matches!(self.state, State::Idle) {
                    self.arm();
                }
                Input::FnDown
            }
            ModifierEvent::FnUp { .. } => Input::FnUp,
            ModifierEvent::KeyDown { key_code, .. } if key_code == KEY_ESCAPE => {
                self.cancel_active();
                Input::Escape
            }
            ModifierEvent::KeyDown { key_code, .. } => Input::KeyDown { key_code },
        };
        let gestures = self.recognizer.on_input(input, now);
        for g in gestures {
            self.on_gesture(g);
        }
        // A key press while fn is down aborts and the recognizer holds
        // `Aborted` until fn goes up, which `Gesture::Abort` handles below.
        // But a key press that lands after fn is already up (during a tap's
        // double-tap window) drops the recognizer straight to `Idle` with
        // no gesture at all, so an `Armed` stream opened by the tap's
        // `FnDown` is never closed by `on_gesture`. Close it here whenever
        // the recognizer is fully at rest but the state machine is still
        // `Armed`.
        if matches!(self.state, State::Armed { .. })
            && !self.recognizer.fn_is_down()
            && self.recognizer.next_deadline().is_none()
        {
            if let State::Armed { capture } = std::mem::replace(&mut self.state, State::Idle) {
                capture.stop();
            }
            self.settle_idle();
        }
    }

    /// Opens the microphone stream so the first syllable is not lost while
    /// the hold threshold elapses.
    fn arm(&mut self) {
        match self
            .deps
            .audio
            .start(&self.settings.microphone_choice(), self.audio_tx())
        {
            Ok(handle) => {
                self.state = State::Armed {
                    capture: Capture {
                        handle: Some(handle),
                        recorder: None,
                        pause: None,
                    },
                };
            }
            Err(e) => {
                tracing::warn!(error = %e, "audio start failed");
                self.deps.ui.message(UiMessage::error(
                    Msg::WidgetMicrophoneUnavailable,
                    Some(e.to_string()),
                ));
            }
        }
    }

    fn audio_tx(&self) -> Sender<AudioMsg> {
        super::forward(self.tx.clone(), ControllerMsg::Audio)
    }

    fn on_gesture(&mut self, g: Gesture) {
        match g {
            Gesture::HoldStart => self.start_recording(false),
            Gesture::DoubleTap => self.start_recording(true),
            Gesture::HoldEnd => {
                if matches!(self.state, State::Recording { .. }) {
                    self.finish_recording();
                }
            }
            Gesture::Tap => match std::mem::replace(&mut self.state, State::Idle) {
                State::Recording {
                    locked: true,
                    session,
                    capture,
                    captured,
                } => {
                    self.state = State::Recording {
                        locked: true,
                        session,
                        capture,
                        captured,
                    };
                    self.finish_recording();
                }
                State::Armed { capture } => {
                    capture.stop();
                    self.go_idle();
                }
                other => self.state = other,
            },
            Gesture::Abort => match std::mem::replace(&mut self.state, State::Idle) {
                State::Armed { capture } => {
                    capture.stop();
                    self.settle_idle();
                }
                State::Recording {
                    session, capture, ..
                } => {
                    capture.stop();
                    session.cancel();
                    self.deps.ui.message(UiMessage::info(Msg::WidgetCancelled));
                    self.go_idle();
                }
                other => self.state = other,
            },
        }
    }

    fn start_recording(&mut self, locked: bool) {
        // A double tap arrives with the stream already open from the first
        // tap's FnDown (Armed), or from Idle when the taps were fast.
        let capture = match std::mem::replace(&mut self.state, State::Idle) {
            State::Armed { capture } => Some(capture),
            State::Idle => None,
            other => {
                // Already recording or processing: ignore the gesture.
                self.state = other;
                return;
            }
        };
        let Some(engine) = self.engine.clone() else {
            if let Some(c) = capture {
                c.stop();
            }
            self.deps
                .ui
                .message(UiMessage::error(Msg::WidgetEngineUnavailable, None));
            self.settle_idle();
            return;
        };
        let capture = match capture {
            Some(c) => c,
            None => match self
                .deps
                .audio
                .start(&self.settings.microphone_choice(), self.audio_tx())
            {
                Ok(handle) => Capture {
                    handle: Some(handle),
                    recorder: None,
                    pause: None,
                },
                Err(e) => {
                    self.deps.ui.message(UiMessage::error(
                        Msg::WidgetMicrophoneUnavailable,
                        Some(e.to_string()),
                    ));
                    self.state = State::Idle;
                    return;
                }
            },
        };
        let mut capture = capture;
        let auto_stop = &self.settings.auto_stop;
        if locked && auto_stop.enabled {
            // Audio captured before this point (the first tap) is looked at too.
            capture.pause = Some(vad::SilenceWatch::new(
                (self.deps.vad_factory)(),
                auto_stop.silence_secs,
                arcvox_core::SAMPLE_RATE,
            ));
        }
        let captured = context::capture(self.deps.platform.as_ref(), &self.settings.context);
        let session = engine.start_session(SessionRequest {
            app: captured.app.clone(),
            context: captured.context.clone(),
            profile_override: None,
        });
        self.forward_events(&session);
        self.deps.ui.set_state(if locked {
            UiState::Locked
        } else {
            UiState::Recording
        });
        self.state = State::Recording {
            session,
            capture,
            captured,
            locked,
        };
    }

    /// Streams the session's events back into this controller.
    fn forward_events(&self, session: &Session) {
        let mut events = session.events();
        let id = session.id().clone();
        let tx = self.tx.clone();
        self.deps.runtime.spawn(async move {
            while let Some(event) = events.next().await {
                let done = matches!(event, Event::Done { .. });
                if tx
                    .send(ControllerMsg::Engine {
                        session_id: id.clone(),
                        event,
                    })
                    .is_err()
                    || done
                {
                    break;
                }
            }
        });
    }

    fn on_audio(&mut self, msg: AudioMsg) {
        match msg {
            AudioMsg::Frames { rate, mono } => {
                let max = self.settings.max_utterance_secs as f32;
                match &mut self.state {
                    State::Armed { capture } => {
                        capture.push(rate, &mono);
                    }
                    State::Recording { capture, .. } => {
                        let level = capture.push(rate, &mono);
                        self.deps.ui.level(level.min(1.0));
                        if capture.secs() >= max || capture.paused() {
                            self.finish_recording();
                        }
                    }
                    _ => {}
                }
            }
            // The stream keeps delivering audio; the recording goes on.
            AudioMsg::Glitch(e) => tracing::debug!(error = %e, "audio stream glitch"),
            AudioMsg::Error(e) => {
                tracing::warn!(error = %e, "audio stream error");
                match std::mem::replace(&mut self.state, State::Idle) {
                    State::Armed { capture } => {
                        capture.stop();
                        self.settle_idle();
                    }
                    State::Recording {
                        session,
                        capture,
                        captured,
                        locked,
                    } => {
                        self.state = State::Recording {
                            session,
                            capture,
                            captured,
                            locked,
                        };
                        self.finish_recording();
                    }
                    other => self.state = other,
                }
            }
        }
    }

    fn finish_recording(&mut self) {
        let State::Recording {
            session,
            capture,
            captured,
            ..
        } = std::mem::replace(&mut self.state, State::Idle)
        else {
            return;
        };
        let samples = capture.stop();
        let mut vad = (self.deps.vad_factory)();
        match vad::trim(&samples, vad.as_mut()) {
            None => {
                session.cancel();
                self.deps.ui.message(UiMessage::info(Msg::WidgetNoSpeech));
                self.go_idle();
            }
            Some(range) => {
                session.push_audio(&samples[range]);
                session.finish();
                self.deps.ui.set_state(UiState::Processing);
                self.state = State::Processing {
                    session,
                    captured,
                    refined: None,
                    failure: None,
                    notice: None,
                };
            }
        }
    }

    fn cancel_active(&mut self) {
        match std::mem::replace(&mut self.state, State::Idle) {
            State::Recording {
                session, capture, ..
            } => {
                capture.stop();
                session.cancel();
                self.deps.ui.message(UiMessage::info(Msg::WidgetCancelled));
                self.go_idle();
            }
            State::Processing { session, .. } => {
                session.cancel();
                self.deps.ui.message(UiMessage::info(Msg::WidgetCancelled));
                self.go_idle();
            }
            other => self.state = other,
        }
    }

    fn on_engine_event(&mut self, session_id: SessionId, event: Event) {
        let State::Processing { session, .. } = &self.state else {
            return;
        };
        if session.id() != &session_id {
            return;
        }
        match event {
            Event::Refined { text, .. } => {
                if let State::Processing { refined, .. } = &mut self.state {
                    *refined = Some(text);
                }
            }
            Event::Error {
                message,
                recoverable: true,
                hint,
                ..
            } => {
                // Queued rather than shown immediately: `Refined`/`Done`
                // typically follow within milliseconds and drive the
                // widget into `Inserting`, which would wipe this notice
                // before anyone could read it. `on_done`/`insert` show
                // it once insertion has actually finished and the
                // widget is idle again.
                if let State::Processing { notice, .. } = &mut self.state {
                    *notice = Some(UiMessage::error(
                        Msg::WidgetDegraded,
                        hint.or(Some(message)),
                    ));
                }
            }
            Event::Error {
                code,
                message,
                recoverable: false,
                hint,
                ..
            } => {
                // Non-recoverable provider errors come from the ASR step
                // (LLM failures degrade instead); the core's hint names
                // the provider and goes into the detail line.
                let msg = match code {
                    ErrorCode::NoSpeech | ErrorCode::NoAudio => Msg::WidgetNoSpeech,
                    ErrorCode::ProviderUnreachable
                    | ErrorCode::ProviderTimeout
                    | ErrorCode::ProviderUnauthorized
                    | ErrorCode::ProviderError
                    | ErrorCode::InferenceFailed => Msg::WidgetAsrFailed,
                    ErrorCode::ModelMissing => Msg::WidgetModelMissing,
                    ErrorCode::ModelCorrupt => Msg::WidgetModelCorrupt,
                    ErrorCode::NativeUnavailable => Msg::WidgetNativeUnavailable,
                    _ => Msg::WidgetEngineUnavailable,
                };
                let detail = hint.unwrap_or(message);
                // A failure stays until dismissed, with the detail available
                // to copy; an utterance without speech needs no action.
                let failure = if matches!(code, ErrorCode::NoSpeech | ErrorCode::NoAudio) {
                    UiMessage::error(msg, Some(detail))
                } else {
                    // The core keeps a failed dictation only with recordings on.
                    let kept = self
                        .engine
                        .as_ref()
                        .is_some_and(|e| e.config().current().general.keep_recordings);
                    UiMessage::failure(msg, detail, code, kept)
                };
                if let State::Processing { failure: f, .. } = &mut self.state {
                    *f = Some(failure);
                }
            }
            Event::Transcript {
                text,
                is_final: true,
                ..
            } => self.deps.ui.transcript(text),
            Event::Done { .. } => self.on_done(),
            _ => {}
        }
    }

    fn on_done(&mut self) {
        let State::Processing {
            session,
            captured,
            refined,
            failure,
            notice,
        } = std::mem::replace(&mut self.state, State::Idle)
        else {
            return;
        };
        match refined {
            Some(text) => self.insert(session, captured, text, notice),
            None => {
                let msg = failure.unwrap_or_else(|| UiMessage::error(Msg::WidgetEmptyResult, None));
                self.deps.ui.message(msg);
                self.go_idle();
                // The notice is dropped, not shown, on this failure path:
                // `msg` above already occupies the widget's one message
                // slot.
            }
        }
    }

    fn insert(
        &mut self,
        session: Session,
        captured: Captured,
        text: String,
        notice: Option<UiMessage>,
    ) {
        self.deps.ui.set_state(UiState::Inserting);
        let frontmost = self.deps.platform.frontmost_app();
        let method = self
            .settings
            .method_for(captured.target.bundle_id.as_deref());
        let plan = insert::plan(&Situation {
            text: &text,
            target: &captured.target,
            frontmost: frontmost.as_ref(),
            secure_input: self.deps.platform.secure_input_active(),
            method,
        });
        let outcome = insert::run(&plan, &text, self.deps.platform.as_ref());
        // Only a real insertion, or a copyable show the user can act on,
        // is worth recording as the final text; an empty result has
        // nothing usable to record.
        let should_record = match &outcome {
            Outcome::Inserted => true,
            Outcome::Shown(reason) => {
                let msg = match reason {
                    ShowReason::SecureInput => Msg::WidgetSecureInput,
                    ShowReason::AppChanged => Msg::WidgetAppChanged,
                    ShowReason::EmptyText => Msg::WidgetEmptyResult,
                };
                if matches!(reason, ShowReason::EmptyText) {
                    self.deps.ui.message(UiMessage::error(msg, None));
                    false
                } else {
                    self.deps.ui.message(UiMessage::copyable(msg, text.clone()));
                    true
                }
            }
        };
        if should_record {
            if let Some(engine) = self.engine.clone() {
                let id = session.id().clone();
                let final_text = text.clone();
                self.deps.runtime.spawn(async move {
                    if let Err(e) = engine.record_final(&id, &final_text).await {
                        tracing::debug!(error = %e, "record_final failed");
                    }
                });
            }
        }
        self.go_idle();
        // Shown only for a real insertion: `Outcome::Shown` already used
        // the widget's one message slot for a copyable or empty-result
        // message above, and showing the notice on top of that would
        // silently replace it.
        if matches!(outcome, Outcome::Inserted) {
            if let Some(n) = notice {
                self.deps.ui.message(n);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{AudioError, AudioSource};
    use crate::dictation::ui::fake::FakeUi;
    use crate::dictation::ui::MessageKind;
    use crate::dictation::{spawn, ControllerHandle};
    use crate::platform::fake::FakePlatform;
    use crate::platform::FrontmostApp;
    use crate::settings::{HotkeySettings, InsertMethod};
    use crate::vad::Vad;
    use arcvox_core::{DataDir, MemorySecretStore};
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use url::Url;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct PeakVad;
    impl Vad for PeakVad {
        fn is_speech(&mut self, frame: &[f32]) -> bool {
            frame.iter().any(|s| s.abs() > 0.1)
        }
    }

    #[derive(Default)]
    struct FakeAudio {
        tx: Mutex<Option<Sender<AudioMsg>>>,
        fail: Mutex<bool>,
        starts: Mutex<usize>,
        stops: Arc<Mutex<usize>>,
        /// How long `start` blocks before returning, simulating a slow
        /// microphone open. Zero by default so existing tests are unaffected.
        delay: Mutex<Duration>,
    }
    struct FakeHandle(Arc<Mutex<usize>>);
    impl AudioHandle for FakeHandle {
        fn stop(self: Box<Self>) {
            *self.0.lock().unwrap() += 1;
        }
    }
    impl AudioSource for FakeAudio {
        fn start(
            &self,
            _c: &crate::settings::MicrophoneChoice,
            tx: Sender<AudioMsg>,
        ) -> Result<Box<dyn AudioHandle>, AudioError> {
            if *self.fail.lock().unwrap() {
                return Err(AudioError::NoDevice);
            }
            let delay = *self.delay.lock().unwrap();
            if !delay.is_zero() {
                std::thread::sleep(delay);
            }
            *self.starts.lock().unwrap() += 1;
            *self.tx.lock().unwrap() = Some(tx);
            Ok(Box::new(FakeHandle(self.stops.clone())))
        }
        fn list_devices(&self) -> Vec<String> {
            vec!["Fake".into()]
        }
    }
    impl FakeAudio {
        fn speech(&self, secs: f32) {
            let n = (16_000.0 * secs) as usize;
            let mono: Vec<f32> = (0..n)
                .map(|i| if i % 2 == 0 { 0.5 } else { -0.5 })
                .collect();
            self.tx
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .send(AudioMsg::Frames { rate: 16_000, mono })
                .unwrap();
        }
        fn silence(&self, secs: f32) {
            let n = (16_000.0 * secs) as usize;
            self.tx
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .send(AudioMsg::Frames {
                    rate: 16_000,
                    mono: vec![0.0; n],
                })
                .unwrap();
        }
        fn glitch(&self) {
            self.tx
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .send(AudioMsg::Glitch("dropout".into()))
                .unwrap();
        }
        fn error(&self) {
            self.tx
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .send(AudioMsg::Error("gone".into()))
                .unwrap();
        }
    }

    struct Rig {
        _tmp: tempfile::TempDir,
        engine: Engine,
        asr: MockServer,
        llm: MockServer,
        platform: Arc<FakePlatform>,
        audio: Arc<FakeAudio>,
        ui: Arc<FakeUi>,
        handle: ControllerHandle,
        modifier: Sender<ModifierEvent>,
    }

    fn fast_settings() -> ClientSettings {
        ClientSettings {
            hotkey: HotkeySettings {
                hold_threshold_ms: 30,
                double_tap_window_ms: 60,
            },
            ..ClientSettings::default()
        }
    }

    // `ModifierEvent` now carries the `Instant` the OS event happened at,
    // rather than the controller stamping `Instant::now()` when it gets
    // around to handling it. These helpers stamp "now" for tests that do
    // not care about the distinction; tests that do (late-handling races)
    // construct the event directly with an explicit `at`.
    fn fn_down() -> ModifierEvent {
        ModifierEvent::FnDown { at: Instant::now() }
    }
    fn fn_up() -> ModifierEvent {
        ModifierEvent::FnUp { at: Instant::now() }
    }
    fn key_down(key_code: u16) -> ModifierEvent {
        ModifierEvent::KeyDown {
            key_code,
            at: Instant::now(),
        }
    }

    async fn rig(settings: ClientSettings) -> Rig {
        rig_with(settings, Vec::new(), |_| {}).await
    }

    /// Factory whose ASR construction always fails with a given code, so a
    /// session ends in the matching non-recoverable error event.
    struct FailingAsrFactory {
        kind: arcvox_core::config::ProviderKind,
        code: ErrorCode,
    }

    #[async_trait::async_trait]
    impl arcvox_core::ProviderFactory for FailingAsrFactory {
        fn kind(&self) -> arcvox_core::config::ProviderKind {
            self.kind
        }
        fn asr(
            &self,
            _cfg: &arcvox_core::config::ProviderConfig,
            _general: &arcvox_core::config::General,
        ) -> Result<
            Arc<dyn arcvox_core::providers::AsrProvider>,
            arcvox_core::providers::ProviderError,
        > {
            Err(
                arcvox_core::providers::ProviderError::new(self.code, "provider failed")
                    .with_hint("the hint"),
            )
        }
        fn llm(
            &self,
            _cfg: &arcvox_core::config::ProviderConfig,
            _general: &arcvox_core::config::General,
        ) -> Result<
            Arc<dyn arcvox_core::providers::LlmProvider>,
            arcvox_core::providers::ProviderError,
        > {
            unreachable!("only ASR is used")
        }
        async fn probe(
            &self,
            _cfg: &arcvox_core::config::ProviderConfig,
        ) -> arcvox_core::providers::probe::ProbeResult {
            unreachable!("not probed")
        }
    }

    /// A rig whose default profile uses a provider of `kind` that fails with
    /// `code` when the session builds its ASR step.
    async fn failing_rig(kind: arcvox_core::config::ProviderKind, code: ErrorCode) -> Rig {
        rig_with(
            fast_settings(),
            vec![Arc::new(FailingAsrFactory { kind, code })],
            move |c| {
                use arcvox_core::config::{Capabilities, ProviderConfig, ProviderKind};
                let caps = Capabilities {
                    transcribe: true,
                    chat: false,
                    streaming: false,
                    language_hint: false,
                    hotwords: false,
                };
                let p = match kind {
                    ProviderKind::Native => ProviderConfig::native("failing", "Failing", "x", caps),
                    _ => ProviderConfig::embedded(
                        "failing",
                        "Failing",
                        arcvox_core::config::Runtime::Whisper,
                        "missing-model",
                        caps,
                    ),
                };
                c.providers.push(p);
                c.profiles[0].asr_provider = arcvox_core::types::ProviderId("failing".into());
            },
        )
        .await
    }

    async fn rig_with(
        settings: ClientSettings,
        factories: Vec<Arc<dyn arcvox_core::ProviderFactory>>,
        tweak: impl FnOnce(&mut arcvox_core::config::Config),
    ) -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let asr = MockServer::start().await;
        let llm = MockServer::start().await;
        let engine = Engine::open_with(
            DataDir::custom(tmp.path()),
            arcvox_core::EngineOptions {
                secrets: Arc::new(MemorySecretStore::default()),
                factories,
                first_run: Default::default(),
            },
        )
        .await
        .unwrap();
        engine
            .config()
            .update(|c| {
                *c = arcvox_core::config::Config::http_test_config();
                c.providers[0].base_url = Some(Url::parse(&format!("{}/v1", asr.uri())).unwrap());
                c.providers[1].base_url = Some(Url::parse(&format!("{}/v1", llm.uri())).unwrap());
                tweak(c);
            })
            .unwrap();
        let platform = Arc::new(FakePlatform::new());
        *platform.frontmost.lock().unwrap() = Some(FrontmostApp {
            pid: 7,
            bundle_id: Some("com.apple.TextEdit".into()),
            name: Some("TextEdit".into()),
            ..Default::default()
        });
        let audio = Arc::new(FakeAudio::default());
        let ui = Arc::new(FakeUi::default());
        let deps = Deps {
            platform: platform.clone(),
            audio: audio.clone(),
            ui: ui.clone(),
            vad_factory: Box::new(|| Box::new(PeakVad)),
            runtime: tokio::runtime::Handle::current(),
        };
        let handle = spawn(deps, settings, Some(engine.clone()));
        let modifier = handle.modifier_sender();
        Rig {
            _tmp: tmp,
            engine,
            asr,
            llm,
            platform,
            audio,
            ui,
            handle,
            modifier,
        }
    }

    async fn mock_ok(r: &Rig, asr_text: &str, llm_text: &str) {
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text": asr_text})))
            .mount(&r.asr)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"choices":[{"message":{"content": llm_text}}]})),
            )
            .mount(&r.llm)
            .await;
    }

    async fn wait_for(mut cond: impl FnMut() -> bool, what: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !cond() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn wait_state(r: &Rig, s: UiState, what: &str) {
        wait_for(|| r.ui.states.lock().unwrap().contains(&s), what).await;
    }

    /// Sends a speech block and waits for the controller to have actually
    /// folded it into the recording capture (observed through a new UI
    /// level) before returning. `AudioSource::start` and `modifier_sender`
    /// each hand their sender to a separate forwarding thread, so sending a
    /// speech block and the following `FnUp` back to back does not
    /// guarantee the controller sees them in that order.
    async fn speak_and_wait(r: &Rig, secs: f32) {
        let before = r.ui.levels.lock().unwrap().len();
        r.audio.speech(secs);
        wait_for(
            || r.ui.levels.lock().unwrap().len() > before,
            "level update",
        )
        .await;
    }

    async fn hold_and_speak(r: &Rig, secs: f32) {
        r.modifier.send(fn_down()).unwrap();
        wait_state(r, UiState::Recording, "recording").await;
        speak_and_wait(r, secs).await;
        r.modifier.send(fn_up()).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn final_transcript_reaches_the_ui() {
        let r = rig(fast_settings()).await;
        mock_ok(&r, "软件 原文", "软件 整理後").await;
        hold_and_speak(&r, 1.0).await;
        wait_state(&r, UiState::Idle, "idle").await;
        // The raw ASR output, sent before the OpenCC step runs.
        let transcripts = r.ui.transcripts.lock().unwrap().clone();
        assert_eq!(transcripts, vec!["软件 原文".to_string()]);
        let log = r.ui.log.lock().unwrap().clone();
        let t = log
            .iter()
            .position(|l| l.starts_with("transcript:"))
            .unwrap();
        let inserting = log.iter().position(|l| l == "state:inserting").unwrap();
        assert!(t < inserting);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn push_to_talk_inserts_refined_text_and_records_final() {
        let r = rig(fast_settings()).await;
        mock_ok(&r, "软件 原文", "软件 整理後").await;
        hold_and_speak(&r, 1.0).await;
        // Wait for the clipboard restore, not just the paste: `paste()` in
        // `insert::executors` sleeps for `PASTE_RESTORE_DELAY` between the
        // two, so waiting only on "paste" races the assertion below.
        wait_for(
            || r.platform.calls().contains(&"restore".to_string()),
            "restore",
        )
        .await;
        assert_eq!(
            r.platform.calls(),
            vec!["snapshot", "write:軟體 整理後", "paste", "restore"]
        );
        wait_state(&r, UiState::Idle, "idle").await;
        let states = r.ui.states.lock().unwrap().clone();
        assert_eq!(
            states,
            vec![
                UiState::Recording,
                UiState::Processing,
                UiState::Inserting,
                UiState::Idle
            ]
        );
        assert!(!r.ui.levels.lock().unwrap().is_empty());
        wait_for(
            || {
                r.engine
                    .history()
                    .list_recent(1)
                    .unwrap()
                    .first()
                    .map(|u| u.final_text == "軟體 整理後")
                    .unwrap_or(false)
            },
            "record_final",
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn locked_mode_finishes_after_a_pause_when_set() {
        let mut s = fast_settings();
        s.auto_stop = crate::settings::AutoStop {
            enabled: true,
            silence_secs: 1,
        };
        let r = rig(s).await;
        mock_ok(&r, "一", "一").await;
        r.modifier.send(fn_down()).unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        r.modifier.send(fn_up()).unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        r.modifier.send(fn_down()).unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        r.modifier.send(fn_up()).unwrap();
        wait_state(&r, UiState::Locked, "locked").await;
        r.audio.silence(1.5);
        r.audio.speech(0.5);
        r.audio.silence(0.5);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !r.ui.states.lock().unwrap().contains(&UiState::Processing),
            "a pause shorter than the setting keeps recording"
        );
        r.audio.silence(0.6);
        wait_for(
            || r.platform.calls().contains(&"paste".to_string()),
            "paste without a closing tap",
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_dropout_does_not_end_the_recording() {
        let r = rig(fast_settings()).await;
        mock_ok(&r, "一", "一").await;
        r.modifier.send(fn_down()).unwrap();
        wait_state(&r, UiState::Recording, "recording").await;
        r.audio.speech(0.5);
        r.audio.glitch();
        r.audio.speech(0.5);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !r.ui.states.lock().unwrap().contains(&UiState::Processing),
            "the recording goes on after a dropout"
        );
        r.modifier.send(fn_up()).unwrap();
        wait_for(
            || r.platform.calls().contains(&"paste".to_string()),
            "paste",
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn locked_mode_finishes_on_tap() {
        let r = rig(fast_settings()).await;
        mock_ok(&r, "一", "一").await;
        r.modifier.send(fn_down()).unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        r.modifier.send(fn_up()).unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        r.modifier.send(fn_down()).unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        r.modifier.send(fn_up()).unwrap();
        wait_state(&r, UiState::Locked, "locked").await;
        r.audio.speech(0.5);
        r.modifier.send(fn_down()).unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        r.modifier.send(fn_up()).unwrap();
        wait_for(
            || r.platform.calls().contains(&"paste".to_string()),
            "paste",
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn locked_mode_finishes_on_a_long_press() {
        let r = rig(fast_settings()).await;
        mock_ok(&r, "一", "一").await;
        r.modifier.send(fn_down()).unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        r.modifier.send(fn_up()).unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        r.modifier.send(fn_down()).unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        r.modifier.send(fn_up()).unwrap();
        wait_state(&r, UiState::Locked, "locked").await;
        r.audio.speech(0.5);
        // A press past the hold threshold while already locked must also
        // finish the recording, not just a tap.
        r.modifier.send(fn_down()).unwrap();
        tokio::time::sleep(Duration::from_millis(60)).await;
        r.modifier.send(fn_up()).unwrap();
        wait_for(
            || r.platform.calls().contains(&"paste".to_string()),
            "paste",
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn silence_only_cancels_without_calling_providers() {
        let r = rig(fast_settings()).await;
        r.modifier.send(fn_down()).unwrap();
        wait_state(&r, UiState::Recording, "recording").await;
        r.audio.silence(1.0);
        r.modifier.send(fn_up()).unwrap();
        wait_for(
            || {
                r.ui.messages
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|m| m.msg == Msg::WidgetNoSpeech)
            },
            "no speech",
        )
        .await;
        assert!(r.asr.received_requests().await.unwrap().is_empty());
        assert!(r.platform.calls().is_empty());
        assert_eq!(*r.audio.stops.lock().unwrap(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn escape_cancels_recording_and_processing() {
        let r = rig(fast_settings()).await;
        mock_ok(&r, "x", "x").await;
        r.modifier.send(fn_down()).unwrap();
        wait_state(&r, UiState::Recording, "recording").await;
        r.audio.speech(0.3);
        r.modifier.send(key_down(KEY_ESCAPE)).unwrap();
        wait_for(
            || {
                r.ui.messages
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|m| m.msg == Msg::WidgetCancelled)
            },
            "cancelled",
        )
        .await;
        r.modifier.send(fn_up()).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(r.platform.calls().is_empty());
        assert!(r.asr.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn empty_refined_text_shows_message_and_does_not_insert() {
        let r = rig(fast_settings()).await;
        mock_ok(&r, "   ", "   ").await;
        hold_and_speak(&r, 0.5).await;
        wait_for(|| !r.ui.messages.lock().unwrap().is_empty(), "message").await;
        wait_state(&r, UiState::Idle, "idle").await;
        let msgs = r.ui.messages.lock().unwrap().clone();
        assert!(
            msgs.iter()
                .any(|m| m.msg == Msg::WidgetNoSpeech || m.msg == Msg::WidgetEmptyResult),
            "{msgs:?}"
        );
        assert!(!r.platform.calls().contains(&"paste".to_string()));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn secure_input_and_app_change_show_copyable_text() {
        let r = rig(fast_settings()).await;
        mock_ok(&r, "a", "文字").await;
        *r.platform.secure_input.lock().unwrap() = true;
        hold_and_speak(&r, 0.5).await;
        wait_for(
            || {
                r.ui.messages
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|m| m.copyable.is_some())
            },
            "copyable",
        )
        .await;
        let m =
            r.ui.messages
                .lock()
                .unwrap()
                .iter()
                .find(|m| m.copyable.is_some())
                .cloned()
                .unwrap();
        assert_eq!(m.msg, Msg::WidgetSecureInput);
        assert_eq!(m.copyable.as_deref(), Some("文字"));
        assert!(r.platform.calls().is_empty());

        *r.platform.secure_input.lock().unwrap() = false;
        r.ui.messages.lock().unwrap().clear();
        r.ui.states.lock().unwrap().clear();
        r.modifier.send(fn_down()).unwrap();
        wait_state(&r, UiState::Recording, "recording").await;
        // The user switches apps while dictating: the target was captured at
        // the start, the frontmost app differs at insertion time.
        *r.platform.frontmost.lock().unwrap() = Some(FrontmostApp {
            pid: 99,
            bundle_id: Some("com.other".into()),
            ..Default::default()
        });
        speak_and_wait(&r, 0.5).await;
        r.modifier.send(fn_up()).unwrap();
        wait_for(
            || {
                r.ui.messages
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|m| m.msg == Msg::WidgetAppChanged)
            },
            "app changed",
        )
        .await;
        assert!(r.platform.calls().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn audio_error_during_recording_finishes_early() {
        let r = rig(fast_settings()).await;
        mock_ok(&r, "a", "b").await;
        r.modifier.send(fn_down()).unwrap();
        wait_state(&r, UiState::Recording, "recording").await;
        r.audio.speech(0.5);
        r.audio.error();
        wait_for(
            || r.platform.calls().contains(&"paste".to_string()),
            "paste after device loss",
        )
        .await;
        r.modifier.send(fn_up()).unwrap();
        wait_state(&r, UiState::Idle, "idle").await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn settings_change_applies_to_next_session() {
        let r = rig(fast_settings()).await;
        mock_ok(&r, "a", "b").await;
        r.modifier.send(fn_down()).unwrap();
        wait_state(&r, UiState::Recording, "recording").await;
        let mut s = fast_settings();
        s.insertion
            .per_app
            .insert("com.apple.TextEdit".into(), InsertMethod::Type);
        r.handle.update_settings(s);
        speak_and_wait(&r, 0.5).await;
        r.modifier.send(fn_up()).unwrap();
        wait_for(
            || r.platform.calls().contains(&"paste".to_string()),
            "first session pastes",
        )
        .await;
        wait_state(&r, UiState::Idle, "idle").await;
        r.platform.calls.lock().unwrap().clear();
        // `wait_state` looks for the state anywhere in the recorded history,
        // so the `Recording` entry from the first session must be cleared
        // too; otherwise `hold_and_speak`'s wait for `Recording` below
        // returns immediately on that stale entry, before the second
        // session's hold threshold has elapsed.
        r.ui.states.lock().unwrap().clear();
        hold_and_speak(&r, 0.5).await;
        wait_for(
            || r.platform.calls().iter().any(|c| c.starts_with("type:")),
            "second session types",
        )
        .await;
        assert_eq!(r.platform.calls(), vec!["type:b"]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn max_utterance_length_finishes_locked_recording() {
        let mut s = fast_settings();
        s.max_utterance_secs = 1;
        let r = rig(s).await;
        mock_ok(&r, "a", "b").await;
        r.modifier.send(fn_down()).unwrap();
        wait_state(&r, UiState::Recording, "recording").await;
        r.audio.speech(0.6);
        r.audio.speech(0.6);
        wait_state(&r, UiState::Processing, "processing without release").await;
        r.modifier.send(fn_up()).unwrap();
        wait_for(
            || r.platform.calls().contains(&"paste".to_string()),
            "paste",
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn no_engine_reports_and_stays_idle() {
        let tmp = tempfile::tempdir().unwrap();
        let platform = Arc::new(FakePlatform::new());
        let audio = Arc::new(FakeAudio::default());
        let ui = Arc::new(FakeUi::default());
        let handle = spawn(
            Deps {
                platform,
                audio: audio.clone(),
                ui: ui.clone(),
                vad_factory: Box::new(|| Box::new(PeakVad)),
                runtime: tokio::runtime::Handle::current(),
            },
            fast_settings(),
            None,
        );
        let m = handle.modifier_sender();
        m.send(fn_down()).unwrap();
        wait_for(
            || {
                ui.messages
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|x| x.msg == Msg::WidgetEngineUnavailable)
            },
            "engine unavailable",
        )
        .await;
        m.send(fn_up()).unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            *audio.stops.lock().unwrap(),
            1,
            "the armed stream was closed"
        );
        drop(tmp);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tap_then_key_press_closes_the_armed_stream() {
        let r = rig(fast_settings()).await;
        r.modifier.send(fn_down()).unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        r.modifier.send(fn_up()).unwrap();
        r.modifier.send(key_down(0)).unwrap();
        wait_for(
            || *r.audio.stops.lock().unwrap() == 1,
            "armed stream closed",
        )
        .await;
        assert!(r.asr.received_requests().await.unwrap().is_empty());
        assert!(!r.ui.states.lock().unwrap().contains(&UiState::Recording));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn settings_sent_while_armed_apply_after_abort() {
        let r = rig(fast_settings()).await;
        mock_ok(&r, "a", "b").await;
        r.modifier.send(fn_down()).unwrap();
        // Wait for `arm()` to actually open the stream before sending the
        // settings change, so it is genuinely queued as pending (the
        // controller busy, not Idle) rather than applied immediately.
        wait_for(|| *r.audio.starts.lock().unwrap() == 1, "armed").await;
        let mut s = fast_settings();
        s.insertion
            .per_app
            .insert("com.apple.TextEdit".into(), InsertMethod::Type);
        r.handle.update_settings(s);
        r.modifier.send(key_down(0)).unwrap();
        r.modifier.send(fn_up()).unwrap();
        hold_and_speak(&r, 0.5).await;
        wait_for(
            || r.platform.calls().iter().any(|c| c.starts_with("type:")),
            "second session types",
        )
        .await;
        assert_eq!(r.platform.calls(), vec!["type:b"]);
    }

    async fn first_failure(r: &Rig) -> UiMessage {
        hold_and_speak(r, 1.0).await;
        wait_for(
            || {
                r.ui.messages
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|m| m.msg != Msg::WidgetNoSpeech)
            },
            "failure message",
        )
        .await;
        let messages = r.ui.messages.lock().unwrap().clone();
        messages
            .into_iter()
            .find(|m| m.msg != Msg::WidgetNoSpeech)
            .expect("a message was recorded")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn model_missing_error_shows_copyable_model_message() {
        let r = failing_rig(
            arcvox_core::config::ProviderKind::Embedded,
            ErrorCode::ModelMissing,
        )
        .await;
        let m = first_failure(&r).await;
        assert_eq!(m.msg, Msg::WidgetModelMissing, "{m:?}");
        assert_eq!(m.copyable.as_deref(), Some("the hint"), "{m:?}");
        assert!(r.platform.calls().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_unavailable_error_maps_to_its_message() {
        let r = failing_rig(
            arcvox_core::config::ProviderKind::Native,
            ErrorCode::NativeUnavailable,
        )
        .await;
        let m = first_failure(&r).await;
        assert_eq!(m.msg, Msg::WidgetNativeUnavailable, "{m:?}");
        assert!(m.copyable.is_some(), "{m:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn asr_failure_shows_the_recognition_message_with_the_core_detail() {
        let r = rig(fast_settings()).await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&r.asr)
            .await;
        hold_and_speak(&r, 1.0).await;
        wait_for(
            || {
                r.ui.messages
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|m| m.kind == MessageKind::Error && m.msg != Msg::WidgetNoSpeech)
            },
            "asr failure message",
        )
        .await;
        let messages = r.ui.messages.lock().unwrap().clone();
        let m = messages
            .iter()
            .find(|m| m.kind == MessageKind::Error)
            .expect("an error message was recorded");
        assert_eq!(m.msg, Msg::WidgetAsrFailed, "{messages:?}");
        // The detail can be copied and the message stays until dismissed.
        assert!(
            m.copyable.as_deref().is_some_and(|d| d.contains("500")),
            "{messages:?}"
        );
        assert!(m.sticky, "{messages:?}");
        assert_eq!(m.code, Some(ErrorCode::ProviderError), "{messages:?}");
        assert!(!m.retry, "recordings are not kept in this rig");
        assert!(r.platform.calls().is_empty(), "{:?}", r.platform.calls());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn degraded_notice_is_shown_after_insertion() {
        let r = rig(fast_settings()).await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text": "软件 原文"})))
            .mount(&r.asr)
            .await;
        // The LLM step fails; the pipeline degrades to the ASR text and
        // reports a recoverable error alongside it.
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&r.llm)
            .await;
        hold_and_speak(&r, 1.0).await;
        wait_for(
            || r.platform.calls().contains(&"paste".to_string()),
            "paste",
        )
        .await;
        wait_for(
            || {
                r.ui.messages
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|m| m.msg == Msg::WidgetDegraded)
            },
            "degraded notice",
        )
        .await;
        let log = r.ui.log.lock().unwrap().clone();
        let last_idle = log
            .iter()
            .rposition(|e| e == "state:idle")
            .expect("an idle state was recorded");
        let notice = log
            .iter()
            .position(|e| e == "message:widget.degraded")
            .expect("the degraded notice was recorded");
        assert!(
            last_idle < notice,
            "the degraded notice must be recorded after the widget went idle: {log:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn degraded_notice_is_shown_on_every_degraded_insertion() {
        let r = rig(fast_settings()).await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text": "软件 原文"})))
            .mount(&r.asr)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&r.llm)
            .await;
        let degraded_count = || {
            r.ui.messages
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.msg == Msg::WidgetDegraded)
                .count()
        };
        hold_and_speak(&r, 1.0).await;
        wait_for(|| degraded_count() == 1, "first degraded notice").await;
        // `hold_and_speak` waits for the first `Recording` entry, which is
        // already there, so the second session waits for its own entry
        // before the speech block is sent.
        r.modifier.send(fn_down()).unwrap();
        wait_for(
            || {
                r.ui.states
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|s| **s == UiState::Recording)
                    .count()
                    == 2
            },
            "second recording",
        )
        .await;
        speak_and_wait(&r, 1.0).await;
        r.modifier.send(fn_up()).unwrap();
        wait_for(
            || r.platform.calls().iter().filter(|c| *c == "paste").count() == 2,
            "second paste",
        )
        .await;
        wait_for(|| degraded_count() == 2, "second degraded notice").await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn degraded_notice_does_not_replace_copyable_text() {
        let r = rig(fast_settings()).await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text": "软件 原文"})))
            .mount(&r.asr)
            .await;
        // The LLM step fails, same as `degraded_notice_is_shown_after_insertion`,
        // but this time the outcome is a copyable show (Secure Input),
        // not a real insertion: the notice must not clobber it.
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&r.llm)
            .await;
        *r.platform.secure_input.lock().unwrap() = true;
        hold_and_speak(&r, 1.0).await;
        wait_for(
            || {
                r.ui.messages
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|m| m.copyable.is_some())
            },
            "copyable",
        )
        .await;
        // Give the (buggy) trailing notice emission a chance to land if
        // the fix regresses.
        tokio::time::sleep(Duration::from_millis(200)).await;
        let messages = r.ui.messages.lock().unwrap().clone();
        let last = messages.last().expect("a message was recorded");
        assert!(last.copyable.is_some(), "{messages:?}");
        assert!(
            !messages.iter().any(|m| m.msg == Msg::WidgetDegraded),
            "{messages:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn late_handled_release_stays_a_tap() {
        let r = rig(fast_settings()).await;
        // Simulates a controller thread that is busy (opening the
        // microphone) for longer than the hold threshold, so the fn-up is
        // only handled long after it actually happened.
        *r.audio.delay.lock().unwrap() = Duration::from_millis(80);
        let t0 = Instant::now();
        r.modifier.send(ModifierEvent::FnDown { at: t0 }).unwrap();
        r.modifier
            .send(ModifierEvent::FnUp {
                at: t0 + Duration::from_millis(5),
            })
            .unwrap();
        wait_for(
            || *r.audio.stops.lock().unwrap() == 1,
            "armed stream closed",
        )
        .await;
        assert!(
            !r.ui.states.lock().unwrap().contains(&UiState::Recording),
            "a release timestamped 5ms after the press must stay a tap even \
             if the controller only gets to it 80ms later"
        );
    }
}
