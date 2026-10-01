//! Accumulates one utterance at 16 kHz and reports the level of each block.

use arcvox_core::SAMPLE_RATE;

use super::resample::Resampler;
use super::rms;

pub struct Recorder {
    resampler: Resampler,
    samples: Vec<f32>,
}

impl Recorder {
    pub fn new(input_rate: u32) -> Recorder {
        Recorder {
            resampler: Resampler::new(input_rate),
            samples: Vec::with_capacity(SAMPLE_RATE as usize * 30),
        }
    }

    /// Appends a block and returns its RMS level (device rate, before resampling).
    pub fn push(&mut self, mono: &[f32]) -> f32 {
        self.resampler.push(mono, &mut self.samples);
        rms(mono)
    }

    /// What was recorded so far, at the engine's sample rate.
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    pub fn len_secs(&self) -> f32 {
        self.samples.len() as f32 / SAMPLE_RATE as f32
    }

    pub fn finish(mut self) -> Vec<f32> {
        self.resampler.flush(&mut self.samples);
        self.samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulates_and_reports_level() {
        let mut r = Recorder::new(16_000);
        let level = r.push(&[0.5; 1600]);
        assert!((level - 0.5).abs() < 1e-6);
        assert_eq!(r.push(&[0.0; 1600]), 0.0);
        assert!((r.len_secs() - 0.2).abs() < 1e-6);
        assert_eq!(r.finish().len(), 3200);
    }
}
