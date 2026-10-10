//! Turns HOTAS input into keyboard, mouse and virtual controller output.
//!
//! The engine wakes the moment a device report arrives (no polling), works out which outputs
//! should be held, and sends only the changes in one batch. A short timer tick exists only to keep
//! analog mouse movement and wheel repeats flowing while nothing new is reported.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::app_state::SharedAppState;
use crate::automation::apply_current_lighting;
use crate::bindings::{
    AxisDirection, ControllerAxis, InputBinding, InputSource, MouseButtonKind, OutputAction, ScreenAxis,
    VirtualControllerKind, WheelDirection,
};
use crate::calibration::CalibrationStore;
use crate::input_injection::InputBatch;
use crate::input_reader::{ControllerSnapshot, ControllerUpdate, InputReaderService};
use crate::known_devices::DeviceKind;
use crate::layers::{LayerSetup, MODE_COUNT, ModeSwitchButtons, selected_mode};
use crate::lighting::LightingService;
use crate::press_styles::{PressStyle, PressTracker};
use crate::profiles::AxisTuning;
use crate::thread_priority::{raise_current_thread_priority, request_one_millisecond_timer_resolution};
use crate::virtual_gamepad::{GamepadState, VirtualGamepad};

const MOVEMENT_TICK: Duration = Duration::from_millis(2);
const CONFIGURATION_REFRESH_INTERVAL: Duration = Duration::from_millis(250);
const VIRTUAL_CONTROLLER_RETRY_INTERVAL: Duration = Duration::from_secs(5);
const WHEEL_REPEAT_INTERVAL: Duration = Duration::from_millis(100);
const MOUSE_DEADZONE: f64 = 0.04;
/// One pulse cycle of a pulsed key. Short enough to feel smooth, long enough for games to see each press.
const PULSE_PERIOD: Duration = Duration::from_millis(60);
const PULSE_DEADZONE: f64 = 0.08;
const PULSE_FULL_PRESS: f64 = 0.92;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum VirtualControllerStatus {
    #[default]
    NotUsed,
    Connected(VirtualControllerKind),
    DriverMissing,
    Failed(String),
}

#[derive(Clone)]
pub struct MappingEngine {
    virtual_controller_status: Arc<Mutex<VirtualControllerStatus>>,
}

impl MappingEngine {
    pub fn start(app_state: SharedAppState, input_reader: &InputReaderService, lighting: LightingService) -> Self {
        let engine = Self {
            virtual_controller_status: Arc::new(Mutex::new(VirtualControllerStatus::NotUsed)),
        };
        let updates = input_reader.subscribe_to_updates();
        let status = Arc::clone(&engine.virtual_controller_status);
        std::thread::Builder::new()
            .name("key-mapping".into())
            .spawn(move || MappingWorker::new(app_state, status, lighting).run(updates))
            .expect("key mapping thread starts");
        engine
    }

    pub fn virtual_controller_status(&self) -> VirtualControllerStatus {
        self.virtual_controller_status.lock().unwrap().clone()
    }
}

/// The settings the engine follows, re-read a few times per second.
#[derive(Default)]
struct MappingConfiguration {
    is_enabled: bool,
    bindings: Vec<InputBinding>,
    virtual_controller: VirtualControllerKind,
    layer_setup: LayerSetup,
    axis_tunings: Vec<AxisTuning>,
    calibration: CalibrationStore,
    mode_switch_buttons: ModeSwitchButtons,
    profiles_by_mode: [Option<String>; MODE_COUNT],
}

/// Everything the bindings ask for right now.
#[derive(Default)]
struct DesiredOutputs {
    keys: HashSet<u16>,
    mouse_buttons: HashSet<MouseButtonKind>,
    wheel_bindings: Vec<(usize, WheelDirection)>,
    mouse_velocity: (f64, f64),
    gamepad: GamepadState,
}

struct MappingWorker {
    app_state: SharedAppState,
    lighting: LightingService,
    /// The profile the mode switch last asked for, so a change is acted on exactly once.
    applied_mode_profile: Option<String>,
    virtual_controller_status: Arc<Mutex<VirtualControllerStatus>>,
    configuration: MappingConfiguration,
    last_configuration_refresh: Instant,
    snapshots: HashMap<DeviceKind, ControllerSnapshot>,
    /// Recent presses of each binding, by its position in the binding list.
    press_trackers: HashMap<usize, PressTracker>,
    held_keys: HashSet<u16>,
    held_mouse_buttons: HashSet<MouseButtonKind>,
    last_wheel_notch: HashMap<usize, Instant>,
    mouse_movement_remainder: (f64, f64),
    last_iteration: Instant,
    started_at: Instant,
    virtual_gamepad: Option<VirtualGamepad>,
    last_sent_gamepad_state: Option<GamepadState>,
    last_virtual_controller_attempt: Option<Instant>,
}

impl MappingWorker {
    fn new(
        app_state: SharedAppState,
        virtual_controller_status: Arc<Mutex<VirtualControllerStatus>>,
        lighting: LightingService,
    ) -> Self {
        Self {
            app_state,
            lighting,
            applied_mode_profile: None,
            virtual_controller_status,
            configuration: MappingConfiguration::default(),
            last_configuration_refresh: Instant::now() - CONFIGURATION_REFRESH_INTERVAL,
            snapshots: HashMap::new(),
            press_trackers: HashMap::new(),
            held_keys: HashSet::new(),
            held_mouse_buttons: HashSet::new(),
            last_wheel_notch: HashMap::new(),
            mouse_movement_remainder: (0.0, 0.0),
            last_iteration: Instant::now(),
            started_at: Instant::now(),
            virtual_gamepad: None,
            last_sent_gamepad_state: None,
            last_virtual_controller_attempt: None,
        }
    }

    fn run(mut self, updates: Receiver<ControllerUpdate>) {
        raise_current_thread_priority();
        request_one_millisecond_timer_resolution();
        loop {
            match updates.recv_timeout(MOVEMENT_TICK) {
                Ok(update) => self.take_update_and_any_queued(update, &updates),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            self.refresh_configuration_when_due();
            self.follow_mode_switch();
            self.keep_virtual_controller_matching_configuration();
            self.apply_bindings();
        }
        self.release_everything();
    }

    /// Several reports may have queued up; only the newest state of each device matters.
    fn take_update_and_any_queued(&mut self, first_update: ControllerUpdate, updates: &Receiver<ControllerUpdate>) {
        self.snapshots.insert(first_update.device_kind, first_update.snapshot);
        while let Ok(update) = updates.try_recv() {
            self.snapshots.insert(update.device_kind, update.snapshot);
        }
    }

    fn refresh_configuration_when_due(&mut self) {
        if self.last_configuration_refresh.elapsed() < CONFIGURATION_REFRESH_INTERVAL {
            return;
        }
        let state = self.app_state.lock().unwrap();
        let profile = state.effective_profile();
        let previous_bindings = std::mem::take(&mut self.configuration.bindings);
        self.configuration = MappingConfiguration {
            is_enabled: state.settings.mapping_enabled,
            bindings: profile
                .bindings
                .into_iter()
                .filter(InputBinding::is_consistent)
                .collect(),
            virtual_controller: profile.virtual_controller,
            layer_setup: profile.layer_setup,
            axis_tunings: profile.axis_tunings,
            calibration: state.calibration.clone(),
            mode_switch_buttons: state.settings.mode_switch_buttons,
            profiles_by_mode: state.settings.profiles_by_mode.clone(),
        };
        if self.configuration.bindings != previous_bindings {
            self.press_trackers.clear();
        }
        self.last_configuration_refresh = Instant::now();
    }

    /// Switches to the profile set for the mode switch position, the moment the switch is flipped.
    fn follow_mode_switch(&mut self) {
        let selected_mode = selected_mode(&self.configuration.mode_switch_buttons, &self.snapshots);
        let wanted_profile =
            selected_mode.and_then(|mode| self.configuration.profiles_by_mode[usize::from(mode) - 1].clone());
        if wanted_profile == self.applied_mode_profile {
            return;
        }
        self.app_state.lock().unwrap().mode_profile_name = wanted_profile.clone();
        self.applied_mode_profile = wanted_profile;
        apply_current_lighting(&self.app_state, &self.lighting);
        self.last_configuration_refresh = Instant::now() - CONFIGURATION_REFRESH_INTERVAL;
        self.refresh_configuration_when_due();
    }

    fn wanted_virtual_controller(&self) -> VirtualControllerKind {
        if self.configuration.is_enabled {
            self.configuration.virtual_controller
        } else {
            VirtualControllerKind::None
        }
    }

    fn keep_virtual_controller_matching_configuration(&mut self) {
        let wanted_kind = self.wanted_virtual_controller();
        let current_kind = self.virtual_gamepad.as_ref().map(VirtualGamepad::kind);
        if current_kind == Some(wanted_kind) {
            return;
        }
        if current_kind.is_some() {
            self.virtual_gamepad = None;
            self.last_sent_gamepad_state = None;
            self.last_virtual_controller_attempt = None;
        }
        if wanted_kind == VirtualControllerKind::None {
            self.set_virtual_controller_status(VirtualControllerStatus::NotUsed);
            return;
        }
        let attempted_recently = self
            .last_virtual_controller_attempt
            .is_some_and(|attempt| attempt.elapsed() < VIRTUAL_CONTROLLER_RETRY_INTERVAL);
        if !attempted_recently {
            self.connect_virtual_controller(wanted_kind);
        }
    }

    fn connect_virtual_controller(&mut self, kind: VirtualControllerKind) {
        self.last_virtual_controller_attempt = Some(Instant::now());
        match VirtualGamepad::connect(kind) {
            Ok(gamepad) => {
                self.virtual_gamepad = Some(gamepad);
                self.set_virtual_controller_status(VirtualControllerStatus::Connected(kind));
            }
            Err(message) if message.contains("not installed") => {
                self.set_virtual_controller_status(VirtualControllerStatus::DriverMissing);
            }
            Err(message) => self.set_virtual_controller_status(VirtualControllerStatus::Failed(message)),
        }
    }

    fn set_virtual_controller_status(&self, status: VirtualControllerStatus) {
        *self.virtual_controller_status.lock().unwrap() = status;
    }

    fn apply_bindings(&mut self) {
        let seconds_since_last_iteration = self.last_iteration.elapsed().as_secs_f64();
        self.last_iteration = Instant::now();
        let desired = if self.configuration.is_enabled {
            let pulse_phase = pulse_phase(self.started_at.elapsed());
            collect_desired_outputs(
                &self.configuration,
                &self.snapshots,
                &mut self.press_trackers,
                pulse_phase,
            )
        } else {
            DesiredOutputs::default()
        };

        let mut batch = InputBatch::default();
        self.update_held_keys(&desired.keys, &mut batch);
        self.update_held_mouse_buttons(&desired.mouse_buttons, &mut batch);
        self.update_wheel_repeats(&desired.wheel_bindings, &mut batch);
        self.add_mouse_movement(desired.mouse_velocity, seconds_since_last_iteration, &mut batch);
        batch.send();
        self.send_gamepad_state_if_changed(desired.gamepad);
    }

    fn update_held_keys(&mut self, wanted_keys: &HashSet<u16>, batch: &mut InputBatch) {
        for released_key in self.held_keys.difference(wanted_keys) {
            batch.add_key(*released_key, false);
        }
        for pressed_key in wanted_keys.difference(&self.held_keys) {
            batch.add_key(*pressed_key, true);
        }
        self.held_keys = wanted_keys.clone();
    }

    fn update_held_mouse_buttons(&mut self, wanted_buttons: &HashSet<MouseButtonKind>, batch: &mut InputBatch) {
        for released_button in self.held_mouse_buttons.difference(wanted_buttons) {
            batch.add_mouse_button(*released_button, false);
        }
        for pressed_button in wanted_buttons.difference(&self.held_mouse_buttons) {
            batch.add_mouse_button(*pressed_button, true);
        }
        self.held_mouse_buttons = wanted_buttons.clone();
    }

    /// One notch when the control is first used, then one every 100 ms while it stays held.
    fn update_wheel_repeats(&mut self, active_wheel_bindings: &[(usize, WheelDirection)], batch: &mut InputBatch) {
        let active_indexes: HashSet<usize> = active_wheel_bindings.iter().map(|(index, _)| *index).collect();
        self.last_wheel_notch.retain(|index, _| active_indexes.contains(index));
        for (binding_index, direction) in active_wheel_bindings {
            let is_due = self
                .last_wheel_notch
                .get(binding_index)
                .is_none_or(|last_notch| last_notch.elapsed() >= WHEEL_REPEAT_INTERVAL);
            if is_due {
                batch.add_wheel_notch(*direction);
                self.last_wheel_notch.insert(*binding_index, Instant::now());
            }
        }
    }

    /// Moves the pointer by velocity × elapsed time, carrying fractions of a pixel to the next step.
    fn add_mouse_movement(&mut self, velocity: (f64, f64), elapsed_seconds: f64, batch: &mut InputBatch) {
        let horizontal = self.mouse_movement_remainder.0 + velocity.0 * elapsed_seconds;
        let vertical = self.mouse_movement_remainder.1 + velocity.1 * elapsed_seconds;
        let whole_horizontal = horizontal.trunc();
        let whole_vertical = vertical.trunc();
        self.mouse_movement_remainder = (horizontal - whole_horizontal, vertical - whole_vertical);
        if velocity == (0.0, 0.0) {
            self.mouse_movement_remainder = (0.0, 0.0);
        }
        batch.add_mouse_movement(whole_horizontal as i32, whole_vertical as i32);
    }

    fn send_gamepad_state_if_changed(&mut self, gamepad_state: GamepadState) {
        let Some(virtual_gamepad) = self.virtual_gamepad.as_mut() else {
            return;
        };
        if self.last_sent_gamepad_state.as_ref() == Some(&gamepad_state) {
            return;
        }
        if let Err(message) = virtual_gamepad.send_state(&gamepad_state) {
            self.virtual_gamepad = None;
            self.set_virtual_controller_status(VirtualControllerStatus::Failed(message));
            return;
        }
        self.last_sent_gamepad_state = Some(gamepad_state);
    }

    fn release_everything(&mut self) {
        let mut batch = InputBatch::default();
        self.update_held_keys(&HashSet::new(), &mut batch);
        self.update_held_mouse_buttons(&HashSet::new(), &mut batch);
        batch.send();
        self.virtual_gamepad = None;
    }
}

fn collect_desired_outputs(
    configuration: &MappingConfiguration,
    snapshots: &HashMap<DeviceKind, ControllerSnapshot>,
    press_trackers: &mut HashMap<usize, PressTracker>,
    pulse_phase: f64,
) -> DesiredOutputs {
    let mut desired = DesiredOutputs::default();
    let now = Instant::now();
    let active_layer = configuration
        .layer_setup
        .active_layer(&configuration.mode_switch_buttons, snapshots);
    for (binding_index, binding) in configuration.bindings.iter().enumerate() {
        let Some(snapshot) = snapshots.get(&binding.source.device()) else {
            continue;
        };
        let is_in_layer = binding.enabled && binding.layer.matches(active_layer);
        if binding.source.is_analog() {
            if !is_in_layer {
                continue;
            }
            if let Some(position) = tuned_axis_position(binding.source, snapshot, configuration) {
                add_analog_output(&mut desired, binding.action, position, pulse_phase);
            }
            continue;
        }
        let is_pressed = is_in_layer && binding.source.is_active(snapshot);
        let waits_for_double_tap = has_double_tap_sibling(&configuration.bindings, binding_index);
        let output_is_on = press_trackers.entry(binding_index).or_default().update(
            binding.press_style,
            is_pressed,
            now,
            waits_for_double_tap,
        );
        if output_is_on {
            add_switch_output(&mut desired, binding_index, binding.action);
        }
    }
    desired
}

/// A single tap must wait briefly when the same control can also be double tapped,
/// otherwise every double tap would fire the single-tap action first.
fn has_double_tap_sibling(bindings: &[InputBinding], binding_index: usize) -> bool {
    let binding = &bindings[binding_index];
    binding.press_style == PressStyle::Tap
        && bindings.iter().enumerate().any(|(other_index, other)| {
            other_index != binding_index
                && other.enabled
                && other.press_style == PressStyle::DoubleTap
                && other.source == binding.source
                && other.layer == binding.layer
        })
}

fn add_analog_output(
    desired: &mut DesiredOutputs,
    action: OutputAction,
    position: TunedAxisPosition,
    pulse_phase: f64,
) {
    let axis_value = position.value;
    match action {
        OutputAction::PulsedKey { virtual_key, direction } => {
            let push = push_toward(position, direction);
            if is_pulse_held(push, pulse_phase) {
                desired.keys.insert(virtual_key);
            }
        }
        OutputAction::MouseMove {
            screen_axis,
            pixels_per_second,
        } => {
            let velocity = deflection_outside_deadzone(axis_value) * f64::from(pixels_per_second);
            match screen_axis {
                ScreenAxis::Horizontal => desired.mouse_velocity.0 += velocity,
                ScreenAxis::Vertical => desired.mouse_velocity.1 += velocity,
            }
        }
        OutputAction::ControllerAxis { axis, inverted } => {
            desired
                .gamepad
                .set_axis(axis, controller_axis_value(axis, axis_value, inverted));
        }
        _ => {}
    }
}

fn add_switch_output(desired: &mut DesiredOutputs, binding_index: usize, action: OutputAction) {
    match action {
        OutputAction::Key { virtual_key } => {
            desired.keys.insert(virtual_key);
        }
        OutputAction::MouseButton { button } => {
            desired.mouse_buttons.insert(button);
        }
        OutputAction::MouseWheel { direction } => desired.wheel_bindings.push((binding_index, direction)),
        OutputAction::ControllerButton { button } => {
            desired.gamepad.pressed_buttons.insert(button);
        }
        _ => {}
    }
}

/// An axis reading after the profile's deadzone and curve, plus where the axis rests.
#[derive(Clone, Copy, Debug)]
struct TunedAxisPosition {
    value: f64,
    rests_at_center: bool,
}

fn tuned_axis_position(
    source: InputSource,
    snapshot: &ControllerSnapshot,
    configuration: &MappingConfiguration,
) -> Option<TunedAxisPosition> {
    let InputSource::Axis { device, axis_index } = source else {
        return None;
    };
    let raw_value = snapshot.axis_value(axis_index)?;
    let drift_corrected_value = configuration.calibration.correct(device, axis_index, raw_value);
    let rests_at_center = device.definition().axis_rests_at_center(axis_index);
    let tuning = configuration
        .axis_tunings
        .iter()
        .find(|tuning| tuning.device_kind == device && tuning.axis_index == axis_index);
    let value = tuning.map_or(drift_corrected_value, |tuning| {
        tuning.apply_to_reading(drift_corrected_value, rests_at_center)
    });
    Some(TunedAxisPosition { value, rests_at_center })
}

/// How far (0..=1) the axis is pushed in one direction: from the centre for sticks, from the end for throttles.
fn push_toward(position: TunedAxisPosition, direction: AxisDirection) -> f64 {
    let push = match (position.rests_at_center, direction) {
        (true, AxisDirection::Increasing) => (position.value - 0.5) * 2.0,
        (true, AxisDirection::Decreasing) => (0.5 - position.value) * 2.0,
        (false, AxisDirection::Increasing) => position.value,
        (false, AxisDirection::Decreasing) => 1.0 - position.value,
    };
    push.clamp(0.0, 1.0)
}

/// Where we are inside the current pulse cycle, 0..1.
fn pulse_phase(time_since_start: Duration) -> f64 {
    let cycle_position = time_since_start.as_secs_f64() % PULSE_PERIOD.as_secs_f64();
    cycle_position / PULSE_PERIOD.as_secs_f64()
}

/// A pulsed key is held for the first `push` share of every cycle; tiny pushes do nothing, near-full pushes hold it.
fn is_pulse_held(push: f64, pulse_phase: f64) -> bool {
    if push <= PULSE_DEADZONE {
        return false;
    }
    push >= PULSE_FULL_PRESS || pulse_phase < push
}

/// Position of a centred axis as -1..=1, with a small deadzone so a resting stick does not drift the pointer.
fn deflection_outside_deadzone(axis_value: f64) -> f64 {
    let deflection = axis_value * 2.0 - 1.0;
    if deflection.abs() <= MOUSE_DEADZONE {
        return 0.0;
    }
    deflection.signum() * (deflection.abs() - MOUSE_DEADZONE) / (1.0 - MOUSE_DEADZONE)
}

/// Converts a HOTAS axis (0..=1, down/forward-is-larger for Y) to a controller axis value.
fn controller_axis_value(axis: ControllerAxis, axis_value: f64, inverted: bool) -> f32 {
    let is_trigger = matches!(axis, ControllerAxis::LeftTrigger | ControllerAxis::RightTrigger);
    if is_trigger {
        let trigger = if inverted { 1.0 - axis_value } else { axis_value };
        return trigger as f32;
    }
    let is_vertical = matches!(axis, ControllerAxis::LeftStickY | ControllerAxis::RightStickY);
    let mut stick = axis_value * 2.0 - 1.0;
    if is_vertical {
        stick = -stick;
    }
    if inverted {
        stick = -stick;
    }
    stick as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stick_pushed_forward_moves_controller_stick_up() {
        assert_eq!(controller_axis_value(ControllerAxis::LeftStickY, 0.0, false), 1.0);
    }

    #[test]
    fn resting_axis_inside_deadzone_does_not_move_the_mouse() {
        assert_eq!(deflection_outside_deadzone(0.51), 0.0);
    }

    #[test]
    fn stick_pushed_a_third_forward_holds_the_key_a_third_of_the_time() {
        let position = TunedAxisPosition {
            value: 0.5 - 0.5 / 3.0,
            rests_at_center: true,
        };
        let push = push_toward(position, AxisDirection::Decreasing);
        assert!(is_pulse_held(push, 0.2));
        assert!(!is_pulse_held(push, 0.5));
    }

    #[test]
    fn small_push_never_presses_the_key() {
        assert!(!is_pulse_held(0.05, 0.0));
    }

    #[test]
    fn inverted_trigger_reads_full_at_zero() {
        assert_eq!(controller_axis_value(ControllerAxis::RightTrigger, 0.0, true), 1.0);
    }
}
