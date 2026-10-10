//! Reads a game controller straight from its HID interface.
//!
//! Unlike Windows.Gaming.Input this keeps working while another window (a game) has focus,
//! which key mapping depends on. The HID report descriptor tells us where every axis,
//! button and hat lives in the report, so no device-specific parsing is needed.

use std::ffi::CStr;

use windows::Win32::Devices::HumanInterfaceDevice::{
    HIDP_BUTTON_CAPS, HIDP_CAPS, HIDP_STATUS_SUCCESS, HIDP_VALUE_CAPS, HidD_FreePreparsedData, HidD_GetPreparsedData,
    HidP_GetButtonCaps, HidP_GetCaps, HidP_GetUsageValue, HidP_GetUsages, HidP_GetValueCaps, HidP_Input,
    PHIDP_PREPARSED_DATA,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile,
};
use windows::core::HSTRING;

pub const GENERIC_DESKTOP_PAGE: u16 = 0x01;
pub const SIMULATION_CONTROLS_PAGE: u16 = 0x02;
pub const BUTTON_PAGE: u16 = 0x09;
pub const JOYSTICK_USAGE: u16 = 0x04;
pub const GAMEPAD_USAGE: u16 = 0x05;
pub const HAT_SWITCH_USAGE: u16 = 0x39;

/// One analog input in the report, in the order the controller's axes are presented.
#[derive(Clone, Debug)]
pub struct AxisLayout {
    pub usage_page: u16,
    pub usage: u16,
    link_collection: u16,
    logical_minimum: i32,
    logical_maximum: i32,
    bit_size: u16,
}

#[derive(Clone, Debug)]
struct HatLayout {
    usage_page: u16,
    link_collection: u16,
    logical_minimum: i32,
    logical_maximum: i32,
}

/// Raw values from one input report. Axes are 0..=1, hats are 0..=7 clockwise from up or `None` when centred.
#[derive(Clone, Debug, Default)]
pub struct RawControllerState {
    pub axis_values: Vec<f64>,
    pub button_states: Vec<bool>,
    pub hat_positions: Vec<Option<u8>>,
}

pub struct HidController {
    device_handle: HANDLE,
    preparsed_data: PHIDP_PREPARSED_DATA,
    input_report_length: usize,
    axes: Vec<AxisLayout>,
    hats: Vec<HatLayout>,
    button_count: usize,
}

// The handle and preparsed data are only used from the thread that owns the controller.
unsafe impl Send for HidController {}

impl HidController {
    pub fn open(device_path: &CStr) -> anyhow::Result<Self> {
        let device_handle = open_shared_for_reading(device_path)?;
        let mut preparsed_data = PHIDP_PREPARSED_DATA::default();
        // SAFETY: device_handle is a valid HID handle; preparsed_data receives an allocation freed in Drop.
        if !unsafe { HidD_GetPreparsedData(device_handle, &mut preparsed_data) } {
            unsafe { CloseHandle(device_handle)? };
            anyhow::bail!("the device did not return its report layout");
        }
        let capabilities = read_capabilities(preparsed_data)?;
        let value_capabilities = read_value_capabilities(preparsed_data, capabilities.NumberInputValueCaps)?;
        Ok(Self {
            device_handle,
            preparsed_data,
            input_report_length: capabilities.InputReportByteLength as usize,
            axes: collect_axis_layouts(&value_capabilities),
            hats: collect_hat_layouts(&value_capabilities),
            button_count: count_buttons(preparsed_data, capabilities.NumberInputButtonCaps)?,
        })
    }

    pub fn axis_layouts(&self) -> &[AxisLayout] {
        &self.axes
    }

    /// Blocks until the device sends its next input report.
    pub fn read_state(&self) -> anyhow::Result<RawControllerState> {
        let mut report = vec![0u8; self.input_report_length];
        let mut bytes_read = 0u32;
        // SAFETY: the buffer is as long as the device's input reports.
        unsafe { ReadFile(self.device_handle, Some(&mut report), Some(&mut bytes_read), None)? };
        Ok(RawControllerState {
            axis_values: self.axes.iter().map(|axis| self.read_axis(axis, &report)).collect(),
            button_states: self.read_buttons(&mut report),
            hat_positions: self.hats.iter().map(|hat| self.read_hat(hat, &report)).collect(),
        })
    }

    fn read_raw_value(&self, usage_page: u16, link_collection: u16, usage: u16, report: &[u8]) -> Option<u32> {
        let mut raw_value = 0u32;
        // SAFETY: report is a complete input report for this device's preparsed data.
        let status = unsafe {
            HidP_GetUsageValue(
                HidP_Input,
                usage_page,
                Some(link_collection),
                usage,
                &mut raw_value,
                self.preparsed_data,
                report,
            )
        };
        (status == HIDP_STATUS_SUCCESS).then_some(raw_value)
    }

    fn read_axis(&self, axis: &AxisLayout, report: &[u8]) -> f64 {
        let Some(raw_value) = self.read_raw_value(axis.usage_page, axis.link_collection, axis.usage, report) else {
            return 0.0;
        };
        let value = sign_extend_if_needed(raw_value, axis.bit_size, axis.logical_minimum);
        let range = (axis.logical_maximum - axis.logical_minimum).max(1) as f64;
        ((value - axis.logical_minimum) as f64 / range).clamp(0.0, 1.0)
    }

    fn read_hat(&self, hat: &HatLayout, report: &[u8]) -> Option<u8> {
        let raw_value = self.read_raw_value(hat.usage_page, hat.link_collection, HAT_SWITCH_USAGE, report)? as i32;
        let is_pressed = (hat.logical_minimum..=hat.logical_maximum).contains(&raw_value);
        let position_count = hat.logical_maximum - hat.logical_minimum + 1;
        let eighths_per_step = (8 / position_count.max(1)) as u8;
        is_pressed.then(|| (raw_value - hat.logical_minimum) as u8 * eighths_per_step.max(1))
    }

    fn read_buttons(&self, report: &mut [u8]) -> Vec<bool> {
        let mut button_states = vec![false; self.button_count];
        let mut pressed_usages = vec![0u16; self.button_count.max(1)];
        let mut pressed_count = pressed_usages.len() as u32;
        // SAFETY: the usage list is sized for every button the device declares.
        let status = unsafe {
            HidP_GetUsages(
                HidP_Input,
                BUTTON_PAGE,
                None,
                pressed_usages.as_mut_ptr(),
                &mut pressed_count,
                self.preparsed_data,
                report,
            )
        };
        if status == HIDP_STATUS_SUCCESS {
            for usage in &pressed_usages[..pressed_count as usize] {
                if let Some(state) = button_states.get_mut(usize::from(*usage).saturating_sub(1)) {
                    *state = true;
                }
            }
        }
        button_states
    }
}

impl Drop for HidController {
    fn drop(&mut self) {
        // SAFETY: both were obtained in `open` and are released exactly once.
        unsafe {
            HidD_FreePreparsedData(self.preparsed_data);
            let _ = CloseHandle(self.device_handle);
        }
    }
}

fn open_shared_for_reading(device_path: &CStr) -> anyhow::Result<HANDLE> {
    let path = HSTRING::from(device_path.to_string_lossy().as_ref());
    // SAFETY: plain CreateFileW call; sharing lets games keep reading the same device.
    let handle = unsafe {
        CreateFileW(
            &path,
            GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )?
    };
    Ok(handle)
}

fn read_capabilities(preparsed_data: PHIDP_PREPARSED_DATA) -> anyhow::Result<HIDP_CAPS> {
    let mut capabilities = HIDP_CAPS::default();
    // SAFETY: preparsed_data is valid; capabilities is an out parameter.
    let status = unsafe { HidP_GetCaps(preparsed_data, &mut capabilities) };
    anyhow::ensure!(status == HIDP_STATUS_SUCCESS, "could not read the device capabilities");
    Ok(capabilities)
}

fn read_value_capabilities(preparsed_data: PHIDP_PREPARSED_DATA, count: u16) -> anyhow::Result<Vec<HIDP_VALUE_CAPS>> {
    let mut value_capabilities = vec![HIDP_VALUE_CAPS::default(); usize::from(count)];
    let mut length = count;
    if count > 0 {
        // SAFETY: the buffer holds `count` entries as reported by HidP_GetCaps.
        let status =
            unsafe { HidP_GetValueCaps(HidP_Input, value_capabilities.as_mut_ptr(), &mut length, preparsed_data) };
        anyhow::ensure!(status == HIDP_STATUS_SUCCESS, "could not read the axis layout");
    }
    value_capabilities.truncate(usize::from(length));
    Ok(value_capabilities)
}

fn count_buttons(preparsed_data: PHIDP_PREPARSED_DATA, button_capability_count: u16) -> anyhow::Result<usize> {
    if button_capability_count == 0 {
        return Ok(0);
    }
    let mut button_capabilities = vec![HIDP_BUTTON_CAPS::default(); usize::from(button_capability_count)];
    let mut length = button_capability_count;
    // SAFETY: the buffer holds as many entries as HidP_GetCaps reported.
    let status = unsafe {
        HidP_GetButtonCaps(
            HidP_Input,
            button_capabilities.as_mut_ptr(),
            &mut length,
            preparsed_data,
        )
    };
    anyhow::ensure!(status == HIDP_STATUS_SUCCESS, "could not read the button layout");
    let highest_button = button_capabilities[..usize::from(length)]
        .iter()
        .filter(|capability| capability.UsagePage == BUTTON_PAGE)
        .map(|capability| {
            // SAFETY: the union member is chosen by IsRange.
            unsafe {
                if capability.IsRange {
                    capability.Anonymous.Range.UsageMax
                } else {
                    capability.Anonymous.NotRange.Usage
                }
            }
        })
        .max()
        .unwrap_or(0);
    Ok(usize::from(highest_button))
}

fn single_usage(capability: &HIDP_VALUE_CAPS) -> u16 {
    // SAFETY: the union member is chosen by IsRange.
    unsafe {
        if capability.IsRange {
            capability.Anonymous.Range.UsageMin
        } else {
            capability.Anonymous.NotRange.Usage
        }
    }
}

fn is_axis_usage(usage_page: u16, usage: u16) -> bool {
    match usage_page {
        GENERIC_DESKTOP_PAGE => (0x30..=0x38).contains(&usage),
        SIMULATION_CONTROLS_PAGE => true,
        _ => false,
    }
}

/// Axes sorted by usage (X, Y, Z, Rx, Ry, Rz, Slider…), the order games and Windows use.
fn collect_axis_layouts(value_capabilities: &[HIDP_VALUE_CAPS]) -> Vec<AxisLayout> {
    let mut axes: Vec<AxisLayout> = value_capabilities
        .iter()
        .filter(|capability| is_axis_usage(capability.UsagePage, single_usage(capability)))
        .map(|capability| AxisLayout {
            usage_page: capability.UsagePage,
            usage: single_usage(capability),
            link_collection: capability.LinkCollection,
            logical_minimum: capability.LogicalMin,
            logical_maximum: capability.LogicalMax,
            bit_size: capability.BitSize,
        })
        .collect();
    axes.sort_by_key(|axis| (axis.usage_page, axis.usage));
    axes
}

fn collect_hat_layouts(value_capabilities: &[HIDP_VALUE_CAPS]) -> Vec<HatLayout> {
    value_capabilities
        .iter()
        .filter(|capability| {
            capability.UsagePage == GENERIC_DESKTOP_PAGE && single_usage(capability) == HAT_SWITCH_USAGE
        })
        .map(|capability| HatLayout {
            usage_page: capability.UsagePage,
            link_collection: capability.LinkCollection,
            logical_minimum: capability.LogicalMin,
            logical_maximum: capability.LogicalMax,
        })
        .collect()
}

/// Values with a negative logical minimum arrive as raw bit fields and must be sign extended.
fn sign_extend_if_needed(raw_value: u32, bit_size: u16, logical_minimum: i32) -> i32 {
    if logical_minimum >= 0 || bit_size == 0 || bit_size >= 32 {
        return raw_value as i32;
    }
    let unused_bits = 32 - u32::from(bit_size);
    ((raw_value << unused_bits) as i32) >> unused_bits
}

#[cfg(test)]
mod tests {
    use super::sign_extend_if_needed;

    #[test]
    fn negative_twelve_bit_value_is_sign_extended() {
        assert_eq!(sign_extend_if_needed(0xFFF, 12, -2048), -1);
    }

    #[test]
    fn unsigned_range_is_left_alone() {
        assert_eq!(sign_extend_if_needed(0xFFF, 12, 0), 4095);
    }
}
