//! Tauri IPC. Every command is a thin wrapper over a plain function so the
//! logic is testable without Tauri.

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
