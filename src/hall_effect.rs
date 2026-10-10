//! Hall effect keys: an axis drives a keyboard key the way a magnetic (Hall effect) keyboard drives
//! its switches. The key goes down at an adjustable actuation point and, with rapid trigger, comes
//! back up the moment the axis eases off a little, wherever it is in its travel.
//!
//! The key itself is still an ordinary key press, because Windows keyboard input is either down or
//! up. Analog keyboards work the same way; a game only sees partial travel through a controller axis.

use serde::{Deserialize, Serialize};

use crate::localization::tr;

pub const DEFAULT_ACTUATION_POINT: f32 = 0.35;
pub const DEFAULT_RAPID_TRIGGER_SENSITIVITY: f32 = 0.08;
/// Travel of a typical Hall effect switch, used to show settings in the millimetres keyboard software uses.
pub const KEY_TRAVEL_MILLIMETRES: f32 = 4.0;
/// Without rapid trigger the key comes up slightly above the actuation point, so sensor noise at the
/// actuation point cannot make it chatter.
const CLASSIC_RELEASE_GAP: f32 = 0.02;
/// Continuous rapid trigger ends when the axis is back this close to its resting position.
const FULL_RELEASE_POSITION: f32 = 0.03;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RapidTriggerMode {
    /// The key comes up when the axis returns past the actuation point.
    Off,
    /// The key comes up as soon as the axis eases off by the sensitivity, and goes down again when it
    /// is pushed further by the same amount, while past the actuation point.
    #[default]
    On,
    /// Like `On`, but presses and releases keep following the axis anywhere in its travel until it
    /// returns to rest.
    Continuous,
}

impl RapidTriggerMode {
    pub const ALL: [RapidTriggerMode; 3] = [
        RapidTriggerMode::Off,
        RapidTriggerMode::On,
        RapidTriggerMode::Continuous,
    ];

    pub fn label(self) -> &'static str {
        tr(match self {
            RapidTriggerMode::Off => "Off",
            RapidTriggerMode::On => "Rapid trigger",
            RapidTriggerMode::Continuous => "Continuous rapid trigger",
        })
    }
}

/// How a Hall effect key reacts to the axis.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeyResponse {
    /// Share of the axis travel, 0..1, at which the key goes down.
    pub actuation_point: f32,
    pub rapid_trigger: RapidTriggerMode,
    /// Share of the axis travel the axis must ease off (or push further) for rapid trigger to act.
    pub rapid_trigger_sensitivity: f32,
}

impl Default for KeyResponse {
    fn default() -> Self {
        Self {
            actuation_point: DEFAULT_ACTUATION_POINT,
            rapid_trigger: RapidTriggerMode::On,
            rapid_trigger_sensitivity: DEFAULT_RAPID_TRIGGER_SENSITIVITY,
        }
    }
}

/// One Hall effect key's state between axis reports.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HallEffectKeyState {
    is_pressed: bool,
    /// Deepest push while the key is down, shallowest push since it came up through rapid trigger.
    turning_point: Option<f32>,
    /// Continuous rapid trigger stays in charge from the first actuation until the axis is back at rest.
    is_in_continuous_run: bool,
}

impl HallEffectKeyState {
    /// Feeds the axis push (0 at rest, 1 fully pushed) and returns whether the key should be down.
    pub fn update(&mut self, push: f32, response: KeyResponse) -> bool {
        if self.is_pressed {
            self.update_while_pressed(push, response);
        } else {
            self.update_while_released(push, response);
        }
        self.is_pressed
    }

    fn update_while_pressed(&mut self, push: f32, response: KeyResponse) {
        let deepest = self.turning_point.map_or(push, |deepest| deepest.max(push));
        self.turning_point = Some(deepest);
        let should_release = match response.rapid_trigger {
            RapidTriggerMode::Off => push < response.actuation_point - CLASSIC_RELEASE_GAP,
            RapidTriggerMode::On => {
                push < response.actuation_point || deepest - push >= response.rapid_trigger_sensitivity
            }
            RapidTriggerMode::Continuous => {
                push <= FULL_RELEASE_POSITION || deepest - push >= response.rapid_trigger_sensitivity
            }
        };
        if should_release {
            self.release(push, response);
        }
    }

    fn update_while_released(&mut self, push: f32, response: KeyResponse) {
        if push <= FULL_RELEASE_POSITION {
            self.is_in_continuous_run = false;
        }
        let left_actuation_zone = response.rapid_trigger == RapidTriggerMode::On && push < response.actuation_point;
        let shallowest = self
            .turning_point
            .filter(|_| !left_actuation_zone)
            .map(|shallowest| shallowest.min(push));
        self.turning_point = shallowest;
        let pushed_back_down =
            shallowest.is_some_and(|shallowest| push - shallowest >= response.rapid_trigger_sensitivity);
        let past_actuation_point = push >= response.actuation_point;
        let should_press = match response.rapid_trigger {
            RapidTriggerMode::Off => past_actuation_point,
            RapidTriggerMode::On => past_actuation_point && (shallowest.is_none() || pushed_back_down),
            RapidTriggerMode::Continuous if self.is_in_continuous_run => pushed_back_down,
            RapidTriggerMode::Continuous => past_actuation_point,
        };
        if should_press {
            self.press(push, response);
        }
    }

    fn press(&mut self, push: f32, response: KeyResponse) {
        self.is_pressed = true;
        self.turning_point = Some(push);
        self.is_in_continuous_run = response.rapid_trigger == RapidTriggerMode::Continuous;
    }

    /// After a rapid trigger release the shallowest point is tracked so the next push can re-press.
    /// Below the actuation point (plain rapid trigger) a fresh press only needs the actuation point.
    fn release(&mut self, push: f32, response: KeyResponse) {
        self.is_pressed = false;
        let keeps_tracking = match response.rapid_trigger {
            RapidTriggerMode::Off => false,
            RapidTriggerMode::On => push >= response.actuation_point,
            RapidTriggerMode::Continuous => push > FULL_RELEASE_POSITION,
        };
        self.turning_point = keeps_tracking.then_some(push);
        if push <= FULL_RELEASE_POSITION {
            self.is_in_continuous_run = false;
        }
    }
}

/// A share of the axis travel shown as millimetres of a Hall effect switch.
pub fn as_key_travel_millimetres(share_of_travel: f32) -> f32 {
    share_of_travel * KEY_TRAVEL_MILLIMETRES
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(rapid_trigger: RapidTriggerMode) -> KeyResponse {
        KeyResponse {
            actuation_point: 0.4,
            rapid_trigger,
            rapid_trigger_sensitivity: 0.1,
        }
    }

    fn run(pushes: &[f32], response: KeyResponse) -> Vec<bool> {
        let mut state = HallEffectKeyState::default();
        pushes.iter().map(|push| state.update(*push, response)).collect()
    }

    #[test]
    fn key_goes_down_only_at_the_actuation_point() {
        let key_states = run(&[0.2, 0.39, 0.41], response(RapidTriggerMode::Off));
        assert_eq!(key_states, [false, false, true]);
    }

    #[test]
    fn without_rapid_trigger_the_key_holds_until_back_above_the_actuation_point() {
        let key_states = run(&[0.5, 0.9, 0.45, 0.39, 0.37], response(RapidTriggerMode::Off));
        assert_eq!(key_states, [true, true, true, true, false]);
    }

    #[test]
    fn rapid_trigger_releases_as_soon_as_the_axis_eases_off() {
        let key_states = run(&[0.5, 0.9, 0.85, 0.79], response(RapidTriggerMode::On));
        assert_eq!(key_states, [true, true, true, false]);
    }

    #[test]
    fn rapid_trigger_presses_again_when_pushed_further_by_the_sensitivity() {
        let key_states = run(&[0.9, 0.7, 0.75, 0.81], response(RapidTriggerMode::On));
        assert_eq!(key_states, [true, false, false, true]);
    }

    #[test]
    fn plain_rapid_trigger_needs_the_actuation_point_again_after_leaving_it() {
        let key_states = run(&[0.9, 0.2, 0.35, 0.42], response(RapidTriggerMode::On));
        assert_eq!(key_states, [true, false, false, true]);
    }

    #[test]
    fn continuous_rapid_trigger_works_above_the_actuation_point() {
        let key_states = run(&[0.9, 0.2, 0.31, 0.15], response(RapidTriggerMode::Continuous));
        assert_eq!(key_states, [true, false, true, false]);
    }

    #[test]
    fn continuous_rapid_trigger_ends_at_rest() {
        let key_states = run(&[0.9, 0.0, 0.2, 0.39], response(RapidTriggerMode::Continuous));
        assert_eq!(key_states, [true, false, false, false]);
    }
}
