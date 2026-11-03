//! llama.cpp models, contexts, batches and samplers.
use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr::NonNull;

use arcvox_ggml_sys as sys;

pub type Token = sys::llama_token;

/// A loaded model. Read-only after loading, so it may be shared between threads.
pub struct Model {
    ptr: NonNull<sys::llama_model>,
}

// SAFETY: llama.cpp does not mutate a model after loading; contexts made from
// it on other threads only read it.
unsafe impl Send for Model {}
unsafe impl Sync for Model {}

impl Model {
    /// Loads a GGUF file; `n_gpu_layers` = `u32::MAX` puts every layer on the GPU.
    pub fn load(path: &Path, n_gpu_layers: u32) -> Result<Self, String> {
        super::init();
        let c_path = CString::new(path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        // SAFETY: default parameters from the library; the path outlives the call.
        let ptr = unsafe {
            let mut params = sys::llama_model_default_params();
            params.n_gpu_layers = i32::try_from(n_gpu_layers).unwrap_or(i32::MAX);
            sys::llama_model_load_from_file(c_path.as_ptr(), params)
        };
        NonNull::new(ptr)
            .map(|ptr| Self { ptr })
            .ok_or_else(|| "the model file could not be loaded".to_string())
    }

    pub(crate) fn as_ptr(&self) -> *mut sys::llama_model {
        self.ptr.as_ptr()
    }

    fn vocab(&self) -> *const sys::llama_vocab {
        // SAFETY: the model is valid for `self`'s lifetime.
        unsafe { sys::llama_model_get_vocab(self.ptr.as_ptr()) }
    }

    pub fn n_vocab(&self) -> i32 {
        // SAFETY: as above.
        unsafe { sys::llama_vocab_n_tokens(self.vocab()) }
    }

    /// Tokenizes `text`, special tokens parsed; `add_special` adds the
    /// beginning-of-text token where the model uses one.
    pub fn tokenize(&self, text: &str, add_special: bool) -> Result<Vec<Token>, String> {
        let c = CString::new(text).map_err(|e| e.to_string())?;
        let len = i32::try_from(c.as_bytes().len()).map_err(|e| e.to_string())?;
        let mut tokens: Vec<Token> = vec![0; (text.len() / 2 + usize::from(add_special)).max(8)];
        for _ in 0..2 {
            // SAFETY: the buffer holds `tokens.len()` entries; a negative result
            // is the size needed, and the call is retried once with it.
            let n = unsafe {
                sys::llama_tokenize(
                    self.vocab(),
                    c.as_ptr(),
                    len,
                    tokens.as_mut_ptr(),
                    i32::try_from(tokens.len()).unwrap_or(i32::MAX),
                    add_special,
                    true,
                )
            };
            if n >= 0 {
                tokens.truncate(n as usize);
                return Ok(tokens);
            }
            tokens.resize((-n) as usize, 0);
        }
        Err("tokenizing failed".into())
    }

    /// The bytes of one token; `special` renders special tokens as text.
    pub fn token_bytes(&self, token: Token, special: bool) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; 8];
        for _ in 0..2 {
            // SAFETY: the buffer holds `buf.len()` bytes; a negative result is
            // the size needed.
            let n = unsafe {
                sys::llama_token_to_piece(
                    self.vocab(),
                    token,
                    buf.as_mut_ptr().cast::<c_char>(),
                    i32::try_from(buf.len()).unwrap_or(i32::MAX),
                    0,
                    special,
                )
            };
            if n >= 0 {
                buf.truncate(n as usize);
                return Ok(buf);
            }
            buf.resize((-n) as usize, 0);
        }
        Err("token text failed".into())
    }

    /// The token's text, decoded through `decoder` so a character split across
    /// tokens comes out whole.
    pub fn token_piece(
        &self,
        token: Token,
        decoder: &mut encoding_rs::Decoder,
        special: bool,
    ) -> Result<String, String> {
        let bytes = self.token_bytes(token, special)?;
        let mut out = String::with_capacity(
            decoder
                .max_utf8_buffer_length(bytes.len())
                .unwrap_or(bytes.len() * 3),
        );
        let _ = decoder.decode_to_string(&bytes, &mut out, false);
        Ok(out)
    }

    pub fn is_eog(&self, token: Token) -> bool {
        // SAFETY: the model is valid.
        unsafe { sys::llama_vocab_is_eog(self.vocab(), token) }
    }

    /// The model's built-in chat template.
    pub fn chat_template(&self) -> Option<String> {
        // SAFETY: the returned string belongs to the model and is copied at once.
        let p = unsafe { sys::llama_model_chat_template(self.ptr.as_ptr(), std::ptr::null()) };
        if p.is_null() {
            return None;
        }
        // SAFETY: non-null, NUL-terminated.
        Some(unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }

    /// Renders `messages` (role, content) with `template`; `add_assistant`
    /// opens the assistant's turn.
    pub fn apply_chat_template(
        &self,
        template: &str,
        messages: &[(&str, &str)],
        add_assistant: bool,
    ) -> Result<String, String> {
        let tpl = CString::new(template).map_err(|e| e.to_string())?;
        let owned: Vec<(CString, CString)> = messages
            .iter()
            .map(|(r, c)| Ok((CString::new(*r)?, CString::new(*c)?)))
            .collect::<Result<_, std::ffi::NulError>>()
            .map_err(|e| e.to_string())?;
        let chat: Vec<sys::llama_chat_message> = owned
            .iter()
            .map(|(r, c)| sys::llama_chat_message {
                role: r.as_ptr(),
                content: c.as_ptr(),
            })
            .collect();
        let size = owned
            .iter()
            .map(|(r, c)| r.as_bytes().len() + c.as_bytes().len())
            .sum::<usize>()
            * 2;
        let mut buf = vec![0u8; size.max(1)];
        for _ in 0..2 {
            // SAFETY: every pointer stays valid during the call; the buffer
            // holds `buf.len()` bytes and a larger result is the size needed.
            let n = unsafe {
                sys::llama_chat_apply_template(
                    tpl.as_ptr(),
                    chat.as_ptr(),
                    chat.len(),
                    add_assistant,
                    buf.as_mut_ptr().cast::<c_char>(),
                    i32::try_from(buf.len()).unwrap_or(i32::MAX),
                )
            };
            if n < 0 {
                return Err("the chat template is not supported".into());
            }
            let n = n as usize;
            if n <= buf.len() {
                buf.truncate(n);
                return String::from_utf8(buf).map_err(|e| e.to_string());
            }
            buf.resize(n, 0);
        }
        Err("rendering the chat template failed".into())
    }
}

impl Drop for Model {
    fn drop(&mut self) {
        // SAFETY: owned pointer, freed once.
        unsafe { sys::llama_model_free(self.ptr.as_ptr()) }
    }
}

/// One inference context of a model.
pub struct Context<'m> {
    ptr: NonNull<sys::llama_context>,
    _model: &'m Model,
}

impl<'m> Context<'m> {
    pub fn new(model: &'m Model, n_ctx: u32, n_batch: u32, n_threads: i32) -> Result<Self, String> {
        // SAFETY: default parameters from the library; the model outlives the context.
        let ptr = unsafe {
            let mut p = sys::llama_context_default_params();
            p.n_ctx = n_ctx;
            p.n_batch = n_batch;
            p.n_threads = n_threads;
            p.n_threads_batch = n_threads;
            sys::llama_init_from_model(model.as_ptr(), p)
        };
        NonNull::new(ptr)
            .map(|ptr| Self { ptr, _model: model })
            .ok_or_else(|| "the context could not be created".to_string())
    }

    pub(crate) fn as_ptr(&self) -> *mut sys::llama_context {
        self.ptr.as_ptr()
    }

    pub fn decode(&mut self, batch: &mut Batch) -> Result<(), String> {
        // SAFETY: both are valid; the batch holds `n_tokens` initialised entries.
        let r = unsafe { sys::llama_decode(self.ptr.as_ptr(), batch.raw) };
        match r {
            0 => Ok(()),
            1 => Err("no room in the context for the batch".into()),
            n => Err(format!("decoding failed ({n})")),
        }
    }
}

impl Drop for Context<'_> {
    fn drop(&mut self) {
        // SAFETY: owned pointer, freed once.
        unsafe { sys::llama_free(self.ptr.as_ptr()) }
    }
}

/// Tokens to decode, one sequence each.
pub struct Batch {
    raw: sys::llama_batch,
    capacity: usize,
}

impl Batch {
    pub fn new(capacity: usize, n_seq_max: i32) -> Self {
        // SAFETY: allocates `capacity` entries of every array.
        let raw = unsafe { sys::llama_batch_init(capacity as i32, 0, n_seq_max) };
        Self { raw, capacity }
    }

    pub fn clear(&mut self) {
        self.raw.n_tokens = 0;
    }

    pub fn n_tokens(&self) -> i32 {
        self.raw.n_tokens
    }

    pub fn add(&mut self, token: Token, pos: i32, seq_ids: &[i32], logits: bool) -> Result<(), String> {
        let i = self.raw.n_tokens as usize;
        if i >= self.capacity {
            return Err(format!("the batch holds {} tokens", self.capacity));
        }
        // SAFETY: `i` is below the allocated length of every array; each
        // `seq_id` entry was allocated for `n_seq_max` ids.
        unsafe {
            *self.raw.token.add(i) = token;
            *self.raw.pos.add(i) = pos;
            *self.raw.n_seq_id.add(i) = seq_ids.len() as i32;
            for (j, s) in seq_ids.iter().enumerate() {
                *(*self.raw.seq_id.add(i)).add(j) = *s;
            }
            *self.raw.logits.add(i) = i8::from(logits);
        }
        self.raw.n_tokens += 1;
        Ok(())
    }
}

impl Drop for Batch {
    fn drop(&mut self) {
        // SAFETY: allocated by `llama_batch_init`, freed once.
        unsafe { sys::llama_batch_free(self.raw) }
    }
}

/// A token sampler, or a chain of them.
pub struct Sampler {
    ptr: NonNull<sys::llama_sampler>,
}

// SAFETY: a sampler is used by one thread at a time; it holds no thread-bound state.
unsafe impl Send for Sampler {}

impl Sampler {
    fn wrap(ptr: *mut sys::llama_sampler) -> Self {
        Self {
            ptr: NonNull::new(ptr).expect("llama.cpp returned a sampler"),
        }
    }

    /// Runs `samplers` in order; the chain takes ownership of them.
    pub fn chain(samplers: impl IntoIterator<Item = Sampler>) -> Self {
        // SAFETY: default chain parameters; each added sampler is owned by the
        // chain from then on, so its own handle must not free it.
        unsafe {
            let chain = sys::llama_sampler_chain_init(sys::llama_sampler_chain_default_params());
            for s in samplers {
                sys::llama_sampler_chain_add(chain, s.ptr.as_ptr());
                std::mem::forget(s);
            }
            Self::wrap(chain)
        }
    }

    pub fn greedy() -> Self {
        // SAFETY: constructor without arguments.
        Self::wrap(unsafe { sys::llama_sampler_init_greedy() })
    }

    pub fn penalties(n_vocab: i32, last_n: i32, repeat: f32, freq: f32, present: f32) -> Self {
        // SAFETY: plain values.
        Self::wrap(unsafe { sys::llama_sampler_init_penalties(n_vocab, last_n, repeat, freq, present) })
    }

    pub fn top_p(p: f32, min_keep: usize) -> Self {
        // SAFETY: plain values.
        Self::wrap(unsafe { sys::llama_sampler_init_top_p(p, min_keep) })
    }

    pub fn temp(t: f32) -> Self {
        // SAFETY: plain value.
        Self::wrap(unsafe { sys::llama_sampler_init_temp(t) })
    }

    pub fn dist(seed: u32) -> Self {
        // SAFETY: plain value.
        Self::wrap(unsafe { sys::llama_sampler_init_dist(seed) })
    }

    /// Picks the next token from the logits at batch index `idx` (-1 = last)
    /// and accepts it into the sampler's history.
    pub fn sample(&mut self, ctx: &Context<'_>, idx: i32) -> Token {
        // SAFETY: both valid; `idx` names a position with logits.
        unsafe { sys::llama_sampler_sample(self.ptr.as_ptr(), ctx.as_ptr(), idx) }
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        // SAFETY: owned pointer (a chain frees the samplers it holds).
        unsafe { sys::llama_sampler_free(self.ptr.as_ptr()) }
    }
}
