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
        // The detector takes samples within [-1, 1]. A loud input, or the
        // resampler next to a clipped peak, goes past that.
        let mut bounded = [0.0f32; FRAME_SAMPLES];
        for (out, sample) in bounded.iter_mut().zip(frame) {
            *out = if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
        self.detector.predict_f32(&bounded) > self.threshold
    }
}

/// A range this long or longer with less than `SPARSE_RATIO` of its frames
/// speech is sparse: a key pressed and released around a long silence, as
/// the detector hears the press and the release.
pub const SPARSE_MIN_SAMPLES: usize = 20 * 16_000;
pub const SPARSE_RATIO: f32 = 0.05;

/// What a recording held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// No frame contained speech.
    Nothing,
    /// Speech frames few and far apart over a long range: not worth a
    /// transcription, but kept for the user to judge.
    Sparse(Range<usize>),
    /// The range to keep: the speech with `PAD_SAMPLES` on both sides.
    Speech(Range<usize>),
}

/// Where the speech is in a recording, found frame by frame as the
/// recording grows, so the range to keep is known the moment it ends.
#[derive(Default)]
struct Scan {
    /// Samples already looked at.
    seen: usize,
    first: Option<usize>,
    last_end: usize,
    /// Frames that contained speech.
    speech: usize,
}

impl Scan {
    /// Looks at the whole frames added to `samples` since the last call.
    fn feed(&mut self, samples: &[f32], vad: &mut dyn Vad) {
        while self.seen + FRAME_SAMPLES <= samples.len() {
            let frame = &samples[self.seen..self.seen + FRAME_SAMPLES];
            if vad.is_speech(frame) {
                self.first.get_or_insert(self.seen);
                self.last_end = self.seen + FRAME_SAMPLES;
                self.speech += 1;
            }
            self.seen += FRAME_SAMPLES;
        }
    }

    /// What the `len` samples fed so far held. A trailing partial frame is
    /// not looked at but kept in the range when it follows speech.
    fn found(&self, len: usize) -> Found {
        let Some(first) = self.first else {
            return Found::Nothing;
        };
        let start = first.saturating_sub(PAD_SAMPLES);
        let end = (self.last_end + PAD_SAMPLES).min(len);
        let span = self.last_end - first;
        let sparse = span >= SPARSE_MIN_SAMPLES
            && ((self.speech * FRAME_SAMPLES) as f32) < span as f32 * SPARSE_RATIO;
        if sparse {
            Found::Sparse(start..end)
        } else {
            Found::Speech(start..end)
        }
    }
}

/// Returns the sample range to keep, or `None` when no frame contains
/// speech.
pub fn trim(samples: &[f32], vad: &mut dyn Vad) -> Option<Range<usize>> {
    let mut scan = Scan::default();
    scan.feed(samples, vad);
    match scan.found(samples.len()) {
        Found::Nothing => None,
        Found::Sparse(r) | Found::Speech(r) => Some(r),
    }
}

/// `trim` for a recording that is still being made: fed as the audio
/// arrives, it has the range ready when the recording ends, whatever its
/// length.
pub struct SpeechSpan {
    vad: Box<dyn Vad>,
    scan: Scan,
}

impl SpeechSpan {
    pub fn new(vad: Box<dyn Vad>) -> Self {
        Self {
            vad,
            scan: Scan::default(),
        }
    }

    /// Looks at the whole frames added to `samples` since the last call.
    pub fn feed(&mut self, samples: &[f32]) {
        self.scan.feed(samples, self.vad.as_mut());
    }

    /// What the `len` samples fed so far held.
    pub fn found(&self, len: usize) -> Found {
        self.scan.found(len)
    }
}

/// Watches a growing recording for a pause after speech.
pub struct SilenceWatch {
    vad: Box<dyn Vad>,
    /// Samples already looked at.
    seen: usize,
    heard_speech: bool,
    silent_samples: usize,
    limit_samples: usize,
}

impl SilenceWatch {
    pub fn new(vad: Box<dyn Vad>, silence_secs: u32, sample_rate: u32) -> Self {
        Self {
            vad,
            seen: 0,
            heard_speech: false,
            silent_samples: 0,
            limit_samples: silence_secs as usize * sample_rate as usize,
        }
    }

    /// Looks at the whole frames added to `samples` since the last call.
    /// True once speech has been followed by silence for the set time; a
    /// recording that starts with silence is left running.
    pub fn paused(&mut self, samples: &[f32]) -> bool {
        while self.seen + FRAME_SAMPLES <= samples.len() {
            let frame = &samples[self.seen..self.seen + FRAME_SAMPLES];
            self.seen += FRAME_SAMPLES;
            if self.vad.is_speech(frame) {
                self.heard_speech = true;
                self.silent_samples = 0;
            } else {
                self.silent_samples += FRAME_SAMPLES;
            }
        }
        self.heard_speech && self.silent_samples >= self.limit_samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_outside_the_range_are_taken() {
        // A loud input, or the resampler next to a clipped peak, goes past 1.0.
        for value in [1.2f32, 5.0, -5.0, f32::INFINITY, f32::NAN] {
            let samples = vec![value; FRAME_SAMPLES * 4];
            let mut vad = EarshotVad::default();
            let _ = trim(&samples, &mut vad);
        }
    }

    #[test]
    fn a_pause_counts_only_after_speech() {
        let mut watch = SilenceWatch::new(Box::new(PeakVad), 1, 16_000);
        let mut audio = signal(32_000, 0, 0);
        assert!(!watch.paused(&audio), "silence before any speech");
        audio.extend(signal(0, 8_000, 8_000));
        assert!(!watch.paused(&audio), "half a second of silence");
        audio.extend(signal(0, 0, 8_704));
        assert!(watch.paused(&audio));
        audio.extend(signal(0, 4_096, 0));
        assert!(!watch.paused(&audio), "speech again resets the pause");
    }

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
    fn a_span_fed_in_pieces_equals_the_trim_of_the_whole() {
        let s = signal(16_000, 8_000, 16_000);
        let mut span = SpeechSpan::new(Box::new(PeakVad));
        // Pieces that do not end on a frame boundary.
        let mut fed = 0;
        for piece in [100usize, 15_000, 9_000, 15_900] {
            fed += piece;
            span.feed(&s[..fed]);
        }
        assert_eq!(fed, s.len());
        assert_eq!(
            span.found(s.len()),
            Found::Speech(trim(&s, &mut PeakVad).unwrap())
        );
        assert_eq!(
            SpeechSpan::new(Box::new(PeakVad)).found(16_000),
            Found::Nothing
        );
    }

    #[test]
    fn a_long_range_with_little_speech_is_sparse() {
        // A key pressed and released around a long silence: a few frames
        // of noise at each end, nothing in between.
        let mut s = signal(0, 512, 0);
        s.extend(signal(30 * 16_000, 1_024, 0));
        let mut span = SpeechSpan::new(Box::new(PeakVad));
        span.feed(&s);
        let Found::Sparse(r) = span.found(s.len()) else {
            panic!("sparse expected");
        };
        assert_eq!(r, 0..s.len());
        // The same ends with speech in between are speech.
        let mut s = signal(0, 512, 0);
        s.extend(signal(16_000, 20 * 16_000, 0));
        s.extend(signal(9 * 16_000, 1_024, 0));
        let mut span = SpeechSpan::new(Box::new(PeakVad));
        span.feed(&s);
        assert!(matches!(span.found(s.len()), Found::Speech(_)));
        // Under 20 s the proportion does not count.
        let mut s = signal(0, 512, 0);
        s.extend(signal(15 * 16_000, 512, 0));
        let mut span = SpeechSpan::new(Box::new(PeakVad));
        span.feed(&s);
        assert!(matches!(span.found(s.len()), Found::Speech(_)));
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
