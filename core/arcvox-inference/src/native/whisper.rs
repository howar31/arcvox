//! whisper.cpp models, states and decoding parameters.
use std::ffi::{c_void, CStr, CString};
use std::path::Path;
use std::ptr::NonNull;

use arcvox_ggml_sys as sys;

/// A loaded model, without decoding state; read-only, so shared between threads.
pub struct Whisper {
    ptr: NonNull<sys::whisper_context>,
}

// SAFETY: a context without state is not mutated by decoding; each run uses
// its own state.
unsafe impl Send for Whisper {}
unsafe impl Sync for Whisper {}

impl Whisper {
    pub fn load(path: &Path, use_gpu: bool, flash_attn: bool) -> Result<Self, String> {
        super::init();
        let c_path = CString::new(path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        // SAFETY: default parameters from the library; the path outlives the call.
        let ptr = unsafe {
            let mut p = sys::whisper_context_default_params();
            p.use_gpu = use_gpu;
            p.flash_attn = flash_attn;
            p.gpu_device = 0;
            sys::whisper_init_from_file_with_params_no_state(c_path.as_ptr(), p)
        };
        NonNull::new(ptr)
            .map(|ptr| Self { ptr })
            .ok_or_else(|| "the model file could not be loaded".to_string())
    }

    /// A decoding state for one run.
    pub fn state(&self) -> Result<State<'_>, String> {
        // SAFETY: valid context; the state is freed before the context.
        let ptr = unsafe { sys::whisper_init_state(self.ptr.as_ptr()) };
        NonNull::new(ptr)
            .map(|ptr| State { ptr, ctx: self })
            .ok_or_else(|| "the decoding state could not be created".to_string())
    }
}

impl Drop for Whisper {
    fn drop(&mut self) {
        // SAFETY: owned pointer, freed once, after every state of it.
        unsafe { sys::whisper_free(self.ptr.as_ptr()) }
    }
}

/// The short name of a language id (`zh`, `en`).
pub fn lang_str(id: i32) -> Option<&'static str> {
    // SAFETY: returns a static string or null.
    let p = unsafe { sys::whisper_lang_str(id) };
    if p.is_null() {
        return None;
    }
    // SAFETY: non-null, static, NUL-terminated.
    unsafe { CStr::from_ptr(p) }.to_str().ok()
}

/// Decoding parameters (greedy) with the strings they point to.
pub struct Params {
    raw: sys::whisper_full_params,
    _language: Option<CString>,
    _prompt: Option<CString>,
}

impl Params {
    /// Greedy decoding with `best_of` candidates per step.
    pub fn greedy(best_of: i32) -> Self {
        // SAFETY: default parameters from the library.
        let mut raw = unsafe { sys::whisper_full_default_params(sys::WHISPER_SAMPLING_GREEDY) };
        raw.greedy.best_of = best_of;
        Self {
            raw,
            _language: None,
            _prompt: None,
        }
    }

    /// A language code, or `None` to leave the language unset.
    pub fn language(&mut self, code: Option<&str>) -> &mut Self {
        let c = code.and_then(|c| CString::new(c).ok());
        self.raw.language = c.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());
        self._language = c;
        self
    }

    pub fn detect_language(&mut self, on: bool) -> &mut Self {
        self.raw.detect_language = on;
        self
    }

    /// Text that conditions the decoder; NUL bytes are dropped.
    pub fn initial_prompt(&mut self, text: &str) -> &mut Self {
        let c = CString::new(text.replace('\0', "")).ok();
        self.raw.initial_prompt = c.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());
        self._prompt = c;
        self
    }

    pub fn quiet(&mut self) -> &mut Self {
        self.raw.print_progress = false;
        self.raw.print_special = false;
        self.raw.print_realtime = false;
        self.raw.print_timestamps = false;
        self
    }

    pub fn suppress_blank(&mut self, on: bool) -> &mut Self {
        self.raw.suppress_blank = on;
        self
    }

    pub fn no_speech_thold(&mut self, t: f32) -> &mut Self {
        self.raw.no_speech_thold = t;
        self
    }

    pub fn n_threads(&mut self, n: i32) -> &mut Self {
        self.raw.n_threads = n;
        self
    }

    /// Stops decoding once `flag` reads true. The flag must outlive the run.
    pub fn abort_flag(&mut self, flag: &std::sync::atomic::AtomicBool) -> &mut Self {
        self.raw.abort_callback = Some(abort_requested);
        self.raw.abort_callback_user_data = flag as *const _ as *mut c_void;
        self
    }

    /// The flag given to `abort_flag`, null without one.
    fn abort_flag_ptr(&self) -> *const std::sync::atomic::AtomicBool {
        if self.raw.abort_callback.is_some() {
            self.raw.abort_callback_user_data as *const _
        } else {
            std::ptr::null()
        }
    }
}

unsafe extern "C" fn abort_requested(data: *mut c_void) -> bool {
    // SAFETY: `data` is the AtomicBool set in `Params::abort_flag`, alive for the run.
    unsafe {
        (*(data as *const std::sync::atomic::AtomicBool)).load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// One run's decoding state and its results.
pub struct State<'c> {
    ptr: NonNull<sys::whisper_state>,
    ctx: &'c Whisper,
}

impl State<'_> {
    /// Decodes 16 kHz mono samples.
    pub fn full(&mut self, params: &Params, samples: &[f32]) -> Result<(), String> {
        if samples.is_empty() {
            return Err("no samples".into());
        }
        let flag = params.abort_flag_ptr();
        // SAFETY: valid context and state; the parameters' strings live in
        // `params`, and so does the abort flag for the whole run.
        let r = unsafe {
            super::with_abort_flag(flag, || {
                sys::whisper_full_with_state(
                    self.ctx.ptr.as_ptr(),
                    self.ptr.as_ptr(),
                    params.raw,
                    samples.as_ptr(),
                    samples.len() as i32,
                )
            })
        };
        match r {
            0 => Ok(()),
            -2 => Err("the spectrogram could not be computed".into()),
            -3 => Err("language detection failed".into()),
            -6 => Err("encoding failed".into()),
            -9..=-7 => Err("decoding failed".into()),
            n => Err(format!("decoding failed ({n})")),
        }
    }

    pub fn n_segments(&self) -> i32 {
        // SAFETY: valid state.
        unsafe { sys::whisper_full_n_segments_from_state(self.ptr.as_ptr()) }
    }

    pub fn segment_text(&self, i: i32) -> String {
        // SAFETY: `i` below `n_segments`; the string belongs to the state.
        cstr_lossy(unsafe { sys::whisper_full_get_segment_text_from_state(self.ptr.as_ptr(), i) })
    }

    /// Start and end in centiseconds.
    pub fn segment_times(&self, i: i32) -> (i64, i64) {
        // SAFETY: `i` below `n_segments`.
        unsafe {
            (
                sys::whisper_full_get_segment_t0_from_state(self.ptr.as_ptr(), i),
                sys::whisper_full_get_segment_t1_from_state(self.ptr.as_ptr(), i),
            )
        }
    }

    pub fn segment_no_speech(&self, i: i32) -> f32 {
        // SAFETY: `i` below `n_segments`.
        unsafe { sys::whisper_full_get_segment_no_speech_prob_from_state(self.ptr.as_ptr(), i) }
    }

    pub fn n_tokens(&self, segment: i32) -> i32 {
        // SAFETY: `segment` below `n_segments`.
        unsafe { sys::whisper_full_n_tokens_from_state(self.ptr.as_ptr(), segment) }
    }

    pub fn token_text(&self, segment: i32, token: i32) -> String {
        // SAFETY: indices in range; the string belongs to the context.
        cstr_lossy(unsafe {
            sys::whisper_full_get_token_text_from_state(
                self.ctx.ptr.as_ptr(),
                self.ptr.as_ptr(),
                segment,
                token,
            )
        })
    }

    pub fn token_p(&self, segment: i32, token: i32) -> f32 {
        // SAFETY: indices in range.
        unsafe { sys::whisper_full_get_token_p_from_state(self.ptr.as_ptr(), segment, token) }
    }

    pub fn lang_id(&self) -> i32 {
        // SAFETY: valid state.
        unsafe { sys::whisper_full_lang_id_from_state(self.ptr.as_ptr()) }
    }
}

impl Drop for State<'_> {
    fn drop(&mut self) {
        // SAFETY: owned pointer, freed once, before its context.
        unsafe { sys::whisper_free_state(self.ptr.as_ptr()) }
    }
}

fn cstr_lossy(p: *const std::ffi::c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: non-null, NUL-terminated, valid while the state lives.
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}
