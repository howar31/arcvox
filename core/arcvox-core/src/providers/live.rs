//! A speech session that takes the audio while it is recorded, whatever
//! engine runs it.

use std::future::Future;
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::providers::ProviderError;
use crate::types::{ErrorCode, Transcript};

/// Gets the text so far each time it changes.
pub type Partial = Arc<dyn Fn(String) + Send + Sync>;

/// What a session receives, in order.
pub enum LiveInput {
    /// 16 kHz mono samples.
    Audio(Vec<f32>),
    /// The recording ended; the final text is wanted.
    End,
}

/// A session under way. Audio pushed before the engine is ready waits in
/// order; dropping the stream without `finish` closes the input and stops
/// the session.
pub struct LiveStream {
    input: mpsc::UnboundedSender<LiveInput>,
    result: oneshot::Receiver<Result<Transcript, ProviderError>>,
    task: JoinHandle<()>,
}

impl LiveStream {
    /// Runs `session` on the current tokio runtime with the input as it comes.
    pub fn spawn<F, Fut>(session: F) -> Self
    where
        F: FnOnce(mpsc::UnboundedReceiver<LiveInput>) -> Fut,
        Fut: Future<Output = Result<Transcript, ProviderError>> + Send + 'static,
    {
        let (input, rx) = mpsc::unbounded_channel();
        let (done, result) = oneshot::channel();
        let run = session(rx);
        let task = tokio::spawn(async move {
            let r = run.await;
            if let Err(e) = &r {
                tracing::warn!(code = ?e.code, error = %e.message, "live session failed");
            }
            let _ = done.send(r);
        });
        Self {
            input,
            result,
            task,
        }
    }

    /// Appends 16 kHz mono samples.
    pub fn push(&self, samples: &[f32]) {
        let _ = self.input.send(LiveInput::Audio(samples.to_vec()));
    }

    /// Ends the audio and waits for the final text.
    pub async fn finish(mut self) -> Result<Transcript, ProviderError> {
        let _ = self.input.send(LiveInput::End);
        match (&mut self.result).await {
            Ok(r) => r,
            Err(_) => Err(ProviderError::new(
                ErrorCode::Internal,
                "the live session ended without an answer",
            )),
        }
    }
}

impl Drop for LiveStream {
    fn drop(&mut self) {
        self.task.abort();
    }
}
