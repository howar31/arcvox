//! Tauri application builder and shared state.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use arcvox_core::backup::{folder_state, FolderState};
use arcvox_core::models::{ModelStore, DEFAULT_MANIFEST_URL};
use arcvox_core::{DataDir, DeviceOptions, Engine, EngineOptions, FirstRunDefaults};
use tauri::{Emitter, Manager, Wry};

use crate::audio::capture::CpalSource;
use crate::audio::AudioSource;
use crate::backup_auto::{BackupAuto, BackupStatus, Failure, LastWrite};
use crate::commands::bench::BenchState;
use crate::commands::CmdError;
use crate::dictation::ui::{Ui, UiState};
use crate::dictation::{self, ControllerHandle, Deps};
use crate::hotkey::KEY_ESCAPE;
use crate::i18n::{self, Language};
use crate::platform::{CaptureEvent, ModifierEvent, Platform};
use crate::providers::{build_factories, BUNDLED_MANIFEST};
use crate::secrets::KeychainStore;
use crate::settings::ClientSettings;
use crate::trust;
use crate::ui::window_state::{WindowState, SETTINGS_WINDOW};
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
    /// Why the engine could not open, coded for the interface.
    pub engine_error: Mutex<Option<crate::commands::CmdError>>,
    pub bench: BenchState,
    pub window_state: WindowState,
    /// Keeps the backup folder up to date.
    pub backup: BackupAuto,
    /// The app's version, as written into settings files and backups.
    pub version: String,
}

impl AppState {
    /// Looks again at what is wrong with the profiles and the engines; the
    /// installed models change without the configuration changing.
    pub fn refresh_problems(&self) {
        let engine = self.engine.lock().unwrap().clone();
        crate::problems::refresh(engine.as_ref(), &self.models, &self.ui);
    }
}

/// The keys held while a new shortcut is recorded, sent to the settings window.
#[derive(serde::Serialize, Clone)]
struct CapturePayload {
    keys: Vec<crate::shortcut::Key>,
    /// The combination was pressed and let go.
    done: bool,
    /// Recording ended without a combination.
    cancelled: bool,
}

/// What the items of the menu-bar menu do.
pub fn tray_action(app: &tauri::AppHandle<Wry>, id: &str) {
    use crate::ui::tray;
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    match id {
        // The window comes to the front as it is; only the other items pick a page.
        tray::ID_SETTINGS => tray::open_settings(app, None),
        tray::ID_PROFILES_MANAGE => tray::open_settings(app, Some("flow")),
        tray::ID_HISTORY => tray::open_settings(app, Some("history")),
        tray::ID_STATISTICS => tray::open_settings(app, Some("home")),
        tray::ID_ABOUT => tray::open_settings(app, Some("about")),
        tray::ID_QUIT => app.exit(0),
        tray::ID_PLACE => state.ui.set_placing(!state.ui.is_placing()),
        tray::ID_FIX => match state.ui.tray().state() {
            UiState::NoAccessibility => tray::open_settings(app, Some(tray::PERMISSIONS_PAGE)),
            _ => tray::open_settings(app, Some("engines")),
        },
        tray::ID_FIX_PERMISSIONS => tray::open_settings(app, Some(tray::PERMISSIONS_PAGE)),
        tray::ID_FIX_FLOW => tray::open_settings(app, Some("flow")),
        tray::ID_FIX_ENGINES => tray::open_settings(app, Some("engines")),
        tray::ID_FIX_BACKUP => tray::open_settings(app, Some("backup")),
        tray::ID_COPY_LAST => {
            let engine = state.engine.lock().unwrap().clone();
            // Rows of failed dictations have no text.
            let last = engine
                .and_then(|e| e.history().list_recent(20).ok())
                .and_then(|rows| rows.into_iter().find(|u| !u.final_text.is_empty()));
            if let Some(u) = last {
                if !state.platform.clipboard_write_text(&u.final_text) {
                    tracing::warn!("the last result could not be put on the clipboard");
                }
            }
        }
        other => {
            // A profile picked in the menu becomes the default one.
            if let Some(id) = tray::profile_of(other) {
                let engine = state.engine.lock().unwrap().clone();
                if let Some(e) = engine {
                    if let Err(err) = crate::commands::profiles::set_default(&e, &id) {
                        tracing::warn!(error = ?err, "the default profile could not be set from the menu");
                    }
                }
                return;
            }
            // What the widget shows while idle: kept, and applied at once.
            if let Some(idle) = tray::idle_of(other) {
                let mut settings = ClientSettings::load(&state.dirs.config_dir);
                settings.widget_idle = idle;
                if let Err(e) = settings.save(&state.dirs.config_dir) {
                    tracing::warn!(error = %e, "widget idle choice not saved");
                }
                state.controller.update_settings(settings);
                state.ui.set_idle(idle);
            }
        }
    }
}

pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            crate::ui::tray::open_settings(app, None);
        }))
        .plugin(tauri_plugin_dialog::init())
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
            crate::commands::system::widget_drag_start,
            crate::commands::system::widget_placing,
            crate::commands::system::widget_preview,
            crate::commands::system::widget_reset_position,
            crate::commands::system::widget_measure,
            crate::commands::system::widget_scale,
            crate::commands::system::widget_idle,
            crate::commands::system::widget_fade,
            crate::commands::system::widget_is_placing,
            crate::commands::system::widget_menu,
            crate::commands::system::dictation_state,
            crate::commands::system::panel_fit,
            crate::commands::system::system_open_settings_window,
            crate::commands::system::system_list_microphones,
            crate::commands::system::system_list_displays,
            crate::commands::system::system_permissions,
            crate::commands::system::system_request_accessibility,
            crate::commands::system::system_open_privacy_pane,
            crate::commands::system::system_request_microphone,
            crate::commands::system::system_problem_pages,
            crate::commands::system::system_running_apps,
            crate::commands::system::system_app_names,
            crate::commands::system::system_info,
            crate::commands::system::system_reveal,
            crate::commands::system::system_open_link,
            crate::commands::config::config_get,
            crate::commands::config::config_update_general,
            crate::commands::providers::providers_upsert,
            crate::commands::providers::providers_delete,
            crate::commands::providers::providers_probe,
            crate::commands::profiles::profiles_upsert,
            crate::commands::profiles::profiles_delete,
            crate::commands::profiles::profiles_set_default,
            crate::commands::profiles::profiles_reset,
            crate::commands::profiles::profiles_presets,
            crate::commands::profiles::profiles_add_preset,
            crate::commands::dictionary::dictionary_set,
            crate::commands::prompt::prompt_get,
            crate::commands::prompt::prompt_set,
            crate::commands::prompt::prompt_restore,
            crate::commands::prompt::prompt_default,
            crate::commands::history::history_list,
            crate::commands::history::history_audio,
            crate::commands::history::history_delete_recording,
            crate::commands::history::history_clear_recordings,
            crate::commands::history::history_reveal_recording,
            crate::commands::history::history_storage,
            crate::commands::history::history_days,
            crate::commands::history::history_hours,
            crate::commands::history::recordings_dir_get,
            crate::commands::history::recordings_dir_set,
            crate::commands::history::recordings_dir_open,
            crate::commands::history::history_reprocess,
            crate::commands::history::history_attempts,
            crate::commands::history::history_stages,
            crate::commands::history::history_last,
            crate::commands::history::history_choose_attempt,
            crate::commands::history::history_delete_attempt,
            crate::commands::stats::stats_get,
            crate::commands::stats::stats_reset,
            crate::commands::history::history_delete,
            crate::commands::history::history_clear,
            crate::commands::history::history_clear_range,
            crate::commands::history::history_clear_recordings_range,
            crate::commands::secrets::secrets_set,
            crate::commands::secrets::secrets_has,
            crate::commands::secrets::secrets_delete,
            crate::commands::client::client_get,
            crate::commands::client::settings_defaults,
            crate::commands::client::client_update,
            crate::commands::client::shortcut_check,
            crate::commands::client::shortcut_capture,
            crate::commands::models::models_manifest,
            crate::commands::models::models_manifest_now,
            crate::commands::models::models_installed,
            crate::commands::models::models_download,
            crate::commands::models::models_download_recommended,
            crate::commands::models::models_cancel,
            crate::commands::models::models_delete,
            crate::commands::models::models_import,
            crate::commands::models::models_dir,
            crate::commands::native::native_status,
            crate::commands::native::native_install_language,
            crate::commands::bench::bench_clip_status,
            crate::commands::bench::bench_record_start,
            crate::commands::bench::bench_record_stop,
            crate::commands::bench::bench_clip_from_history,
            crate::commands::bench::bench_history_candidates,
            crate::commands::bench::bench_start,
            crate::commands::bench::bench_cancel,
            crate::commands::backup::settings_export,
            crate::commands::backup::settings_inspect,
            crate::commands::backup::settings_import,
            crate::commands::backup::backup_sizes,
            crate::commands::backup::backup_export,
            crate::commands::backup::backup_inspect,
            crate::commands::backup::backup_import,
            crate::commands::backup::backup_status,
            crate::commands::backup::backup_run_now,
            crate::commands::backup::backup_folder_state,
            crate::commands::backup::backup_folder_create,
            crate::commands::backup::backup_folder_open,
            crate::commands::backup::devices_list,
            crate::commands::backup::device_rename,
            crate::commands::backup::device_remove,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            setup(app.handle().clone())?;
            Ok(())
        })
        .on_menu_event(|app, event| {
            if event.id().as_ref() == crate::ui::app_menu::ID_QUIT {
                if let Some(state) = app.try_state::<AppState>() {
                    if state.ui.menu().quit_requested(app) {
                        app.exit(0);
                    }
                }
            }
        })
        .on_window_event(|window, event| {
            if window.label() == SETTINGS_WINDOW
                && matches!(
                    event,
                    tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)
                )
            {
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    state.window_state.changed(window.app_handle());
                }
            }
            // Closing the settings window hides it; the app lives in the tray.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == SETTINGS_WINDOW {
                    api.prevent_close();
                    let _ = window.hide();
                    // The speed test page is gone, so nothing will stop its
                    // recording; release the microphone now.
                    if let Some(state) = window.app_handle().try_state::<AppState>() {
                        state.bench.discard_recording();
                        // Nothing is left to end the recording of a shortcut.
                        state.platform.set_shortcut_capture(false);
                        crate::commands::bench::cancel_run(window.app_handle(), &state);
                        // Nothing is left to end the positioning of the widget either.
                        state.ui.set_placing(false);
                    }
                    #[cfg(target_os = "macos")]
                    let _ = window
                        .app_handle()
                        .set_activation_policy(tauri::ActivationPolicy::Accessory);
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                end_process(app);
            }
        });
}

/// What this machine is, for the identity the history rows carry.
fn device_options(version: &str) -> DeviceOptions {
    #[cfg(target_os = "macos")]
    let (hardware_id, name) = (
        crate::platform::macos::hardware::hardware_id(),
        crate::platform::macos::hardware::computer_name(),
    );
    #[cfg(not(target_os = "macos"))]
    let (hardware_id, name) = (None, None);
    let default = DeviceOptions::default();
    DeviceOptions {
        hardware_id,
        name: name.unwrap_or(default.name),
        platform: default.platform,
        app_version: version.to_string(),
    }
}

/// The worker that writes this device's history, statistics, recordings and
/// settings to the folder chosen on the backup page.
fn backup_auto(
    app: &tauri::AppHandle<Wry>,
    engine: Option<Engine>,
    dirs: &DataDir,
    version: &str,
    runtime: tokio::runtime::Handle,
) -> BackupAuto {
    let config_dir = dirs.config_dir.clone();
    let version = version.to_string();
    let busy_engine = engine.clone();
    let app = app.clone();
    let state_dir = dirs.config_dir.clone();
    let chosen = ClientSettings::load(&state_dir).backup.folder;
    let last = LastWrite::load(&state_dir).and_then(|l| l.status_for(chosen.as_deref()));
    let auto = BackupAuto::spawn(
        Box::new(move |manual| {
            let client = ClientSettings::load(&config_dir);
            let Some(folder) = client.backup.folder.clone() else {
                return if manual {
                    Err(Failure::error(CmdError::new(
                        "no_backup_folder",
                        "no folder is chosen",
                    )))
                } else {
                    Ok(None)
                };
            };
            if !manual && !client.backup.auto {
                return Ok(None);
            }
            // A folder that is gone is made again only when the user chose that.
            let recreate = client.backup.recreate;
            let found = folder_state(&folder, true);
            if !(found.usable() || (found == FolderState::Missing && recreate)) {
                return Err(Failure::folder(found));
            }
            let engine = engine.clone().ok_or_else(|| {
                Failure::error(CmdError::new("engine_unavailable", "engine unavailable"))
            })?;
            let sections = crate::commands::backup::snapshot_sections(client.backup.categories);
            let snapshot = if sections.is_empty() {
                None
            } else {
                let file = crate::commands::backup::build_settings_file(
                    &engine, &client, &sections, &version,
                )
                .map_err(Failure::error)?;
                Some(file.to_bytes())
            };
            runtime
                .block_on(engine.backup_write_folder(
                    &folder,
                    client.backup.categories.into(),
                    snapshot,
                    recreate,
                ))
                .map(Some)
                .map_err(|e| Failure::error(e.into()))
        }),
        Box::new(move || busy_engine.as_ref().is_some_and(Engine::busy)),
        Box::new(move |status| {
            if let BackupStatus::Ok { at, report } = status {
                if let Some(folder) = ClientSettings::load(&state_dir).backup.folder {
                    let last = LastWrite {
                        folder,
                        at: *at,
                        report: report.clone(),
                    };
                    if let Err(e) = last.save(&state_dir) {
                        tracing::warn!(error = %e, "the last backup was not noted");
                    }
                }
            }
            let _ = app.emit_to(SETTINGS_WINDOW, "backup-status", status.clone());
        }),
    );
    if let Some(status) = last {
        auto.restore(status);
    }
    auto
}

/// Ends the process at once when the app exits. A normal exit runs the
/// inference runtime's static destructors, and those abort while a model
/// still holds GPU resources; a model that is loading or in use cannot be
/// freed first, so the destructors are skipped instead.
fn end_process(app: &tauri::AppHandle<Wry>) {
    if let Some(state) = app.try_state::<AppState>() {
        state.bench.abort_run();
        state.bench.discard_recording();
        // A change that still waits for its write to the backup folder.
        state.backup.flush();
    }
    tracing::info!("exiting");
    // `_exit` below skips every destructor, so the log's buffered lines are
    // written here.
    if let Some(slot) = app.try_state::<crate::logging::LogSlot>() {
        slot.close();
    }
    #[cfg(unix)]
    {
        extern "C" {
            fn _exit(status: i32) -> !;
        }
        // SAFETY: `_exit` takes no pointers and does not return.
        unsafe { _exit(0) }
    }
}

fn setup(app: tauri::AppHandle<Wry>) -> Result<(), Box<dyn std::error::Error>> {
    let app_name = crate::identity::name(app.package_info());
    let dirs = DataDir::resolve(&app_name).ok_or("no home directory")?;
    dirs.ensure()?;
    // The settings window carries the name and version of this build.
    if let Some(w) = app.get_webview_window(SETTINGS_WINDOW) {
        let _ = w.set_title(&crate::identity::name_and_version(app.package_info()));
    }
    #[cfg(target_os = "macos")]
    crate::platform::macos::dock_icon::follow_appearance();
    let log_guard = crate::logging::init(&dirs.data_dir);
    app.manage(crate::logging::LogSlot(std::sync::Mutex::new(log_guard)));
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
    let capture_app = app.clone();
    platform.set_capture_sink(Arc::new(move |event| {
        let payload = match event {
            CaptureEvent::Keys { keys, done } => CapturePayload {
                keys,
                done,
                cancelled: false,
            },
            CaptureEvent::Cancelled => CapturePayload {
                keys: Vec::new(),
                done: false,
                cancelled: true,
            },
        };
        let _ = capture_app.emit_to(SETTINGS_WINDOW, "shortcut-capture", payload);
    }));

    crate::ui::widget::convert_to_panel(&app)?;
    let window_state = WindowState::new(dirs.config_dir.clone());
    window_state.restore(&app);
    let ui = Arc::new(TauriUi::new(
        app.clone(),
        language,
        settings.widget_size.scale(),
        &dirs.config_dir,
    )?);
    ui.set_display(settings.widget_display.clone());
    ui.set_idle(settings.widget_idle);
    ui.set_fade(settings.widget_fade);

    let secrets = Arc::new(KeychainStore::new(app_name.clone()));
    // A stalled connection fails the read instead of hanging a download or its Cancel.
    let client = reqwest::Client::builder()
        .read_timeout(Duration::from_secs(30))
        .build()?;
    let models = Arc::new(ModelStore::new(
        &dirs,
        client,
        BUNDLED_MANIFEST,
        Some(manifest_url(settings.manifest_url.as_deref())),
    ));
    let version = app.package_info().version.to_string();
    let config_exists = dirs.config_path().exists();
    let (engine, engine_error, runtime) = tauri::async_runtime::block_on(async {
        let runtime = tokio::runtime::Handle::current();
        // Only a new config reads these defaults. The Apple status checks
        // block and refuse the main thread.
        #[cfg(target_os = "macos")]
        let first_run: FirstRunDefaults = if config_exists {
            FirstRunDefaults::default()
        } else {
            tokio::task::spawn_blocking(crate::platform::macos::apple::first_run_defaults)
                .await
                .unwrap_or_default()
        };
        #[cfg(not(target_os = "macos"))]
        let first_run = {
            let _ = config_exists;
            FirstRunDefaults::default()
        };
        match Engine::open_with(
            dirs.clone(),
            EngineOptions {
                secrets,
                factories: build_factories(models.clone()),
                first_run,
                identity: device_options(&version),
            },
        )
        .await
        {
            Ok(e) => (Some(e), None, runtime),
            Err(e) => (None, Some(crate::commands::CmdError::from(e)), runtime),
        }
    });

    let backup = backup_auto(&app, engine.clone(), &dirs, &version, runtime.clone());
    {
        let (ui, app) = (ui.clone(), app.clone());
        backup.on_folder(Box::new(move |state| {
            ui.set_backup_folder(state);
            let _ = app.emit_to(SETTINGS_WINDOW, "backup-folder", state);
        }));
        // A folder that went away while the app was not running.
        let chosen = ClientSettings::load(&dirs.config_dir).backup;
        if let Some(folder) = chosen.folder {
            let remake = chosen.auto && chosen.recreate;
            backup.saw_with(folder_state(&folder, false), false, remake);
        }
    }
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

    let state_models = models.clone();
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
        bench: BenchState::default(),
        window_state,
        backup: backup.clone(),
        version,
    });

    // A quit request from outside the app's own menus (the Dock menu, a
    // quit Apple event) takes the same two steps as the menu item.
    #[cfg(target_os = "macos")]
    {
        let handle = app.clone();
        crate::platform::macos::terminate::install(move || {
            handle
                .try_state::<AppState>()
                .is_none_or(|state| state.ui.menu().quit_requested(&handle))
        });
    }

    {
        let mut rx = state_models.subscribe();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(p) => {
                        let _ =
                            tauri::Emitter::emit_to(&app, "settings", "model-download-progress", p);
                    }
                    // Dropped events are only intermediate progress; keep receiving.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    if let Some(e) = engine.as_ref() {
        e.warm_up_default_asr();
        let mut rx = e.config().subscribe();
        let app = app.clone();
        let (watched, models, ui) = (e.clone(), state_models.clone(), ui.clone());
        crate::problems::refresh(Some(&watched), &models, &ui);
        ui.tray().set_profiles(tray_profiles(&e.config().current()));
        tauri::async_runtime::spawn(async move {
            while rx.changed().await.is_ok() {
                backup.touch();
                crate::problems::refresh(Some(&watched), &models, &ui);
                let cfg = rx.borrow().clone();
                ui.tray().set_profiles(tray_profiles(&cfg));
                let _ = tauri::Emitter::emit_to(&app, "settings", "config-changed", (*cfg).clone());
            }
        });
    }

    if engine.is_none() {
        tracing::error!(error = ?engine_error, "engine failed to open");
        ui.set_state(UiState::EngineError);
    } else {
        ui.set_state(UiState::Idle);
    }

    follow_trust(app, platform, controller, ui, engine.is_some());
    Ok(())
}

/// The profiles as the menu-bar menu lists them.
fn tray_profiles(cfg: &arcvox_core::config::model::Config) -> Vec<crate::ui::tray::ProfileEntry> {
    cfg.profiles
        .iter()
        .map(|p| crate::ui::tray::ProfileEntry {
            id: p.id.clone(),
            name: p.name.clone(),
            is_default: p.is_default,
        })
        .collect()
}

/// The `client.json` override when it parses, else the built-in catalogue URL.
fn manifest_url(override_url: Option<&str>) -> url::Url {
    override_url
        .and_then(|raw| {
            url::Url::parse(raw)
                .map_err(|e| tracing::warn!(error = %e, "ignoring invalid manifest_url"))
                .ok()
        })
        .unwrap_or_else(|| url::Url::parse(DEFAULT_MANIFEST_URL).expect("default manifest URL"))
}

/// Keeps the shortcut monitor in step with the Accessibility trust for the
/// life of the process: installed while the trust is there, stopped the
/// moment it goes (a running tap without the trust stalls the keyboard of
/// the whole system), installed again when it comes back. The trust is read
/// again on every announcement by the system and at least once a second.
/// `engine_ready` gates the state transitions this function makes: when the
/// engine never opened, `EngineError` must stay on screen instead of being
/// overwritten with `NoAccessibility`/`Idle`, even though the monitor is
/// still installed once Accessibility is granted.
fn follow_trust(
    app: tauri::AppHandle<Wry>,
    platform: Arc<dyn Platform>,
    controller: ControllerHandle,
    ui: Arc<TauriUi>,
    engine_ready: bool,
) {
    let trusted = platform.accessibility_trusted(false);
    if trusted {
        install_monitor(&platform, &controller);
    } else {
        if let Some(state) = trust::state_for(false, engine_ready) {
            ui.set_state(state);
        }
    }
    ui.set_microphone(platform.microphone_access());

    let (wake_tx, wake_rx) = std::sync::mpsc::channel::<()>();
    platform.watch_accessibility(Box::new(move || {
        let _ = wake_tx.send(());
    }));
    std::thread::spawn(move || {
        let mut trust = trust::Trust::new(trusted);
        loop {
            // Woken by the system's announcement, else once a second.
            let _ = wake_rx.recv_timeout(TRUST_POLL);
            ui.set_microphone(platform.microphone_access());
            let Some(action) = trust.observe(platform.accessibility_trusted(false)) else {
                continue;
            };
            match action {
                trust::Action::Suspend => {
                    tracing::warn!("accessibility trust taken away; shortcuts stopped");
                    platform.stop_modifier_monitor();
                    // A dictation in progress has lost the key that ends it.
                    let _ = controller.modifier_sender().send(ModifierEvent::KeyDown {
                        key_code: KEY_ESCAPE,
                        at: std::time::Instant::now(),
                    });
                    if let Some(state) = trust::state_for(false, engine_ready) {
                        ui.set_state(state);
                    }
                }
                trust::Action::Install => {
                    tracing::info!("accessibility trust granted; shortcuts started");
                    let platform = platform.clone();
                    let controller = controller.clone();
                    let ui = ui.clone();
                    let _ = app.run_on_main_thread(move || {
                        install_monitor(&platform, &controller);
                        if let Some(state) = trust::state_for(true, engine_ready) {
                            ui.set_state(state);
                        }
                    });
                }
            }
        }
    });
}

/// How long the trust goes unchecked when the system announces nothing.
const TRUST_POLL: Duration = Duration::from_secs(1);

fn install_monitor(platform: &Arc<dyn Platform>, controller: &ControllerHandle) {
    if let Err(e) = platform.start_modifier_monitor(controller.modifier_sender()) {
        tracing::error!(error = %e, "modifier monitor not installed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_url_uses_the_override_else_the_default() {
        assert_eq!(manifest_url(None).as_str(), DEFAULT_MANIFEST_URL);
        assert_eq!(
            manifest_url(Some("not a url")).as_str(),
            DEFAULT_MANIFEST_URL
        );
        assert_eq!(
            manifest_url(Some("http://127.0.0.1:8000/m.json")).as_str(),
            "http://127.0.0.1:8000/m.json"
        );
    }
}
