//! The app's own updates, as the About page shows them.

use tauri::{AppHandle, State, Wry};

use super::{CmdError, CmdResult};
use crate::update::{Status, Updates};

#[tauri::command]
pub fn update_status(updates: State<'_, Updates>) -> Status {
    updates.status()
}

#[tauri::command]
pub async fn update_check(app: AppHandle<Wry>) -> Status {
    crate::update::check(&app).await
}

/// Installs the update found and starts the app again; returns only when
/// that failed.
#[tauri::command]
pub async fn update_install(app: AppHandle<Wry>) -> CmdResult<()> {
    crate::update::install(&app).await.map_err(CmdError::from)
}
