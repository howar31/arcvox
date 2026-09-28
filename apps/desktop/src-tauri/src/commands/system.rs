use serde::Serialize;
use tauri::{AppHandle, State, Wry};

use super::CmdResult;
use crate::app::AppState;

#[tauri::command]
pub fn ui_language(state: State<'_, AppState>) -> String {
    state.language.lock().unwrap().tag().to_string()
}

#[tauri::command]
pub fn widget_copy(state: State<'_, AppState>, text: String) -> CmdResult<()> {
    state.platform.clipboard_write_text(&text);
    Ok(())
}

#[tauri::command]
pub fn system_open_settings_window(app: AppHandle<Wry>, page: Option<String>) {
    crate::ui::tray::open_settings(&app, page.as_deref());
}

#[derive(Serialize)]
pub struct Permissions {
    pub accessibility: bool,
}

#[derive(Serialize)]
pub struct RunningApp {
    pub bundle_id: String,
    pub name: String,
}

#[derive(Serialize)]
pub struct SystemInfo {
    pub version: String,
    pub config_dir: String,
    pub data_dir: String,
    pub cache_dir: String,
}

#[tauri::command]
pub fn system_list_microphones(state: State<'_, AppState>) -> Vec<String> {
    state.audio.list_devices()
}

#[tauri::command]
pub fn system_permissions(state: State<'_, AppState>) -> Permissions {
    Permissions {
        accessibility: state.platform.accessibility_trusted(false),
    }
}

#[tauri::command]
pub fn system_request_accessibility(state: State<'_, AppState>) -> bool {
    state.platform.accessibility_trusted(true)
}

#[tauri::command]
pub fn system_open_privacy_pane(pane: String) -> CmdResult<()> {
    let anchor = match pane.as_str() {
        "accessibility" => "Privacy_Accessibility",
        "microphone" => "Privacy_Microphone",
        _ => "Privacy",
    };
    std::process::Command::new("open")
        .arg(format!(
            "x-apple.systempreferences:com.apple.preference.security?{anchor}"
        ))
        .spawn()?;
    Ok(())
}

#[tauri::command]
pub fn system_running_apps() -> Vec<RunningApp> {
    #[cfg(target_os = "macos")]
    {
        let ws = objc2_app_kit::NSWorkspace::sharedWorkspace();
        let apps = ws.runningApplications();
        let mut out: Vec<RunningApp> = apps
            .iter()
            .filter_map(|a| {
                let bundle_id = a.bundleIdentifier()?.to_string();
                let name = a.localizedName().map(|n| n.to_string()).unwrap_or_default();
                (a.activationPolicy() == objc2_app_kit::NSApplicationActivationPolicy::Regular)
                    .then_some(RunningApp { bundle_id, name })
            })
            .collect();
        out.sort_by_key(|a| a.name.to_lowercase());
        out.dedup_by(|a, b| a.bundle_id == b.bundle_id);
        out
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

#[tauri::command]
pub fn system_info(app: AppHandle<Wry>, state: State<'_, AppState>) -> SystemInfo {
    SystemInfo {
        version: app.package_info().version.to_string(),
        config_dir: state.dirs.config_dir.display().to_string(),
        data_dir: state.dirs.data_dir.display().to_string(),
        cache_dir: state.dirs.cache_dir.display().to_string(),
    }
}

#[tauri::command]
pub fn system_reveal(path: String) -> CmdResult<()> {
    std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn()?;
    Ok(())
}
