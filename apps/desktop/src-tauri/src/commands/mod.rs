//! Tauri IPC. Every command is a thin wrapper over a plain function so the
//! logic is testable without Tauri.

pub mod client;
pub mod config;
pub mod dictionary;
pub mod history;
pub mod models;
pub mod profiles;
pub mod prompt;
pub mod providers;
pub mod secrets;
pub mod system;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CmdError {
    pub message: String,
}

impl<E: std::fmt::Display> From<E> for CmdError {
    fn from(e: E) -> Self {
        CmdError {
            message: e.to_string(),
        }
    }
}

pub type CmdResult<T> = Result<T, CmdError>;

use arcvox_core::Engine;
use tauri::State;

use crate::app::AppState;

/// Clones the engine out of the lock rather than holding it, since every
/// caller needs to `.await` afterward and the lock must not cross that.
pub fn engine(state: &State<'_, AppState>) -> CmdResult<Engine> {
    state
        .engine
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| CmdError {
            message: state
                .engine_error
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| "engine unavailable".into()),
        })
}
