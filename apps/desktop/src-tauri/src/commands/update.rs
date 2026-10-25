//! The app's own updates, as the About page shows them.

use tauri::{AppHandle, State, Wry};

use super::{CmdError, CmdResult};
use crate::update::{Release, Status, Updates};

#[tauri::command]
pub fn update_status(updates: State<'_, Updates>) -> Status {
    updates.status()
}

/// The releases the What's new page shows, newest first.
#[tauri::command]
pub fn update_releases(updates: State<'_, Updates>) -> Vec<Release> {
    updates.releases()
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
