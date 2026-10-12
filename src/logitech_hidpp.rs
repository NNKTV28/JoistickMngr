//! Logitech HID++ 2.0: the protocol Logitech mice and keyboards use for their settings.
//!
//! Requests go to the receiver's vendor-defined interface as 20-byte "long" reports:
//! `[0x11, device index, feature index, function << 4 | software id, parameters…]`.
//! Each capability is a numbered feature (battery, DPI, report rate…) whose position in the
//! device's feature table is looked up once through the root feature.

use std::ffi::CString;
use std::time::{Duration, Instant};

use hidapi::{HidApi, HidDevice};

pub const LOGITECH_VENDOR_ID: u16 = 0x046D;
const VENDOR_DEFINED_PAGE: u16 = 0xFF00;
const LONG_REPORT_USAGE: u16 = 0x0002;
const SHORT_REPORT_ID: u8 = 0x10;
const LONG_REPORT_ID: u8 = 0x11;
const LONG_REPORT_LENGTH: usize = 20;
/// HID++ 1.0 error reports, sent by the receiver when a device slot is empty or asleep.
const RECEIVER_ERROR_SUB_ID: u8 = 0x8F;
/// HID++ 2.0 error responses use this in place of the feature index.
const FEATURE_ERROR_INDEX: u8 = 0xFF;
/// Identifies our requests among the device's own notifications.
const SOFTWARE_ID: u8 = 0x0A;
const RESPONSE_TIMEOUT: Duration = Duration::from_millis(800);
/// Receivers pair up to six devices, numbered from 1.
pub const RECEIVER_DEVICE_SLOTS: std::ops::RangeInclusive<u8> = 1..=6;

pub const ROOT_FEATURE: u16 = 0x0000;
pub const DEVICE_NAME: u16 = 0x0005;
pub const BATTERY_STATUS: u16 = 0x1000;
pub const UNIFIED_BATTERY: u16 = 0x1004;
pub const ADJUSTABLE_DPI: u16 = 0x2201;
pub const EXTENDED_ADJUSTABLE_DPI: u16 = 0x2202;
pub const REPORT_RATE: u16 = 0x8060;
pub const EXTENDED_REPORT_RATE: u16 = 0x8061;
pub const ONBOARD_PROFILES: u16 = 0x8100;

/// Parameters of one HID++ response, after the four header bytes.
pub type ResponseParameters = [u8; LONG_REPORT_LENGTH - 4];

#[derive(Debug)]
pub enum HidppError {
    /// The device answered with a HID++ error code.
    Device(u8),
    /// No answer arrived in time; the device may be asleep or switched off.
    NoAnswer,
    Transport(String),
}

impl std::fmt::Display for HidppError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HidppError::Device(code) => write!(formatter, "device error {code:#04x}"),
            HidppError::NoAnswer => write!(formatter, "no answer"),
            HidppError::Transport(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for HidppError {}

impl From<hidapi::HidError> for HidppError {
    fn from(error: hidapi::HidError) -> Self {
        HidppError::Transport(error.to_string())
    }
}

/// The HID++ interface of one receiver or directly connected device.
pub struct HidppChannel {
    long_reports: HidDevice,
}

/// Where a receiver's HID++ long-report collection is, so it can be opened later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HidppInterface {
    pub path: CString,
    pub product_id: u16,
}

/// Every Logitech HID++ interface currently plugged in.
pub fn find_hidpp_interfaces(hid_api: &HidApi) -> Vec<HidppInterface> {
    hid_api
        .device_list()
        .filter(|device| {
            device.vendor_id() == LOGITECH_VENDOR_ID
                && device.usage_page() == VENDOR_DEFINED_PAGE
                && device.usage() == LONG_REPORT_USAGE
        })
        .map(|device| HidppInterface {
            path: device.path().to_owned(),
            product_id: device.product_id(),
        })
        .collect()
}

impl HidppChannel {
    pub fn open(hid_api: &HidApi, interface: &HidppInterface) -> Result<Self, HidppError> {
        Ok(Self {
            long_reports: hid_api.open_path(&interface.path)?,
        })
    }

    /// Sends one request and waits for its answer, skipping unrelated notifications.
    pub fn request(
        &self,
        device_index: u8,
        feature_index: u8,
        function: u8,
        parameters: &[u8],
    ) -> Result<ResponseParameters, HidppError> {
        let function_and_software_id = (function << 4) | SOFTWARE_ID;
        self.long_reports.write(&build_long_report(
            device_index,
            feature_index,
            function_and_software_id,
            parameters,
        ))?;
        let started = Instant::now();
        while let Some(time_left) = RESPONSE_TIMEOUT.checked_sub(started.elapsed()) {
            let mut report = [0u8; LONG_REPORT_LENGTH];
            let length = self
                .long_reports
                .read_timeout(&mut report, time_left.as_millis() as i32)?;
            if length == 0 {
                break;
            }
            match classify_report(&report[..length], device_index, feature_index, function_and_software_id) {
                ReportMatch::Answer(parameters) => return Ok(parameters),
                ReportMatch::Error(code) => return Err(HidppError::Device(code)),
                ReportMatch::Unrelated => {}
            }
        }
        Err(HidppError::NoAnswer)
    }

    /// Position of a feature in the device's feature table, or `None` when it does not have it.
    pub fn feature_index(&self, device_index: u8, feature_id: u16) -> Result<Option<u8>, HidppError> {
        let [feature_high, feature_low] = feature_id.to_be_bytes();
        let answer = self.request(device_index, 0, 0, &[feature_high, feature_low])?;
        Ok((answer[0] != 0 || feature_id == ROOT_FEATURE).then_some(answer[0]))
    }
}

fn build_long_report(
    device_index: u8,
    feature_index: u8,
    function_and_software_id: u8,
    parameters: &[u8],
) -> [u8; LONG_REPORT_LENGTH] {
    let mut report = [0u8; LONG_REPORT_LENGTH];
    report[0] = LONG_REPORT_ID;
    report[1] = device_index;
    report[2] = feature_index;
    report[3] = function_and_software_id;
    let parameter_count = parameters.len().min(LONG_REPORT_LENGTH - 4);
    report[4..4 + parameter_count].copy_from_slice(&parameters[..parameter_count]);
    report
}

#[derive(Debug, PartialEq, Eq)]
enum ReportMatch {
    Answer(ResponseParameters),
    Error(u8),
    Unrelated,
}

/// Decides whether an incoming report answers our request, reports an error for it, or is
/// something else (a key press, a battery notification, another program's request).
fn classify_report(report: &[u8], device_index: u8, feature_index: u8, function_and_software_id: u8) -> ReportMatch {
    if report.len() < 7 || report[1] != device_index {
        return ReportMatch::Unrelated;
    }
    let is_receiver_error = report[0] == SHORT_REPORT_ID && report[2] == RECEIVER_ERROR_SUB_ID;
    let is_feature_error = report[2] == FEATURE_ERROR_INDEX;
    if (is_receiver_error || is_feature_error) && report[3] == feature_index && report[4] == function_and_software_id {
        return ReportMatch::Error(report[5]);
    }
    if report[0] == LONG_REPORT_ID && report[2] == feature_index && report[3] == function_and_software_id {
        let mut parameters = [0u8; LONG_REPORT_LENGTH - 4];
        let available = (report.len() - 4).min(parameters.len());
        parameters[..available].copy_from_slice(&report[4..4 + available]);
        return ReportMatch::Answer(parameters);
    }
    ReportMatch::Unrelated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_report_carries_header_and_parameters() {
        let report = build_long_report(1, 0, 0x0A, &[0x00, 0x05]);
        assert_eq!(&report[..6], &[0x11, 0x01, 0x00, 0x0A, 0x00, 0x05]);
        assert_eq!(report.len(), 20);
    }

    #[test]
    fn matching_long_report_is_the_answer() {
        let mut report = [0u8; 20];
        report[..6].copy_from_slice(&[0x11, 0x01, 0x03, 0x1A, 0x55, 0x01]);
        let ReportMatch::Answer(parameters) = classify_report(&report, 1, 3, 0x1A) else {
            panic!("expected an answer");
        };
        assert_eq!(&parameters[..2], &[0x55, 0x01]);
    }

    #[test]
    fn feature_error_for_our_request_is_reported() {
        let report = [0x11, 0x01, 0xFF, 0x03, 0x1A, 0x02, 0, 0];
        assert_eq!(classify_report(&report, 1, 3, 0x1A), ReportMatch::Error(0x02));
    }

    #[test]
    fn receiver_error_for_an_empty_slot_is_reported() {
        let report = [0x10, 0x02, 0x8F, 0x00, 0x1A, 0x09, 0x00];
        assert_eq!(classify_report(&report, 2, 0, 0x1A), ReportMatch::Error(0x09));
    }

    #[test]
    fn notification_from_another_feature_is_skipped() {
        let report = [0x11, 0x01, 0x05, 0x00, 0x10, 0x00, 0x00];
        assert_eq!(classify_report(&report, 1, 3, 0x1A), ReportMatch::Unrelated);
    }
}
