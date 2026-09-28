//! The crate's entry point. Owns config, history, and providers; opens sessions.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, oneshot};

use crate::audio::encode_wav_pcm16;
use crate::config::{ConfigStore, ProviderConfig, ProviderKind, StoreError};
use crate::history::{HistoryError, HistoryStore, NewUtterance};
use crate::paths::DataDir;
use crate::pipeline::Pipeline;
use crate::profile::resolve;
use crate::protocol::{Event, PROTOCOL_VERSION};
use crate::providers::probe::{probe_http, ProbeFailure, ProbeResult};
use crate::providers::registry::ProviderRegistry;
use crate::secrets::SecretStore;
use crate::session::Session;
use crate::steps::{PipelineContext, StepError};
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
    #[error(transparent)]
    Pipeline(#[from] StepError),
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
}

impl Engine {
    pub async fn open(dirs: DataDir, secrets: Arc<dyn SecretStore>) -> Result<Engine, EngineError> {
        dirs.ensure()?;
        let config = ConfigStore::open(&dirs)?;
        let path = dirs.history_path();
        let history = Arc::new(
            tokio::task::spawn_blocking(move || HistoryStore::open(&path))
                .await
                .expect("join")?,
        );

        let general = config.current().general.clone();
        let recordings = dirs.recordings_dir();
        let h = history.clone();
        tokio::task::spawn_blocking(move || {
            h.prune(
                general.history_retention_days,
                general.recording_retention_days,
                &recordings,
            )
        })
        .await
        .expect("join")?;

        let client = reqwest::Client::builder().build().expect("reqwest client");
        let registry = ProviderRegistry::new(client.clone(), secrets.clone());
        Ok(Engine {
            inner: Arc::new(Inner {
                dirs,
                config,
                history,
                registry,
                client,
                secrets,
                sessions: Mutex::new(HashMap::new()),
            }),
        })
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
            _ => ProbeResult {
                ok: false,
                failure: Some(ProbeFailure::Other {
                    message: "provider kind not available in this build".into(),
                }),
            },
        }
    }

    pub fn start_session(&self, req: SessionRequest) -> Session {
        let config = self.inner.config.current();
        let profile =
            Arc::new(resolve(&config.profiles, &req.app, req.profile_override.as_ref()).clone());
        let id = SessionId(uuid::Uuid::new_v4().to_string());
        let (events_tx, events_rx) = mpsc::channel(EVENT_BUFFER);
        let (finish_tx, finish_rx) = oneshot::channel();
        let audio = Arc::new(Mutex::new(Vec::<f32>::new()));

        let _ = events_tx.try_send(Event::SessionStarted {
            session_id: id.clone(),
            profile_id: profile.id.clone(),
            protocol_version: PROTOCOL_VERSION,
        });

        let task = tokio::spawn(run_session(
            self.inner.clone(),
            id.clone(),
            profile.clone(),
            req,
            audio.clone(),
            finish_rx,
            events_tx.clone(),
        ));

        Session {
            id,
            profile_id: profile.id.clone(),
            audio,
            finish_tx: Mutex::new(Some(finish_tx)),
            events_tx,
            events_rx: Mutex::new(Some(events_rx)),
            task: Mutex::new(Some(task)),
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

async fn run_session(
    inner: Arc<Inner>,
    session_id: SessionId,
    profile: Arc<crate::config::Profile>,
    req: SessionRequest,
    audio: Arc<Mutex<Vec<f32>>>,
    finish_rx: oneshot::Receiver<()>,
    tx: mpsc::Sender<Event>,
) {
    let done = |history_id: Option<HistoryId>| Event::Done {
        session_id: session_id.clone(),
        history_id,
    };

    if finish_rx.await.is_err() {
        let _ = tx.send(done(None)).await;
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
        let _ = tx.send(done(None)).await;
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
            let _ = tx.send(done(None)).await;
            return;
        }
    };

    let clip = Arc::new(clip);
    let outcome = match pipeline.run(clip.clone(), &ctx, &tx).await {
        Ok(o) => o,
        Err(_) => {
            let _ = tx.send(done(None)).await;
            return;
        }
    };

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

    let new = NewUtterance {
        ts: crate::history::now_unix(),
        profile_id: profile.id.clone(),
        app_name: req.app.app_name.clone(),
        raw_text: outcome.raw_text,
        refined_text: outcome.final_text.clone(),
        final_text: outcome.final_text,
        degraded: outcome.degraded,
        audio_path,
        asr_ms: outcome.asr_ms,
        llm_ms: outcome.llm_ms,
    };
    let history = inner.history.clone();
    let history_id = match tokio::task::spawn_blocking(move || history.insert(new)).await {
        Ok(Ok(id)) => Some(id),
        _ => None,
    };
    if let Some(id) = history_id {
        inner
            .sessions
            .lock()
            .unwrap()
            .insert(session_id.clone(), id);
    }
    let _ = tx.send(done(history_id)).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemorySecretStore;
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
}
