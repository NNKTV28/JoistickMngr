//! Logitech mice over HID++: battery, DPI, report rate and whether an onboard profile is in charge.
//!
//! A background thread finds the receiver, asks the mouse for its settings every few seconds and
//! applies changes the interface asks for, reading them back so the page shows what took effect.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hidapi::HidApi;

use crate::logitech_hidpp::{
    ADJUSTABLE_DPI, BATTERY_STATUS, DEVICE_NAME, EXTENDED_ADJUSTABLE_DPI, EXTENDED_REPORT_RATE, HidppChannel,
    HidppError, ONBOARD_PROFILES, RECEIVER_DEVICE_SLOTS, REPORT_RATE, UNIFIED_BATTERY, find_hidpp_interfaces,
};

const STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const DIRECT_CONNECTION_INDEX: u8 = 0xFF;
/// Marks a range in a DPI list: the next value is the end, these 13 bits are the step.
const DPI_RANGE_MARKER: u16 = 0b111 << 13;
const DPI_RANGE_STEP_MASK: u16 = 0x1FFF;
const ONBOARD_MODE: u8 = 1;
/// Report intervals of the extended report rate feature, by bit position: 8 ms down to 125 µs.
const EXTENDED_REPORT_RATES_HZ: [u16; 7] = [125, 250, 500, 1000, 2000, 4000, 8000];

#[derive(Clone, Debug, PartialEq)]
pub struct BatteryStatus {
    pub percent: u8,
    pub is_charging: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DpiSettings {
    pub current: u16,
    /// Every DPI value the sensor accepts, lowest first.
    pub choices: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReportRateSettings {
    pub current_hz: u16,
    pub choices_hz: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LogitechMouseStatus {
    pub name: String,
    pub battery: Option<BatteryStatus>,
    pub dpi: Option<DpiSettings>,
    pub report_rate: Option<ReportRateSettings>,
    /// The onboard profile in use, when the mouse runs from its own memory rather than from software.
    pub active_onboard_profile: Option<u8>,
}

enum MouseCommand {
    SetDpi(u16),
    SetReportRate(u16),
}

#[derive(Clone)]
pub struct LogitechMouseService {
    status: Arc<Mutex<Option<LogitechMouseStatus>>>,
    commands: Sender<MouseCommand>,
}

impl LogitechMouseService {
    pub fn start() -> Self {
        let (commands, received_commands) = mpsc::channel();
        let service = Self {
            status: Arc::new(Mutex::new(None)),
            commands,
        };
        let status = Arc::clone(&service.status);
        std::thread::Builder::new()
            .name("logitech-mouse".into())
            .spawn(move || run_mouse_thread(&status, &received_commands))
            .expect("mouse thread starts");
        service
    }

    /// The connected mouse, or `None` when no Logitech mouse answers.
    pub fn status(&self) -> Option<LogitechMouseStatus> {
        self.status.lock().unwrap().clone()
    }

    pub fn set_dpi(&self, dpi: u16) {
        let _ = self.commands.send(MouseCommand::SetDpi(dpi));
    }

    pub fn set_report_rate(&self, rate_hz: u16) {
        let _ = self.commands.send(MouseCommand::SetReportRate(rate_hz));
    }
}

fn run_mouse_thread(shared_status: &Mutex<Option<LogitechMouseStatus>>, commands: &Receiver<MouseCommand>) {
    let Ok(mut hid_api) = HidApi::new() else {
        return;
    };
    loop {
        let _ = hid_api.refresh_devices();
        let connection = connect_to_mouse(&hid_api);
        *shared_status.lock().unwrap() = connection.as_ref().and_then(MouseConnection::read_status);
        match commands.recv_timeout(STATUS_REFRESH_INTERVAL) {
            Ok(command) => {
                if let Some(connection) = &connection {
                    connection.apply(command);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// A mouse that answered, with where to reach it.
struct MouseConnection {
    channel: HidppChannel,
    device_index: u8,
}

fn connect_to_mouse(hid_api: &HidApi) -> Option<MouseConnection> {
    for interface in find_hidpp_interfaces(hid_api) {
        let Ok(channel) = HidppChannel::open(hid_api, &interface) else {
            continue;
        };
        let mouse_slot = RECEIVER_DEVICE_SLOTS
            .chain([DIRECT_CONNECTION_INDEX])
            .find(|device_index| {
                has_feature(&channel, *device_index, ADJUSTABLE_DPI)
                    || has_feature(&channel, *device_index, EXTENDED_ADJUSTABLE_DPI)
            });
        if let Some(device_index) = mouse_slot {
            return Some(MouseConnection { channel, device_index });
        }
    }
    None
}

fn has_feature(channel: &HidppChannel, device_index: u8, feature_id: u16) -> bool {
    matches!(channel.feature_index(device_index, feature_id), Ok(Some(_)))
}

impl MouseConnection {
    fn call(&self, feature_id: u16, function: u8, parameters: &[u8]) -> Result<[u8; 16], HidppError> {
        let feature_index = self
            .channel
            .feature_index(self.device_index, feature_id)?
            .ok_or(HidppError::Device(0))?;
        self.channel
            .request(self.device_index, feature_index, function, parameters)
    }

    fn read_status(&self) -> Option<LogitechMouseStatus> {
        Some(LogitechMouseStatus {
            name: self.read_name().ok()?,
            battery: self.read_battery(),
            dpi: self.read_dpi(),
            report_rate: self.read_report_rate(),
            active_onboard_profile: self.read_active_onboard_profile(),
        })
    }

    fn read_name(&self) -> Result<String, HidppError> {
        let name_length = usize::from(self.call(DEVICE_NAME, 0, &[])?[0]);
        let mut name_bytes = Vec::with_capacity(name_length);
        while name_bytes.len() < name_length {
            let chunk = self.call(DEVICE_NAME, 1, &[name_bytes.len() as u8])?;
            let remaining = name_length - name_bytes.len();
            name_bytes.extend(chunk.iter().take(remaining).take_while(|byte| **byte != 0));
            if chunk[0] == 0 {
                break;
            }
        }
        Ok(String::from_utf8_lossy(&name_bytes).into_owned())
    }

    fn read_battery(&self) -> Option<BatteryStatus> {
        if let Ok(answer) = self.call(UNIFIED_BATTERY, 1, &[]) {
            return Some(parse_unified_battery(&answer));
        }
        self.call(BATTERY_STATUS, 0, &[])
            .ok()
            .map(|answer| parse_battery_status(&answer))
    }

    fn read_dpi(&self) -> Option<DpiSettings> {
        self.read_extended_dpi().or_else(|| self.read_classic_dpi())
    }

    fn read_extended_dpi(&self) -> Option<DpiSettings> {
        let current = u16::from_be_bytes(
            self.call(EXTENDED_ADJUSTABLE_DPI, 5, &[0]).ok()?[1..3]
                .try_into()
                .ok()?,
        );
        let mut list_bytes = Vec::new();
        for page in 0..=u8::MAX {
            let answer = self.call(EXTENDED_ADJUSTABLE_DPI, 2, &[0, 0, page]).ok()?;
            list_bytes.extend_from_slice(&answer[3..]);
            if list_ends(&list_bytes) {
                break;
            }
        }
        Some(DpiSettings {
            current,
            choices: expand_dpi_list(&list_bytes),
        })
    }

    fn read_classic_dpi(&self) -> Option<DpiSettings> {
        let current = u16::from_be_bytes(self.call(ADJUSTABLE_DPI, 2, &[0]).ok()?[1..3].try_into().ok()?);
        let list_answer = self.call(ADJUSTABLE_DPI, 1, &[0]).ok()?;
        Some(DpiSettings {
            current,
            choices: expand_dpi_list(&list_answer[1..]),
        })
    }

    fn read_report_rate(&self) -> Option<ReportRateSettings> {
        if let (Ok(list), Ok(current)) = (
            self.call(EXTENDED_REPORT_RATE, 1, &[]),
            self.call(EXTENDED_REPORT_RATE, 2, &[]),
        ) {
            return Some(parse_extended_report_rate(&list, current[0]));
        }
        let list = self.call(REPORT_RATE, 0, &[]).ok()?;
        let current = self.call(REPORT_RATE, 1, &[]).ok()?;
        Some(parse_classic_report_rate(list[0], current[0]))
    }

    fn read_active_onboard_profile(&self) -> Option<u8> {
        let mode = self.call(ONBOARD_PROFILES, 2, &[]).ok()?[0];
        if mode != ONBOARD_MODE {
            return None;
        }
        self.call(ONBOARD_PROFILES, 4, &[]).ok().map(|answer| answer[1])
    }

    fn apply(&self, command: MouseCommand) {
        let _ = match command {
            MouseCommand::SetDpi(dpi) => self.write_dpi(dpi),
            MouseCommand::SetReportRate(rate_hz) => self.write_report_rate(rate_hz),
        };
    }

    fn write_dpi(&self, dpi: u16) -> Result<(), HidppError> {
        let [dpi_high, dpi_low] = dpi.to_be_bytes();
        if self
            .call(
                EXTENDED_ADJUSTABLE_DPI,
                6,
                &[0, dpi_high, dpi_low, dpi_high, dpi_low, 0],
            )
            .is_ok()
        {
            return Ok(());
        }
        self.call(ADJUSTABLE_DPI, 3, &[0, dpi_high, dpi_low]).map(|_| ())
    }

    fn write_report_rate(&self, rate_hz: u16) -> Result<(), HidppError> {
        if let Some(rate_index) = EXTENDED_REPORT_RATES_HZ.iter().position(|rate| *rate == rate_hz)
            && self.call(EXTENDED_REPORT_RATE, 3, &[rate_index as u8]).is_ok()
        {
            return Ok(());
        }
        let interval_milliseconds = (1000 / rate_hz.max(1)) as u8;
        self.call(REPORT_RATE, 2, &[interval_milliseconds]).map(|_| ())
    }
}

/// Unified battery status: state of charge in percent, then level flags, then charging state.
fn parse_unified_battery(answer: &[u8]) -> BatteryStatus {
    BatteryStatus {
        percent: answer[0].min(100),
        is_charging: matches!(answer[2], 1 | 2),
    }
}

/// Older battery status: level in percent, next level, then charging state.
fn parse_battery_status(answer: &[u8]) -> BatteryStatus {
    BatteryStatus {
        percent: answer[0].min(100),
        is_charging: matches!(answer[2], 1..=3),
    }
}

/// A DPI list ends with a zero value; the pages after it are empty.
fn list_ends(list_bytes: &[u8]) -> bool {
    list_bytes.ends_with(&[0, 0])
}

/// Turns the device's compact list into every value: plain entries are single DPI values, and a
/// range marker (`111` in the top bits plus a step) fills in from the previous value to the next one.
fn expand_dpi_list(list_bytes: &[u8]) -> Vec<u16> {
    let values: Vec<u16> = list_bytes
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .take_while(|value| *value != 0)
        .collect();
    let mut choices: Vec<u16> = Vec::new();
    let mut position = 0;
    while position < values.len() {
        let value = values[position];
        let is_range = value & DPI_RANGE_MARKER == DPI_RANGE_MARKER;
        match (is_range, choices.last(), values.get(position + 1)) {
            (true, Some(&start), Some(&end)) => {
                let step = (value & DPI_RANGE_STEP_MASK).max(1);
                choices.extend((start + step..=end).step_by(usize::from(step)));
                position += 2;
            }
            (true, _, _) => position += 2,
            (false, _, _) => {
                choices.push(value);
                position += 1;
            }
        }
    }
    choices
}

/// Extended report rate: a bit per supported rate (8 ms upwards) and the index of the current one.
fn parse_extended_report_rate(list: &[u8], current_index: u8) -> ReportRateSettings {
    let supported_flags = u16::from_be_bytes([list[0], list[1]]);
    ReportRateSettings {
        current_hz: EXTENDED_REPORT_RATES_HZ
            .get(usize::from(current_index))
            .copied()
            .unwrap_or(0),
        choices_hz: EXTENDED_REPORT_RATES_HZ
            .iter()
            .enumerate()
            .filter(|(bit, _)| supported_flags & (1 << bit) != 0)
            .map(|(_, rate)| *rate)
            .collect(),
    }
}

/// Classic report rate: bit n means an interval of n + 1 milliseconds is supported.
fn parse_classic_report_rate(supported_intervals: u8, current_interval_ms: u8) -> ReportRateSettings {
    let mut choices_hz: Vec<u16> = (0..8)
        .filter(|bit| supported_intervals & (1 << bit) != 0)
        .map(|bit| 1000 / (bit + 1))
        .collect();
    choices_hz.sort_unstable();
    ReportRateSettings {
        current_hz: 1000 / u16::from(current_interval_ms.max(1)),
        choices_hz,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g309_dpi_list_expands_both_ranges() {
        let list_bytes = [0x00, 0x64, 0xE0, 0x32, 0x32, 0x00, 0xE0, 0x64, 0x64, 0x00, 0x00, 0x00];
        let choices = expand_dpi_list(&list_bytes);
        assert_eq!(choices.first(), Some(&100));
        assert!(choices.contains(&1600));
        assert!(choices.contains(&12800));
        assert!(choices.contains(&12900));
        assert_eq!(choices.last(), Some(&25600));
        assert!(!choices.contains(&12850));
    }

    #[test]
    fn plain_dpi_list_is_kept_as_is() {
        let list_bytes = [0x01, 0x90, 0x03, 0x20, 0x06, 0x40, 0x00, 0x00];
        assert_eq!(expand_dpi_list(&list_bytes), [400, 800, 1600]);
    }

    #[test]
    fn g309_report_rates_are_read_from_flags() {
        let settings = parse_extended_report_rate(&[0x00, 0x0F], 3);
        assert_eq!(settings.choices_hz, [125, 250, 500, 1000]);
        assert_eq!(settings.current_hz, 1000);
    }

    #[test]
    fn classic_report_rates_come_from_intervals() {
        let settings = parse_classic_report_rate(0b1000_1011, 1);
        assert_eq!(settings.choices_hz, [125, 250, 500, 1000]);
        assert_eq!(settings.current_hz, 1000);
    }

    #[test]
    fn unified_battery_reads_percent_and_charging() {
        assert_eq!(
            parse_unified_battery(&[0x46, 0x08, 0x00, 0x00]),
            BatteryStatus {
                percent: 70,
                is_charging: false
            }
        );
    }
}
