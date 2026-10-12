//! Spots plugged-in peripherals that have a product photo, such as the keyboard, by the name the
//! device itself reports over USB.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use hidapi::HidApi;

use crate::product_images::ProductPhoto;

const SCAN_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsbPeripheral {
    pub name: String,
    pub photo: ProductPhoto,
    pub vendor_id: u16,
    pub product_id: u16,
}

#[derive(Clone, Default)]
pub struct UsbPeripheralScanner {
    connected: Arc<Mutex<Vec<UsbPeripheral>>>,
}

impl UsbPeripheralScanner {
    pub fn start() -> Self {
        let scanner = Self::default();
        let connected = Arc::clone(&scanner.connected);
        std::thread::Builder::new()
            .name("usb-peripherals".into())
            .spawn(move || run_scan_loop(&connected))
            .expect("peripheral scan thread starts");
        scanner
    }

    pub fn connected(&self) -> Vec<UsbPeripheral> {
        self.connected.lock().unwrap().clone()
    }
}

fn run_scan_loop(connected: &Mutex<Vec<UsbPeripheral>>) {
    let Ok(mut hid_api) = HidApi::new() else {
        return;
    };
    loop {
        let _ = hid_api.refresh_devices();
        *connected.lock().unwrap() = find_peripherals(&hid_api);
        std::thread::sleep(SCAN_INTERVAL);
    }
}

/// One entry per physical device, even though most expose several HID interfaces.
fn find_peripherals(hid_api: &HidApi) -> Vec<UsbPeripheral> {
    let mut peripherals: Vec<UsbPeripheral> = Vec::new();
    for device in hid_api.device_list() {
        let Some(name) = device.product_string() else {
            continue;
        };
        let Some(photo) = ProductPhoto::matching_name(name) else {
            continue;
        };
        if peripherals.iter().any(|known| known.photo == photo) {
            continue;
        }
        peripherals.push(UsbPeripheral {
            name: name.trim().to_owned(),
            photo,
            vendor_id: device.vendor_id(),
            product_id: device.product_id(),
        });
    }
    peripherals
}
