//! Apple on-device speech through the Swift bridge in `apple/KongweApple.swift`.
//!
//! The bridge calls block the calling thread, so none of these functions may
//! run on the main thread.

mod dictation;
mod ffi;

use std::ffi::{c_void, CString};
use std::sync::Mutex;

pub use dictation::AppleAsrProvider;

/// Readiness of an Apple on-device model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeStatus {
    Ready,
    NeedsDownload,
    LocaleUnsupported,
    OsTooOld,
    NotEligible,
    IntelligenceOff,
    ModelNotReady,
    Failed(i32),
}

impl NativeStatus {
    /// Maps a dictation status code from the bridge.
    pub fn from_code(code: i32) -> Self {
        match code {
            0 => Self::Ready,
            1 => Self::NeedsDownload,
            2 => Self::LocaleUnsupported,
            3 => Self::OsTooOld,
            other => Self::Failed(other),
        }
    }
}

/// Maps a BCP 47 language hint to the dictation locale; unknown hints fall back to `zh_TW`.
pub fn locale_for_hint(hint: Option<&str>) -> &'static str {
    let primary = hint
        .and_then(|h| h.split(['-', '_']).next())
        .map(str::to_ascii_lowercase);
    match primary.as_deref() {
        Some("en") => "en_US",
        Some("ja") => "ja_JP",
        _ => "zh_TW",
    }
}

fn c_locale(locale: &str) -> CString {
    CString::new(locale.replace('\0', "")).expect("no interior NUL")
}

/// Whether dictation for `locale` (for example `zh_TW`) is installed. Blocks.
pub fn dictation_status(locale: &str) -> NativeStatus {
    let loc = c_locale(locale);
    // SAFETY: `loc` outlives the call; the bridge copies the string.
    NativeStatus::from_code(unsafe { ffi::kw_apple_dictation_status(loc.as_ptr()) })
}

unsafe extern "C" fn progress_trampoline<F: FnMut(f64)>(fraction: f64, user: *mut c_void) {
    // SAFETY: `user` is the `Mutex<F>` owned by `dictation_install`, alive for the call.
    let f = unsafe { &*(user as *const Mutex<F>) };
    (f.lock().unwrap_or_else(|p| p.into_inner()))(fraction);
}

/// Downloads Apple's dictation asset for `locale`, reporting progress in `0.0..=1.0`.
/// Blocks until the install finishes; `progress` runs on a framework thread.
pub fn dictation_install(
    locale: &str,
    progress: impl FnMut(f64) + Send,
) -> Result<(), NativeStatus> {
    fn inner<F: FnMut(f64) + Send>(loc: &CString, progress: &Mutex<F>) -> i32 {
        // SAFETY: `loc` and `progress` outlive the call, which returns only after
        // the bridge has stopped reporting progress.
        unsafe {
            ffi::kw_apple_dictation_install(
                loc.as_ptr(),
                progress_trampoline::<F>,
                progress as *const Mutex<F> as *mut c_void,
            )
        }
    }
    match NativeStatus::from_code(inner(&c_locale(locale), &Mutex::new(progress))) {
        NativeStatus::Ready => Ok(()),
        other => Err(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_for_hint_maps_supported_hints() {
        assert_eq!(locale_for_hint(Some("zh-TW")), "zh_TW");
        assert_eq!(locale_for_hint(Some("zh")), "zh_TW");
        assert_eq!(locale_for_hint(Some("en")), "en_US");
        assert_eq!(locale_for_hint(Some("ja-JP")), "ja_JP");
        assert_eq!(locale_for_hint(None), "zh_TW");
        assert_eq!(locale_for_hint(Some("de")), "zh_TW");
    }

    #[test]
    fn status_maps_os_too_old() {
        assert_eq!(NativeStatus::from_code(3), NativeStatus::OsTooOld);
        assert_eq!(NativeStatus::from_code(1), NativeStatus::NeedsDownload);
        assert_eq!(NativeStatus::from_code(42), NativeStatus::Failed(42));
    }
}
