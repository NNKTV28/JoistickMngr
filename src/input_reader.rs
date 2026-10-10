//! Reads live axes, buttons and hats from the peripherals through their HID interfaces.
//! Reading continues while games have focus, so key mapping can use the same data.

use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::CString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::hid_controller::{
    AxisLayout, GAMEPAD_USAGE, GENERIC_DESKTOP_PAGE, HidController, JOYSTICK_USAGE, RawControllerState,
};
use crate::known_devices::{DeviceKind, KnownDevice, find_known_device};
use crate::thread_priority::raise_current_thread_priority;

const DEVICE_DISCOVERY_INTERVAL: Duration = Duration::from_secs(2);
/// The X56 throttle reports its M1 / M2 / M3 switch on a separate vendor-defined interface.
const VENDOR_DEFINED_PAGE: u16 = 0xFF00;
/// Start of the throttle's mode report; the byte after it is the mode, 1..=3.
const MODE_REPORT_PREFIX: [u8; 4] = [0xFF, 0x09, 0x01, 0x01];
const NOISE_WINDOW_SAMPLE_COUNT: usize = 200;
const BUTTON_EVENT_LOG_LIMIT: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HatDirection {
    Centered,
    Up,
    UpRight,
    Right,
    DownRight,
    Down,
    DownLeft,
    Left,
    UpLeft,
}

impl HatDirection {
    /// Converts a hat position in eighths clockwise from up (0 = up, 2 = right…), or `None` for centred.
    fn from_hat_position(position: Option<u8>) -> Self {
        match position {
            Some(0) => HatDirection::Up,
            Some(1) => HatDirection::UpRight,
            Some(2) => HatDirection::Right,
            Some(3) => HatDirection::DownRight,
            Some(4) => HatDirection::Down,
            Some(5) => HatDirection::DownLeft,
            Some(6) => HatDirection::Left,
            Some(7) => HatDirection::UpLeft,
            _ => HatDirection::Centered,
        }
    }

    /// Unit offset of the direction, with y pointing down like screen coordinates.
    pub fn offset(self) -> (f32, f32) {
        let diagonal = std::f32::consts::FRAC_1_SQRT_2;
        match self {
            HatDirection::Centered => (0.0, 0.0),
            HatDirection::Up => (0.0, -1.0),
            HatDirection::UpRight => (diagonal, -diagonal),
            HatDirection::Right => (1.0, 0.0),
            HatDirection::DownRight => (diagonal, diagonal),
            HatDirection::Down => (0.0, 1.0),
            HatDirection::DownLeft => (-diagonal, diagonal),
            HatDirection::Left => (-1.0, 0.0),
            HatDirection::UpLeft => (-diagonal, -diagonal),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AxisStatistics {
    recent_values: VecDeque<f64>,
    pub recorded_minimum: f64,
    pub recorded_maximum: f64,
}

impl Default for AxisStatistics {
    fn default() -> Self {
        Self {
            recent_values: VecDeque::with_capacity(NOISE_WINDOW_SAMPLE_COUNT),
            recorded_minimum: f64::MAX,
            recorded_maximum: f64::MIN,
        }
    }
}

impl AxisStatistics {
    fn record(&mut self, value: f64) {
        if self.recent_values.len() == NOISE_WINDOW_SAMPLE_COUNT {
            self.recent_values.pop_front();
        }
        self.recent_values.push_back(value);
        self.recorded_minimum = self.recorded_minimum.min(value);
        self.recorded_maximum = self.recorded_maximum.max(value);
    }

    /// Two standard deviations of the recent readings, as a percentage of the full axis range.
    /// While the axis is untouched this is the sensor jitter.
    pub fn noise_percent(&self) -> f64 {
        let sample_count = self.recent_values.len();
        if sample_count < 2 {
            return 0.0;
        }
        let mean = self.recent_values.iter().sum::<f64>() / sample_count as f64;
        let variance = self
            .recent_values
            .iter()
            .map(|value| (value - mean).powi(2))
            .sum::<f64>()
            / sample_count as f64;
        variance.sqrt() * 2.0 * 100.0
    }

    pub fn has_recorded_range(&self) -> bool {
        self.recorded_minimum <= self.recorded_maximum
    }
}

/// The latest state of every control on one device. Cheap to copy, so it can be sent with every report.
#[derive(Clone, Debug, Default)]
pub struct ControllerSnapshot {
    pub axis_values: Vec<f64>,
    pub button_states: Vec<bool>,
    pub hat_directions: Vec<HatDirection>,
    /// Position of a mode switch the device reports by itself (the X56 throttle), 1..=3.
    /// `None` until the switch has been moved, because the device only reports changes.
    pub reported_mode: Option<u8>,
}

/// Sent to subscribers the moment a device reports new input.
#[derive(Clone, Debug)]
pub struct ControllerUpdate {
    pub device_kind: DeviceKind,
    pub snapshot: ControllerSnapshot,
}

impl ControllerSnapshot {
    /// Some devices (such as the rudder pedals) send nothing until a control moves; until then every value reads 0.
    pub fn has_reported_input(&self) -> bool {
        let any_axis_away_from_zero = self.axis_values.iter().any(|value| *value != 0.0);
        let any_button_pressed = self.button_states.iter().any(|is_pressed| *is_pressed);
        any_axis_away_from_zero || any_button_pressed
    }

    pub fn axis_value(&self, axis_index: usize) -> Option<f64> {
        self.axis_values.get(axis_index).copied()
    }

    pub fn is_button_pressed(&self, button_number: usize) -> bool {
        button_number
            .checked_sub(1)
            .and_then(|index| self.button_states.get(index))
            .copied()
            .unwrap_or(false)
    }

    pub fn hat_direction(&self, hat_index: usize) -> HatDirection {
        self.hat_directions
            .get(hat_index)
            .copied()
            .unwrap_or(HatDirection::Centered)
    }
}

#[derive(Clone, Debug)]
pub struct ButtonPressEvent {
    pub device_kind: DeviceKind,
    pub button_number: usize,
    pub pressed_at: Instant,
}

#[derive(Default)]
struct SharedInputState {
    snapshots: HashMap<DeviceKind, ControllerSnapshot>,
    axis_statistics: HashMap<DeviceKind, Vec<AxisStatistics>>,
    button_press_log: VecDeque<ButtonPressEvent>,
    update_subscribers: Vec<Sender<ControllerUpdate>>,
    reported_modes: HashMap<DeviceKind, u8>,
}

#[derive(Clone)]
pub struct InputReaderService {
    shared_state: Arc<Mutex<SharedInputState>>,
    devices_being_read: Arc<Mutex<HashSet<DeviceKind>>>,
    mode_switches_being_read: Arc<Mutex<HashSet<DeviceKind>>>,
    reset_requested: Arc<AtomicBool>,
}

impl InputReaderService {
    pub fn start() -> Self {
        let service = Self {
            shared_state: Arc::new(Mutex::new(SharedInputState::default())),
            devices_being_read: Arc::new(Mutex::new(HashSet::new())),
            mode_switches_being_read: Arc::new(Mutex::new(HashSet::new())),
            reset_requested: Arc::new(AtomicBool::new(false)),
        };
        let thread_service = service.clone();
        std::thread::Builder::new()
            .name("input-discovery".into())
            .spawn(move || thread_service.run_discovery_loop())
            .expect("input discovery thread starts");
        service
    }

    pub fn snapshot(&self, device_kind: DeviceKind) -> Option<ControllerSnapshot> {
        self.shared_state.lock().unwrap().snapshots.get(&device_kind).cloned()
    }

    pub fn axis_statistics(&self, device_kind: DeviceKind) -> Vec<AxisStatistics> {
        let shared_state = self.shared_state.lock().unwrap();
        shared_state
            .axis_statistics
            .get(&device_kind)
            .cloned()
            .unwrap_or_default()
    }

    /// Every report from every device, delivered as soon as it is read.
    pub fn subscribe_to_updates(&self) -> Receiver<ControllerUpdate> {
        let (sender, receiver) = mpsc::channel();
        self.shared_state.lock().unwrap().update_subscribers.push(sender);
        receiver
    }

    pub fn button_press_log(&self) -> Vec<ButtonPressEvent> {
        self.shared_state
            .lock()
            .unwrap()
            .button_press_log
            .iter()
            .cloned()
            .collect()
    }

    pub fn reset_statistics(&self) {
        self.reset_requested.store(true, Ordering::Relaxed);
    }

    /// Looks for newly plugged-in controllers and starts one reading thread per controller.
    fn run_discovery_loop(&self) {
        let Ok(mut hid_api) = hidapi::HidApi::new() else { return };
        loop {
            if self.reset_requested.swap(false, Ordering::Relaxed) {
                self.clear_statistics_and_log();
            }
            let _ = hid_api.refresh_devices();
            for (definition, device_path) in find_controller_interfaces(&hid_api) {
                if self.devices_being_read.lock().unwrap().insert(definition.kind) {
                    self.spawn_reader(definition, device_path);
                }
            }
            for (definition, device_path) in find_mode_switch_interfaces(&hid_api) {
                if self.mode_switches_being_read.lock().unwrap().insert(definition.kind) {
                    self.spawn_mode_switch_reader(definition, device_path);
                }
            }
            std::thread::sleep(DEVICE_DISCOVERY_INTERVAL);
        }
    }

    fn spawn_reader(&self, definition: &'static KnownDevice, device_path: CString) {
        let service = self.clone();
        let spawn_result = std::thread::Builder::new()
            .name(format!("input-{}", definition.short_name))
            .spawn(move || {
                service.read_until_disconnected(definition, &device_path);
                service.forget_device(definition.kind);
            });
        if spawn_result.is_err() {
            self.devices_being_read.lock().unwrap().remove(&definition.kind);
        }
    }

    fn spawn_mode_switch_reader(&self, definition: &'static KnownDevice, device_path: CString) {
        let service = self.clone();
        let spawn_result = std::thread::Builder::new()
            .name(format!("mode-switch-{}", definition.short_name))
            .spawn(move || {
                service.read_mode_switch_until_disconnected(definition.kind, &device_path);
                service.forget_mode_switch(definition.kind);
            });
        if spawn_result.is_err() {
            self.mode_switches_being_read.lock().unwrap().remove(&definition.kind);
        }
    }

    fn read_mode_switch_until_disconnected(&self, device_kind: DeviceKind, device_path: &CString) {
        let Ok(interface) = HidController::open(device_path) else {
            return;
        };
        while let Ok(report) = interface.read_report() {
            if let Some(mode) = parse_mode_report(&report) {
                self.store_reported_mode(device_kind, mode);
            }
        }
    }

    /// Records the new mode and tells subscribers straight away, so a mode profile loads instantly.
    fn store_reported_mode(&self, device_kind: DeviceKind, mode: u8) {
        let mut shared_state = self.shared_state.lock().unwrap();
        shared_state.reported_modes.insert(device_kind, mode);
        let mut snapshot = shared_state.snapshots.get(&device_kind).cloned().unwrap_or_default();
        snapshot.reported_mode = Some(mode);
        notify_subscribers(&mut shared_state.update_subscribers, device_kind, &snapshot);
        shared_state.snapshots.insert(device_kind, snapshot);
    }

    fn forget_mode_switch(&self, device_kind: DeviceKind) {
        self.shared_state.lock().unwrap().reported_modes.remove(&device_kind);
        self.mode_switches_being_read.lock().unwrap().remove(&device_kind);
    }

    fn read_until_disconnected(&self, definition: &'static KnownDevice, device_path: &CString) {
        raise_current_thread_priority();
        let Ok(controller) = HidController::open(device_path) else {
            return;
        };
        let axis_order = order_axes_like_definition(controller.axis_layouts(), definition);
        while let Ok(raw_state) = controller.read_state() {
            self.store_reading(definition.kind, reorder_axes(raw_state, &axis_order));
        }
    }

    fn forget_device(&self, device_kind: DeviceKind) {
        let mut shared_state = self.shared_state.lock().unwrap();
        shared_state.snapshots.remove(&device_kind);
        shared_state.axis_statistics.remove(&device_kind);
        drop(shared_state);
        self.devices_being_read.lock().unwrap().remove(&device_kind);
    }

    /// Hands the new state to subscribers first (they drive key mapping), then updates the bookkeeping.
    fn store_reading(&self, device_kind: DeviceKind, raw_state: RawControllerState) {
        let mut snapshot = snapshot_from_raw_state(raw_state);
        let mut shared_state = self.shared_state.lock().unwrap();
        snapshot.reported_mode = shared_state.reported_modes.get(&device_kind).copied();
        notify_subscribers(&mut shared_state.update_subscribers, device_kind, &snapshot);

        let previous_buttons = shared_state
            .snapshots
            .get(&device_kind)
            .map(|previous| previous.button_states.clone());
        let new_presses = find_new_button_presses(&previous_buttons.unwrap_or_default(), &snapshot.button_states);
        for button_number in new_presses {
            log_button_press(&mut shared_state.button_press_log, device_kind, button_number);
        }
        let statistics = shared_state.axis_statistics.entry(device_kind).or_default();
        record_axis_statistics(statistics, &snapshot.axis_values);
        shared_state.snapshots.insert(device_kind, snapshot);
    }

    fn clear_statistics_and_log(&self) {
        let mut shared_state = self.shared_state.lock().unwrap();
        shared_state.button_press_log.clear();
        shared_state.axis_statistics.clear();
    }
}

/// The game controller interface of each known device that is plugged in.
fn find_controller_interfaces(hid_api: &hidapi::HidApi) -> Vec<(&'static KnownDevice, CString)> {
    hid_api
        .device_list()
        .filter(|device| {
            device.usage_page() == GENERIC_DESKTOP_PAGE
                && (device.usage() == JOYSTICK_USAGE || device.usage() == GAMEPAD_USAGE)
        })
        .filter_map(|device| {
            let definition = find_known_device(device.vendor_id(), device.product_id())?;
            Some((definition, device.path().to_owned()))
        })
        .collect()
}

/// The vendor-defined interface of each known device that reports a mode switch on it.
fn find_mode_switch_interfaces(hid_api: &hidapi::HidApi) -> Vec<(&'static KnownDevice, CString)> {
    hid_api
        .device_list()
        .filter(|device| device.usage_page() == VENDOR_DEFINED_PAGE)
        .filter_map(|device| {
            let definition = find_known_device(device.vendor_id(), device.product_id())?;
            (definition.kind == DeviceKind::Throttle).then(|| (definition, device.path().to_owned()))
        })
        .collect()
}

/// The mode in a report such as `FF 09 01 01 02 …` (mode 2), or `None` for any other report.
fn parse_mode_report(report: &[u8]) -> Option<u8> {
    let mode = *report.strip_prefix(&MODE_REPORT_PREFIX)?.first()?;
    (1..=3).contains(&mode).then_some(mode)
}

/// Positions of the device's axes in the order the device definition lists them, so axis numbers
/// in profiles always mean the same physical control. Axes the definition does not know come last.
fn order_axes_like_definition(axis_layouts: &[AxisLayout], definition: &KnownDevice) -> Vec<usize> {
    let mut unused_positions: Vec<usize> = (0..axis_layouts.len()).collect();
    let mut ordered_positions = Vec::with_capacity(axis_layouts.len());
    for defined_axis in definition.axes {
        let matching = unused_positions
            .iter()
            .position(|position| axis_layouts[*position].usage == defined_axis.hid_usage);
        if let Some(index_in_unused) = matching {
            ordered_positions.push(unused_positions.remove(index_in_unused));
        }
    }
    ordered_positions.extend(unused_positions);
    ordered_positions
}

fn reorder_axes(mut raw_state: RawControllerState, axis_order: &[usize]) -> RawControllerState {
    raw_state.axis_values = axis_order
        .iter()
        .filter_map(|position| raw_state.axis_values.get(*position).copied())
        .collect();
    raw_state
}

fn find_new_button_presses(previous_states: &[bool], current_states: &[bool]) -> Vec<usize> {
    if previous_states.len() != current_states.len() {
        return Vec::new();
    }
    current_states
        .iter()
        .zip(previous_states)
        .enumerate()
        .filter(|(_, (is_pressed_now, was_pressed))| **is_pressed_now && !**was_pressed)
        .map(|(button_index, _)| button_index + 1)
        .collect()
}

fn snapshot_from_raw_state(raw_state: RawControllerState) -> ControllerSnapshot {
    ControllerSnapshot {
        axis_values: raw_state.axis_values,
        button_states: raw_state.button_states,
        hat_directions: raw_state
            .hat_positions
            .into_iter()
            .map(HatDirection::from_hat_position)
            .collect(),
        reported_mode: None,
    }
}

fn record_axis_statistics(statistics: &mut Vec<AxisStatistics>, axis_values: &[f64]) {
    statistics.resize(axis_values.len(), AxisStatistics::default());
    for (axis_statistics, value) in statistics.iter_mut().zip(axis_values) {
        axis_statistics.record(*value);
    }
}

/// Sends the update to every subscriber and forgets the ones that have gone away.
fn notify_subscribers(
    subscribers: &mut Vec<Sender<ControllerUpdate>>,
    device_kind: DeviceKind,
    snapshot: &ControllerSnapshot,
) {
    subscribers.retain(|subscriber| {
        let update = ControllerUpdate {
            device_kind,
            snapshot: snapshot.clone(),
        };
        subscriber.send(update).is_ok()
    });
}

fn log_button_press(log: &mut VecDeque<ButtonPressEvent>, device_kind: DeviceKind, button_number: usize) {
    if log.len() == BUTTON_EVENT_LOG_LIMIT {
        log.pop_front();
    }
    log.push_back(ButtonPressEvent {
        device_kind,
        button_number,
        pressed_at: Instant::now(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_report_gives_the_switch_position() {
        assert_eq!(parse_mode_report(&[0xFF, 0x09, 0x01, 0x01, 0x02, 0x00]), Some(2));
    }

    #[test]
    fn other_vendor_reports_are_ignored() {
        assert_eq!(parse_mode_report(&[0xFF, 0x0A, 0x01, 0x01, 0x02]), None);
        assert_eq!(parse_mode_report(&[0xFF, 0x09, 0x01, 0x01, 0x07]), None);
        assert_eq!(parse_mode_report(&[0xFF, 0x09]), None);
    }
}
