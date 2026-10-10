//! "Fix stick drift": measure every centred axis at rest for a few seconds and store the correction.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};

use crate::app::JoystickManagerApp;
use crate::calibration::{AxisCalibration, CalibrationStore};
use crate::known_devices::DeviceKind;
use crate::localization::{tr, tr_format};
use crate::theme::*;
use crate::ui::widgets::*;

const MEASUREMENT_DURATION: Duration = Duration::from_secs(3);

/// Readings collected while the user keeps their hands off the controls.
pub struct DriftMeasurement {
    started_at: Instant,
    samples_per_axis: HashMap<(DeviceKind, usize), Vec<f64>>,
}

impl DriftMeasurement {
    fn start() -> Self {
        Self {
            started_at: Instant::now(),
            samples_per_axis: HashMap::new(),
        }
    }

    fn is_finished(&self) -> bool {
        self.started_at.elapsed() >= MEASUREMENT_DURATION
    }

    fn seconds_left(&self) -> u64 {
        MEASUREMENT_DURATION.saturating_sub(self.started_at.elapsed()).as_secs() + 1
    }

    fn record(&mut self, device: DeviceKind, axis_index: usize, value: f64) {
        self.samples_per_axis
            .entry((device, axis_index))
            .or_default()
            .push(value);
    }

    fn into_calibrations(self) -> Vec<AxisCalibration> {
        let mut calibrations: Vec<AxisCalibration> = self
            .samples_per_axis
            .into_iter()
            .filter_map(|((device, axis_index), samples)| {
                AxisCalibration::from_rest_samples(device, axis_index, &samples)
            })
            .collect();
        calibrations.sort_by_key(|calibration| (calibration.device as u8, calibration.axis_index));
        calibrations
    }
}

impl JoystickManagerApp {
    pub(crate) fn show_drift_card(&mut self, ui: &mut Ui) {
        self.record_drift_samples();
        let calibration = self.app_state.lock().unwrap().calibration.clone();
        let mut start_clicked = false;
        let mut reset_clicked = false;
        card(ui, |ui| {
            ui.horizontal(|ui| {
                card_title(ui, "Stick drift");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (text, tone) = if calibration.axes.is_empty() {
                        ("Not corrected", Tone::Neutral)
                    } else {
                        ("Drift corrected", Tone::Success)
                    };
                    status_pill(ui, text, tone);
                });
            });
            muted_text(
                ui,
                "Measures where each stick axis really rests and how much it jitters, then re-centres it and hides the jitter. \
                 The correction applies to everything this app sends: the virtual controller, the mouse and analog keys.",
            );
            ui.add_space(8.0);
            match &self.drift_measurement {
                Some(measurement) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new(tr_format(
                                "Keep your hands off the controls… {} s",
                                &[&measurement.seconds_left()],
                            ))
                            .color(LIVE_CYAN),
                        );
                    });
                }
                None => {
                    ui.horizontal(|ui| {
                        start_clicked = primary_button(ui, "Fix stick drift").clicked();
                        reset_clicked =
                            enabled_secondary_button(ui, !calibration.axes.is_empty(), "Remove correction").clicked();
                    });
                }
            }
            show_calibration_table(ui, &calibration);
        });
        if start_clicked {
            self.drift_measurement = Some(DriftMeasurement::start());
        }
        if reset_clicked {
            self.replace_calibration(CalibrationStore::default());
            self.show_status("Stick drift correction removed.", Tone::Info);
        }
    }

    /// Adds the current reading of every centred axis while a measurement runs, and stores the result when it ends.
    fn record_drift_samples(&mut self) {
        let Some(measurement) = self.drift_measurement.as_mut() else {
            return;
        };
        for device in DeviceKind::ALL {
            let Some(snapshot) = self.services.input_reader.snapshot(device) else {
                continue;
            };
            let definition = device.definition();
            for (axis_index, value) in snapshot.axis_values.iter().enumerate() {
                if definition.axis_rests_at_center(axis_index) {
                    measurement.record(device, axis_index, *value);
                }
            }
        }
        if measurement.is_finished() {
            self.finish_drift_measurement();
        }
    }

    fn finish_drift_measurement(&mut self) {
        let Some(measurement) = self.drift_measurement.take() else {
            return;
        };
        let new_calibrations = measurement.into_calibrations();
        let corrected_axis_count = new_calibrations.len();
        let mut calibration = self.app_state.lock().unwrap().calibration.clone();
        calibration.store(new_calibrations);
        self.replace_calibration(calibration);
        self.show_status(
            tr_format("Stick drift fixed on {} axes.", &[&corrected_axis_count]),
            Tone::Success,
        );
    }

    fn replace_calibration(&mut self, calibration: CalibrationStore) {
        let save_result = calibration.save();
        self.app_state.lock().unwrap().calibration = calibration;
        if let Err(error) = save_result {
            self.show_status(tr_format("Could not save settings: {}", &[&error]), Tone::Danger);
        }
    }
}

fn show_calibration_table(ui: &mut Ui, calibration: &CalibrationStore) {
    if calibration.axes.is_empty() {
        return;
    }
    ui.add_space(8.0);
    egui::Grid::new("drift-corrections")
        .num_columns(3)
        .spacing([28.0, 6.0])
        .show(ui, |ui| {
            for heading in ["Axis", "Drift found", "Jitter hidden"] {
                caption(ui, heading);
            }
            ui.end_row();
            for axis in &calibration.axes {
                let definition = axis.device.definition();
                ui.label(format!(
                    "{} · {}",
                    definition.short_name,
                    definition.axis_name(axis.axis_index)
                ));
                monospace_text(ui, format!("{:+.1}%", axis.drift_percent()));
                monospace_text(ui, format!("{:.1}%", axis.deadzone * 100.0));
                ui.end_row();
            }
        });
    ui.label(
        RichText::new(tr("Measured once per device; run it again if the stick wears further."))
            .size(13.0)
            .color(TEXT_MUTED),
    );
}
