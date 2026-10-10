//! A virtual Xbox 360 or DualShock 4 controller, created through the ViGEmBus driver.
//! Games see it as a real controller plugged into the PC.

use std::collections::HashSet;

use vigem_client::{Client, DS4Report, DualShock4Wired, TargetId, XButtons, XGamepad, Xbox360Wired};

use crate::bindings::{ControllerAxis, ControllerButton, VirtualControllerKind};

pub const VIGEM_BUS_DOWNLOAD_URL: &str = "https://github.com/nefarius/ViGEmBus/releases/latest";

/// Controller state in a layout-neutral form: sticks are -1..=1 with up and right positive, triggers 0..=1.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GamepadState {
    pub pressed_buttons: HashSet<ControllerButton>,
    pub left_stick: (f32, f32),
    pub right_stick: (f32, f32),
    pub left_trigger: f32,
    pub right_trigger: f32,
}

impl GamepadState {
    pub fn set_axis(&mut self, axis: ControllerAxis, value: f32) {
        match axis {
            ControllerAxis::LeftStickX => self.left_stick.0 = value,
            ControllerAxis::LeftStickY => self.left_stick.1 = value,
            ControllerAxis::RightStickX => self.right_stick.0 = value,
            ControllerAxis::RightStickY => self.right_stick.1 = value,
            ControllerAxis::LeftTrigger => self.left_trigger = value,
            ControllerAxis::RightTrigger => self.right_trigger = value,
        }
    }

    fn is_pressed(&self, button: ControllerButton) -> bool {
        self.pressed_buttons.contains(&button)
    }

    fn effective_left_trigger(&self) -> f32 {
        if self.is_pressed(ControllerButton::LeftTrigger) {
            1.0
        } else {
            self.left_trigger
        }
    }

    fn effective_right_trigger(&self) -> f32 {
        if self.is_pressed(ControllerButton::RightTrigger) {
            1.0
        } else {
            self.right_trigger
        }
    }
}

pub enum VirtualGamepad {
    Xbox(Xbox360Wired<Client>),
    DualShock(DualShock4Wired<Client>),
}

impl VirtualGamepad {
    /// Plugs a new virtual controller in. Fails with a readable message when ViGEmBus is missing.
    pub fn connect(kind: VirtualControllerKind) -> Result<Self, String> {
        let client = Client::connect().map_err(|error| describe_connection_error(&error))?;
        match kind {
            VirtualControllerKind::Xbox360 => {
                let mut target = Xbox360Wired::new(client, TargetId::XBOX360_WIRED);
                target.plugin().map_err(|error| format!("{error}"))?;
                target.wait_ready().map_err(|error| format!("{error}"))?;
                Ok(VirtualGamepad::Xbox(target))
            }
            VirtualControllerKind::DualShock4 => {
                let mut target = DualShock4Wired::new(client, TargetId::DUALSHOCK4_WIRED);
                target.plugin().map_err(|error| format!("{error}"))?;
                target.wait_ready().map_err(|error| format!("{error}"))?;
                Ok(VirtualGamepad::DualShock(target))
            }
            VirtualControllerKind::None => Err("no virtual controller selected".to_owned()),
        }
    }

    pub fn kind(&self) -> VirtualControllerKind {
        match self {
            VirtualGamepad::Xbox(_) => VirtualControllerKind::Xbox360,
            VirtualGamepad::DualShock(_) => VirtualControllerKind::DualShock4,
        }
    }

    pub fn send_state(&mut self, state: &GamepadState) -> Result<(), String> {
        let result = match self {
            VirtualGamepad::Xbox(target) => target.update(&build_xbox_report(state)),
            VirtualGamepad::DualShock(target) => target.update(&build_dualshock_report(state)),
        };
        result.map_err(|error| format!("{error}"))
    }
}

impl Drop for VirtualGamepad {
    fn drop(&mut self) {
        let _ = match self {
            VirtualGamepad::Xbox(target) => target.unplug(),
            VirtualGamepad::DualShock(target) => target.unplug(),
        };
    }
}

fn describe_connection_error(error: &vigem_client::Error) -> String {
    match error {
        vigem_client::Error::BusNotFound => "ViGEmBus driver is not installed".to_owned(),
        other => format!("ViGEmBus error: {other}"),
    }
}

fn stick_to_i16(value: f32) -> i16 {
    (value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

fn trigger_to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn build_xbox_report(state: &GamepadState) -> XGamepad {
    let button_bits = [
        (ControllerButton::A, XButtons::A),
        (ControllerButton::B, XButtons::B),
        (ControllerButton::X, XButtons::X),
        (ControllerButton::Y, XButtons::Y),
        (ControllerButton::LeftBumper, XButtons::LB),
        (ControllerButton::RightBumper, XButtons::RB),
        (ControllerButton::Back, XButtons::BACK),
        (ControllerButton::Start, XButtons::START),
        (ControllerButton::Guide, XButtons::GUIDE),
        (ControllerButton::LeftStickClick, XButtons::LTHUMB),
        (ControllerButton::RightStickClick, XButtons::RTHUMB),
        (ControllerButton::DpadUp, XButtons::UP),
        (ControllerButton::DpadDown, XButtons::DOWN),
        (ControllerButton::DpadLeft, XButtons::LEFT),
        (ControllerButton::DpadRight, XButtons::RIGHT),
    ];
    let raw_buttons = button_bits
        .iter()
        .filter(|(button, _)| state.is_pressed(*button))
        .fold(0u16, |bits, (_, bit)| bits | bit);
    XGamepad {
        buttons: XButtons { raw: raw_buttons },
        left_trigger: trigger_to_u8(state.effective_left_trigger()),
        right_trigger: trigger_to_u8(state.effective_right_trigger()),
        thumb_lx: stick_to_i16(state.left_stick.0),
        thumb_ly: stick_to_i16(state.left_stick.1),
        thumb_rx: stick_to_i16(state.right_stick.0),
        thumb_ry: stick_to_i16(state.right_stick.1),
    }
}

/// DualShock 4 report bits: the low nibble is the d-pad as a hat (8 = released), the rest are buttons.
fn build_dualshock_report(state: &GamepadState) -> DS4Report {
    const SQUARE: u16 = 1 << 4;
    const CROSS: u16 = 1 << 5;
    const CIRCLE: u16 = 1 << 6;
    const TRIANGLE: u16 = 1 << 7;
    const L1: u16 = 1 << 8;
    const R1: u16 = 1 << 9;
    const L2: u16 = 1 << 10;
    const R2: u16 = 1 << 11;
    const SHARE: u16 = 1 << 12;
    const OPTIONS: u16 = 1 << 13;
    const L3: u16 = 1 << 14;
    const R3: u16 = 1 << 15;
    const PS_BUTTON: u8 = 1;

    let button_bits = [
        (ControllerButton::X, SQUARE),
        (ControllerButton::A, CROSS),
        (ControllerButton::B, CIRCLE),
        (ControllerButton::Y, TRIANGLE),
        (ControllerButton::LeftBumper, L1),
        (ControllerButton::RightBumper, R1),
        (ControllerButton::Back, SHARE),
        (ControllerButton::Start, OPTIONS),
        (ControllerButton::LeftStickClick, L3),
        (ControllerButton::RightStickClick, R3),
    ];
    let mut buttons = button_bits
        .iter()
        .filter(|(button, _)| state.is_pressed(*button))
        .fold(dualshock_dpad_value(state), |bits, (_, bit)| bits | bit);
    let left_trigger = state.effective_left_trigger();
    let right_trigger = state.effective_right_trigger();
    if left_trigger > 0.1 {
        buttons |= L2;
    }
    if right_trigger > 0.1 {
        buttons |= R2;
    }
    DS4Report {
        thumb_lx: stick_to_u8(state.left_stick.0),
        thumb_ly: stick_to_u8(-state.left_stick.1),
        thumb_rx: stick_to_u8(state.right_stick.0),
        thumb_ry: stick_to_u8(-state.right_stick.1),
        buttons,
        special: if state.is_pressed(ControllerButton::Guide) {
            PS_BUTTON
        } else {
            0
        },
        trigger_l: trigger_to_u8(left_trigger),
        trigger_r: trigger_to_u8(right_trigger),
    }
}

/// DualShock sticks are 0..=255 with 128 centred and down positive.
fn stick_to_u8(value: f32) -> u8 {
    ((value.clamp(-1.0, 1.0) + 1.0) * 127.5).round() as u8
}

fn dualshock_dpad_value(state: &GamepadState) -> u16 {
    const RELEASED: u16 = 8;
    let up = state.is_pressed(ControllerButton::DpadUp);
    let down = state.is_pressed(ControllerButton::DpadDown);
    let left = state.is_pressed(ControllerButton::DpadLeft);
    let right = state.is_pressed(ControllerButton::DpadRight);
    match (up, right, down, left) {
        (true, false, false, false) => 0,
        (true, true, false, false) => 1,
        (false, true, false, false) => 2,
        (false, true, true, false) => 3,
        (false, false, true, false) => 4,
        (false, false, true, true) => 5,
        (false, false, false, true) => 6,
        (true, false, false, true) => 7,
        _ => RELEASED,
    }
}
