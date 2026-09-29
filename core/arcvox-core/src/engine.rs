//! The crate's entry point. Owns config, history, and providers; opens sessions.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, oneshot};

use crate::audio::encode_wav_pcm16;
use crate::config::{ConfigStore, FirstRunDefaults, ProviderConfig, ProviderKind, StoreError};
use crate::history::{HistoryError, HistoryStore, NewUtterance};
use crate::paths::DataDir;
use crate::pipeline::Pipeline;
use crate::profile::resolve;
use crate::protocol::{Event, PROTOCOL_VERSION};
use crate::providers::factory::ProviderFactory;
use crate::providers::probe::{probe_http, ProbeFailure, ProbeResult};
use crate::providers::registry::ProviderRegistry;
use crate::secrets::SecretStore;
use crate::session::{Session, SessionFlags};
use crate::steps::PipelineContext;
use crate::types::{AppInfo, AudioClip, Context, ErrorCode, HistoryId, ProfileId, SessionId};
use crate::SAMPLE_RATE;

/// Shorter utterances are rejected before any provider call.
pub const MIN_UTTERANCE_SECS: f32 = 0.2;
const EVENT_BUFFER: usize = 256;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    History(#[from] HistoryError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("unknown session `{0}`")]
    UnknownSession(SessionId),
}

#[derive(Debug, Clone, Default)]
pub struct SessionRequest {
    pub app: AppInfo,
    /// Already filtered by the client's per-field opt-ins.
    pub context: Context,
    pub profile_override: Option<ProfileId>,
}

#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

struct Inner {
    dirs: DataDir,
    config: ConfigStore,
    history: Arc<HistoryStore>,
    registry: ProviderRegistry,
    client: reqwest::Client,
    secrets: Arc<dyn SecretStore>,
    sessions: Mutex<HashMap<SessionId, HistoryId>>,
    /// Used by `start_session` so it can be called from a non-runtime thread
    /// (for example a platform hotkey callback) rather than only from
    /// inside an `async fn` already running on this runtime.
    runtime: tokio::runtime::Handle,
    /// Bumped whenever a session starts or finishes, so a pending idle
    /// unload only fires if nothing happened since it was scheduled.
    idle_generation: AtomicU64,
    /// Test-only hook: when greater than zero, `run_session` sleeps this
    /// many milliseconds just before the history insert, so a test can
    /// deterministically land a `cancel()` inside the non-abortable tail
    /// instead of racing real I/O timing.
    #[cfg(test)]
    pub(crate) tail_delay_ms: AtomicU64,
}

/// Everything a client supplies when it opens the engine.
pub struct EngineOptions {
    pub secrets: Arc<dyn SecretStore>,
    pub factories: Vec<Arc<dyn ProviderFactory>>,
    pub first_run: FirstRunDefaults,
}

impl Engine {
    pub async fn open(dirs: DataDir, secrets: Arc<dyn SecretStore>) -> Result<Engine, EngineError> {
        Self::open_with(
            dirs,
            EngineOptions {
                secrets,
                factories: Vec::new(),
                first_run: FirstRunDefaults::default(),
            },
        )
        .await
    }

    pub async fn open_with(dirs: DataDir, opts: EngineOptions) -> Result<Engine, EngineError> {
        let runtime = tokio::runtime::Handle::current();
        dirs.ensure()?;
        let config = ConfigStore::open_with_defaults(&dirs, &opts.first_run)?;
        let path = dirs.history_path();
        let history = Arc::new(
            tokio::task::spawn_blocking(move || HistoryStore::open(&path))
                .await
                .expect("join")?,
        );

        let general = config.current().general.clone();
        let recordings = dirs.recordings_dir();
        let h = history.clone();
        // Best effort: a prune failure (or panic) must not prevent opening
        // the engine, so its result is discarded rather than propagated.
        let _ = tokio::task::spawn_blocking(move || {
            h.prune(
                general.history_retention_days,
                general.recording_retention_days,
                &recordings,
            )
        })
        .await;

        let client = reqwest::Client::builder().build().expect("reqwest client");
        let registry =
            ProviderRegistry::with_factories(client.clone(), opts.secrets.clone(), opts.factories);
        Ok(Engine {
            inner: Arc::new(Inner {
                dirs,
                config,
                history,
                registry,
                client,
                secrets: opts.secrets,
                sessions: Mutex::new(HashMap::new()),
                runtime,
                idle_generation: AtomicU64::new(0),
                #[cfg(test)]
                tail_delay_ms: AtomicU64::new(0),
            }),
        })
    }

    /// Loads the default profile's ASR provider in the background so the
    /// first dictation does not pay the model load. Failures are ignored:
    /// the session reports them when it needs the provider.
    pub fn warm_up_default_asr(&self) {
        let inner = self.inner.clone();
        self.inner.runtime.spawn(async move {
            let config = inner.config.current();
            let Some(profile) = config.profiles.iter().find(|p| p.is_default) else {
                return;
            };
            let Some(cfg) = config.provider(&profile.asr_provider) else {
                return;
            };
            if let Ok(p) = inner.registry.asr(cfg, &config.general) {
                let _ = p.load().await;
            }
        });
    }

    pub fn config(&self) -> &ConfigStore {
        &self.inner.config
    }

    pub fn history(&self) -> Arc<HistoryStore> {
        self.inner.history.clone()
    }

    pub fn dirs(&self) -> &DataDir {
        &self.inner.dirs
    }

    pub async fn probe_provider(&self, cfg: &ProviderConfig) -> ProbeResult {
        match cfg.kind {
            ProviderKind::Http => probe_http(&self.inner.client, cfg, &self.inner.secrets).await,
            kind => match self.inner.registry.factory_for(kind) {
                Some(f) => f.probe(cfg).await,
                None => ProbeResult {
                    ok: false,
                    failure: Some(ProbeFailure::Other {
                        message: "provider kind not available in this build".into(),
                    }),
                },
            },
        }
    }

    /// May be called from a thread that is not running on this engine's
    /// tokio runtime (the task is spawned via the captured `Handle`).
    pub fn start_session(&self, req: SessionRequest) -> Session {
        // Bumped here rather than inside the spawned task, so a session
        // cancelled before its task first runs still cancels a pending
        // unload from an earlier session.
        self.inner.idle_generation.fetch_add(1, Ordering::SeqCst);
        let config = self.inner.config.current();
        let profile =
            Arc::new(resolve(&config.profiles, &req.app, req.profile_override.as_ref()).clone());
        let id = SessionId(uuid::Uuid::new_v4().to_string());
        let (events_tx, events_rx) = mpsc::channel(EVENT_BUFFER);
        let (finish_tx, finish_rx) = oneshot::channel();
        let audio = Arc::new(Mutex::new(Vec::<f32>::new()));
        let flags = Arc::new(SessionFlags::default());

        let _ = events_tx.try_send(Event::SessionStarted {
            session_id: id.clone(),
            profile_id: profile.id.clone(),
            protocol_version: PROTOCOL_VERSION,
        });

        let task = self.inner.runtime.spawn(run_session(
            self.inner.clone(),
            id.clone(),
            profile.clone(),
            req,
            audio.clone(),
            finish_rx,
            events_tx.clone(),
            flags.clone(),
        ));

        Session {
            id,
            profile_id: profile.id.clone(),
            audio,
            finish_tx: Mutex::new(Some(finish_tx)),
            events_tx,
            events_rx: Mutex::new(Some(events_rx)),
            task: Mutex::new(Some(task)),
            flags,
            on_cancel_done: {
                let inner = self.inner.clone();
                Arc::new(move || inner.schedule_idle_unload())
            },
        }
    }

    /// Records what the user actually inserted for a finished session.
    pub async fn record_final(
        &self,
        session_id: &SessionId,
        text: &str,
    ) -> Result<(), EngineError> {
        let history_id = self
            .inner
            .sessions
            .lock()
            .unwrap()
            .get(session_id)
            .copied()
            .ok_or_else(|| EngineError::UnknownSession(session_id.clone()))?;
        let history = self.inner.history.clone();
        let text = text.to_string();
        tokio::task::spawn_blocking(move || history.set_final(history_id, &text))
            .await
            .expect("join")?;
        Ok(())
    }
}

impl Inner {
    /// Arms a timer that unloads every cached LLM provider once the engine
    /// has been idle for `llm_idle_unload_secs`. Zero disables it.
    fn schedule_idle_unload(self: &Arc<Self>) {
        let secs = self.config.current().general.llm_idle_unload_secs;
        if secs == 0 {
            return;
        }
        let generation = self.idle_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let inner = self.clone();
        self.runtime.spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
            if inner.idle_generation.load(Ordering::SeqCst) == generation {
                inner.unload_idle_llms().await;
            }
        });
    }

    pub(crate) async fn unload_idle_llms(&self) {
        for p in self.registry.llm_providers() {
            p.unload().await;
        }
    }
}

/// Sends `Done` at most once per session: whichever of `run_session` or
/// `Session::cancel` gets here first wins, and the other is a no-op. This is
/// what keeps a cancel racing the pipeline's tail from producing a second
/// `Done` alongside the one `cancel` already sent.
async fn send_done_once(
    tx: &mpsc::Sender<Event>,
    done_sent: &AtomicBool,
    session_id: &SessionId,
    history_id: Option<HistoryId>,
) {
    if !done_sent.swap(true, Ordering::SeqCst) {
        let _ = tx
            .send(Event::Done {
                session_id: session_id.clone(),
                history_id,
            })
            .await;
    }
}

// Folding done_sent/cancelled/tail_started into `SessionFlags` (one
// parameter instead of three) brings this down from 9 to 8 arguments, still
// one over clippy's default threshold of 7; every remaining parameter is a
// distinct, necessary piece of state, so the lint stays silenced rather than
// inventing a further grouping not called for elsewhere.
#[allow(clippy::too_many_arguments)]
async fn run_session(
    inner: Arc<Inner>,
    session_id: SessionId,
    profile: Arc<crate::config::Profile>,
    req: SessionRequest,
    audio: Arc<Mutex<Vec<f32>>>,
    finish_rx: oneshot::Receiver<()>,
    tx: mpsc::Sender<Event>,
    flags: Arc<SessionFlags>,
) {
    if finish_rx.await.is_err() {
        send_done_once(&tx, &flags.done_sent, &session_id, None).await;
        inner.schedule_idle_unload();
        return;
    }

    let samples = std::mem::take(&mut *audio.lock().unwrap());
    let clip = AudioClip {
        samples,
        sample_rate: SAMPLE_RATE,
    };
    if clip.duration_secs() < MIN_UTTERANCE_SECS {
        let _ = tx
            .send(Event::Error {
                step: None,
                code: ErrorCode::NoAudio,
                message: format!("utterance shorter than {MIN_UTTERANCE_SECS} s"),
                recoverable: false,
                hint: None,
            })
            .await;
        send_done_once(&tx, &flags.done_sent, &session_id, None).await;
        inner.schedule_idle_unload();
        return;
    }

    let config = inner.config.current();
    let base_prompt = inner
        .config
        .base_prompt()
        .unwrap_or_else(|_| crate::config::base_prompt::DEFAULT_BASE_PROMPT.to_string());
    let ctx = PipelineContext {
        profile: profile.clone(),
        context: req.context.clone(),
        dictionary: Arc::new(config.dictionary.clone()),
        base_prompt,
        general: config.general.clone(),
    };

    let pipeline = match Pipeline::build(&config, &profile, &inner.registry) {
        Ok(p) => p,
        Err(err) => {
            let _ = tx
                .send(Event::Error {
                    step: None,
                    code: err.code,
                    message: err.message,
                    recoverable: false,
                    hint: err.hint,
                })
                .await;
            send_done_once(&tx, &flags.done_sent, &session_id, None).await;
            inner.schedule_idle_unload();
            return;
        }
    };

    let clip = Arc::new(clip);
    let outcome = match pipeline.run(clip.clone(), &ctx, &tx).await {
        Ok(o) => o,
        Err(_) => {
            send_done_once(&tx, &flags.done_sent, &session_id, None).await;
            inner.schedule_idle_unload();
            return;
        }
    };

    // From here on the task performs non-abortable spawn_blocking work (a
    // WAV write, a history insert), so tell `cancel` not to abort us
    // anymore: past this point, the `cancelled` checks below are what undo
    // a cancel, not `Session::cancel`'s `task.abort()`.
    flags.tail_started.store(true, Ordering::SeqCst);
    if flags.cancelled.load(Ordering::SeqCst) {
        return;
    }

    let audio_path = if config.general.keep_recordings {
        let path = inner
            .dirs
            .recordings_dir()
            .join(format!("{session_id}.wav"));
        let bytes = encode_wav_pcm16(&clip);
        let p = path.clone();
        match tokio::task::spawn_blocking(move || std::fs::write(&p, bytes)).await {
            Ok(Ok(())) => Some(path),
            _ => None,
        }
    } else {
        None
    };

    // Because `tail_started` is now set, `cancel` will not abort this task
    // (see `Session::cancel`), so this check is guaranteed to run rather
    // than racing an abort that could tear the task down first: if
    // cancelled, undo the WAV write above instead of leaving an orphan
    // recording for a cancelled session.
    if flags.cancelled.load(Ordering::SeqCst) {
        if let Some(p) = &audio_path {
            let _ = std::fs::remove_file(p);
        }
        return;
    }

    let new = NewUtterance {
        ts: crate::history::now_unix(),
        profile_id: profile.id.clone(),
        app_name: req.app.app_name.clone(),
        raw_text: outcome.raw_text,
        refined_text: outcome.final_text.clone(),
        final_text: outcome.final_text,
        degraded: outcome.degraded,
        audio_path: audio_path.clone(),
        asr_ms: outcome.asr_ms,
        llm_ms: outcome.llm_ms,
    };
    let history = inner.history.clone();
    #[cfg(test)]
    let tail_delay_ms = inner.tail_delay_ms.load(Ordering::SeqCst);
    let history_id = match tokio::task::spawn_blocking(move || {
        // Test-only hook: lets a test land a `cancel()` deterministically
        // while this closure is running, instead of racing real I/O timing.
        #[cfg(test)]
        if tail_delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(tail_delay_ms));
        }
        history.insert(new)
    })
    .await
    {
        Ok(Ok(id)) => Some(id),
        _ => None,
    };

    // The insert has already happened (or failed); it can only be taken
    // back with an explicit delete now, not prevented. This swap is the
    // single point where ownership of the session's one and only Done is
    // decided: whoever transitions `done_sent` from false to true (a
    // `false` result here) owns reporting the real outcome; if `cancel`
    // already claimed it (a `true` result, meaning it already sent
    // Done(None) to the client), undo what was just written instead of
    // reporting it as kept.
    if flags.done_sent.swap(true, Ordering::SeqCst) {
        if let Some(id) = history_id {
            let history = inner.history.clone();
            let _ = tokio::task::spawn_blocking(move || history.delete(id)).await;
        }
        if let Some(p) = &audio_path {
            let _ = std::fs::remove_file(p);
        }
        return;
    }

    if let Some(id) = history_id {
        inner
            .sessions
            .lock()
            .unwrap()
            .insert(session_id.clone(), id);
    }
    // done_sent was already claimed by the swap above, so send directly
    // rather than through send_done_once (which would just re-check it).
    let _ = tx
        .send(Event::Done {
            session_id: session_id.clone(),
            history_id,
        })
        .await;
    inner.schedule_idle_unload();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemorySecretStore;
    use crate::types::StepName;
    use serde_json::json;
    use tokio_stream::StreamExt;
    use url::Url;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Rig {
        _tmp: tempfile::TempDir,
        engine: Engine,
        asr: MockServer,
        llm: MockServer,
    }

    async fn rig(keep_recordings: bool) -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let asr = MockServer::start().await;
        let llm = MockServer::start().await;
        let engine = Engine::open(
            DataDir::custom(tmp.path()),
            Arc::new(MemorySecretStore::default()),
        )
        .await
        .unwrap();
        engine
            .config()
            .update(|c| {
                *c = crate::config::Config::http_test_config();
                c.providers[0].base_url = Some(Url::parse(&format!("{}/v1", asr.uri())).unwrap());
                c.providers[1].base_url = Some(Url::parse(&format!("{}/v1", llm.uri())).unwrap());
                c.general.keep_recordings = keep_recordings;
            })
            .unwrap();
        Rig {
            _tmp: tmp,
            engine,
            asr,
            llm,
        }
    }

    #[tokio::test]
    async fn probe_dispatches_to_factory_for_embedded() {
        let tmp = tempfile::tempdir().unwrap();
        let e = Engine::open_with(
            DataDir::custom(tmp.path()),
            EngineOptions {
                secrets: Arc::new(MemorySecretStore::default()),
                factories: vec![Arc::new(crate::providers::registry::tests::FakeFactory)],
                first_run: FirstRunDefaults::default(),
            },
        )
        .await
        .unwrap();
        let r = e
            .probe_provider(&crate::providers::registry::tests::embedded_cfg())
            .await;
        assert!(r.ok);
    }

    async fn idle_rig() -> (
        Arc<crate::steps::llm::fakes::FakeLlm>,
        tempfile::TempDir,
        Engine,
    ) {
        use crate::config::General;
        use crate::config::Runtime;
        use crate::providers::{AsrProvider, LlmProvider, ProviderError};
        use crate::steps::llm::fakes::FakeLlm;

        // One shared fake LLM so the test can count unloads.
        struct F(Arc<FakeLlm>);
        #[async_trait::async_trait]
        impl ProviderFactory for F {
            fn kind(&self) -> ProviderKind {
                ProviderKind::Embedded
            }
            fn asr(
                &self,
                _: &ProviderConfig,
                _: &General,
            ) -> Result<Arc<dyn AsrProvider>, ProviderError> {
                Ok(crate::steps::asr::fakes::FakeAsr::ok("原文"))
            }
            fn llm(
                &self,
                _: &ProviderConfig,
                _: &General,
            ) -> Result<Arc<dyn LlmProvider>, ProviderError> {
                Ok(self.0.clone())
            }
            async fn probe(&self, _: &ProviderConfig) -> ProbeResult {
                ProbeResult {
                    ok: true,
                    failure: None,
                }
            }
        }

        let fake = FakeLlm::ok("ok");
        let tmp = tempfile::tempdir().unwrap();
        let e = Engine::open_with(
            DataDir::custom(tmp.path()),
            EngineOptions {
                secrets: Arc::new(MemorySecretStore::default()),
                factories: vec![Arc::new(F(fake.clone()))],
                first_run: FirstRunDefaults::default(),
            },
        )
        .await
        .unwrap();
        e.config()
            .update(|c| {
                c.general.llm_idle_unload_secs = 2;
                for p in &mut c.providers {
                    p.kind = ProviderKind::Embedded;
                    p.base_url = None;
                    p.runtime = Some(Runtime::Llama);
                    p.model_id = Some("m".into());
                }
            })
            .unwrap();

        (fake, tmp, e)
    }

    async fn settle_and_expect_one_unload(fake: &crate::steps::llm::fakes::FakeLlm) {
        use std::time::Duration;
        // Let the timer be armed before the clock moves.
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        tokio::time::advance(Duration::from_secs(3)).await;
        // Let the timer task observe the advanced clock.
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert_eq!(fake.unloads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn llm_is_unloaded_after_idle_period() {
        let (fake, _tmp, e) = idle_rig().await;
        let s = e.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.finish();
        drain(&s).await;
        assert_eq!(fake.unloads.load(Ordering::SeqCst), 0);
        settle_and_expect_one_unload(&fake).await;
    }

    #[tokio::test(start_paused = true)]
    async fn llm_is_unloaded_after_a_cancelled_session() {
        let (fake, _tmp, e) = idle_rig().await;
        // A full session first, so the LLM provider is built and cached.
        let warm = e.start_session(SessionRequest::default());
        warm.push_audio(&one_second());
        warm.finish();
        drain(&warm).await;
        let s = e.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.cancel();
        drain(&s).await;
        assert_eq!(fake.unloads.load(Ordering::SeqCst), 0);
        settle_and_expect_one_unload(&fake).await;
    }

    #[tokio::test(start_paused = true)]
    async fn llm_is_unloaded_after_a_too_short_session() {
        let (fake, _tmp, e) = idle_rig().await;
        // A full session first, so the LLM provider is built and cached.
        let warm = e.start_session(SessionRequest::default());
        warm.push_audio(&one_second());
        warm.finish();
        drain(&warm).await;
        let s = e.start_session(SessionRequest::default());
        s.push_audio(&[0.0; 100]);
        s.finish();
        drain(&s).await;
        settle_and_expect_one_unload(&fake).await;
    }

    async fn mock_ok(rig: &Rig, asr_text: &str, llm_text: &str) {
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text": asr_text})))
            .mount(&rig.asr)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"choices":[{"message":{"content": llm_text}}]})),
            )
            .mount(&rig.llm)
            .await;
    }

    async fn drain(session: &Session) -> Vec<Event> {
        let mut stream = session.events();
        let mut out = Vec::new();
        while let Some(e) = stream.next().await {
            let is_done = matches!(e, Event::Done { .. });
            out.push(e);
            if is_done {
                break;
            }
        }
        out
    }

    fn one_second() -> Vec<f32> {
        vec![0.05; SAMPLE_RATE as usize]
    }

    #[tokio::test]
    async fn full_session_writes_history_and_emits_done() {
        let r = rig(false).await;
        mock_ok(&r, "软件 原文", "软件 整理後").await;
        let s = r.engine.start_session(SessionRequest {
            app: AppInfo {
                app_name: Some("TextEdit".into()),
                ..Default::default()
            },
            ..Default::default()
        });
        s.push_audio(&one_second());
        s.finish();
        let events = drain(&s).await;
        assert!(matches!(
            events.first(),
            Some(Event::SessionStarted {
                protocol_version: 1,
                ..
            })
        ));
        assert!(events.iter().any(
            |e| matches!(e, Event::Refined { text, degraded: false } if text == "軟體 整理後")
        ));
        let Some(Event::Done {
            history_id: Some(id),
            ..
        }) = events.last()
        else {
            panic!("{events:?}")
        };
        let row = r.engine.history().get(*id).unwrap().unwrap();
        assert_eq!(row.raw_text, "软件 原文");
        assert_eq!(row.final_text, "軟體 整理後");
        assert_eq!(row.app_name.as_deref(), Some("TextEdit"));
        assert!(row.audio_path.is_none());

        r.engine.record_final(s.id(), "使用者改過").await.unwrap();
        assert_eq!(
            r.engine.history().get(*id).unwrap().unwrap().final_text,
            "使用者改過"
        );
        assert!(r
            .engine
            .record_final(&SessionId("nope".into()), "x")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn finish_without_audio_emits_error_and_done() {
        let r = rig(false).await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text":"x"})))
            .expect(0)
            .mount(&r.asr)
            .await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&vec![0.0; 1_000]);
        s.finish();
        let events = drain(&s).await;
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Error {
                code: ErrorCode::NoAudio,
                recoverable: false,
                ..
            }
        )));
        assert!(matches!(
            events.last(),
            Some(Event::Done {
                history_id: None,
                ..
            })
        ));
        assert!(r.engine.history().list_recent(10).unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancel_while_recording_emits_done_only() {
        let r = rig(false).await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.cancel();
        let events = drain(&s).await;
        assert_eq!(events.len(), 2, "{events:?}");
        assert!(matches!(
            events[1],
            Event::Done {
                history_id: None,
                ..
            }
        ));
        assert!(r.engine.history().list_recent(10).unwrap().is_empty());
    }

    #[tokio::test]
    async fn asr_unreachable_ends_with_error_then_done() {
        let r = rig(false).await;
        r.engine
            .config()
            .update(|c| {
                c.providers[0].base_url = Some(Url::parse("http://127.0.0.1:9/v1").unwrap())
            })
            .unwrap();
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.finish();
        let events = drain(&s).await;
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Error {
                code: ErrorCode::ProviderUnreachable,
                recoverable: false,
                ..
            }
        )));
        assert!(matches!(
            events.last(),
            Some(Event::Done {
                history_id: None,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn keep_recordings_writes_wav_next_to_history() {
        let r = rig(true).await;
        mock_ok(&r, "原文", "整理後").await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.finish();
        let events = drain(&s).await;
        let Some(Event::Done {
            history_id: Some(id),
            ..
        }) = events.last()
        else {
            panic!("{events:?}")
        };
        let row = r.engine.history().get(*id).unwrap().unwrap();
        let path = row.audio_path.expect("recording path");
        assert!(path.starts_with(r.engine.dirs().recordings_dir()));
        assert!(path.exists());
        assert_eq!(&std::fs::read(&path).unwrap()[0..4], b"RIFF");
    }

    #[tokio::test]
    async fn profile_override_is_reported_in_session_started() {
        let r = rig(false).await;
        r.engine
            .config()
            .update(|c| {
                let mut p = c.profiles[0].clone();
                p.id = ProfileId("slack".into());
                p.is_default = false;
                c.profiles.push(p);
            })
            .unwrap();
        let s = r.engine.start_session(SessionRequest {
            profile_override: Some(ProfileId("slack".into())),
            ..Default::default()
        });
        assert_eq!(s.profile_id().0, "slack");
        s.cancel();
    }

    #[tokio::test]
    async fn cancel_while_pipeline_runs_emits_one_done_and_records_nothing() {
        let r = rig(true).await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text": "原文"})))
            .mount(&r.asr)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"choices":[{"message":{"content": "整理後"}}]}))
                    .set_delay(std::time::Duration::from_millis(500)),
            )
            .mount(&r.llm)
            .await;

        let s = r.engine.start_session(SessionRequest::default());
        // Take the stream now: the channel only closes once every Sender is
        // dropped, which includes the one `Session` itself holds, so this
        // stream is dropped along with `s` below to let it end.
        let mut stream = s.events();
        s.push_audio(&one_second());
        s.finish();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        s.cancel();
        drop(s);

        let mut events = Vec::new();
        while let Some(e) = stream.next().await {
            events.push(e);
        }

        let dones: Vec<&Event> = events
            .iter()
            .filter(|e| matches!(e, Event::Done { .. }))
            .collect();
        assert_eq!(dones.len(), 1, "{events:?}");
        assert!(matches!(
            dones[0],
            Event::Done {
                history_id: None,
                ..
            }
        ));
        assert!(r.engine.history().list_recent(10).unwrap().is_empty());
        let recordings = r.engine.dirs().recordings_dir();
        let left_a_file = std::fs::read_dir(&recordings)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false);
        assert!(!left_a_file, "cancel must not leave a recording behind");
    }

    #[tokio::test]
    async fn cancel_during_tail_records_nothing_and_emits_one_done() {
        let r = rig(true).await;
        mock_ok(&r, "原文", "整理後").await;
        // Slows down the history insert so a cancel can be landed
        // deterministically while it is running, rather than racing real
        // I/O timing the way `cancel_while_pipeline_runs_...` does for the
        // (already abortable) network-await window.
        r.engine.inner.tail_delay_ms.store(300, Ordering::SeqCst);

        let s = r.engine.start_session(SessionRequest::default());
        let mut stream = s.events();
        s.push_audio(&one_second());
        s.finish();

        // Wait for Refined: the pipeline has finished and the task is now
        // in the (non-abortable) tail, about to write the WAV and insert
        // the history row.
        while let Some(e) = stream.next().await {
            if matches!(e, Event::Refined { .. }) {
                break;
            }
        }
        // Land the cancel inside the 300 ms insert delay above, rather than
        // racing it, so this deterministically exercises the claim-loser
        // delete path in `run_session` (the `done_sent.swap` branch that
        // deletes an already-inserted row).
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        s.cancel();
        drop(s);

        let mut events = Vec::new();
        while let Some(e) = stream.next().await {
            events.push(e);
        }

        let dones: Vec<&Event> = events
            .iter()
            .filter(|e| matches!(e, Event::Done { .. }))
            .collect();
        assert_eq!(dones.len(), 1, "{events:?}");
        assert!(matches!(
            dones[0],
            Event::Done {
                history_id: None,
                ..
            }
        ));
        assert!(r.engine.history().list_recent(10).unwrap().is_empty());
        let recordings = r.engine.dirs().recordings_dir();
        let left_a_file = std::fs::read_dir(&recordings)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false);
        assert!(!left_a_file, "cancel must not leave a recording behind");
    }

    #[tokio::test]
    async fn cancel_after_completion_is_a_noop() {
        let r = rig(false).await;
        mock_ok(&r, "原文", "整理後").await;
        let s = r.engine.start_session(SessionRequest::default());
        let mut stream = s.events();
        s.push_audio(&one_second());
        s.finish();

        let mut id = None;
        while let Some(e) = stream.next().await {
            if let Event::Done { history_id, .. } = e {
                id = history_id;
                break;
            }
        }
        let id = id.expect("session should complete with a history id");

        s.cancel();
        s.cancel();

        let result =
            tokio::time::timeout(std::time::Duration::from_millis(100), stream.next()).await;
        assert!(
            matches!(result, Err(_) | Ok(None)),
            "cancel after completion must not emit another event: {result:?}"
        );
        assert!(r.engine.history().get(id).unwrap().is_some());
    }

    #[tokio::test]
    async fn empty_transcript_ends_with_error_and_no_history() {
        let r = rig(false).await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text": ""})))
            .mount(&r.asr)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"choices":[{"message":{"content": "x"}}]})),
            )
            .expect(0)
            .mount(&r.llm)
            .await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.finish();
        let events = drain(&s).await;
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Error {
                step: Some(StepName::Asr),
                code: ErrorCode::NoSpeech,
                ..
            }
        )));
        assert!(matches!(
            events.last(),
            Some(Event::Done {
                history_id: None,
                ..
            })
        ));
        assert!(r.engine.history().list_recent(10).unwrap().is_empty());
    }

    #[tokio::test]
    async fn missing_llm_key_degrades_to_asr_text() {
        let r = rig(false).await;
        r.engine
            .config()
            .update(|c| c.providers[1].api_key_ref = Some("absent".into()))
            .unwrap();
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text": "原文"})))
            .mount(&r.asr)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"choices":[{"message":{"content": "x"}}]})),
            )
            .expect(0)
            .mount(&r.llm)
            .await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.finish();
        let events = drain(&s).await;
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::Refined { text, degraded: true } if text == "原文")));
        assert!(matches!(
            events.last(),
            Some(Event::Done {
                history_id: Some(_),
                ..
            })
        ));
    }

    #[tokio::test]
    async fn llm_5xx_degrades_with_history_row() {
        let r = rig(false).await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text": "原文"})))
            .mount(&r.asr)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&r.llm)
            .await;
        let s = r.engine.start_session(SessionRequest::default());
        s.push_audio(&one_second());
        s.finish();
        let events = drain(&s).await;
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::Refined { text, degraded: true } if text == "原文")));
        let Some(Event::Done {
            history_id: Some(id),
            ..
        }) = events.last()
        else {
            panic!("{events:?}")
        };
        let row = r.engine.history().get(*id).unwrap().unwrap();
        assert!(row.degraded);
    }
}
