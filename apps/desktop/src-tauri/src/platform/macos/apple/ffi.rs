//! Declarations for the entry points in `apple/KongweApple.swift`.
//!
//! Every call except `kw_apple_fm_status` blocks until the framework finishes,
//! so none of them may run on the main thread; call them from `spawn_blocking`
//! or a worker thread.

use std::ffi::{c_char, c_void};

pub type ProgressFn = unsafe extern "C" fn(fraction: f64, user: *mut c_void);

unsafe extern "C" {
    pub fn kw_apple_free(p: *mut c_char);
    pub fn kw_apple_dictation_status(locale: *const c_char) -> i32;
    pub fn kw_apple_dictation_install(
        locale: *const c_char,
        progress: ProgressFn,
        user: *mut c_void,
    ) -> i32;
    /// `cancel` points at an `AtomicBool` the bridge polls; `out` receives a
    /// string owned by the bridge, released with `kw_apple_free`.
    pub fn kw_apple_dictation_transcribe(
        locale: *const c_char,
        pcm: *const f32,
        len: usize,
        sample_rate: i32,
        contextual_json: *const c_char,
        cancel: *const u8,
        out: *mut *mut c_char,
    ) -> i32;
    pub fn kw_apple_fm_status() -> i32;
    /// `max_tokens` <= 0 leaves the length to the model; `out` receives a
    /// string owned by the bridge, released with `kw_apple_free`.
    pub fn kw_apple_fm_complete(
        instructions: *const c_char,
        prompt: *const c_char,
        max_tokens: i32,
        temperature: f64,
        out: *mut *mut c_char,
    ) -> i32;
}
