//! Recording shortcuts: which keys and mouse buttons can form one, why a
//! combination is refused, and the matcher that turns key events into
//! "shortcut down" and "shortcut up". Pure logic; the platform layer feeds
//! it and acts on what it returns.

use std::collections::BTreeSet;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Most keys in one shortcut.
pub const MAX_KEYS: usize = 3;

/// Every key a shortcut can name, by position on the keyboard. The order
/// is the order keys are shown in: modifiers first.
const NAMES: &[&str] = &[
    "Fn",
    "ControlLeft",
    "ControlRight",
    "AltLeft",
    "AltRight",
    "ShiftLeft",
    "ShiftRight",
    "MetaLeft",
    "MetaRight",
    "KeyA",
    "KeyB",
    "KeyC",
    "KeyD",
    "KeyE",
    "KeyF",
    "KeyG",
    "KeyH",
    "KeyI",
    "KeyJ",
    "KeyK",
    "KeyL",
    "KeyM",
    "KeyN",
    "KeyO",
    "KeyP",
    "KeyQ",
    "KeyR",
    "KeyS",
    "KeyT",
    "KeyU",
    "KeyV",
    "KeyW",
    "KeyX",
    "KeyY",
    "KeyZ",
    "Digit0",
    "Digit1",
    "Digit2",
    "Digit3",
    "Digit4",
    "Digit5",
    "Digit6",
    "Digit7",
    "Digit8",
    "Digit9",
    "Space",
    "Minus",
    "Equal",
    "BracketLeft",
    "BracketRight",
    "Backslash",
    "Semicolon",
    "Quote",
    "Backquote",
    "Comma",
    "Period",
    "Slash",
    "Numpad0",
    "Numpad1",
    "Numpad2",
    "Numpad3",
    "Numpad4",
    "Numpad5",
    "Numpad6",
    "Numpad7",
    "Numpad8",
    "Numpad9",
    "NumpadDecimal",
    "NumpadAdd",
    "NumpadSubtract",
    "NumpadMultiply",
    "NumpadDivide",
    "NumpadEqual",
    "NumpadEnter",
    "Enter",
    "Tab",
    "Backspace",
    "Delete",
    "ArrowLeft",
    "ArrowRight",
    "ArrowUp",
    "ArrowDown",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "Escape",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
    "F13",
    "F14",
    "F15",
    "F16",
    "F17",
    "F18",
    "F19",
    "F20",
    "MouseMiddle",
    "Mouse4",
    "Mouse5",
    "Mouse6",
    "Mouse7",
    "Mouse8",
    "Mouse9",
    "Mouse10",
];

/// A key or mouse button, as an index into `NAMES`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key(u8);

fn index(name: &str) -> Option<usize> {
    NAMES.iter().position(|n| *n == name)
}

impl Key {
    pub fn named(name: &str) -> Option<Key> {
        index(name).map(|i| Key(i as u8))
    }

    pub fn name(self) -> &'static str {
        NAMES[self.0 as usize]
    }

    fn between(self, first: &str, last: &str) -> bool {
        let (a, b) = (index(first).unwrap(), index(last).unwrap());
        (a..=b).contains(&(self.0 as usize))
    }

    /// Held together with other keys without producing input of its own.
    pub fn is_modifier(self) -> bool {
        self.between("Fn", "MetaRight")
    }

    /// The modifier without its side: both Command keys are `Meta`.
    fn family(self) -> &'static str {
        self.name()
            .trim_end_matches("Left")
            .trim_end_matches("Right")
    }

    pub fn is_mouse(self) -> bool {
        self.between("MouseMiddle", "Mouse10")
    }

    fn is_function(self) -> bool {
        self.between("F1", "F20")
    }

    /// Moves the caret or edits text when pressed alone.
    fn is_editing(self) -> bool {
        self.between("NumpadEnter", "PageDown")
    }

    pub fn is_escape(self) -> bool {
        self.name() == "Escape"
    }
}

impl Serialize for Key {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let name = String::deserialize(d)?;
        Key::named(&name).ok_or_else(|| serde::de::Error::custom(format!("unknown key `{name}`")))
    }
}

/// One recording shortcut: its keys, in display order, without repeats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Shortcut(Vec<Key>);

impl Shortcut {
    pub fn new(keys: &[Key]) -> Shortcut {
        let set: BTreeSet<Key> = keys.iter().copied().collect();
        Shortcut(set.into_iter().collect())
    }

    pub fn keys(&self) -> &[Key] {
        &self.0
    }

    fn modifiers(&self) -> impl Iterator<Item = Key> + '_ {
        self.0.iter().copied().filter(|k| k.is_modifier())
    }

    /// The key or button pressed on top of the modifiers, when there is one.
    fn main(&self) -> Option<Key> {
        self.0.iter().copied().find(|k| !k.is_modifier())
    }

    /// The default: the fn (Globe) key alone.
    pub fn fn_key() -> Shortcut {
        Shortcut(vec![Key::named("Fn").unwrap()])
    }

    /// The shortcuts a new installation starts with: the fn key and the
    /// right Control, Option and Shift keys, each on its own.
    pub fn defaults() -> Vec<Shortcut> {
        ["Fn", "ControlRight", "AltRight", "ShiftRight"]
            .iter()
            .map(|name| Shortcut(vec![Key::named(name).unwrap()]))
            .collect()
    }
}

/// Why a combination cannot be a recording shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    Empty,
    /// Escape cancels a dictation and cannot be bound.
    Escape,
    TooManyKeys,
    /// More than one key besides the modifiers.
    TwoKeys,
    /// A function key without a modifier.
    FunctionAlone,
    /// An editing or arrow key without a modifier.
    EditingAlone,
    /// A letter, digit, symbol or the space bar without a modifier.
    CharacterAlone,
    /// Shift is the only modifier of a key that types or selects.
    ShiftOnly,
    /// Used by the system or by most applications.
    Reserved,
    Duplicate,
}

/// Modifier families plus a key that the system or most applications use.
const RESERVED: &[(&[&str], &str)] = &[
    (&["Meta"], "KeyA"),
    (&["Meta"], "KeyC"),
    (&["Meta"], "KeyV"),
    (&["Meta"], "KeyX"),
    (&["Meta"], "KeyZ"),
    (&["Meta", "Shift"], "KeyZ"),
    (&["Meta"], "KeyS"),
    (&["Meta"], "KeyF"),
    (&["Meta"], "KeyP"),
    (&["Meta"], "KeyQ"),
    (&["Meta"], "KeyW"),
    (&["Meta"], "KeyH"),
    (&["Meta"], "KeyM"),
    (&["Meta"], "KeyN"),
    (&["Meta"], "KeyT"),
    (&["Meta"], "KeyO"),
    (&["Meta"], "Comma"),
    (&["Meta"], "Tab"),
    (&["Meta"], "Backquote"),
    (&["Meta"], "Backspace"),
    (&["Meta"], "Delete"),
    (&["Meta"], "Space"),
    (&["Control"], "Space"),
    (&["Control", "Meta"], "KeyQ"),
    (&["Control", "Meta"], "KeyF"),
    (&["Meta", "Shift"], "Digit3"),
    (&["Meta", "Shift"], "Digit4"),
    (&["Meta", "Shift"], "Digit5"),
];

fn reserved(s: &Shortcut) -> bool {
    let Some(main) = s.main() else {
        return false;
    };
    let families: BTreeSet<&str> = s.modifiers().map(Key::family).collect();
    RESERVED.iter().any(|(mods, key)| {
        *key == main.name()
            && mods.len() == families.len()
            && mods.iter().all(|m| families.contains(m))
    })
}

/// Checks a combination against the rules and the shortcuts already set.
pub fn refusal(keys: &[Key], existing: &[Shortcut]) -> Option<Refusal> {
    let s = Shortcut::new(keys);
    if s.0.is_empty() {
        return Some(Refusal::Empty);
    }
    if s.0.iter().any(|k| k.is_escape()) {
        return Some(Refusal::Escape);
    }
    if s.0.len() > MAX_KEYS {
        return Some(Refusal::TooManyKeys);
    }
    if s.0.iter().filter(|k| !k.is_modifier()).count() > 1 {
        return Some(Refusal::TwoKeys);
    }
    if let Some(main) = s.main().filter(|k| !k.is_mouse()) {
        let mut mods = s.modifiers().peekable();
        if mods.peek().is_none() {
            return Some(if main.is_function() {
                Refusal::FunctionAlone
            } else if main.is_editing() {
                Refusal::EditingAlone
            } else {
                Refusal::CharacterAlone
            });
        }
        if !main.is_function() && mods.all(|m| m.family() == "Shift") {
            return Some(Refusal::ShiftOnly);
        }
        if reserved(&s) {
            return Some(Refusal::Reserved);
        }
    }
    if existing.contains(&s) {
        return Some(Refusal::Duplicate);
    }
    None
}

/// Reads a stored list, leaving out a shortcut that names a key this build
/// does not know, so one such entry does not make the whole settings file
/// unreadable.
pub fn lenient<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Shortcut>, D::Error> {
    let raw = Vec::<Vec<String>>::deserialize(d)?;
    Ok(raw
        .iter()
        .filter_map(|names| {
            let keys: Option<Vec<Key>> = names.iter().map(|n| Key::named(n)).collect();
            keys.map(|k| Shortcut::new(&k))
        })
        .collect())
}

/// Drops what the rules refuse and repeats; an empty result becomes the
/// default, so there is always a shortcut.
pub fn sanitized(list: &[Shortcut]) -> Vec<Shortcut> {
    let mut out: Vec<Shortcut> = Vec::new();
    for s in list {
        if refusal(s.keys(), &out).is_none() {
            out.push(Shortcut::new(s.keys()));
        }
    }
    if out.is_empty() {
        out.push(Shortcut::fn_key());
    }
    out
}

/// What the matcher tells the dictation controller or the settings window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    /// A shortcut went down.
    Down,
    /// The shortcut that was down is no longer held.
    Up,
    /// Another key went down; it ends a short press and aborts a held one.
    Other,
    Escape,
    /// While recording a new shortcut: the keys held now, or, when `done`,
    /// the combination that was pressed and let go.
    Capture {
        keys: Vec<Key>,
        done: bool,
    },
    /// Escape was pressed alone while recording a new shortcut.
    CaptureCancelled,
}

/// What to do with the event that was just fed in.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Decision {
    /// Keep the event from the frontmost application.
    pub swallow: bool,
    pub out: Vec<Out>,
}

/// Follows the modifiers held and decides when a shortcut is down. Other
/// keys are not tracked between events: a key completes a shortcut at the
/// moment it goes down, together with the modifiers held then, so a missed
/// key-up cannot leave the matcher stuck.
#[derive(Debug, Default)]
pub struct Matcher {
    shortcuts: Vec<Shortcut>,
    held: BTreeSet<Key>,
    /// The shortcut that is down.
    active: Option<Shortcut>,
    /// Keys whose key-down was swallowed, so their repeats and key-up are too.
    swallowed: BTreeSet<Key>,
    capture: Option<Capture>,
    /// Escape cancels a dictation right now, so the press is kept from
    /// the application in front; otherwise Escape is the application's.
    escape_cancels: bool,
}

#[derive(Debug, Default)]
struct Capture {
    /// Non-modifier keys held while recording.
    down: BTreeSet<Key>,
    /// The largest combination seen since nothing was held.
    best: Vec<Key>,
}

impl Matcher {
    pub fn new(shortcuts: Vec<Shortcut>) -> Matcher {
        Matcher {
            shortcuts,
            ..Matcher::default()
        }
    }

    /// Replaces the shortcuts. One that is down is let go first.
    pub fn set_shortcuts(&mut self, shortcuts: Vec<Shortcut>) -> Vec<Out> {
        self.shortcuts = shortcuts;
        self.release()
    }

    /// Forgets every key held, as after events were missed.
    pub fn reset(&mut self) -> Vec<Out> {
        self.held.clear();
        self.swallowed.clear();
        if let Some(c) = &mut self.capture {
            *c = Capture::default();
        }
        self.release()
    }

    fn release(&mut self) -> Vec<Out> {
        match self.active.take() {
            Some(_) => vec![Out::Up],
            None => Vec::new(),
        }
    }

    /// Starts or ends recording a new shortcut. While it runs no shortcut
    /// triggers and every key is kept from the application.
    pub fn set_capture(&mut self, on: bool) -> Vec<Out> {
        self.capture = on.then(Capture::default);
        self.release()
    }

    pub fn capturing(&self) -> bool {
        self.capture.is_some()
    }

    /// Whether an Escape press would cancel a dictation now. While it
    /// does, the press and its release are swallowed.
    pub fn set_escape_cancels(&mut self, on: bool) {
        self.escape_cancels = on;
    }

    /// The modifiers held now, all of them. Called whenever one changes.
    pub fn modifiers(&mut self, now: &[Key]) -> Vec<Out> {
        let now: BTreeSet<Key> = now.iter().copied().collect();
        let pressed: Vec<Key> = now.difference(&self.held).copied().collect();
        let let_go: Vec<Key> = self.held.difference(&now).copied().collect();
        self.held = now;
        if self.capture.is_some() {
            return self.capture_step();
        }
        let mut out = Vec::new();
        if let Some(active) = &self.active {
            if let_go.iter().any(|k| active.keys().contains(k)) {
                self.active = None;
                out.push(Out::Up);
            }
        }
        if !pressed.is_empty() {
            let held: Vec<Key> = self.held.iter().copied().collect();
            out.extend(self.activate(&held));
        }
        out
    }

    /// Makes the shortcut with exactly these keys the one that is down.
    fn activate(&mut self, keys: &[Key]) -> Vec<Out> {
        let candidate = Shortcut::new(keys);
        if !self.shortcuts.contains(&candidate) || self.active.as_ref() == Some(&candidate) {
            return Vec::new();
        }
        let mut out = Vec::new();
        if self.active.is_some() {
            // A longer shortcut takes over from the one pressed on the way to it.
            out.extend([Out::Other, Out::Up]);
        }
        self.active = Some(candidate);
        out.push(Out::Down);
        out
    }

    pub fn key_down(&mut self, key: Key, repeat: bool) -> Decision {
        if self.capture.is_some() {
            return self.capture_down(key, repeat);
        }
        if repeat {
            return Decision {
                swallow: self.swallowed.contains(&key),
                out: Vec::new(),
            };
        }
        if key.is_escape() {
            if self.escape_cancels {
                self.swallowed.insert(key);
            }
            return Decision {
                swallow: self.escape_cancels,
                out: vec![Out::Escape],
            };
        }
        let mut keys: Vec<Key> = self.held.iter().copied().collect();
        keys.push(key);
        let out = self.activate(&keys);
        if !out.is_empty() {
            self.swallowed.insert(key);
            return Decision { swallow: true, out };
        }
        // The key of the shortcut that is down, pressed again without a repeat flag.
        if self.active.as_ref().and_then(Shortcut::main) == Some(key) {
            return Decision {
                swallow: true,
                out: Vec::new(),
            };
        }
        // A mouse button that is not bound is no concern of a dictation.
        if key.is_mouse() {
            return Decision::default();
        }
        Decision {
            swallow: false,
            out: vec![Out::Other],
        }
    }

    pub fn key_up(&mut self, key: Key) -> Decision {
        if let Some(c) = &mut self.capture {
            c.down.remove(&key);
            return Decision {
                swallow: true,
                out: self.capture_step(),
            };
        }
        let swallow = self.swallowed.remove(&key);
        let mut out = Vec::new();
        if self.active.as_ref().and_then(Shortcut::main) == Some(key) {
            self.active = None;
            out.push(Out::Up);
        }
        Decision { swallow, out }
    }

    fn capture_down(&mut self, key: Key, repeat: bool) -> Decision {
        let nothing_held = self.held.is_empty();
        let c = self.capture.as_mut().expect("capturing");
        if repeat {
            return Decision {
                swallow: true,
                out: Vec::new(),
            };
        }
        if key.is_escape() && nothing_held && c.down.is_empty() {
            return Decision {
                swallow: true,
                out: vec![Out::CaptureCancelled],
            };
        }
        c.down.insert(key);
        Decision {
            swallow: true,
            out: self.capture_step(),
        }
    }

    /// Reports the keys held while recording; when the last one is let go,
    /// the largest combination that was held.
    fn capture_step(&mut self) -> Vec<Out> {
        let c = self.capture.as_mut().expect("capturing");
        let keys: Vec<Key> = self.held.iter().chain(c.down.iter()).copied().collect();
        if keys.is_empty() {
            let best = std::mem::take(&mut c.best);
            return match best.is_empty() {
                true => Vec::new(),
                false => vec![Out::Capture {
                    keys: Shortcut::new(&best).0,
                    done: true,
                }],
            };
        }
        if keys.len() >= c.best.len() {
            c.best = keys.clone();
        }
        vec![Out::Capture {
            keys: Shortcut::new(&keys).0,
            done: false,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(name: &str) -> Key {
        Key::named(name).unwrap()
    }
    fn keys(names: &[&str]) -> Vec<Key> {
        names.iter().map(|n| k(n)).collect()
    }
    fn sc(names: &[&str]) -> Shortcut {
        Shortcut::new(&keys(names))
    }
    fn check(names: &[&str]) -> Option<Refusal> {
        refusal(&keys(names), &[])
    }

    #[test]
    fn key_names_round_trip_and_unknown_names_fail() {
        for name in NAMES {
            assert_eq!(k(name).name(), *name);
        }
        let s: Shortcut = serde_json::from_str(r#"["KeyD","MetaLeft"]"#).unwrap();
        assert_eq!(serde_json::to_string(&s).unwrap(), r#"["KeyD","MetaLeft"]"#);
        assert!(serde_json::from_str::<Shortcut>(r#"["Hyper"]"#).is_err());
    }

    #[test]
    fn a_shortcut_shows_modifiers_first_whatever_the_order_pressed() {
        assert_eq!(
            sc(&["KeyD", "MetaLeft", "Fn"]).keys(),
            keys(&["Fn", "MetaLeft", "KeyD"])
        );
        assert_eq!(sc(&["MetaLeft", "MetaLeft"]).keys(), keys(&["MetaLeft"]));
    }

    #[test]
    fn allowed_combinations() {
        for ok in [
            vec!["Fn"],
            vec!["AltRight"],
            vec!["MetaLeft", "MetaRight"],
            vec!["ControlLeft", "AltLeft"],
            vec!["MetaLeft", "KeyD"],
            vec!["ControlLeft", "AltLeft", "Space"],
            vec!["AltLeft", "F1"],
            vec!["ShiftLeft", "F5"],
            vec!["Fn", "ArrowLeft"],
            vec!["MouseMiddle"],
            vec!["Mouse4"],
            vec!["ShiftLeft", "Mouse5"],
            vec!["MetaRight", "Numpad1"],
        ] {
            assert_eq!(check(&ok), None, "{ok:?}");
        }
    }

    #[test]
    fn refused_combinations_say_why() {
        for (combo, why) in [
            (vec![], Refusal::Empty),
            (vec!["KeyK"], Refusal::CharacterAlone),
            (vec!["Digit1"], Refusal::CharacterAlone),
            (vec!["Space"], Refusal::CharacterAlone),
            (vec!["Comma"], Refusal::CharacterAlone),
            (vec!["Numpad5"], Refusal::CharacterAlone),
            (vec!["Enter"], Refusal::EditingAlone),
            (vec!["ArrowUp"], Refusal::EditingAlone),
            (vec!["Backspace"], Refusal::EditingAlone),
            (vec!["F5"], Refusal::FunctionAlone),
            (vec!["F13"], Refusal::FunctionAlone),
            (vec!["ShiftLeft", "KeyK"], Refusal::ShiftOnly),
            (vec!["ShiftLeft", "ShiftRight", "KeyK"], Refusal::ShiftOnly),
            (vec!["ShiftRight", "ArrowLeft"], Refusal::ShiftOnly),
            (vec!["Escape"], Refusal::Escape),
            (vec!["MetaLeft", "AltLeft", "Escape"], Refusal::Escape),
            (
                vec!["ControlLeft", "AltLeft", "ShiftLeft", "KeyK"],
                Refusal::TooManyKeys,
            ),
            (vec!["MetaLeft", "KeyD", "KeyK"], Refusal::TwoKeys),
            (vec!["MetaLeft", "KeyA"], Refusal::Reserved),
            (vec!["MetaRight", "KeyV"], Refusal::Reserved),
            (vec!["ShiftLeft", "MetaLeft", "KeyZ"], Refusal::Reserved),
            (vec!["MetaLeft", "Space"], Refusal::Reserved),
            (vec!["ControlRight", "Space"], Refusal::Reserved),
            (vec!["ControlLeft", "MetaLeft", "KeyF"], Refusal::Reserved),
        ] {
            assert_eq!(check(&combo), Some(why), "{combo:?}");
        }
        // The reserved list names exact modifier sets.
        assert_eq!(check(&["AltLeft", "MetaLeft", "KeyA"]), None);
        assert_eq!(check(&["MetaLeft", "KeyD"]), None);
    }

    #[test]
    fn a_duplicate_is_refused_and_the_list_has_no_limit() {
        let have = vec![sc(&["Fn"]), sc(&["MetaLeft", "KeyD"])];
        assert_eq!(
            refusal(&keys(&["KeyD", "MetaLeft"]), &have),
            Some(Refusal::Duplicate)
        );
        assert_eq!(refusal(&keys(&["MetaRight", "KeyD"]), &have), None);
        let many = vec![
            sc(&["Fn"]),
            sc(&["AltRight"]),
            sc(&["MetaRight"]),
            sc(&["ControlRight"]),
            sc(&["ShiftRight"]),
            sc(&["MetaLeft", "KeyD"]),
        ];
        assert_eq!(refusal(&keys(&["AltLeft"]), &many), None);
        assert_eq!(sanitized(&many), many);
    }

    #[test]
    fn a_stored_shortcut_with_an_unknown_key_is_left_out() {
        #[derive(Deserialize)]
        struct Stored {
            #[serde(deserialize_with = "lenient")]
            list: Vec<Shortcut>,
        }
        let s: Stored = serde_json::from_str(
            r#"{"list":[["Fn"],["CapsLock"],["MetaLeft","Hyper"],["AltRight"]]}"#,
        )
        .unwrap();
        assert_eq!(s.list, vec![sc(&["Fn"]), sc(&["AltRight"])]);
        assert!(Key::named("CapsLock").is_none());
    }

    #[test]
    fn a_stored_list_is_cleaned_and_never_empty() {
        let list = vec![sc(&["KeyK"]), sc(&["Fn"]), sc(&["Fn"]), sc(&["AltRight"])];
        assert_eq!(sanitized(&list), vec![sc(&["Fn"]), sc(&["AltRight"])]);
        assert_eq!(sanitized(&[sc(&["KeyK"])]), vec![Shortcut::fn_key()]);
        assert_eq!(sanitized(&[]), vec![Shortcut::fn_key()]);
    }

    fn matcher(list: &[&[&str]]) -> Matcher {
        Matcher::new(list.iter().map(|s| sc(s)).collect())
    }

    #[test]
    fn a_single_modifier_goes_down_and_up_and_is_never_swallowed() {
        let mut m = matcher(&[&["Fn"], &["AltRight"]]);
        assert_eq!(m.modifiers(&keys(&["Fn"])), vec![Out::Down]);
        assert_eq!(m.modifiers(&[]), vec![Out::Up]);
        assert_eq!(m.modifiers(&keys(&["AltRight"])), vec![Out::Down]);
        assert_eq!(m.modifiers(&[]), vec![Out::Up]);
        // The other side is another key.
        assert_eq!(m.modifiers(&keys(&["AltLeft"])), vec![]);
        assert_eq!(m.modifiers(&[]), vec![]);
    }

    #[test]
    fn another_key_while_a_modifier_shortcut_is_down_is_reported_and_passes() {
        let mut m = matcher(&[&["MetaLeft"]]);
        assert_eq!(m.modifiers(&keys(&["MetaLeft"])), vec![Out::Down]);
        assert_eq!(
            m.key_down(k("KeyC"), false),
            Decision {
                swallow: false,
                out: vec![Out::Other]
            }
        );
        assert_eq!(m.key_up(k("KeyC")), Decision::default());
        assert_eq!(m.modifiers(&[]), vec![Out::Up]);
    }

    #[test]
    fn a_key_combination_is_swallowed_with_its_repeats_and_release() {
        let mut m = matcher(&[&["MetaLeft", "KeyD"]]);
        assert_eq!(m.modifiers(&keys(&["MetaLeft"])), vec![]);
        assert_eq!(
            m.key_down(k("KeyD"), false),
            Decision {
                swallow: true,
                out: vec![Out::Down]
            }
        );
        assert_eq!(
            m.key_down(k("KeyD"), true),
            Decision {
                swallow: true,
                out: vec![]
            }
        );
        assert_eq!(
            m.key_up(k("KeyD")),
            Decision {
                swallow: true,
                out: vec![Out::Up]
            }
        );
        // With another modifier held it is a different combination.
        assert_eq!(m.modifiers(&keys(&["MetaLeft", "ShiftLeft"])), vec![]);
        assert_eq!(
            m.key_down(k("KeyD"), false),
            Decision {
                swallow: false,
                out: vec![Out::Other]
            }
        );
    }

    #[test]
    fn letting_go_of_the_modifier_first_ends_the_shortcut_and_still_swallows_the_key_up() {
        let mut m = matcher(&[&["MetaLeft", "KeyD"]]);
        m.modifiers(&keys(&["MetaLeft"]));
        m.key_down(k("KeyD"), false);
        assert_eq!(m.modifiers(&[]), vec![Out::Up]);
        assert_eq!(
            m.key_up(k("KeyD")),
            Decision {
                swallow: true,
                out: vec![]
            }
        );
        // The matcher is ready for the next press.
        m.modifiers(&keys(&["MetaLeft"]));
        assert_eq!(m.key_down(k("KeyD"), false).out, vec![Out::Down]);
    }

    #[test]
    fn a_longer_shortcut_takes_over_from_a_shorter_one() {
        let mut m = matcher(&[
            &["AltRight"],
            &["AltRight", "KeyD"],
            &["Fn"],
            &["Fn", "ControlLeft"],
        ]);
        assert_eq!(m.modifiers(&keys(&["AltRight"])), vec![Out::Down]);
        assert_eq!(
            m.key_down(k("KeyD"), false),
            Decision {
                swallow: true,
                out: vec![Out::Other, Out::Up, Out::Down]
            }
        );
        assert_eq!(m.key_up(k("KeyD")).out, vec![Out::Up]);
        // Back on the shorter set, nothing starts by itself.
        assert_eq!(m.modifiers(&[]), vec![]);

        assert_eq!(m.modifiers(&keys(&["Fn"])), vec![Out::Down]);
        assert_eq!(
            m.modifiers(&keys(&["Fn", "ControlLeft"])),
            vec![Out::Other, Out::Up, Out::Down]
        );
        assert_eq!(m.modifiers(&keys(&["Fn"])), vec![Out::Up]);
        assert_eq!(m.modifiers(&[]), vec![]);
    }

    #[test]
    fn both_sides_of_a_modifier_together_are_one_shortcut() {
        let mut m = matcher(&[&["MetaLeft", "MetaRight"]]);
        assert_eq!(m.modifiers(&keys(&["MetaLeft"])), vec![]);
        assert_eq!(
            m.modifiers(&keys(&["MetaLeft", "MetaRight"])),
            vec![Out::Down]
        );
        assert_eq!(m.modifiers(&keys(&["MetaRight"])), vec![Out::Up]);
    }

    #[test]
    fn a_mouse_button_is_a_shortcut_alone_or_with_a_modifier() {
        let mut m = matcher(&[&["MouseMiddle"], &["ShiftLeft", "Mouse4"]]);
        assert_eq!(
            m.key_down(k("MouseMiddle"), false),
            Decision {
                swallow: true,
                out: vec![Out::Down]
            }
        );
        assert_eq!(
            m.key_up(k("MouseMiddle")),
            Decision {
                swallow: true,
                out: vec![Out::Up]
            }
        );
        assert_eq!(m.key_down(k("Mouse4"), false), Decision::default());
        m.key_up(k("Mouse4"));
        m.modifiers(&keys(&["ShiftLeft"]));
        assert_eq!(m.key_down(k("Mouse4"), false).out, vec![Out::Down]);
    }

    #[test]
    fn escape_is_reported_and_reaches_the_application_when_it_cancels_nothing() {
        let mut m = matcher(&[&["Fn"]]);
        assert_eq!(
            m.key_down(k("Escape"), false),
            Decision {
                swallow: false,
                out: vec![Out::Escape]
            }
        );
        assert_eq!(m.key_up(k("Escape")), Decision::default());
    }

    #[test]
    fn escape_is_swallowed_with_its_release_while_it_cancels_a_dictation() {
        let mut m = matcher(&[&["Fn"]]);
        m.set_escape_cancels(true);
        assert_eq!(
            m.key_down(k("Escape"), false),
            Decision {
                swallow: true,
                out: vec![Out::Escape]
            }
        );
        // The dictation is cancelled by the press; the release is still
        // kept from the application even after the state went back.
        m.set_escape_cancels(false);
        assert_eq!(
            m.key_down(k("Escape"), true),
            Decision {
                swallow: true,
                out: Vec::new()
            }
        );
        assert_eq!(
            m.key_up(k("Escape")),
            Decision {
                swallow: true,
                out: Vec::new()
            }
        );
        assert_eq!(
            m.key_down(k("Escape"), false),
            Decision {
                swallow: false,
                out: vec![Out::Escape]
            }
        );
    }

    #[test]
    fn new_shortcuts_and_a_reset_let_go_of_the_one_that_is_down() {
        let mut m = matcher(&[&["Fn"]]);
        m.modifiers(&keys(&["Fn"]));
        assert_eq!(m.set_shortcuts(vec![sc(&["AltRight"])]), vec![Out::Up]);
        m.modifiers(&[]);
        assert_eq!(m.modifiers(&keys(&["AltRight"])), vec![Out::Down]);
        assert_eq!(m.reset(), vec![Out::Up]);
        assert_eq!(m.reset(), vec![]);
    }

    #[test]
    fn recording_reports_what_is_held_and_then_the_whole_combination() {
        let mut m = matcher(&[&["Fn"]]);
        m.set_capture(true);
        let held = |names: &[&str]| {
            vec![Out::Capture {
                keys: keys(names),
                done: false,
            }]
        };
        assert_eq!(m.modifiers(&keys(&["MetaLeft"])), held(&["MetaLeft"]));
        assert_eq!(
            m.key_down(k("KeyD"), false),
            Decision {
                swallow: true,
                out: held(&["MetaLeft", "KeyD"])
            }
        );
        assert_eq!(m.key_up(k("KeyD")).out, held(&["MetaLeft"]));
        assert_eq!(
            m.modifiers(&[]),
            vec![Out::Capture {
                keys: keys(&["MetaLeft", "KeyD"]),
                done: true
            }]
        );
        // A shortcut that is set does not trigger while recording.
        assert_eq!(m.modifiers(&keys(&["Fn"])), held(&["Fn"]));
        assert_eq!(
            m.modifiers(&[]),
            vec![Out::Capture {
                keys: keys(&["Fn"]),
                done: true
            }]
        );
        m.set_capture(false);
        assert_eq!(m.modifiers(&keys(&["Fn"])), vec![Out::Down]);
    }

    #[test]
    fn escape_alone_cancels_recording_and_is_part_of_a_combination_otherwise() {
        let mut m = matcher(&[]);
        m.set_capture(true);
        assert_eq!(
            m.key_down(k("Escape"), false),
            Decision {
                swallow: true,
                out: vec![Out::CaptureCancelled]
            }
        );
        m.modifiers(&keys(&["MetaLeft"]));
        let d = m.key_down(k("Escape"), false);
        assert_eq!(
            d.out,
            vec![Out::Capture {
                keys: keys(&["MetaLeft", "Escape"]),
                done: false
            }]
        );
    }
}
