//! Arcvox dictation core.

pub mod audio;
pub mod backup;
pub mod bench;
pub mod config;
pub mod device;
pub mod dictionary;
pub mod engine;
pub mod eval;
pub mod history;
pub(crate) mod journal;
pub mod learn;
pub mod models;
pub mod paths;
pub mod pipeline;
pub mod profile;
pub mod prompt;
pub mod protocol;
pub mod providers;
pub mod secrets;
pub mod segments;
pub mod session;
pub mod settings_file;
pub mod stats;
pub mod steps;
pub mod tidy;
pub mod types;

pub use config::FirstRunDefaults;
pub use device::Device;
pub use engine::{
    BackupSource, DeviceOptions, Engine, EngineError, EngineOptions, ReprocessError, Reprocessed,
    SessionRequest, MIN_UTTERANCE_SECS,
};
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
