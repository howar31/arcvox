//! Streaming resampler to 16 kHz built on rubato 0.16 `SincFixedIn`.

use kongwe_core::SAMPLE_RATE;
use rubato::{
    Resampler as _, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

const CHUNK: usize = 1024;

pub struct Resampler {
    inner: Option<SincFixedIn<f32>>,
    pending: Vec<f32>,
}

impl Resampler {
    pub fn new(input_rate: u32) -> Resampler {
        if input_rate == SAMPLE_RATE {
            return Resampler {
                inner: None,
                pending: Vec::new(),
            };
        }
        let params = SincInterpolationParameters {
            sinc_len: 128,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 128,
            window: WindowFunction::BlackmanHarris2,
        };
        let inner = SincFixedIn::<f32>::new(
            SAMPLE_RATE as f64 / input_rate as f64,
            1.0,
            params,
            CHUNK,
            1,
        )
        .expect("valid resampler parameters");
        Resampler {
            inner: Some(inner),
            pending: Vec::with_capacity(CHUNK * 2),
        }
    }

    pub fn push(&mut self, mono: &[f32], out: &mut Vec<f32>) {
        let Some(inner) = self.inner.as_mut() else {
            out.extend_from_slice(mono);
            return;
        };
        self.pending.extend_from_slice(mono);
        while self.pending.len() >= CHUNK {
            let chunk: Vec<f32> = self.pending.drain(..CHUNK).collect();
            if let Ok(res) = inner.process(&[chunk], None) {
                out.extend_from_slice(&res[0]);
            }
        }
    }

    pub fn flush(&mut self, out: &mut Vec<f32>) {
        let Some(inner) = self.inner.as_mut() else {
            return;
        };
        if self.pending.is_empty() {
            return;
        }
        let rest = std::mem::take(&mut self.pending);
        if let Ok(res) = inner.process_partial(Some(&[rest]), None) {
            out.extend_from_slice(&res[0]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, hz: f32, secs: f32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize)
            .map(|i| (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin() * 0.5)
            .collect()
    }

    #[test]
    fn sixteen_k_passes_through() {
        let mut r = Resampler::new(16_000);
        let mut out = Vec::new();
        r.push(&[0.1, 0.2, 0.3], &mut out);
        r.flush(&mut out);
        assert_eq!(out, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn forty_eight_k_becomes_sixteen_k_with_similar_energy() {
        let input = sine(48_000, 440.0, 1.0);
        let mut r = Resampler::new(48_000);
        let mut out = Vec::new();
        for block in input.chunks(480) {
            r.push(block, &mut out);
        }
        r.flush(&mut out);
        let expected = 16_000usize;
        assert!(
            (out.len() as i64 - expected as i64).abs() < 400,
            "got {} samples",
            out.len()
        );
        let in_rms = crate::audio::rms(&input);
        let out_rms = crate::audio::rms(&out[1000..out.len() - 1000]);
        assert!((in_rms - out_rms).abs() < 0.05, "in {in_rms} out {out_rms}");
    }

    #[test]
    fn forty_four_one_k_length_is_proportional() {
        let input = sine(44_100, 300.0, 0.5);
        let mut r = Resampler::new(44_100);
        let mut out = Vec::new();
        r.push(&input, &mut out);
        r.flush(&mut out);
        assert!((out.len() as i64 - 8_000).abs() < 400, "got {}", out.len());
    }
}
