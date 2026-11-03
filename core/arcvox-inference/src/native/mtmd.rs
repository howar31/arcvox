//! mtmd: a model that hears through an audio encoder file.
use std::ffi::{CStr, CString};
use std::path::Path;
use std::ptr::NonNull;

use arcvox_ggml_sys as sys;

use super::llama::{Context, Model};

/// The text that marks where the sound goes in a prompt.
pub fn default_marker() -> &'static str {
    // SAFETY: a static NUL-terminated string inside the library.
    unsafe { CStr::from_ptr(sys::mtmd_default_marker()) }
        .to_str()
        .unwrap_or("<__media__>")
}

/// An audio encoder bound to its model.
pub struct Mtmd {
    ptr: NonNull<sys::mtmd_context>,
}

// SAFETY: callers serialise use (the encoder's evaluation is not thread-safe);
// the context holds no thread-bound state.
unsafe impl Send for Mtmd {}
unsafe impl Sync for Mtmd {}

impl Mtmd {
    pub fn init(
        encoder: &Path,
        model: &Model,
        use_gpu: bool,
        n_threads: i32,
    ) -> Result<Self, String> {
        let path = CString::new(encoder.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        // SAFETY: default parameters from the library; the path and the model
        // outlive the call (the model also outlives the context).
        let ptr = unsafe {
            let mut p = sys::mtmd_context_params_default();
            p.use_gpu = use_gpu;
            p.print_timings = false;
            p.n_threads = n_threads;
            sys::mtmd_init_from_file(path.as_ptr(), model.as_ptr(), p)
        };
        NonNull::new(ptr)
            .map(|ptr| Self { ptr })
            .ok_or_else(|| "the encoder file could not be loaded".to_string())
    }

    pub fn support_audio(&self) -> bool {
        // SAFETY: valid context.
        unsafe { sys::mtmd_support_audio(self.ptr.as_ptr()) }
    }

    /// The sample rate the encoder expects, when it takes sound.
    pub fn audio_sample_rate(&self) -> Option<u32> {
        // SAFETY: valid context.
        let r = unsafe { sys::mtmd_get_audio_sample_rate(self.ptr.as_ptr()) };
        u32::try_from(r).ok().filter(|r| *r > 0)
    }

    /// Splits `text` at its markers and puts `bitmaps` in their places.
    pub fn tokenize(
        &self,
        text: &str,
        add_special: bool,
        parse_special: bool,
        bitmaps: &[&Bitmap],
    ) -> Result<Chunks, String> {
        let c = CString::new(text).map_err(|e| e.to_string())?;
        let input = sys::mtmd_input_text {
            text: c.as_ptr(),
            text_len: c.as_bytes().len(),
            add_special,
            parse_special,
        };
        // SAFETY: a fresh chunk list, filled by `mtmd_tokenize`; every pointer
        // stays valid during the call.
        unsafe {
            let chunks = Chunks {
                ptr: NonNull::new(sys::mtmd_input_chunks_init()).ok_or("no chunk list")?,
            };
            let mut ptrs: Vec<*const sys::mtmd_bitmap> =
                bitmaps.iter().map(|b| b.ptr.as_ptr() as *const _).collect();
            let r = sys::mtmd_tokenize(
                self.ptr.as_ptr(),
                chunks.ptr.as_ptr(),
                &input,
                ptrs.as_mut_ptr(),
                ptrs.len(),
            );
            match r {
                0 => Ok(chunks),
                1 => Err("the number of sounds does not match the markers".into()),
                2 => Err("the sound could not be encoded".into()),
                n => Err(format!("tokenizing failed ({n})")),
            }
        }
    }
}

impl Drop for Mtmd {
    fn drop(&mut self) {
        // SAFETY: owned pointer, freed once.
        unsafe { sys::mtmd_free(self.ptr.as_ptr()) }
    }
}

/// Sound samples for the encoder.
pub struct Bitmap {
    ptr: NonNull<sys::mtmd_bitmap>,
}

impl Bitmap {
    /// Mono samples at the encoder's rate.
    pub fn from_audio(samples: &[f32]) -> Result<Self, String> {
        // SAFETY: the library copies `samples`.
        let ptr = unsafe { sys::mtmd_bitmap_init_from_audio(samples.len(), samples.as_ptr()) };
        NonNull::new(ptr)
            .map(|ptr| Self { ptr })
            .ok_or_else(|| "the sound could not be prepared".to_string())
    }
}

impl Drop for Bitmap {
    fn drop(&mut self) {
        // SAFETY: owned pointer, freed once.
        unsafe { sys::mtmd_bitmap_free(self.ptr.as_ptr()) }
    }
}

/// A tokenized prompt: text and sound pieces.
pub struct Chunks {
    ptr: NonNull<sys::mtmd_input_chunks>,
}

impl Chunks {
    /// Context positions the whole prompt takes.
    pub fn total_positions(&self) -> i32 {
        // SAFETY: valid list.
        unsafe { sys::mtmd_helper_get_n_pos(self.ptr.as_ptr()) }
    }

    /// Decodes every chunk into `ctx` from position `n_past`; returns the
    /// position after the prompt.
    pub fn eval(
        &self,
        mtmd: &Mtmd,
        ctx: &Context<'_>,
        n_past: i32,
        seq_id: i32,
        n_batch: i32,
        logits_last: bool,
    ) -> Result<i32, String> {
        let mut new_n_past = 0;
        // SAFETY: all valid; the result position is written to `new_n_past`.
        let r = unsafe {
            sys::mtmd_helper_eval_chunks(
                mtmd.ptr.as_ptr(),
                ctx.as_ptr(),
                self.ptr.as_ptr(),
                n_past,
                seq_id,
                n_batch,
                logits_last,
                &mut new_n_past,
            )
        };
        if r == 0 {
            Ok(new_n_past)
        } else {
            Err(format!("evaluating the prompt failed ({r})"))
        }
    }
}

impl Drop for Chunks {
    fn drop(&mut self) {
        // SAFETY: owned pointer, freed once.
        unsafe { sys::mtmd_input_chunks_free(self.ptr.as_ptr()) }
    }
}
