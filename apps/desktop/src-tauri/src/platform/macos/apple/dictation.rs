//! `AsrProvider` over Apple's on-device dictation.

use std::ffi::{c_char, CStr, CString};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use async_trait::async_trait;
use arcvox_core::providers::{AsrHint, AsrProvider, ProviderError};
use arcvox_core::types::{AudioClip, ErrorCode, Transcript};
use arcvox_inference::{CancelFlag, CancelOnDrop};

use super::{ffi, locale_for_hint};

pub struct AppleAsrProvider {
    /// `None` derives the locale from each call's language hint.
    locale: Option<String>,
    label: String,
    timeout: Duration,
}

impl AppleAsrProvider {
    /// `locale` is a dictation locale such as `zh_TW` (see `locale_for_hint`).
    pub fn new(locale: String, label: String, timeout: Duration) -> Self {
        Self {
            locale: Some(locale),
            label,
            timeout,
        }
    }

    /// Picks the dictation locale from `AsrHint::language` at each call.
    pub fn from_hint(label: String, timeout: Duration) -> Self {
        Self {
            locale: None,
            label,
            timeout,
        }
    }

    fn locale_for(&self, hint: &AsrHint) -> String {
        self.locale
            .clone()
            .unwrap_or_else(|| locale_for_hint(hint.language.as_deref()).to_string())
    }
}

/// Hotwords arrive as one per line; the bridge takes them as a JSON array.
fn contextual_json(prompt: Option<&str>) -> String {
    let words: Vec<&str> = prompt
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    serde_json::to_string(&words).expect("strings serialize")
}

fn run(
    locale: &str,
    samples: &[f32],
    sample_rate: u32,
    contextual: &str,
    cancel: &AtomicBool,
) -> Result<String, i32> {
    let loc = CString::new(locale.replace('\0', "")).expect("no interior NUL");
    let ctx = CString::new(contextual.replace('\0', "")).expect("no interior NUL");
    let rate = i32::try_from(sample_rate).map_err(|_| 9)?;
    let mut out: *mut c_char = std::ptr::null_mut();
    // SAFETY: every pointer outlives the call; `cancel` is an AtomicBool, which
    // has the layout of a u8 the bridge only reads.
    let code = unsafe {
        ffi::kw_apple_dictation_transcribe(
            loc.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            rate,
            ctx.as_ptr(),
            cancel.as_ptr() as *const u8,
            &mut out,
        )
    };
    let text = if out.is_null() {
        None
    } else {
        // SAFETY: a non-null `out` is a NUL-terminated string from the bridge,
        // freed right after the copy.
        let s = unsafe { CStr::from_ptr(out) }
            .to_string_lossy()
            .into_owned();
        unsafe { ffi::kw_apple_free(out) };
        Some(s)
    };
    match (code, text) {
        (0, Some(t)) => Ok(t),
        (0, None) => Err(9),
        (c, _) => Err(c),
    }
}

fn map_code(code: i32, label: &str) -> ProviderError {
    match code {
        1 => ProviderError::new(
            ErrorCode::NativeUnavailable,
            format!("{label}: dictation language not installed"),
        )
        .with_hint("install the dictation language in Settings > Engines and models"),
        2 => ProviderError::new(
            ErrorCode::NativeUnavailable,
            format!("{label}: this language is not supported by Apple dictation"),
        ),
        3 => ProviderError::new(
            ErrorCode::NativeUnavailable,
            format!("{label}: requires macOS 26"),
        ),
        8 => ProviderError::new(ErrorCode::Cancelled, format!("{label}: cancelled")),
        c => ProviderError::new(
            ErrorCode::InferenceFailed,
            format!("{label}: dictation failed (code {c})"),
        ),
    }
}

#[async_trait]
impl AsrProvider for AppleAsrProvider {
    async fn transcribe(
        &self,
        audio: &AudioClip,
        hint: &AsrHint,
    ) -> Result<Transcript, ProviderError> {
        let samples = audio.samples.clone();
        let rate = audio.sample_rate;
        let locale = self.locale_for(hint);
        let contextual = contextual_json(hint.prompt.as_deref());
        let text = run_abortable(&self.label, self.timeout, move |cancel| {
            run(&locale, &samples, rate, &contextual, &cancel.0)
        })
        .await?;
        Ok(Transcript {
            text: text.trim().to_string(),
            language: hint.language.clone(),
        })
    }
}

/// Runs `work` on a blocking thread, bounded by `timeout`. The flag handed to
/// `work` is set on timeout and also when this future is dropped, so a
/// cancelled session stops the analyzer (the bridge polls the flag).
async fn run_abortable<F>(label: &str, timeout: Duration, work: F) -> Result<String, ProviderError>
where
    F: FnOnce(CancelFlag) -> Result<String, i32> + Send + 'static,
{
    let cancel = CancelFlag::default();
    let _guard = CancelOnDrop(cancel.clone());
    let for_task = cancel.clone();
    // The bridge blocks its thread, so it must never run on the main thread.
    let task = tokio::task::spawn_blocking(move || work(for_task));
    match tokio::time::timeout(timeout, task).await {
        Ok(Ok(Ok(text))) => Ok(text),
        Ok(Ok(Err(code))) => Err(map_code(code, label)),
        Ok(Err(join)) => Err(ProviderError::new(
            ErrorCode::InferenceFailed,
            format!("{label}: {join}"),
        )),
        Err(_) => {
            cancel.cancel();
            Err(ProviderError::new(
                ErrorCode::ProviderTimeout,
                format!("{label}: transcription timed out"),
            )
            .with_hint("raise the speech recognition timeout in Settings > General"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_lines_become_a_json_array() {
        assert_eq!(
            contextual_json(Some("Arcvox\n\n 語音 \n")),
            r#"["Arcvox","語音"]"#
        );
        assert_eq!(contextual_json(None), "[]");
    }

    #[test]
    fn codes_map_to_error_kinds() {
        let e = map_code(1, "Apple");
        assert_eq!(e.code, ErrorCode::NativeUnavailable);
        assert_eq!(
            e.hint.as_deref(),
            Some("install the dictation language in Settings > Engines and models")
        );
        assert_eq!(map_code(2, "Apple").code, ErrorCode::NativeUnavailable);
        assert_eq!(map_code(3, "Apple").code, ErrorCode::NativeUnavailable);
        assert_eq!(map_code(8, "Apple").code, ErrorCode::Cancelled);
        assert_eq!(map_code(9, "Apple").code, ErrorCode::InferenceFailed);
    }

    #[test]
    fn locale_comes_from_the_hint_unless_fixed() {
        let hint = AsrHint {
            language: Some("ja".into()),
            prompt: None,
        };
        let from_hint = AppleAsrProvider::from_hint("A".into(), Duration::from_secs(1));
        assert_eq!(from_hint.locale_for(&hint), "ja_JP");
        assert_eq!(from_hint.locale_for(&AsrHint::default()), "zh_TW");
        let fixed = AppleAsrProvider::new("en_US".into(), "A".into(), Duration::from_secs(1));
        assert_eq!(fixed.locale_for(&hint), "en_US");
    }

    fn wait_for_flag(
        tx: std::sync::mpsc::Sender<CancelFlag>,
    ) -> impl FnOnce(CancelFlag) -> Result<String, i32> {
        move |cancel| {
            tx.send(cancel.clone()).unwrap();
            while !cancel.is_cancelled() {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(8)
        }
    }

    #[tokio::test]
    async fn dropping_the_future_sets_the_cancel_flag() {
        let (tx, rx) = std::sync::mpsc::channel();
        let dropped = tokio::time::timeout(
            Duration::from_millis(20),
            run_abortable("Apple", Duration::from_secs(60), wait_for_flag(tx)),
        )
        .await;
        assert!(dropped.is_err());
        assert!(rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .is_cancelled());
    }

    #[tokio::test]
    async fn timeout_sets_the_cancel_flag() {
        let (tx, rx) = std::sync::mpsc::channel();
        let err = run_abortable("Apple", Duration::from_millis(20), wait_for_flag(tx))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ProviderTimeout);
        assert!(rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .is_cancelled());
    }
}
