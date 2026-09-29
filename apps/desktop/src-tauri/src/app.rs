//! Tauri application builder and shared state.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use arcvox_core::models::ModelStore;
use arcvox_core::{DataDir, Engine, EngineOptions, FirstRunDefaults};
use tauri::{Manager, Wry};

use crate::audio::capture::CpalSource;
use crate::audio::AudioSource;
use crate::dictation::ui::{Ui, UiState};
use crate::dictation::{self, ControllerHandle, Deps};
use crate::i18n::{self, Language};
use crate::platform::Platform;
use crate::providers::{build_factories, BUNDLED_MANIFEST};
use crate::secrets::KeychainStore;
use crate::settings::ClientSettings;
use crate::ui::TauriUi;
use crate::vad::EarshotVad;

pub struct AppState {
    pub models: Arc<ModelStore>,
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
            crate::commands::system::widget_dismiss,
            crate::commands::system::system_open_settings_window,
            crate::commands::system::system_list_microphones,
            crate::commands::system::system_permissions,
            crate::commands::system::system_request_accessibility,
            crate::commands::system::system_open_privacy_pane,
            crate::commands::system::system_running_apps,
            crate::commands::system::system_info,
            crate::commands::system::system_reveal,
            crate::commands::config::config_get,
            crate::commands::config::config_update_general,
            crate::commands::providers::providers_upsert,
            crate::commands::providers::providers_delete,
            crate::commands::providers::providers_probe,
            crate::commands::profiles::profiles_upsert,
            crate::commands::profiles::profiles_delete,
            crate::commands::profiles::profiles_set_default,
            crate::commands::dictionary::dictionary_set,
            crate::commands::prompt::prompt_get,
            crate::commands::prompt::prompt_set,
            crate::commands::prompt::prompt_restore,
            crate::commands::history::history_list,
            crate::commands::history::history_delete,
            crate::commands::history::history_clear,
            crate::commands::secrets::secrets_set,
            crate::commands::secrets::secrets_has,
            crate::commands::secrets::secrets_delete,
            crate::commands::client::client_get,
            crate::commands::client::client_update,
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
    let models = Arc::new(ModelStore::new(
        &dirs,
        reqwest::Client::new(),
        BUNDLED_MANIFEST,
        None,
    ));
    let (engine, engine_error, runtime) = tauri::async_runtime::block_on(async {
        let runtime = tokio::runtime::Handle::current();
        // The Apple status checks block and refuse the main thread.
        #[cfg(target_os = "macos")]
        let first_run: FirstRunDefaults =
            tokio::task::spawn_blocking(crate::platform::macos::apple::first_run_defaults)
                .await
                .unwrap_or_default();
        #[cfg(not(target_os = "macos"))]
        let first_run = FirstRunDefaults::default();
        match Engine::open_with(
            dirs.clone(),
            EngineOptions {
                secrets,
                factories: build_factories(models.clone()),
                first_run,
            },
        )
        .await
        {
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
        models,
        engine: Mutex::new(engine.clone()),
        controller: controller.clone(),
        platform: platform.clone(),
        audio,
        ui: ui.clone(),
        dirs,
        language: Mutex::new(language),
        engine_error: Mutex::new(engine_error.clone()),
    });

    if let Some(e) = engine.as_ref() {
        e.warm_up_default_asr();
        let mut rx = e.config().subscribe();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            while rx.changed().await.is_ok() {
                let cfg = rx.borrow().clone();
                let _ = tauri::Emitter::emit_to(&app, "settings", "config-changed", (*cfg).clone());
            }
        });
    }

    if engine.is_none() {
        tracing::error!(error = ?engine_error, "engine failed to open");
        ui.set_state(UiState::EngineError);
        crate::ui::tray::open_settings(&app, Some("general"));
    } else {
        ui.set_state(UiState::Idle);
    }

    start_monitor_when_trusted(app, platform, controller, ui, engine.is_some());
    Ok(())
}

/// Installs the fn key monitor once Accessibility is granted, polling
/// every 5 s until then. The install itself runs on the main thread.
/// `engine_ready` gates the state transitions this function makes: when the
/// engine never opened, `EngineError` must stay on screen instead of being
/// overwritten with `NoAccessibility`/`Idle`, even though the monitor is
/// still installed once Accessibility is granted.
fn start_monitor_when_trusted(
    app: tauri::AppHandle<Wry>,
    platform: Arc<dyn Platform>,
    controller: ControllerHandle,
    ui: Arc<TauriUi>,
    engine_ready: bool,
) {
    if platform.accessibility_trusted(false) {
        install_monitor(&platform, &controller);
        return;
    }
    if engine_ready {
        ui.set_state(UiState::NoAccessibility);
    }
    crate::ui::tray::open_settings(&app, Some("general"));
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(5));
        if platform.accessibility_trusted(false) {
            let platform = platform.clone();
            let controller = controller.clone();
            let ui = ui.clone();
            let _ = app.run_on_main_thread(move || {
                install_monitor(&platform, &controller);
                if engine_ready {
                    ui.set_state(UiState::Idle);
                }
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
