//! One dictation. Audio is pushed while the hotkey is held; `finish` runs
//! the pipeline; events stream back to the client.

use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;

use crate::protocol::Event;
use crate::types::{ProfileId, SessionId};

pub struct Session {
    pub(crate) id: SessionId,
    pub(crate) profile_id: ProfileId,
    pub(crate) audio: Arc<Mutex<Vec<f32>>>,
    pub(crate) finish_tx: Mutex<Option<oneshot::Sender<()>>>,
    pub(crate) events_tx: mpsc::Sender<Event>,
    pub(crate) events_rx: Mutex<Option<mpsc::Receiver<Event>>>,
    pub(crate) task: Mutex<Option<JoinHandle<()>>>,
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

    /// Stops everything. Emits `Done` with no history id; records nothing.
    pub fn cancel(&self) {
        let was_recording = self.finish_tx.lock().unwrap().take().is_some();
        if was_recording {
            // Dropping the sender wakes the task, which emits Done and exits.
            return;
        }
        if let Some(handle) = self.task.lock().unwrap().take() {
            handle.abort();
        }
        let _ = self.events_tx.try_send(Event::Done {
            session_id: self.id.clone(),
            history_id: None,
        });
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
