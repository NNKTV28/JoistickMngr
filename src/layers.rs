//! Binding layers: the throttle's M1 / M2 / M3 mode switch (or any buttons chosen for it) and a
//! "shift" button let every control have several jobs.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::input_reader::ControllerSnapshot;
use crate::known_devices::DeviceKind;
use crate::localization::{tr, tr_format};

pub const MODE_COUNT: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ButtonReference {
    pub device: DeviceKind,
    pub button_number: usize,
}

impl ButtonReference {
    pub fn is_pressed(&self, snapshots: &HashMap<DeviceKind, ControllerSnapshot>) -> bool {
        snapshots
            .get(&self.device)
            .is_some_and(|snapshot| snapshot.is_button_pressed(self.button_number))
    }

    pub fn describe(&self) -> String {
        tr_format(
            "{} · Button {}",
            &[&self.device.definition().short_name, &self.button_number],
        )
    }
}

/// The buttons the throttle's mode switch holds down in positions 1, 2 and 3.
/// They are part of the hardware, so every profile shares them.
pub type ModeSwitchButtons = [Option<ButtonReference>; MODE_COUNT];

/// 1..=3 for the position the mode switch is in, or `None` when it is not known.
/// A switch the device reports by itself (the X56 throttle) wins over buttons chosen for it.
pub fn selected_mode(
    mode_switch: &ModeSwitchButtons,
    snapshots: &HashMap<DeviceKind, ControllerSnapshot>,
) -> Option<u8> {
    reported_mode(snapshots).or_else(|| mode_from_buttons(mode_switch, snapshots))
}

/// The mode switch position a device reported by itself, if any device did.
pub fn reported_mode(snapshots: &HashMap<DeviceKind, ControllerSnapshot>) -> Option<u8> {
    snapshots.values().find_map(|snapshot| snapshot.reported_mode)
}

fn mode_from_buttons(
    mode_switch: &ModeSwitchButtons,
    snapshots: &HashMap<DeviceKind, ControllerSnapshot>,
) -> Option<u8> {
    mode_switch
        .iter()
        .position(|button| button.is_some_and(|button| button.is_pressed(snapshots)))
        .map(|index| index as u8 + 1)
}

/// The profile's shift button, which gives every control a second job while it is held.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LayerSetup {
    pub shift_button: Option<ButtonReference>,
}

impl LayerSetup {
    pub fn active_layer(
        &self,
        mode_switch: &ModeSwitchButtons,
        snapshots: &HashMap<DeviceKind, ControllerSnapshot>,
    ) -> ActiveLayer {
        ActiveLayer {
            mode: selected_mode(mode_switch, snapshots),
            shift_is_held: self.shift_button.is_some_and(|button| button.is_pressed(snapshots)),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActiveLayer {
    /// 1..=3, or `None` when no mode button is set up or none is pressed.
    pub mode: Option<u8>,
    pub shift_is_held: bool,
}

impl ActiveLayer {
    pub fn describe(&self) -> String {
        let mode_text = match self.mode {
            Some(mode) => tr_format("Mode {}", &[&mode]),
            None => tr("No mode").to_owned(),
        };
        if self.shift_is_held {
            tr_format("{} · shift held", &[&mode_text])
        } else {
            mode_text
        }
    }
}

/// When a binding is allowed to fire.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingLayer {
    /// `None` fires in every mode; `Some(2)` only while mode 2 is selected.
    pub mode: Option<u8>,
    /// Shift-layer bindings fire only while the shift button is held, all others only while it is not.
    pub shift_layer: bool,
}

impl BindingLayer {
    pub fn matches(&self, active_layer: ActiveLayer) -> bool {
        let mode_matches = self.mode.is_none() || self.mode == active_layer.mode;
        mode_matches && self.shift_layer == active_layer.shift_is_held
    }

    pub fn describe(&self) -> String {
        let mode_text = match self.mode {
            Some(mode) => format!("M{mode}"),
            None => tr("All modes").to_owned(),
        };
        if self.shift_layer {
            format!("{mode_text} · {}", tr("Shift"))
        } else {
            mode_text
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_specific_binding_fires_only_in_its_mode() {
        let layer = BindingLayer {
            mode: Some(2),
            shift_layer: false,
        };
        assert!(layer.matches(ActiveLayer {
            mode: Some(2),
            shift_is_held: false
        }));
        assert!(!layer.matches(ActiveLayer {
            mode: Some(1),
            shift_is_held: false
        }));
    }

    #[test]
    fn base_binding_is_silent_while_shift_is_held() {
        let layer = BindingLayer::default();
        assert!(!layer.matches(ActiveLayer {
            mode: None,
            shift_is_held: true
        }));
    }
}
