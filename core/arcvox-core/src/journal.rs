//! Writes a recording to its file while it is still being made, so that a
//! crash leaves what was said up to the last moment. The file is a whole
//! WAV at every flush: the header is rewritten with the length so far.

use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;

use crate::history::HistoryStore;
use crate::types::HistoryId;
use crate::SAMPLE_RATE;

/// How much audio goes by between two flushes of the header.
const FLUSH_EVERY_SAMPLES: usize = SAMPLE_RATE as usize;

enum Msg {
    Chunk(Vec<f32>),
    /// The history row that keeps the recording; its length follows the file.
    Row(HistoryId),
    Close(mpsc::Sender<bool>),
}

/// The writer of one recording. Dropping it closes the file as a whole
/// file too, from the writer's own thread.
pub(crate) struct Journal {
    tx: mpsc::Sender<Msg>,
    thread: Option<JoinHandle<()>>,
}

impl Journal {
    /// Creates the file and starts writing. The file comes first, so that
    /// a row never names a file that is not there; `set_row` names the row
    /// whose length follows the file.
    pub(crate) fn open(path: PathBuf, history: Arc<HistoryStore>) -> std::io::Result<Journal> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let file = BufWriter::new(std::fs::File::create(&path)?);
        let mut writer = hound::WavWriter::new(file, spec).map_err(io_error)?;
        let (tx, rx) = mpsc::channel::<Msg>();
        let thread = std::thread::Builder::new()
            .name("recording-journal".into())
            .spawn(move || {
                let mut since_flush = 0usize;
                let mut whole = true;
                let mut close = None;
                let mut row: Option<HistoryId> = None;
                loop {
                    match rx.recv() {
                        Ok(Msg::Row(id)) => row = Some(id),
                        Ok(Msg::Chunk(chunk)) => {
                            for s in &chunk {
                                let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
                                if writer.write_sample(v).is_err() {
                                    whole = false;
                                }
                            }
                            since_flush += chunk.len();
                            if since_flush >= FLUSH_EVERY_SAMPLES {
                                since_flush = 0;
                                if writer.flush().is_err() {
                                    whole = false;
                                }
                                if let Some(id) = row {
                                    let _ = history.set_audio_ms(id, Some(duration_ms(&writer)));
                                }
                            }
                        }
                        Ok(Msg::Close(reply)) => {
                            close = Some(reply);
                            break;
                        }
                        // The session ended without closing: the file is
                        // made whole all the same.
                        Err(_) => break,
                    }
                }
                let ms = duration_ms(&writer);
                if writer.finalize().is_err() {
                    whole = false;
                }
                if let Some(id) = row {
                    let _ = history.set_audio_ms(id, Some(ms));
                }
                if let Some(reply) = close {
                    let _ = reply.send(whole);
                }
            })?;
        Ok(Journal {
            tx,
            thread: Some(thread),
        })
    }

    pub(crate) fn set_row(&self, row: HistoryId) {
        let _ = self.tx.send(Msg::Row(row));
    }

    pub(crate) fn push(&self, chunk: Vec<f32>) {
        let _ = self.tx.send(Msg::Chunk(chunk));
    }

    /// Finishes the file. True when every write succeeded.
    pub(crate) async fn close(mut self) -> bool {
        let (reply_tx, reply_rx) = mpsc::channel();
        let _ = self.tx.send(Msg::Close(reply_tx));
        let thread = self.thread.take();
        tokio::task::spawn_blocking(move || {
            let whole = reply_rx.recv().unwrap_or(false);
            if let Some(t) = thread {
                let _ = t.join();
            }
            whole
        })
        .await
        .unwrap_or(false)
    }
}

fn duration_ms<W: std::io::Write + std::io::Seek>(writer: &hound::WavWriter<W>) -> u64 {
    u64::from(writer.duration()) * 1000 / u64::from(SAMPLE_RATE)
}

fn io_error(e: hound::Error) -> std::io::Error {
    match e {
        hound::Error::IoError(e) => e,
        other => std::io::Error::other(other.to_string()),
    }
}
