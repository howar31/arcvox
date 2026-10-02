//! The floating widget and the panel next to it: two Tauri windows turned
//! into non-activating NSPanels that join every Space and sit above
//! full-screen apps.

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewWindow};

#[cfg(target_os = "macos")]
use objc2_app_kit::{NSWindowCollectionBehavior, NSWindowStyleMask};
#[cfg(target_os = "macos")]
use tauri_nspanel::{tauri_panel, ManagerExt, WebviewWindowExt};

use super::placement::{self, Long, Rect, Size, WidgetPositions};

pub const WINDOW: &str = "widget";
/// The window that holds what does not fit the widget: messages, the
/// recognised text, buttons.
pub const PANEL: &str = "panel";
/// Bounds for the size the panel reports for its content.
const PANEL_MAX: Size = Size { w: 520.0, h: 800.0 };
/// kCGPopUpMenuWindowLevel from CGWindowLevel.h.
#[cfg(target_os = "macos")]
const LEVEL_POPUP_MENU: i64 = 101;
/// A press that moves the pointer less than this is a click, not a drag.
const DRAG_THRESHOLD: f64 = 3.0;

// A panel that never becomes key or main so it never steals focus from the
// frontmost app, and that stays visible while Arcvox itself is inactive
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
    for label in [WINDOW, PANEL] {
        let window = app
            .get_webview_window(label)
            .ok_or(tauri::Error::WindowNotFound)?;
        let panel = window.to_panel::<WidgetPanel<R>>()?;
        panel.set_level(LEVEL_POPUP_MENU);
        if let Err(e) = panel.add_style_mask(NSWindowStyleMask::NonactivatingPanel) {
            tracing::warn!(error = %e, window = label, "panel style mask not applied");
        }
        panel.set_collection_behavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary,
        );
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn convert_to_panel<R: Runtime>(_app: &AppHandle<R>) -> tauri::Result<()> {
    Ok(())
}

pub(super) struct Display {
    /// The name the position is saved under.
    pub(super) key: String,
    pub(super) rect: Rect,
}

// All geometry below is in global points with the origin at the top left of
// the primary display. The window system reports each monitor in pixels of
// that monitor's scale and the pointer in pixels of the primary display's
// scale, so each value is divided by the scale it was reported in.

pub(super) fn displays<R: Runtime>(app: &AppHandle<R>) -> Vec<Display> {
    let displays = app
        .available_monitors()
        .unwrap_or_default()
        .into_iter()
        .map(|m| {
            let scale = m.scale_factor();
            let origin = m.position().to_logical::<f64>(scale);
            let size = m.size().to_logical::<f64>(scale);
            Display {
                key: m.name().cloned().unwrap_or_default(),
                rect: Rect {
                    x: origin.x,
                    y: origin.y,
                    w: size.width,
                    h: size.height,
                },
            }
        })
        .collect();
    distinct_keys(displays)
}

/// Monitors of the same model report the same name. The first one, from the
/// left and then from the top, keeps the name; the others get a number, so
/// each has a position of its own.
fn distinct_keys(mut displays: Vec<Display>) -> Vec<Display> {
    let mut order: Vec<usize> = (0..displays.len()).collect();
    order.sort_by(|&a, &b| {
        let (a, b) = (&displays[a].rect, &displays[b].rect);
        a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y))
    });
    let mut seen: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for i in order {
        let n = seen.entry(displays[i].key.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            displays[i].key = format!("{} #{n}", displays[i].key);
        }
    }
    displays
}

fn pointer<R: Runtime>(app: &AppHandle<R>) -> Option<(f64, f64)> {
    let scale = app.primary_monitor().ok()??.scale_factor();
    let p = app.cursor_position().ok()?.to_logical::<f64>(scale);
    Some((p.x, p.y))
}

pub(super) fn window_frame<R: Runtime>(window: &WebviewWindow<R>) -> Option<Rect> {
    let scale = window.scale_factor().ok()?;
    let origin = window.outer_position().ok()?.to_logical::<f64>(scale);
    let size = window.outer_size().ok()?.to_logical::<f64>(scale);
    Some(Rect {
        x: origin.x,
        y: origin.y,
        w: size.width,
        h: size.height,
    })
}

/// Moves and resizes the window in one step, so a change of shape never
/// shows the new size at the old place. Must be called on the main thread.
#[cfg(target_os = "macos")]
pub(super) fn apply_frame<R: Runtime>(
    window: &WebviewWindow<R>,
    frame: Rect,
    displays: &[Display],
) {
    use objc2_app_kit::NSWindow;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let Ok(ptr) = window.ns_window() else {
        return;
    };
    // AppKit measures from the bottom left of the primary display.
    let primary_height = displays
        .iter()
        .find(|d| d.rect.x == 0.0 && d.rect.y == 0.0)
        .or(displays.first())
        .map_or(0.0, |d| d.rect.h);
    let origin = NSPoint::new(frame.x, primary_height - frame.y - frame.h);
    // SAFETY: the pointer is the window's live NSWindow and this runs on
    // the main thread.
    let ns_window = unsafe { &*ptr.cast::<NSWindow>() };
    ns_window.setFrame_display(NSRect::new(origin, NSSize::new(frame.w, frame.h)), true);
}

#[cfg(not(target_os = "macos"))]
pub(super) fn apply_frame<R: Runtime>(
    window: &WebviewWindow<R>,
    frame: Rect,
    _displays: &[Display],
) {
    let _ = window.set_size(tauri::LogicalSize::new(frame.w, frame.h));
    let _ = window.set_position(tauri::LogicalPosition::new(frame.x, frame.y));
}

#[cfg(target_os = "macos")]
fn primary_button_down() -> bool {
    objc2_app_kit::NSEvent::pressedMouseButtons() & 1 == 1
}

#[cfg(not(target_os = "macos"))]
fn primary_button_down() -> bool {
    false
}

/// Orders the window front without activating the app.
fn order_front<R: Runtime>(app: &AppHandle<R>, label: &str) {
    #[cfg(target_os = "macos")]
    {
        if let Ok(panel) = app.get_webview_panel(label) {
            panel.order_front_regardless();
            return;
        }
    }
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.show();
    }
}

fn hide_window<R: Runtime>(app: &AppHandle<R>, label: &str) {
    #[cfg(target_os = "macos")]
    {
        if let Ok(panel) = app.get_webview_panel(label) {
            panel.hide();
            return;
        }
    }
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.hide();
    }
}

/// Hides the widget and its panel.
pub fn hide<R: Runtime>(app: &AppHandle<R>) {
    hide_window(app, PANEL);
    hide_window(app, WINDOW);
}

struct Drag {
    /// Pointer and window frame when the drag began.
    pointer: (f64, f64),
    frame: Rect,
}

/// Where the widget and its panel are. Every method that takes the app
/// handle must be called on the main thread.
pub struct Layout {
    config_dir: PathBuf,
    positions: WidgetPositions,
    /// The display the widget is on while it is visible. It is chosen when
    /// the widget appears, so the widget stays put if the pointer moves to
    /// another display meanwhile.
    display: Option<String>,
    /// The factor of the size the user chose.
    scale: f64,
    /// The long side the widget's labels need.
    long: Long,
    /// The size of the panel's content while the panel has any.
    panel: Option<Size>,
    drag: Option<Drag>,
}

impl Layout {
    pub fn load(config_dir: PathBuf, scale: f64) -> Self {
        Self {
            scale,
            positions: WidgetPositions::load(&config_dir),
            config_dir,
            display: None,
            long: Long::scaled(scale),
            panel: None,
            drag: None,
        }
    }

    /// Puts the widget at its place on the display under the pointer (or
    /// the display it is already on) and orders it front.
    pub fn show<R: Runtime>(&mut self, app: &AppHandle<R>) {
        // A drag owns the frame until the drop.
        if self.drag.is_none() {
            self.place(app);
        }
        order_front(app, WINDOW);
    }

    pub fn hidden(&mut self) {
        self.display = None;
        // The panel reports its content again before it is shown again.
        self.panel = None;
    }

    fn short(&self) -> f64 {
        (placement::SHORT * self.scale).round()
    }

    /// The user chose another size. The widget measures its labels again;
    /// until then the long side is an estimate.
    pub fn set_scale<R: Runtime>(&mut self, app: &AppHandle<R>, scale: f64) {
        if self.scale == scale {
            return;
        }
        self.scale = scale;
        self.long = Long::scaled(scale);
        if self.display.is_some() && self.drag.is_none() {
            self.place(app);
        }
    }

    /// The widget measured its labels: it takes the new length.
    pub fn set_long<R: Runtime>(&mut self, app: &AppHandle<R>, long: Long) {
        if self.long == long {
            return;
        }
        self.long = long;
        if self.display.is_some() && self.drag.is_none() {
            self.place(app);
        }
    }

    /// The panel has content of this size, or none. It is on screen only
    /// together with the widget.
    pub fn set_panel<R: Runtime>(&mut self, app: &AppHandle<R>, size: Option<Size>) {
        self.panel = size.filter(|s| s.w >= 1.0 && s.h >= 1.0).map(|s| Size {
            w: s.w.ceil().min(PANEL_MAX.w),
            h: s.h.ceil().min(PANEL_MAX.h),
        });
        if self.panel.is_none() {
            hide_window(app, PANEL);
        } else if self.display.is_some() && self.drag.is_none() {
            self.place(app);
        }
    }

    /// Puts the panel next to the widget at `widget` and orders it front.
    fn place_panel<R: Runtime>(&self, app: &AppHandle<R>, displays: &[Display], widget: Rect) {
        let (Some(size), Some(window)) = (self.panel, app.get_webview_window(PANEL)) else {
            return;
        };
        let rects: Vec<Rect> = displays.iter().map(|d| d.rect).collect();
        let Some(display) = placement::display_at(&rects, widget.center()).map(|i| rects[i]) else {
            return;
        };
        let frame = placement::panel_frame(display, widget, size);
        apply_frame(&window, frame, displays);
        order_front(app, PANEL);
        // The panel opens from the edge that faces the widget.
        let _ = app.emit_to(
            PANEL,
            "panel-placed",
            placement::facing_edge(widget, frame).name(),
        );
    }

    /// Forgets every saved position.
    pub fn reset<R: Runtime>(&mut self, app: &AppHandle<R>) {
        self.positions.positions.clear();
        self.save();
        if self.display.is_some() && self.drag.is_none() {
            self.place(app);
        }
    }

    fn save(&self) {
        if let Err(e) = self.positions.save(&self.config_dir) {
            tracing::warn!(error = %e, "widget positions not saved");
        }
    }

    fn place<R: Runtime>(&mut self, app: &AppHandle<R>) {
        let Some(window) = app.get_webview_window(WINDOW) else {
            return;
        };
        let displays = displays(app);
        let rects: Vec<Rect> = displays.iter().map(|d| d.rect).collect();
        let current = self
            .display
            .as_ref()
            .and_then(|key| displays.iter().position(|d| &d.key == key));
        let under_pointer = || pointer(app).and_then(|p| placement::display_at(&rects, p));
        let Some(display) = current.or_else(under_pointer).and_then(|i| displays.get(i)) else {
            return;
        };
        self.display = Some(display.key.clone());
        let anchor = self
            .positions
            .positions
            .get(&display.key)
            .copied()
            .unwrap_or_else(|| placement::default_anchor(display.rect, self.short()));
        let size = placement::size_for(anchor.dock, self.long, self.short());
        let frame = placement::frame(display.rect, anchor, size);
        apply_frame(&window, frame, &displays);
        self.place_panel(app, &displays, frame);
    }

    /// Starts following the pointer. Returns false when the drag cannot start.
    pub fn drag_begin<R: Runtime>(&mut self, app: &AppHandle<R>) -> bool {
        let frame = app
            .get_webview_window(WINDOW)
            .and_then(|w| window_frame(&w));
        let (Some(frame), Some(pointer)) = (frame, pointer(app)) else {
            return false;
        };
        self.drag = Some(Drag { pointer, frame });
        true
    }

    /// Moves the widget with the pointer while the mouse button is down.
    /// On release it saves the new position for the display the widget was
    /// dropped on, settles the widget there, and returns false.
    pub fn drag_step<R: Runtime>(&mut self, app: &AppHandle<R>) -> bool {
        let Some(drag) = self.drag.as_ref() else {
            return false;
        };
        let (Some(window), Some(pointer)) = (app.get_webview_window(WINDOW), pointer(app)) else {
            self.drag = None;
            return false;
        };
        let (dx, dy) = (pointer.0 - drag.pointer.0, pointer.1 - drag.pointer.1);
        let moved = Rect {
            x: drag.frame.x + dx,
            y: drag.frame.y + dy,
            ..drag.frame
        };
        let displays = displays(app);
        if primary_button_down() {
            apply_frame(&window, moved, &displays);
            self.place_panel(app, &displays, moved);
            return true;
        }
        self.drag = None;
        if dx.hypot(dy) >= DRAG_THRESHOLD {
            let rects: Vec<Rect> = displays.iter().map(|d| d.rect).collect();
            if let Some(display) =
                placement::display_at(&rects, moved.center()).and_then(|i| displays.get(i))
            {
                self.positions.positions.insert(
                    display.key.clone(),
                    placement::anchor_after_drag(display.rect, moved, self.long.wide),
                );
                self.save();
                self.display = Some(display.key.clone());
            }
        }
        self.place(app);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(key: &str, x: f64) -> Display {
        Display {
            key: key.into(),
            rect: Rect {
                x,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            },
        }
    }

    #[test]
    fn displays_with_the_same_name_get_keys_of_their_own() {
        let keys: Vec<String> = distinct_keys(vec![
            display("Studio", 200.0),
            display("Built-in", 0.0),
            display("Studio", 100.0),
        ])
        .into_iter()
        .map(|d| d.key)
        .collect();
        assert_eq!(keys, ["Studio #2", "Built-in", "Studio"]);
    }
}
