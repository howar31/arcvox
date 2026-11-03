//! The part of llama.cpp, mtmd and whisper.cpp the providers use, over the
//! raw bindings in `arcvox-ggml-sys`. Every `unsafe` block in the crate's
//! inference code lives here.
pub mod llama;
pub mod mtmd;
pub mod whisper;

use std::ffi::{c_char, c_void, CStr};
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
    // The libraries report every model load in hundreds of info lines; only
    // warnings and errors reach the info-level log.
    match level {
        sys::GGML_LOG_LEVEL_ERROR => tracing::error!(target: "ggml", "{text}"),
        sys::GGML_LOG_LEVEL_WARN => tracing::warn!(target: "ggml", "{text}"),
        _ => tracing::debug!(target: "ggml", "{text}"),
    }
}
