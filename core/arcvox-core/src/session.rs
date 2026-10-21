//! One dictation. Audio is pushed while the hotkey is held; `store` keeps
//! the recording, `finish` runs the pipeline; events stream back to the
//! client.

use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
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
    /// Set with `cancelled` by `cancel_keep`: what the dictation had
    /// produced so far stays in the history instead of being removed.
    pub(crate) keep: AtomicBool,
    /// The part of the audio that holds speech, given by `cancel_keep`
    /// while the recording was still going.
    pub(crate) keep_range: Mutex<Option<Range<usize>>>,
    /// Wakes `run_session` out of a pipeline run when the dictation is
    /// cancelled and kept.
    pub(crate) cancel: tokio::sync::Notify,
}

/// What a session is told while it records and once its recording is complete.
pub(crate) enum SessionCmd {
    /// A block of audio, in the order it was spoken.
    Audio(Vec<f32>),
    /// Keep the recording now, before anything is done with it.
    Store,
    /// Run the pipeline on this part of the audio, or on all of it.
    Finish(Option<Range<usize>>),
}

pub struct Session {
    pub(crate) id: SessionId,
    pub(crate) profile_id: ProfileId,
    pub(crate) audio: Arc<Mutex<Vec<f32>>>,
    /// Samples kept at most; see `engine::MAX_UTTERANCE_SECS`.
    pub(crate) max_samples: usize,
    pub(crate) cmd_tx: Mutex<Option<mpsc::UnboundedSender<SessionCmd>>>,
    pub(crate) events_tx: mpsc::Sender<Event>,
    pub(crate) events_rx: Mutex<Option<mpsc::Receiver<Event>>>,
    pub(crate) task: Mutex<Option<JoinHandle<()>>>,
    pub(crate) flags: Arc<SessionFlags>,
    /// Runs when `cancel` or `cancel_keep` is the one that sends `Done`:
    /// the engine's idle timer restarts on that path as well, and, unless
    /// the dictation is kept, a recording kept for it is removed.
    pub(crate) on_cancel_done: Arc<dyn Fn(bool) + Send + Sync>,
}

impl Session {
    pub fn id(&self) -> &SessionId {
        &self.id
    }

    pub fn profile_id(&self) -> &ProfileId {
        &self.profile_id
    }

    /// Appends 16 kHz mono f32 samples. Safe to call from the audio thread.
    /// When recordings are kept, the block goes to the recording's file at
    /// once, so a dictation cut short by a crash is not lost. Audio past
    /// the session's limit is dropped.
    pub fn push_audio(&self, pcm: &[f32]) {
        let kept = {
            let mut audio = self.audio.lock().unwrap();
            let room = self.max_samples.saturating_sub(audio.len());
            if room == 0 {
                return;
            }
            if pcm.len() > room {
                tracing::warn!(
                    samples = self.max_samples,
                    "the recording reached its limit; further audio is dropped"
                );
            }
            let kept = &pcm[..pcm.len().min(room)];
            audio.extend_from_slice(kept);
            kept.to_vec()
        };
        if let Some(tx) = self.cmd_tx.lock().unwrap().as_ref() {
            let _ = tx.send(SessionCmd::Audio(kept));
        }
    }

    /// The recording is complete. When recordings are kept it is written
    /// now, with a history row that says the dictation did not finish, so
    /// whatever happens next cannot lose it: the result replaces that row,
    /// a failure names its reason in it, and a session dropped without
    /// `finish` or `cancel` leaves it to be processed again from the
    /// history. Without `store`, `finish` does the same first.
    pub fn store(&self) {
        if let Some(tx) = self.cmd_tx.lock().unwrap().as_ref() {
            let _ = tx.send(SessionCmd::Store);
        }
    }

    /// Ends recording and starts the pipeline. Calling twice is a no-op.
    pub fn finish(&self) {
        self.end(None);
    }

    /// `finish` for the part of the audio that holds speech; the kept
    /// recording becomes that part.
    pub fn finish_range(&self, range: Range<usize>) {
        self.end(Some(range));
    }

    fn end(&self, range: Option<Range<usize>>) {
        if let Some(tx) = self.cmd_tx.lock().unwrap().take() {
            let _ = tx.send(SessionCmd::Finish(range));
        }
    }

    /// Stops everything. Calling it any number of times is safe. If it wins
    /// the race against `run_session` (the session was still recording,
    /// running its pipeline, or in the middle of recording its tail), it
    /// emits at most one `Done` with no history id and nothing is recorded:
    /// a recording kept by `store` is removed with its row.
    /// If the session had already completed, `cancel` is a no-op: no event
    /// is emitted, and the row already recorded for it stays.
    pub fn cancel(&self) {
        self.flags.cancelled.store(true, Ordering::SeqCst);
        // Dropping the sender (rather than sending) wakes a task still
        // waiting to be finished without running the pipeline.
        self.cmd_tx.lock().unwrap().take();
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
            self.send_done();
            (self.on_cancel_done)(false);
        }
    }

    /// Stops the dictation now, like `cancel`, but keeps what it had
    /// produced: a cancelled row of the history with the recording (when
    /// recordings are kept) and the text the stages had made by then, so
    /// the dictation can be processed again or its text taken from there.
    /// `range` is the part of the audio that holds speech, for a dictation
    /// still being recorded; a recording too short to hold speech leaves
    /// nothing. `Done` carries no row: the row is written after it.
    pub fn cancel_keep(&self, range: Option<Range<usize>>) {
        *self.flags.keep_range.lock().unwrap() = range;
        self.flags.keep.store(true, Ordering::SeqCst);
        self.flags.cancelled.store(true, Ordering::SeqCst);
        // The task is not aborted: it stops at its next step boundary and
        // writes the row itself.
        self.cmd_tx.lock().unwrap().take();
        self.flags.cancel.notify_one();
        if !self.flags.done_sent.swap(true, Ordering::SeqCst) {
            self.send_done();
            (self.on_cancel_done)(true);
        }
    }

    /// Sends the `Done` of a cancel without waiting. The channel is wide
    /// enough for a session's events, so the event normally goes in at
    /// once; when it is full the send finishes on a task of its own, so the
    /// listener still gets its `Done`. Without a runtime it is dropped,
    /// as before.
    fn send_done(&self) {
        let done = Event::Done {
            session_id: self.id.clone(),
            history_id: None,
        };
        if let Err(mpsc::error::TrySendError::Full(done)) = self.events_tx.try_send(done) {
            let tx = self.events_tx.clone();
            match tokio::runtime::Handle::try_current() {
                Ok(handle) => {
                    handle.spawn(async move {
                        let _ = tx.send(done).await;
                    });
                }
                Err(_) => tracing::warn!("the event channel is full; the cancel's Done is dropped"),
            }
        }
    }

    /// The event stream. May be called once; panics on a second call.
    /// `Done` is the last event of a session: nothing follows it, although
    /// the stream itself stays open for as long as the `Session` lives.
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
