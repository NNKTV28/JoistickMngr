//! How a button press turns into output: held, toggled, tapped, held long or double tapped.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::localization::tr;

/// A press shorter than this counts as a tap.
pub const TAP_MAXIMUM: Duration = Duration::from_millis(250);
/// A press must last this long before a long-hold binding fires.
pub const LONG_HOLD_MINIMUM: Duration = Duration::from_millis(450);
/// The second tap of a double tap must start within this time after the first one ends.
pub const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(300);
/// Taps and double taps send a short press: long enough for games to notice, short enough to feel instant.
pub const PULSE_LENGTH: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PressStyle {
    /// Output is on while the control is held.
    #[default]
    WhileHeld,
    /// Each press switches the output on or off.
    Toggle,
    /// A quick press sends a short press.
    Tap,
    /// Output turns on once the control has been held for a moment, and stays on until release.
    LongHold,
    /// Two quick presses send a short press.
    DoubleTap,
}

impl PressStyle {
    pub const ALL: [PressStyle; 5] = [
        PressStyle::WhileHeld,
        PressStyle::Toggle,
        PressStyle::Tap,
        PressStyle::LongHold,
        PressStyle::DoubleTap,
    ];

    pub fn label(self) -> &'static str {
        tr(match self {
            PressStyle::WhileHeld => "While held",
            PressStyle::Toggle => "Toggle on / off",
            PressStyle::Tap => "Short tap",
            PressStyle::LongHold => "Long hold",
            PressStyle::DoubleTap => "Double tap",
        })
    }

    pub fn explanation(self) -> &'static str {
        tr(match self {
            PressStyle::WhileHeld => "The output stays on while the control is held.",
            PressStyle::Toggle => "First press switches the output on, the next press switches it off.",
            PressStyle::Tap => "A quick press sends a short press. Holding does nothing.",
            PressStyle::LongHold => "Fires after holding for half a second and stays on until release.",
            PressStyle::DoubleTap => "Two quick presses send a short press.",
        })
    }
}

/// Remembers one binding's recent presses so its press style can be applied.
#[derive(Clone, Debug, Default)]
pub struct PressTracker {
    was_pressed: bool,
    pressed_at: Option<Instant>,
    is_toggled_on: bool,
    /// When the last short press ended, for spotting a double tap.
    last_short_release_at: Option<Instant>,
    /// The current press already completed a double tap, so its release must not start another.
    press_completed_double_tap: bool,
    /// A single tap waiting to see whether a second tap turns it into a double tap.
    single_tap_waiting_since: Option<Instant>,
    pulse_until: Option<Instant>,
}

impl PressTracker {
    /// Whether the binding's output should be on right now.
    /// `waits_for_double_tap` is true when the same control also has a double-tap binding.
    pub fn update(&mut self, style: PressStyle, is_pressed: bool, now: Instant, waits_for_double_tap: bool) -> bool {
        let press_started = is_pressed && !self.was_pressed;
        let press_ended = !is_pressed && self.was_pressed;
        let press_was_short = press_ended && self.pressed_at.is_some_and(|start| now - start < TAP_MAXIMUM);
        if press_started {
            self.pressed_at = Some(now);
        }
        let output_is_on = match style {
            PressStyle::WhileHeld => is_pressed,
            PressStyle::Toggle => self.update_toggle(press_started),
            PressStyle::LongHold => is_pressed && self.pressed_at.is_some_and(|start| now - start >= LONG_HOLD_MINIMUM),
            PressStyle::Tap => self.update_tap(press_started, press_was_short, now, waits_for_double_tap),
            PressStyle::DoubleTap => self.update_double_tap(press_started, press_ended, press_was_short, now),
        };
        self.was_pressed = is_pressed;
        output_is_on
    }

    fn update_toggle(&mut self, press_started: bool) -> bool {
        if press_started {
            self.is_toggled_on = !self.is_toggled_on;
        }
        self.is_toggled_on
    }

    fn update_tap(
        &mut self,
        press_started: bool,
        press_was_short: bool,
        now: Instant,
        waits_for_double_tap: bool,
    ) -> bool {
        if press_started {
            self.single_tap_waiting_since = None;
        }
        if press_was_short {
            if waits_for_double_tap {
                self.single_tap_waiting_since = Some(now);
            } else {
                self.start_pulse(now);
            }
        }
        let wait_is_over = self
            .single_tap_waiting_since
            .is_some_and(|since| now - since >= DOUBLE_TAP_WINDOW);
        if wait_is_over {
            self.single_tap_waiting_since = None;
            self.start_pulse(now);
        }
        self.is_pulsing(now)
    }

    fn update_double_tap(
        &mut self,
        press_started: bool,
        press_ended: bool,
        press_was_short: bool,
        now: Instant,
    ) -> bool {
        let follows_quick_tap = self
            .last_short_release_at
            .is_some_and(|release| now - release <= DOUBLE_TAP_WINDOW);
        if press_started && follows_quick_tap {
            self.start_pulse(now);
            self.last_short_release_at = None;
            self.press_completed_double_tap = true;
        }
        if press_ended {
            if press_was_short && !self.press_completed_double_tap {
                self.last_short_release_at = Some(now);
            }
            self.press_completed_double_tap = false;
        }
        self.is_pulsing(now)
    }

    fn start_pulse(&mut self, now: Instant) {
        self.pulse_until = Some(now + PULSE_LENGTH);
    }

    fn is_pulsing(&self, now: Instant) -> bool {
        self.pulse_until.is_some_and(|until| now < until)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, milliseconds: u64) -> Instant {
        start + Duration::from_millis(milliseconds)
    }

    #[test]
    fn toggle_switches_on_first_press_and_off_on_second() {
        let start = Instant::now();
        let mut tracker = PressTracker::default();
        assert!(tracker.update(PressStyle::Toggle, true, at(start, 0), false));
        assert!(tracker.update(PressStyle::Toggle, false, at(start, 100), false));
        assert!(!tracker.update(PressStyle::Toggle, true, at(start, 200), false));
    }

    #[test]
    fn long_hold_fires_only_after_the_minimum_time() {
        let start = Instant::now();
        let mut tracker = PressTracker::default();
        assert!(!tracker.update(PressStyle::LongHold, true, at(start, 0), false));
        assert!(!tracker.update(PressStyle::LongHold, true, at(start, 300), false));
        assert!(tracker.update(PressStyle::LongHold, true, at(start, 500), false));
    }

    #[test]
    fn tap_fires_a_short_pulse_on_release() {
        let start = Instant::now();
        let mut tracker = PressTracker::default();
        assert!(!tracker.update(PressStyle::Tap, true, at(start, 0), false));
        assert!(tracker.update(PressStyle::Tap, false, at(start, 100), false));
        assert!(!tracker.update(PressStyle::Tap, false, at(start, 200), false));
    }

    #[test]
    fn slow_press_is_not_a_tap() {
        let start = Instant::now();
        let mut tracker = PressTracker::default();
        tracker.update(PressStyle::Tap, true, at(start, 0), false);
        assert!(!tracker.update(PressStyle::Tap, false, at(start, 600), false));
    }

    #[test]
    fn double_tap_fires_on_the_second_quick_press() {
        let start = Instant::now();
        let mut tracker = PressTracker::default();
        tracker.update(PressStyle::DoubleTap, true, at(start, 0), false);
        assert!(!tracker.update(PressStyle::DoubleTap, false, at(start, 80), false));
        assert!(tracker.update(PressStyle::DoubleTap, true, at(start, 200), false));
    }

    #[test]
    fn single_tap_waits_when_a_double_tap_shares_the_button() {
        let start = Instant::now();
        let mut tracker = PressTracker::default();
        tracker.update(PressStyle::Tap, true, at(start, 0), true);
        assert!(!tracker.update(PressStyle::Tap, false, at(start, 80), true));
        assert!(tracker.update(PressStyle::Tap, false, at(start, 400), true));
    }

    #[test]
    fn single_tap_is_dropped_when_a_second_tap_follows() {
        let start = Instant::now();
        let mut tracker = PressTracker::default();
        tracker.update(PressStyle::Tap, true, at(start, 0), true);
        tracker.update(PressStyle::Tap, false, at(start, 80), true);
        tracker.update(PressStyle::Tap, true, at(start, 200), true);
        assert!(!tracker.update(PressStyle::Tap, true, at(start, 450), true));
    }
}
