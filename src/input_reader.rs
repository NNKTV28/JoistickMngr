//! Reads live axes, buttons and hats from the peripherals through Windows.Gaming.Input.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Gaming::Input::{GameControllerSwitchPosition, RawGameController};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

use crate::known_devices::{DeviceKind, find_known_device};

const ACTIVE_POLL_INTERVAL: Duration = Duration::from_millis(10);
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(250);
const CONTROLLER_LIST_REFRESH_INTERVAL: Duration = Duration::from_secs(2);
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
    fn from_switch_position(position: GameControllerSwitchPosition) -> Self {
        match position {
            GameControllerSwitchPosition::Up => HatDirection::Up,
            GameControllerSwitchPosition::UpRight => HatDirection::UpRight,
            GameControllerSwitchPosition::Right => HatDirection::Right,
            GameControllerSwitchPosition::DownRight => HatDirection::DownRight,
            GameControllerSwitchPosition::Down => HatDirection::Down,
            GameControllerSwitchPosition::DownLeft => HatDirection::DownLeft,
            GameControllerSwitchPosition::Left => HatDirection::Left,
            GameControllerSwitchPosition::UpLeft => HatDirection::UpLeft,
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

#[derive(Clone, Debug, Default)]
pub struct ControllerSnapshot {
    pub axis_values: Vec<f64>,
    pub button_states: Vec<bool>,
    pub hat_directions: Vec<HatDirection>,
    pub axis_statistics: Vec<AxisStatistics>,
}

impl ControllerSnapshot {
    /// Some devices (such as the rudder pedals) send nothing until a control moves; until then every value reads 0.
    pub fn has_reported_input(&self) -> bool {
        let any_axis_away_from_zero = self.axis_values.iter().any(|value| *value != 0.0);
        let any_button_pressed = self.button_states.iter().any(|is_pressed| *is_pressed);
        any_axis_away_from_zero || any_button_pressed
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
    button_press_log: VecDeque<ButtonPressEvent>,
}

#[derive(Clone)]
pub struct InputReaderService {
    shared_state: Arc<Mutex<SharedInputState>>,
    polling_enabled: Arc<AtomicBool>,
    reset_requested: Arc<AtomicBool>,
}

impl InputReaderService {
    pub fn start() -> Self {
        let service = Self {
            shared_state: Arc::new(Mutex::new(SharedInputState::default())),
            polling_enabled: Arc::new(AtomicBool::new(true)),
            reset_requested: Arc::new(AtomicBool::new(false)),
        };
        let thread_service = service.clone();
        std::thread::Builder::new()
            .name("input-reader".into())
            .spawn(move || thread_service.run_polling_loop())
            .expect("input reader thread starts");
        service
    }

    pub fn snapshot(&self, device_kind: DeviceKind) -> Option<ControllerSnapshot> {
        self.shared_state.lock().unwrap().snapshots.get(&device_kind).cloned()
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

    /// Fast polling is only needed while a page shows live input.
    pub fn set_polling_enabled(&self, enabled: bool) {
        self.polling_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn reset_statistics(&self) {
        self.reset_requested.store(true, Ordering::Relaxed);
    }

    fn run_polling_loop(&self) {
        // SAFETY: COM/WinRT is initialised once for this dedicated thread.
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let mut controllers: HashMap<DeviceKind, RawGameController> = HashMap::new();
        let mut last_controller_refresh: Option<Instant> = None;

        loop {
            if last_controller_refresh.is_none_or(|refreshed| refreshed.elapsed() >= CONTROLLER_LIST_REFRESH_INTERVAL) {
                controllers = find_known_controllers();
                last_controller_refresh = Some(Instant::now());
            }
            if self.reset_requested.swap(false, Ordering::Relaxed) {
                self.clear_statistics_and_log();
            }
            self.read_all_controllers(&controllers);

            let poll_interval = if self.polling_enabled.load(Ordering::Relaxed) {
                ACTIVE_POLL_INTERVAL
            } else {
                IDLE_POLL_INTERVAL
            };
            std::thread::sleep(poll_interval);
        }
    }

    fn clear_statistics_and_log(&self) {
        let mut shared_state = self.shared_state.lock().unwrap();
        shared_state.button_press_log.clear();
        for snapshot in shared_state.snapshots.values_mut() {
            snapshot
                .axis_statistics
                .iter_mut()
                .for_each(|statistics| *statistics = AxisStatistics::default());
        }
    }

    fn read_all_controllers(&self, controllers: &HashMap<DeviceKind, RawGameController>) {
        let mut shared_state = self.shared_state.lock().unwrap();
        shared_state
            .snapshots
            .retain(|device_kind, _| controllers.contains_key(device_kind));
        for (device_kind, controller) in controllers {
            let Ok(reading) = read_controller(controller) else {
                continue;
            };
            let previous_snapshot = shared_state.snapshots.remove(device_kind).unwrap_or_default();
            let new_presses = find_new_button_presses(&previous_snapshot.button_states, &reading.button_states);
            let updated_snapshot = merge_reading_into_snapshot(previous_snapshot, reading);
            shared_state.snapshots.insert(*device_kind, updated_snapshot);
            for button_number in new_presses {
                log_button_press(&mut shared_state.button_press_log, *device_kind, button_number);
            }
        }
    }
}

struct ControllerReading {
    axis_values: Vec<f64>,
    button_states: Vec<bool>,
    hat_directions: Vec<HatDirection>,
}

fn find_known_controllers() -> HashMap<DeviceKind, RawGameController> {
    let mut controllers_by_kind: HashMap<DeviceKind, RawGameController> = HashMap::new();
    let Ok(all_controllers) = RawGameController::RawGameControllers() else {
        return controllers_by_kind;
    };
    for controller in &all_controllers {
        let (Ok(vendor_id), Ok(product_id)) = (controller.HardwareVendorId(), controller.HardwareProductId()) else {
            continue;
        };
        let Some(definition) = find_known_device(vendor_id, product_id) else {
            continue;
        };
        let has_more_axes_than_existing = controllers_by_kind
            .get(&definition.kind)
            .is_none_or(|existing| controller.AxisCount().unwrap_or(0) > existing.AxisCount().unwrap_or(0));
        if has_more_axes_than_existing {
            controllers_by_kind.insert(definition.kind, controller);
        }
    }
    controllers_by_kind
}

fn read_controller(controller: &RawGameController) -> windows::core::Result<ControllerReading> {
    let mut button_states = vec![false; controller.ButtonCount()? as usize];
    let mut switch_positions = vec![GameControllerSwitchPosition::Center; controller.SwitchCount()? as usize];
    let mut axis_values = vec![0.0; controller.AxisCount()? as usize];
    controller.GetCurrentReading(&mut button_states, &mut switch_positions, &mut axis_values)?;
    Ok(ControllerReading {
        axis_values,
        button_states,
        hat_directions: switch_positions
            .into_iter()
            .map(HatDirection::from_switch_position)
            .collect(),
    })
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

fn merge_reading_into_snapshot(mut snapshot: ControllerSnapshot, reading: ControllerReading) -> ControllerSnapshot {
    snapshot
        .axis_statistics
        .resize(reading.axis_values.len(), AxisStatistics::default());
    for (statistics, value) in snapshot.axis_statistics.iter_mut().zip(&reading.axis_values) {
        statistics.record(*value);
    }
    snapshot.axis_values = reading.axis_values;
    snapshot.button_states = reading.button_states;
    snapshot.hat_directions = reading.hat_directions;
    snapshot
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
