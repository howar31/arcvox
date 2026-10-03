//! The names the system shows for the connected displays.

use core_graphics::display::CGDisplay;
use objc2::MainThreadMarker;
use objc2_app_kit::NSScreen;
use objc2_foundation::{NSNumber, NSString};

/// The system's name of each connected display, with the top left corner of
/// the display in global points, which tells displays of one model apart.
pub fn names(mtm: MainThreadMarker) -> Vec<((f64, f64), String)> {
    let number = NSString::from_str("NSScreenNumber");
    NSScreen::screens(mtm)
        .iter()
        .filter_map(|screen| {
            let id = screen
                .deviceDescription()
                .objectForKey(&number)?
                .downcast::<NSNumber>()
                .ok()?
                .unsignedIntValue();
            let origin = CGDisplay::new(id).bounds().origin;
            Some(((origin.x, origin.y), screen.localizedName().to_string()))
        })
        .collect()
}

/// The camera housing at the top of a display.
pub struct Housing {
    /// The top left corner of the display in global points.
    pub display: (f64, f64),
    /// Its left edge from that corner, its width and its height.
    pub left: f64,
    pub width: f64,
    pub height: f64,
}

/// The camera housing of each display that has one.
pub fn housings(mtm: MainThreadMarker) -> Vec<Housing> {
    let number = NSString::from_str("NSScreenNumber");
    NSScreen::screens(mtm)
        .iter()
        .filter_map(|screen| {
            let height = screen.safeAreaInsets().top;
            let (left, right) = (
                screen.auxiliaryTopLeftArea(),
                screen.auxiliaryTopRightArea(),
            );
            // The housing is what lies between the two areas beside it.
            let start = left.origin.x + left.size.width;
            let width = right.origin.x - start;
            if height <= 0.0 || width <= 0.0 {
                return None;
            }
            let id = screen
                .deviceDescription()
                .objectForKey(&number)?
                .downcast::<NSNumber>()
                .ok()?
                .unsignedIntValue();
            let origin = CGDisplay::new(id).bounds().origin;
            Some(Housing {
                display: (origin.x, origin.y),
                left: start - screen.frame().origin.x,
                width,
                height,
            })
        })
        .collect()
}
