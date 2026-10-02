//! Where the floating widget sits: the geometry, in display points, and
//! the position the user chose on each display by dragging.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const FILE_NAME: &str = "widget.json";

/// The widget's short side at the small size: the height of the horizontal
/// pill and the width of the vertical one.
pub const SHORT: f64 = 48.0;
/// Gap between the widget and the panel next to it.
const PANEL_GAP: f64 = 12.0;
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

/// The widget's long side in each shape. It holds the longest state label
/// of the interface language, which the widget measures and reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Long {
    /// Width of the horizontal pill.
    pub wide: f64,
    /// Height of the vertical pill.
    pub tall: f64,
}

impl Long {
    /// Used at the small size until the widget has measured its labels.
    pub const DEFAULT: Long = Long {
        wide: 140.0,
        tall: 148.0,
    };

    /// The default at another size.
    pub fn scaled(scale: f64) -> Long {
        Long {
            wide: (Self::DEFAULT.wide * scale).ceil(),
            tall: (Self::DEFAULT.tall * scale).ceil(),
        }
    }

    /// A measured value kept within sane bounds.
    pub fn measured(wide: f64, tall: f64) -> Long {
        let bound = |v: f64, fallback: f64| {
            if v.is_finite() {
                v.ceil().clamp(SHORT * 2.0, 400.0)
            } else {
                fallback
            }
        };
        Long {
            wide: bound(wide, Self::DEFAULT.wide),
            tall: bound(tall, Self::DEFAULT.tall),
        }
    }
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

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
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

/// Bottom center, `BOTTOM_MARGIN` above the display's lower edge, for a
/// widget `short` high.
pub fn default_anchor(display: Rect, short: f64) -> Anchor {
    Anchor {
        x: 0.5,
        y: (display.h - BOTTOM_MARGIN - short / 2.0) / display.h,
        dock: Dock::None,
    }
}

/// A widget docked to a left or right edge is vertical.
pub fn size_for(dock: Dock, long: Long, short: f64) -> Size {
    if dock == Dock::None {
        Size {
            w: long.wide,
            h: short,
        }
    } else {
        Size {
            w: short,
            h: long.tall,
        }
    }
}

/// `wanted`, moved so that `extent` stays inside `start..start + length`
/// with the edge margin on both sides.
fn fit(start: f64, length: f64, wanted: f64, extent: f64) -> f64 {
    let low = start + EDGE_MARGIN;
    let high = (start + length - extent - EDGE_MARGIN).max(low);
    wanted.clamp(low, high)
}

/// The frame of the panel that belongs to the widget at `widget`. Next to a
/// horizontal widget it is centred above, or below when there is no room
/// above; next to a vertical widget it is on the side with more room,
/// centred on the widget. It is kept inside the display.
pub fn panel_frame(display: Rect, widget: Rect, size: Size) -> Rect {
    let (x, y) = if widget.h > widget.w {
        let room_left = widget.x - display.x;
        let room_right = display.right() - widget.right();
        let x = if room_left > room_right {
            widget.x - PANEL_GAP - size.w
        } else {
            widget.right() + PANEL_GAP
        };
        let y = fit(
            display.y,
            display.h,
            widget.center().1 - size.h / 2.0,
            size.h,
        );
        (x, y)
    } else {
        let above = widget.y - PANEL_GAP - size.h;
        let y = if above >= display.y + EDGE_MARGIN {
            above
        } else {
            widget.bottom() + PANEL_GAP
        };
        let x = fit(
            display.x,
            display.w,
            widget.center().0 - size.w / 2.0,
            size.w,
        );
        (x, y)
    };
    Rect {
        x,
        y,
        w: size.w,
        h: size.h,
    }
}

/// The edge of the panel that faces its widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

impl Edge {
    pub fn name(self) -> &'static str {
        match self {
            Edge::Top => "top",
            Edge::Bottom => "bottom",
            Edge::Left => "left",
            Edge::Right => "right",
        }
    }
}

/// Which edge of the panel at `panel` faces the widget at `widget`.
pub fn facing_edge(widget: Rect, panel: Rect) -> Edge {
    if panel.bottom() <= widget.y {
        Edge::Bottom
    } else if panel.y >= widget.bottom() {
        Edge::Top
    } else if panel.right() <= widget.x {
        Edge::Right
    } else {
        Edge::Left
    }
}

/// The widget's frame on `display`, kept inside the display.
pub fn frame(display: Rect, anchor: Anchor, size: Size) -> Rect {
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
/// horizontal pill (`wide` across) would end up against that edge anyway.
pub fn anchor_after_drag(display: Rect, dropped: Rect, wide: f64) -> Anchor {
    let (cx, cy) = dropped.center();
    let left_gap = (dropped.x).min(cx - wide / 2.0) - display.x;
    let right_gap = display.right() - (dropped.right()).max(cx + wide / 2.0);
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

    const LONG: Long = Long {
        wide: 140.0,
        tall: 148.0,
    };
    const WIDE: Size = Size { w: 140.0, h: 48.0 };
    const TALL: Size = Size { w: 48.0, h: 148.0 };

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
        let f = frame(MAIN, default_anchor(MAIN, SHORT), WIDE);
        assert!(close(f, rect(686.0, 886.0, WIDE)), "{f:?}");
        let f = frame(SIDE, default_anchor(SIDE, SHORT), WIDE);
        assert!(close(f, rect(1512.0 + 890.0, 200.0 + 984.0, WIDE)), "{f:?}");
    }

    #[test]
    fn a_docked_widget_is_vertical() {
        assert_eq!(size_for(Dock::Left, LONG, SHORT), TALL);
        assert_eq!(size_for(Dock::Right, LONG, SHORT), TALL);
        assert_eq!(size_for(Dock::None, LONG, SHORT), WIDE);
        assert_eq!(size_for(Dock::None, LONG, 56.0), Size { w: 140.0, h: 56.0 });
    }

    #[test]
    fn a_measured_length_is_rounded_up_and_bounded() {
        assert_eq!(
            Long::measured(113.2, 116.5),
            Long {
                wide: 114.0,
                tall: 117.0
            }
        );
        let odd = Long::measured(f64::NAN, 9000.0);
        assert_eq!(odd.wide, Long::DEFAULT.wide);
        assert_eq!(odd.tall, 400.0);
        assert_eq!(
            Long::scaled(4.0 / 3.0),
            Long {
                wide: 187.0,
                tall: 198.0
            }
        );
        assert_eq!(Long::measured(10.0, 10.0).wide, 96.0);
    }

    #[test]
    fn the_panel_is_above_a_horizontal_widget_or_below_without_room() {
        let size = Size { w: 340.0, h: 174.0 };
        let widget = rect(686.0, 886.0, WIDE);
        assert_eq!(
            panel_frame(MAIN, widget, size),
            rect(586.0, 886.0 - 12.0 - 174.0, size)
        );
        let high = rect(686.0, 100.0, WIDE);
        assert_eq!(panel_frame(MAIN, high, size), rect(586.0, 160.0, size));
        // Near a side edge the panel is pushed inwards.
        let corner = rect(12.0, 886.0, WIDE);
        assert_eq!(panel_frame(MAIN, corner, size).x, 12.0);
        // On a second display the display's own origin counts.
        let top = rect(1512.0 + 800.0, 200.0 + 20.0, WIDE);
        assert_eq!(panel_frame(SIDE, top, size).y, 200.0 + 20.0 + 48.0 + 12.0);
    }

    #[test]
    fn the_panel_is_on_the_roomier_side_of_a_vertical_widget() {
        let size = Size { w: 340.0, h: 174.0 };
        let left = rect(12.0, 417.0, TALL);
        assert_eq!(
            panel_frame(MAIN, left, size),
            rect(12.0 + 48.0 + 12.0, 417.0 + 74.0 - 87.0, size)
        );
        let right = rect(1512.0 - 60.0, 417.0, TALL);
        assert_eq!(
            panel_frame(MAIN, right, size).x,
            1512.0 - 60.0 - 12.0 - 340.0
        );
        // Near the bottom the panel is pushed up to stay on the display.
        let low = rect(1512.0 - 60.0, 982.0 - 160.0, TALL);
        assert_eq!(panel_frame(MAIN, low, size).y, 982.0 - 12.0 - 174.0);
    }

    #[test]
    fn a_docked_widget_sits_at_its_edge() {
        let anchor = Anchor {
            x: 0.0,
            y: 0.5,
            dock: Dock::Left,
        };
        assert_eq!(frame(MAIN, anchor, TALL), rect(12.0, 417.0, TALL));
        let anchor = Anchor {
            dock: Dock::Right,
            ..anchor
        };
        assert_eq!(
            frame(SIDE, anchor, TALL),
            rect(1512.0 + 1920.0 - 60.0, 200.0 + 466.0, TALL)
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
        assert_eq!(f.x, 1512.0 - 140.0 - 12.0);
        assert!(f.y >= 12.0 && f.y <= 982.0 - 48.0 - 12.0);
    }

    #[test]
    fn a_drop_away_from_the_edges_keeps_the_center() {
        let dropped = rect(400.0, 300.0, WIDE);
        let anchor = anchor_after_drag(MAIN, dropped, LONG.wide);
        assert_eq!(anchor.dock, Dock::None);
        assert!(close(frame(MAIN, anchor, WIDE), dropped));
    }

    #[test]
    fn a_drop_at_a_side_edge_docks_there() {
        let anchor = anchor_after_drag(MAIN, rect(20.0, 300.0, WIDE), LONG.wide);
        assert_eq!(anchor.dock, Dock::Left);
        let anchor = anchor_after_drag(MAIN, rect(1512.0 - 150.0, 300.0, WIDE), LONG.wide);
        assert_eq!(anchor.dock, Dock::Right);
        let anchor = anchor_after_drag(SIDE, rect(1512.0 + 4.0, 500.0, WIDE), LONG.wide);
        assert_eq!(anchor.dock, Dock::Left);
        // The centre of the drop is kept: the vertical widget is centred on it.
        let centre = frame(SIDE, anchor, TALL).center().1;
        assert!((centre - 524.0).abs() < 1e-6);
    }

    #[test]
    fn a_vertical_widget_undocks_only_when_the_horizontal_one_would_clear_the_edge() {
        // Pulled a little away from the left edge: the horizontal pill
        // would still sit against the edge, so it stays docked.
        let near = anchor_after_drag(MAIN, rect(40.0, 300.0, TALL), LONG.wide);
        assert_eq!(near.dock, Dock::Left);
        let clear = anchor_after_drag(MAIN, rect(400.0, 300.0, TALL), LONG.wide);
        assert_eq!(clear.dock, Dock::None);
        assert!(close(frame(MAIN, clear, WIDE), rect(354.0, 350.0, WIDE)));
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

    #[test]
    fn the_panel_knows_which_of_its_edges_faces_the_widget() {
        let size = Size { w: 240.0, h: 100.0 };
        let across = Rect {
            x: 600.0,
            y: 900.0,
            w: 140.0,
            h: 48.0,
        };
        assert_eq!(
            facing_edge(across, panel_frame(MAIN, across, size)),
            Edge::Bottom
        );
        let top = Rect { y: 20.0, ..across };
        assert_eq!(facing_edge(top, panel_frame(MAIN, top, size)), Edge::Top);
        let right = Rect {
            x: 1452.0,
            y: 400.0,
            w: 48.0,
            h: 148.0,
        };
        assert_eq!(
            facing_edge(right, panel_frame(MAIN, right, size)),
            Edge::Right
        );
        let left = Rect { x: 12.0, ..right };
        assert_eq!(facing_edge(left, panel_frame(MAIN, left, size)), Edge::Left);
    }
}
