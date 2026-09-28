//! The floating panel: a Tauri window turned into a non-activating NSPanel
//! that joins every Space and sits above full-screen apps.

use tauri::{AppHandle, Manager, PhysicalPosition, Runtime};

#[cfg(target_os = "macos")]
use objc2_app_kit::{NSWindowCollectionBehavior, NSWindowStyleMask};
#[cfg(target_os = "macos")]
use tauri_nspanel::{tauri_panel, ManagerExt, WebviewWindowExt};

pub const WINDOW: &str = "widget";
/// kCGPopUpMenuWindowLevel from CGWindowLevel.h.
#[cfg(target_os = "macos")]
const LEVEL_POPUP_MENU: i64 = 101;
const BOTTOM_MARGIN_LOGICAL: f64 = 48.0;

// A panel that never becomes key or main so it never steals focus from the
// frontmost app, and that stays visible while Kongwe itself is inactive
// (the app has no window of its own; it only shows this widget and the
// settings window).
#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(WidgetPanel {
        config: {
            can_become_key_window: false,
            can_become_main_window: false,
            is_floating_panel: true,
            hides_on_deactivate: false
        }
    })
}

#[cfg(target_os = "macos")]
pub fn convert_to_panel<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let window = app
        .get_webview_window(WINDOW)
        .ok_or(tauri::Error::WindowNotFound)?;
    let panel = window.to_panel::<WidgetPanel<R>>()?;
    panel.set_level(LEVEL_POPUP_MENU);
    if let Err(e) = panel.add_style_mask(NSWindowStyleMask::NonactivatingPanel) {
        tracing::warn!(error = %e, "widget panel style mask not applied");
    }
    panel.set_collection_behavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary,
    );
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn convert_to_panel<R: Runtime>(_app: &AppHandle<R>) -> tauri::Result<()> {
    Ok(())
}

/// Moves the widget to the bottom center of the screen under the cursor
/// and orders it front without activating the app.
pub fn show_at_bottom<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(WINDOW) else {
        return;
    };
    if let Ok(cursor) = window.cursor_position() {
        if let Ok(Some(monitor)) = window.monitor_from_point(cursor.x, cursor.y) {
            let size = window
                .outer_size()
                .unwrap_or(tauri::PhysicalSize::new(640, 128));
            let scale = monitor.scale_factor();
            let pos = monitor.position();
            let mon = monitor.size();
            let x = pos.x + (mon.width as i32 - size.width as i32) / 2;
            let y = pos.y + mon.height as i32
                - size.height as i32
                - (BOTTOM_MARGIN_LOGICAL * scale) as i32;
            let _ = window.set_position(PhysicalPosition::new(x, y));
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(panel) = app.get_webview_panel(WINDOW) {
            panel.order_front_regardless();
            return;
        }
    }
    let _ = window.show();
}

/// Toggles whether the widget panel ignores mouse events. The panel is
/// click-through by default so it never steals clicks from the app
/// underneath it; it becomes interactive only while a copyable message
/// (with a Copy button) is shown. Must be called on the main thread.
pub fn set_click_through<R: Runtime>(app: &AppHandle<R>, ignore: bool) {
    let Some(window) = app.get_webview_window(WINDOW) else {
        return;
    };
    if let Err(e) = window.set_ignore_cursor_events(ignore) {
        tracing::debug!(error = %e, "widget click-through not applied");
    }
}

pub fn hide<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "macos")]
    {
        if let Ok(panel) = app.get_webview_panel(WINDOW) {
            panel.hide();
            return;
        }
    }
    if let Some(window) = app.get_webview_window(WINDOW) {
        let _ = window.hide();
    }
}
