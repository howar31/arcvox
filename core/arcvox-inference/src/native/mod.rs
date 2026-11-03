//! The part of llama.cpp, mtmd and whisper.cpp the providers use, over the
//! raw bindings in `arcvox-ggml-sys`. Every `unsafe` block in the crate's
//! inference code lives here.
pub mod llama;
pub mod mtmd;
pub mod whisper;

use std::cell::Cell;
use std::ffi::{c_char, c_void, CStr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;

use arcvox_ggml_sys as sys;

/// Starts the llama.cpp backend once per process and routes every library's
/// log lines to `tracing`.
pub fn init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // SAFETY: plain C setters; `log` lives for the whole process.
        unsafe {
            sys::llama_log_set(Some(log), std::ptr::null_mut());
            sys::mtmd_helper_log_set(Some(log), std::ptr::null_mut());
            sys::whisper_log_set(Some(log), std::ptr::null_mut());
            sys::llama_backend_init();
        }
    });
}

thread_local! {
    /// The abort flag of the run on this thread, null outside a run.
    static ABORT_FLAG: Cell<*const AtomicBool> = const { Cell::new(std::ptr::null()) };
}

/// Runs `f` with `flag` as this thread's abort flag. The libraries report a
/// run their abort callback stopped as a failure, so while the flag reads
/// true their error lines go to the debug log instead.
///
/// # Safety
/// `flag` is null or points at an `AtomicBool` alive until `f` returns.
pub(crate) unsafe fn with_abort_flag<R>(flag: *const AtomicBool, f: impl FnOnce() -> R) -> R {
    struct Restore(*const AtomicBool);
    impl Drop for Restore {
        fn drop(&mut self) {
            ABORT_FLAG.set(self.0);
        }
    }
    let _restore = Restore(ABORT_FLAG.replace(flag));
    f()
}

fn aborting() -> bool {
    let flag = ABORT_FLAG.get();
    // SAFETY: `with_abort_flag` keeps the flag alive while it is set here.
    !flag.is_null() && unsafe { (*flag).load(Ordering::SeqCst) }
}

#[derive(Debug, PartialEq, Eq)]
enum Route {
    Error,
    Warn,
    Debug,
}

/// The libraries report every model load in hundreds of info lines; only
/// warnings and errors reach the info-level log, and an error reported
/// while the caller is aborting the run is the abort itself.
fn route(level: sys::ggml_log_level, aborting: bool) -> Route {
    match level {
        sys::GGML_LOG_LEVEL_ERROR if !aborting => Route::Error,
        sys::GGML_LOG_LEVEL_WARN => Route::Warn,
        _ => Route::Debug,
    }
}

unsafe extern "C" fn log(level: sys::ggml_log_level, text: *const c_char, _: *mut c_void) {
    if text.is_null() {
        return;
    }
    // SAFETY: the libraries pass a NUL-terminated string valid for this call.
    let text = unsafe { CStr::from_ptr(text) }.to_string_lossy();
    let text = text.trim_end();
    if text.is_empty() {
        return;
    }
    match route(level, aborting()) {
        Route::Error => tracing::error!(target: "ggml", "{text}"),
        Route::Warn => tracing::warn!(target: "ggml", "{text}"),
        Route::Debug => tracing::debug!(target: "ggml", "{text}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_stay_errors_unless_the_run_is_being_aborted() {
        assert_eq!(route(sys::GGML_LOG_LEVEL_ERROR, false), Route::Error);
        assert_eq!(route(sys::GGML_LOG_LEVEL_ERROR, true), Route::Debug);
        assert_eq!(route(sys::GGML_LOG_LEVEL_WARN, true), Route::Warn);
        assert_eq!(route(sys::GGML_LOG_LEVEL_INFO, false), Route::Debug);
    }

    #[test]
    fn the_abort_flag_counts_only_while_set_and_raised() {
        let flag = AtomicBool::new(false);
        assert!(!aborting());
        // SAFETY: `flag` outlives the closure.
        unsafe {
            with_abort_flag(&flag, || {
                assert!(!aborting());
                flag.store(true, Ordering::SeqCst);
                assert!(aborting());
            });
        }
        assert!(!aborting());
    }
}
