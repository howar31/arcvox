//! Where the floating widget sits: the geometry, in display points, and
//! the position the user chose on each display by dragging.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const FILE_NAME: &str = "widget.json";

/// The horizontal pill.
pub const WIDE: Size = Size { w: 320.0, h: 64.0 };
/// The vertical pill, used at a left or right edge while there is only a
/// short label to show.
pub const TALL: Size = Size { w: 48.0, h: 148.0 };
/// Gap between the default position and the bottom of the display.
const BOTTOM_MARGIN: f64 = 48.0;
/// Gap kept between the widget and the display's edges.
const EDGE_MARGIN: f64 = 12.0;
/// A widget dropped this close to a left or right edge docks to it.
const DOCK_DISTANCE: f64 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub w: f64,
    pub h: f64,
}

/// A rectangle in global points; `y` grows downwards.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    fn right(&self) -> f64 {
        self.x + self.w
    }

    fn bottom(&self) -> f64 {
        self.y + self.h
    }

    fn contains(&self, (x, y): (f64, f64)) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// Distance from the point to the rectangle; zero inside it.
    fn distance(&self, (x, y): (f64, f64)) -> f64 {
        let dx = (self.x - x).max(x - self.right()).max(0.0);
        let dy = (self.y - y).max(y - self.bottom()).max(0.0);
        dx.hypot(dy)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dock {
    #[default]
    None,
    Left,
    Right,
}

/// The widget's place on one display: its center as fractions of the
/// display's width and height, and the edge it is docked to.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub dock: Dock,
}

/// Bottom center, `BOTTOM_MARGIN` above the display's lower edge.
pub fn default_anchor(display: Rect) -> Anchor {
    Anchor {
        x: 0.5,
        y: (display.h - BOTTOM_MARGIN - WIDE.h / 2.0) / display.h,
        dock: Dock::None,
    }
}

/// A docked widget is vertical while it shows only the ring and a short
/// label (`compact`); text that needs room uses the horizontal pill.
pub fn size_for(dock: Dock, compact: bool) -> Size {
    if compact && dock != Dock::None {
        TALL
    } else {
        WIDE
    }
}

/// The widget's frame on `display`, kept inside the display.
pub fn frame(display: Rect, anchor: Anchor, size: Size) -> Rect {
    let fit = |start: f64, length: f64, wanted: f64, extent: f64| {
        let low = start + EDGE_MARGIN;
        let high = (start + length - extent - EDGE_MARGIN).max(low);
        wanted.clamp(low, high)
    };
    // A value read from a hand-edited file may be anything.
    let unit = |v: f64| {
        if v.is_finite() {
            v.clamp(0.0, 1.0)
        } else {
            0.5
        }
    };
    let center_y = display.y + unit(anchor.y) * display.h;
    let y = fit(display.y, display.h, center_y - size.h / 2.0, size.h);
    let x = match anchor.dock {
        Dock::Left => display.x + EDGE_MARGIN,
        Dock::Right => display.right() - size.w - EDGE_MARGIN,
        Dock::None => {
            let center_x = display.x + unit(anchor.x) * display.w;
            fit(display.x, display.w, center_x - size.w / 2.0, size.w)
        }
    };
    Rect {
        x,
        y,
        w: size.w,
        h: size.h,
    }
}

/// The anchor for a widget dropped at `dropped` on `display`. It docks
/// when it is dropped at a left or right edge, or so close to one that the
/// horizontal pill would end up against that edge anyway.
pub fn anchor_after_drag(display: Rect, dropped: Rect) -> Anchor {
    let (cx, cy) = dropped.center();
    let left_gap = (dropped.x).min(cx - WIDE.w / 2.0) - display.x;
    let right_gap = display.right() - (dropped.right()).max(cx + WIDE.w / 2.0);
    let dock = if left_gap <= DOCK_DISTANCE && left_gap <= right_gap {
        Dock::Left
    } else if right_gap <= DOCK_DISTANCE {
        Dock::Right
    } else {
        Dock::None
    };
    Anchor {
        x: ((cx - display.x) / display.w).clamp(0.0, 1.0),
        y: ((cy - display.y) / display.h).clamp(0.0, 1.0),
        dock,
    }
}

/// The display that contains the point, else the nearest one.
pub fn display_at(displays: &[Rect], point: (f64, f64)) -> Option<usize> {
    displays.iter().position(|d| d.contains(point)).or_else(|| {
        displays
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.distance(point).total_cmp(&b.1.distance(point)))
            .map(|(i, _)| i)
    })
}

/// The positions chosen by dragging, one per display, in `widget.json`
/// next to the client settings.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WidgetPositions {
    /// Display name -> anchor. A display that is not listed uses the default.
    pub positions: BTreeMap<String, Anchor>,
}

impl WidgetPositions {
    pub fn path(config_dir: &Path) -> PathBuf {
        config_dir.join(FILE_NAME)
    }

    /// Never fails: a missing or unreadable file yields no saved positions.
    pub fn load(config_dir: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(Self::path(config_dir)) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "widget positions unparsable, using defaults");
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

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 1512.0,
        h: 982.0,
    };
    /// A second display to the right of the main one, placed lower.
    const SIDE: Rect = Rect {
        x: 1512.0,
        y: 200.0,
        w: 1920.0,
        h: 1080.0,
    };

    /// Equal within a rounding error of the fraction arithmetic.
    fn close(a: Rect, b: Rect) -> bool {
        [a.x - b.x, a.y - b.y, a.w - b.w, a.h - b.h]
            .iter()
            .all(|d| d.abs() < 1e-6)
    }

    fn rect(x: f64, y: f64, size: Size) -> Rect {
        Rect {
            x,
            y,
            w: size.w,
            h: size.h,
        }
    }

    #[test]
    fn the_default_is_the_bottom_center_of_the_display() {
        let f = frame(MAIN, default_anchor(MAIN), WIDE);
        assert!(close(f, rect(596.0, 870.0, WIDE)), "{f:?}");
        let f = frame(SIDE, default_anchor(SIDE), WIDE);
        assert!(close(f, rect(1512.0 + 800.0, 200.0 + 968.0, WIDE)), "{f:?}");
    }

    #[test]
    fn a_docked_widget_is_vertical_only_while_compact() {
        assert_eq!(size_for(Dock::Left, true), TALL);
        assert_eq!(size_for(Dock::Right, true), TALL);
        assert_eq!(size_for(Dock::Left, false), WIDE);
        assert_eq!(size_for(Dock::None, true), WIDE);
    }

    #[test]
    fn a_docked_widget_keeps_its_edge_and_height_in_both_shapes() {
        let anchor = Anchor {
            x: 0.0,
            y: 0.5,
            dock: Dock::Left,
        };
        assert_eq!(frame(MAIN, anchor, TALL), rect(12.0, 417.0, TALL));
        assert_eq!(frame(MAIN, anchor, WIDE), rect(12.0, 459.0, WIDE));
        let anchor = Anchor {
            dock: Dock::Right,
            ..anchor
        };
        assert_eq!(
            frame(SIDE, anchor, TALL),
            rect(1512.0 + 1920.0 - 60.0, 200.0 + 466.0, TALL)
        );
        assert_eq!(
            frame(SIDE, anchor, WIDE),
            rect(1512.0 + 1920.0 - 332.0, 200.0 + 508.0, WIDE)
        );
    }

    #[test]
    fn the_frame_stays_inside_the_display() {
        let top_left = Anchor {
            x: 0.0,
            y: 0.0,
            dock: Dock::None,
        };
        assert_eq!(frame(MAIN, top_left, WIDE), rect(12.0, 12.0, WIDE));
        let beyond = Anchor {
            x: 7.0,
            y: f64::NAN,
            dock: Dock::None,
        };
        let f = frame(MAIN, beyond, WIDE);
        assert_eq!(f.x, 1512.0 - 320.0 - 12.0);
        assert!(f.y >= 12.0 && f.y <= 982.0 - 64.0 - 12.0);
    }

    #[test]
    fn a_drop_away_from_the_edges_keeps_the_center() {
        let dropped = rect(400.0, 300.0, WIDE);
        let anchor = anchor_after_drag(MAIN, dropped);
        assert_eq!(anchor.dock, Dock::None);
        assert!(close(frame(MAIN, anchor, WIDE), dropped));
    }

    #[test]
    fn a_drop_at_a_side_edge_docks_there() {
        let anchor = anchor_after_drag(MAIN, rect(20.0, 300.0, WIDE));
        assert_eq!(anchor.dock, Dock::Left);
        let anchor = anchor_after_drag(MAIN, rect(1512.0 - 330.0, 300.0, WIDE));
        assert_eq!(anchor.dock, Dock::Right);
        let anchor = anchor_after_drag(SIDE, rect(1512.0 + 4.0, 500.0, WIDE));
        assert_eq!(anchor.dock, Dock::Left);
        // The height of the drop is kept.
        assert!((frame(SIDE, anchor, WIDE).y - 500.0).abs() < 1e-6);
    }

    #[test]
    fn a_vertical_widget_undocks_only_when_the_horizontal_one_would_clear_the_edge() {
        // Pulled a little away from the left edge: the horizontal pill
        // would still sit against the edge, so it stays docked.
        let near = anchor_after_drag(MAIN, rect(80.0, 300.0, TALL));
        assert_eq!(near.dock, Dock::Left);
        let clear = anchor_after_drag(MAIN, rect(400.0, 300.0, TALL));
        assert_eq!(clear.dock, Dock::None);
        assert!(close(frame(MAIN, clear, WIDE), rect(264.0, 342.0, WIDE)));
    }

    #[test]
    fn the_display_is_the_one_under_the_point_or_the_nearest() {
        let displays = [MAIN, SIDE];
        assert_eq!(display_at(&displays, (100.0, 100.0)), Some(0));
        assert_eq!(display_at(&displays, (2000.0, 600.0)), Some(1));
        // In the gap above the second display.
        assert_eq!(display_at(&displays, (2000.0, 50.0)), Some(1));
        assert_eq!(display_at(&[], (0.0, 0.0)), None);
    }

    #[test]
    fn positions_round_trip_and_a_broken_file_yields_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            WidgetPositions::load(dir.path()),
            WidgetPositions::default()
        );
        let mut saved = WidgetPositions::default();
        saved.positions.insert(
            "Monitor #1".into(),
            Anchor {
                x: 1.0,
                y: 0.25,
                dock: Dock::Right,
            },
        );
        saved.save(dir.path()).unwrap();
        assert_eq!(WidgetPositions::load(dir.path()), saved);
        assert!(!dir.path().join("widget.json.tmp").exists());
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join(FILE_NAME)).unwrap())
                .unwrap();
        assert_eq!(json["positions"]["Monitor #1"]["dock"], "right");

        std::fs::write(WidgetPositions::path(dir.path()), b"{ not json").unwrap();
        assert_eq!(
            WidgetPositions::load(dir.path()),
            WidgetPositions::default()
        );
    }
}
