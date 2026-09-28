//! Carries out a `Plan` against the platform.

use std::time::Duration;

use crate::insert::planner::{Plan, ShowReason};
use crate::platform::Platform;

/// Time given to the target app to read the pasteboard before it is restored.
pub const PASTE_RESTORE_DELAY: Duration = Duration::from_millis(250);
/// CGEventKeyboardSetUnicodeString accepts at most this many UTF-16 units.
pub const MAX_UNITS_PER_EVENT: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Inserted,
    Shown(ShowReason),
}

pub fn run(plan: &Plan, text: &str, platform: &dyn Platform) -> Outcome {
    match plan {
        Plan::Show(reason) => Outcome::Shown(*reason),
        Plan::Paste => {
            paste(text, platform);
            Outcome::Inserted
        }
        Plan::Type => {
            platform.type_unicode(text);
            Outcome::Inserted
        }
        Plan::Ax { pid } => {
            if let Err(e) = platform.ax_insert(*pid, text) {
                tracing::debug!(error = %e, "ax insert failed, pasting instead");
                paste(text, platform);
            }
            Outcome::Inserted
        }
    }
}

fn paste(text: &str, platform: &dyn Platform) {
    let snapshot = platform.clipboard_snapshot();
    platform.clipboard_write_text(text);
    platform.post_paste();
    std::thread::sleep(PASTE_RESTORE_DELAY);
    platform.clipboard_restore(snapshot);
}

/// Splits into runs of at most `MAX_UNITS_PER_EVENT` UTF-16 units without
/// separating a surrogate pair.
pub fn chunk_utf16(text: &str) -> Vec<Vec<u16>> {
    let mut chunks: Vec<Vec<u16>> = Vec::new();
    let mut current: Vec<u16> = Vec::with_capacity(MAX_UNITS_PER_EVENT);
    for ch in text.chars() {
        let mut buf = [0u16; 2];
        let units = ch.encode_utf16(&mut buf);
        if current.len() + units.len() > MAX_UNITS_PER_EVENT {
            chunks.push(std::mem::take(&mut current));
        }
        current.extend_from_slice(units);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::fake::FakePlatform;
    use crate::platform::PlatformError;

    #[test]
    fn chunks_never_split_surrogate_pairs() {
        let text = "a".repeat(19) + "😀" + &"b".repeat(5);
        let chunks = chunk_utf16(&text);
        assert_eq!(chunks[0].len(), 19);
        assert_eq!(
            chunks[1].len(),
            7,
            "emoji (2 units) starts the second chunk"
        );
        for c in &chunks {
            assert!(c.len() <= MAX_UNITS_PER_EVENT);
            let decoded = String::from_utf16(c).expect("each chunk is valid UTF-16");
            assert!(!decoded.contains('\u{FFFD}'));
        }
        assert_eq!(String::from_utf16(&chunks.concat()).unwrap(), text);
        assert!(chunk_utf16("").is_empty());
        assert_eq!(chunk_utf16("你好").len(), 1);
    }

    #[test]
    fn paste_writes_posts_and_restores() {
        let p = FakePlatform::new();
        *p.clipboard.lock().unwrap() = Some("before".into());
        assert_eq!(run(&Plan::Paste, "你好", &p), Outcome::Inserted);
        assert_eq!(
            p.calls(),
            vec!["snapshot", "write:你好", "paste", "restore"]
        );
        assert_eq!(p.clipboard.lock().unwrap().as_deref(), Some("before"));
    }

    #[test]
    fn type_uses_unicode_typing_only() {
        let p = FakePlatform::new();
        assert_eq!(run(&Plan::Type, "hi", &p), Outcome::Inserted);
        assert_eq!(p.calls(), vec!["type:hi"]);
    }

    #[test]
    fn ax_falls_back_to_paste_on_error() {
        let p = FakePlatform::new();
        *p.ax_insert_failure.lock().unwrap() = Some(PlatformError::Ax(-25204));
        assert_eq!(run(&Plan::Ax { pid: 1 }, "x", &p), Outcome::Inserted);
        assert_eq!(
            p.calls(),
            vec!["ax:x", "snapshot", "write:x", "paste", "restore"]
        );
        let ok = FakePlatform::new();
        run(&Plan::Ax { pid: 1 }, "x", &ok);
        assert_eq!(ok.calls(), vec!["ax:x"]);
    }

    #[test]
    fn show_touches_nothing() {
        let p = FakePlatform::new();
        assert_eq!(
            run(&Plan::Show(ShowReason::SecureInput), "x", &p),
            Outcome::Shown(ShowReason::SecureInput)
        );
        assert!(p.calls().is_empty());
    }
}
