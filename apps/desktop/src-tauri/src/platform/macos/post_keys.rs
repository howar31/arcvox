//! Synthetic keyboard events: Cmd+V and unicode typing. Posting needs the
//! Accessibility permission.

use std::thread::sleep;
use std::time::Duration;

use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, EventField, KeyCode};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

use super::event_tap::OWN_EVENT_MARK;

/// Gap between unicode chunks so slow apps keep the order.
const TYPE_GAP: Duration = Duration::from_millis(2);

fn source() -> Option<CGEventSource> {
    CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()
}

/// Posts an event marked as this process's own, so the shortcut tap does
/// not take it for a key the user pressed.
fn post(event: &CGEvent) {
    event.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, OWN_EVENT_MARK);
    event.post(CGEventTapLocation::HID);
}

pub fn post_paste() {
    let Some(src) = source() else { return };
    for down in [true, false] {
        if let Ok(ev) = CGEvent::new_keyboard_event(src.clone(), KeyCode::ANSI_V, down) {
            ev.set_flags(CGEventFlags::CGEventFlagCommand);
            post(&ev);
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
            post(&down);
        }
        if let Ok(up) = CGEvent::new_keyboard_event(src.clone(), 0, false) {
            post(&up);
        }
        sleep(TYPE_GAP);
    }
}
