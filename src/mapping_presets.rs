//! Ready-made sets of bindings that give a working setup in one click.

use crate::bindings::{
    AxisDirection, ControllerAxis, InputBinding, InputSource, OutputAction, ScreenAxis, VirtualControllerKind,
};
use crate::hall_effect::KeyResponse;
use crate::known_devices::DeviceKind;
use crate::profiles::Profile;

const STICK_ROLL_AXIS: usize = 0;
const STICK_PITCH_AXIS: usize = 1;
const STICK_THUMB_X_AXIS: usize = 2;
const STICK_THUMB_Y_AXIS: usize = 3;
const STICK_TWIST_AXIS: usize = 4;
const THROTTLE_LEFT_AXIS: usize = 0;
const THROTTLE_RIGHT_AXIS: usize = 1;
const MOUSE_LOOK_PIXELS_PER_SECOND: f32 = 1500.0;

const VIRTUAL_KEY_W: u16 = 0x57;
const VIRTUAL_KEY_A: u16 = 0x41;
const VIRTUAL_KEY_S: u16 = 0x53;
const VIRTUAL_KEY_D: u16 = 0x44;
const VIRTUAL_KEY_Q: u16 = 0x51;
const VIRTUAL_KEY_E: u16 = 0x45;

fn axis_source(device: DeviceKind, axis_index: usize) -> InputSource {
    InputSource::Axis { device, axis_index }
}

fn hall_effect_key(axis_index: usize, direction: AxisDirection, virtual_key: u16) -> InputBinding {
    InputBinding::new(
        axis_source(DeviceKind::Joystick, axis_index),
        OutputAction::HallEffectKey {
            virtual_key,
            direction,
            response: KeyResponse::default(),
        },
        Default::default(),
    )
}

fn controller_axis(device: DeviceKind, axis_index: usize, axis: ControllerAxis, inverted: bool) -> InputBinding {
    InputBinding::new(
        axis_source(device, axis_index),
        OutputAction::ControllerAxis { axis, inverted },
        Default::default(),
    )
}

/// Stick forward / back on W / S, left / right on A / D, twist on Q / E, as Hall effect keys with rapid trigger.
pub fn add_wasd_flight_layout(profile: &mut Profile) {
    profile.bindings.extend([
        hall_effect_key(STICK_PITCH_AXIS, AxisDirection::Decreasing, VIRTUAL_KEY_W),
        hall_effect_key(STICK_PITCH_AXIS, AxisDirection::Increasing, VIRTUAL_KEY_S),
        hall_effect_key(STICK_ROLL_AXIS, AxisDirection::Decreasing, VIRTUAL_KEY_A),
        hall_effect_key(STICK_ROLL_AXIS, AxisDirection::Increasing, VIRTUAL_KEY_D),
        hall_effect_key(STICK_TWIST_AXIS, AxisDirection::Decreasing, VIRTUAL_KEY_Q),
        hall_effect_key(STICK_TWIST_AXIS, AxisDirection::Increasing, VIRTUAL_KEY_E),
    ]);
}

/// The stick's thumb stick moves the mouse pointer, for looking around.
pub fn add_mouse_look_layout(profile: &mut Profile) {
    let mouse_axis = |axis_index: usize, screen_axis: ScreenAxis| {
        InputBinding::new(
            axis_source(DeviceKind::Joystick, axis_index),
            OutputAction::MouseMove {
                screen_axis,
                pixels_per_second: MOUSE_LOOK_PIXELS_PER_SECOND,
            },
            Default::default(),
        )
    };
    profile.bindings.extend([
        mouse_axis(STICK_THUMB_X_AXIS, ScreenAxis::Horizontal),
        mouse_axis(STICK_THUMB_Y_AXIS, ScreenAxis::Vertical),
    ]);
}

/// Stick on the left stick, thumb stick on the right stick, throttles on the triggers,
/// for an Xbox or a PlayStation controller.
pub fn add_controller_starter_layout(profile: &mut Profile, controller_kind: VirtualControllerKind) {
    profile.virtual_controller = controller_kind;
    profile.bindings.extend([
        controller_axis(DeviceKind::Joystick, STICK_ROLL_AXIS, ControllerAxis::LeftStickX, false),
        controller_axis(
            DeviceKind::Joystick,
            STICK_PITCH_AXIS,
            ControllerAxis::LeftStickY,
            false,
        ),
        controller_axis(
            DeviceKind::Joystick,
            STICK_THUMB_X_AXIS,
            ControllerAxis::RightStickX,
            false,
        ),
        controller_axis(
            DeviceKind::Joystick,
            STICK_THUMB_Y_AXIS,
            ControllerAxis::RightStickY,
            false,
        ),
        controller_axis(
            DeviceKind::Throttle,
            THROTTLE_LEFT_AXIS,
            ControllerAxis::RightTrigger,
            true,
        ),
        controller_axis(
            DeviceKind::Throttle,
            THROTTLE_RIGHT_AXIS,
            ControllerAxis::LeftTrigger,
            true,
        ),
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_binding_is_consistent() {
        let mut profile = Profile::default();
        add_wasd_flight_layout(&mut profile);
        add_mouse_look_layout(&mut profile);
        add_controller_starter_layout(&mut profile, VirtualControllerKind::Xbox360);
        assert!(profile.bindings.iter().all(InputBinding::is_consistent));
    }
}
