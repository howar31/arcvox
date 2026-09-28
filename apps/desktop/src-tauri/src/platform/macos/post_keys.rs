//! Synthetic keyboard events: Cmd+V and unicode typing. Posting needs the
//! Accessibility permission.

use std::thread::sleep;
use std::time::Duration;

use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, KeyCode};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

/// Gap between unicode chunks so slow apps keep the order.
const TYPE_GAP: Duration = Duration::from_millis(2);

fn source() -> Option<CGEventSource> {
    CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()
}

pub fn post_paste() {
    let Some(src) = source() else { return };
    for down in [true, false] {
        if let Ok(ev) = CGEvent::new_keyboard_event(src.clone(), KeyCode::ANSI_V, down) {
            ev.set_flags(CGEventFlags::CGEventFlagCommand);
            ev.post(CGEventTapLocation::HID);
        }
    }
}

/// `chunks` come from `insert::executors::chunk_utf16` (at most 20 units,
/// surrogate pairs intact).
pub fn type_chunks(chunks: &[Vec<u16>]) {
    let Some(src) = source() else { return };
    for chunk in chunks {
        if let Ok(down) = CGEvent::new_keyboard_event(src.clone(), 0, true) {
            down.set_string_from_utf16_unchecked(chunk);
            down.post(CGEventTapLocation::HID);
        }
        if let Ok(up) = CGEvent::new_keyboard_event(src.clone(), 0, false) {
            up.post(CGEventTapLocation::HID);
        }
        sleep(TYPE_GAP);
    }
}
