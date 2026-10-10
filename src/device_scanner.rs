//! Finds the known peripherals in Windows' device tree and collects the drivers bound to each USB interface.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;
use wmi::WMIConnection;

use crate::known_devices::{KNOWN_DEVICES, KnownDevice, describe_interface_role};

const AUTOMATIC_RESCAN_INTERVAL: Duration = Duration::from_secs(10);
const RESCAN_REQUEST_POLL_INTERVAL: Duration = Duration::from_millis(250);
const DEVICE_FILTER: &str = "DeviceID LIKE '%VID_0738%' OR DeviceID LIKE '%VID_06A3%'";

#[derive(Clone, Debug)]
pub struct DeviceInterface {
    pub instance_id: String,
    pub interface_label: String,
    pub role: &'static str,
    pub device_name: String,
    pub driver_provider: String,
    pub driver_version: String,
    pub driver_date: String,
    pub inf_name: String,
    pub is_signed: bool,
    pub is_working: bool,
}

impl DeviceInterface {
    pub fn is_usb_node(&self) -> bool {
        self.instance_id.starts_with("USB\\")
    }

    pub fn is_hid_node(&self) -> bool {
        self.instance_id.starts_with("HID\\")
    }
}

#[derive(Clone, Debug)]
pub struct ConnectedDevice {
    pub definition: &'static KnownDevice,
    pub serial_number: Option<String>,
    pub interfaces: Vec<DeviceInterface>,
}

impl ConnectedDevice {
    /// The HID game controller node, whose driver decides how Windows reads the axes and buttons.
    pub fn game_controller_interface(&self) -> Option<&DeviceInterface> {
        self.interfaces
            .iter()
            .filter(|interface| interface.is_hid_node())
            .find(|interface| interface.role.starts_with("Game controller"))
    }

    pub fn usb_instance_ids(&self) -> Vec<String> {
        self.interfaces
            .iter()
            .filter(|interface| interface.is_usb_node())
            .map(|interface| interface.instance_id.clone())
            .collect()
    }

    pub fn all_drivers_signed(&self) -> bool {
        self.interfaces.iter().all(|interface| interface.is_signed)
    }

    pub fn all_interfaces_working(&self) -> bool {
        self.interfaces.iter().all(|interface| interface.is_working)
    }
}

#[derive(Clone, Debug, Default)]
pub enum ScanState {
    #[default]
    NotScannedYet,
    Scanned(Vec<ConnectedDevice>),
    Failed(String),
}

#[derive(Clone)]
pub struct DeviceScannerService {
    scan_state: Arc<Mutex<ScanState>>,
    rescan_requested: Arc<AtomicBool>,
}

impl DeviceScannerService {
    pub fn start() -> Self {
        let service = Self {
            scan_state: Arc::new(Mutex::new(ScanState::NotScannedYet)),
            rescan_requested: Arc::new(AtomicBool::new(true)),
        };
        let thread_service = service.clone();
        std::thread::Builder::new()
            .name("device-scanner".into())
            .spawn(move || thread_service.run_scan_loop())
            .expect("device scanner thread starts");
        service
    }

    pub fn scan_state(&self) -> ScanState {
        self.scan_state.lock().unwrap().clone()
    }

    pub fn connected_devices(&self) -> Vec<ConnectedDevice> {
        match self.scan_state() {
            ScanState::Scanned(devices) => devices,
            _ => Vec::new(),
        }
    }

    pub fn request_rescan(&self) {
        self.rescan_requested.store(true, Ordering::Relaxed);
    }

    fn run_scan_loop(&self) {
        let mut time_since_last_scan = Duration::ZERO;
        loop {
            let rescan_requested = self.rescan_requested.swap(false, Ordering::Relaxed);
            if rescan_requested || time_since_last_scan >= AUTOMATIC_RESCAN_INTERVAL {
                let new_state = match scan_connected_devices() {
                    Ok(devices) => ScanState::Scanned(devices),
                    Err(error) => ScanState::Failed(error.to_string()),
                };
                *self.scan_state.lock().unwrap() = new_state;
                time_since_last_scan = Duration::ZERO;
            }
            std::thread::sleep(RESCAN_REQUEST_POLL_INTERVAL);
            time_since_last_scan += RESCAN_REQUEST_POLL_INTERVAL;
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SignedDriverRecord {
    #[serde(rename = "DeviceID")]
    device_id: Option<String>,
    device_name: Option<String>,
    driver_version: Option<String>,
    driver_date: Option<String>,
    driver_provider_name: Option<String>,
    inf_name: Option<String>,
    is_signed: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PlugAndPlayEntityRecord {
    #[serde(rename = "DeviceID")]
    device_id: Option<String>,
    config_manager_error_code: Option<u32>,
}

pub fn scan_connected_devices() -> anyhow::Result<Vec<ConnectedDevice>> {
    let connection = WMIConnection::new()?;
    let driver_records = query_signed_drivers(&connection)?;
    let working_state_by_instance = query_working_state_by_instance(&connection)?;
    let devices = KNOWN_DEVICES
        .iter()
        .filter_map(|definition| assemble_connected_device(definition, &driver_records, &working_state_by_instance))
        .collect();
    Ok(devices)
}

fn query_signed_drivers(connection: &WMIConnection) -> anyhow::Result<Vec<SignedDriverRecord>> {
    let query = format!(
        "SELECT DeviceID, DeviceName, DriverVersion, DriverDate, DriverProviderName, InfName, IsSigned \
         FROM Win32_PnPSignedDriver WHERE {DEVICE_FILTER}"
    );
    Ok(connection.raw_query(query)?)
}

fn query_working_state_by_instance(connection: &WMIConnection) -> anyhow::Result<HashMap<String, bool>> {
    let query = format!("SELECT DeviceID, ConfigManagerErrorCode FROM Win32_PnPEntity WHERE {DEVICE_FILTER}");
    let records: Vec<PlugAndPlayEntityRecord> = connection.raw_query(query)?;
    let working_state = records
        .into_iter()
        .filter_map(|record| {
            let is_working = record.config_manager_error_code.unwrap_or(0) == 0;
            Some((record.device_id?.to_uppercase(), is_working))
        })
        .collect();
    Ok(working_state)
}

fn assemble_connected_device(
    definition: &'static KnownDevice,
    driver_records: &[SignedDriverRecord],
    working_state_by_instance: &HashMap<String, bool>,
) -> Option<ConnectedDevice> {
    let hardware_id = definition.hardware_id_fragment();
    let mut interfaces: Vec<DeviceInterface> = driver_records
        .iter()
        .filter(|record| record_belongs_to(record, &hardware_id))
        .filter_map(|record| build_device_interface(definition, record, working_state_by_instance))
        .collect();
    if interfaces.is_empty() {
        return None;
    }
    interfaces.sort_by(|first, second| first.interface_label.cmp(&second.interface_label));
    Some(ConnectedDevice {
        definition,
        serial_number: find_serial_number(&interfaces, &hardware_id),
        interfaces,
    })
}

fn record_belongs_to(record: &SignedDriverRecord, hardware_id: &str) -> bool {
    record
        .device_id
        .as_deref()
        .is_some_and(|device_id| device_id.to_uppercase().contains(hardware_id))
}

fn build_device_interface(
    definition: &KnownDevice,
    record: &SignedDriverRecord,
    working_state_by_instance: &HashMap<String, bool>,
) -> Option<DeviceInterface> {
    let instance_id = record.device_id.clone()?;
    let interface_number = parse_interface_number(&instance_id);
    Some(DeviceInterface {
        interface_label: build_interface_label(&instance_id, interface_number),
        role: describe_interface_role(definition.kind, interface_number),
        device_name: record.device_name.clone().unwrap_or_default(),
        driver_provider: record.driver_provider_name.clone().unwrap_or_else(|| "Unknown".into()),
        driver_version: record.driver_version.clone().unwrap_or_else(|| "—".into()),
        driver_date: format_wmi_date(record.driver_date.as_deref()),
        inf_name: record.inf_name.clone().unwrap_or_default(),
        is_signed: record.is_signed.unwrap_or(false),
        is_working: working_state_by_instance
            .get(&instance_id.to_uppercase())
            .copied()
            .unwrap_or(true),
        instance_id,
    })
}

/// Reads the `MI_xx` part of an instance id such as `USB\VID_0738&PID_2221&MI_01\...`.
fn parse_interface_number(instance_id: &str) -> Option<u8> {
    let hardware_part = instance_id.split('\\').nth(1)?;
    let interface_part = hardware_part.split('&').find(|segment| segment.starts_with("MI_"))?;
    u8::from_str_radix(&interface_part[3..], 16).ok()
}

fn build_interface_label(instance_id: &str, interface_number: Option<u8>) -> String {
    let bus_name = instance_id.split('\\').next().unwrap_or("USB");
    match interface_number {
        Some(number) => format!("MI_{number:02} · {bus_name}"),
        None if bus_name == "USB" => "USB device".to_owned(),
        None => format!("{bus_name} device"),
    }
}

/// Converts a CIM date such as `20180614000000.******+***` to `2018-06-14`.
fn format_wmi_date(raw_date: Option<&str>) -> String {
    match raw_date {
        Some(date) if date.len() >= 8 => format!("{}-{}-{}", &date[0..4], &date[4..6], &date[6..8]),
        _ => "—".to_owned(),
    }
}

/// USB devices that report a serial number use it as the last part of the parent instance id.
fn find_serial_number(interfaces: &[DeviceInterface], hardware_id: &str) -> Option<String> {
    let parent_prefix = format!("USB\\{hardware_id}\\");
    interfaces
        .iter()
        .filter_map(|interface| {
            interface
                .instance_id
                .to_uppercase()
                .strip_prefix(&parent_prefix)
                .map(str::to_owned)
        })
        .find(|instance_suffix| !instance_suffix.contains('&'))
}
