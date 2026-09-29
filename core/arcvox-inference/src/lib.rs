//! In-process speech recognition and language model providers.
pub mod chat;
pub mod lang;
pub mod llama;
pub mod whisper;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Shared abort flag: set by the session, read by the inference abort callback.
#[derive(Clone, Debug, Default)]
pub struct CancelFlag(pub Arc<AtomicBool>);

impl CancelFlag {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}
