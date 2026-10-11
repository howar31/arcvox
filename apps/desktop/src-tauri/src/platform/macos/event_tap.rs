//! A session event tap for the recording shortcuts: it sees keys, modifier
//! changes and the extra mouse buttons system-wide and keeps a bound key
//! combination or mouse button from the frontmost application. Needs the
//! process to be trusted for Accessibility. Runs on a thread of its own.

use std::collections::BTreeSet;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use core_foundation::base::TCFType;
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{
    CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    CallbackResult, EventField,
};

use crate::hotkey::KEY_ESCAPE;
use crate::platform::{CaptureEvent, CaptureSink, ModifierEvent};
use crate::shortcut::{Key, Matcher, Out};

/// Marks the key events this process posts itself, so the tap leaves them alone.
pub const OWN_EVENT_MARK: i64 = 0x4B4F_4E47;

/// Recording a new shortcut ends by itself when no key arrives for this
/// long, so a forgotten recording cannot keep swallowing the keyboard.
const CAPTURE_IDLE: Duration = Duration::from_secs(20);

const FLAG_SHIFT: u64 = 0x0002_0000;
const FLAG_CONTROL: u64 = 0x0004_0000;
const FLAG_ALTERNATE: u64 = 0x0008_0000;
const FLAG_COMMAND: u64 = 0x0010_0000;
const FLAG_FN: u64 = 0x0080_0000;

/// One modifier with both of its keys: the flag that says either is down,
/// then for each side its virtual key code, its name and the device bit.
type Side = (u16, &'static str, u64);
const PAIRS: &[(u64, [Side; 2])] = &[
    (
        FLAG_CONTROL,
        [(59, "ControlLeft", 0x1), (62, "ControlRight", 0x2000)],
    ),
    (
        FLAG_SHIFT,
        [(56, "ShiftLeft", 0x2), (60, "ShiftRight", 0x4)],
    ),
    (
        FLAG_ALTERNATE,
        [(58, "AltLeft", 0x20), (61, "AltRight", 0x40)],
    ),
    (
        FLAG_COMMAND,
        [(55, "MetaLeft", 0x8), (54, "MetaRight", 0x10)],
    ),
];
const KVK_FUNCTION: u16 = 63;
const KVK_CAPS_LOCK: u16 = 57;
/// Sent by the system as a key press right after fn was pressed and
/// released quickly, in addition to fn's own modifier events.
const KVK_FN_PRESS: u16 = 179;

/// Whether a key no shortcut can name counts as another key being pressed.
fn is_other_key(code: u16) -> bool {
    code != KVK_FN_PRESS
}

/// Virtual key codes of the keys a shortcut can name besides the modifiers.
const KEYS: &[(u16, &str)] = &[
    (0, "KeyA"),
    (1, "KeyS"),
    (2, "KeyD"),
    (3, "KeyF"),
    (4, "KeyH"),
    (5, "KeyG"),
    (6, "KeyZ"),
    (7, "KeyX"),
    (8, "KeyC"),
    (9, "KeyV"),
    (11, "KeyB"),
    (12, "KeyQ"),
    (13, "KeyW"),
    (14, "KeyE"),
    (15, "KeyR"),
    (16, "KeyY"),
    (17, "KeyT"),
    (18, "Digit1"),
    (19, "Digit2"),
    (20, "Digit3"),
    (21, "Digit4"),
    (22, "Digit6"),
    (23, "Digit5"),
    (24, "Equal"),
    (25, "Digit9"),
    (26, "Digit7"),
    (27, "Minus"),
    (28, "Digit8"),
    (29, "Digit0"),
    (30, "BracketRight"),
    (31, "KeyO"),
    (32, "KeyU"),
    (33, "BracketLeft"),
    (34, "KeyI"),
    (35, "KeyP"),
    (36, "Enter"),
    (37, "KeyL"),
    (38, "KeyJ"),
    (39, "Quote"),
    (40, "KeyK"),
    (41, "Semicolon"),
    (42, "Backslash"),
    (43, "Comma"),
    (44, "Slash"),
    (45, "KeyN"),
    (46, "KeyM"),
    (47, "Period"),
    (48, "Tab"),
    (49, "Space"),
    (50, "Backquote"),
    (51, "Backspace"),
    (53, "Escape"),
    (64, "F17"),
    (65, "NumpadDecimal"),
    (67, "NumpadMultiply"),
    (69, "NumpadAdd"),
    (75, "NumpadDivide"),
    (76, "NumpadEnter"),
    (78, "NumpadSubtract"),
    (79, "F18"),
    (80, "F19"),
    (81, "NumpadEqual"),
    (82, "Numpad0"),
    (83, "Numpad1"),
    (84, "Numpad2"),
    (85, "Numpad3"),
    (86, "Numpad4"),
    (87, "Numpad5"),
    (88, "Numpad6"),
    (89, "Numpad7"),
    (90, "F20"),
    (91, "Numpad8"),
    (92, "Numpad9"),
    (96, "F5"),
    (97, "F6"),
    (98, "F7"),
    (99, "F3"),
    (100, "F8"),
    (101, "F9"),
    (103, "F11"),
    (105, "F13"),
    (106, "F16"),
    (107, "F14"),
    (109, "F10"),
    (111, "F12"),
    (113, "F15"),
    (115, "Home"),
    (116, "PageUp"),
    (117, "Delete"),
    (118, "F4"),
    (119, "End"),
    (120, "F2"),
    (121, "PageDown"),
    (122, "F1"),
    (123, "ArrowLeft"),
    (124, "ArrowRight"),
    (125, "ArrowDown"),
    (126, "ArrowUp"),
];

fn key_for(code: u16) -> Option<Key> {
    KEYS.iter()
        .find(|(c, _)| *c == code)
        .and_then(|(_, name)| Key::named(name))
}

/// The third button is the middle one; the next ones are numbered from 4.
fn mouse_key(button: i64) -> Option<Key> {
    match button {
        2 => Key::named("MouseMiddle"),
        3..=9 => Key::named(&format!("Mouse{}", button + 1)),
        _ => None,
    }
}

/// The modifiers held after a modifier key changed. `code` is the key that
/// changed and `flags` the state after it. The device bits say which side
/// is down; an event without them (posted by a remapping tool) is taken by
/// its key code.
fn modifiers_after(held: &BTreeSet<Key>, code: u16, flags: u64) -> BTreeSet<Key> {
    let mut now = held.clone();
    for (class, sides) in PAIRS {
        let keys = sides.map(|(_, name, _)| Key::named(name).expect("modifier name"));
        if flags & class == 0 {
            for k in keys {
                now.remove(&k);
            }
            continue;
        }
        let device = sides[0].2 | sides[1].2;
        if flags & device != 0 {
            for ((_, _, bit), k) in sides.iter().zip(keys) {
                if flags & bit != 0 {
                    now.insert(k);
                } else {
                    now.remove(&k);
                }
            }
        } else if let Some(i) = sides.iter().position(|(c, _, _)| *c == code) {
            if !now.remove(&keys[i]) {
                now.insert(keys[i]);
            }
        }
    }
    if code == KVK_FUNCTION {
        let key = Key::named("Fn").expect("fn");
        if flags & FLAG_FN != 0 {
            now.insert(key);
        } else {
            now.remove(&key);
        }
    }
    now
}

/// What the tap shares with the rest of the app.
pub struct TapState {
    matcher: Mutex<Matcher>,
    modifiers: Mutex<BTreeSet<Key>>,
    tx: Mutex<Option<Sender<ModifierEvent>>>,
    sink: Mutex<Option<CaptureSink>>,
    /// When the last key arrived while a new shortcut was being recorded.
    capture_seen: Mutex<Instant>,
    port: AtomicPtr<c_void>,
    running: AtomicBool,
    /// Switched off on purpose (the trust went away); a disable reported
    /// by the system is then left alone instead of being undone.
    suspended: AtomicBool,
}

impl TapState {
    pub fn new() -> Arc<TapState> {
        Arc::new(TapState {
            matcher: Mutex::new(Matcher::default()),
            modifiers: Mutex::new(BTreeSet::new()),
            tx: Mutex::new(None),
            sink: Mutex::new(None),
            capture_seen: Mutex::new(Instant::now()),
            port: AtomicPtr::new(std::ptr::null_mut()),
            running: AtomicBool::new(false),
            suspended: AtomicBool::new(false),
        })
    }

    pub fn running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Stops the tap from seeing events. Without the Accessibility trust the
    /// system still routes every keyboard event through the tap but no
    /// longer accepts what the tap hands back, so each event waits for the
    /// tap's timeout: the keyboard stands still for everyone.
    pub fn suspend(&self) {
        self.suspended.store(true, Ordering::SeqCst);
        self.set_enabled(false);
        self.modifiers.lock().unwrap().clear();
        let out = self.matcher.lock().unwrap().reset();
        self.dispatch(out, Instant::now());
    }

    /// Lets a suspended tap see events again.
    pub fn resume(&self) {
        self.suspended.store(false, Ordering::SeqCst);
        self.set_enabled(true);
    }

    fn set_enabled(&self, on: bool) {
        let port = self.port.load(Ordering::SeqCst);
        if !port.is_null() {
            // SAFETY: `port` is the tap's mach port, kept alive by the tap
            // thread for the life of the process.
            unsafe { CGEventTapEnable(port, on) };
        }
    }

    pub fn set_shortcuts(&self, shortcuts: Vec<crate::shortcut::Shortcut>) {
        let out = self.matcher.lock().unwrap().set_shortcuts(shortcuts);
        self.dispatch(out, Instant::now());
    }

    pub fn set_sink(&self, sink: CaptureSink) {
        *self.sink.lock().unwrap() = Some(sink);
    }

    /// Whether Escape cancels a dictation now, so the press is kept from
    /// the application in front.
    pub fn set_escape_cancels(&self, on: bool) {
        self.matcher.lock().unwrap().set_escape_cancels(on);
    }

    pub fn set_capture(&self, on: bool) {
        *self.capture_seen.lock().unwrap() = Instant::now();
        let out = self.matcher.lock().unwrap().set_capture(on);
        self.dispatch(out, Instant::now());
    }

    fn dispatch(&self, out: Vec<Out>, at: Instant) {
        for o in out {
            let event = match o {
                Out::Down => ModifierEvent::TriggerDown { at },
                Out::Up => ModifierEvent::TriggerUp { at },
                Out::Other => ModifierEvent::KeyDown { key_code: 0, at },
                Out::Escape => ModifierEvent::KeyDown {
                    key_code: KEY_ESCAPE,
                    at,
                },
                Out::Capture { keys, done } => {
                    self.emit(CaptureEvent::Keys { keys, done });
                    continue;
                }
                Out::CaptureCancelled => {
                    self.emit(CaptureEvent::Cancelled);
                    continue;
                }
            };
            if let Some(tx) = self.tx.lock().unwrap().as_ref() {
                let _ = tx.send(event);
            }
        }
    }

    fn emit(&self, event: CaptureEvent) {
        if let Some(sink) = self.sink.lock().unwrap().as_ref() {
            sink(event);
        }
    }

    /// Ends a recording nobody finished.
    fn expire_capture(&self, now: Instant) {
        let mut matcher = self.matcher.lock().unwrap();
        if !matcher.capturing() {
            return;
        }
        let mut seen = self.capture_seen.lock().unwrap();
        if now.duration_since(*seen) > CAPTURE_IDLE {
            matcher.set_capture(false);
            drop(matcher);
            drop(seen);
            self.emit(CaptureEvent::Cancelled);
        } else {
            *seen = now;
        }
    }

    /// Handles one event; true when it must not reach the application.
    fn handle(&self, kind: CGEventType, event: &CGEvent) -> bool {
        let at = Instant::now();
        match kind {
            CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
                // The system switched the tap off; events were missed.
                tracing::warn!(?kind, "event tap disabled by the system");
                if !self.suspended.load(Ordering::SeqCst) {
                    self.set_enabled(true);
                }
                self.modifiers.lock().unwrap().clear();
                let out = self.matcher.lock().unwrap().reset();
                self.dispatch(out, at);
                return false;
            }
            _ => {}
        }
        if event.get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA) == OWN_EVENT_MARK {
            return false;
        }
        self.expire_capture(at);
        let code = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;
        match kind {
            CGEventType::FlagsChanged => {
                // Caps Lock reports its lock state, one event for each press,
                // not when the key goes down and up; it cannot be a shortcut.
                if code == KVK_CAPS_LOCK {
                    return false;
                }
                let now = {
                    let mut held = self.modifiers.lock().unwrap();
                    *held = modifiers_after(&held, code, event.get_flags().bits());
                    held.iter().copied().collect::<Vec<Key>>()
                };
                let out = self.matcher.lock().unwrap().modifiers(&now);
                self.dispatch(out, at);
                false
            }
            CGEventType::KeyDown | CGEventType::KeyUp => {
                let Some(key) = key_for(code) else {
                    // A key no shortcut can name still ends a short press.
                    if matches!(kind, CGEventType::KeyDown) && is_other_key(code) {
                        self.dispatch(vec![Out::Other], at);
                    }
                    return false;
                };
                let decision = if matches!(kind, CGEventType::KeyDown) {
                    let repeat =
                        event.get_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT) != 0;
                    self.matcher.lock().unwrap().key_down(key, repeat)
                } else {
                    self.matcher.lock().unwrap().key_up(key)
                };
                self.dispatch(decision.out, at);
                decision.swallow
            }
            CGEventType::OtherMouseDown | CGEventType::OtherMouseUp => {
                let button = event.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER);
                let Some(key) = mouse_key(button) else {
                    return false;
                };
                let decision = if matches!(kind, CGEventType::OtherMouseDown) {
                    self.matcher.lock().unwrap().key_down(key, false)
                } else {
                    self.matcher.lock().unwrap().key_up(key)
                };
                self.dispatch(decision.out, at);
                decision.swallow
            }
            _ => false,
        }
    }
}

/// Whether the process may tap the keyboard right now. The system answers
/// this from the current Accessibility list, while `AXIsProcessTrusted`
/// keeps its earlier answer for the life of the process. The probe is a
/// filtering tap like the real one: a listen-only tap is also allowed with
/// the Input Monitoring permission alone, which lets the process neither
/// filter nor post key events. It asks for an event that a keyboard never
/// sends, so that no key press ever waits on it.
pub fn can_tap_keyboard() -> bool {
    probe(CGEventTapOptions::Default, CGEventType::TabletProximity)
}

/// Whether the process may at least watch the keyboard, which the Input
/// Monitoring permission allows on its own.
pub fn can_listen_keyboard() -> bool {
    probe(CGEventTapOptions::ListenOnly, CGEventType::KeyDown)
}

fn probe(options: CGEventTapOptions, kind: CGEventType) -> bool {
    CGEventTap::new(
        CGEventTapLocation::Session,
        CGEventTapPlacement::HeadInsertEventTap,
        options,
        vec![kind],
        |_proxy, _kind, _event| CallbackResult::Keep,
    )
    .is_ok()
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
}

/// Starts the tap on a thread of its own and delivers shortcut events to
/// `tx`. False when the tap could not be created (the process is not
/// trusted for Accessibility). Calling it again only replaces `tx`.
pub fn install(state: &Arc<TapState>, tx: Sender<ModifierEvent>) -> bool {
    *state.tx.lock().unwrap() = Some(tx);
    if state.running() {
        state.resume();
        return true;
    }
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let shared = state.clone();
    std::thread::spawn(move || {
        let in_tap = shared.clone();
        let tap = CGEventTap::new(
            CGEventTapLocation::Session,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default,
            vec![
                CGEventType::KeyDown,
                CGEventType::KeyUp,
                CGEventType::FlagsChanged,
                CGEventType::OtherMouseDown,
                CGEventType::OtherMouseUp,
            ],
            move |_proxy, kind, event| {
                if in_tap.handle(kind, event) {
                    CallbackResult::Drop
                } else {
                    CallbackResult::Keep
                }
            },
        );
        let Ok(tap) = tap else {
            let _ = ready_tx.send(false);
            return;
        };
        let Ok(source) = tap.mach_port().create_runloop_source(0) else {
            let _ = ready_tx.send(false);
            return;
        };
        shared.port.store(
            tap.mach_port().as_concrete_TypeRef() as *mut c_void,
            Ordering::SeqCst,
        );
        // SAFETY: the mode constant is a static provided by CoreFoundation.
        CFRunLoop::get_current().add_source(&source, unsafe { kCFRunLoopCommonModes });
        tap.enable();
        shared.running.store(true, Ordering::SeqCst);
        let _ = ready_tx.send(true);
        // Runs for the life of the process; `tap` stays alive with it.
        CFRunLoop::run_current();
    });
    ready_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(names: &[&str]) -> BTreeSet<Key> {
        names.iter().map(|n| Key::named(n).unwrap()).collect()
    }

    #[test]
    fn every_key_code_names_a_known_key_once() {
        let mut seen = BTreeSet::new();
        for (code, name) in KEYS {
            assert!(Key::named(name).is_some(), "{name}");
            assert!(seen.insert(name), "{name} twice");
            assert_eq!(key_for(*code).map(Key::name), Some(*name));
        }
        assert_eq!(key_for(63), None);
    }

    #[test]
    fn mouse_buttons_are_numbered_from_the_middle_one() {
        assert_eq!(mouse_key(2).map(Key::name), Some("MouseMiddle"));
        assert_eq!(mouse_key(3).map(Key::name), Some("Mouse4"));
        assert_eq!(mouse_key(9).map(Key::name), Some("Mouse10"));
        assert_eq!(mouse_key(0), None);
        assert_eq!(mouse_key(1), None);
        assert_eq!(mouse_key(10), None);
    }

    #[test]
    fn device_bits_tell_the_sides_apart() {
        // Right Command down, then left Command too, then right up.
        let a = modifiers_after(&held(&[]), 54, FLAG_COMMAND | 0x10);
        assert_eq!(a, held(&["MetaRight"]));
        let b = modifiers_after(&a, 55, FLAG_COMMAND | 0x10 | 0x8);
        assert_eq!(b, held(&["MetaLeft", "MetaRight"]));
        let c = modifiers_after(&b, 54, FLAG_COMMAND | 0x8);
        assert_eq!(c, held(&["MetaLeft"]));
        assert_eq!(modifiers_after(&c, 55, 0), held(&[]));
    }

    #[test]
    fn an_event_without_device_bits_goes_by_its_key_code() {
        let a = modifiers_after(&held(&[]), 61, FLAG_ALTERNATE);
        assert_eq!(a, held(&["AltRight"]));
        // A missed key-up is repaired by the next event without the flag.
        assert_eq!(
            modifiers_after(&a, 56, FLAG_SHIFT | 0x2),
            held(&["ShiftLeft"])
        );
    }

    #[test]
    fn the_key_event_that_follows_a_short_fn_press_is_not_another_key() {
        assert!(!is_other_key(KVK_FN_PRESS));
        // A key without a name, such as a media key, still counts.
        assert!(is_other_key(200));
    }

    #[test]
    fn fn_follows_its_own_key_only() {
        let a = modifiers_after(&held(&[]), KVK_FUNCTION, FLAG_FN);
        assert_eq!(a, held(&["Fn"]));
        // Another modifier's event carrying the fn flag does not change it.
        let b = modifiers_after(&a, 56, FLAG_SHIFT | 0x2);
        assert_eq!(b, held(&["Fn", "ShiftLeft"]));
        assert_eq!(
            modifiers_after(&b, KVK_FUNCTION, FLAG_SHIFT | 0x2),
            held(&["ShiftLeft"])
        );
    }
}
