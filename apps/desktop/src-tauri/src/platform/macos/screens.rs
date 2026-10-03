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
