//! What the app does when the Accessibility trust is granted or taken away
//! while it runs. The platform-specific watching is in `app.rs`.

use crate::dictation::ui::UiState;

/// A change of the trust that needs an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// The trust arrived: install the shortcut monitor.
    Install,
    /// The trust went away: stop the shortcut monitor at once. A session
    /// event tap that keeps running without the trust holds every keyboard
    /// event of the system until the tap times out.
    Suspend,
}

/// Follows the trust and names the action a change calls for.
#[derive(Debug)]
pub struct Trust {
    trusted: bool,
}

impl Trust {
    pub fn new(trusted: bool) -> Self {
        Trust { trusted }
    }

    pub fn trusted(&self) -> bool {
        self.trusted
    }

    /// Records the trust seen now; the action when it differs from before.
    pub fn observe(&mut self, trusted: bool) -> Option<Action> {
        if trusted == self.trusted {
            return None;
        }
        self.trusted = trusted;
        Some(if trusted {
            Action::Install
        } else {
            Action::Suspend
        })
    }
}

/// The state the menu-bar status shows for the trust. None while the engine
/// never opened: `EngineError` stays on screen, whatever the trust does.
pub fn state_for(trusted: bool, engine_ready: bool) -> Option<UiState> {
    if !engine_ready {
        return None;
    }
    Some(if trusted {
        UiState::Idle
    } else {
        UiState::NoAccessibility
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_change_of_the_trust_is_an_action() {
        let mut t = Trust::new(true);
        assert_eq!(t.observe(true), None);
        assert_eq!(t.observe(false), Some(Action::Suspend));
        assert!(!t.trusted());
        assert_eq!(t.observe(false), None);
        assert_eq!(t.observe(true), Some(Action::Install));
        assert!(t.trusted());
    }

    #[test]
    fn the_status_follows_the_trust_unless_the_engine_never_opened() {
        assert_eq!(state_for(true, true), Some(UiState::Idle));
        assert_eq!(state_for(false, true), Some(UiState::NoAccessibility));
        assert_eq!(state_for(true, false), None);
        assert_eq!(state_for(false, false), None);
    }
}
