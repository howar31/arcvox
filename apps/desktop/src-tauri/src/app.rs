//! Tauri application builder and shared state.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use kongwe_core::{DataDir, Engine};
use tauri::{Manager, Wry};

use crate::audio::capture::CpalSource;
use crate::audio::AudioSource;
use crate::dictation::ui::{Ui, UiState};
use crate::dictation::{self, ControllerHandle, Deps};
use crate::i18n::{self, Language};
use crate::platform::Platform;
use crate::secrets::KeychainStore;
use crate::settings::ClientSettings;
use crate::ui::TauriUi;
use crate::vad::EarshotVad;

pub struct AppState {
    pub engine: Mutex<Option<Engine>>,
    pub controller: ControllerHandle,
    pub platform: Arc<dyn Platform>,
    pub audio: Arc<dyn AudioSource>,
    pub ui: Arc<TauriUi>,
    pub dirs: DataDir,
    pub language: Mutex<Language>,
    pub engine_error: Mutex<Option<String>>,
}

pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            crate::ui::tray::open_settings(app, None);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ));
    // The nspanel plugin manages the panel registry that `to_panel` and
    // `get_webview_panel` read; without it those calls panic.
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());
    builder
        .invoke_handler(tauri::generate_handler![
            crate::commands::system::ui_language,
            crate::commands::system::widget_copy,
            crate::commands::system::system_open_settings_window,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            setup(app.handle().clone())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the settings window hides it; the app lives in the tray.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "settings" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn setup(app: tauri::AppHandle<Wry>) -> Result<(), Box<dyn std::error::Error>> {
    let dirs = DataDir::resolve().ok_or("no home directory")?;
    dirs.ensure()?;
    let log_guard = crate::logging::init(&dirs.data_dir);
    if let Some(g) = log_guard {
        app.manage(g);
    }
    let settings = ClientSettings::load(&dirs.config_dir);
    let language = Language::from_setting(
        settings.ui_language,
        i18n::system_preferred_language().as_deref(),
    );

    #[cfg(target_os = "macos")]
    let platform: Arc<dyn Platform> = Arc::new(crate::platform::macos::MacPlatform::new());
    #[cfg(not(target_os = "macos"))]
    let platform: Arc<dyn Platform> = unimplemented!("only macOS is supported in this phase");
    let audio: Arc<dyn AudioSource> = Arc::new(CpalSource);

    crate::ui::widget::convert_to_panel(&app)?;
    let ui = Arc::new(TauriUi::new(app.clone(), language)?);

    let secrets = Arc::new(KeychainStore::new());
    let (engine, engine_error, runtime) = tauri::async_runtime::block_on(async {
        let runtime = tokio::runtime::Handle::current();
        match Engine::open(dirs.clone(), secrets).await {
            Ok(e) => (Some(e), None, runtime),
            Err(e) => (None, Some(e.to_string()), runtime),
        }
    });

    let controller = dictation::spawn(
        Deps {
            platform: platform.clone(),
            audio: audio.clone(),
            ui: ui.clone(),
            vad_factory: Box::new(|| Box::new(EarshotVad::new())),
            runtime,
        },
        settings,
        engine.clone(),
    );

    app.manage(AppState {
        engine: Mutex::new(engine.clone()),
        controller: controller.clone(),
        platform: platform.clone(),
        audio,
        ui: ui.clone(),
        dirs,
        language: Mutex::new(language),
        engine_error: Mutex::new(engine_error.clone()),
    });

    if engine.is_none() {
        tracing::error!(error = ?engine_error, "engine failed to open");
        ui.set_state(UiState::EngineError);
        crate::ui::tray::open_settings(&app, Some("general"));
    } else {
        ui.set_state(UiState::Idle);
    }

    start_monitor_when_trusted(app, platform, controller, ui);
    Ok(())
}

/// Installs the fn key monitor once Accessibility is granted, polling
/// every 5 s until then. The install itself runs on the main thread.
fn start_monitor_when_trusted(
    app: tauri::AppHandle<Wry>,
    platform: Arc<dyn Platform>,
    controller: ControllerHandle,
    ui: Arc<TauriUi>,
) {
    if platform.accessibility_trusted(false) {
        install_monitor(&platform, &controller);
        return;
    }
    ui.set_state(UiState::NoAccessibility);
    crate::ui::tray::open_settings(&app, Some("general"));
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(5));
        if platform.accessibility_trusted(false) {
            let platform = platform.clone();
            let controller = controller.clone();
            let ui = ui.clone();
            let _ = app.run_on_main_thread(move || {
                install_monitor(&platform, &controller);
                ui.set_state(UiState::Idle);
            });
            break;
        }
    });
}

fn install_monitor(platform: &Arc<dyn Platform>, controller: &ControllerHandle) {
    if let Err(e) = platform.start_modifier_monitor(controller.modifier_sender()) {
        tracing::error!(error = %e, "modifier monitor not installed");
    }
}
