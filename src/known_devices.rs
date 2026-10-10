//! The peripherals this application knows how to manage, identified by USB vendor and product id.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeviceKind {
    Joystick,
    Throttle,
    RudderPedals,
}

impl DeviceKind {
    pub const ALL: [DeviceKind; 3] = [DeviceKind::Joystick, DeviceKind::Throttle, DeviceKind::RudderPedals];

    pub fn category_label(self) -> &'static str {
        match self {
            DeviceKind::Joystick => "Joystick",
            DeviceKind::Throttle => "Throttle",
            DeviceKind::RudderPedals => "Rudder",
        }
    }

    pub fn definition(self) -> &'static KnownDevice {
        KNOWN_DEVICES
            .iter()
            .find(|device| device.kind == self)
            .expect("every device kind has a definition")
    }
}

#[derive(Debug)]
pub struct AxisDefinition {
    pub name: &'static str,
    /// Centred axes rest in the middle (stick, rudder); others rest at one end (throttle, brakes).
    pub rests_at_center: bool,
}

#[derive(Debug)]
pub struct KnownDevice {
    pub kind: DeviceKind,
    pub product_name: &'static str,
    pub short_name: &'static str,
    pub vendor_id: u16,
    pub product_id: u16,
    pub supports_rgb_lighting: bool,
    pub axes: &'static [AxisDefinition],
}

impl KnownDevice {
    /// The `VID_xxxx&PID_xxxx` fragment Windows uses in device instance ids.
    pub fn hardware_id_fragment(&self) -> String {
        format!("VID_{:04X}&PID_{:04X}", self.vendor_id, self.product_id)
    }

    pub fn vendor_and_product_label(&self) -> String {
        format!("{:04X}:{:04X}", self.vendor_id, self.product_id)
    }

    pub fn axis_name(&self, axis_index: usize) -> String {
        match self.axes.get(axis_index) {
            Some(axis) => axis.name.to_owned(),
            None => format!("Axis {}", axis_index + 1),
        }
    }

    pub fn axis_rests_at_center(&self, axis_index: usize) -> bool {
        self.axes.get(axis_index).is_some_and(|axis| axis.rests_at_center)
    }
}

const fn centered_axis(name: &'static str) -> AxisDefinition {
    AxisDefinition {
        name,
        rests_at_center: true,
    }
}

const fn end_resting_axis(name: &'static str) -> AxisDefinition {
    AxisDefinition {
        name,
        rests_at_center: false,
    }
}

pub const LOGITECH_SAITEK_VENDOR_ID: u16 = 0x0738;
pub const SAITEK_VENDOR_ID: u16 = 0x06A3;

pub static KNOWN_DEVICES: [KnownDevice; 3] = [
    KnownDevice {
        kind: DeviceKind::Joystick,
        product_name: "X56 H.O.T.A.S. Stick",
        short_name: "X56 Stick",
        vendor_id: LOGITECH_SAITEK_VENDOR_ID,
        product_id: 0x2221,
        supports_rgb_lighting: true,
        axes: &[
            centered_axis("Roll (X)"),
            centered_axis("Pitch (Y)"),
            centered_axis("Thumb X"),
            centered_axis("Thumb Y"),
            centered_axis("Twist"),
        ],
    },
    KnownDevice {
        kind: DeviceKind::Throttle,
        product_name: "X56 H.O.T.A.S. Throttle",
        short_name: "X56 Throttle",
        vendor_id: LOGITECH_SAITEK_VENDOR_ID,
        product_id: 0xA221,
        supports_rgb_lighting: true,
        axes: &[
            end_resting_axis("Throttle L"),
            end_resting_axis("Throttle R"),
            end_resting_axis("Rotary F"),
            centered_axis("Mini stick X"),
            centered_axis("Mini stick Y"),
            end_resting_axis("Rotary G"),
            end_resting_axis("RTY3"),
            end_resting_axis("RTY4"),
        ],
    },
    KnownDevice {
        kind: DeviceKind::RudderPedals,
        product_name: "Pro Flight Rudder Pedals",
        short_name: "Rudder Pedals",
        vendor_id: SAITEK_VENDOR_ID,
        product_id: 0x0763,
        supports_rgb_lighting: false,
        axes: &[
            end_resting_axis("Left brake"),
            end_resting_axis("Right brake"),
            centered_axis("Rudder"),
        ],
    },
];

pub fn find_known_device(vendor_id: u16, product_id: u16) -> Option<&'static KnownDevice> {
    KNOWN_DEVICES
        .iter()
        .find(|device| device.vendor_id == vendor_id && device.product_id == product_id)
}

/// Describes what a USB interface of a device is used for, based on its interface number.
pub fn describe_interface_role(kind: DeviceKind, interface_number: Option<u8>) -> &'static str {
    match (kind, interface_number) {
        (DeviceKind::RudderPedals, _) => "Game controller (HID)",
        (_, Some(0)) => "Game controller (HID)",
        (_, Some(1)) => "Configuration and lighting (WinUSB)",
        (_, Some(2)) => "Vendor channel (HID)",
        (_, Some(_)) => "Additional interface",
        (_, None) => "USB composite parent",
    }
}
