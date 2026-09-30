//! Speed test: records or picks a clip, then times each chosen provider
//! cold and warm against it.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, UNIX_EPOCH};

use kongwe_core::audio::{decode_wav_pcm16, encode_wav_pcm16};
use kongwe_core::bench::BenchRun;
use kongwe_core::history::HistoryStore;
use kongwe_core::types::{AudioClip, HistoryId, ProviderId};
use kongwe_core::{Engine, SAMPLE_RATE};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use super::{engine, CmdError, CmdResult};
use crate::app::AppState;
use crate::audio::recorder::Recorder;
use crate::audio::{AudioError, AudioHandle, AudioMsg, AudioSource};
use crate::settings::{ClientSettings, MicrophoneChoice};
use crate::vad::{self, EarshotVad, Vad};

/// Longest clip kept; later audio is dropped.
const MAX_CLIP_SECS: usize = 30;
const SETTINGS_WINDOW: &str = "settings";

#[derive(Debug, thiserror::Error)]
pub enum BenchError {
    #[error("no speech in the recording")]
    NoSpeech,
    #[error("{0}")]
    Audio(String),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClipStatus {
    pub present: bool,
    pub secs: f32,
    /// File modification time, unix seconds.
    pub recorded_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClipCandidate {
    pub id: i64,
    pub ts: i64,
    pub raw_text: String,
}

#[derive(Debug, Clone, Serialize)]
struct BenchLevel {
    rms: f32,
}

#[derive(Debug, Clone, Serialize)]
struct BenchProgress {
    run: BenchRun,
    remaining: u32,
}

#[derive(Debug, Clone, Serialize)]
struct BenchDone {}

#[derive(Default)]
pub struct BenchState {
    recording: Mutex<Option<Recording>>,
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

/// A microphone capture feeding a worker thread that resamples to 16 kHz
/// and reports each block's level.
pub struct Recording {
    handle: Box<dyn AudioHandle>,
    stopping: Arc<AtomicBool>,
    worker: JoinHandle<Result<Vec<f32>, String>>,
}

impl Recording {
    pub fn start(
        audio: &dyn AudioSource,
        choice: &MicrophoneChoice,
        on_level: impl Fn(f32) + Send + 'static,
    ) -> Result<Recording, AudioError> {
        let (tx, rx) = mpsc::channel();
        let handle = audio.start(choice, tx)?;
        let stopping = Arc::new(AtomicBool::new(false));
        let flag = stopping.clone();
        let worker = std::thread::spawn(move || capture(rx, &flag, on_level));
        Ok(Recording {
            handle,
            stopping,
            worker,
        })
    }

    /// Stops the capture and returns the clip trimmed to its speech.
    pub fn stop(self, vad: &mut dyn Vad) -> Result<AudioClip, BenchError> {
        let samples = self.finish()?;
        let range = vad::trim(&samples, vad).ok_or(BenchError::NoSpeech)?;
        Ok(AudioClip {
            samples: samples[range].to_vec(),
            sample_rate: SAMPLE_RATE,
        })
    }

    fn finish(self) -> Result<Vec<f32>, BenchError> {
        self.handle.stop();
        self.stopping.store(true, Ordering::SeqCst);
        self.worker
            .join()
            .map_err(|_| BenchError::Audio("recording thread failed".into()))?
            .map_err(BenchError::Audio)
    }
}

/// Runs until the stream ends or `stopping` is set, then drains the frames
/// already queued.
fn capture(
    rx: Receiver<AudioMsg>,
    stopping: &AtomicBool,
    on_level: impl Fn(f32),
) -> Result<Vec<f32>, String> {
    let max = MAX_CLIP_SECS * SAMPLE_RATE as usize;
    let mut recorder: Option<Recorder> = None;
    let mut handle_msg = |msg: AudioMsg| -> Result<(), String> {
        match msg {
            AudioMsg::Frames { rate, mono } => {
                let r = recorder.get_or_insert_with(|| Recorder::new(rate));
                if r.len_secs() < MAX_CLIP_SECS as f32 {
                    on_level(r.push(&mono));
                }
                Ok(())
            }
            AudioMsg::Error(e) => Err(e),
        }
    };
    loop {
        if stopping.load(Ordering::SeqCst) {
            while let Ok(msg) = rx.try_recv() {
                handle_msg(msg)?;
            }
            break;
        }
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(msg) => handle_msg(msg)?,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    let mut samples = recorder.map(Recorder::finish).unwrap_or_default();
    samples.truncate(max);
    Ok(samples)
}

/// Writes `clip` as 16-bit WAV through a temporary file, so a failed write
/// leaves the previous clip in place.
pub fn write_clip(path: &Path, clip: &AudioClip) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("wav.tmp");
    std::fs::write(&tmp, encode_wav_pcm16(clip))?;
    std::fs::rename(&tmp, path)
}

pub fn read_clip(path: &Path) -> Option<AudioClip> {
    let bytes = std::fs::read(path).ok()?;
    decode_wav_pcm16(&bytes).ok()
}

pub fn clip_status(path: &Path) -> ClipStatus {
    let Some(clip) = read_clip(path) else {
        return ClipStatus {
            present: false,
            secs: 0.0,
            recorded_at: None,
        };
    };
    let recorded_at = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    ClipStatus {
        present: true,
        secs: clip.duration_secs(),
        recorded_at,
    }
}

/// The most recent 20 history entries that still have their recording.
pub fn history_candidates(history: &HistoryStore) -> CmdResult<Vec<ClipCandidate>> {
    Ok(history
        .list_recent(200)?
        .into_iter()
        .filter(|u| u.audio_path.is_some())
        .take(20)
        .map(|u| ClipCandidate {
            id: u.id.0,
            ts: u.ts,
            raw_text: u.raw_text,
        })
        .collect())
}

/// Copies the recording of history entry `id` to `dest`.
pub fn clip_from_history(history: &HistoryStore, id: i64, dest: &Path) -> CmdResult<ClipStatus> {
    let entry = history.get(HistoryId(id))?.ok_or_else(|| CmdError {
        message: "history entry not found".into(),
    })?;
    let source = entry.audio_path.ok_or_else(|| CmdError {
        message: "recording not kept".into(),
    })?;
    let clip = read_clip(&source).ok_or_else(|| CmdError {
        message: "recording not readable".into(),
    })?;
    write_clip(dest, &clip)?;
    Ok(clip_status(dest))
}

/// Times every ASR id, then every LLM id, each cold then warm. The LLMs
/// refine the first non-empty ASR output, or `fallback` when there is none.
/// `emit` gets each run and how many runs are still to come.
pub async fn run_all(
    engine: &Engine,
    clip: Arc<AudioClip>,
    asr: Vec<ProviderId>,
    llm: Vec<ProviderId>,
    fallback: String,
    emit: impl Fn(BenchRun, u32),
) {
    let mut remaining = 2 * (asr.len() + llm.len()) as u32;
    let mut emit = |run: BenchRun| {
        remaining -= 1;
        emit(run, remaining);
    };
    let mut transcript: Option<String> = None;
    for id in &asr {
        for cold in [true, false] {
            let run = engine.bench_asr(id, clip.clone(), cold).await;
            if transcript.is_none() && run.error.is_none() && !run.output.trim().is_empty() {
                transcript = Some(run.output.clone());
            }
            emit(run);
        }
    }
    let transcript = transcript.unwrap_or(fallback);
    for id in &llm {
        for cold in [true, false] {
            emit(engine.bench_llm(id, &transcript, cold).await);
        }
    }
}

fn clip_path(state: &State<'_, AppState>) -> std::path::PathBuf {
    state.dirs.bench_clip_path()
}

#[tauri::command]
pub fn bench_clip_status(state: State<'_, AppState>) -> CmdResult<ClipStatus> {
    Ok(clip_status(&clip_path(&state)))
}

/// Starts recording a new clip, replacing any recording in progress.
#[tauri::command]
pub async fn bench_record_start(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    if let Some(old) = state.bench.recording.lock().unwrap().take() {
        let _ = old.finish();
    }
    let choice = ClientSettings::load(&state.dirs.config_dir).microphone_choice();
    let rec = Recording::start(state.audio.as_ref(), &choice, move |rms| {
        let _ = app.emit_to(SETTINGS_WINDOW, "bench-level", BenchLevel { rms });
    })?;
    *state.bench.recording.lock().unwrap() = Some(rec);
    Ok(())
}

/// Stops recording and saves the clip. Without speech the previous clip is
/// kept and an error is returned.
#[tauri::command]
pub async fn bench_record_stop(state: State<'_, AppState>) -> CmdResult<ClipStatus> {
    let rec = state
        .bench
        .recording
        .lock()
        .unwrap()
        .take()
        .ok_or_else(|| CmdError {
            message: "not recording".into(),
        })?;
    let clip = rec.stop(&mut EarshotVad::new())?;
    let path = clip_path(&state);
    write_clip(&path, &clip)?;
    Ok(clip_status(&path))
}

#[tauri::command]
pub fn bench_history_candidates(state: State<'_, AppState>) -> CmdResult<Vec<ClipCandidate>> {
    history_candidates(&engine(&state)?.history())
}

#[tauri::command]
pub fn bench_clip_from_history(id: i64, state: State<'_, AppState>) -> CmdResult<ClipStatus> {
    clip_from_history(&engine(&state)?.history(), id, &clip_path(&state))
}

/// Runs the speed test in the background, replacing a run in progress.
/// Progress arrives as `bench-progress` events and the end as `bench-done`.
#[tauri::command]
pub fn bench_start(
    asr: Vec<String>,
    llm: Vec<String>,
    fallback_text: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<()> {
    let engine = engine(&state)?;
    let clip = match read_clip(&clip_path(&state)) {
        Some(c) => c,
        None if asr.is_empty() => AudioClip {
            samples: Vec::new(),
            sample_rate: SAMPLE_RATE,
        },
        None => {
            return Err(CmdError {
                message: "no test clip".into(),
            })
        }
    };
    let asr = asr.into_iter().map(ProviderId).collect();
    let llm = llm.into_iter().map(ProviderId).collect();
    let mut task = state.bench.task.lock().unwrap();
    if let Some(old) = task.take() {
        old.abort();
    }
    *task = Some(tauri::async_runtime::spawn(async move {
        let progress = app.clone();
        run_all(
            &engine,
            Arc::new(clip),
            asr,
            llm,
            fallback_text,
            move |run, remaining| {
                let _ = progress.emit_to(
                    SETTINGS_WINDOW,
                    "bench-progress",
                    BenchProgress { run, remaining },
                );
            },
        )
        .await;
        let _ = app.emit_to(SETTINGS_WINDOW, "bench-done", BenchDone {});
    }));
    Ok(())
}

/// Stops a run in progress; the provider call in flight is dropped.
#[tauri::command]
pub fn bench_cancel(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    if let Some(task) = state.bench.task.lock().unwrap().take() {
        task.abort();
    }
    let _ = app.emit_to(SETTINGS_WINDOW, "bench-done", BenchDone {});
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{AudioError, AudioHandle, AudioMsg, AudioSource};
    use crate::settings::MicrophoneChoice;
    use crate::vad::Vad;
    use async_trait::async_trait;
    use kongwe_core::audio::encode_wav_pcm16;
    use kongwe_core::bench::BenchRole;
    use kongwe_core::config::{General, ProviderConfig, ProviderKind};
    use kongwe_core::providers::probe::ProbeResult;
    use kongwe_core::providers::{AsrHint, AsrProvider, LlmOptions, LlmProvider, ProviderError};
    use kongwe_core::types::{AudioClip, ChatMessage, ProviderId, Transcript};
    use kongwe_core::ProviderFactory;
    use kongwe_core::{DataDir, Engine, EngineOptions, FirstRunDefaults, MemorySecretStore};
    use std::sync::mpsc::Sender;
    use std::sync::{Arc, Mutex};

    struct PeakVad;
    impl Vad for PeakVad {
        fn is_speech(&mut self, frame: &[f32]) -> bool {
            frame.iter().any(|s| s.abs() > 0.1)
        }
    }

    #[derive(Default)]
    struct FakeAudio {
        tx: Mutex<Option<Sender<AudioMsg>>>,
    }
    struct FakeHandle;
    impl AudioHandle for FakeHandle {
        fn stop(self: Box<Self>) {}
    }
    impl AudioSource for FakeAudio {
        fn start(
            &self,
            _c: &MicrophoneChoice,
            tx: Sender<AudioMsg>,
        ) -> Result<Box<dyn AudioHandle>, AudioError> {
            *self.tx.lock().unwrap() = Some(tx);
            Ok(Box::new(FakeHandle))
        }
        fn list_devices(&self) -> Vec<String> {
            vec!["Fake".into()]
        }
    }
    impl FakeAudio {
        fn send(&self, mono: Vec<f32>) {
            self.tx
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .send(AudioMsg::Frames { rate: 16_000, mono })
                .unwrap();
        }
        fn speech(&self, secs: f32) {
            let n = (16_000.0 * secs) as usize;
            self.send(
                (0..n)
                    .map(|i| if i % 2 == 0 { 0.5 } else { -0.5 })
                    .collect(),
            );
        }
        fn silence(&self, secs: f32) {
            self.send(vec![0.0; (16_000.0 * secs) as usize]);
        }
    }

    struct FakeAsr;
    #[async_trait]
    impl AsrProvider for FakeAsr {
        async fn transcribe(
            &self,
            _audio: &AudioClip,
            _hint: &AsrHint,
        ) -> Result<Transcript, ProviderError> {
            Ok(Transcript {
                text: "原文".into(),
                language: None,
            })
        }
    }

    #[derive(Default)]
    struct FakeLlm {
        calls: Mutex<Vec<Vec<ChatMessage>>>,
    }
    #[async_trait]
    impl LlmProvider for FakeLlm {
        async fn complete(
            &self,
            messages: Vec<ChatMessage>,
            _opts: &LlmOptions,
        ) -> Result<String, ProviderError> {
            self.calls.lock().unwrap().push(messages);
            Ok("ok".into())
        }
    }

    struct Factory(Arc<FakeLlm>);
    #[async_trait]
    impl ProviderFactory for Factory {
        fn kind(&self) -> ProviderKind {
            ProviderKind::Embedded
        }
        fn asr(
            &self,
            _: &ProviderConfig,
            _: &General,
        ) -> Result<Arc<dyn AsrProvider>, ProviderError> {
            Ok(Arc::new(FakeAsr))
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

    async fn rig() -> (
        tempfile::TempDir,
        Engine,
        Arc<FakeLlm>,
        ProviderId,
        ProviderId,
    ) {
        let tmp = tempfile::tempdir().unwrap();
        let llm = Arc::new(FakeLlm::default());
        let engine = Engine::open_with(
            DataDir::custom(tmp.path()),
            EngineOptions {
                secrets: Arc::new(MemorySecretStore::default()),
                factories: vec![Arc::new(Factory(llm.clone()))],
                first_run: FirstRunDefaults::default(),
            },
        )
        .await
        .unwrap();
        let profile = engine.config().current().profiles[0].clone();
        let asr = profile.asr_provider.clone();
        let llm_id = profile.llm_provider.clone().unwrap();
        (tmp, engine, llm, asr, llm_id)
    }

    fn one_second_clip() -> AudioClip {
        AudioClip {
            samples: vec![0.5; 16_000],
            sample_rate: 16_000,
        }
    }

    #[test]
    fn record_stop_with_silence_keeps_old_clip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("clip.wav");
        std::fs::write(&path, encode_wav_pcm16(&one_second_clip())).unwrap();
        let audio = FakeAudio::default();
        let rec = Recording::start(&audio, &MicrophoneChoice::SystemDefault, |_| {}).unwrap();
        audio.silence(1.0);
        let err = rec.stop(&mut PeakVad).unwrap_err();
        assert!(matches!(err, BenchError::NoSpeech));
        let status = clip_status(&path);
        assert!(status.present);
        assert!((status.secs - 1.0).abs() < 0.01);
        assert!(status.recorded_at.is_some());
    }

    #[test]
    fn record_stop_with_speech_writes_a_trimmed_clip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("bench").join("clip.wav");
        let audio = FakeAudio::default();
        let levels = Arc::new(Mutex::new(Vec::new()));
        let l2 = levels.clone();
        let rec = Recording::start(&audio, &MicrophoneChoice::SystemDefault, move |r| {
            l2.lock().unwrap().push(r)
        })
        .unwrap();
        audio.silence(0.5);
        audio.speech(1.0);
        audio.silence(0.5);
        let clip = rec.stop(&mut PeakVad).unwrap();
        // Speech plus the trim padding on each side, rounded out to whole
        // detector frames.
        assert!(clip.duration_secs() < 1.7 && clip.duration_secs() > 0.9);
        write_clip(&path, &clip).unwrap();
        let status = clip_status(&path);
        assert!(status.present);
        assert!((status.secs - clip.duration_secs()).abs() < 0.01);
        assert!(!levels.lock().unwrap().is_empty());
    }

    #[test]
    fn recording_is_capped_at_thirty_seconds() {
        let audio = FakeAudio::default();
        let rec = Recording::start(&audio, &MicrophoneChoice::SystemDefault, |_| {}).unwrap();
        audio.speech(31.0);
        audio.speech(1.0);
        let clip = rec.stop(&mut PeakVad).unwrap();
        assert!(clip.duration_secs() <= 30.0);
    }

    #[test]
    fn missing_clip_is_not_present() {
        let tmp = tempfile::tempdir().unwrap();
        let status = clip_status(&tmp.path().join("clip.wav"));
        assert!(!status.present);
        assert_eq!(status.secs, 0.0);
        assert!(status.recorded_at.is_none());
    }

    #[tokio::test]
    async fn run_all_emits_cold_and_warm_rows_in_order_and_feeds_the_llm() {
        let (_tmp, engine, llm, asr_id, llm_id) = rig().await;
        let runs = Arc::new(Mutex::new(Vec::new()));
        let r2 = runs.clone();
        run_all(
            &engine,
            Arc::new(one_second_clip()),
            vec![asr_id.clone()],
            vec![llm_id.clone()],
            "fallback".into(),
            move |run, remaining| r2.lock().unwrap().push((run, remaining)),
        )
        .await;
        let got = runs.lock().unwrap();
        assert_eq!(got.len(), 4);
        assert_eq!(
            (got[0].0.role, got[0].0.cold, got[0].1),
            (BenchRole::Asr, true, 3)
        );
        assert_eq!(
            (got[1].0.role, got[1].0.cold, got[1].1),
            (BenchRole::Asr, false, 2)
        );
        assert_eq!(
            (got[2].0.role, got[2].0.cold, got[2].1),
            (BenchRole::Llm, true, 1)
        );
        assert_eq!(
            (got[3].0.role, got[3].0.cold, got[3].1),
            (BenchRole::Llm, false, 0)
        );
        let calls = llm.calls.lock().unwrap();
        assert!(calls
            .last()
            .unwrap()
            .last()
            .unwrap()
            .content
            .contains("原文"));
    }

    #[tokio::test]
    async fn run_all_uses_fallback_text_when_no_asr_selected() {
        let (_tmp, engine, llm, _asr_id, llm_id) = rig().await;
        let runs = Arc::new(Mutex::new(Vec::new()));
        let r2 = runs.clone();
        run_all(
            &engine,
            Arc::new(one_second_clip()),
            vec![],
            vec![llm_id],
            "fallback".into(),
            move |run, remaining| r2.lock().unwrap().push((run, remaining)),
        )
        .await;
        assert_eq!(runs.lock().unwrap().len(), 2);
        let calls = llm.calls.lock().unwrap();
        assert!(calls
            .last()
            .unwrap()
            .last()
            .unwrap()
            .content
            .contains("fallback"));
    }

    #[test]
    fn history_candidates_skip_entries_without_audio_and_copy_the_clip() {
        use kongwe_core::history::{HistoryStore, NewUtterance};
        use kongwe_core::types::ProfileId;
        let tmp = tempfile::tempdir().unwrap();
        let wav = tmp.path().join("rec.wav");
        std::fs::write(&wav, encode_wav_pcm16(&one_second_clip())).unwrap();
        let history = HistoryStore::open_in_memory().unwrap();
        let row = |audio: Option<std::path::PathBuf>, text: &str| NewUtterance {
            ts: 1,
            profile_id: ProfileId("default".into()),
            app_name: None,
            raw_text: text.into(),
            refined_text: text.into(),
            final_text: text.into(),
            degraded: false,
            audio_path: audio,
            asr_ms: 1,
            llm_ms: None,
        };
        let kept = history.insert(row(Some(wav), "kept")).unwrap();
        let dropped = history.insert(row(None, "dropped")).unwrap();
        let got = history_candidates(&history).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, kept.0);
        assert_eq!(got[0].raw_text, "kept");

        let dest = tmp.path().join("bench").join("clip.wav");
        let status = clip_from_history(&history, kept.0, &dest).unwrap();
        assert!(status.present);
        assert!((status.secs - 1.0).abs() < 0.01);
        let err = clip_from_history(&history, dropped.0, &dest).unwrap_err();
        assert!(err.message.contains("not kept"));
    }
}
