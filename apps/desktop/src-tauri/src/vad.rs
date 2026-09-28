//! Voice activity detection used once per utterance, after recording, to
//! drop leading and trailing silence and to skip the pipeline when nothing
//! was said. 16 kHz mono f32 in; frames are 256 samples (16 ms).

use std::ops::Range;

pub const FRAME_SAMPLES: usize = 256;
/// 300 ms kept before the first and after the last speech frame.
pub const PAD_SAMPLES: usize = 4800;

pub trait Vad: Send {
    /// `frame` has exactly `FRAME_SAMPLES` samples.
    fn is_speech(&mut self, frame: &[f32]) -> bool;
}

pub struct EarshotVad {
    detector: earshot::Detector,
    threshold: f32,
}

impl EarshotVad {
    pub fn new() -> Self {
        Self {
            detector: earshot::Detector::default(),
            threshold: 0.5,
        }
    }
}

impl Default for EarshotVad {
    fn default() -> Self {
        Self::new()
    }
}

impl Vad for EarshotVad {
    fn is_speech(&mut self, frame: &[f32]) -> bool {
        debug_assert_eq!(frame.len(), FRAME_SAMPLES);
        self.detector.predict_f32(frame) > self.threshold
    }
}

/// Returns the sample range to keep, or `None` when no frame contains
/// speech. A trailing partial frame is ignored by the detector but kept in
/// the range when it follows speech.
pub fn trim(samples: &[f32], vad: &mut dyn Vad) -> Option<Range<usize>> {
    let mut first: Option<usize> = None;
    let mut last_end: usize = 0;
    for (i, frame) in samples.as_chunks::<FRAME_SAMPLES>().0.iter().enumerate() {
        if vad.is_speech(frame) {
            let start = i * FRAME_SAMPLES;
            if first.is_none() {
                first = Some(start);
            }
            last_end = start + FRAME_SAMPLES;
        }
    }
    let first = first?;
    let start = first.saturating_sub(PAD_SAMPLES);
    let end = (last_end + PAD_SAMPLES).min(samples.len());
    Some(start..end)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Speech = any frame whose peak exceeds 0.1.
    struct PeakVad;
    impl Vad for PeakVad {
        fn is_speech(&mut self, frame: &[f32]) -> bool {
            frame.iter().any(|s| s.abs() > 0.1)
        }
    }

    fn signal(silence_before: usize, speech: usize, silence_after: usize) -> Vec<f32> {
        let mut v = vec![0.0; silence_before];
        v.extend((0..speech).map(|i| if i % 2 == 0 { 0.5 } else { -0.5 }));
        v.extend(vec![0.0; silence_after]);
        v
    }

    #[test]
    fn all_silence_is_none() {
        assert_eq!(trim(&vec![0.0; 16_000], &mut PeakVad), None);
        assert_eq!(trim(&[], &mut PeakVad), None);
    }

    #[test]
    fn speech_in_the_middle_keeps_padding_on_both_sides() {
        let s = signal(16_000, 8_000, 16_000);
        let r = trim(&s, &mut PeakVad).unwrap();
        let first_speech_frame = (16_000 / FRAME_SAMPLES) * FRAME_SAMPLES;
        assert_eq!(r.start, first_speech_frame - PAD_SAMPLES);
        let speech_end_frame = (16_000usize + 8_000).div_ceil(FRAME_SAMPLES) * FRAME_SAMPLES;
        assert_eq!(r.end, speech_end_frame + PAD_SAMPLES);
    }

    #[test]
    fn padding_clamps_to_the_buffer() {
        let s = signal(1_000, 8_000, 1_000);
        let r = trim(&s, &mut PeakVad).unwrap();
        assert_eq!(r, 0..s.len());
    }

    #[test]
    fn speech_at_the_very_start_and_end() {
        let s = signal(0, 2_000, 0);
        assert_eq!(trim(&s, &mut PeakVad).unwrap(), 0..2_000);
    }

    #[test]
    fn earshot_scores_a_tone_above_silence() {
        // A synthetic 220 Hz tone with harmonics is not speech, but it must
        // score higher than digital silence; this pins the wiring and the
        // frame size, not the model's accuracy.
        let mut vad = EarshotVad::new();
        let silence = vec![0.0f32; FRAME_SAMPLES];
        let tone: Vec<f32> = (0..FRAME_SAMPLES)
            .map(|i| {
                let t = i as f32 / 16_000.0;
                0.4 * (2.0 * std::f32::consts::PI * 220.0 * t).sin()
                    + 0.2 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()
            })
            .collect();
        let s0 = vad.detector.predict_f32(&silence);
        let s1 = vad.detector.predict_f32(&tone);
        assert!(s1 > s0, "tone {s1} should score above silence {s0}");
        assert!(!EarshotVad::new().is_speech(&silence));
    }
}
