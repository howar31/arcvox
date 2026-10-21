//! Turns fn key up/down events into gestures. Pure logic: the caller feeds
//! timestamps and polls `next_deadline` so the hold threshold fires without
//! waiting for the next key event.

use std::time::{Duration, Instant};

use crate::settings::HotkeySettings;

/// Virtual key code of Escape on Apple keyboards.
pub const KEY_ESCAPE: u16 = 53;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    TriggerDown,
    TriggerUp,
    /// Any other key went down. `key_code` is the virtual key code.
    KeyDown {
        key_code: u16,
    },
    /// Escape went down (already separated from `KeyDown` by the caller).
    Escape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    /// fn held past the threshold: start push-to-talk recording.
    HoldStart,
    /// fn released after a `HoldStart`. `keyed`: other keys were pressed
    /// during the hold, so it served a key combination as well.
    HoldEnd { keyed: bool },
    /// A short press (below the threshold), not part of a double tap.
    Tap,
    /// Two short presses within the window.
    DoubleTap,
    /// A key was pressed while fn was down and the hold had not started, or
    /// Escape was pressed: abort whatever fn was doing.
    Abort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    /// fn is down, threshold not yet reached.
    Pending {
        since: Instant,
    },
    /// `HoldStart` emitted; waiting for fn up. `keyed`: another key was
    /// pressed meanwhile.
    Holding {
        keyed: bool,
    },
    /// One tap seen; a second within the window is a double tap.
    TapWait {
        until: Instant,
    },
    /// fn is down but the gesture was aborted; wait for fn up.
    Aborted,
}

pub struct Recognizer {
    hold: Duration,
    double_tap: Duration,
    state: State,
    fn_down: bool,
}

impl Recognizer {
    pub fn new(settings: &HotkeySettings) -> Self {
        Self {
            hold: Duration::from_millis(settings.hold_threshold_ms),
            double_tap: Duration::from_millis(settings.double_tap_window_ms),
            state: State::Idle,
            fn_down: false,
        }
    }

    /// Takes new thresholds and keeps the state, so that settings saved
    /// while the trigger is held do not lose its release.
    pub fn retune(&mut self, settings: &HotkeySettings) {
        self.hold = Duration::from_millis(settings.hold_threshold_ms);
        self.double_tap = Duration::from_millis(settings.double_tap_window_ms);
    }

    pub fn fn_is_down(&self) -> bool {
        self.fn_down
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        match self.state {
            State::Pending { since } => Some(since + self.hold),
            State::TapWait { until } => Some(until),
            _ => None,
        }
    }

    pub fn on_input(&mut self, input: Input, now: Instant) -> Vec<Gesture> {
        let mut out = self.on_tick(now);
        match input {
            Input::TriggerDown => {
                if self.fn_down {
                    return out;
                }
                self.fn_down = true;
                self.state = match self.state {
                    State::TapWait { .. } => {
                        out.push(Gesture::DoubleTap);
                        State::Aborted
                    }
                    _ => State::Pending { since: now },
                };
            }
            Input::TriggerUp => {
                if !self.fn_down {
                    return out;
                }
                self.fn_down = false;
                self.state = match self.state {
                    State::Pending { .. } => State::TapWait {
                        until: now + self.double_tap,
                    },
                    State::Holding { keyed } => {
                        out.push(Gesture::HoldEnd { keyed });
                        State::Idle
                    }
                    State::Aborted | State::Idle | State::TapWait { .. } => State::Idle,
                };
            }
            Input::KeyDown { .. } | Input::Escape => {
                if self.fn_down {
                    match self.state {
                        // Once the hold is on, other keys go to the
                        // application and the recording goes on.
                        State::Holding { .. } if input != Input::Escape => {
                            self.state = State::Holding { keyed: true };
                        }
                        State::Pending { .. } | State::Holding { .. } => {
                            out.push(Gesture::Abort);
                            self.state = State::Aborted;
                        }
                        _ => self.state = State::Aborted,
                    }
                } else if let State::TapWait { .. } = self.state {
                    self.state = State::Idle;
                }
            }
        }
        out
    }

    /// Ends a short press as a tap right away instead of waiting for the
    /// double-tap window, for a state in which a second tap means nothing.
    pub fn tap_now(&mut self) -> Option<Gesture> {
        match self.state {
            State::TapWait { .. } => {
                self.state = State::Idle;
                Some(Gesture::Tap)
            }
            _ => None,
        }
    }

    pub fn on_tick(&mut self, now: Instant) -> Vec<Gesture> {
        let mut out = Vec::new();
        match self.state {
            State::Pending { since } if now >= since + self.hold => {
                self.state = State::Holding { keyed: false };
                out.push(Gesture::HoldStart);
            }
            State::TapWait { until } if now >= until => {
                self.state = State::Idle;
                out.push(Gesture::Tap);
            }
            _ => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec() -> (Recognizer, Instant) {
        (Recognizer::new(&HotkeySettings::default()), Instant::now())
    }

    fn ms(t0: Instant, n: u64) -> Instant {
        t0 + Duration::from_millis(n)
    }

    #[test]
    fn hold_emits_start_at_threshold_and_end_on_release() {
        let (mut r, t0) = rec();
        assert!(r.on_input(Input::TriggerDown, t0).is_empty());
        assert_eq!(r.next_deadline(), Some(ms(t0, 250)));
        assert!(r.on_tick(ms(t0, 100)).is_empty());
        assert_eq!(r.on_tick(ms(t0, 250)), vec![Gesture::HoldStart]);
        assert_eq!(r.next_deadline(), None);
        assert_eq!(
            r.on_input(Input::TriggerUp, ms(t0, 900)),
            vec![Gesture::HoldEnd { keyed: false }]
        );
        assert!(!r.fn_is_down());
    }

    #[test]
    fn hold_start_fires_from_the_release_event_when_no_tick_ran() {
        let (mut r, t0) = rec();
        r.on_input(Input::TriggerDown, t0);
        assert_eq!(
            r.on_input(Input::TriggerUp, ms(t0, 600)),
            vec![Gesture::HoldStart, Gesture::HoldEnd { keyed: false }]
        );
    }

    #[test]
    fn a_waiting_tap_can_be_ended_at_once() {
        let (mut r, t0) = rec();
        assert!(r.tap_now().is_none());
        r.on_input(Input::TriggerDown, t0);
        assert!(r.tap_now().is_none());
        assert!(r.on_input(Input::TriggerUp, ms(t0, 50)).is_empty());
        assert_eq!(r.tap_now(), Some(Gesture::Tap));
        assert_eq!(r.next_deadline(), None);
        assert!(r.on_tick(ms(t0, 1000)).is_empty());
        // The next press starts over: it is not the second half of a double tap.
        assert!(r.on_input(Input::TriggerDown, ms(t0, 100)).is_empty());
        assert!(r.on_input(Input::TriggerUp, ms(t0, 150)).is_empty());
        assert!(r.on_tick(ms(t0, 200)).is_empty());
        assert_eq!(r.on_tick(ms(t0, 2000)), vec![Gesture::Tap]);
    }

    #[test]
    fn short_press_becomes_tap_after_the_window() {
        let (mut r, t0) = rec();
        r.on_input(Input::TriggerDown, t0);
        assert!(r.on_input(Input::TriggerUp, ms(t0, 100)).is_empty());
        assert_eq!(r.next_deadline(), Some(ms(t0, 450)));
        assert!(r.on_tick(ms(t0, 400)).is_empty());
        assert_eq!(r.on_tick(ms(t0, 450)), vec![Gesture::Tap]);
    }

    #[test]
    fn two_short_presses_are_a_double_tap() {
        let (mut r, t0) = rec();
        r.on_input(Input::TriggerDown, t0);
        r.on_input(Input::TriggerUp, ms(t0, 80));
        assert_eq!(
            r.on_input(Input::TriggerDown, ms(t0, 300)),
            vec![Gesture::DoubleTap]
        );
        assert!(r.on_input(Input::TriggerUp, ms(t0, 380)).is_empty());
        assert!(r.on_tick(ms(t0, 2000)).is_empty());
    }

    #[test]
    fn tap_then_hold_is_a_double_tap_not_a_hold() {
        let (mut r, t0) = rec();
        r.on_input(Input::TriggerDown, t0);
        r.on_input(Input::TriggerUp, ms(t0, 80));
        assert_eq!(
            r.on_input(Input::TriggerDown, ms(t0, 200)),
            vec![Gesture::DoubleTap]
        );
        assert!(r.on_tick(ms(t0, 1000)).is_empty());
        assert!(r.on_input(Input::TriggerUp, ms(t0, 1200)).is_empty());
    }

    #[test]
    fn key_press_before_the_threshold_aborts() {
        let (mut r, t0) = rec();
        r.on_input(Input::TriggerDown, t0);
        assert_eq!(
            r.on_input(Input::KeyDown { key_code: 123 }, ms(t0, 50)),
            vec![Gesture::Abort]
        );
        assert!(r.on_input(Input::TriggerUp, ms(t0, 100)).is_empty());
        assert!(r.on_tick(ms(t0, 1000)).is_empty());
    }

    #[test]
    fn key_press_while_holding_keeps_the_hold() {
        let (mut r, t0) = rec();
        r.on_input(Input::TriggerDown, t0);
        assert_eq!(r.on_tick(ms(t0, 250)), vec![Gesture::HoldStart]);
        assert!(r
            .on_input(Input::KeyDown { key_code: 123 }, ms(t0, 500))
            .is_empty());
        assert!(r
            .on_input(Input::KeyDown { key_code: 0 }, ms(t0, 700))
            .is_empty());
        assert_eq!(
            r.on_input(Input::TriggerUp, ms(t0, 900)),
            vec![Gesture::HoldEnd { keyed: true }]
        );
    }

    #[test]
    fn escape_while_holding_aborts() {
        let (mut r, t0) = rec();
        r.on_input(Input::TriggerDown, t0);
        assert_eq!(r.on_tick(ms(t0, 250)), vec![Gesture::HoldStart]);
        assert_eq!(r.on_input(Input::Escape, ms(t0, 500)), vec![Gesture::Abort]);
        assert!(r.on_input(Input::TriggerUp, ms(t0, 600)).is_empty());
    }

    #[test]
    fn key_press_without_fn_clears_a_pending_tap() {
        let (mut r, t0) = rec();
        r.on_input(Input::TriggerDown, t0);
        r.on_input(Input::TriggerUp, ms(t0, 80));
        assert!(r
            .on_input(Input::KeyDown { key_code: 0 }, ms(t0, 100))
            .is_empty());
        assert!(r.on_tick(ms(t0, 1000)).is_empty());
    }

    #[test]
    fn retune_keeps_the_hold_and_the_aborted_press() {
        let (mut r, t0) = rec();
        r.on_input(Input::TriggerDown, t0);
        assert_eq!(r.on_tick(ms(t0, 250)), vec![Gesture::HoldStart]);
        r.retune(&HotkeySettings {
            hold_threshold_ms: 500,
            ..HotkeySettings::default()
        });
        assert!(r.fn_is_down());
        assert_eq!(
            r.on_input(Input::TriggerUp, ms(t0, 900)),
            vec![Gesture::HoldEnd { keyed: false }]
        );

        let (mut r, t0) = rec();
        r.on_input(Input::TriggerDown, t0);
        r.on_input(Input::KeyDown { key_code: 123 }, ms(t0, 50));
        r.retune(&HotkeySettings::default());
        assert!(r.on_input(Input::TriggerUp, ms(t0, 100)).is_empty());
        assert_eq!(r.next_deadline(), None);
        assert!(r.on_input(Input::TriggerDown, ms(t0, 2000)).is_empty());
        assert_eq!(r.next_deadline(), Some(ms(t0, 2250)));
    }

    #[test]
    fn a_key_after_the_threshold_without_a_tick_keeps_the_hold() {
        // No tick ran between the press and the key: the hold starts from
        // the key event itself and the key counts as part of a combination.
        let (mut r, t0) = rec();
        r.on_input(Input::TriggerDown, t0);
        assert_eq!(
            r.on_input(Input::KeyDown { key_code: 123 }, ms(t0, 300)),
            vec![Gesture::HoldStart]
        );
        assert_eq!(
            r.on_input(Input::TriggerUp, ms(t0, 400)),
            vec![Gesture::HoldEnd { keyed: true }]
        );
    }

    #[test]
    fn a_press_at_the_end_of_the_window_is_a_new_press() {
        // The window runs 350 ms from the release at 80 ms: a press at
        // exactly 430 ms ends the first press as a tap and starts over; one
        // ms earlier is a double tap.
        let (mut r, t0) = rec();
        r.on_input(Input::TriggerDown, t0);
        r.on_input(Input::TriggerUp, ms(t0, 80));
        assert_eq!(
            r.on_input(Input::TriggerDown, ms(t0, 430)),
            vec![Gesture::Tap]
        );
        assert_eq!(r.next_deadline(), Some(ms(t0, 680)));

        let (mut r, t0) = rec();
        r.on_input(Input::TriggerDown, t0);
        r.on_input(Input::TriggerUp, ms(t0, 80));
        assert_eq!(
            r.on_input(Input::TriggerDown, ms(t0, 429)),
            vec![Gesture::DoubleTap]
        );
    }

    #[test]
    fn repeated_down_or_up_events_are_ignored() {
        let (mut r, t0) = rec();
        r.on_input(Input::TriggerDown, t0);
        assert!(r.on_input(Input::TriggerDown, ms(t0, 10)).is_empty());
        assert!(r
            .on_input(Input::TriggerUp, ms(t0, 1000))
            .contains(&Gesture::HoldEnd { keyed: false }));
        assert!(r.on_input(Input::TriggerUp, ms(t0, 1010)).is_empty());
    }
}
