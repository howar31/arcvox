//! Tauri IPC. Every command is a thin wrapper over a plain function so the
//! logic is testable without Tauri.

pub mod backup;
pub mod bench;
pub mod client;
pub mod config;
pub mod dictionary;
pub mod history;
pub mod models;
pub mod native;
pub mod profiles;
pub mod prompt;
pub mod providers;
pub mod quality;
pub mod secrets;
pub mod share;
pub mod stats;
pub mod system;
pub mod update;

use std::collections::BTreeMap;

use serde::Serialize;

/// What a command reports when it fails. `code` names the kind of failure
/// for the interface's own wording, with `params` for its placeholders;
/// `message` is the reason as the backend gives it, shown when no wording
/// exists for the code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CmdError {
    pub message: String,
    pub code: Option<&'static str>,
    pub params: BTreeMap<&'static str, String>,
}

impl CmdError {
    /// A failure with a code and no placeholders.
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        CmdError {
            message: message.into(),
            code: Some(code),
            params: BTreeMap::new(),
        }
    }

    /// A failure with a code and placeholders.
    pub fn with(
        code: &'static str,
        message: impl Into<String>,
        params: &[(&'static str, String)],
    ) -> Self {
        CmdError {
            message: message.into(),
            code: Some(code),
            params: params.iter().cloned().collect(),
        }
    }

    /// A failure the interface shows as the backend words it.
    pub fn plain(message: impl Into<String>) -> Self {
        CmdError {
            message: message.into(),
            code: None,
            params: BTreeMap::new(),
        }
    }
}

impl From<String> for CmdError {
    fn from(message: String) -> Self {
        CmdError::plain(message)
    }
}

impl From<&str> for CmdError {
    fn from(message: &str) -> Self {
        CmdError::plain(message)
    }
}

impl From<std::io::Error> for CmdError {
    fn from(e: std::io::Error) -> Self {
        CmdError::with("io", e.to_string(), &[("error", e.to_string())])
    }
}

impl From<tauri::Error> for CmdError {
    fn from(e: tauri::Error) -> Self {
        CmdError::with("internal", e.to_string(), &[("error", e.to_string())])
    }
}

impl From<tokio::task::JoinError> for CmdError {
    fn from(e: tokio::task::JoinError) -> Self {
        CmdError::with("internal", e.to_string(), &[("error", e.to_string())])
    }
}

impl From<arcvox_core::engine::EngineError> for CmdError {
    fn from(e: arcvox_core::engine::EngineError) -> Self {
        use arcvox_core::engine::EngineError as E;
        let message = e.to_string();
        match e {
            E::Store(e) => e.into(),
            E::History(e) => e.into(),
            E::Backup(e) => e.into(),
            E::Settings(e) => e.into(),
            E::Io(e) => e.into(),
            E::UnknownSession(_) => CmdError::new("unknown_session", message),
            E::Busy => CmdError::new("busy", message),
            E::Folder(reason) => CmdError::with("folder_unusable", message, &[("reason", reason)]),
            E::Task(e) => CmdError::with("internal", message, &[("error", e.to_string())]),
        }
    }
}

impl From<arcvox_core::config::StoreError> for CmdError {
    fn from(e: arcvox_core::config::StoreError) -> Self {
        use arcvox_core::config::StoreError as E;
        let message = e.to_string();
        match e {
            E::Io { source, .. } => {
                CmdError::with("config_io", message, &[("error", source.to_string())])
            }
            E::Broken { backup, reason } => CmdError::with(
                "config_broken",
                message,
                &[("backup", backup.display().to_string()), ("reason", reason)],
            ),
            E::UnsupportedSchema(_) => CmdError::new("config_newer", message),
            // Only a reader outside the application reads without side effects.
            E::Unreadable { .. } => CmdError::plain(message),
            E::Invalid(e) => e.into(),
        }
    }
}

impl From<arcvox_core::config::ConfigError> for CmdError {
    fn from(e: arcvox_core::config::ConfigError) -> Self {
        use arcvox_core::config::ConfigError as E;
        let message = e.to_string();
        match e {
            E::DuplicateId { id, .. } => {
                CmdError::with("config_duplicate_id", message, &[("id", id)])
            }
            E::UnknownDefaultEngine { role, provider } => CmdError::with(
                "config_unknown_default_engine",
                message,
                &[
                    ("role", format!("{role:?}").to_lowercase()),
                    ("provider", provider.to_string()),
                ],
            ),
            E::DefaultEngineRole { role, provider } => CmdError::with(
                "config_default_engine_role",
                message,
                &[
                    ("role", format!("{role:?}").to_lowercase()),
                    ("provider", provider.to_string()),
                ],
            ),
            E::CapabilityMismatch {
                profile,
                provider,
                layer,
            } => CmdError::with(
                "config_capability",
                message,
                &[
                    ("profile", profile.to_string()),
                    ("provider", provider.to_string()),
                    ("layer", layer.to_string()),
                ],
            ),
            E::DefaultProfileCount(n) => {
                CmdError::with("config_default_count", message, &[("count", n.to_string())])
            }
            E::StepOrder { profile, reason } => CmdError::with(
                "config_step_order",
                message,
                &[("profile", profile.to_string()), ("reason", reason)],
            ),
            E::EmbeddedProviderIncomplete(provider) => CmdError::with(
                "config_embedded_incomplete",
                message,
                &[("provider", provider.to_string())],
            ),
            E::GeneralRange { field, min } => CmdError::with(
                "config_min",
                message,
                &[("field", field.to_string()), ("min", min.to_string())],
            ),
            E::GeneralMax { field, max } => CmdError::with(
                "config_max",
                message,
                &[("field", field.to_string()), ("max", max.to_string())],
            ),
            E::SnippetWithoutAlias(term) => {
                CmdError::with("config_snippet_alias", message, &[("term", term)])
            }
        }
    }
}

impl From<arcvox_core::history::HistoryError> for CmdError {
    fn from(e: arcvox_core::history::HistoryError) -> Self {
        use arcvox_core::history::HistoryError as E;
        let message = e.to_string();
        match e {
            E::Sqlite(e) => CmdError::with("database", message, &[("error", e.to_string())]),
            E::Io(e) => e.into(),
            E::OwnDevice => CmdError::new("own_device", message),
        }
    }
}

impl From<arcvox_core::models::ModelError> for CmdError {
    fn from(e: arcvox_core::models::ModelError) -> Self {
        use arcvox_core::models::ModelError as E;
        let message = e.to_string();
        match e {
            E::NotInCatalogue(model) => {
                CmdError::with("model_not_in_catalogue", message, &[("model", model)])
            }
            E::NotInstalled(model) => {
                CmdError::with("model_not_installed", message, &[("model", model)])
            }
            E::UnknownFormat => CmdError::new("model_unknown_format", message),
            E::Io(e) => e.into(),
            E::Manifest(reason) => CmdError::with("manifest", message, &[("reason", reason)]),
            E::Http(reason) => CmdError::with("download_failed", message, &[("reason", reason)]),
            E::Sha256Mismatch { .. } => CmdError::new("checksum_mismatch", message),
            E::InsufficientDisk { needed, free } => CmdError::with(
                "disk_full",
                message,
                &[("needed", gigabytes(needed)), ("free", gigabytes(free))],
            ),
            E::Cancelled => CmdError::new("download_cancelled", message),
        }
    }
}

impl From<arcvox_core::SecretError> for CmdError {
    fn from(e: arcvox_core::SecretError) -> Self {
        use arcvox_core::SecretError as E;
        let message = e.to_string();
        match e {
            E::Unavailable(reason) => {
                CmdError::with("secret_store_unavailable", message, &[("reason", reason)])
            }
        }
    }
}

impl From<arcvox_core::settings_file::SettingsFileError> for CmdError {
    fn from(e: arcvox_core::settings_file::SettingsFileError) -> Self {
        use arcvox_core::settings_file::SettingsFileError as E;
        let message = e.to_string();
        match e {
            E::NotSettings(reason) => {
                CmdError::with("not_a_settings_file", message, &[("reason", reason)])
            }
            E::Newer(_) => CmdError::new("settings_file_newer", message),
            E::Section { name, reason } => CmdError::with(
                "settings_section_unreadable",
                message,
                &[("name", name), ("reason", reason)],
            ),
        }
    }
}

impl From<arcvox_core::backup::BackupError> for CmdError {
    fn from(e: arcvox_core::backup::BackupError) -> Self {
        use arcvox_core::backup::BackupError as E;
        let message = e.to_string();
        match e {
            E::Io { source, .. } => CmdError::with("io", message, &[("error", source.to_string())]),
            E::History(e) => e.into(),
            E::NotABackup(reason) => {
                CmdError::with("not_a_records_file", message, &[("reason", reason)])
            }
            E::Newer(_) => CmdError::new("records_file_newer", message),
            E::Busy => CmdError::new("busy", message),
            E::Json(reason) => {
                CmdError::with("records_file_broken", message, &[("reason", reason)])
            }
        }
    }
}

impl From<arcvox_core::engine::ReprocessError> for CmdError {
    fn from(e: arcvox_core::engine::ReprocessError) -> Self {
        use arcvox_core::engine::ReprocessError as E;
        let message = e.to_string();
        match e {
            E::NoRow => CmdError::new("history_row_gone", message),
            E::NoRecording => CmdError::new("recording_not_kept", message),
            E::History(e) => e.into(),
            // The interface words these by the dictation failure's own code.
            E::Failed { code, text } => CmdError::with(
                "dictation_failed",
                text,
                &[("code", code.name().to_string())],
            ),
        }
    }
}

impl From<crate::settings::SettingsError> for CmdError {
    fn from(e: crate::settings::SettingsError) -> Self {
        use crate::settings::SettingsError as E;
        let message = e.to_string();
        match e {
            E::Io { source, .. } => {
                CmdError::with("settings_io", message, &[("error", source.to_string())])
            }
        }
    }
}

impl From<crate::audio::AudioError> for CmdError {
    fn from(e: crate::audio::AudioError) -> Self {
        use crate::audio::AudioError as E;
        let message = e.to_string();
        match e {
            E::NoDevice => CmdError::new("no_input_device", message),
            E::UnsupportedFormat(format) => {
                CmdError::with("audio_format", message, &[("format", format)])
            }
            E::Other(reason) => CmdError::with("audio", message, &[("reason", reason)]),
        }
    }
}

impl From<bench::BenchError> for CmdError {
    fn from(e: bench::BenchError) -> Self {
        use bench::BenchError as E;
        let message = e.to_string();
        match e {
            E::NoSpeech => CmdError::new("no_speech", message),
            E::Audio(reason) => CmdError::with("audio", message, &[("reason", reason)]),
        }
    }
}

/// Bytes as gigabytes with one decimal, for the interface.
fn gigabytes(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / 1e9)
}

pub type CmdResult<T> = Result<T, CmdError>;

use arcvox_core::Engine;
use tauri::State;

use crate::app::AppState;

/// Clones the engine out of the lock rather than holding it, since every
/// caller needs to `.await` afterward and the lock must not cross that.
pub fn engine(state: &State<'_, AppState>) -> CmdResult<Engine> {
    state.engine.lock().unwrap().clone().ok_or_else(|| {
        state
            .engine_error
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| CmdError::new("engine_unavailable", "engine unavailable"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_errors_carry_a_code_and_their_placeholders() {
        let e: CmdError = arcvox_core::models::ModelError::InsufficientDisk {
            needed: 2_100_000_000,
            free: 400_000_000,
        }
        .into();
        assert_eq!(e.code, Some("disk_full"));
        assert_eq!(e.params["needed"], "2.1");
        assert_eq!(e.params["free"], "0.4");
        assert!(e.message.contains("free disk space"));

        let e: CmdError = arcvox_core::engine::EngineError::Folder("read only".into()).into();
        assert_eq!(e.code, Some("folder_unusable"));
        assert_eq!(e.params["reason"], "read only");

        let join = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { tokio::task::spawn_blocking(|| panic!("boom")).await })
            .unwrap_err();
        let e: CmdError = arcvox_core::engine::EngineError::Task(join).into();
        assert_eq!(e.code, Some("internal"));
        assert!(e.params["error"].contains("boom"));

        // Wrapped errors keep the inner code.
        let inner = arcvox_core::config::ConfigError::DuplicateId {
            kind: "engine",
            id: "x".into(),
        };
        let e: CmdError = arcvox_core::engine::EngineError::Store(inner.into()).into();
        assert_eq!(e.code, Some("config_duplicate_id"));
        assert_eq!(e.params["id"], "x");

        let e: CmdError = arcvox_core::engine::ReprocessError::Failed {
            code: arcvox_core::types::ErrorCode::ProviderTimeout,
            text: "Ollama: timed out".into(),
        }
        .into();
        assert_eq!(e.code, Some("dictation_failed"));
        assert_eq!(e.params["code"], "provider_timeout");
        assert_eq!(e.message, "Ollama: timed out");
    }

    #[test]
    fn a_plain_message_has_no_code() {
        let e: CmdError = "os_too_old".into();
        assert_eq!(e.code, None);
        assert_eq!(e.message, "os_too_old");
        let e = CmdError::plain(String::from("x"));
        assert!(e.params.is_empty());
    }
}
