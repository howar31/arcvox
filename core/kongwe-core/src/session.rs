//! One dictation. Audio is pushed while the hotkey is held; `finish` runs
//! the pipeline; events stream back to the client.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;

use crate::protocol::Event;
use crate::types::{ProfileId, SessionId};

/// Cancellation and completion state shared between a `Session` and the
/// `run_session` task driving it. Bundled into one struct (rather than
/// separate `Arc<AtomicBool>` fields/parameters) to keep `run_session`'s
/// parameter count down.
#[derive(Debug, Default)]
pub(crate) struct SessionFlags {
    /// Set once, by whichever of `run_session` or `cancel` gets there first
    /// (via a swap), so exactly one `Done` is ever sent for a session.
    pub(crate) done_sent: AtomicBool,
    /// Set by `cancel`. Checked by `run_session` at points where a blocking
    /// write (history row, WAV file) is about to become visible, so a
    /// cancelled session records nothing.
    pub(crate) cancelled: AtomicBool,
    /// Set by `run_session` just before it starts the non-abortable tail
    /// (WAV write, history insert). While this is `false`, `cancel` may
    /// abort the task outright; once it is `true`, `cancel` leaves the
    /// task running instead, so it reaches its own `cancelled` checks and
    /// undoes whatever it already wrote rather than being torn down
    /// mid-write with the write left behind (tokio cannot abort a
    /// `spawn_blocking` closure that is already running).
    pub(crate) tail_started: AtomicBool,
}

pub struct Session {
    pub(crate) id: SessionId,
    pub(crate) profile_id: ProfileId,
    pub(crate) audio: Arc<Mutex<Vec<f32>>>,
    pub(crate) finish_tx: Mutex<Option<oneshot::Sender<()>>>,
    pub(crate) events_tx: mpsc::Sender<Event>,
    pub(crate) events_rx: Mutex<Option<mpsc::Receiver<Event>>>,
    pub(crate) task: Mutex<Option<JoinHandle<()>>>,
    pub(crate) flags: Arc<SessionFlags>,
}

impl Session {
    pub fn id(&self) -> &SessionId {
        &self.id
    }

    pub fn profile_id(&self) -> &ProfileId {
        &self.profile_id
    }

    /// Appends 16 kHz mono f32 samples. Safe to call from the audio thread.
    pub fn push_audio(&self, pcm: &[f32]) {
        self.audio.lock().unwrap().extend_from_slice(pcm);
    }

    /// Ends recording and starts the pipeline. Calling twice is a no-op.
    pub fn finish(&self) {
        if let Some(tx) = self.finish_tx.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }

    /// Stops everything. Calling it any number of times is safe. If it wins
    /// the race against `run_session` (the session was still recording,
    /// running its pipeline, or in the middle of recording its tail), it
    /// emits at most one `Done` with no history id and nothing is recorded.
    /// If the session had already completed, `cancel` is a no-op: no event
    /// is emitted, and the row already recorded for it stays.
    pub fn cancel(&self) {
        self.flags.cancelled.store(true, Ordering::SeqCst);
        // Dropping the sender (rather than sending) wakes a task still
        // waiting to be finished without running the pipeline.
        self.finish_tx.lock().unwrap().take();
        // Once the tail has started, a WAV write or history insert may be
        // in flight on the blocking thread pool, which tokio cannot abort;
        // aborting the task here would only tear it down before it reaches
        // its own `cancelled` checks, leaving that write behind instead of
        // undone. So only abort while the task is still guaranteed to be
        // somewhere abortable, i.e. before the tail starts.
        if !self.flags.tail_started.load(Ordering::SeqCst) {
            if let Some(handle) = self.task.lock().unwrap().take() {
                handle.abort();
            }
        }
        if !self.flags.done_sent.swap(true, Ordering::SeqCst) {
            let _ = self.events_tx.try_send(Event::Done {
                session_id: self.id.clone(),
                history_id: None,
            });
        }
    }

    /// The event stream. May be called once; panics on a second call.
    pub fn events(&self) -> ReceiverStream<Event> {
        let rx = self
            .events_rx
            .lock()
            .unwrap()
            .take()
            .expect("Session::events() may be called only once");
        ReceiverStream::new(rx)
    }
}
