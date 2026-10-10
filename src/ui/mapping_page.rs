//! Key mapping: bind HOTAS controls to keys, the mouse, or a virtual Xbox / PlayStation controller.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use eframe::egui::{self, ComboBox, RichText, Slider, Ui};

use crate::app::JoystickManagerApp;
use crate::bindings::{
    AxisDirection, ControllerAxis, ControllerButton, HatPush, InputBinding, InputSource, MouseButtonKind, OutputAction,
    ScreenAxis, VirtualControllerKind, WheelDirection,
};
use crate::input_reader::{ControllerSnapshot, HatDirection};
use crate::keyboard_keys::{all_keyboard_keys, find_key_for_egui_key, key_name};
use crate::known_devices::DeviceKind;
use crate::layers::{BindingLayer, ButtonReference, MODE_COUNT};
use crate::localization::{tr, tr_format};
use crate::mapping_engine::VirtualControllerStatus;
use crate::profiles::Profile;
use crate::system_actions::open_in_default_handler;
use crate::theme::*;
use crate::ui::widgets::*;
use crate::virtual_gamepad::VIGEM_BUS_DOWNLOAD_URL;

const DETECTION_TIMEOUT: Duration = Duration::from_secs(10);
const AXIS_DETECTION_DISTANCE: f64 = 0.3;
const DEFAULT_MOUSE_SPEED: f32 = 1200.0;

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

/// The binding being put together in the "Add a binding" card.
pub struct MappingEditor {
    detection: Option<ControlDetection>,
    detected_control: Option<DetectedControl>,
    action_kind: ActionKind,
    virtual_key: u16,
    is_capturing_key: bool,
    mouse_button: MouseButtonKind,
    wheel_direction: WheelDirection,
    screen_axis: ScreenAxis,
    pixels_per_second: f32,
    controller_button: ControllerButton,
    controller_axis: ControllerAxis,
    controller_axis_inverted: bool,
    layer: BindingLayer,
}

impl Default for MappingEditor {
    fn default() -> Self {
        Self {
            detection: None,
            detected_control: None,
            action_kind: ActionKind::Key,
            virtual_key: u16::from(b'W'),
            is_capturing_key: false,
            mouse_button: MouseButtonKind::Left,
            wheel_direction: WheelDirection::Up,
            screen_axis: ScreenAxis::Horizontal,
            pixels_per_second: DEFAULT_MOUSE_SPEED,
            controller_button: ControllerButton::A,
            controller_axis: ControllerAxis::LeftStickX,
            controller_axis_inverted: false,
            layer: BindingLayer::default(),
        }
    }
}

impl MappingEditor {
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
        Some(InputBinding {
            source,
            action: self.chosen_action(),
            layer: self.layer,
        })
    }
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

        self.show_virtual_controller_card(ui, &profile);
        ui.add_space(14.0);
        self.show_layers_card(ui, &profile);
        ui.add_space(14.0);
        self.show_new_binding_card(ui);
        ui.add_space(14.0);
        self.show_binding_list_card(ui, &profile);
    }

    fn show_virtual_controller_card(&mut self, ui: &mut Ui, profile: &Profile) {
        let mut chosen_kind = profile.virtual_controller;
        let status = self.services.mapping_engine.virtual_controller_status();
        let mut add_starter_layout = false;
        card(ui, |ui| {
            card_title(ui, "Virtual controller");
            muted_text(ui, "Games see it as a real gamepad. PS5 games accept the DualShock 4.");
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                for kind in VirtualControllerKind::ALL {
                    ui.selectable_value(&mut chosen_kind, kind, tr(kind.label()));
                }
            });
            ui.add_space(6.0);
            show_virtual_controller_status(ui, &status);
            ui.add_space(6.0);
            add_starter_layout = secondary_button(ui, "Add Xbox starter layout (sticks and throttles)").clicked();
        });
        if chosen_kind != profile.virtual_controller {
            self.change_effective_profile(|profile| profile.virtual_controller = chosen_kind);
        }
        if add_starter_layout {
            self.change_effective_profile(add_xbox_starter_layout);
            self.show_status("Starter layout added. Adjust or remove bindings below.", Tone::Success);
        }
    }

    fn show_new_binding_card(&mut self, ui: &mut Ui) {
        self.check_control_detection();
        self.check_key_capture(ui);
        let mut add_clicked = false;
        card(ui, |ui| {
            card_title(ui, "Add a binding");
            ui.add_space(6.0);
            self.show_control_detection_row(ui);
            ui.add_space(10.0);
            self.show_action_choice(ui);
            ui.add_space(10.0);
            let draft = self.mapping_editor.draft_binding();
            if self.mapping_editor.detected_control.is_some() && draft.is_none() {
                ui.label(RichText::new(tr("Buttons and hats can only drive keys, clicks, the wheel or controller buttons. Move an axis for this action.")).color(WARNING_TEXT));
            }
            add_clicked = enabled_primary_button(ui, draft.is_some(), "Add binding").clicked();
        });
        if add_clicked && let Some(binding) = self.mapping_editor.draft_binding() {
            self.change_effective_profile(|profile| profile.bindings.push(binding));
            self.mapping_editor.detected_control = None;
            self.show_status(
                tr_format(
                    "Added: {} → {}",
                    &[&binding.source.describe(), &binding.action.describe()],
                ),
                Tone::Success,
            );
        }
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
                Some(control) => describe_detected_control(control),
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
                .width(260.0)
                .show_ui(ui, |ui| {
                    for kind in ActionKind::ALL {
                        ui.selectable_value(&mut editor.action_kind, kind, tr(kind.label()));
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
    }

    fn show_binding_list_card(&mut self, ui: &mut Ui, profile: &Profile) {
        let mut removed_index = None;
        card(ui, |ui| {
            ui.horizontal(|ui| {
                card_title(ui, "Bindings");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    muted_text(ui, tr_format("{} in \"{}\"", &[&profile.bindings.len(), &profile.name]));
                });
            });
            ui.add_space(6.0);
            if profile.bindings.is_empty() {
                muted_text(
                    ui,
                    "No bindings yet. Use Detect above, then pick what the control should do.",
                );
                return;
            }
            egui::Grid::new("binding-list")
                .num_columns(5)
                .spacing([18.0, 8.0])
                .show(ui, |ui| {
                    for (index, binding) in profile.bindings.iter().enumerate() {
                        monospace_text(ui, binding.source.describe());
                        ui.label(RichText::new(tr("→")).color(TEXT_MUTED));
                        ui.label(binding.action.describe());
                        ui.label(RichText::new(binding.layer.describe()).color(TEXT_MUTED));
                        if secondary_button(ui, "Remove").clicked() {
                            removed_index = Some(index);
                        }
                        ui.end_row();
                    }
                });
        });
        if let Some(index) = removed_index {
            self.change_effective_profile(|profile| {
                profile.bindings.remove(index);
            });
        }
    }

    fn start_control_detection(&mut self, purpose: DetectionPurpose) {
        let baseline = DeviceKind::ALL
            .into_iter()
            .filter_map(|device| Some((device, self.services.input_reader.snapshot(device)?)))
            .collect();
        self.mapping_editor.detection = Some(ControlDetection {
            purpose,
            baseline,
            started_at: Instant::now(),
        });
        self.mapping_editor.detected_control = None;
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
        self.change_effective_profile(|profile| match mode_index {
            Some(index) => profile.layer_setup.mode_buttons[index] = Some(button),
            None => profile.layer_setup.shift_button = Some(button),
        });
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
}

fn show_key_choice(ui: &mut Ui, editor: &mut MappingEditor) {
    ComboBox::from_id_salt("keyboard-key")
        .selected_text(key_name(editor.virtual_key))
        .height(320.0)
        .show_ui(ui, |ui| {
            for key in all_keyboard_keys() {
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
        VirtualControllerStatus::NotUsed => {
            muted_text(
                ui,
                "Not in use. Turn mapping on with a controller selected to plug it in.",
            );
        }
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

fn describe_detected_control(control: DetectedControl) -> String {
    match control {
        DetectedControl::Button { device, button_number } => {
            tr_format("{} · Button {}", &[&device.definition().short_name, &button_number])
        }
        DetectedControl::Hat {
            device,
            hat_index,
            push,
        } => tr_format(
            "{} · Hat {} {}",
            &[&device.definition().short_name, &(hat_index + 1), &push.label()],
        ),
        DetectedControl::Axis { device, axis_index, .. } => {
            let definition = device.definition();
            tr_format(
                "{} · {} (axis)",
                &[&definition.short_name, &definition.axis_name(axis_index)],
            )
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

/// Stick on the left stick, thumb stick on the right stick, throttles on the triggers.
fn add_xbox_starter_layout(profile: &mut Profile) {
    let axis_binding = |device: DeviceKind, axis_index: usize, axis: ControllerAxis, inverted: bool| InputBinding {
        source: InputSource::Axis { device, axis_index },
        action: OutputAction::ControllerAxis { axis, inverted },
        layer: BindingLayer::default(),
    };
    profile.virtual_controller = VirtualControllerKind::Xbox360;
    profile.bindings.extend([
        axis_binding(DeviceKind::Joystick, 0, ControllerAxis::LeftStickX, false),
        axis_binding(DeviceKind::Joystick, 1, ControllerAxis::LeftStickY, false),
        axis_binding(DeviceKind::Joystick, 2, ControllerAxis::RightStickX, false),
        axis_binding(DeviceKind::Joystick, 3, ControllerAxis::RightStickY, false),
        axis_binding(DeviceKind::Throttle, 0, ControllerAxis::RightTrigger, true),
        axis_binding(DeviceKind::Throttle, 1, ControllerAxis::LeftTrigger, true),
    ]);
}

impl JoystickManagerApp {
    /// Mode buttons (the throttle's M1 / M2 / M3 switch) and the shift button for the current profile.
    fn show_layers_card(&mut self, ui: &mut Ui, profile: &Profile) {
        let snapshots: HashMap<DeviceKind, ControllerSnapshot> = DeviceKind::ALL
            .into_iter()
            .filter_map(|device| Some((device, self.services.input_reader.snapshot(device)?)))
            .collect();
        let active_layer = profile.layer_setup.active_layer(&snapshots);
        let detecting_purpose = self
            .mapping_editor
            .detection
            .as_ref()
            .map(|detection| detection.purpose);
        let mut detection_requested = None;
        card(ui, |ui| {
            ui.horizontal(|ui| {
                card_title(ui, "Layers");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    status_pill(ui, &tr_format("Now: {}", &[&active_layer.describe()]), Tone::Info);
                });
            });
            muted_text(
                ui,
                "Pick the button each position of the mode switch holds down, and an optional shift button. \
                 Bindings can then be limited to one mode or to the shift layer.",
            );
            ui.add_space(6.0);
            egui::Grid::new("layer-setup")
                .num_columns(3)
                .spacing([18.0, 6.0])
                .show(ui, |ui| {
                    for mode_index in 0..MODE_COUNT {
                        let purpose = DetectionPurpose::ModeButton(mode_index);
                        let button = profile.layer_setup.mode_buttons[mode_index];
                        if show_layer_button_row(
                            ui,
                            &tr_format("Mode {}", &[&(mode_index + 1)]),
                            button,
                            detecting_purpose == Some(purpose),
                        ) {
                            detection_requested = Some(purpose);
                        }
                    }
                    let shift_purpose = DetectionPurpose::ShiftButton;
                    let shift_button = profile.layer_setup.shift_button;
                    if show_layer_button_row(ui, "Shift", shift_button, detecting_purpose == Some(shift_purpose)) {
                        detection_requested = Some(shift_purpose);
                    }
                });
        });
        if let Some(purpose) = detection_requested {
            self.start_control_detection(purpose);
        }
    }
}

/// One row of the layer setup. Returns true when the user asks to detect the button.
fn show_layer_button_row(ui: &mut Ui, label: &str, button: Option<ButtonReference>, is_detecting: bool) -> bool {
    caption(ui, label);
    let button_text = button.map_or_else(|| "Not set".to_owned(), |button| button.describe());
    monospace_text(ui, button_text);
    let detect_clicked = if is_detecting {
        ui.label(RichText::new(tr("Press it now…")).color(LIVE_CYAN));
        false
    } else {
        secondary_button(ui, "Detect").clicked()
    };
    ui.end_row();
    detect_clicked
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
