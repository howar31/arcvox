//! Updates of the installed app: the release feed is looked at when the app
//! starts and every few hours, the menu-bar menu and the About page show the
//! result, and an update is installed on request. Each check also reads the
//! published releases, whose descriptions the What's new page shows.

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
/// The published releases, newest first; release candidates are left out.
const RELEASES_URL: &str = "https://api.github.com/repos/howar31/arcvox/releases?per_page=20";

/// A published release and its description.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Release {
    pub version: String,
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Status {
    /// Not looked at yet.
    Idle,
    Checking,
    /// The installed version is the newest; seconds since the epoch.
    Current {
        checked_at: u64,
    },
    /// `notes` is the release description; `installable` is false for a
    /// copy the app cannot replace (see `installable`).
    Available {
        version: String,
        notes: Option<String>,
        installable: bool,
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
    /// What the What's new page shows: see `whats_new`.
    releases: Mutex<Vec<Release>>,
}

impl Default for Updates {
    fn default() -> Self {
        Self {
            status: Mutex::new(Status::Idle),
            found: Mutex::new(None),
            installing: AtomicBool::new(false),
            releases: Mutex::new(Vec::new()),
        }
    }
}

impl Updates {
    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    pub fn releases(&self) -> Vec<Release> {
        self.releases.lock().unwrap().clone()
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

/// Looks at the feed now and then every `INTERVAL`, in every build; only
/// an installable copy installs what it finds.
pub fn start(app: &AppHandle<tauri::Wry>) {
    app.manage(Updates::default());
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
    if matches!(before, Status::Checking) || updates.installing.load(Ordering::SeqCst) {
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
                installable: installable(),
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
    // Read before the state goes out: the pages read the releases again
    // whenever a state arrives.
    read_releases(app).await;
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
    if !installable() {
        return Err("this copy cannot be replaced".into());
    }
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
            #[cfg(target_os = "macos")]
            {
                if let Err(e) = relaunch_after_exit() {
                    tracing::warn!(error = %e, "the app will not start again by itself");
                }
                // The usual exit, the same as Quit in the menu-bar menu.
                app.exit(0);
                Ok(())
            }
            #[cfg(not(target_os = "macos"))]
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
                    installable: true,
                },
            );
            Err(e.to_string())
        }
    }
}

/// Whether this copy can replace itself: an app in /Applications whose
/// folder can be written. Another copy (the development build, one opened
/// from the disk image) gets the release page instead.
fn installable() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Some(bundle) = exe
        .ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
    else {
        return false;
    };
    let Some(folder) = bundle.parent() else {
        return false;
    };
    if !bundle.starts_with("/Applications") {
        return false;
    }
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = std::ffi::CString::new(folder.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `path` is a valid NUL-terminated string for the call.
    unsafe { libc::access(path.as_ptr(), libc::W_OK) == 0 }
}

/// Keeps the earlier list when the releases cannot be read.
async fn read_releases<R: Runtime>(app: &AppHandle<R>) {
    let Some(updates) = app.try_state::<Updates>() else {
        return;
    };
    match fetch_releases().await {
        Ok(all) => {
            *updates.releases.lock().unwrap() = whats_new(all, &app.package_info().version);
        }
        Err(e) => tracing::warn!(error = %e, "releases not read"),
    }
}

async fn fetch_releases() -> Result<Vec<(semver::Version, Release)>, reqwest::Error> {
    let body = reqwest::Client::new()
        .get(RELEASES_URL)
        .header(reqwest::header::USER_AGENT, "Arcvox")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .timeout(Duration::from_secs(15))
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    Ok(parse_releases(&body))
}

/// The published releases in a GitHub releases listing; drafts, release
/// candidates and tags that are not a version are left out.
fn parse_releases(body: &[u8]) -> Vec<(semver::Version, Release)> {
    #[derive(serde::Deserialize)]
    struct Item {
        tag_name: String,
        body: Option<String>,
        #[serde(default)]
        draft: bool,
        #[serde(default)]
        prerelease: bool,
    }
    let items: Vec<Item> = serde_json::from_slice(body).unwrap_or_default();
    items
        .into_iter()
        .filter(|i| !i.draft && !i.prerelease)
        .filter_map(|i| {
            let tag = i.tag_name.trim_start_matches('v');
            let version = semver::Version::parse(tag).ok()?;
            Some((
                version,
                Release {
                    version: tag.to_string(),
                    notes: i.body.unwrap_or_default(),
                },
            ))
        })
        .collect()
}

/// The releases newer than the installed version, newest first; when there
/// is none, the newest release alone.
fn whats_new(
    mut all: Vec<(semver::Version, Release)>,
    installed: &semver::Version,
) -> Vec<Release> {
    all.sort_by(|a, b| b.0.cmp(&a.0));
    let newer: Vec<Release> = all
        .iter()
        .filter(|(v, _)| v > installed)
        .map(|(_, r)| r.clone())
        .collect();
    if newer.is_empty() {
        all.into_iter().take(1).map(|(_, r)| r).collect()
    } else {
        newer
    }
}

/// Starts the app again once this process has ended: a detached shell waits
/// for the process id to go away, then opens the bundle, which by then holds
/// the new version. The app's exit ends the process with `_exit`, so a
/// relaunch has to come from outside the process.
#[cfg(target_os = "macos")]
fn relaunch_after_exit() -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    let exe = std::env::current_exe()?;
    let bundle = exe
        .ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .ok_or_else(|| std::io::Error::other("not inside an app bundle"))?;
    // The bundle path goes in as $0, so no quoting is needed.
    let script = format!(
        "while /bin/kill -0 {} 2>/dev/null; do /bin/sleep 0.1; done; /usr/bin/open \"$0\"",
        std::process::id()
    );
    Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .arg(bundle)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(items: &[(&str, bool, bool)]) -> Vec<u8> {
        let items: Vec<_> = items
            .iter()
            .map(|(tag, draft, pre)| {
                serde_json::json!({"tag_name": tag, "body": format!("notes {tag}"), "draft": draft, "prerelease": pre})
            })
            .collect();
        serde_json::to_vec(&items).unwrap()
    }

    fn versions(r: &[Release]) -> Vec<&str> {
        r.iter().map(|r| r.version.as_str()).collect()
    }

    #[test]
    fn drafts_candidates_and_other_tags_are_left_out() {
        let all = parse_releases(&listing(&[
            ("v0.3.0", true, false),
            ("v0.2.0-rc.1", false, true),
            ("nightly", false, false),
            ("v0.1.0", false, false),
        ]));
        let releases: Vec<_> = all.into_iter().map(|(_, r)| r).collect();
        assert_eq!(versions(&releases), ["0.1.0"]);
        assert_eq!(releases[0].notes, "notes v0.1.0");
        assert!(parse_releases(b"{\"message\": \"rate limited\"}").is_empty());
    }

    #[test]
    fn whats_new_lists_the_newer_releases_newest_first() {
        let all = parse_releases(&listing(&[
            ("v0.1.1", false, false),
            ("v0.3.0", false, false),
            ("v0.2.0", false, false),
            ("v0.1.0", false, false),
        ]));
        let installed = semver::Version::parse("0.1.1").unwrap();
        assert_eq!(versions(&whats_new(all, &installed)), ["0.3.0", "0.2.0"]);
    }

    #[test]
    fn whats_new_without_a_newer_release_is_the_newest_one() {
        let all = parse_releases(&listing(&[
            ("v0.1.0", false, false),
            ("v0.2.0", false, false),
        ]));
        let installed = semver::Version::parse("0.2.0").unwrap();
        assert_eq!(versions(&whats_new(all.clone(), &installed)), ["0.2.0"]);
        // A release candidate of the newest version is older than it.
        let candidate = semver::Version::parse("0.2.0-rc.2").unwrap();
        assert_eq!(versions(&whats_new(all, &candidate)), ["0.2.0"]);
        assert!(whats_new(Vec::new(), &installed).is_empty());
    }

    #[test]
    fn the_state_reaches_the_page_tagged_by_its_name() {
        let s = serde_json::to_value(Status::Available {
            version: "0.2.0".into(),
            notes: None,
            installable: false,
        })
        .unwrap();
        assert_eq!(
            s,
            serde_json::json!({"state": "available", "version": "0.2.0", "notes": null, "installable": false})
        );
        let s = serde_json::to_value(Status::Current { checked_at: 5 }).unwrap();
        assert_eq!(s, serde_json::json!({"state": "current", "checked_at": 5}));
    }
}
