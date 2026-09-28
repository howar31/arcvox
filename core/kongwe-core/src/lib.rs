//! Kongwe dictation core.

pub mod protocol;
pub mod types;

/// Sample rate every session accepts, in Hz. Mono f32 samples.
pub const SAMPLE_RATE: u32 = 16_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_rate_is_16k() {
        assert_eq!(SAMPLE_RATE, 16_000);
    }
}
