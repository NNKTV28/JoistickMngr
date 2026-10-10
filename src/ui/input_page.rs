//! Input tester: live gauges per device, axis health and a log of button presses for spotting ghost inputs.

use eframe::egui::{self, RichText, Ui};

use crate::app::JoystickManagerApp;
use crate::input_reader::{AxisStatistics, ControllerSnapshot, HatDirection};
use crate::known_devices::{DeviceKind, KnownDevice};
use crate::localization::{tr, tr_format};
use crate::profiles::Profile;
use crate::system_actions::open_game_controllers_panel;
use crate::theme::*;
use crate::ui::widgets::*;

const DEVICE_PANEL_MINIMUM_WIDTH: f32 = 460.0;
const NOISY_AXIS_THRESHOLD_PERCENT: f64 = 1.0;
const VERTICAL_BAR_WIDTH: f32 = 34.0;

impl JoystickManagerApp {
    pub(crate) fn show_input_page(&mut self, ui: &mut Ui) {
        let mut reset_clicked = false;
        let mut control_panel_clicked = false;
        page_header(ui, "Live · 100 Hz", "Input tester", None, |ui| {
            control_panel_clicked = secondary_button(ui, "Open joy.cpl").clicked();
            reset_clicked = secondary_button(ui, "Reset statistics").clicked();
        });
        if reset_clicked {
            self.services.input_reader.reset_statistics();
        }
        if control_panel_clicked {
            open_game_controllers_panel();
        }

        let snapshots: Vec<(DeviceKind, ControllerSnapshot)> = DeviceKind::ALL
            .into_iter()
            .filter_map(|device_kind| Some((device_kind, self.services.input_reader.snapshot(device_kind)?)))
            .collect();
        if snapshots.is_empty() {
            card(ui, |ui| {
                muted_text(
                    ui,
                    "No controllers are reporting input yet. Move a stick or press a button.",
                );
            });
            return;
        }

        show_device_panels(ui, &snapshots);
        ui.add_space(18.0);
        let active_profile = self.app_state.lock().unwrap().effective_profile();
        let statistics_per_device: Vec<Vec<AxisStatistics>> = snapshots
            .iter()
            .map(|(device_kind, _)| self.services.input_reader.axis_statistics(*device_kind))
            .collect();
        show_axis_health_card(ui, &snapshots, &statistics_per_device, &active_profile);
        ui.add_space(18.0);
        self.show_button_press_log(ui);
    }

    fn show_button_press_log(&self, ui: &mut Ui) {
        let press_log = self.services.input_reader.button_press_log();
        card(ui, |ui| {
            card_title(ui, "Button press log");
            muted_text(
                ui,
                "Every press is listed here. Presses you did not make are ghost inputs, usually caused by USB power saving.",
            );
            ui.add_space(6.0);
            if press_log.is_empty() {
                muted_text(ui, "No presses recorded since the last reset.");
            }
            for event in press_log.iter().rev().take(12) {
                let seconds_ago = event.pressed_at.elapsed().as_secs();
                monospace_text(
                    ui,
                    tr_format(
                        "{} · button {} · {} s ago",
                        &[
                            &event.device_kind.definition().short_name,
                            &event.button_number,
                            &seconds_ago,
                        ],
                    ),
                );
            }
        });
    }
}

fn show_device_panels(ui: &mut Ui, snapshots: &[(DeviceKind, ControllerSnapshot)]) {
    let column_count = ((ui.available_width() / DEVICE_PANEL_MINIMUM_WIDTH) as usize).clamp(1, snapshots.len());
    for row in snapshots.chunks(column_count) {
        ui.columns(column_count, |columns| {
            for (column, (device_kind, snapshot)) in columns.iter_mut().zip(row) {
                card(column, |ui| {
                    show_device_panel_heading(ui, device_kind.definition(), snapshot);
                    ui.add_space(8.0);
                    if !snapshot.has_reported_input() {
                        muted_text(
                            ui,
                            "No report received yet. Move an axis or press a button on this device.",
                        );
                    }
                    match device_kind {
                        DeviceKind::Joystick => show_joystick_gauges(ui, device_kind.definition(), snapshot),
                        DeviceKind::Throttle => show_throttle_gauges(ui, device_kind.definition(), snapshot),
                        DeviceKind::RudderPedals => show_rudder_gauges(ui, snapshot),
                    }
                });
            }
        });
        ui.add_space(12.0);
    }
}

fn show_device_panel_heading(ui: &mut Ui, definition: &KnownDevice, snapshot: &ControllerSnapshot) {
    ui.horizontal(|ui| {
        card_title(ui, definition.short_name);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            caption(
                ui,
                &tr_format(
                    "{} axes · {} buttons · {} hats",
                    &[
                        &snapshot.axis_values.len(),
                        &snapshot.button_states.len(),
                        &snapshot.hat_directions.len(),
                    ],
                ),
            );
        });
    });
}

fn axis_value(snapshot: &ControllerSnapshot, axis_index: usize) -> f64 {
    snapshot.axis_values.get(axis_index).copied().unwrap_or(0.5)
}

fn show_joystick_gauges(ui: &mut Ui, definition: &KnownDevice, snapshot: &ControllerSnapshot) {
    ui.horizontal(|ui| {
        crosshair_plot(ui, axis_value(snapshot, 0), axis_value(snapshot, 1), 170.0);
        for axis_index in 2..snapshot.axis_values.len() {
            vertical_level_bar(
                ui,
                axis_value(snapshot, axis_index),
                &definition.axis_name(axis_index),
                VERTICAL_BAR_WIDTH,
                LIVE_CYAN,
            );
        }
    });
    show_axis_value_line(ui, definition, snapshot);
    show_hats(ui, &snapshot.hat_directions);
    button_state_grid(ui, &snapshot.button_states);
}

fn show_throttle_gauges(ui: &mut Ui, definition: &KnownDevice, snapshot: &ControllerSnapshot) {
    ui.horizontal_wrapped(|ui| {
        for (axis_index, value) in snapshot.axis_values.iter().enumerate() {
            vertical_level_bar(
                ui,
                *value,
                &definition.axis_name(axis_index),
                VERTICAL_BAR_WIDTH,
                LIVE_CYAN,
            );
        }
    });
    show_axis_value_line(ui, definition, snapshot);
    show_hats(ui, &snapshot.hat_directions);
    button_state_grid(ui, &snapshot.button_states);
}

fn show_rudder_gauges(ui: &mut Ui, snapshot: &ControllerSnapshot) {
    caption(ui, "Rudder");
    centered_axis_bar(ui, axis_value(snapshot, 2), ui.available_width());
    monospace_text(ui, format_axis_value(axis_value(snapshot, 2), true));
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        vertical_level_bar(ui, axis_value(snapshot, 0), "Left brake", 56.0, ACCENT_AMBER);
        ui.add_space(20.0);
        vertical_level_bar(ui, axis_value(snapshot, 1), "Right brake", 56.0, ACCENT_AMBER);
    });
}

fn show_axis_value_line(ui: &mut Ui, definition: &KnownDevice, snapshot: &ControllerSnapshot) {
    let values_text = snapshot
        .axis_values
        .iter()
        .enumerate()
        .map(|(axis_index, value)| {
            format!(
                "{} {}",
                definition.axis_name(axis_index),
                format_axis_value(*value, definition.axis_rests_at_center(axis_index))
            )
        })
        .collect::<Vec<_>>()
        .join(" · ");
    ui.label(
        RichText::new(values_text)
            .font(monospace_font(12.5))
            .color(TEXT_SECONDARY),
    );
}

fn show_hats(ui: &mut Ui, hat_directions: &[HatDirection]) {
    if hat_directions.is_empty() {
        return;
    }
    ui.horizontal(|ui| {
        for (hat_index, direction) in hat_directions.iter().enumerate() {
            hat_direction_indicator(ui, *direction, &tr_format("Hat {}", &[&(hat_index + 1)]));
        }
    });
}

fn show_axis_health_card(
    ui: &mut Ui,
    snapshots: &[(DeviceKind, ControllerSnapshot)],
    statistics_per_device: &[Vec<AxisStatistics>],
    active_profile: &Profile,
) {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            card_title(ui, "Axis health");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                muted_text(ui, "Leave the controls untouched for a few seconds to measure jitter");
            });
        });
        ui.add_space(8.0);
        egui::ScrollArea::horizontal().show(ui, |ui| {
            egui::Grid::new("axis-health")
                .num_columns(6)
                .spacing([28.0, 8.0])
                .show(ui, |ui| {
                    for heading in ["Axis", "Now", "Tuned", "Jitter", "Range seen", "Verdict"] {
                        caption(ui, heading);
                    }
                    ui.end_row();
                    for ((device_kind, snapshot), statistics) in snapshots.iter().zip(statistics_per_device) {
                        show_axis_health_rows(ui, device_kind.definition(), snapshot, statistics, active_profile);
                    }
                });
        });
    });
}

fn show_axis_health_rows(
    ui: &mut Ui,
    definition: &KnownDevice,
    snapshot: &ControllerSnapshot,
    statistics: &[AxisStatistics],
    active_profile: &Profile,
) {
    for (axis_index, (value, statistics)) in snapshot.axis_values.iter().zip(statistics).enumerate() {
        let rests_at_center = definition.axis_rests_at_center(axis_index);
        let tuned_value = active_profile
            .tuning_for_axis(definition.kind, axis_index)
            .map(|tuning| tuning.apply_to_reading(*value, rests_at_center))
            .unwrap_or(*value);
        let noise_percent = statistics.noise_percent();
        let range_text = if statistics.has_recorded_range() {
            format!(
                "{:.2} – {:.2}",
                statistics.recorded_minimum, statistics.recorded_maximum
            )
        } else {
            "—".to_owned()
        };

        ui.label(format!(
            "{} · {}",
            definition.kind.category_label(),
            definition.axis_name(axis_index)
        ));
        monospace_text(ui, format_axis_value(*value, rests_at_center));
        monospace_text(ui, format_axis_value(tuned_value, rests_at_center));
        monospace_text(ui, format!("±{noise_percent:.1}%"));
        monospace_text(ui, range_text);
        if noise_percent > NOISY_AXIS_THRESHOLD_PERCENT {
            ui.label(RichText::new(tr("Jitter or moving — add a deadzone if at rest")).color(WARNING_TEXT));
        } else {
            ui.label(RichText::new(tr("Clean")).color(SUCCESS_TEXT));
        }
        ui.end_row();
    }
}
