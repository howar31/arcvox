//! Pure decision: given what we know, insert how, or show the text instead.

use crate::context::InsertTarget;
use crate::platform::FrontmostApp;
use crate::settings::InsertMethod;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    Paste,
    Type,
    /// AX selected-text insertion; the executor falls back to paste on error.
    Ax {
        pid: i32,
    },
    /// Do not insert; show the text with a Copy button and say why.
    Show(ShowReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowReason {
    SecureInput,
    AppChanged,
    EmptyText,
}

pub struct Situation<'a> {
    pub text: &'a str,
    pub target: &'a InsertTarget,
    pub frontmost: Option<&'a FrontmostApp>,
    pub secure_input: bool,
    pub method: InsertMethod,
}

pub fn plan(s: &Situation<'_>) -> Plan {
    if s.text.trim().is_empty() {
        return Plan::Show(ShowReason::EmptyText);
    }
    if s.secure_input {
        return Plan::Show(ShowReason::SecureInput);
    }
    let same_app = match s.frontmost {
        Some(f) => {
            f.pid == s.target.pid || (f.bundle_id.is_some() && f.bundle_id == s.target.bundle_id)
        }
        None => false,
    };
    if !same_app {
        return Plan::Show(ShowReason::AppChanged);
    }
    match s.method {
        InsertMethod::Paste => Plan::Paste,
        InsertMethod::Type => Plan::Type,
        InsertMethod::Ax => Plan::Ax { pid: s.target.pid },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> InsertTarget {
        InsertTarget {
            pid: 7,
            bundle_id: Some("com.apple.Terminal".into()),
        }
    }

    fn front(pid: i32, id: &str) -> FrontmostApp {
        FrontmostApp {
            pid,
            bundle_id: Some(id.into()),
            ..Default::default()
        }
    }

    fn sit<'a>(
        text: &'a str,
        target: &'a InsertTarget,
        front: Option<&'a FrontmostApp>,
        secure: bool,
        method: InsertMethod,
    ) -> Situation<'a> {
        Situation {
            text,
            target,
            frontmost: front,
            secure_input: secure,
            method,
        }
    }

    #[test]
    fn default_is_paste_into_the_same_app() {
        let t = target();
        let f = front(7, "com.apple.Terminal");
        assert_eq!(
            plan(&sit("你好", &t, Some(&f), false, InsertMethod::Paste)),
            Plan::Paste
        );
    }

    #[test]
    fn empty_text_is_shown_not_inserted() {
        let t = target();
        let f = front(7, "com.apple.Terminal");
        assert_eq!(
            plan(&sit("  \n", &t, Some(&f), false, InsertMethod::Paste)),
            Plan::Show(ShowReason::EmptyText)
        );
    }

    #[test]
    fn secure_input_wins_over_everything_but_empty_text() {
        let t = target();
        let f = front(7, "com.apple.Terminal");
        assert_eq!(
            plan(&sit("x", &t, Some(&f), true, InsertMethod::Type)),
            Plan::Show(ShowReason::SecureInput)
        );
    }

    #[test]
    fn app_change_is_detected_by_pid_or_bundle_id() {
        let t = target();
        let other = front(9, "com.google.Chrome");
        assert_eq!(
            plan(&sit("x", &t, Some(&other), false, InsertMethod::Paste)),
            Plan::Show(ShowReason::AppChanged)
        );
        let relaunched = front(99, "com.apple.Terminal");
        assert_eq!(
            plan(&sit("x", &t, Some(&relaunched), false, InsertMethod::Paste)),
            Plan::Paste
        );
        assert_eq!(
            plan(&sit("x", &t, None, false, InsertMethod::Paste)),
            Plan::Show(ShowReason::AppChanged)
        );
    }

    #[test]
    fn per_app_methods_are_honoured() {
        let t = target();
        let f = front(7, "com.apple.Terminal");
        assert_eq!(
            plan(&sit("x", &t, Some(&f), false, InsertMethod::Type)),
            Plan::Type
        );
        assert_eq!(
            plan(&sit("x", &t, Some(&f), false, InsertMethod::Ax)),
            Plan::Ax { pid: 7 }
        );
    }
}
