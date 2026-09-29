//! Kongwe dictation core.

pub mod audio;
pub mod config;
pub mod dictionary;
pub mod engine;
pub mod history;
pub mod paths;
pub mod pipeline;
pub mod profile;
pub mod prompt;
pub mod protocol;
pub mod providers;
pub mod secrets;
pub mod session;
pub mod steps;
pub mod types;

pub use config::FirstRunDefaults;
pub use engine::{Engine, EngineError, EngineOptions, SessionRequest, MIN_UTTERANCE_SECS};
pub use paths::DataDir;
pub use protocol::{Event, PROTOCOL_VERSION};
pub use providers::factory::ProviderFactory;
pub use secrets::{MemorySecretStore, SecretError, SecretStore};
pub use session::Session;

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
