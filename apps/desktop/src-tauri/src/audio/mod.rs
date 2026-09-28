//! Microphone capture at the device's native rate, resampled to the core's
//! 16 kHz mono f32.

pub mod capture;
pub mod recorder;
pub mod resample;

use std::sync::mpsc::Sender;

use crate::settings::MicrophoneChoice;

#[derive(Debug, Clone, PartialEq)]
pub enum AudioMsg {
    /// Mono samples at `rate` Hz, in [-1, 1].
    Frames { rate: u32, mono: Vec<f32> },
    /// The stream failed (device removed, format error). No more frames follow.
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AudioError {
    #[error("no input device")]
    NoDevice,
    #[error("unsupported sample format {0}")]
    UnsupportedFormat(String),
    #[error("{0}")]
    Other(String),
}

pub trait AudioHandle: Send {
    fn stop(self: Box<Self>);
}

pub trait AudioSource: Send + Sync {
    fn start(
        &self,
        choice: &MicrophoneChoice,
        tx: Sender<AudioMsg>,
    ) -> Result<Box<dyn AudioHandle>, AudioError>;
    fn list_devices(&self) -> Vec<String>;
}

pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}
