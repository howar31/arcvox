//! Kongwe dictation core.

pub mod config;
pub mod dictionary;
pub mod paths;
pub mod prompt;
pub mod protocol;
pub mod secrets;
pub mod steps;
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
