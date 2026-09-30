//! Audio helpers. Sessions carry f32 samples; HTTP providers get WAV PCM16.

use std::io::Cursor;

use crate::types::AudioClip;

pub fn encode_wav_pcm16(clip: &AudioClip) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: clip.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec).expect("in-memory writer");
        for s in &clip.samples {
            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
            writer.write_sample(v).expect("in-memory write");
        }
        writer.finalize().expect("in-memory finalize");
    }
    cursor.into_inner()
}

/// Reads a mono 16-bit PCM WAV file, the format `encode_wav_pcm16` writes.
pub fn decode_wav_pcm16(bytes: &[u8]) -> Result<AudioClip, String> {
    let reader = hound::WavReader::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    if spec.channels != 1 {
        return Err("expected mono audio".into());
    }
    if spec.bits_per_sample != 16 || spec.sample_format != hound::SampleFormat::Int {
        return Err("expected 16-bit PCM".into());
    }
    let samples = reader
        .into_samples::<i16>()
        .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(AudioClip {
        samples,
        sample_rate: spec.sample_rate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_and_samples_round_trip() {
        let clip = AudioClip {
            samples: vec![0.0, 0.5, -0.5, 1.0, -1.0, 2.0],
            sample_rate: 16_000,
        };
        let bytes = encode_wav_pcm16(&clip);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        let reader = hound::WavReader::new(Cursor::new(bytes)).unwrap();
        let spec = reader.spec();
        assert_eq!(
            (spec.channels, spec.sample_rate, spec.bits_per_sample),
            (1, 16_000, 16)
        );
        let samples: Vec<i16> = reader.into_samples::<i16>().map(|s| s.unwrap()).collect();
        assert_eq!(samples[0], 0);
        assert_eq!(samples[1], 16_384);
        assert_eq!(samples[2], -16_384);
        assert_eq!(samples[3], i16::MAX);
        assert_eq!(samples[4], -i16::MAX);
        assert_eq!(samples[5], i16::MAX, "values above 1.0 are clamped");
    }

    #[test]
    fn decode_round_trips_the_encoder_and_rejects_other_files() {
        let clip = AudioClip {
            samples: vec![0.0, 0.5, -0.5],
            sample_rate: 16_000,
        };
        let back = decode_wav_pcm16(&encode_wav_pcm16(&clip)).unwrap();
        assert_eq!(back.sample_rate, 16_000);
        assert_eq!(back.samples.len(), 3);
        assert!((back.samples[1] - 0.5).abs() < 1e-3);
        assert!(decode_wav_pcm16(b"not a wav").is_err());
        let stereo = {
            let spec = hound::WavSpec {
                channels: 2,
                sample_rate: 16_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut cursor = Cursor::new(Vec::new());
            {
                let mut w = hound::WavWriter::new(&mut cursor, spec).unwrap();
                w.write_sample(0i16).unwrap();
                w.write_sample(0i16).unwrap();
                w.finalize().unwrap();
            }
            cursor.into_inner()
        };
        assert!(decode_wav_pcm16(&stereo).unwrap_err().contains("mono"));
    }
}
