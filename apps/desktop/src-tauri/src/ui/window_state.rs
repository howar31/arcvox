//! Remembers the settings window's size and position between launches.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

use super::placement::{Rect, Size};
use super::widget;

pub const FILE_NAME: &str = "window.json";
pub const SETTINGS_WINDOW: &str = "settings";
/// A move or resize is saved once the window has been still this long.
const SAVE_DELAY: Duration = Duration::from_millis(400);
/// This much of the window must be on a display for the saved frame to be
/// used; otherwise the display it was on is gone.
const MIN_VISIBLE: Size = Size { w: 120.0, h: 60.0 };

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowFrames {
    /// Outer frame of the settings window in global points.
    pub settings: Option<Rect>,
}

impl WindowFrames {
    pub fn path(config_dir: &Path) -> PathBuf {
        config_dir.join(FILE_NAME)
    }

    /// Never fails: a missing or unreadable file yields no saved frame.
    pub fn load(config_dir: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(Self::path(config_dir)) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "window frames unparsable, using defaults");
            Self::default()
        })
    }

    /// Atomic: temp file in the same directory, then rename.
    pub fn save(&self, config_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(config_dir)?;
        let tmp = config_dir.join(format!("{FILE_NAME}.tmp"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, Self::path(config_dir))
    }
}

/// The frame to restore for a saved one: on the display that shows most of
/// it, no smaller than `min`, no wider than `max_width`, no larger than that
/// display, and with its top edge on the display. `None` when no connected display shows enough of
/// the saved frame, so the default placement is used instead.
pub fn restored_frame(saved: Rect, displays: &[Rect], min: Size, max_width: f64) -> Option<Rect> {
    let values = [saved.x, saved.y, saved.w, saved.h];
    if values.iter().any(|v| !v.is_finite()) || saved.w <= 0.0 || saved.h <= 0.0 {
        return None;
    }
    let overlap = |d: &Rect| {
        let w = (saved.x + saved.w).min(d.x + d.w) - saved.x.max(d.x);
        let h = (saved.y + saved.h).min(d.y + d.h) - saved.y.max(d.y);
        (w.max(0.0), h.max(0.0))
    };
    let display = displays
        .iter()
        .map(|d| (d, overlap(d)))
        .filter(|(_, (w, h))| *w >= MIN_VISIBLE.w && *h >= MIN_VISIBLE.h)
        .max_by(|a, b| (a.1 .0 * a.1 .1).total_cmp(&(b.1 .0 * b.1 .1)))
        .map(|(d, _)| *d)?;
    let w = saved.w.min(max_width).max(min.w).min(display.w);
    let h = saved.h.max(min.h).min(display.h);
    Some(Rect {
        x: saved.x,
        // The title bar must be reachable; the visible part keeps the bottom on the display.
        y: saved.y.max(display.y),
        w,
        h,
    })
}

/// Saves the settings window's frame after each move or resize.
pub struct WindowState {
    config_dir: PathBuf,
    /// Bumped on every change; a pending save runs only if unchanged.
    generation: Arc<AtomicU64>,
}

impl WindowState {
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            config_dir,
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Puts the settings window where it was last time, if that place still
    /// exists. Must be called on the main thread.
    pub fn restore<R: Runtime>(&self, app: &AppHandle<R>) {
        let Some(saved) = WindowFrames::load(&self.config_dir).settings else {
            return;
        };
        let Some(window) = app.get_webview_window(SETTINGS_WINDOW) else {
            return;
        };
        let displays = widget::displays(app);
        let rects: Vec<Rect> = displays.iter().map(|d| d.rect).collect();
        let config = app
            .config()
            .app
            .windows
            .iter()
            .find(|w| w.label == SETTINGS_WINDOW);
        let min = Size {
            w: config.and_then(|w| w.min_width).unwrap_or(0.0),
            h: config.and_then(|w| w.min_height).unwrap_or(0.0),
        };
        let max_width = config.and_then(|w| w.max_width).unwrap_or(f64::INFINITY);
        if let Some(frame) = restored_frame(saved, &rects, min, max_width) {
            widget::apply_frame(&window, frame, &displays);
        }
    }

    /// The settings window moved or was resized: saves its frame once it
    /// has settled.
    pub fn changed<R: Runtime>(&self, app: &AppHandle<R>) {
        let gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let generation = self.generation.clone();
        let config_dir = self.config_dir.clone();
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(SAVE_DELAY);
            if generation.load(Ordering::SeqCst) != gen {
                return;
            }
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                let Some(window) = handle.get_webview_window(SETTINGS_WINDOW) else {
                    return;
                };
                // A full-screen or minimized window has no frame worth keeping.
                if window.is_fullscreen().unwrap_or(false) || window.is_minimized().unwrap_or(false)
                {
                    return;
                }
                let Some(frame) = widget::window_frame(&window) else {
                    return;
                };
                let frames = WindowFrames {
                    settings: Some(frame),
                };
                if let Err(e) = frames.save(&config_dir) {
                    tracing::warn!(error = %e, "window frame not saved");
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 1512.0,
        h: 982.0,
    };
    const SIDE: Rect = Rect {
        x: 1512.0,
        y: 0.0,
        w: 1920.0,
        h: 1080.0,
    };
    const MIN: Size = Size { w: 900.0, h: 560.0 };

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn a_frame_on_a_connected_display_is_restored_as_saved() {
        let saved = rect(200.0, 120.0, 1000.0, 700.0);
        assert_eq!(
            restored_frame(saved, &[MAIN, SIDE], MIN, f64::INFINITY),
            Some(saved)
        );
        let on_side = rect(1700.0, 50.0, 960.0, 900.0);
        assert_eq!(
            restored_frame(on_side, &[MAIN, SIDE], MIN, f64::INFINITY),
            Some(on_side)
        );
        // Partly off the side of the display is how the user left it.
        let hanging = rect(-300.0, 100.0, 900.0, 600.0);
        assert_eq!(
            restored_frame(hanging, &[MAIN], MIN, f64::INFINITY),
            Some(hanging)
        );
    }

    #[test]
    fn a_frame_on_a_display_that_is_gone_is_not_restored() {
        let on_side = rect(1700.0, 50.0, 960.0, 900.0);
        assert_eq!(restored_frame(on_side, &[MAIN], MIN, f64::INFINITY), None);
        assert_eq!(restored_frame(on_side, &[], MIN, f64::INFINITY), None);
        // Only a sliver is left on the remaining display.
        let sliver = rect(1450.0, 50.0, 960.0, 900.0);
        assert_eq!(restored_frame(sliver, &[MAIN], MIN, f64::INFINITY), None);
    }

    #[test]
    fn the_size_is_kept_between_the_minimum_and_the_display() {
        let small = restored_frame(
            rect(100.0, 100.0, 400.0, 300.0),
            &[MAIN],
            MIN,
            f64::INFINITY,
        )
        .unwrap();
        assert_eq!((small.w, small.h), (900.0, 560.0));
        let huge =
            restored_frame(rect(0.0, 0.0, 3000.0, 2000.0), &[MAIN], MIN, f64::INFINITY).unwrap();
        assert_eq!((huge.w, huge.h), (1512.0, 982.0));
    }

    #[test]
    fn a_frame_wider_than_the_window_may_be_is_narrowed() {
        let wide = restored_frame(rect(100.0, 100.0, 1300.0, 700.0), &[MAIN], MIN, 900.0).unwrap();
        assert_eq!((wide.x, wide.w, wide.h), (100.0, 900.0, 700.0));
    }

    #[test]
    fn the_top_of_the_window_stays_on_the_display() {
        let above = restored_frame(
            rect(100.0, -200.0, 900.0, 600.0),
            &[MAIN],
            MIN,
            f64::INFINITY,
        )
        .unwrap();
        assert_eq!(above.y, 0.0);
        let low = rect(100.0, 900.0, 900.0, 600.0);
        assert_eq!(restored_frame(low, &[MAIN], MIN, f64::INFINITY), Some(low));
    }

    #[test]
    fn values_that_are_not_a_frame_are_not_restored() {
        assert_eq!(
            restored_frame(
                rect(f64::NAN, 0.0, 900.0, 600.0),
                &[MAIN],
                MIN,
                f64::INFINITY
            ),
            None
        );
        assert_eq!(
            restored_frame(rect(0.0, 0.0, 0.0, 600.0), &[MAIN], MIN, f64::INFINITY),
            None
        );
    }

    #[test]
    fn frames_round_trip_and_a_broken_file_yields_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(WindowFrames::load(dir.path()), WindowFrames::default());
        let saved = WindowFrames {
            settings: Some(rect(10.0, 20.0, 900.0, 930.0)),
        };
        saved.save(dir.path()).unwrap();
        assert_eq!(WindowFrames::load(dir.path()), saved);
        assert!(!dir.path().join("window.json.tmp").exists());
        std::fs::write(WindowFrames::path(dir.path()), b"{ not json").unwrap();
        assert_eq!(WindowFrames::load(dir.path()), WindowFrames::default());
    }
}
