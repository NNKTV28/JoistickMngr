//! What a control on the HOTAS does when it is used: press a key, move the mouse,
//! or drive a button or stick of a virtual Xbox / PlayStation controller.

use serde::{Deserialize, Serialize};

use crate::input_reader::{ControllerSnapshot, HatDirection};
use crate::keyboard_keys::key_name;
use crate::known_devices::DeviceKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AxisDirection {
    Increasing,
    Decreasing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HatPush {
    Up,
    Right,
    Down,
    Left,
}

impl HatPush {
    fn is_part_of(self, direction: HatDirection) -> bool {
        match self {
            HatPush::Up => matches!(
                direction,
                HatDirection::Up | HatDirection::UpLeft | HatDirection::UpRight
            ),
            HatPush::Down => {
                matches!(
                    direction,
                    HatDirection::Down | HatDirection::DownLeft | HatDirection::DownRight
                )
            }
            HatPush::Left => matches!(
                direction,
                HatDirection::Left | HatDirection::UpLeft | HatDirection::DownLeft
            ),
            HatPush::Right => {
                matches!(
                    direction,
                    HatDirection::Right | HatDirection::UpRight | HatDirection::DownRight
                )
            }
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            HatPush::Up => "up",
            HatPush::Right => "right",
            HatPush::Down => "down",
            HatPush::Left => "left",
        }
    }
}

/// The control on the HOTAS that triggers a binding.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum InputSource {
    Button {
        device: DeviceKind,
        button_number: usize,
    },
    /// An axis acting as a switch: active once it moves past `threshold` (0..=1) in `direction`.
    AxisPastThreshold {
        device: DeviceKind,
        axis_index: usize,
        direction: AxisDirection,
        threshold: f64,
    },
    /// An axis used for its full analog range.
    Axis {
        device: DeviceKind,
        axis_index: usize,
    },
    Hat {
        device: DeviceKind,
        hat_index: usize,
        push: HatPush,
    },
}

impl InputSource {
    pub fn device(&self) -> DeviceKind {
        match *self {
            InputSource::Button { device, .. }
            | InputSource::AxisPastThreshold { device, .. }
            | InputSource::Axis { device, .. }
            | InputSource::Hat { device, .. } => device,
        }
    }

    pub fn is_analog(&self) -> bool {
        matches!(self, InputSource::Axis { .. })
    }

    /// Whether a switch-like source is currently held. Analog sources are never "held".
    pub fn is_active(&self, snapshot: &ControllerSnapshot) -> bool {
        match *self {
            InputSource::Button { button_number, .. } => snapshot.is_button_pressed(button_number),
            InputSource::AxisPastThreshold {
                axis_index,
                direction,
                threshold,
                ..
            } => snapshot.axis_value(axis_index).is_some_and(|value| match direction {
                AxisDirection::Increasing => value >= threshold,
                AxisDirection::Decreasing => value <= threshold,
            }),
            InputSource::Axis { .. } => false,
            InputSource::Hat { hat_index, push, .. } => push.is_part_of(snapshot.hat_direction(hat_index)),
        }
    }

    /// Current position of an analog source, 0..=1.
    pub fn analog_value(&self, snapshot: &ControllerSnapshot) -> Option<f64> {
        match *self {
            InputSource::Axis { axis_index, .. } => snapshot.axis_value(axis_index),
            _ => None,
        }
    }

    pub fn describe(&self) -> String {
        let device = self.device().definition();
        match *self {
            InputSource::Button { button_number, .. } => format!("{} · Button {button_number}", device.short_name),
            InputSource::AxisPastThreshold {
                axis_index, direction, ..
            } => {
                let arrow = if direction == AxisDirection::Increasing {
                    "+"
                } else {
                    "−"
                };
                format!("{} · {} {arrow}", device.short_name, device.axis_name(axis_index))
            }
            InputSource::Axis { axis_index, .. } => {
                format!("{} · {} (axis)", device.short_name, device.axis_name(axis_index))
            }
            InputSource::Hat { hat_index, push, .. } => {
                format!("{} · Hat {} {}", device.short_name, hat_index + 1, push.label())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MouseButtonKind {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

impl MouseButtonKind {
    pub const ALL: [MouseButtonKind; 5] = [
        MouseButtonKind::Left,
        MouseButtonKind::Right,
        MouseButtonKind::Middle,
        MouseButtonKind::Back,
        MouseButtonKind::Forward,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MouseButtonKind::Left => "Left click",
            MouseButtonKind::Right => "Right click",
            MouseButtonKind::Middle => "Middle click",
            MouseButtonKind::Back => "Back button",
            MouseButtonKind::Forward => "Forward button",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WheelDirection {
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ControllerButton {
    A,
    B,
    X,
    Y,
    LeftBumper,
    RightBumper,
    LeftTrigger,
    RightTrigger,
    Back,
    Start,
    Guide,
    LeftStickClick,
    RightStickClick,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
}

impl ControllerButton {
    pub const ALL: [ControllerButton; 17] = [
        ControllerButton::A,
        ControllerButton::B,
        ControllerButton::X,
        ControllerButton::Y,
        ControllerButton::LeftBumper,
        ControllerButton::RightBumper,
        ControllerButton::LeftTrigger,
        ControllerButton::RightTrigger,
        ControllerButton::Back,
        ControllerButton::Start,
        ControllerButton::Guide,
        ControllerButton::LeftStickClick,
        ControllerButton::RightStickClick,
        ControllerButton::DpadUp,
        ControllerButton::DpadDown,
        ControllerButton::DpadLeft,
        ControllerButton::DpadRight,
    ];

    /// Xbox name first, PlayStation name second.
    pub fn label(self) -> &'static str {
        match self {
            ControllerButton::A => "A / Cross",
            ControllerButton::B => "B / Circle",
            ControllerButton::X => "X / Square",
            ControllerButton::Y => "Y / Triangle",
            ControllerButton::LeftBumper => "LB / L1",
            ControllerButton::RightBumper => "RB / R1",
            ControllerButton::LeftTrigger => "LT / L2",
            ControllerButton::RightTrigger => "RT / R2",
            ControllerButton::Back => "View / Share",
            ControllerButton::Start => "Menu / Options",
            ControllerButton::Guide => "Xbox / PS button",
            ControllerButton::LeftStickClick => "LS / L3",
            ControllerButton::RightStickClick => "RS / R3",
            ControllerButton::DpadUp => "D-pad up",
            ControllerButton::DpadDown => "D-pad down",
            ControllerButton::DpadLeft => "D-pad left",
            ControllerButton::DpadRight => "D-pad right",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControllerAxis {
    LeftStickX,
    LeftStickY,
    RightStickX,
    RightStickY,
    LeftTrigger,
    RightTrigger,
}

impl ControllerAxis {
    pub const ALL: [ControllerAxis; 6] = [
        ControllerAxis::LeftStickX,
        ControllerAxis::LeftStickY,
        ControllerAxis::RightStickX,
        ControllerAxis::RightStickY,
        ControllerAxis::LeftTrigger,
        ControllerAxis::RightTrigger,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ControllerAxis::LeftStickX => "Left stick X",
            ControllerAxis::LeftStickY => "Left stick Y",
            ControllerAxis::RightStickX => "Right stick X",
            ControllerAxis::RightStickY => "Right stick Y",
            ControllerAxis::LeftTrigger => "Left trigger (LT / L2)",
            ControllerAxis::RightTrigger => "Right trigger (RT / R2)",
        }
    }
}

/// What happens on the PC when the source is used.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum OutputAction {
    Key {
        virtual_key: u16,
    },
    MouseButton {
        button: MouseButtonKind,
    },
    MouseWheel {
        direction: WheelDirection,
    },
    /// Moves the pointer at up to `pixels_per_second` when the axis is at its end.
    MouseMove {
        screen_axis: ScreenAxis,
        pixels_per_second: f32,
    },
    ControllerButton {
        button: ControllerButton,
    },
    ControllerAxis {
        axis: ControllerAxis,
        inverted: bool,
    },
}

impl OutputAction {
    pub fn needs_analog_source(&self) -> bool {
        matches!(
            self,
            OutputAction::MouseMove { .. } | OutputAction::ControllerAxis { .. }
        )
    }

    pub fn describe(&self) -> String {
        match *self {
            OutputAction::Key { virtual_key } => format!("Key {}", key_name(virtual_key)),
            OutputAction::MouseButton { button } => button.label().to_owned(),
            OutputAction::MouseWheel { direction } => match direction {
                WheelDirection::Up => "Scroll up".to_owned(),
                WheelDirection::Down => "Scroll down".to_owned(),
            },
            OutputAction::MouseMove {
                screen_axis,
                pixels_per_second,
            } => {
                let direction = if screen_axis == ScreenAxis::Horizontal {
                    "left/right"
                } else {
                    "up/down"
                };
                format!("Mouse {direction} · {pixels_per_second:.0} px/s")
            }
            OutputAction::ControllerButton { button } => format!("Controller {}", button.label()),
            OutputAction::ControllerAxis { axis, inverted } => {
                let inverted_note = if inverted { " (inverted)" } else { "" };
                format!("Controller {}{inverted_note}", axis.label())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct InputBinding {
    pub source: InputSource,
    pub action: OutputAction,
}

impl InputBinding {
    /// Analog sources drive analog actions; switch-like sources drive key, button and wheel actions.
    pub fn is_consistent(&self) -> bool {
        self.source.is_analog() == self.action.needs_analog_source()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VirtualControllerKind {
    #[default]
    None,
    Xbox360,
    DualShock4,
}

impl VirtualControllerKind {
    pub const ALL: [VirtualControllerKind; 3] = [
        VirtualControllerKind::None,
        VirtualControllerKind::Xbox360,
        VirtualControllerKind::DualShock4,
    ];

    pub fn label(self) -> &'static str {
        match self {
            VirtualControllerKind::None => "No virtual controller",
            VirtualControllerKind::Xbox360 => "Xbox controller",
            VirtualControllerKind::DualShock4 => "PlayStation controller (DualShock 4)",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_with_axis(value: f64) -> ControllerSnapshot {
        ControllerSnapshot {
            axis_values: vec![value],
            ..Default::default()
        }
    }

    #[test]
    fn axis_threshold_switches_on_past_the_threshold() {
        let source = InputSource::AxisPastThreshold {
            device: DeviceKind::Throttle,
            axis_index: 0,
            direction: AxisDirection::Decreasing,
            threshold: 0.3,
        };
        assert!(source.is_active(&snapshot_with_axis(0.1)));
        assert!(!source.is_active(&snapshot_with_axis(0.6)));
    }

    #[test]
    fn diagonal_hat_counts_for_both_directions() {
        assert!(HatPush::Up.is_part_of(HatDirection::UpLeft));
        assert!(HatPush::Left.is_part_of(HatDirection::UpLeft));
        assert!(!HatPush::Down.is_part_of(HatDirection::UpLeft));
    }

    #[test]
    fn analog_actions_need_analog_sources() {
        let binding = InputBinding {
            source: InputSource::Button {
                device: DeviceKind::Joystick,
                button_number: 1,
            },
            action: OutputAction::ControllerAxis {
                axis: ControllerAxis::LeftStickX,
                inverted: false,
            },
        };
        assert!(!binding.is_consistent());
    }
}
