//! Availability and language installation for the operating system's own
//! speech and language providers.

use serde::Serialize;
use tauri::{AppHandle, State};

use super::{CmdError, CmdResult};
use crate::app::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct NativeStatusView {
    pub status: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct InstallProgress {
    fraction: f64,
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use crate::platform::macos::apple::{
        dictation_install, dictation_status, fm_status, locale_for_hint, NativeStatus,
    };
    use arcvox_core::config::NATIVE_APPLE_DICTATION;

    pub fn status_string(s: NativeStatus) -> &'static str {
        match s {
            NativeStatus::Ready => "ready",
            NativeStatus::NeedsDownload => "needs_download",
            NativeStatus::LocaleUnsupported => "locale_unsupported",
            NativeStatus::OsTooOld => "os_too_old",
            NativeStatus::NotEligible => "not_eligible",
            NativeStatus::IntelligenceOff => "intelligence_off",
            NativeStatus::ModelNotReady => "model_not_ready",
            NativeStatus::Failed(_) => "failed",
        }
    }

    pub fn view(s: NativeStatus) -> NativeStatusView {
        let reason = match s {
            NativeStatus::Failed(code) => Some(format!("code {code}")),
            _ => None,
        };
        NativeStatusView {
            status: status_string(s).into(),
            reason,
        }
    }

    pub fn status(native_id: &str, hint: Option<&str>) -> NativeStatusView {
        view(if native_id == NATIVE_APPLE_DICTATION {
            dictation_status(locale_for_hint(hint))
        } else {
            fm_status()
        })
    }

    pub fn install(app: &AppHandle, hint: Option<&str>) -> Result<(), NativeStatus> {
        dictation_install(locale_for_hint(hint), |fraction| {
            let _ = tauri::Emitter::emit_to(
                app,
                "settings",
                "native-install-progress",
                InstallProgress { fraction },
            );
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn status_string_covers_every_variant() {
            assert_eq!(status_string(NativeStatus::Ready), "ready");
            assert_eq!(status_string(NativeStatus::NeedsDownload), "needs_download");
            assert_eq!(
                status_string(NativeStatus::LocaleUnsupported),
                "locale_unsupported"
            );
            assert_eq!(status_string(NativeStatus::OsTooOld), "os_too_old");
            assert_eq!(status_string(NativeStatus::NotEligible), "not_eligible");
            assert_eq!(
                status_string(NativeStatus::IntelligenceOff),
                "intelligence_off"
            );
            assert_eq!(
                status_string(NativeStatus::ModelNotReady),
                "model_not_ready"
            );
            assert_eq!(status_string(NativeStatus::Failed(3)), "failed");
        }
    }
}

/// Which of the platform's services are there.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct NativeServices {
    pub apple_dictation: bool,
    pub apple_foundation_models: bool,
}

/// Looks again at the platform's services (a user may have switched Apple
/// Intelligence on since launch), keeps what was found and makes the
/// engines follow it.
#[tauri::command]
pub async fn native_refresh(state: State<'_, AppState>) -> CmdResult<NativeServices> {
    #[cfg(target_os = "macos")]
    let found = tokio::task::spawn_blocking(crate::platform::macos::apple::first_run_defaults)
        .await
        .map_err(CmdError::from)?;
    #[cfg(not(target_os = "macos"))]
    let found = arcvox_core::FirstRunDefaults::default();
    *state.native.lock().unwrap() = found;
    state.sync_engines();
    state.refresh_problems();
    Ok(NativeServices {
        apple_dictation: found.apple_dictation,
        apple_foundation_models: found.apple_foundation_models,
    })
}

#[tauri::command]
pub async fn native_status(
    native_id: String,
    language_hint: Option<String>,
) -> CmdResult<NativeStatusView> {
    #[cfg(target_os = "macos")]
    {
        tokio::task::spawn_blocking(move || imp::status(&native_id, language_hint.as_deref()))
            .await
            .map_err(CmdError::from)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (native_id, language_hint);
        Ok(NativeStatusView {
            status: "os_too_old".into(),
            reason: None,
        })
    }
}

#[tauri::command]
pub async fn native_install_language(
    app: AppHandle,
    language_hint: Option<String>,
) -> CmdResult<()> {
    #[cfg(target_os = "macos")]
    {
        tokio::task::spawn_blocking(move || imp::install(&app, language_hint.as_deref()))
            .await
            .map_err(CmdError::from)?
            .map_err(|s| CmdError::plain(imp::status_string(s)))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, language_hint);
        Err(CmdError::plain("os_too_old"))
    }
}
