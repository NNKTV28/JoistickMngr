//! Compares installed drivers with the newest versions the vendor has published.

use std::cmp::Ordering;

use crate::version_numbers::compare_versions;

use winreg::RegKey;
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};

use crate::device_scanner::ConnectedDevice;
use crate::known_devices::DeviceKind;
use crate::localization::{tr, tr_format};

const UNINSTALL_REGISTRY_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const VENDOR_PROVIDER_NAME: &str = "Logitech";

pub struct VendorDriverRelease {
    pub package_name: &'static str,
    /// Text that identifies the package in "Apps & features".
    pub installed_package_match: &'static str,
    pub latest_package_version: Option<&'static str>,
    pub latest_driver_version: Option<&'static str>,
    pub release_date: Option<&'static str>,
    pub download_page_url: &'static str,
    /// When false, the Windows built-in driver is a complete solution for the device.
    pub vendor_driver_required: bool,
}

const X56_RELEASE: VendorDriverRelease = VendorDriverRelease {
    package_name: "Logitech X56 H.O.T.A.S.",
    installed_package_match: "X56",
    latest_package_version: Some("8.0.213.0"),
    latest_driver_version: Some("8.0.197.0"),
    release_date: Some("2018-09-24"),
    download_page_url: "https://support.logi.com/hc/en-us/articles/360025130494",
    vendor_driver_required: true,
};

const RUDDER_PEDALS_RELEASE: VendorDriverRelease = VendorDriverRelease {
    package_name: "Saitek Pro Flight Rudder Pedals",
    installed_package_match: "Rudder Pedals",
    latest_package_version: None,
    latest_driver_version: None,
    release_date: None,
    download_page_url: "https://support.logi.com/hc/en-us/search?query=Pro%20Flight%20Rudder%20Pedals",
    vendor_driver_required: false,
};

pub fn vendor_release_for(device_kind: DeviceKind) -> &'static VendorDriverRelease {
    match device_kind {
        DeviceKind::Joystick | DeviceKind::Throttle => &X56_RELEASE,
        DeviceKind::RudderPedals => &RUDDER_PEDALS_RELEASE,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DriverHealth {
    UpToDate {
        installed_version: String,
    },
    Outdated {
        installed_version: String,
        latest_version: String,
    },
    VendorDriverMissing,
    UsingWindowsDriver,
    DeviceProblem,
}

impl DriverHealth {
    pub fn summary(&self) -> String {
        match self {
            DriverHealth::UpToDate { .. } => tr("Driver up to date").to_owned(),
            DriverHealth::Outdated { latest_version, .. } => tr_format("Update available ({})", &[latest_version]),
            DriverHealth::VendorDriverMissing => tr("Logitech driver missing").to_owned(),
            DriverHealth::UsingWindowsDriver => tr("Windows driver in use").to_owned(),
            DriverHealth::DeviceProblem => tr("Device reports a problem").to_owned(),
        }
    }

    pub fn needs_attention(&self) -> bool {
        matches!(
            self,
            DriverHealth::Outdated { .. } | DriverHealth::VendorDriverMissing | DriverHealth::DeviceProblem
        )
    }
}

pub fn evaluate_driver_health(device: &ConnectedDevice) -> DriverHealth {
    if !device.all_interfaces_working() {
        return DriverHealth::DeviceProblem;
    }
    let release = vendor_release_for(device.definition.kind);
    let Some(vendor_driver_version) = installed_vendor_driver_version(device) else {
        return if release.vendor_driver_required {
            DriverHealth::VendorDriverMissing
        } else {
            DriverHealth::UsingWindowsDriver
        };
    };
    match release.latest_driver_version {
        Some(latest_version) if compare_versions(&vendor_driver_version, latest_version) == Ordering::Less => {
            DriverHealth::Outdated {
                installed_version: vendor_driver_version,
                latest_version: latest_version.into(),
            }
        }
        _ => DriverHealth::UpToDate {
            installed_version: vendor_driver_version,
        },
    }
}

/// The version of the vendor driver on the game controller interface, if a vendor driver is bound.
pub fn installed_vendor_driver_version(device: &ConnectedDevice) -> Option<String> {
    device
        .game_controller_interface()
        .filter(|interface| interface.driver_provider.eq_ignore_ascii_case(VENDOR_PROVIDER_NAME))
        .map(|interface| interface.driver_version.clone())
}

/// Reads the version shown in "Apps & features" for the vendor software package.
pub fn installed_package_version(device_kind: DeviceKind) -> Option<String> {
    let release = vendor_release_for(device_kind);
    [KEY_WOW64_64KEY, KEY_WOW64_32KEY]
        .into_iter()
        .find_map(|registry_view| find_installed_package_version(release.installed_package_match, registry_view))
}

fn find_installed_package_version(name_fragment: &str, registry_view: u32) -> Option<String> {
    let uninstall_key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(UNINSTALL_REGISTRY_KEY, KEY_READ | registry_view)
        .ok()?;
    uninstall_key.enum_keys().flatten().find_map(|product_key_name| {
        let product_key = uninstall_key.open_subkey(&product_key_name).ok()?;
        let display_name: String = product_key.get_value("DisplayName").ok()?;
        if !display_name.contains(name_fragment) {
            return None;
        }
        product_key.get_value("DisplayVersion").ok()
    })
}
