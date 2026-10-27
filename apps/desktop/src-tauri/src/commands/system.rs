use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State, Wry};

use super::{CmdError, CmdResult};
use crate::app::AppState;
use crate::i18n::Language;
use crate::settings::{UiLanguage, WidgetFade, WidgetIdle, WidgetSize};
use crate::ui::preview::{Look, Sample, Scene};

#[tauri::command]
pub fn ui_language(state: State<'_, AppState>) -> String {
    state.language.lock().unwrap().tag().to_string()
}

/// The interface language that "follow the system" resolves to, whatever is set.
#[tauri::command]
pub fn ui_language_system() -> String {
    Language::from_setting(
        UiLanguage::System,
        crate::i18n::system_preferred_language().as_deref(),
    )
    .tag()
    .to_string()
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

/// Whether the default profile can dictate now.
#[tauri::command]
pub fn system_dictation_ready(state: State<'_, AppState>) -> bool {
    let engine = state.engine.lock().unwrap().clone();
    engine.is_some_and(|e| {
        crate::problems::default_profile_ready(&e.config().current(), |cfg| e.can_run(cfg))
    })
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
pub fn system_info(app: AppHandle<Wry>) -> SystemInfo {
    SystemInfo {
        name: crate::identity::name(app.package_info()),
        version: crate::identity::version(app.package_info()),
        dev: crate::identity::is_dev(app.package_info()),
        memory_bytes: memory_bytes(),
    }
}

/// A place the app keeps files in, for the report page.
#[derive(Serialize)]
pub struct Location {
    /// `app`, `data` (only where it is not the app folder), `recordings`,
    /// `models`, `backup`, `log` or `cache`.
    pub kind: &'static str,
    pub path: String,
    /// For the folders the user can choose: whether it is the default one.
    pub is_default: Option<bool>,
    pub exists: bool,
}

#[derive(Serialize)]
pub struct Diagnostics {
    pub text: String,
    /// What the bug form is opened with.
    pub issue: crate::diagnostics::IssueFields,
    pub locations: Vec<Location>,
}

/// The report page's snapshot and the places the app keeps files in;
/// collected when the page opens, nothing runs in the background.
#[tauri::command(async)]
pub fn system_diagnostics(app: AppHandle<Wry>, state: State<'_, AppState>) -> Diagnostics {
    let engine = state.engine.lock().unwrap().clone();
    let client = crate::settings::ClientSettings::load(&state.dirs.config_dir);
    let config = match &engine {
        Some(e) => (*e.config().current()).clone(),
        None => arcvox_core::config::ConfigStore::read_only(&state.dirs)
            .unwrap_or_else(|_| arcvox_core::config::Config::first_run()),
    };
    let now = arcvox_core::history::now_unix();
    let (outcomes, last_failure) = match &engine {
        Some(e) => {
            let h = e.history();
            (
                h.outcomes(now - 7 * 86_400).ok(),
                h.last_failure().ok().flatten(),
            )
        }
        None => (None, None),
    };
    let microphone_connected = match client.microphone_choice() {
        crate::settings::MicrophoneChoice::Named(name) => {
            Some(state.audio.list_devices().contains(&name))
        }
        _ => None,
    };
    state.refresh_problems();
    let mut problems: Vec<&'static str> =
        state.ui.tray().notices().iter().map(|n| n.kind()).collect();
    problems.dedup();
    let backup_folder =
        crate::backup_auto::target(client.backup.folder.as_deref(), &state.dirs.backup_dir);
    let machine = crate::diagnostics::Machine {
        version: crate::identity::version(app.package_info()),
        dev: crate::identity::is_dev(app.package_info()),
        os: crate::diagnostics::sysctl_string(c"kern.osproductversion"),
        os_build: crate::diagnostics::sysctl_string(c"kern.osversion"),
        arch: std::env::consts::ARCH,
        chip: crate::diagnostics::sysctl_string(c"machdep.cpu.brand_string"),
        memory_bytes: memory_bytes(),
        interface: state.language.lock().unwrap().tag(),
        system_language: crate::i18n::system_preferred_language(),
        install: crate::diagnostics::install_place(),
    };
    let live = crate::diagnostics::Live {
        permissions: system_permissions(state.clone()),
        microphone_connected,
        displays: system_list_displays(app.clone()).len(),
        engine_error: state.engine_error.lock().unwrap().as_ref().map(|e| e.code),
        problems,
        backup_folder: crate::diagnostics::folder_state_name(state.backup.folder()),
        outcomes,
        last_failure,
        now,
    };
    let text = crate::diagnostics::report(&machine, &live, &config, &client);
    let issue = crate::diagnostics::issue_fields(&machine, &config, text.clone());

    let dirs = &state.dirs;
    let place = |kind: &'static str, path: std::path::PathBuf, is_default: Option<bool>| Location {
        kind,
        exists: path.exists(),
        path: path.display().to_string(),
        is_default,
    };
    let mut locations = vec![place("app", dirs.config_dir.clone(), None)];
    if dirs.data_dir != dirs.config_dir {
        locations.push(place("data", dirs.data_dir.clone(), None));
    }
    let recordings = engine
        .as_ref()
        .map_or_else(|| dirs.recordings_dir(), |e| e.recordings_dir());
    locations.push(place(
        "recordings",
        recordings,
        Some(config.general.recordings_dir.is_none()),
    ));
    locations.push(place(
        "models",
        state.models.models_dir(),
        Some(config.general.models_dir.is_none()),
    ));
    locations.push(place(
        "backup",
        backup_folder,
        Some(client.backup.folder.is_none()),
    ));
    locations.push(place("log", newest_log(&dirs.data_dir), None));
    locations.push(place("cache", dirs.cache_dir.clone(), None));
    Diagnostics {
        text,
        issue,
        locations,
    }
}

/// The log file written last; the day's name when there is none yet.
fn newest_log(dir: &std::path::Path) -> std::path::PathBuf {
    std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("arcvox.") && n.ends_with(".log"))
        })
        .max()
        .unwrap_or_else(|| dir.join("arcvox.log"))
}

/// The bug report form; its fields fill from query parameters named by
/// their ids.
const ISSUE_FORM: &str = "https://github.com/howar31/arcvox/issues/new?template=bug_report.yml";

/// The longest form address made, well under where the server refuses one.
const ISSUE_URL_MAX: usize = 4000;

/// The form's address with the fields filled in. When the snapshot would
/// make the address too long it is left out and the other fields stay; the
/// page's copy button covers it.
fn issue_url(fields: &crate::diagnostics::IssueFields) -> String {
    let build = |with_diagnostics: bool| {
        let mut url = url::Url::parse(ISSUE_FORM).expect("the form address parses");
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("version", &fields.version);
            q.append_pair("system", &fields.system);
            q.append_pair("engines", &fields.engines);
            if with_diagnostics && !fields.diagnostics.is_empty() {
                q.append_pair("diagnostics", &fields.diagnostics);
            }
        }
        url.to_string()
    };
    let url = build(true);
    if url.len() > ISSUE_URL_MAX {
        build(false)
    } else {
        url
    }
}

/// Opens the bug report form with the fields the page was given.
#[tauri::command]
pub fn system_open_issue(fields: crate::diagnostics::IssueFields) -> CmdResult<()> {
    std::process::Command::new("open")
        .arg(issue_url(&fields))
        .spawn()?;
    Ok(())
}

/// The pages the About page links to, by name; the page never passes a URL.
fn link_url(link: &str) -> Option<&'static str> {
    Some(match link {
        "sponsor" => "https://donate.howar31.com/",
        "source" => "https://github.com/howar31/arcvox",
        "website" => "https://arcvox.howar31.com/",
        "releases" => "https://github.com/howar31/arcvox/releases",
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

    fn query(url: &str) -> Vec<(String, String)> {
        url::Url::parse(url)
            .unwrap()
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    #[test]
    fn the_issue_form_takes_every_field_and_drops_only_a_long_snapshot() {
        let mut f = crate::diagnostics::IssueFields {
            version: "0.4.0".into(),
            system: "macOS 26.0 (25A354), aarch64".into(),
            engines: "asr=breeze-asr-25-q5_0, llm=gemma-3-4b-it-q4_k_m (default profile: casual)"
                .into(),
            diagnostics: "Arcvox 0.4.0\nChip: Apple M4 & more".into(),
        };
        let url = issue_url(&f);
        assert!(url.starts_with(ISSUE_FORM), "{url}");
        let pairs = query(&url);
        let get = |k: &str| {
            pairs
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("template"), Some("bug_report.yml"));
        assert_eq!(get("version"), Some("0.4.0"));
        assert_eq!(get("system"), Some("macOS 26.0 (25A354), aarch64"));
        assert_eq!(get("engines"), Some(f.engines.as_str()));
        assert_eq!(
            get("diagnostics"),
            Some("Arcvox 0.4.0\nChip: Apple M4 & more")
        );
        f.diagnostics = "x y".repeat(2000);
        let url = issue_url(&f);
        assert!(url.len() <= ISSUE_URL_MAX);
        let pairs = query(&url);
        assert!(pairs.iter().any(|(k, v)| k == "version" && v == "0.4.0"));
        assert!(!pairs.iter().any(|(k, _)| k == "diagnostics"));
    }

    #[test]
    fn the_newest_log_is_the_latest_day() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(newest_log(dir.path()), dir.path().join("arcvox.log"));
        std::fs::write(dir.path().join("arcvox.2026-10-25.log"), "").unwrap();
        std::fs::write(dir.path().join("arcvox.2026-10-26.log"), "").unwrap();
        std::fs::write(dir.path().join("other.log"), "").unwrap();
        assert_eq!(
            newest_log(dir.path()),
            dir.path().join("arcvox.2026-10-26.log")
        );
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
