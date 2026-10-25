//! Updates of the installed app: the release feed is looked at when the app
//! starts and every few hours, the menu-bar menu and the About page show the
//! result, and an update is installed on request.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_updater::{Update, UpdaterExt};

/// The event the settings window follows the state with.
pub const EVENT: &str = "update-status";
/// Time between two checks while the app runs.
const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Status {
    /// Not looked at yet.
    Idle,
    /// A build that is not updated: the development identity.
    Unsupported,
    Checking,
    /// The installed version is the newest; seconds since the epoch.
    Current {
        checked_at: u64,
    },
    /// `notes` is the release description.
    Available {
        version: String,
        notes: Option<String>,
    },
    Installing {
        version: String,
        received: u64,
        total: Option<u64>,
    },
    Failed {
        message: String,
    },
}

pub struct Updates {
    status: Mutex<Status>,
    /// The update the last check found, ready to install.
    found: Mutex<Option<Update>>,
    installing: AtomicBool,
}

impl Default for Updates {
    fn default() -> Self {
        Self {
            status: Mutex::new(Status::Idle),
            found: Mutex::new(None),
            installing: AtomicBool::new(false),
        }
    }
}

impl Updates {
    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn publish<R: Runtime>(app: &AppHandle<R>, status: Status) {
    let Some(updates) = app.try_state::<Updates>() else {
        return;
    };
    *updates.status.lock().unwrap() = status.clone();
    let item = match &status {
        Status::Unsupported => None,
        Status::Available { version, .. } | Status::Installing { version, .. } => {
            Some(Some(version.clone()))
        }
        _ => Some(None),
    };
    if let Some(state) = app.try_state::<crate::app::AppState>() {
        state.ui.tray().set_update(item);
    }
    let _ = app.emit_to(crate::ui::window_state::SETTINGS_WINDOW, EVENT, status);
}

/// Looks at the feed now and then every `INTERVAL`. The development
/// identity is never updated: its bundle is another app than the release.
pub fn start(app: &AppHandle<tauri::Wry>) {
    app.manage(Updates::default());
    if crate::identity::is_dev(app.package_info()) {
        publish(app, Status::Unsupported);
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            check(&app).await;
            tokio::time::sleep(INTERVAL).await;
        }
    });
}

/// Looks at the release feed. Returns the state it left.
pub async fn check<R: Runtime>(app: &AppHandle<R>) -> Status {
    let Some(updates) = app.try_state::<Updates>() else {
        return Status::Idle;
    };
    let before = updates.status();
    if matches!(before, Status::Unsupported | Status::Checking)
        || updates.installing.load(Ordering::SeqCst)
    {
        return before;
    }
    publish(app, Status::Checking);
    let result = match app.updater() {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    let status = match result {
        Ok(Some(update)) => {
            let status = Status::Available {
                version: update.version.clone(),
                notes: update.body.clone(),
            };
            *updates.found.lock().unwrap() = Some(update);
            status
        }
        Ok(None) => {
            *updates.found.lock().unwrap() = None;
            Status::Current { checked_at: now() }
        }
        Err(e) => {
            tracing::warn!(error = %e, "update check failed");
            // An update found earlier is still there to install.
            match before {
                Status::Available { .. } => before,
                _ => Status::Failed {
                    message: e.to_string(),
                },
            }
        }
    };
    publish(app, status.clone());
    status
}

/// Downloads the update the last check found, checks its signature,
/// replaces the app and starts it again.
pub async fn install(app: &AppHandle<tauri::Wry>) -> Result<(), String> {
    let updates = app.state::<Updates>();
    let Some(update) = updates.found.lock().unwrap().clone() else {
        return Err("no update to install".into());
    };
    if updates.installing.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let version = update.version.clone();
    let mut received = 0u64;
    let mut shown = 0u64;
    let progress = |chunk: usize, total: Option<u64>| {
        received += chunk as u64;
        // A new state for every whole percent, not for every chunk.
        let step = total.map_or(1 << 20, |t| (t / 100).max(1));
        if received - shown >= step {
            shown = received;
            publish(
                app,
                Status::Installing {
                    version: version.clone(),
                    received,
                    total,
                },
            );
        }
    };
    publish(
        app,
        Status::Installing {
            version: version.clone(),
            received: 0,
            total: None,
        },
    );
    match update.download_and_install(progress, || {}).await {
        Ok(()) => {
            tracing::info!(version = %version, "update installed, restarting");
            app.restart();
        }
        Err(e) => {
            tracing::warn!(error = %e, "update not installed");
            updates.installing.store(false, Ordering::SeqCst);
            // The update stays found: it can be tried again; the caller
            // shows the error.
            publish(
                app,
                Status::Available {
                    version,
                    notes: update.body.clone(),
                },
            );
            Err(e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_reaches_the_page_tagged_by_its_name() {
        let s = serde_json::to_value(Status::Available {
            version: "0.2.0".into(),
            notes: None,
        })
        .unwrap();
        assert_eq!(
            s,
            serde_json::json!({"state": "available", "version": "0.2.0", "notes": null})
        );
        let s = serde_json::to_value(Status::Current { checked_at: 5 }).unwrap();
        assert_eq!(s, serde_json::json!({"state": "current", "checked_at": 5}));
        assert_eq!(
            serde_json::to_value(Status::Unsupported).unwrap(),
            serde_json::json!({"state": "unsupported"})
        );
    }
}
