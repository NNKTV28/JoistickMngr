//! Key mapping: bind HOTAS controls to keys, the mouse, or a virtual Xbox / PlayStation controller.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, ComboBox, RichText, Slider, Ui};

use crate::app::JoystickManagerApp;
use crate::bindings::{
    AxisDirection, ControllerAxis, ControllerButton, HatPush, InputBinding, InputSource, MouseButtonKind, OutputAction,
    OutputFamily, ScreenAxis, VirtualControllerKind, WheelDirection,
};
use crate::calibration::CalibrationStore;
use crate::input_reader::{ControllerSnapshot, HatDirection};
use crate::keyboard_keys::{all_keyboard_keys, find_key_for_egui_key, key_name};
use crate::known_devices::DeviceKind;
use crate::layers::{ActiveLayer, BindingLayer, ButtonReference, MODE_COUNT};
use crate::localization::{tr, tr_format};
use crate::mapping_engine::VirtualControllerStatus;
use crate::mapping_presets::{add_controller_starter_layout, add_mouse_look_layout, add_wasd_flight_layout};
use crate::press_styles::PressStyle;
use crate::profiles::Profile;
use crate::system_actions::open_in_default_handler;
use crate::theme::*;
use crate::ui::widgets::*;
use crate::virtual_gamepad::VIGEM_BUS_DOWNLOAD_URL;

const DETECTION_TIMEOUT: Duration = Duration::from_secs(10);
const AXIS_DETECTION_DISTANCE: f64 = 0.3;
const DEFAULT_MOUSE_SPEED: f32 = 1200.0;
const OUTPUT_TILE_MINIMUM_WIDTH: f32 = 300.0;
/// Deflection above which an analog binding's light turns on.
const ANALOG_ACTIVITY_THRESHOLD: f64 = 0.08;
/// When an existing axis switch is edited, its threshold is rebuilt from two points this far apart.
const THRESHOLD_EDIT_SPREAD: f64 = 0.2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActionKind {
    Key,
    PulsedKey,
    MouseButton,
    MouseWheel,
    MouseMove,
    ControllerButton,
    ControllerAxis,
}

impl ActionKind {
    const ALL: [ActionKind; 7] = [
        ActionKind::Key,
        ActionKind::PulsedKey,
        ActionKind::MouseButton,
        ActionKind::MouseWheel,
        ActionKind::MouseMove,
        ActionKind::ControllerButton,
        ActionKind::ControllerAxis,
    ];

    fn of_action(action: OutputAction) -> Self {
        match action {
            OutputAction::Key { .. } => ActionKind::Key,
            OutputAction::PulsedKey { .. } => ActionKind::PulsedKey,
            OutputAction::MouseButton { .. } => ActionKind::MouseButton,
            OutputAction::MouseWheel { .. } => ActionKind::MouseWheel,
            OutputAction::MouseMove { .. } => ActionKind::MouseMove,
            OutputAction::ControllerButton { .. } => ActionKind::ControllerButton,
            OutputAction::ControllerAxis { .. } => ActionKind::ControllerAxis,
        }
    }

    fn label(self) -> &'static str {
        tr(match self {
            ActionKind::Key => "Keyboard key",
            ActionKind::PulsedKey => "Keyboard key, analog (like a magnetic keyboard)",
            ActionKind::MouseButton => "Mouse button",
            ActionKind::MouseWheel => "Mouse wheel",
            ActionKind::MouseMove => "Mouse movement (axis)",
            ActionKind::ControllerButton => "Controller button",
            ActionKind::ControllerAxis => "Controller stick or trigger (axis)",
        })
    }

    fn needs_axis(self) -> bool {
        matches!(
            self,
            ActionKind::PulsedKey | ActionKind::MouseMove | ActionKind::ControllerAxis
        )
    }
}

/// A control the user pressed or moved while detection was running.
#[derive(Clone, Copy, Debug)]
enum DetectedControl {
    Button {
        device: DeviceKind,
        button_number: usize,
    },
    Hat {
        device: DeviceKind,
        hat_index: usize,
        push: HatPush,
    },
    Axis {
        device: DeviceKind,
        axis_index: usize,
        rest_value: f64,
        moved_value: f64,
    },
}

impl DetectedControl {
    fn is_axis(&self) -> bool {
        matches!(self, DetectedControl::Axis { .. })
    }

    /// Rebuilds the control of an existing binding so it can be edited. `push_direction` is the
    /// direction of a pulsed key, which is stored on the action rather than the source.
    fn from_input_source(source: InputSource, push_direction: Option<AxisDirection>) -> Self {
        match source {
            InputSource::Button { device, button_number } => DetectedControl::Button { device, button_number },
            InputSource::Hat {
                device,
                hat_index,
                push,
            } => DetectedControl::Hat {
                device,
                hat_index,
                push,
            },
            InputSource::Axis { device, axis_index } => DetectedControl::Axis {
                device,
                axis_index,
                rest_value: 0.5,
                moved_value: if push_direction == Some(AxisDirection::Decreasing) {
                    0.0
                } else {
                    1.0
                },
            },
            InputSource::AxisPastThreshold {
                device,
                axis_index,
                direction,
                threshold,
            } => {
                let step_toward_movement = if direction == AxisDirection::Increasing {
                    THRESHOLD_EDIT_SPREAD
                } else {
                    -THRESHOLD_EDIT_SPREAD
                };
                DetectedControl::Axis {
                    device,
                    axis_index,
                    rest_value: threshold - step_toward_movement,
                    moved_value: threshold + step_toward_movement,
                }
            }
        }
    }

    /// An axis becomes a full analog source for analog actions, or a switch half-way to where it was moved.
    fn to_input_source(self, wants_analog: bool) -> Option<InputSource> {
        match self {
            DetectedControl::Button { device, button_number } => {
                (!wants_analog).then_some(InputSource::Button { device, button_number })
            }
            DetectedControl::Hat {
                device,
                hat_index,
                push,
            } => (!wants_analog).then_some(InputSource::Hat {
                device,
                hat_index,
                push,
            }),
            DetectedControl::Axis { device, axis_index, .. } if wants_analog => {
                Some(InputSource::Axis { device, axis_index })
            }
            DetectedControl::Axis {
                device,
                axis_index,
                rest_value,
                moved_value,
            } => Some(InputSource::AxisPastThreshold {
                device,
                axis_index,
                direction: if moved_value > rest_value {
                    AxisDirection::Increasing
                } else {
                    AxisDirection::Decreasing
                },
                threshold: (rest_value + moved_value) / 2.0,
            }),
        }
    }

    fn describe(self) -> String {
        let source = self
            .to_input_source(self.is_axis())
            .expect("a detected control always converts to its own kind of source");
        source.describe()
    }
}

/// What a detected control will be used for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DetectionPurpose {
    BindingControl,
    ModeButton(usize),
    ShiftButton,
}

struct ControlDetection {
    purpose: DetectionPurpose,
    baseline: HashMap<DeviceKind, ControllerSnapshot>,
    started_at: Instant,
}

/// The binding being put together, or changed, in the editor card.
pub struct MappingEditor {
    detection: Option<ControlDetection>,
    detected_control: Option<DetectedControl>,
    /// Position in the profile's binding list when an existing binding is being edited.
    editing_index: Option<usize>,
    action_kind: ActionKind,
    virtual_key: u16,
    key_search: String,
    is_capturing_key: bool,
    mouse_button: MouseButtonKind,
    wheel_direction: WheelDirection,
    screen_axis: ScreenAxis,
    pixels_per_second: f32,
    controller_button: ControllerButton,
    controller_axis: ControllerAxis,
    controller_axis_inverted: bool,
    layer: BindingLayer,
    press_style: PressStyle,
}

impl Default for MappingEditor {
    fn default() -> Self {
        Self {
            detection: None,
            detected_control: None,
            editing_index: None,
            action_kind: ActionKind::Key,
            virtual_key: u16::from(b'W'),
            key_search: String::new(),
            is_capturing_key: false,
            mouse_button: MouseButtonKind::Left,
            wheel_direction: WheelDirection::Up,
            screen_axis: ScreenAxis::Horizontal,
            pixels_per_second: DEFAULT_MOUSE_SPEED,
            controller_button: ControllerButton::A,
            controller_axis: ControllerAxis::LeftStickX,
            controller_axis_inverted: false,
            layer: BindingLayer::default(),
            press_style: PressStyle::WhileHeld,
        }
    }
}

impl MappingEditor {
    /// Fills the editor with an existing binding so it can be changed.
    fn load_binding(&mut self, binding_index: usize, binding: &InputBinding) {
        let push_direction = match binding.action {
            OutputAction::PulsedKey { direction, .. } => Some(direction),
            _ => None,
        };
        self.editing_index = Some(binding_index);
        self.detection = None;
        self.detected_control = Some(DetectedControl::from_input_source(binding.source, push_direction));
        self.action_kind = ActionKind::of_action(binding.action);
        self.layer = binding.layer;
        self.press_style = binding.press_style;
        self.load_action_details(binding.action);
    }

    fn load_action_details(&mut self, action: OutputAction) {
        match action {
            OutputAction::Key { virtual_key } | OutputAction::PulsedKey { virtual_key, .. } => {
                self.virtual_key = virtual_key;
            }
            OutputAction::MouseButton { button } => self.mouse_button = button,
            OutputAction::MouseWheel { direction } => self.wheel_direction = direction,
            OutputAction::MouseMove {
                screen_axis,
                pixels_per_second,
            } => {
                self.screen_axis = screen_axis;
                self.pixels_per_second = pixels_per_second;
            }
            OutputAction::ControllerButton { button } => self.controller_button = button,
            OutputAction::ControllerAxis { axis, inverted } => {
                self.controller_axis = axis;
                self.controller_axis_inverted = inverted;
            }
        }
    }

    fn finish_editing(&mut self) {
        self.editing_index = None;
        self.detected_control = None;
    }

    fn chosen_action(&self) -> OutputAction {
        match self.action_kind {
            ActionKind::Key => OutputAction::Key {
                virtual_key: self.virtual_key,
            },
            ActionKind::PulsedKey => OutputAction::PulsedKey {
                virtual_key: self.virtual_key,
                direction: self.detected_push_direction(),
            },
            ActionKind::MouseButton => OutputAction::MouseButton {
                button: self.mouse_button,
            },
            ActionKind::MouseWheel => OutputAction::MouseWheel {
                direction: self.wheel_direction,
            },
            ActionKind::MouseMove => OutputAction::MouseMove {
                screen_axis: self.screen_axis,
                pixels_per_second: self.pixels_per_second,
            },
            ActionKind::ControllerButton => OutputAction::ControllerButton {
                button: self.controller_button,
            },
            ActionKind::ControllerAxis => OutputAction::ControllerAxis {
                axis: self.controller_axis,
                inverted: self.controller_axis_inverted,
            },
        }
    }

    /// The way the detected axis was moved, which is the direction a pulsed key reacts to.
    fn detected_push_direction(&self) -> AxisDirection {
        match self.detected_control {
            Some(DetectedControl::Axis {
                rest_value,
                moved_value,
                ..
            }) if moved_value < rest_value => AxisDirection::Decreasing,
            _ => AxisDirection::Increasing,
        }
    }

    fn draft_binding(&self) -> Option<InputBinding> {
        let source = self.detected_control?.to_input_source(self.action_kind.needs_axis())?;
        let press_style = if source.is_analog() {
            PressStyle::WhileHeld
        } else {
            self.press_style
        };
        Some(InputBinding {
            press_style,
            ..InputBinding::new(source, self.chosen_action(), self.layer)
        })
    }
}

/// A one-click layout requested from the output tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LayoutRequest {
    WasdFlight,
    MouseLook,
    ControllerStarter(VirtualControllerKind),
}

/// What the user did in the binding list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BindingListAction {
    SwitchOnOrOff(usize),
    Edit(usize),
    Remove(usize),
}

impl JoystickManagerApp {
    pub(crate) fn show_mapping_page(&mut self, ui: &mut Ui) {
        let (profile, mapping_enabled) = {
            let state = self.app_state.lock().unwrap();
            (state.effective_profile(), state.settings.mapping_enabled)
        };
        let mut toggle_clicked = false;
        let header_caption = tr_format("Profile · {}", &[&profile.name]);
        page_header(
            ui,
            &header_caption,
            "Key mapping",
            Some(
                "Turn HOTAS controls into keys, mouse input or a virtual Xbox / PlayStation controller. Works while games have focus.",
            ),
            |ui| {
                toggle_clicked = if mapping_enabled {
                    secondary_button(ui, "Turn mapping off").clicked()
                } else {
                    primary_button(ui, "Turn mapping on").clicked()
                };
            },
        );
        if toggle_clicked {
            self.update_settings(|settings| settings.mapping_enabled = !mapping_enabled);
        }

        self.show_output_card(ui, &profile);
        ui.add_space(14.0);
        self.show_binding_editor_card(ui);
        ui.add_space(14.0);
        self.show_binding_list_card(ui, &profile);
        ui.add_space(14.0);
        self.show_layers_card(ui, &profile);
    }

    /// Keyboard & mouse, Xbox and PlayStation side by side, each with its one-click layouts.
    fn show_output_card(&mut self, ui: &mut Ui, profile: &Profile) {
        let status = self.services.mapping_engine.virtual_controller_status();
        let mut layout_request = None;
        let mut chosen_controller = profile.virtual_controller;
        card(ui, |ui| {
            card_title(ui, "Outputs");
            muted_text(
                ui,
                "Mix freely: one profile can send keys, mouse input and a virtual controller at once.",
            );
            ui.add_space(8.0);
            let column_count = ((ui.available_width() / OUTPUT_TILE_MINIMUM_WIDTH) as usize).clamp(1, 3);
            ui.columns(column_count, |columns| {
                if let Some(request) = show_keyboard_tile(&mut columns[0]) {
                    layout_request = Some(request);
                }
                for (tile_index, controller_kind) in [VirtualControllerKind::Xbox360, VirtualControllerKind::DualShock4]
                    .into_iter()
                    .enumerate()
                {
                    let column = &mut columns[(tile_index + 1) % column_count];
                    if let Some(request) = show_controller_tile(column, controller_kind, &mut chosen_controller) {
                        layout_request = Some(request);
                    }
                }
            });
            ui.add_space(8.0);
            show_virtual_controller_status(ui, &status);
        });
        if chosen_controller != profile.virtual_controller {
            self.change_effective_profile(|profile| profile.virtual_controller = chosen_controller);
        }
        if let Some(request) = layout_request {
            self.add_layout(request);
        }
    }

    fn add_layout(&mut self, request: LayoutRequest) {
        match request {
            LayoutRequest::WasdFlight => self.change_effective_profile(add_wasd_flight_layout),
            LayoutRequest::MouseLook => self.change_effective_profile(add_mouse_look_layout),
            LayoutRequest::ControllerStarter(kind) => {
                self.change_effective_profile(|profile| add_controller_starter_layout(profile, kind));
            }
        }
        self.show_status("Layout added. Adjust or remove bindings below.", Tone::Success);
    }

    fn show_binding_editor_card(&mut self, ui: &mut Ui) {
        self.check_control_detection();
        self.check_key_capture(ui);
        let is_editing = self.mapping_editor.editing_index.is_some();
        let mut save_clicked = false;
        let mut cancel_clicked = false;
        card(ui, |ui| {
            card_title(ui, if is_editing { "Edit binding" } else { "Add a binding" });
            ui.add_space(6.0);
            self.show_control_detection_row(ui);
            ui.add_space(10.0);
            self.show_action_choice(ui);
            ui.add_space(10.0);
            let draft = self.mapping_editor.draft_binding();
            if self.mapping_editor.detected_control.is_some() && draft.is_none() {
                ui.label(RichText::new(tr("Buttons and hats can only drive keys, clicks, the wheel or controller buttons. Move an axis for this action.")).color(WARNING_TEXT));
            }
            ui.horizontal(|ui| {
                let save_label = if is_editing { "Save binding" } else { "Add binding" };
                save_clicked = enabled_primary_button(ui, draft.is_some(), save_label).clicked();
                if is_editing {
                    cancel_clicked = secondary_button(ui, "Cancel").clicked();
                }
            });
        });
        if cancel_clicked {
            self.mapping_editor.finish_editing();
        }
        if save_clicked && let Some(binding) = self.mapping_editor.draft_binding() {
            self.save_draft_binding(binding);
        }
    }

    /// Adds the new binding, or replaces the one being edited while keeping it switched on or off.
    fn save_draft_binding(&mut self, binding: InputBinding) {
        let editing_index = self.mapping_editor.editing_index;
        self.change_effective_profile(|profile| match editing_index {
            Some(index) if index < profile.bindings.len() => {
                let was_enabled = profile.bindings[index].enabled;
                profile.bindings[index] = InputBinding {
                    enabled: was_enabled,
                    ..binding
                };
            }
            _ => profile.bindings.push(binding),
        });
        self.mapping_editor.finish_editing();
        let template = if editing_index.is_some() {
            "Saved: {} as {}"
        } else {
            "Added: {} as {}"
        };
        self.show_status(
            tr_format(template, &[&binding.source.describe(), &binding.action.describe()]),
            Tone::Success,
        );
    }

    fn show_control_detection_row(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            caption(ui, "1 · Control");
            let binding_detection = self
                .mapping_editor
                .detection
                .as_ref()
                .filter(|detection| detection.purpose == DetectionPurpose::BindingControl);
            if let Some(detection) = binding_detection {
                let seconds_left = DETECTION_TIMEOUT
                    .saturating_sub(detection.started_at.elapsed())
                    .as_secs();
                ui.spinner();
                ui.label(
                    RichText::new(tr_format("Press a button or move a control… ({} s)", &[&seconds_left]))
                        .color(LIVE_CYAN),
                );
                if secondary_button(ui, "Cancel").clicked() {
                    self.mapping_editor.detection = None;
                }
                return;
            }
            let detected_text = match self.mapping_editor.detected_control {
                Some(control) => control.describe(),
                None => tr("Nothing chosen yet").to_owned(),
            };
            monospace_text(ui, detected_text);
            if primary_button(ui, "Detect").clicked() {
                self.start_control_detection(DetectionPurpose::BindingControl);
            }
        });
    }

    fn show_action_choice(&mut self, ui: &mut Ui) {
        let editor = &mut self.mapping_editor;
        ui.horizontal_wrapped(|ui| {
            caption(ui, "2 · Action");
            ComboBox::from_id_salt("action-kind")
                .selected_text(editor.action_kind.label())
                .width(300.0)
                .show_ui(ui, |ui| {
                    for kind in ActionKind::ALL {
                        ui.selectable_value(&mut editor.action_kind, kind, kind.label());
                    }
                });
        });
        ui.horizontal_wrapped(|ui| match editor.action_kind {
            ActionKind::Key => show_key_choice(ui, editor),
            ActionKind::PulsedKey => {
                show_key_choice(ui, editor);
                muted_text(
                    ui,
                    "The key is tapped faster the further the axis moves, like a partial press on a magnetic keyboard.",
                );
            }
            ActionKind::MouseButton => {
                ComboBox::from_id_salt("mouse-button")
                    .selected_text(editor.mouse_button.label())
                    .show_ui(ui, |ui| {
                        for button in MouseButtonKind::ALL {
                            ui.selectable_value(&mut editor.mouse_button, button, button.label());
                        }
                    });
            }
            ActionKind::MouseWheel => {
                ui.selectable_value(&mut editor.wheel_direction, WheelDirection::Up, tr("Scroll up"));
                ui.selectable_value(&mut editor.wheel_direction, WheelDirection::Down, tr("Scroll down"));
            }
            ActionKind::MouseMove => {
                ui.selectable_value(&mut editor.screen_axis, ScreenAxis::Horizontal, tr("Left / right"));
                ui.selectable_value(&mut editor.screen_axis, ScreenAxis::Vertical, tr("Up / down"));
                muted_text(ui, "Top speed");
                ui.add(Slider::new(&mut editor.pixels_per_second, 100.0..=5000.0).suffix(" px/s"));
            }
            ActionKind::ControllerButton => {
                ComboBox::from_id_salt("controller-button")
                    .selected_text(editor.controller_button.label())
                    .show_ui(ui, |ui| {
                        for button in ControllerButton::ALL {
                            ui.selectable_value(&mut editor.controller_button, button, button.label());
                        }
                    });
            }
            ActionKind::ControllerAxis => {
                ComboBox::from_id_salt("controller-axis")
                    .selected_text(editor.controller_axis.label())
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for axis in ControllerAxis::ALL {
                            ui.selectable_value(&mut editor.controller_axis, axis, axis.label());
                        }
                    });
                ui.checkbox(&mut editor.controller_axis_inverted, tr("Invert"));
            }
        });
        show_layer_choice(ui, &mut editor.layer);
        if !editor.action_kind.needs_axis() {
            show_press_style_choice(ui, &mut editor.press_style);
        }
    }

    /// Bindings grouped by device, each with a live light, an on/off switch, Edit and Remove.
    fn show_binding_list_card(&mut self, ui: &mut Ui, profile: &Profile) {
        let snapshots = self.current_snapshots();
        let mode_switch = self.app_state.lock().unwrap().settings.mode_switch_buttons;
        let active_layer = profile.layer_setup.active_layer(&mode_switch, &snapshots);
        let calibration = self.app_state.lock().unwrap().calibration.clone();
        let mut requested_action = None;
        card(ui, |ui| {
            ui.horizontal(|ui| {
                card_title(ui, "Bindings");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    muted_text(ui, tr_format("{} in \"{}\"", &[&profile.bindings.len(), &profile.name]));
                });
            });
            if profile.bindings.is_empty() {
                ui.add_space(6.0);
                muted_text(
                    ui,
                    "No bindings yet. Use Detect above, then pick what the control should do.",
                );
                return;
            }
            for device in DeviceKind::ALL {
                let device_bindings: Vec<(usize, &InputBinding)> = profile
                    .bindings
                    .iter()
                    .enumerate()
                    .filter(|(_, binding)| binding.source.device() == device)
                    .collect();
                if device_bindings.is_empty() {
                    continue;
                }
                ui.add_space(10.0);
                caption(ui, device.definition().short_name);
                let activity_context = ActivityContext {
                    snapshot: snapshots.get(&device),
                    active_layer,
                    calibration: &calibration,
                };
                let grid_id = format!("bindings-{device:?}");
                if let Some(action) = show_device_binding_rows(ui, &grid_id, &device_bindings, &activity_context) {
                    requested_action = Some(action);
                }
            }
        });
        if let Some(action) = requested_action {
            self.run_binding_list_action(action, profile);
        }
    }

    fn run_binding_list_action(&mut self, action: BindingListAction, profile: &Profile) {
        match action {
            BindingListAction::SwitchOnOrOff(index) => self.change_effective_profile(|profile| {
                if let Some(binding) = profile.bindings.get_mut(index) {
                    binding.enabled = !binding.enabled;
                }
            }),
            BindingListAction::Edit(index) => {
                if let Some(binding) = profile.bindings.get(index) {
                    self.mapping_editor.load_binding(index, binding);
                }
            }
            BindingListAction::Remove(index) => {
                self.change_effective_profile(|profile| {
                    if index < profile.bindings.len() {
                        profile.bindings.remove(index);
                    }
                });
                self.mapping_editor.finish_editing();
            }
        }
    }

    fn current_snapshots(&self) -> HashMap<DeviceKind, ControllerSnapshot> {
        DeviceKind::ALL
            .into_iter()
            .filter_map(|device| Some((device, self.services.input_reader.snapshot(device)?)))
            .collect()
    }

    fn start_control_detection(&mut self, purpose: DetectionPurpose) {
        self.mapping_editor.detection = Some(ControlDetection {
            purpose,
            baseline: self.current_snapshots(),
            started_at: Instant::now(),
        });
        if purpose == DetectionPurpose::BindingControl {
            self.mapping_editor.detected_control = None;
        }
    }

    fn check_control_detection(&mut self) {
        let Some(detection) = &self.mapping_editor.detection else {
            return;
        };
        if detection.started_at.elapsed() > DETECTION_TIMEOUT {
            self.mapping_editor.detection = None;
            return;
        }
        let found = DeviceKind::ALL.into_iter().find_map(|device| {
            let current = self.services.input_reader.snapshot(device)?;
            let baseline = detection.baseline.get(&device).cloned().unwrap_or_default();
            find_changed_control(device, &baseline, &current)
        });
        let purpose = detection.purpose;
        if let Some(control) = found {
            self.mapping_editor.detection = None;
            match purpose {
                DetectionPurpose::BindingControl => self.use_detected_binding_control(control),
                DetectionPurpose::ModeButton(mode_index) => self.use_detected_layer_button(control, Some(mode_index)),
                DetectionPurpose::ShiftButton => self.use_detected_layer_button(control, None),
            }
        }
    }

    /// Moving an axis while a key is chosen switches to the analog key; other digital actions switch to a controller axis.
    fn use_detected_binding_control(&mut self, control: DetectedControl) {
        let editor = &mut self.mapping_editor;
        editor.detected_control = Some(control);
        if control.is_axis() && !editor.action_kind.needs_axis() {
            editor.action_kind = if editor.action_kind == ActionKind::Key {
                ActionKind::PulsedKey
            } else {
                ActionKind::ControllerAxis
            };
        }
    }

    /// Stores a mode button (`Some(index)`) or the shift button (`None`) in the profile's layer setup.
    fn use_detected_layer_button(&mut self, control: DetectedControl, mode_index: Option<usize>) {
        let DetectedControl::Button { device, button_number } = control else {
            self.show_status("Layers need a button or switch, not an axis or hat.", Tone::Warning);
            return;
        };
        let button = ButtonReference { device, button_number };
        match mode_index {
            Some(index) => self.update_settings(|settings| settings.mode_switch_buttons[index] = Some(button)),
            None => self.change_effective_profile(|profile| profile.layer_setup.shift_button = Some(button)),
        }
    }

    fn check_key_capture(&mut self, ui: &Ui) {
        if !self.mapping_editor.is_capturing_key {
            return;
        }
        let pressed_key = ui.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Key { key, pressed: true, .. } => Some(*key),
                _ => None,
            })
        });
        if let Some(keyboard_key) = pressed_key.and_then(find_key_for_egui_key) {
            self.mapping_editor.virtual_key = keyboard_key.virtual_key;
            self.mapping_editor.is_capturing_key = false;
        }
    }

    /// Changes the profile currently in charge and saves it; the mapping engine picks it up within a moment.
    fn change_effective_profile(&mut self, change: impl FnOnce(&mut Profile)) {
        let save_result = {
            let mut state = self.app_state.lock().unwrap();
            let profile_name = state.effective_profile_name();
            if let Some(profile) = state
                .profile_store
                .profiles
                .iter_mut()
                .find(|profile| profile.name == profile_name)
            {
                change(profile);
            }
            state.profile_store.save()
        };
        self.profile_editor.reload_from(&self.app_state.lock().unwrap());
        if let Err(error) = save_result {
            self.show_status(tr_format("Could not save the profile: {}", &[&error]), Tone::Danger);
        }
    }

    /// The throttle's M1 / M2 / M3 switch (shared by all profiles, each position can pick a profile)
    /// and the current profile's shift button.
    fn show_layers_card(&mut self, ui: &mut Ui, profile: &Profile) {
        let (mode_switch, mut profiles_by_mode, profile_names) = {
            let state = self.app_state.lock().unwrap();
            let names: Vec<String> = state
                .profile_store
                .profiles
                .iter()
                .map(|profile| profile.name.clone())
                .collect();
            (
                state.settings.mode_switch_buttons,
                state.settings.profiles_by_mode.clone(),
                names,
            )
        };
        let active_layer = profile
            .layer_setup
            .active_layer(&mode_switch, &self.current_snapshots());
        let detecting_purpose = self
            .mapping_editor
            .detection
            .as_ref()
            .map(|detection| detection.purpose);
        let original_profiles_by_mode = profiles_by_mode.clone();
        let mut detection_requested = None;
        card(ui, |ui| {
            ui.horizontal(|ui| {
                card_title(ui, "Mode switch and layers");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    status_pill(ui, &tr_format("Now: {}", &[&active_layer.describe()]), Tone::Info);
                });
            });
            muted_text(
                ui,
                "Detect the button each position of the throttle's mode switch holds down. Each position can switch to its own profile, and bindings can be limited to one mode. The shift button gives this profile's controls a second job while it is held.",
            );
            ui.add_space(6.0);
            egui::Grid::new("layer-setup")
                .num_columns(4)
                .spacing([18.0, 6.0])
                .show(ui, |ui| {
                    for mode_index in 0..MODE_COUNT {
                        let purpose = DetectionPurpose::ModeButton(mode_index);
                        let label = tr_format("Mode {}", &[&(mode_index + 1)]);
                        if show_layer_button_row(
                            ui,
                            &label,
                            mode_switch[mode_index],
                            detecting_purpose == Some(purpose),
                        ) {
                            detection_requested = Some(purpose);
                        }
                        show_mode_profile_choice(ui, mode_index, &mut profiles_by_mode[mode_index], &profile_names);
                        ui.end_row();
                    }
                    let shift_purpose = DetectionPurpose::ShiftButton;
                    let shift_button = profile.layer_setup.shift_button;
                    if show_layer_button_row(ui, "Shift", shift_button, detecting_purpose == Some(shift_purpose)) {
                        detection_requested = Some(shift_purpose);
                    }
                    muted_text(ui, "This profile only");
                    ui.end_row();
                });
        });
        if profiles_by_mode != original_profiles_by_mode {
            self.update_settings(|settings| settings.profiles_by_mode = profiles_by_mode);
        }
        if let Some(purpose) = detection_requested {
            self.start_control_detection(purpose);
        }
    }
}

/// The profile a mode switch position switches to, or none to keep the current one.
fn show_mode_profile_choice(
    ui: &mut Ui,
    mode_index: usize,
    chosen_profile: &mut Option<String>,
    profile_names: &[String],
) {
    let selected_text = chosen_profile
        .clone()
        .unwrap_or_else(|| tr("Keep current profile").to_owned());
    ComboBox::from_id_salt(("mode-profile", mode_index))
        .selected_text(selected_text)
        .width(220.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(chosen_profile, None, tr("Keep current profile"));
            for name in profile_names {
                ui.selectable_value(chosen_profile, Some(name.clone()), name.as_str());
            }
        });
}

/// The always-available tile. Returns the layout the user asked to add, if any.
fn show_keyboard_tile(ui: &mut Ui) -> Option<LayoutRequest> {
    let mut request = None;
    output_tile_frame(true).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            card_title(ui, "Keyboard & mouse");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                status_pill(ui, "Always on", Tone::Success);
            });
        });
        muted_text(ui, "Keys, clicks, scrolling and pointer movement. No driver needed.");
        ui.add_space(6.0);
        if secondary_button(ui, "Add WASD flight layout").clicked() {
            request = Some(LayoutRequest::WasdFlight);
        }
        if secondary_button(ui, "Add mouse look (thumb stick)").clicked() {
            request = Some(LayoutRequest::MouseLook);
        }
    });
    request
}

/// One virtual controller tile. Switches `chosen_controller` when the user picks or drops this controller.
fn show_controller_tile(
    ui: &mut Ui,
    controller_kind: VirtualControllerKind,
    chosen_controller: &mut VirtualControllerKind,
) -> Option<LayoutRequest> {
    let is_chosen = *chosen_controller == controller_kind;
    let mut request = None;
    output_tile_frame(is_chosen).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            card_title(ui, controller_kind.label());
            if is_chosen {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    status_pill(ui, "In use", Tone::Success);
                });
            }
        });
        let description = match controller_kind {
            VirtualControllerKind::DualShock4 => {
                "A DualShock 4 for games that want a PlayStation pad. PS5 games accept it."
            }
            _ => "An Xbox 360 controller, which nearly every PC game supports.",
        };
        muted_text(ui, description);
        ui.add_space(6.0);
        if is_chosen {
            if secondary_button(ui, "Stop using").clicked() {
                *chosen_controller = VirtualControllerKind::None;
            }
        } else if primary_button(ui, "Use this controller").clicked() {
            *chosen_controller = controller_kind;
        }
        if secondary_button(ui, "Add starter layout").clicked() {
            request = Some(LayoutRequest::ControllerStarter(controller_kind));
        }
    });
    request
}

fn output_tile_frame(is_highlighted: bool) -> egui::Frame {
    let border = if is_highlighted { ACCENT_AMBER } else { BORDER };
    inset_tile_frame().stroke(egui::Stroke::new(1.0, border))
}

fn inset_tile_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(INSET_BACKGROUND)
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(14))
}

/// What a binding row needs to tell whether it is firing right now.
struct ActivityContext<'a> {
    snapshot: Option<&'a ControllerSnapshot>,
    active_layer: ActiveLayer,
    calibration: &'a CalibrationStore,
}

/// Rows for one device's bindings. Returns the action the user clicked, if any.
fn show_device_binding_rows(
    ui: &mut Ui,
    grid_id: &str,
    device_bindings: &[(usize, &InputBinding)],
    activity_context: &ActivityContext,
) -> Option<BindingListAction> {
    let mut clicked_action = None;
    egui::Grid::new(grid_id)
        .num_columns(9)
        .spacing([12.0, 8.0])
        .min_row_height(36.0)
        .show(ui, |ui| {
            for (index, binding) in device_bindings {
                show_binding_activity(ui, binding, activity_context);
                let mut is_enabled = binding.enabled;
                if ui
                    .checkbox(&mut is_enabled, "")
                    .on_hover_text(tr("Switch this binding on or off"))
                    .changed()
                {
                    clicked_action = Some(BindingListAction::SwitchOnOrOff(*index));
                }
                let text_color = if binding.enabled { TEXT_PRIMARY } else { TEXT_MUTED };
                ui.label(
                    RichText::new(binding.source.describe_control())
                        .font(monospace_font(13.5))
                        .color(text_color),
                );
                flow_arrow(ui);
                let family = binding.output_family();
                category_tag(ui, family.label(), output_family_color(family));
                ui.label(RichText::new(binding.action.describe()).color(text_color));
                ui.label(
                    RichText::new(describe_layer_and_press_style(binding))
                        .size(13.0)
                        .color(TEXT_MUTED),
                );
                if secondary_button(ui, "Edit").clicked() {
                    clicked_action = Some(BindingListAction::Edit(*index));
                }
                if secondary_button(ui, "Remove").clicked() {
                    clicked_action = Some(BindingListAction::Remove(*index));
                }
                ui.end_row();
            }
        });
    clicked_action
}

/// A light for switch-like bindings, a small bar for analog ones.
fn show_binding_activity(ui: &mut Ui, binding: &InputBinding, activity_context: &ActivityContext) {
    let Some(snapshot) = activity_context.snapshot else {
        activity_light(ui, false);
        return;
    };
    let is_allowed = binding.enabled && binding.layer.matches(activity_context.active_layer);
    match analog_deflection(binding, snapshot, activity_context.calibration) {
        Some(deflection) => {
            let shown_deflection = if is_allowed { deflection } else { 0.0 };
            ui.horizontal(|ui| {
                activity_light(ui, shown_deflection > ANALOG_ACTIVITY_THRESHOLD);
                deflection_meter(ui, shown_deflection);
            });
        }
        None => activity_light(ui, is_allowed && binding.source.is_active(snapshot)),
    }
}

/// How far (0..=1) the axis behind an analog binding is pushed, in the direction that matters.
fn analog_deflection(
    binding: &InputBinding,
    snapshot: &ControllerSnapshot,
    calibration: &CalibrationStore,
) -> Option<f64> {
    let InputSource::Axis { device, axis_index } = binding.source else {
        return None;
    };
    let value = calibration.correct(device, axis_index, snapshot.axis_value(axis_index)?);
    let rests_at_center = device.definition().axis_rests_at_center(axis_index);
    let deflection = match (binding.action, rests_at_center) {
        (OutputAction::PulsedKey { direction, .. }, true) => match direction {
            AxisDirection::Increasing => (value - 0.5) * 2.0,
            AxisDirection::Decreasing => (0.5 - value) * 2.0,
        },
        (OutputAction::PulsedKey { direction, .. }, false) => match direction {
            AxisDirection::Increasing => value,
            AxisDirection::Decreasing => 1.0 - value,
        },
        (_, true) => (value - 0.5).abs() * 2.0,
        (_, false) => value,
    };
    Some(deflection.clamp(0.0, 1.0))
}

fn output_family_color(family: OutputFamily) -> Color32 {
    match family {
        OutputFamily::Keyboard => ACCENT_AMBER,
        OutputFamily::Mouse => LIVE_CYAN,
        OutputFamily::Controller => SUCCESS_TEXT,
    }
}

/// Key list with a search box, plus a button that picks the next key pressed on the real keyboard.
fn show_key_choice(ui: &mut Ui, editor: &mut MappingEditor) {
    ComboBox::from_id_salt("keyboard-key")
        .selected_text(key_name(editor.virtual_key))
        .height(360.0)
        .show_ui(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut editor.key_search).hint_text(tr("Search keys")));
            let search = editor.key_search.to_lowercase();
            let matching_keys = all_keyboard_keys()
                .iter()
                .filter(|key| search.is_empty() || key.name.to_lowercase().contains(&search));
            for key in matching_keys {
                ui.selectable_value(&mut editor.virtual_key, key.virtual_key, key.name.as_str());
            }
        });
    let capture_label = if editor.is_capturing_key {
        "Press a key now…"
    } else {
        "Press a key to pick it"
    };
    if secondary_button(ui, capture_label).clicked() {
        editor.is_capturing_key = !editor.is_capturing_key;
    }
}

fn show_virtual_controller_status(ui: &mut Ui, status: &VirtualControllerStatus) {
    match status {
        VirtualControllerStatus::NotUsed => {}
        VirtualControllerStatus::Connected(kind) => {
            status_pill(ui, &tr_format("{} connected", &[&kind.label()]), Tone::Success);
        }
        VirtualControllerStatus::DriverMissing => {
            ui.label(
                RichText::new(tr("The free ViGEmBus driver is needed to create a virtual controller."))
                    .color(WARNING_TEXT),
            );
            if ui.link(tr("Download ViGEmBus (official release page)")).clicked() {
                open_in_default_handler(VIGEM_BUS_DOWNLOAD_URL);
            }
        }
        VirtualControllerStatus::Failed(message) => {
            ui.label(RichText::new(tr_format("Virtual controller failed: {}", &[message])).color(DANGER_TEXT));
        }
    }
}

/// The first control that differs from the baseline: a newly pressed button, a pushed hat, or a moved axis.
fn find_changed_control(
    device: DeviceKind,
    baseline: &ControllerSnapshot,
    current: &ControllerSnapshot,
) -> Option<DetectedControl> {
    let pressed_button = (1..=current.button_states.len())
        .find(|button_number| current.is_button_pressed(*button_number) && !baseline.is_button_pressed(*button_number));
    if let Some(button_number) = pressed_button {
        return Some(DetectedControl::Button { device, button_number });
    }
    let pushed_hat = (0..current.hat_directions.len()).find_map(|hat_index| {
        let push = main_hat_push(current.hat_direction(hat_index))?;
        (baseline.hat_direction(hat_index) == HatDirection::Centered).then_some((hat_index, push))
    });
    if let Some((hat_index, push)) = pushed_hat {
        return Some(DetectedControl::Hat {
            device,
            hat_index,
            push,
        });
    }
    (0..current.axis_values.len()).find_map(|axis_index| {
        let moved_value = current.axis_value(axis_index)?;
        let rest_value = baseline.axis_value(axis_index).unwrap_or(moved_value);
        ((moved_value - rest_value).abs() >= AXIS_DETECTION_DISTANCE).then_some(DetectedControl::Axis {
            device,
            axis_index,
            rest_value,
            moved_value,
        })
    })
}

fn main_hat_push(direction: HatDirection) -> Option<HatPush> {
    match direction {
        HatDirection::Up => Some(HatPush::Up),
        HatDirection::Right => Some(HatPush::Right),
        HatDirection::Down => Some(HatPush::Down),
        HatDirection::Left => Some(HatPush::Left),
        _ => None,
    }
}

/// The first three cells of a layer setup row. Returns true when the user asks to detect the button.
fn show_layer_button_row(ui: &mut Ui, label: &str, button: Option<ButtonReference>, is_detecting: bool) -> bool {
    caption(ui, label);
    let button_text = button.map_or_else(|| tr("Not set").to_owned(), |button| button.describe());
    monospace_text(ui, button_text);
    if is_detecting {
        ui.label(RichText::new(tr("Press it now…")).color(LIVE_CYAN));
        false
    } else {
        secondary_button(ui, "Detect").clicked()
    }
}

fn show_layer_choice(ui: &mut Ui, layer: &mut BindingLayer) {
    ui.horizontal_wrapped(|ui| {
        caption(ui, "3 · Layer");
        let mode_text = layer.mode.map_or_else(
            || tr("All modes").to_owned(),
            |mode| tr_format("Mode {} only", &[&mode]),
        );
        ComboBox::from_id_salt("binding-mode")
            .selected_text(mode_text)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut layer.mode, None, tr("All modes"));
                for mode in 1..=MODE_COUNT as u8 {
                    ui.selectable_value(&mut layer.mode, Some(mode), tr_format("Mode {} only", &[&mode]));
                }
            });
        ui.checkbox(&mut layer.shift_layer, tr("Only while shift is held"));
    });
}

fn show_press_style_choice(ui: &mut Ui, press_style: &mut PressStyle) {
    ui.horizontal_wrapped(|ui| {
        caption(ui, "4 · Press");
        ComboBox::from_id_salt("press-style")
            .selected_text(press_style.label())
            .show_ui(ui, |ui| {
                for style in PressStyle::ALL {
                    ui.selectable_value(press_style, style, style.label());
                }
            });
        muted_text(ui, press_style.explanation());
    });
}

/// "All modes", or "All modes · Double tap" when the binding reacts to a special kind of press.
fn describe_layer_and_press_style(binding: &InputBinding) -> String {
    let layer_text = binding.layer.describe();
    if binding.press_style == PressStyle::WhileHeld || binding.source.is_analog() {
        return layer_text;
    }
    format!("{layer_text} · {}", binding.press_style.label())
}
