//! whisper.cpp speech recognition provider.
use std::ffi::c_void;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

use async_trait::async_trait;
use arcvox_core::providers::{AsrHint, AsrProvider, ProviderError};
use arcvox_core::types::{AudioClip, ErrorCode, Transcript};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::{lang, CancelFlag, CancelOnDrop};

#[derive(Default)]
struct SharedContext {
    slot: Mutex<Option<Arc<WhisperContext>>>,
    /// Held for the whole load so concurrent callers never load the model twice,
    /// while `slot` stays free for `unload`.
    loading: Mutex<()>,
}

pub struct WhisperAsrProvider {
    path: PathBuf,
    label: String,
    timeout: Duration,
    use_gpu: bool,
    ctx: Arc<SharedContext>,
}

impl WhisperAsrProvider {
    pub fn new(path: PathBuf, label: String, timeout: Duration, use_gpu: bool) -> Self {
        // Routes whisper.cpp and ggml logs to `tracing` instead of stderr.
        static LOG_HOOKS: Once = Once::new();
        LOG_HOOKS.call_once(whisper_rs::install_logging_hooks);
        Self {
            path,
            label,
            timeout,
            use_gpu,
            ctx: Arc::default(),
        }
    }

    fn loader(&self) -> Loader {
        Loader {
            path: self.path.clone(),
            label: self.label.clone(),
            use_gpu: self.use_gpu,
            ctx: self.ctx.clone(),
        }
    }

    /// Loads the model in a blocking task. Not bounded by the ASR timeout: the
    /// abort flag cannot stop a load, and a first load is not an inference stall.
    async fn context(&self) -> Result<Arc<WhisperContext>, ProviderError> {
        let loader = self.loader();
        match tokio::task::spawn_blocking(move || loader.context()).await {
            Ok(r) => r,
            Err(join) => Err(ProviderError::new(
                ErrorCode::InferenceFailed,
                format!("{}: {join}", self.label),
            )),
        }
    }
}

/// Everything needed to load the model from inside a blocking task.
struct Loader {
    path: PathBuf,
    label: String,
    use_gpu: bool,
    ctx: Arc<SharedContext>,
}

impl Loader {
    /// Returns the loaded context, loading it on first use.
    fn context(&self) -> Result<Arc<WhisperContext>, ProviderError> {
        let _loading = self.ctx.loading.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(ctx) = self
            .ctx
            .slot
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            return Ok(ctx.clone());
        }
        let ctx = Arc::new(load_context(&self.path, &self.label, self.use_gpu)?);
        *self.ctx.slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(ctx.clone());
        Ok(ctx)
    }
}

fn load_context(path: &Path, label: &str, use_gpu: bool) -> Result<WhisperContext, ProviderError> {
    if !path.is_file() {
        return Err(ProviderError::new(
            ErrorCode::ModelMissing,
            format!("{label}: model file not found"),
        )
        .with_hint("download or import the model in Settings > Models"));
    }
    check_header(path).map_err(|why| {
        ProviderError::new(
            ErrorCode::ModelCorrupt,
            format!("{label}: not a valid whisper model ({why})"),
        )
        .with_hint("re-download or re-import the model in Settings > Models")
    })?;
    let mut params = WhisperContextParameters::default();
    params.use_gpu(use_gpu);
    params.flash_attn(use_gpu);
    WhisperContext::new_with_params(path, params).map_err(|e| {
        ProviderError::new(
            ErrorCode::ModelCorrupt,
            format!("{label}: cannot load model ({e:?})"),
        )
        .with_hint("re-download or re-import the model in Settings > Models")
    })
}

/// Rejects files whose header whisper.cpp would read without bounds checks.
/// A garbage header can make the loader allocate or index out of range and crash
/// the process instead of returning an error.
fn check_header(path: &Path) -> Result<(), String> {
    const MAGIC: u32 = 0x6767_6d6c;
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    // magic, 11 hyperparameters, then the mel filter dimensions
    let mut head = [0u8; 4 * 14];
    file.read_exact(&mut head)
        .map_err(|_| "file too short".to_string())?;
    let word = |i: usize| u32::from_le_bytes(head[i * 4..i * 4 + 4].try_into().unwrap());
    if word(0) != MAGIC {
        return Err("bad magic".into());
    }
    let n_vocab = word(1);
    let dims = [
        word(2),
        word(3),
        word(4),
        word(5),
        word(6),
        word(7),
        word(8),
        word(9),
    ];
    let n_mels = word(10);
    let (filter_mels, filter_fft) = (word(12), word(13));
    if !(1..=200_000).contains(&n_vocab) {
        return Err("bad vocabulary size".into());
    }
    if dims.iter().any(|d| !(1..=65_536).contains(d)) {
        return Err("bad model dimensions".into());
    }
    if !(1..=512).contains(&n_mels)
        || !(1..=512).contains(&filter_mels)
        || !(1..=4_096).contains(&filter_fft)
    {
        return Err("bad mel filter dimensions".into());
    }
    let min_len = head.len() as u64 + u64::from(filter_mels) * u64::from(filter_fft) * 4 + 4;
    if len < min_len {
        return Err("file truncated".into());
    }
    Ok(())
}

unsafe extern "C" fn abort_requested(data: *mut c_void) -> bool {
    // SAFETY: `data` is the AtomicBool pointer installed in `run`.
    unsafe { (*(data as *const AtomicBool)).load(Ordering::SeqCst) }
}

fn run(
    ctx: &WhisperContext,
    samples: &[f32],
    lang: Option<&'static str>,
    prompt: Option<&str>,
    abort: CancelFlag,
) -> Result<Transcript, String> {
    let mut state = ctx.create_state().map_err(|e| format!("{e:?}"))?;
    let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    p.set_language(lang);
    if lang.is_none() {
        p.set_detect_language(true);
    }
    if let Some(pr) = prompt {
        // whisper.cpp takes a C string; interior NUL bytes would panic in the binding.
        p.set_initial_prompt(&pr.replace('\0', ""));
    }
    p.set_print_progress(false);
    p.set_print_special(false);
    p.set_print_realtime(false);
    p.set_print_timestamps(false);
    p.set_suppress_blank(true);
    p.set_no_speech_thold(0.6);
    p.set_n_threads(
        std::thread::available_parallelism()
            .map(|n| n.get().min(4) as i32)
            .unwrap_or(4),
    );
    // set_abort_callback_safe in whisper-rs 0.16 casts the boxed closure to the
    // wrong type, so the flag is wired through the raw callback instead.
    // SAFETY: the pointer targets the AtomicBool owned by `abort`, which lives
    // until this function returns, after `state.full` has finished.
    unsafe {
        p.set_abort_callback(Some(abort_requested));
        p.set_abort_callback_user_data(Arc::as_ptr(&abort.0) as *mut c_void);
    }
    state.full(p, samples).map_err(|e| format!("{e:?}"))?;
    let mut text = String::new();
    for seg in state.as_iter() {
        text.push_str(&seg.to_str_lossy().map_err(|e| format!("{e:?}"))?);
    }
    let language = whisper_rs::get_lang_str(state.full_lang_id_from_state()).map(str::to_string);
    Ok(Transcript {
        text: text.trim().to_string(),
        language,
    })
}

/// Runs `work` on a blocking thread, bounded by `timeout`. The flag handed to
/// `work` is set on timeout and also when this future is dropped, so a
/// cancelled session stops whisper.cpp at its next abort check.
async fn run_abortable<F>(
    label: &str,
    timeout: Duration,
    work: F,
) -> Result<Transcript, ProviderError>
where
    F: FnOnce(CancelFlag) -> Result<Transcript, String> + Send + 'static,
{
    let abort = CancelFlag::default();
    let _guard = CancelOnDrop(abort.clone());
    let for_task = abort.clone();
    let task = tokio::task::spawn_blocking(move || work(for_task));
    match tokio::time::timeout(timeout, task).await {
        Ok(Ok(Ok(t))) => Ok(t),
        Ok(Ok(Err(msg))) => Err(ProviderError::new(
            ErrorCode::InferenceFailed,
            format!("{label}: {msg}"),
        )),
        Ok(Err(join)) => Err(ProviderError::new(
            ErrorCode::InferenceFailed,
            format!("{label}: {join}"),
        )),
        Err(_) => {
            // The blocking task stops on its own at the next abort check.
            abort.cancel();
            Err(ProviderError::new(
                ErrorCode::ProviderTimeout,
                format!("{label}: transcription timed out"),
            )
            .with_hint("raise the speech recognition timeout in Settings > General"))
        }
    }
}

#[async_trait]
impl AsrProvider for WhisperAsrProvider {
    async fn transcribe(
        &self,
        audio: &AudioClip,
        hint: &AsrHint,
    ) -> Result<Transcript, ProviderError> {
        if audio.sample_rate != 16_000 {
            return Err(ProviderError::new(
                ErrorCode::InferenceFailed,
                format!(
                    "{}: expected 16000 Hz audio, got {} Hz",
                    self.label, audio.sample_rate
                ),
            ));
        }
        let ctx = self.context().await?;
        let samples = audio.samples.clone();
        let lang = lang::whisper_code(hint.language.as_deref());
        let prompt = hint.prompt.clone();
        // The task owns its own Arc of the context, so unload() during a run only
        // frees the model once this task has finished with it.
        run_abortable(&self.label, self.timeout, move |abort| {
            run(&ctx, &samples, lang, prompt.as_deref(), abort)
        })
        .await
    }

    async fn load(&self) -> Result<(), ProviderError> {
        self.context().await.map(|_| ())
    }

    async fn unload(&self) {
        *self.ctx.slot.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcvox_core::providers::{AsrHint, AsrProvider};
    use arcvox_core::types::{AudioClip, ErrorCode};
    use std::path::PathBuf;
    use std::time::Duration;

    #[tokio::test]
    async fn missing_file_is_model_missing() {
        let p = WhisperAsrProvider::new(
            PathBuf::from("/nonexistent/model.bin"),
            "Local".into(),
            Duration::from_secs(5),
            false,
        );
        let clip = AudioClip {
            samples: vec![0.0; 16_000],
            sample_rate: 16_000,
        };
        let err = p.transcribe(&clip, &AsrHint::default()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::ModelMissing);
        assert!(err.hint.as_deref().unwrap().contains("Models"));
    }

    #[tokio::test]
    async fn corrupt_file_is_model_corrupt() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("bad.bin");
        std::fs::write(&f, b"lmgg garbage garbage").unwrap();
        let p = WhisperAsrProvider::new(f, "Local".into(), Duration::from_secs(5), false);
        let clip = AudioClip {
            samples: vec![0.0; 16_000],
            sample_rate: 16_000,
        };
        assert_eq!(
            p.transcribe(&clip, &AsrHint::default())
                .await
                .unwrap_err()
                .code,
            ErrorCode::ModelCorrupt
        );
    }

    #[test]
    fn header_with_huge_vocabulary_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("huge.bin");
        let mut words: Vec<u32> = vec![0x6767_6d6c, 1_918_986_016];
        words.extend([1500, 384, 6, 4, 448, 384, 6, 4, 80, 1, 80, 201]);
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        assert_eq!(bytes.len(), 56);
        std::fs::write(&f, &bytes).unwrap();
        assert_eq!(check_header(&f).unwrap_err(), "bad vocabulary size");
    }

    #[tokio::test]
    async fn non_16k_audio_is_inference_failed() {
        let p = WhisperAsrProvider::new(
            PathBuf::from("/nonexistent/model.bin"),
            "Local".into(),
            Duration::from_secs(5),
            false,
        );
        let clip = AudioClip {
            samples: vec![0.0; 44_100],
            sample_rate: 44_100,
        };
        let err = p.transcribe(&clip, &AsrHint::default()).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::InferenceFailed);
    }

    #[tokio::test]
    async fn dropping_the_future_sets_the_abort_flag() {
        let (tx, rx) = std::sync::mpsc::channel();
        let work = move |abort: CancelFlag| {
            tx.send(abort.clone()).unwrap();
            while !abort.is_cancelled() {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err("aborted".to_string())
        };
        let dropped = tokio::time::timeout(
            Duration::from_millis(20),
            run_abortable("Local", Duration::from_secs(60), work),
        )
        .await;
        assert!(dropped.is_err());
        let seen = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(seen.is_cancelled());
    }

    #[tokio::test]
    async fn timeout_sets_the_abort_flag() {
        let (tx, rx) = std::sync::mpsc::channel();
        let work = move |abort: CancelFlag| {
            tx.send(abort.clone()).unwrap();
            while !abort.is_cancelled() {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err("aborted".to_string())
        };
        let err = run_abortable("Local", Duration::from_millis(20), work)
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ProviderTimeout);
        assert!(rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .is_cancelled());
    }
}
