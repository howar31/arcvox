use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State, Wry};

use super::{CmdError, CmdResult};
use crate::app::AppState;
use crate::settings::{WidgetFade, WidgetIdle, WidgetSize};
use crate::ui::preview::{Look, Sample, Scene};

#[tauri::command]
pub fn ui_language(state: State<'_, AppState>) -> String {
    state.language.lock().unwrap().tag().to_string()
}

#[tauri::command]
pub fn widget_copy(state: State<'_, AppState>, text: String) -> CmdResult<()> {
    if !state.platform.clipboard_write_text(&text) {
        return Err(CmdError::new(
            "clipboard_failed",
            "the clipboard refused the text",
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn widget_dismiss(state: State<'_, AppState>) {
    state.ui.dismiss();
}

/// The pointer went down on the widget: it follows the pointer until release.
#[tauri::command]
pub fn widget_drag_start(state: State<'_, AppState>) {
    state.ui.clone().drag_start();
}

/// Shows the widget so it can be dragged into place, or ends that.
#[tauri::command]
pub fn widget_placing(state: State<'_, AppState>, on: bool) {
    state.ui.set_placing(on);
}

/// A state of the widget and the settings to show it with, sent by the
/// settings window while it keeps the widget on screen.
#[derive(Deserialize)]
pub struct WidgetPreview {
    scene: Scene,
    size: WidgetSize,
    idle: WidgetIdle,
    fade: WidgetFade,
    /// Sample texts in the interface language; none when the recognised
    /// text is not shown while processing.
    raw: Option<String>,
    refined: Option<String>,
    detail: Option<String>,
    /// Begin the scene again even when it is the one shown.
    play: bool,
    /// Show a resting widget faint without waiting.
    faint: bool,
}

/// Shows the widget in a chosen state with settings that are not saved.
#[tauri::command]
pub fn widget_preview(state: State<'_, AppState>, preview: WidgetPreview) {
    state.ui.preview(
        preview.scene,
        Look {
            scale: preview.size.scale(),
            idle: preview.idle,
            fade: preview.fade,
        },
        Sample {
            raw: preview.raw,
            refined: preview.refined,
            detail: preview.detail,
        },
        preview.play,
        preview.faint,
    );
}

/// The factor of the widget size the user chose.
#[tauri::command]
pub fn widget_scale(state: State<'_, AppState>) -> f64 {
    state.ui.scale()
}

/// Whether the widget is kept on screen to be positioned.
#[tauri::command]
pub fn widget_is_placing(state: State<'_, AppState>) -> bool {
    state.ui.is_placing()
}

/// What the widget shows while no dictation goes on.
#[tauri::command]
pub fn widget_idle(state: State<'_, AppState>) -> &'static str {
    state.ui.idle().name()
}

/// Whether and how a resting widget turns faint.
#[tauri::command]
pub fn widget_fade(state: State<'_, AppState>) -> crate::settings::WidgetFade {
    state.ui.fade()
}

/// Opens the menu of the menu-bar icon at the pointer, over the widget.
#[tauri::command]
pub fn widget_menu(state: State<'_, AppState>) {
    state.ui.popup_menu();
}

/// What the app is doing now, by the name the state events carry.
#[tauri::command]
pub fn dictation_state(state: State<'_, AppState>) -> &'static str {
    state.ui.state_label()
}

/// The widget measured the length its labels need in each shape.
#[tauri::command]
pub fn widget_measure(state: State<'_, AppState>, wide: f64, tall: f64) {
    state.ui.measured(wide, tall);
}

/// The panel next to the widget has content of this size, or none.
#[tauri::command]
pub fn panel_fit(state: State<'_, AppState>, width: Option<f64>, height: Option<f64>) {
    state.ui.panel_fit(width.zip(height));
}

#[tauri::command]
pub fn widget_reset_position(state: State<'_, AppState>) {
    state.ui.reset_positions();
}

#[tauri::command]
pub fn system_open_settings_window(app: AppHandle<Wry>, page: Option<String>) {
    crate::ui::tray::open_settings(&app, page.as_deref());
}

#[derive(Serialize)]
pub struct Permissions {
    pub accessibility: bool,
    /// The system lets the app watch the keyboard but not act on it.
    pub keyboard_watch_only: bool,
    pub microphone: crate::platform::MicAccess,
}

#[derive(Serialize)]
pub struct RunningApp {
    pub bundle_id: String,
    pub name: String,
}

#[derive(Serialize)]
pub struct SystemInfo {
    /// The app name of this build ("Arcvox", or "Arcvox Dev").
    pub name: String,
    pub version: String,
    /// A development build, shown as such beside the version.
    pub dev: bool,
    pub config_dir: String,
    pub data_dir: String,
    pub cache_dir: String,
    pub recordings_dir: String,
    /// The machine's memory, for the model entries that state what they need.
    pub memory_bytes: u64,
}

/// The machine's physical memory in bytes; 0 when it cannot be read.
pub fn memory_bytes() -> u64 {
    #[cfg(target_os = "macos")]
    {
        let mut value: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        // SAFETY: `hw.memsize` is a 64-bit value and `len` says how much
        // room the out pointer has.
        let rc = unsafe {
            libc::sysctlbyname(
                c"hw.memsize".as_ptr(),
                &mut value as *mut u64 as *mut libc::c_void,
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc == 0 {
            value
        } else {
            0
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        0
    }
}

#[tauri::command]
pub fn system_list_microphones(state: State<'_, AppState>) -> Vec<String> {
    state.audio.list_devices()
}

/// The connected displays, by the names the widget's positions are saved under.
#[tauri::command(async)]
pub fn system_list_displays(app: tauri::AppHandle) -> Vec<crate::ui::widget::DisplayName> {
    // The system's names are read on the main thread.
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = app.clone();
    let sent = app.run_on_main_thread(move || {
        let _ = tx.send(crate::ui::widget::display_names(&handle));
    });
    match sent {
        Ok(()) => rx.recv().unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

#[tauri::command]
pub fn system_permissions(state: State<'_, AppState>) -> Permissions {
    Permissions {
        accessibility: state.platform.accessibility_trusted(false),
        keyboard_watch_only: state.platform.keyboard_watch_only(),
        microphone: state.platform.microphone_access(),
    }
}

/// The settings pages that have a problem to show, read afresh.
#[tauri::command]
pub fn system_problem_pages(state: State<'_, AppState>) -> Vec<crate::ui::tray::PageProblem> {
    state.refresh_problems();
    state.ui.tray().problem_pages()
}

/// Asks the system for the microphone; answers once the user has chosen.
#[tauri::command(async)]
pub fn system_request_microphone(state: State<'_, AppState>) -> crate::platform::MicAccess {
    state.platform.request_microphone()
}

#[tauri::command]
pub fn system_request_accessibility(state: State<'_, AppState>) -> bool {
    state.platform.accessibility_trusted(true)
}

#[tauri::command]
pub fn system_open_privacy_pane(pane: String) -> CmdResult<()> {
    // Apple Intelligence is switched on in its own pane, not a privacy one.
    if pane == "intelligence" {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.Siri-Settings.extension")
            .spawn()?;
        return Ok(());
    }
    let anchor = match pane.as_str() {
        "accessibility" => "Privacy_Accessibility",
        "microphone" => "Privacy_Microphone",
        "files" => "Privacy_FilesAndFolders",
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
        out.sort_by(|a, b| a.bundle_id.cmp(&b.bundle_id));
        out.dedup_by(|a, b| a.bundle_id == b.bundle_id);
        out.sort_by_key(|a| a.name.to_lowercase());
        out
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

/// Display name of the installed application with this bundle id, or `None`
/// when no such application is installed (or off macOS).
pub fn app_name(bundle_id: &str) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSFileManager, NSString};
        if bundle_id.is_empty() {
            return None;
        }
        let ws = objc2_app_kit::NSWorkspace::sharedWorkspace();
        let url = ws.URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))?;
        let path = url.path()?;
        let display = NSFileManager::defaultManager()
            .displayNameAtPath(&path)
            .to_string();
        // The display name carries the extension when Finder shows all extensions.
        let name = display.strip_suffix(".app").unwrap_or(&display).trim();
        (!name.is_empty()).then(|| name.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = bundle_id;
        None
    }
}

/// The icon of the installed application with this bundle id as a PNG
/// data URL, from the icon's representation nearest to `ICON_PIXELS`.
pub fn app_icon(bundle_id: &str) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        use base64::Engine;
        use objc2::AnyThread;
        use objc2_app_kit::{
            NSBitmapImageFileType, NSBitmapImageRep, NSCalibratedRGBColorSpace,
            NSCompositingOperation, NSGraphicsContext,
        };
        use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};
        if bundle_id.is_empty() {
            return None;
        }
        let ws = objc2_app_kit::NSWorkspace::sharedWorkspace();
        let url = ws.URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))?;
        let path = url.path()?;
        let icon = ws.iconForFile(&path);
        // The icon is drawn once at the size the lists show, into a bitmap
        // of its own; its stored representations are left undrawn.
        // SAFETY: a fresh bitmap of the given geometry with its own
        // storage (null planes), 8 bits per sample, RGBA.
        let bitmap = unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                NSBitmapImageRep::alloc(),
                std::ptr::null_mut(),
                ICON_PIXELS,
                ICON_PIXELS,
                8,
                4,
                true,
                false,
                NSCalibratedRGBColorSpace,
                0,
                0,
            )
        }?;
        let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap)?;
        let rect = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(ICON_PIXELS as f64, ICON_PIXELS as f64),
        );
        NSGraphicsContext::saveGraphicsState_class();
        NSGraphicsContext::setCurrentContext(Some(&context));
        icon.drawInRect_fromRect_operation_fraction(
            rect,
            NSRect::ZERO,
            NSCompositingOperation::SourceOver,
            1.0,
        );
        context.flushGraphics();
        NSGraphicsContext::restoreGraphicsState_class();
        // SAFETY: the type is a PNG and the properties are an empty
        // dictionary, both plain values the method documents.
        let png = unsafe {
            bitmap.representationUsingType_properties(
                NSBitmapImageFileType::PNG,
                &NSDictionary::new(),
            )
        }?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(png.to_vec());
        Some(format!("data:image/png;base64,{encoded}"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = bundle_id;
        None
    }
}

/// Icon size drawn: 2x of the largest size the lists show it at.
#[cfg(target_os = "macos")]
const ICON_PIXELS: isize = 64;

#[tauri::command(async)]
pub fn system_app_icons(bundle_ids: Vec<String>) -> HashMap<String, Option<String>> {
    bundle_ids
        .into_iter()
        .map(|id| {
            let icon = app_icon(&id);
            (id, icon)
        })
        .collect()
}

#[tauri::command(async)]
pub fn system_app_names(bundle_ids: Vec<String>) -> HashMap<String, Option<String>> {
    bundle_ids
        .into_iter()
        .map(|id| {
            let name = app_name(&id);
            (id, name)
        })
        .collect()
}

#[tauri::command]
pub fn system_info(app: AppHandle<Wry>, state: State<'_, AppState>) -> SystemInfo {
    SystemInfo {
        name: crate::identity::name(app.package_info()),
        version: crate::identity::version(app.package_info()),
        dev: crate::identity::is_dev(app.package_info()),
        config_dir: state.dirs.config_dir.display().to_string(),
        data_dir: state.dirs.data_dir.display().to_string(),
        cache_dir: state.dirs.cache_dir.display().to_string(),
        recordings_dir: state
            .engine
            .lock()
            .unwrap()
            .as_ref()
            .map(|e| e.recordings_dir())
            .unwrap_or_else(|| state.dirs.recordings_dir())
            .display()
            .to_string(),
        memory_bytes: memory_bytes(),
    }
}

/// The pages the About page links to, by name; the page never passes a URL.
fn link_url(link: &str) -> Option<&'static str> {
    Some(match link {
        "sponsor" => "https://donate.howar31.com/",
        "source" => "https://github.com/howar31/arcvox",
        "issue" => "https://github.com/howar31/arcvox/issues/new/choose",
        "website" => "https://arcvox.howar31.com/",
        _ => return None,
    })
}

#[tauri::command]
pub fn system_open_link(link: String) -> CmdResult<()> {
    let url = link_url(&link).ok_or_else(|| {
        CmdError::with(
            "unknown_link",
            format!("unknown link {link}"),
            &[("link", link.clone())],
        )
    })?;
    std::process::Command::new("open").arg(url).spawn()?;
    Ok(())
}

#[tauri::command]
pub fn system_reveal(path: String) -> CmdResult<()> {
    std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn()?;
    Ok(())
}

/// Opens a folder in the file manager.
#[tauri::command]
pub fn system_open_folder(path: String) -> CmdResult<()> {
    std::process::Command::new("open").arg(path).spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn app_name_resolves_an_installed_app() {
        assert!(app_name("com.apple.finder").is_some_and(|n| !n.is_empty()));
    }

    #[test]
    fn app_name_is_none_for_an_unknown_bundle_id() {
        assert_eq!(app_name("invalid.bundle.id.zz"), None);
    }

    #[test]
    fn app_names_keys_every_requested_id() {
        let names = system_app_names(vec!["invalid.bundle.id.zz".into()]);
        assert_eq!(names.get("invalid.bundle.id.zz"), Some(&None));
    }
}

/// An error the web view caught (an uncaught exception or a rejected
/// promise): written to the app's log, since a broken page shows nothing
/// of its own. `window` names the page (settings, widget, panel).
#[tauri::command]
pub fn webview_error(
    window: String,
    message: String,
    source: Option<String>,
    stack: Option<String>,
) {
    tracing::error!(
        window,
        source = source.unwrap_or_default(),
        stack = stack.unwrap_or_default(),
        "webview error: {message}"
    );
}
