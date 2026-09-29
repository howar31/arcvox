//! In-process speech recognition and language model providers.
pub mod chat;
pub mod factory;
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

/// Sets the flag when dropped. Held across the await on a blocking inference
/// task, so dropping the provider's future (a cancelled session) stops the work.
#[derive(Debug)]
pub struct CancelOnDrop(pub CancelFlag);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
